//! Screencopy support for capturing toplevel window images
//!
//! Uses the `ext-image-copy-capture v1` protocol to capture window contents
//! as SHM pixel data. Captured frames are forwarded as events which can be
//! used to create thumbnails (e.g. via `iced::widget::image::Handle::from_rgba()`).
//!
//! Requires the `screencopy` feature and the `foreign-toplevel` feature
//! (for `ext_foreign_toplevel_handle_v1` handles).

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::os::fd::AsFd;

use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_shm;
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};

use wayland_protocols::ext::image_capture_source::v1::client::{
    ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1,
    ext_image_capture_source_v1::ExtImageCaptureSourceV1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_frame_v1::{self, ExtImageCopyCaptureFrameV1},
    ext_image_copy_capture_manager_v1::{self, ExtImageCopyCaptureManagerV1},
    ext_image_copy_capture_session_v1::{self, ExtImageCopyCaptureSessionV1},
};

/// A captured frame from a toplevel window
#[derive(Debug, Clone)]
pub struct CapturedFrame {
    /// The ext toplevel handle protocol ID this frame was captured from
    pub toplevel_id: u32,
    /// Width of the captured image in pixels
    pub width: u32,
    /// Height of the captured image in pixels
    pub height: u32,
    /// Raw RGBA pixel data (R, G, B, A order, 4 bytes per pixel)
    pub rgba: Vec<u8>,
}

/// Screencopy events forwarded to the application
#[derive(Debug, Clone)]
pub enum ScreencopyEvent {
    /// A frame was successfully captured
    Ready(CapturedFrame),
    /// A capture session failed
    Failed { toplevel_id: u32, reason: String },
}

/// Actions the application can request for screencopy
#[derive(Debug, Clone)]
pub enum ScreencopyAction {
    /// Capture a single frame from the toplevel with the given ext handle ID
    Capture(u32),
}

// ============================================================================
// Internal state
// ============================================================================

/// Per-session constraints collected from session events
#[derive(Debug, Clone)]
pub(crate) struct SessionConstraints {
    pub width: u32,
    pub height: u32,
    pub shm_format: Option<wl_shm::Format>,
}

/// Pending frame: SHM file + metadata for reading pixels after the ready event
pub(crate) struct PendingFrame {
    pub toplevel_id: u32,
    pub width: u32,
    pub height: u32,
    pub shm_format: wl_shm::Format,
    pub file: std::fs::File,
}

impl std::fmt::Debug for PendingFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingFrame")
            .field("toplevel_id", &self.toplevel_id)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("shm_format", &self.shm_format)
            .finish()
    }
}

/// Internal state for screencopy protocol objects stored in WindowState
#[derive(Debug)]
pub(crate) struct ScreencopyState {
    /// ext_foreign_toplevel_image_capture_source_manager_v1 global
    pub source_manager: Option<ExtForeignToplevelImageCaptureSourceManagerV1>,
    /// ext_image_copy_capture_manager_v1 global
    pub capture_manager: Option<ExtImageCopyCaptureManagerV1>,
    /// Active capture sessions keyed by toplevel ID
    pub sessions: HashMap<u32, ExtImageCopyCaptureSessionV1>,
    /// Per-session constraints (updated by session events, consumed on done)
    pub constraints: HashMap<u32, SessionConstraints>,
    /// Pending frames waiting for ready event
    pub pending_frames: HashMap<u32, PendingFrame>,
}

impl ScreencopyState {
    pub fn new() -> Self {
        Self {
            source_manager: None,
            capture_manager: None,
            sessions: HashMap::new(),
            constraints: HashMap::new(),
            pending_frames: HashMap::new(),
        }
    }

    pub fn is_available(&self) -> bool {
        self.source_manager.is_some() && self.capture_manager.is_some()
    }
}

/// Trait for handling screencopy events (implemented by WindowState)
pub(crate) trait ScreencopyHandler {
    fn screencopy_event(&mut self, event: ScreencopyEvent);
    fn screencopy_state(&self) -> &ScreencopyState;
    fn screencopy_state_mut(&mut self) -> &mut ScreencopyState;
    fn get_shm(&self) -> Option<&wl_shm::WlShm>;
    fn get_ext_toplevel_handle(
        &self,
        id: u32,
    ) -> Option<
        &wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
    >;
}

// ============================================================================
// Public API
// ============================================================================

/// Start a capture for a toplevel.
pub(crate) fn start_capture<D>(state: &mut D, toplevel_id: u32, qh: &QueueHandle<D>)
where
    D: ScreencopyHandler
        + Dispatch<ExtImageCaptureSourceV1, ImageCaptureSourceData>
        + Dispatch<ExtImageCopyCaptureSessionV1, CaptureSessionData>
        + 'static,
{
    // Check availability
    let (has_source_mgr, has_capture_mgr) = {
        let sc = state.screencopy_state();
        (sc.source_manager.is_some(), sc.capture_manager.is_some())
    };
    if !has_source_mgr || !has_capture_mgr {
        log::warn!("Screencopy not available (missing protocol globals)");
        state.screencopy_event(ScreencopyEvent::Failed {
            toplevel_id,
            reason: "Screencopy protocols not available".to_string(),
        });
        return;
    }

    let Some(ext_handle) = state.get_ext_toplevel_handle(toplevel_id) else {
        log::warn!("No ext_foreign_toplevel_handle for id={}", toplevel_id);
        state.screencopy_event(ScreencopyEvent::Failed {
            toplevel_id,
            reason: format!("No ext toplevel handle for id {toplevel_id}"),
        });
        return;
    };
    let ext_handle = ext_handle.clone();

    let sc = state.screencopy_state();
    let source_manager = sc.source_manager.as_ref().unwrap();
    let capture_manager = sc.capture_manager.as_ref().unwrap();

    // Create capture source from toplevel handle
    let source = source_manager.create_source(&ext_handle, qh, ImageCaptureSourceData);

    // Create capture session (options = 0 = don't paint cursors)
    let session = capture_manager.create_session(
        &source,
        ext_image_copy_capture_manager_v1::Options::empty(),
        qh,
        CaptureSessionData { toplevel_id },
    );

    // Source can be destroyed immediately — session holds server-side reference
    source.destroy();

    // Initialize constraints tracking
    let sc = state.screencopy_state_mut();
    sc.sessions.insert(toplevel_id, session);
    sc.constraints.insert(
        toplevel_id,
        SessionConstraints {
            width: 0,
            height: 0,
            shm_format: None,
        },
    );

    log::debug!("Started screencopy capture for toplevel id={}", toplevel_id);
}

// ============================================================================
// Pixel format conversion
// ============================================================================

/// Convert SHM buffer pixels to RGBA order.
///
/// WL_SHM format names describe the pixel as a 32-bit integer in native byte
/// order. On little-endian (x86), the memory byte layout is reversed:
///   Argb8888 → u32 0xAARRGGBB → memory bytes [B, G, R, A]
///   Xrgb8888 → u32 0x__RRGGBB → memory bytes [B, G, R, X]
///   Abgr8888 → u32 0xAABBGGRR → memory bytes [R, G, B, A]  (already RGBA!)
///   Xbgr8888 → u32 0x__BBGGRR → memory bytes [R, G, B, X]
fn convert_to_rgba(data: &mut [u8], format: wl_shm::Format) {
    match format {
        // Memory bytes: [R, G, B, A] — already RGBA, nothing to do
        wl_shm::Format::Abgr8888 => {}
        // Memory bytes: [B, G, R, A] → need [R, G, B, A]
        wl_shm::Format::Argb8888 => {
            for px in data.chunks_exact_mut(4) {
                px.swap(0, 2); // swap B and R
            }
        }
        // Memory bytes: [B, G, R, X] → need [R, G, B, 255]
        wl_shm::Format::Xrgb8888 => {
            for px in data.chunks_exact_mut(4) {
                px.swap(0, 2); // swap B and R
                px[3] = 255;
            }
        }
        // Memory bytes: [R, G, B, X] → need [R, G, B, 255]
        wl_shm::Format::Xbgr8888 => {
            for px in data.chunks_exact_mut(4) {
                px[3] = 255;
            }
        }
        _ => {
            log::warn!("Unsupported SHM format {:?}, leaving as-is", format);
        }
    }
}

/// Allocate an SHM buffer and start capturing a frame
fn capture_frame<D>(state: &mut D, toplevel_id: u32, qh: &QueueHandle<D>)
where
    D: ScreencopyHandler
        + Dispatch<ExtImageCopyCaptureFrameV1, CaptureFrameData>
        + Dispatch<WlBuffer, BufferData>
        + Dispatch<WlShmPool, ShmPoolData>
        + 'static,
{
    let sc = state.screencopy_state();
    let Some(constraints) = sc.constraints.get(&toplevel_id) else {
        return;
    };
    let width = constraints.width;
    let height = constraints.height;
    let Some(shm_format) = constraints.shm_format else {
        log::warn!("No SHM format for toplevel={}", toplevel_id);
        state.screencopy_event(ScreencopyEvent::Failed {
            toplevel_id,
            reason: "No SHM format provided by compositor".to_string(),
        });
        return;
    };

    let Some(shm) = state.get_shm() else {
        log::warn!("No wl_shm available for screencopy");
        return;
    };
    let shm = shm.clone();

    let buf_size = (width as i32) * (height as i32) * 4;

    // Create temp file for the SHM buffer
    let file = match tempfile::tempfile() {
        Ok(f) => f,
        Err(e) => {
            log::error!("Failed to create tempfile for screencopy: {}", e);
            state.screencopy_event(ScreencopyEvent::Failed {
                toplevel_id,
                reason: format!("tempfile creation failed: {e}"),
            });
            return;
        }
    };

    // Size the file
    if let Err(e) = file.set_len(buf_size as u64) {
        log::error!("Failed to set tempfile size: {}", e);
        return;
    }

    // Create wl_shm_pool → wl_buffer
    let pool = shm.create_pool(file.as_fd(), buf_size, qh, ShmPoolData);
    let buffer = pool.create_buffer(
        0,
        width as i32,
        height as i32,
        (width as i32) * 4,
        shm_format,
        qh,
        BufferData,
    );
    pool.destroy();

    // Get session and create frame
    let sc = state.screencopy_state();
    let Some(session) = sc.sessions.get(&toplevel_id) else {
        log::warn!("No session for toplevel={}", toplevel_id);
        buffer.destroy();
        return;
    };

    let frame = session.create_frame(qh, CaptureFrameData { toplevel_id });

    // Attach buffer, damage full area, capture
    frame.attach_buffer(&buffer);
    frame.damage_buffer(0, 0, width as i32, height as i32);
    frame.capture();

    // Store pending frame for reading pixels on ready
    state.screencopy_state_mut().pending_frames.insert(
        toplevel_id,
        PendingFrame {
            toplevel_id,
            width,
            height,
            shm_format,
            file,
        },
    );

    // Buffer will be destroyed after we read the pixels
    buffer.destroy();
}

/// Read pixels from a pending frame's SHM file and emit the event
fn read_frame_pixels(state: &mut impl ScreencopyHandler, toplevel_id: u32) {
    let Some(mut pending) = state
        .screencopy_state_mut()
        .pending_frames
        .remove(&toplevel_id)
    else {
        return;
    };

    let byte_count = (pending.width as usize) * (pending.height as usize) * 4;
    let mut rgba = vec![0u8; byte_count];

    if let Err(e) = pending.file.seek(SeekFrom::Start(0)) {
        log::error!("Failed to seek screencopy buffer: {}", e);
        return;
    }
    if let Err(e) = pending.file.read_exact(&mut rgba) {
        log::error!("Failed to read screencopy buffer: {}", e);
        return;
    }

    convert_to_rgba(&mut rgba, pending.shm_format);

    // Clean up session
    if let Some(session) = state.screencopy_state_mut().sessions.remove(&toplevel_id) {
        session.destroy();
    }
    state
        .screencopy_state_mut()
        .constraints
        .remove(&toplevel_id);

    state.screencopy_event(ScreencopyEvent::Ready(CapturedFrame {
        toplevel_id,
        width: pending.width,
        height: pending.height,
        rgba,
    }));
}

// ============================================================================
// User data types for Dispatch
// ============================================================================

#[derive(Debug, Clone, Default)]
pub(crate) struct CaptureManagerData;

#[derive(Debug, Clone, Default)]
pub(crate) struct CaptureSourceManagerData;

#[derive(Debug, Clone, Default)]
pub(crate) struct ImageCaptureSourceData;

#[derive(Debug)]
pub(crate) struct CaptureSessionData {
    pub toplevel_id: u32,
}

#[derive(Debug)]
pub(crate) struct CaptureFrameData {
    pub toplevel_id: u32,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BufferData;

#[derive(Debug, Clone, Default)]
pub(crate) struct ShmPoolData;

// ============================================================================
// Dispatch implementations
// ============================================================================

/// ext_image_copy_capture_manager_v1 — global, no events
impl<D> Dispatch<ExtImageCopyCaptureManagerV1, CaptureManagerData, D> for ()
where
    D: Dispatch<ExtImageCopyCaptureManagerV1, CaptureManagerData> + 'static,
{
    fn event(
        _state: &mut D,
        _proxy: &ExtImageCopyCaptureManagerV1,
        _event: <ExtImageCopyCaptureManagerV1 as Proxy>::Event,
        _data: &CaptureManagerData,
        _conn: &Connection,
        _qh: &QueueHandle<D>,
    ) {
    }
}

/// ext_foreign_toplevel_image_capture_source_manager_v1 — global, no events
impl<D> Dispatch<ExtForeignToplevelImageCaptureSourceManagerV1, CaptureSourceManagerData, D> for ()
where
    D: Dispatch<ExtForeignToplevelImageCaptureSourceManagerV1, CaptureSourceManagerData> + 'static,
{
    fn event(
        _state: &mut D,
        _proxy: &ExtForeignToplevelImageCaptureSourceManagerV1,
        _event: <ExtForeignToplevelImageCaptureSourceManagerV1 as Proxy>::Event,
        _data: &CaptureSourceManagerData,
        _conn: &Connection,
        _qh: &QueueHandle<D>,
    ) {
    }
}

/// ext_image_capture_source_v1 — opaque, no events
impl<D> Dispatch<ExtImageCaptureSourceV1, ImageCaptureSourceData, D> for ()
where
    D: Dispatch<ExtImageCaptureSourceV1, ImageCaptureSourceData> + 'static,
{
    fn event(
        _state: &mut D,
        _proxy: &ExtImageCaptureSourceV1,
        _event: <ExtImageCaptureSourceV1 as Proxy>::Event,
        _data: &ImageCaptureSourceData,
        _conn: &Connection,
        _qh: &QueueHandle<D>,
    ) {
    }
}

/// ext_image_copy_capture_session_v1 — receives constraints then done
impl<D> Dispatch<ExtImageCopyCaptureSessionV1, CaptureSessionData, D> for ()
where
    D: ScreencopyHandler
        + Dispatch<ExtImageCopyCaptureSessionV1, CaptureSessionData>
        + Dispatch<ExtImageCopyCaptureFrameV1, CaptureFrameData>
        + Dispatch<WlBuffer, BufferData>
        + Dispatch<WlShmPool, ShmPoolData>
        + 'static,
{
    fn event(
        state: &mut D,
        proxy: &ExtImageCopyCaptureSessionV1,
        event: ext_image_copy_capture_session_v1::Event,
        data: &CaptureSessionData,
        _conn: &Connection,
        qh: &QueueHandle<D>,
    ) {
        let tid = data.toplevel_id;
        match event {
            ext_image_copy_capture_session_v1::Event::BufferSize { width, height } => {
                log::debug!(
                    "Screencopy buffer_size: {}x{} toplevel={}",
                    width,
                    height,
                    tid
                );
                if let Some(c) = state.screencopy_state_mut().constraints.get_mut(&tid) {
                    c.width = width;
                    c.height = height;
                }
            }
            ext_image_copy_capture_session_v1::Event::ShmFormat { format } => {
                if let WEnum::Value(f) = format {
                    log::debug!("Screencopy shm_format: {:?} toplevel={}", f, tid);
                    if let Some(c) = state.screencopy_state_mut().constraints.get_mut(&tid) {
                        // Prefer ABGR/ARGB; take the first format offered
                        if c.shm_format.is_none() {
                            c.shm_format = Some(f);
                        }
                    }
                }
            }
            ext_image_copy_capture_session_v1::Event::Done => {
                log::debug!("Screencopy constraints done toplevel={}", tid);
                capture_frame(state, tid, qh);
            }
            ext_image_copy_capture_session_v1::Event::Stopped => {
                log::debug!("Screencopy session stopped toplevel={}", tid);
                state.screencopy_state_mut().sessions.remove(&tid);
                state.screencopy_state_mut().constraints.remove(&tid);
                proxy.destroy();
            }
            _ => {}
        }
    }
}

/// ext_image_copy_capture_frame_v1 — ready / failed
impl<D> Dispatch<ExtImageCopyCaptureFrameV1, CaptureFrameData, D> for ()
where
    D: ScreencopyHandler + Dispatch<ExtImageCopyCaptureFrameV1, CaptureFrameData> + 'static,
{
    fn event(
        state: &mut D,
        proxy: &ExtImageCopyCaptureFrameV1,
        event: ext_image_copy_capture_frame_v1::Event,
        data: &CaptureFrameData,
        _conn: &Connection,
        _qh: &QueueHandle<D>,
    ) {
        let tid = data.toplevel_id;
        match event {
            ext_image_copy_capture_frame_v1::Event::Ready => {
                log::debug!("Screencopy frame ready toplevel={}", tid);
                read_frame_pixels(state, tid);
                proxy.destroy();
            }
            ext_image_copy_capture_frame_v1::Event::Failed { reason } => {
                let reason_str = match reason {
                    WEnum::Value(
                        ext_image_copy_capture_frame_v1::FailureReason::BufferConstraints,
                    ) => "buffer constraints changed".to_string(),
                    WEnum::Value(ext_image_copy_capture_frame_v1::FailureReason::Stopped) => {
                        "session stopped".to_string()
                    }
                    WEnum::Value(ext_image_copy_capture_frame_v1::FailureReason::Unknown) => {
                        "unknown".to_string()
                    }
                    WEnum::Unknown(v) => format!("unknown reason ({v})"),
                    _ => "unrecognized".to_string(),
                };
                log::warn!("Screencopy frame failed toplevel={}: {}", tid, reason_str);
                state.screencopy_event(ScreencopyEvent::Failed {
                    toplevel_id: tid,
                    reason: reason_str,
                });
                proxy.destroy();
            }
            _ => {}
        }
    }
}

/// wl_buffer — release event (buffer can be reused)
impl<D> Dispatch<WlBuffer, BufferData, D> for ()
where
    D: Dispatch<WlBuffer, BufferData> + 'static,
{
    fn event(
        _state: &mut D,
        _proxy: &WlBuffer,
        _event: <WlBuffer as Proxy>::Event,
        _data: &BufferData,
        _conn: &Connection,
        _qh: &QueueHandle<D>,
    ) {
    }
}

/// wl_shm_pool — no client events
impl<D> Dispatch<WlShmPool, ShmPoolData, D> for ()
where
    D: Dispatch<WlShmPool, ShmPoolData> + 'static,
{
    fn event(
        _state: &mut D,
        _proxy: &WlShmPool,
        _event: <WlShmPool as Proxy>::Event,
        _data: &ShmPoolData,
        _conn: &Connection,
        _qh: &QueueHandle<D>,
    ) {
    }
}
