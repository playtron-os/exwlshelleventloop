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
use std::time::Instant;

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
    /// Start continuous capture for all active sessions.
    /// Frames auto-recapture at the wayland level without round-tripping through iced.
    /// The target dimensions are used for server-side downscaling so only small
    /// thumbnail data (≈500KB) flows through the event channel instead of
    /// full-resolution frames (14+ MB).
    StartContinuous {
        target_width: u32,
        target_height: u32,
    },
    /// Stop continuous capture. No more auto-recapture after current in-flight frames.
    StopContinuous,
}

/// Minimum interval between captures per toplevel (~30fps)
const MIN_CAPTURE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(33);

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

/// One SHM-backed buffer in the double-buffer swapchain
pub(crate) struct ShmBuffer {
    pub file: std::fs::File,
    pub wl_buffer: WlBuffer,
    pub width: u32,
    pub height: u32,
    pub shm_format: wl_shm::Format,
}

impl std::fmt::Debug for ShmBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShmBuffer")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("shm_format", &self.shm_format)
            .finish()
    }
}

impl Drop for ShmBuffer {
    fn drop(&mut self) {
        self.wl_buffer.destroy();
    }
}

/// Double-buffer swapchain for a toplevel capture session
#[derive(Debug)]
pub(crate) struct BufferSwapchain {
    pub buffers: [ShmBuffer; 2],
    /// Index of the back buffer (being captured into)
    pub back: usize,
    /// Whether a capture is currently in flight (waiting for Ready)
    pub capture_in_flight: bool,
}

impl BufferSwapchain {
    /// Get the back buffer (compositor writes here)
    pub fn back_buffer(&self) -> &ShmBuffer {
        &self.buffers[self.back]
    }

    /// Swap: current back becomes front, current front becomes back
    pub fn swap(&mut self) {
        self.back = 1 - self.back;
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
    /// Double-buffer swapchains keyed by toplevel ID
    pub swapchains: HashMap<u32, BufferSwapchain>,
    /// Whether continuous capture is active (auto-recapture on Ready)
    pub continuous: bool,
    /// Target thumbnail dimensions for downscaling (set by StartContinuous)
    pub target_size: Option<(u32, u32)>,
    /// Last capture timestamp per toplevel (for throttling)
    pub last_capture: HashMap<u32, Instant>,
}

impl ScreencopyState {
    pub fn new() -> Self {
        Self {
            source_manager: None,
            capture_manager: None,
            sessions: HashMap::new(),
            constraints: HashMap::new(),
            swapchains: HashMap::new(),
            continuous: false,
            target_size: None,
            last_capture: HashMap::new(),
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
/// If a session already exists, reuses it and just requests a new frame.
pub(crate) fn start_capture<D>(state: &mut D, toplevel_id: u32, qh: &QueueHandle<D>)
where
    D: ScreencopyHandler
        + Dispatch<ExtImageCaptureSourceV1, ImageCaptureSourceData>
        + Dispatch<ExtImageCopyCaptureSessionV1, CaptureSessionData>
        + Dispatch<ExtImageCopyCaptureFrameV1, CaptureFrameData>
        + Dispatch<WlBuffer, BufferData>
        + Dispatch<WlShmPool, ShmPoolData>
        + 'static,
{
    // If we already have a session with constraints, just capture another frame
    if state.screencopy_state().sessions.contains_key(&toplevel_id)
        && state
            .screencopy_state()
            .constraints
            .get(&toplevel_id)
            .is_some_and(|c| c.width > 0 && c.shm_format.is_some())
    {
        capture_frame(state, toplevel_id, qh);
        return;
    }
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

/// Create a double-buffer swapchain for a toplevel session
fn create_swapchain<D>(state: &mut D, toplevel_id: u32, qh: &QueueHandle<D>)
where
    D: ScreencopyHandler
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
        return;
    };

    let Some(shm) = state.get_shm() else {
        log::warn!("No wl_shm available for screencopy");
        return;
    };
    let shm = shm.clone();

    let buf_size = (width as i32) * (height as i32) * 4;

    let make_buffer = || -> Option<ShmBuffer> {
        let file = tempfile::tempfile().ok()?;
        file.set_len(buf_size as u64).ok()?;
        let pool = shm.create_pool(file.as_fd(), buf_size, qh, ShmPoolData);
        let wl_buffer = pool.create_buffer(
            0,
            width as i32,
            height as i32,
            (width as i32) * 4,
            shm_format,
            qh,
            BufferData,
        );
        pool.destroy();
        Some(ShmBuffer {
            file,
            wl_buffer,
            width,
            height,
            shm_format,
        })
    };

    let Some(buf0) = make_buffer() else {
        log::error!("Failed to allocate screencopy buffer 0");
        return;
    };
    let Some(buf1) = make_buffer() else {
        log::error!("Failed to allocate screencopy buffer 1");
        return;
    };

    state.screencopy_state_mut().swapchains.insert(
        toplevel_id,
        BufferSwapchain {
            buffers: [buf0, buf1],
            back: 0,
            capture_in_flight: false,
        },
    );
}

/// Start capturing into the back buffer of the swapchain
fn capture_frame<D>(state: &mut D, toplevel_id: u32, qh: &QueueHandle<D>)
where
    D: ScreencopyHandler + Dispatch<ExtImageCopyCaptureFrameV1, CaptureFrameData> + 'static,
{
    // Don't start a new capture if one is already in flight
    {
        let sc = state.screencopy_state();
        if let Some(swap) = sc.swapchains.get(&toplevel_id) {
            if swap.capture_in_flight {
                return;
            }
        } else {
            return;
        }
    }

    let sc = state.screencopy_state();
    let Some(session) = sc.sessions.get(&toplevel_id) else {
        return;
    };

    let frame = session.create_frame(qh, CaptureFrameData { toplevel_id });

    // Attach back buffer
    let swap = state
        .screencopy_state()
        .swapchains
        .get(&toplevel_id)
        .unwrap();
    let back = swap.back_buffer();
    frame.attach_buffer(&back.wl_buffer);
    frame.damage_buffer(0, 0, back.width as i32, back.height as i32);
    frame.capture();

    state
        .screencopy_state_mut()
        .swapchains
        .get_mut(&toplevel_id)
        .unwrap()
        .capture_in_flight = true;
}

/// Read pixels from the back buffer (just captured), swap, and emit the event.
/// If a target_size is set, downscales and converts format in a single pass
/// so only small thumbnail data flows through the event channel.
fn read_frame_pixels(state: &mut impl ScreencopyHandler, toplevel_id: u32) {
    let sc = state.screencopy_state_mut();
    let Some(swap) = sc.swapchains.get_mut(&toplevel_id) else {
        return;
    };

    swap.capture_in_flight = false;

    let back = &mut swap.buffers[swap.back];
    let src_w = back.width;
    let src_h = back.height;
    let shm_format = back.shm_format;

    let byte_count = (src_w as usize) * (src_h as usize) * 4;
    let mut raw = vec![0u8; byte_count];

    if let Err(e) = back.file.seek(SeekFrom::Start(0)) {
        log::error!("Failed to seek screencopy buffer: {}", e);
        return;
    }
    if let Err(e) = back.file.read_exact(&mut raw) {
        log::error!("Failed to read screencopy buffer: {}", e);
        return;
    }

    // Swap buffers — the just-read back becomes front, old front becomes back
    swap.swap();

    let target_size = sc.target_size;

    // Downscale + convert in one pass if a target size is set
    let (rgba, out_w, out_h) = if let Some((tw, th)) = target_size {
        downscale_and_convert(&raw, src_w, src_h, tw, th, shm_format)
    } else {
        convert_to_rgba(&mut raw, shm_format);
        (raw, src_w, src_h)
    };

    state.screencopy_event(ScreencopyEvent::Ready(CapturedFrame {
        toplevel_id,
        width: out_w,
        height: out_h,
        rgba,
    }));
}

/// Downscale source pixels to fit within target dimensions and convert from
/// SHM format to RGBA in a single pass. Uses nearest-neighbor sampling for
/// speed (the result is displayed at thumbnail size so quality loss is minimal).
fn downscale_and_convert(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    target_w: u32,
    target_h: u32,
    format: wl_shm::Format,
) -> (Vec<u8>, u32, u32) {
    if src_w <= target_w && src_h <= target_h {
        let mut data = src.to_vec();
        convert_to_rgba(&mut data, format);
        return (data, src_w, src_h);
    }

    // Maintain aspect ratio
    let scale = (target_w as f32 / src_w as f32).min(target_h as f32 / src_h as f32);
    let dst_w = ((src_w as f32 * scale).round() as u32).max(1);
    let dst_h = ((src_h as f32 * scale).round() as u32).max(1);

    let src_stride = src_w as usize * 4;
    let mut dst = vec![0u8; (dst_w * dst_h * 4) as usize];

    // Determine byte-level swizzle for the SHM format → RGBA conversion.
    // On LE: Argb8888 memory = [B,G,R,A], Xrgb8888 = [B,G,R,X], etc.
    let (ri, gi, bi, ai, force_alpha) = match format {
        wl_shm::Format::Abgr8888 => (0, 1, 2, 3, false), // already RGBA
        wl_shm::Format::Argb8888 => (2, 1, 0, 3, false), // BGRA→RGBA
        wl_shm::Format::Xrgb8888 => (2, 1, 0, 3, true),  // BGRX→RGB+255
        wl_shm::Format::Xbgr8888 => (0, 1, 2, 3, true),  // RGBX→RGB+255
        _ => (0, 1, 2, 3, false),
    };

    // Nearest-neighbor sampling
    for dy in 0..dst_h {
        let sy = ((dy as f32 + 0.5) / scale) as u32;
        let sy = sy.min(src_h - 1);
        let src_row = sy as usize * src_stride;

        for dx in 0..dst_w {
            let sx = ((dx as f32 + 0.5) / scale) as u32;
            let sx = sx.min(src_w - 1);
            let si = src_row + sx as usize * 4;
            let di = (dy * dst_w + dx) as usize * 4;

            dst[di] = src[si + ri];
            dst[di + 1] = src[si + gi];
            dst[di + 2] = src[si + bi];
            dst[di + 3] = if force_alpha { 255 } else { src[si + ai] };
        }
    }

    (dst, dst_w, dst_h)
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
                // Allocate double-buffer swapchain if not yet created
                if !state.screencopy_state().swapchains.contains_key(&tid) {
                    create_swapchain(state, tid, qh);
                }
                capture_frame(state, tid, qh);
            }
            ext_image_copy_capture_session_v1::Event::Stopped => {
                log::debug!("Screencopy session stopped toplevel={}", tid);
                state.screencopy_state_mut().sessions.remove(&tid);
                state.screencopy_state_mut().constraints.remove(&tid);
                state.screencopy_state_mut().swapchains.remove(&tid);
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
        qh: &QueueHandle<D>,
    ) {
        let tid = data.toplevel_id;
        match event {
            ext_image_copy_capture_frame_v1::Event::Ready => {
                log::debug!("Screencopy frame ready toplevel={}", tid);
                read_frame_pixels(state, tid);
                proxy.destroy();
                // Auto-recapture with throttling (~30fps per toplevel).
                // Without throttling, continuous capture at max speed floods
                // the event loop and blocks keyboard/render processing.
                if state.screencopy_state().continuous {
                    let now = Instant::now();
                    let should_capture = state
                        .screencopy_state()
                        .last_capture
                        .get(&tid)
                        .map_or(true, |last| {
                            now.duration_since(*last) >= MIN_CAPTURE_INTERVAL
                        });
                    if should_capture {
                        state.screencopy_state_mut().last_capture.insert(tid, now);
                        capture_frame(state, tid, qh);
                    }
                }
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
