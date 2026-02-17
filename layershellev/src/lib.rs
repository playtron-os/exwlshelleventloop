//! # Handle the layer_shell in a winit way
//!
//! Min example is under
//!
//! ```rust, no_run
//! use std::fs::File;
//! use std::os::fd::AsFd;
//!
//! use layershellev::keyboard::{KeyCode, PhysicalKey};
//! use layershellev::reexport::*;
//! use layershellev::*;
//!
//! fn main() {
//!     let mut ev: WindowState<()> = WindowState::new("Hello")
//!         .with_allscreens()
//!         .with_size((0, 400))
//!         .with_layer(Layer::Top)
//!         .with_margin((20, 20, 100, 20))
//!         .with_anchor(Anchor::Bottom | Anchor::Left | Anchor::Right)
//!         .with_keyboard_interacivity(KeyboardInteractivity::Exclusive)
//!         .with_exclusive_zone(-1)
//!         .build()
//!         .unwrap();
//!
//!     ev.running(|event, ev, index| {
//!         match event {
//!             // NOTE: this will send when init, you can request bind extra object from here
//!             LayerShellEvent::InitRequest => ReturnData::RequestBind,
//!             LayerShellEvent::BindProvide(globals, qh) => {
//!                 // NOTE: you can get implied wayland object from here
//!                 let virtual_keyboard_manager = globals
//!                         .bind::<zwp_virtual_keyboard_v1::ZwpVirtualKeyboardManagerV1, _, _>(
//!                             qh,
//!                             1..=1,
//!                             (),
//!                         )
//!                         .unwrap();
//!                 println!("{:?}", virtual_keyboard_manager);
//!                 ReturnData::None
//!             }
//!             LayerShellEvent::XdgInfoChanged(_) => {
//!                 let index = index.unwrap();
//!                 let unit = ev.get_unit_with_id(index).unwrap();
//!                 println!("{:?}", unit.get_xdgoutput_info());
//!                 ReturnData::None
//!             }
//!             LayerShellEvent::RequestBuffer(file, shm, qh, init_w, init_h) => {
//!                 draw(file, (init_w, init_h));
//!                 let pool = shm.create_pool(file.as_fd(), (init_w * init_h * 4) as i32, qh, ());
//!                 ReturnData::WlBuffer(pool.create_buffer(
//!                     0,
//!                     init_w as i32,
//!                     init_h as i32,
//!                     (init_w * 4) as i32,
//!                     wl_shm::Format::Argb8888,
//!                     qh,
//!                     (),
//!                 ))
//!             }
//!             LayerShellEvent::RequestMessages(DispatchMessage::RequestRefresh { width, height, .. }) => {
//!                 println!("{width}, {height}");
//!                 ReturnData::None
//!             }
//!             LayerShellEvent::RequestMessages(DispatchMessage::MouseButton { .. }) => ReturnData::None,
//!             LayerShellEvent::RequestMessages(DispatchMessage::MouseEnter {
//!                 pointer, ..
//!             }) => ReturnData::RequestSetCursorShape((
//!                 "crosshair".to_owned(),
//!                 pointer.clone(),
//!             )),
//!             LayerShellEvent::RequestMessages(DispatchMessage::MouseMotion {
//!                 time,
//!                 surface_x,
//!                 surface_y,
//!             }) => {
//!                 println!("{time}, {surface_x}, {surface_y}");
//!                 ReturnData::None
//!             }
//!             LayerShellEvent::RequestMessages(DispatchMessage::KeyboardInput { event, .. }) => {
//!                if let PhysicalKey::Code(KeyCode::Escape) = event.physical_key {
//!                    ReturnData::RequestExit
//!                } else {
//!                    ReturnData::None
//!                }
//!            }
//!             _ => ReturnData::None,
//!         }
//!     })
//!     .unwrap();
//! }
//!
//! fn draw(tmp: &mut File, (buf_x, buf_y): (u32, u32)) {
//!     use std::{cmp::min, io::Write};
//!     let mut buf = std::io::BufWriter::new(tmp);
//!     for y in 0..buf_y {
//!         for x in 0..buf_x {
//!             let a = 0xFF;
//!             let r = min(((buf_x - x) * 0xFF) / buf_x, ((buf_y - y) * 0xFF) / buf_y);
//!             let g = min((x * 0xFF) / buf_x, ((buf_y - y) * 0xFF) / buf_y);
//!             let b = min(((buf_x - x) * 0xFF) / buf_x, (y * 0xFF) / buf_y);
//!
//!             let color = (a << 24) + (r << 16) + (g << 8) + b;
//!             buf.write_all(&color.to_ne_bytes()).unwrap();
//!         }
//!     }
//!     buf.flush().unwrap();
//! }
//! ```
//!
use calloop::channel::Channel;
pub use events::NewInputPanelSettings;
pub use events::NewLayerShellSettings;
pub use events::NewPopUpSettings;
pub use events::NewXdgWindowSettings;
pub use events::OutputOption;
pub use waycrate_xkbkeycode::keyboard;
pub use waycrate_xkbkeycode::xkb_keyboard;

pub mod blur;
pub mod corner_radius;
pub mod dpi;
mod events;
#[cfg(feature = "foreign-toplevel")]
pub mod foreign_toplevel;
pub mod home_visibility;
pub mod layer_auto_hide;
pub mod layer_surface_dismiss;
pub mod layer_surface_visibility;
pub mod shadow;
mod strtoshape;
pub mod voice_mode;

use events::DispatchMessageInner;

pub mod id;

pub use events::{
    AxisScroll, DispatchMessage, Ime, LayerShellEvent, ReturnData, XdgInfoChangedType,
};

use strtoshape::str_to_shape;

use waycrate_xkbkeycode::xkb_keyboard::ElementState;
use waycrate_xkbkeycode::xkb_keyboard::RepeatInfo;

use wayland_client::{
    ConnectError, Connection, Dispatch, DispatchError, EventQueue, Proxy, QueueHandle, WEnum,
    delegate_noop,
    globals::{BindError, GlobalError, GlobalList, GlobalListContents, registry_queue_init},
    protocol::{
        wl_buffer::WlBuffer,
        wl_callback::{Event as WlCallbackEvent, WlCallback},
        wl_compositor::WlCompositor,
        wl_display::WlDisplay,
        wl_keyboard::{self, KeyState, KeymapFormat, WlKeyboard},
        wl_output::{self, WlOutput},
        wl_pointer::{self, WlPointer},
        wl_region::WlRegion,
        wl_registry,
        wl_seat::{self, WlSeat},
        wl_shm::WlShm,
        wl_shm_pool::WlShmPool,
        wl_surface::WlSurface,
        wl_touch::{self, WlTouch},
    },
};

use wayland_cursor::{CursorImageBuffer, CursorTheme};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, Anchor, ZwlrLayerSurfaceV1},
};

use wayland_protocols::xdg::shell::client::{
    xdg_popup::{self, XdgPopup},
    xdg_positioner::XdgPositioner,
    xdg_surface::{self, XdgSurface},
    xdg_toplevel::{self, XdgToplevel},
    xdg_wm_base::XdgWmBase,
};

use wayland_protocols::{
    wp::fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::{self, WpFractionalScaleV1},
    },
    xdg::xdg_output::zv1::client::{
        zxdg_output_manager_v1::ZxdgOutputManagerV1,
        zxdg_output_v1::{self, ZxdgOutputV1},
    },
};

use wayland_protocols::wp::input_method::zv1::client::{
    zwp_input_panel_surface_v1::{Position as ZwpInputPanelPosition, ZwpInputPanelSurfaceV1},
    zwp_input_panel_v1::ZwpInputPanelV1,
};

use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};

use wayland_protocols::wp::cursor_shape::v1::client::{
    wp_cursor_shape_device_v1::WpCursorShapeDeviceV1,
    wp_cursor_shape_manager_v1::WpCursorShapeManagerV1,
};

use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};

use wayland_protocols::wp::text_input::zv3::client::{
    zwp_text_input_manager_v3::ZwpTextInputManagerV3,
    zwp_text_input_v3::{self, ContentHint, ContentPurpose, ZwpTextInputV3},
};
use wayland_protocols::xdg::decoration::zv1::client::{
    zxdg_decoration_manager_v1::ZxdgDecorationManagerV1,
    zxdg_toplevel_decoration_v1::{self, ZxdgToplevelDecorationV1},
};

pub use calloop;
use calloop::{
    Error as CallLoopError, EventLoop, LoopHandle, RegistrationToken, channel,
    timer::{TimeoutAction, Timer},
};
use calloop_wayland_source::WaylandSource;
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;
use std::time::Instant;

#[derive(Debug, thiserror::Error)]
pub enum LayerEventError {
    #[error("connect error")]
    ConnectError(#[from] ConnectError),
    #[error("Global Error")]
    GlobalError(#[from] GlobalError),
    #[error("Bind Error")]
    BindError(#[from] BindError),
    #[error("Error during queue")]
    DispatchError(#[from] DispatchError),
    #[error("create file failed")]
    TempFileCreateFailed(#[from] std::io::Error),
    #[error("Event Loop Error")]
    EventLoopInitError(#[from] CallLoopError),
}

pub mod reexport {
    pub use wayland_protocols_wlr::layer_shell::v1::client::{
        zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
        zwlr_layer_surface_v1::{Anchor, KeyboardInteractivity},
    };
    pub mod wl_shm {
        pub use wayland_client::protocol::wl_shm::Format;
        pub use wayland_client::protocol::wl_shm::WlShm;
    }
    pub mod zwp_virtual_keyboard_v1 {
        pub use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
            zwp_virtual_keyboard_manager_v1::{self, ZwpVirtualKeyboardManagerV1},
            zwp_virtual_keyboard_v1::{self, ZwpVirtualKeyboardV1},
        };
    }
    pub mod wp_fractional_scale_v1 {
        pub use wayland_protocols::wp::fractional_scale::v1::client::{
            wp_fractional_scale_manager_v1::{self, WpFractionalScaleManagerV1},
            wp_fractional_scale_v1::{self, WpFractionalScaleV1},
        };
    }
    pub mod wayland_client {
        pub use wayland_client::{
            Connection, QueueHandle, WEnum,
            globals::GlobalList,
            protocol::{
                wl_compositor::WlCompositor,
                wl_keyboard::{self, KeyState},
                wl_pointer::{self, ButtonState},
                wl_region::WlRegion,
                wl_seat::WlSeat,
            },
        };
    }
    pub mod wp_cursor_shape_device_v1 {
        pub use crate::strtoshape::ShapeName;
        pub use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;
    }
    pub mod xdg_toplevel {
        pub use wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;
    }
    pub mod wp_viewport {
        pub use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;
    }
}

#[derive(Debug)]
struct BaseState;

// so interesting, it is just need to invoke once, it just used to get the globals
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for BaseState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: <wl_registry::WlRegistry as wayland_client::Proxy>::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

/// this struct store the xdg_output information
#[derive(Debug, Clone)]
pub struct ZxdgOutputInfo {
    name: String,
    description: String,
    zxdgoutput: ZxdgOutputV1,
    logical_size: (i32, i32),
    position: (i32, i32),
}

impl ZxdgOutputInfo {
    fn new(zxdgoutput: ZxdgOutputV1) -> Self {
        Self {
            zxdgoutput,
            name: "".to_owned(),
            description: "".to_owned(),
            logical_size: (0, 0),
            position: (0, 0),
        }
    }

    /// you can get the Logic position of the screen current surface in
    pub fn get_position(&self) -> (i32, i32) {
        self.position
    }

    /// you can get the LogicalPosition of the screen current surface in
    pub fn get_logical_size(&self) -> (i32, i32) {
        self.logical_size
    }
}

/// This is the unit, binding to per screen.
/// Because layer_shell is so unique, on surface bind to only one
/// wl_output, only one buffer, only one output, so it will store
/// includes the information of ZxdgOutput, size, and layer_shell
///
/// and it can set a binding, you to store the related data. like
/// a cario_context, which is binding to the buffer on the wl_surface.
#[allow(clippy::enum_variant_names)]
#[derive(Debug)]
enum Shell {
    LayerShell(ZwlrLayerSurfaceV1),
    PopUp((XdgPopup, XdgSurface)),
    XdgTopLevel((XdgToplevel, XdgSurface, Option<ZxdgToplevelDecorationV1>)),
    InputPanel(#[allow(unused)] ZwpInputPanelSurfaceV1),
}

impl PartialEq<ZwlrLayerSurfaceV1> for Shell {
    fn eq(&self, other: &ZwlrLayerSurfaceV1) -> bool {
        match self {
            Self::LayerShell(shell) => shell == other,
            _ => false,
        }
    }
}

impl PartialEq<XdgPopup> for Shell {
    fn eq(&self, other: &XdgPopup) -> bool {
        match self {
            Self::PopUp((popup, _)) => popup == other,
            _ => false,
        }
    }
}

impl PartialEq<XdgSurface> for Shell {
    fn eq(&self, other: &XdgSurface) -> bool {
        match self {
            Self::PopUp((_, surface)) => surface == other,
            _ => false,
        }
    }
}
impl PartialEq<XdgToplevel> for Shell {
    fn eq(&self, other: &XdgToplevel) -> bool {
        match self {
            Self::XdgTopLevel((level, _, _)) => level == other,
            _ => false,
        }
    }
}
impl Shell {
    fn destroy(&self) {
        match self {
            Self::PopUp((popup, xdg_surface)) => {
                popup.destroy();
                xdg_surface.destroy();
            }
            Self::XdgTopLevel((top_level, xdg_surface, decoration)) => {
                if let Some(decoration) = decoration {
                    decoration.destroy();
                }
                top_level.destroy();
                xdg_surface.destroy();
            }
            Self::LayerShell(shell) => shell.destroy(),
            Self::InputPanel(_) => {}
        }
    }

    fn is_popup(&self) -> bool {
        matches!(self, Self::PopUp(_))
    }

    fn top_level(&self) -> Option<XdgToplevel> {
        match self {
            Self::XdgTopLevel((level, _, _)) => Some(level.clone()),
            _ => None,
        }
    }
}

/// The state of if we can call a `present` for the window.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
enum PresentAvailableState {
    /// A `wl_surface.frame` request has been sent, and there is no callback yet.
    Requested,
    /// A notification has been received, it is a good time to start drawing a new frame. Because
    /// there is no present at first, so the default state is available.
    #[default]
    Available,
    /// Availability is taken.
    Taken,
}

struct WindowStateUnitBuilder<T> {
    inner: WindowStateUnit<T>,
}

impl<T> WindowStateUnitBuilder<T> {
    fn new(
        id: id::Id,
        qh: QueueHandle<WindowState<T>>,
        display: WlDisplay,
        wl_surface: WlSurface,
        shell: Shell,
    ) -> Self {
        Self {
            inner: WindowStateUnit {
                id,
                qh,
                display,
                wl_surface,
                shell,
                size: (0, 0),
                buffer: Default::default(),
                zxdgoutput: Default::default(),
                fractional_scale: Default::default(),
                viewport: Default::default(),
                wl_output: Default::default(),
                binding: Default::default(),
                becreated: Default::default(),
                // Unknown why it is 120
                scale: 120,
                request_flag: Default::default(),
                present_available_state: Default::default(),
            },
        }
    }

    fn build(self) -> WindowStateUnit<T> {
        self.inner
    }

    fn size(mut self, size: (u32, u32)) -> Self {
        self.inner.size = size;
        self
    }

    fn zxdgoutput(mut self, zxdgoutput: Option<ZxdgOutputInfo>) -> Self {
        self.inner.zxdgoutput = zxdgoutput;
        self
    }

    fn fractional_scale(mut self, fractional_scale: Option<WpFractionalScaleV1>) -> Self {
        self.inner.fractional_scale = fractional_scale;
        self
    }

    fn viewport(mut self, viewport: Option<WpViewport>) -> Self {
        self.inner.viewport = viewport;
        self
    }

    fn wl_output(mut self, wl_output: Option<WlOutput>) -> Self {
        self.inner.wl_output = wl_output;
        self
    }

    fn binding(mut self, binding: Option<T>) -> Self {
        self.inner.binding = binding;
        self
    }

    fn becreated(mut self, becreated: bool) -> Self {
        self.inner.becreated = becreated;
        self
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RefreshRequest {
    /// Redraw the next frame.
    NextFrame,

    /// Redraw at the given time.
    At(Instant),

    /// No redraw is needed.
    #[default]
    Wait,
}

#[derive(Debug, Default)]
struct WindowStateUnitRequestFlag {
    /// The flag of if this window has been requested to be closed.
    close: bool,
    /// The flag of if this window has been requested to be refreshed.
    refresh: RefreshRequest,
}

#[derive(Debug)]
pub struct WindowStateUnit<T> {
    id: id::Id,
    qh: QueueHandle<WindowState<T>>,
    display: WlDisplay,
    wl_surface: WlSurface,
    size: (u32, u32),
    buffer: Option<WlBuffer>,
    shell: Shell,
    zxdgoutput: Option<ZxdgOutputInfo>,
    fractional_scale: Option<WpFractionalScaleV1>,
    viewport: Option<WpViewport>,
    wl_output: Option<WlOutput>,
    binding: Option<T>,
    becreated: bool,

    scale: u32,
    request_flag: WindowStateUnitRequestFlag,
    present_available_state: PresentAvailableState,
}

impl<T> WindowStateUnit<T> {
    fn is_popup(&self) -> bool {
        self.shell.is_popup()
    }
}

impl<T> WindowStateUnit<T> {
    /// get the WindowState id
    pub fn id(&self) -> id::Id {
        self.id
    }

    pub fn try_set_viewport_destination(&self, width: i32, height: i32) -> Option<()> {
        let viewport = self.viewport.as_ref()?;
        viewport.set_destination(width, height);
        Some(())
    }

    pub fn try_set_viewport_source(&self, x: f64, y: f64, width: f64, height: f64) -> Option<()> {
        let viewport = self.viewport.as_ref()?;
        viewport.set_source(x, y, width, height);
        Some(())
    }

    /// gen the WindowState [WindowWrapper]
    pub fn gen_wrapper(&self) -> WindowWrapper {
        WindowWrapper {
            id: self.id,
            display: self.display.clone(),
            wl_surface: self.wl_surface.clone(),
            viewport: self.viewport.clone(),
            toplevel: self.shell.top_level(),
        }
    }
}
impl<T> WindowStateUnit<T> {
    #[inline]
    pub fn raw_window_handle_rwh_06(&self) -> Result<rwh_06::RawWindowHandle, rwh_06::HandleError> {
        Ok(rwh_06::WaylandWindowHandle::new({
            let ptr = self.wl_surface.id().as_ptr();
            std::ptr::NonNull::new(ptr as *mut _).expect("wl_surface will never be null")
        })
        .into())
    }

    #[inline]
    pub fn raw_display_handle_rwh_06(
        &self,
    ) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
        Ok(rwh_06::WaylandDisplayHandle::new({
            let ptr = self.display.id().as_ptr();
            std::ptr::NonNull::new(ptr as *mut _).expect("wl_proxy should never be null")
        })
        .into())
    }
}

impl<T> rwh_06::HasWindowHandle for WindowStateUnit<T> {
    fn window_handle(&self) -> Result<rwh_06::WindowHandle<'_>, rwh_06::HandleError> {
        let raw = self.raw_window_handle_rwh_06()?;

        // SAFETY: The window handle will never be deallocated while the window is alive,
        // and the main thread safety requirements are upheld internally by each platform.
        Ok(unsafe { rwh_06::WindowHandle::borrow_raw(raw) })
    }
}

impl<T> rwh_06::HasDisplayHandle for WindowStateUnit<T> {
    fn display_handle(&self) -> Result<rwh_06::DisplayHandle<'_>, rwh_06::HandleError> {
        let raw = self.raw_display_handle_rwh_06()?;

        // SAFETY: The window handle will never be deallocated while the window is alive,
        // and the main thread safety requirements are upheld internally by each platform.
        Ok(unsafe { rwh_06::DisplayHandle::borrow_raw(raw) })
    }
}

// if is only one window, use it will be easy
impl<T> rwh_06::HasWindowHandle for WindowState<T> {
    fn window_handle(&self) -> Result<rwh_06::WindowHandle<'_>, rwh_06::HandleError> {
        let raw = self.main_window().raw_window_handle_rwh_06()?;

        // SAFETY: The window handle will never be deallocated while the window is alive,
        // and the main thread safety requirements are upheld internally by each platform.
        Ok(unsafe { rwh_06::WindowHandle::borrow_raw(raw) })
    }
}

// if is only one window, use it will be easy
impl<T> rwh_06::HasDisplayHandle for WindowState<T> {
    fn display_handle(&self) -> Result<rwh_06::DisplayHandle<'_>, rwh_06::HandleError> {
        let raw = self.main_window().raw_display_handle_rwh_06()?;

        // SAFETY: The window handle will never be deallocated while the window is alive,
        // and the main thread safety requirements are upheld internally by each platform.
        Ok(unsafe { rwh_06::DisplayHandle::borrow_raw(raw) })
    }
}
impl<T> WindowStateUnit<T> {
    /// get the wl surface from WindowState
    pub fn get_wlsurface(&self) -> &WlSurface {
        &self.wl_surface
    }

    /// get the xdg_output info related to this unit
    pub fn get_xdgoutput_info(&self) -> Option<&ZxdgOutputInfo> {
        self.zxdgoutput.as_ref()
    }

    /// set the anchor of the current unit. please take the simple.rs as reference
    pub fn set_anchor(&self, anchor: Anchor) {
        if let Shell::LayerShell(layer_shell) = &self.shell {
            layer_shell.set_anchor(anchor);
            self.wl_surface.commit();
        }
    }

    /// you can reset the margin which bind to the surface
    pub fn set_margin(&self, (top, right, bottom, left): (i32, i32, i32, i32)) {
        if let Shell::LayerShell(layer_shell) = &self.shell {
            layer_shell.set_margin(top, right, bottom, left);
            self.wl_surface.commit();
        }
    }

    /// set the layer
    pub fn set_layer(&self, layer: Layer) {
        if let Shell::LayerShell(layer_shell) = &self.shell {
            layer_shell.set_layer(layer);
            self.wl_surface.commit();
        }
    }

    /// set the anchor and set the size together
    /// When you want to change layer from LEFT|RIGHT|BOTTOM to TOP|LEFT|BOTTOM, use it
    pub fn set_anchor_with_size(&self, anchor: Anchor, (width, height): (u32, u32)) {
        if let Shell::LayerShell(layer_shell) = &self.shell {
            layer_shell.set_anchor(anchor);
            layer_shell.set_size(width, height);
            self.wl_surface.commit();
        }
    }

    /// set the layer size of current unit
    pub fn set_size(&self, (width, height): (u32, u32)) {
        if let Shell::LayerShell(layer_shell) = &self.shell {
            layer_shell.set_size(width, height);
            self.wl_surface.commit();
        }
    }

    /// set current exclusive_zone
    pub fn set_exclusive_zone(&self, zone: i32) {
        if let Shell::LayerShell(layer_shell) = &self.shell {
            layer_shell.set_exclusive_zone(zone);
            self.wl_surface.commit();
        }
    }

    /// you can use this function to set a binding data. the message passed back contain
    /// a index, you can use that to get the unit. It will be very useful, because you can
    /// use the binding data to operate the file binding to the buffer. you can take
    /// startcolorkeyboard as reference.
    pub fn set_binding(&mut self, binding: T) {
        self.binding = Some(binding);
    }

    /// return the binding data, with mut reference
    pub fn get_binding_mut(&mut self) -> Option<&mut T> {
        self.binding.as_mut()
    }

    /// get the binding data
    pub fn get_binding(&self) -> Option<&T> {
        self.binding.as_ref()
    }

    /// get the size of the surface
    pub fn get_size(&self) -> (u32, u32) {
        self.size
    }

    /// this function will refresh whole surface. it will reattach the buffer, and damage whole,
    /// and final commit
    pub fn refresh(&self) {
        self.wl_surface.attach(self.buffer.as_ref(), 0, 0);
        self.wl_surface
            .damage(0, 0, self.size.0 as i32, self.size.1 as i32);
        self.wl_surface.commit();
    }

    pub fn scale_u32(&self) -> u32 {
        self.scale
    }

    pub fn scale_float(&self) -> f64 {
        self.scale as f64 / 120.
    }

    pub fn request_close(&mut self) {
        self.request_flag.close = true;
    }

    pub fn request_refresh(&mut self, request: RefreshRequest) {
        // refresh request in nearest future has the highest priority.
        match self.request_flag.refresh {
            RefreshRequest::NextFrame => {}
            RefreshRequest::At(instant) => match request {
                RefreshRequest::NextFrame => self.request_flag.refresh = request,
                RefreshRequest::At(other_instant) => {
                    if other_instant < instant {
                        self.request_flag.refresh = request;
                    }
                }
                RefreshRequest::Wait => {}
            },
            RefreshRequest::Wait => self.request_flag.refresh = request,
        }
    }

    fn should_refresh(&self) -> bool {
        match self.request_flag.refresh {
            RefreshRequest::NextFrame => true,
            RefreshRequest::At(instant) => instant <= Instant::now(),
            RefreshRequest::Wait => false,
        }
    }

    pub fn take_present_slot(&mut self) -> bool {
        if !self.should_refresh() {
            return false;
        }
        if self.present_available_state != PresentAvailableState::Available {
            return false;
        }
        self.request_flag.refresh = RefreshRequest::Wait;
        self.present_available_state = PresentAvailableState::Taken;
        true
    }

    pub fn reset_present_slot(&mut self) -> bool {
        if self.present_available_state == PresentAvailableState::Taken {
            self.present_available_state = PresentAvailableState::Available;
            true
        } else {
            false
        }
    }
}

impl<T: 'static> WindowStateUnit<T> {
    pub fn request_next_present(&mut self) {
        match self.present_available_state {
            PresentAvailableState::Taken => {
                self.present_available_state = PresentAvailableState::Requested;
                self.wl_surface
                    .frame(&self.qh, (self.id, PresentAvailableState::Available));
            }
            PresentAvailableState::Requested | PresentAvailableState::Available => {}
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub enum ImePurpose {
    /// No special hints for the IME (default).
    Normal,
    /// The IME is used for password input.
    Password,
    /// The IME is used to input into a terminal.
    ///
    /// For example, that could alter OSK on Wayland to show extra buttons.
    Terminal,
}

#[derive(Debug)]
struct KeyboardTokenState {
    delay: Duration,
    key: u32,
    surface_id: Option<id::Id>,
    pressed_state: ElementState,
}

#[derive(Debug)]
pub struct VirtualKeyRelease {
    pub delay: Duration,
    pub time: u32,
    pub key: u32,
}

/// main state, store the main information
#[derive(Debug)]
pub struct WindowState<T> {
    outputs: Vec<(u32, wl_output::WlOutput)>,
    current_surface: Option<WlSurface>,
    active_surfaces: HashMap<Option<i32>, (WlSurface, Option<id::Id>)>,
    units: Vec<WindowStateUnit<T>>,
    message: Vec<(Option<id::Id>, DispatchMessageInner)>,

    connection: Option<Connection>,
    event_queue: Option<EventQueue<WindowState<T>>>,
    wl_compositor: Option<WlCompositor>,
    xdg_output_manager: Option<ZxdgOutputManagerV1>,
    wmbase: Option<XdgWmBase>,
    shm: Option<WlShm>,
    cursor_manager: Option<WpCursorShapeManagerV1>,
    viewporter: Option<WpViewporter>,
    fractional_scale_manager: Option<WpFractionalScaleManagerV1>,
    globals: Option<GlobalList>,

    // background
    background_surface: Option<WlSurface>,
    display: Option<WlDisplay>,

    // base managers
    seat: Option<WlSeat>,
    keyboard_state: Option<xkb_keyboard::KeyboardState>,

    pointer: Option<WlPointer>,
    touch: Option<WlTouch>,
    virtual_keyboard: Option<ZwpVirtualKeyboardV1>,

    // states
    namespace: String,
    keyboard_interactivity: zwlr_layer_surface_v1::KeyboardInteractivity,
    anchor: Anchor,
    layer: Layer,
    size: Option<(u32, u32)>,
    exclusive_zone: Option<i32>,
    margin: Option<(i32, i32, i32, i32)>,

    // settings
    use_display_handle: bool,
    repeat_delay: Option<KeyboardTokenState>,
    to_remove_tokens: Vec<RegistrationToken>,
    closed_ids: Vec<id::Id>,

    to_be_released_key: Option<VirtualKeyRelease>,

    last_unit_index: usize,
    last_wloutput: Option<WlOutput>,

    return_data: Vec<ReturnData<T>>,
    finger_locations: HashMap<i32, (f64, f64)>,
    enter_serial: Option<u32>,

    xdg_info_cache: Vec<(wl_output::WlOutput, ZxdgOutputInfo)>,

    start_mode: StartMode,
    init_finished: bool,
    events_transparent: bool,
    /// Whether to request blur effect for surfaces
    blur: bool,
    /// Blur manager (bound lazily when blur is enabled)
    blur_manager: Option<blur::org_kde_kwin_blur_manager::OrgKdeKwinBlurManager>,
    /// Corner radius for surfaces (all four corners)
    corner_radius: Option<[u32; 4]>,
    /// Corner radius manager (bound lazily when corner_radius is set)
    corner_radius_manager:
        Option<corner_radius::layer_corner_radius_manager_v1::LayerCornerRadiusManagerV1>,
    /// Corner radius surfaces per surface (keyed by surface protocol ID)
    corner_radius_surfaces:
        HashMap<u32, corner_radius::layer_corner_radius_surface_v1::LayerCornerRadiusSurfaceV1>,
    /// Whether to request shadow effect for surfaces
    shadow: bool,
    /// Shadow manager (bound lazily when shadow is enabled)
    shadow_manager: Option<shadow::layer_shadow_manager_v1::LayerShadowManagerV1>,

    /// Auto-hide manager (bound lazily when needed)
    auto_hide_manager:
        Option<layer_auto_hide::layer_auto_hide_manager_v1::LayerAutoHideManagerV1>,
    /// Auto-hide objects per surface (keyed by surface protocol ID)
    auto_hide_surfaces:
        HashMap<u32, layer_auto_hide::layer_auto_hide_v1::LayerAutoHideV1>,

    /// Whether to use home visibility mode (home_only = only visible when at home)
    home_only: bool,
    /// Whether to hide when in home mode (inverse of home_only)
    hide_on_home: bool,
    /// Home visibility manager (bound lazily when home_only or hide_on_home is enabled)
    home_visibility_manager:
        Option<home_visibility::zcosmic_home_visibility_manager_v1::ZcosmicHomeVisibilityManagerV1>,
    /// Home visibility controllers per surface (keyed by surface protocol ID)
    home_visibility_controllers:
        HashMap<u32, home_visibility::zcosmic_home_visibility_v1::ZcosmicHomeVisibilityV1>,
    /// Current home state from compositor (true = at home, false = windows visible)
    is_home: bool,

    /// Whether to register for voice mode events
    voice_mode_enabled: bool,
    /// Voice mode manager (bound lazily when voice_mode_enabled is true)
    voice_mode_manager:
        Option<voice_mode::zcosmic_voice_mode_manager_v1::ZcosmicVoiceModeManagerV1>,
    /// Voice mode receivers per surface (keyed by surface protocol ID)
    voice_mode_receivers:
        HashMap<u32, voice_mode::zcosmic_voice_mode_v1::ZcosmicVoiceModeV1>,
    /// Pending voice mode events from compositor
    voice_mode_events: Vec<voice_mode::VoiceModeEvent>,

    /// Layer surface visibility manager (bound lazily when needed)
    layer_surface_visibility_manager: Option<
        layer_surface_visibility::zcosmic_layer_surface_visibility_manager_v1::ZcosmicLayerSurfaceVisibilityManagerV1,
    >,
    /// Visibility controllers per surface (keyed by surface protocol ID)
    layer_surface_visibility_controllers: HashMap<
        u32,
        layer_surface_visibility::zcosmic_layer_surface_visibility_v1::ZcosmicLayerSurfaceVisibilityV1,
    >,

    /// Layer surface dismiss manager (bound lazily when needed)
    layer_surface_dismiss_manager: Option<
        layer_surface_dismiss::zcosmic_layer_surface_dismiss_manager_v1::ZcosmicLayerSurfaceDismissManagerV1,
    >,
    /// Dismiss controllers per surface (keyed by surface protocol ID)
    layer_surface_dismiss_controllers: HashMap<
        u32,
        layer_surface_dismiss::zcosmic_layer_surface_dismiss_v1::ZcosmicLayerSurfaceDismissV1,
    >,
    /// Pending dismiss events from compositor
    dismiss_requested: bool,

    /// Whether to track foreign toplevel windows (taskbar/dock functionality)
    #[cfg(feature = "foreign-toplevel")]
    foreign_toplevel_enabled: bool,

    // ext_foreign_toplevel_list_v1 (preferred, standard Wayland protocol)
    #[cfg(feature = "foreign-toplevel")]
    ext_foreign_toplevel_list: Option<
        wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1,
    >,

    // zcosmic_toplevel_info_v1 (COSMIC extension for state info)
    #[cfg(feature = "cosmic-toplevel")]
    cosmic_toplevel_info: Option<
        cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1,
    >,

    // zcosmic_toplevel_manager_v1 (COSMIC extension for control - activate, close, etc.)
    #[cfg(feature = "cosmic-toplevel")]
    cosmic_toplevel_manager: Option<
        cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1,
    >,

    /// Mapping from cosmic handle protocol ID to ext handle protocol ID
    #[cfg(feature = "cosmic-toplevel")]
    cosmic_to_ext_handle_map: HashMap<u32, u32>,

    /// COSMIC toplevel handles for control operations (keyed by ext handle protocol ID)
    #[cfg(feature = "cosmic-toplevel")]
    cosmic_toplevel_handles: HashMap<u32, cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1>,

    // zwlr_foreign_toplevel_manager_v1 (wlroots fallback)
    #[cfg(feature = "foreign-toplevel")]
    foreign_toplevel_manager: Option<
        wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1,
    >,

    /// Data for each tracked foreign toplevel window (keyed by protocol object ID)
    #[cfg(feature = "foreign-toplevel")]
    foreign_toplevel_data: HashMap<u32, foreign_toplevel::ToplevelHandleData>,

    /// Handles for each tracked foreign toplevel window (for sending commands)
    #[cfg(feature = "foreign-toplevel")]
    foreign_toplevel_handles: HashMap<u32, wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1>,

    text_input_manager: Option<ZwpTextInputManagerV3>,
    text_input: Option<ZwpTextInputV3>,
    text_inputs: Vec<ZwpTextInputV3>,

    xdg_decoration_manager: Option<ZxdgDecorationManagerV1>,

    ime_purpose: ImePurpose,
    ime_allowed: bool,
}

impl<T> WindowState<T> {
    pub fn append_return_data(&mut self, data: ReturnData<T>) {
        self.return_data.push(data);
    }
    /// remove a shell, destroy the surface
    fn remove_shell(&mut self, id: id::Id) -> Option<()> {
        let index = self
            .units
            .iter()
            .position(|unit| unit.id == id && unit.becreated)?;

        self.units[index].shell.destroy();
        self.units[index].wl_surface.destroy();

        if let Some(buffer) = self.units[index].buffer.as_ref() {
            buffer.destroy()
        }
        self.units.remove(index);
        Some(())
    }

    /// forget the remembered last output, next time it will get the new activated output to set the
    /// layershell
    pub fn forget_last_output(&mut self) {
        self.last_wloutput.take();
    }
}

/// Simple WindowState, without any data binding or info
pub type WindowStateSimple = WindowState<()>;

impl<T> WindowState<T> {
    // return the first window
    // I will use it in iced
    pub fn main_window(&self) -> &WindowStateUnit<T> {
        &self.units[0]
    }

    /// use iced id to find WindowStateUnit
    pub fn get_window_with_id(&self, id: id::Id) -> Option<&WindowStateUnit<T>> {
        self.units.iter().find(|w| w.id() == id)
    }
    // return all windows
    pub fn windows(&self) -> &Vec<WindowStateUnit<T>> {
        &self.units
    }

    /// Get the current home state from compositor
    /// Returns true if at home (no windows visible), false otherwise
    pub fn is_home(&self) -> bool {
        self.is_home
    }

    /// Send audio level to compositor for voice orb visualization.
    /// Level should be 0-1000 (0=silence, 1000=max amplitude).
    /// Only has effect when voice mode is active.
    pub fn send_voice_audio_level(&self, level: u32) {
        // Send to all registered receivers
        for receiver in self.voice_mode_receivers.values() {
            receiver.set_audio_level(level.min(1000));
        }
    }

    /// Acknowledge a will_stop event from the compositor.
    /// serial - the serial from the will_stop event
    /// freeze - if true, freeze the orb in place for processing.
    ///          if false, proceed with hiding the orb.
    pub fn voice_ack_stop(&self, serial: u32, freeze: bool) {
        let freeze_val: u32 = if freeze { 1 } else { 0 };
        for receiver in self.voice_mode_receivers.values() {
            receiver.ack_stop(serial, freeze_val);
        }
    }

    /// Dismiss the frozen voice orb.
    /// This tells the compositor to hide the orb when transcription completes
    /// without spawning a new window (e.g., empty result or error).
    /// Only valid when orb is in frozen state.
    pub fn voice_dismiss(&self) {
        log::info!("Sending voice dismiss to compositor");
        for receiver in self.voice_mode_receivers.values() {
            receiver.dismiss();
        }
    }

    fn push_window(&mut self, window_state_unit: WindowStateUnit<T>) {
        let surface = window_state_unit.wl_surface.clone();
        self.units.push(window_state_unit);
        // new created surface will be current_surface.
        self.update_current_surface(Some(surface));
    }
}

#[derive(Debug)]
pub struct WindowWrapper {
    pub id: id::Id,
    display: WlDisplay,
    wl_surface: WlSurface,
    pub viewport: Option<WpViewport>,
    pub toplevel: Option<XdgToplevel>,
}

/// Define the way layershell program is start
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum StartMode {
    /// default is use the activated display, in layershell, the param is `None`
    #[default]
    Active,
    /// be started as background program, be used with some programs like xdg-desktop-portal
    Background,
    /// listen on the create event of display, always shown on all screens
    AllScreens,
    /// only shown on target screen
    TargetScreen(String),

    /// Target the output
    /// NOTE: use the same wayland connection
    TargetOutput(WlOutput),
}

impl StartMode {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }
    pub fn is_background(&self) -> bool {
        matches!(self, Self::Background)
    }
    pub fn is_allscreens(&self) -> bool {
        matches!(self, Self::AllScreens)
    }
    pub fn is_with_target(&self) -> bool {
        matches!(self, Self::TargetScreen(_))
    }
}

impl WindowWrapper {
    pub fn id(&self) -> id::Id {
        self.id
    }
}

impl<T> WindowState<T> {
    /// get a seat from state
    pub fn get_seat(&self) -> &WlSeat {
        self.seat.as_ref().unwrap()
    }

    /// get the keyboard
    pub fn get_keyboard(&self) -> Option<&WlKeyboard> {
        Some(&self.keyboard_state.as_ref()?.keyboard)
    }

    /// get the pointer
    pub fn get_pointer(&self) -> Option<&WlPointer> {
        self.pointer.as_ref()
    }

    /// get the touch
    pub fn get_touch(&self) -> Option<&WlTouch> {
        self.touch.as_ref()
    }
}

impl<T> WindowState<T> {
    /// gen the wrapper to the main window
    /// used to get display and etc
    pub fn gen_mainwindow_wrapper(&self) -> WindowWrapper {
        self.main_window().gen_wrapper()
    }

    pub fn is_active(&self) -> bool {
        self.start_mode.is_active()
    }

    pub fn is_background(&self) -> bool {
        self.start_mode.is_background()
    }

    pub fn is_allscreens(&self) -> bool {
        self.start_mode.is_allscreens()
    }

    pub fn is_with_target(&self) -> bool {
        self.start_mode.is_with_target()
    }

    /// Execute a toplevel action (activate, close, minimize, etc.)
    ///
    /// Returns true if the action was executed, false if the handle was not found.
    /// Requires the `foreign-toplevel` feature.
    #[cfg(feature = "foreign-toplevel")]
    pub fn execute_toplevel_action(&self, action: foreign_toplevel::ToplevelAction) -> bool
    where
        T: 'static,
    {
        foreign_toplevel::execute_toplevel_action(self, action, self.seat.as_ref())
    }

    pub fn ime_allowed(&self) -> bool {
        self.ime_allowed
    }

    pub fn set_ime_allowed(&mut self, ime_allowed: bool) {
        self.ime_allowed = ime_allowed;
        for text_input in &self.text_inputs {
            if ime_allowed {
                text_input.enable();
                text_input.set_content_type_by_purpose(self.ime_purpose);
            } else {
                text_input.disable();
            }
            text_input.commit();
        }
    }

    pub fn set_ime_cursor_area<P: Into<dpi::Position>, S: Into<dpi::Size>>(
        &self,
        position: P,
        size: S,
        id: id::Id,
    ) {
        if !self.ime_allowed() {
            return;
        }
        let position: dpi::Position = position.into();
        let size: dpi::Size = size.into();
        let Some(unit) = self.get_window_with_id(id) else {
            return;
        };
        let scale_factor = unit.scale_float();
        let position: dpi::LogicalPosition<u32> = position.to_logical(scale_factor);
        let size: dpi::LogicalSize<u32> = size.to_logical(scale_factor);
        let (x, y) = (position.x as i32, position.y as i32);
        let (width, height) = (size.width as i32, size.height as i32);
        for text_input in self.text_inputs.iter() {
            text_input.set_cursor_rectangle(x, y, width, height);
            text_input.commit();
        }
    }

    pub fn set_ime_purpose(&mut self, purpose: ImePurpose) {
        self.ime_purpose = purpose;
        self.text_input.iter().for_each(|text_input| {
            text_input.set_content_type_by_purpose(purpose);
            text_input.commit();
        });
    }

    #[inline]
    pub fn text_input_entered(&mut self, text_input: &ZwpTextInputV3) {
        if !self.text_inputs.iter().any(|t| t == text_input) {
            self.text_inputs.push(text_input.clone());
        }
    }

    #[inline]
    pub fn text_input_left(&mut self, text_input: &ZwpTextInputV3) {
        if let Some(position) = self.text_inputs.iter().position(|t| t == text_input) {
            self.text_inputs.remove(position);
        }
    }

    fn ime_purpose(&self) -> ImePurpose {
        self.ime_purpose
    }
}

impl<T: 'static> WindowState<T> {
    /// Set corner radius for a specific surface
    /// radii: [top_left, top_right, bottom_right, bottom_left] or None to unset
    pub fn set_corner_radius_for_surface(&mut self, surface: &WlSurface, radii: Option<[u32; 4]>) {
        let surface_id = surface.id().protocol_id();

        // Check if we already have a corner radius object for this surface
        if let Some(corner_obj) = self.corner_radius_surfaces.get(&surface_id) {
            if let Some(r) = radii {
                corner_obj.set_radius(r[0], r[1], r[2], r[3]);
                log::info!("Updated corner radius for surface: {:?}", r);
            } else {
                corner_obj.unset_radius();
                log::info!("Unset corner radius for surface");
            }
            surface.commit();
            return;
        }

        // Need to create a new corner radius object
        if let Some(manager) = &self.corner_radius_manager {
            let corner_data = corner_radius::CornerRadiusData {
                surface: surface.clone(),
            };
            // Get a queue handle from the first unit (they all share the same queue)
            if let Some(unit) = self.units.first() {
                let corner_obj = manager.get_corner_radius(surface, &unit.qh, corner_data);
                if let Some(r) = radii {
                    corner_obj.set_radius(r[0], r[1], r[2], r[3]);
                    log::info!("Updated corner radius for surface: {:?}", r);
                } else {
                    corner_obj.unset_radius();
                    log::info!("Unset corner radius for surface");
                }
                self.corner_radius_surfaces.insert(surface_id, corner_obj);
                surface.commit();
            }
        } else {
            log::warn!(
                "Corner radius manager not available - ensure corner_radius was set in settings"
            );
        }
    }

    /// Enable compositor-driven auto-hide for a specific surface.
    /// The compositor will animate hide/show transitions and handle hover detection.
    /// `edge`: which edge to slide off (0 = bottom)
    /// `edge_zone`: hover detection zone in pixels at the screen edge
    /// `mode`: 0 = always hide when cursor leaves, 1 = only hide when maximized/fullscreen exists
    pub fn set_auto_hide_for_surface(
        &mut self,
        surface: &WlSurface,
        edge: u32,
        edge_zone: u32,
        mode: u32,
    ) {
        let surface_id = surface.id().protocol_id();
        let edge_enum = layer_auto_hide::layer_auto_hide_v1::Edge::try_from(edge)
            .unwrap_or(layer_auto_hide::layer_auto_hide_v1::Edge::Bottom);
        let mode_enum = layer_auto_hide::layer_auto_hide_v1::Mode::try_from(mode)
            .unwrap_or(layer_auto_hide::layer_auto_hide_v1::Mode::Always);

        // Check if we already have an auto-hide object for this surface
        if let Some(auto_hide_obj) = self.auto_hide_surfaces.get(&surface_id) {
            auto_hide_obj.set_auto_hide(edge_enum, edge_zone, mode_enum);
            surface.commit();
            return;
        }

        // Need to create a new auto-hide object
        if let Some(manager) = &self.auto_hide_manager {
            let auto_hide_data = layer_auto_hide::LayerAutoHideData {
                surface: surface.clone(),
            };
            // Get a queue handle from the first unit (they all share the same queue)
            if let Some(unit) = self.units.first() {
                let auto_hide_obj = manager.get_auto_hide(surface, &unit.qh, auto_hide_data);
                auto_hide_obj.set_auto_hide(edge_enum, edge_zone, mode_enum);
                self.auto_hide_surfaces.insert(surface_id, auto_hide_obj);
                surface.commit();
            }
        } else {
            log::warn!(
                "Auto-hide manager not available - compositor may not support this protocol"
            );
        }
    }

    /// Disable compositor-driven auto-hide for a specific surface.
    pub fn unset_auto_hide_for_surface(&mut self, surface: &WlSurface) {
        let surface_id = surface.id().protocol_id();

        if let Some(auto_hide_obj) = self.auto_hide_surfaces.get(&surface_id) {
            auto_hide_obj.unset_auto_hide();
            surface.commit();
        }
    }

    /// Set home visibility mode for a specific surface
    /// This allows dynamically changing whether a surface is visible at home or not
    pub fn set_visibility_mode_for_surface(
        &mut self,
        surface: &WlSurface,
        mode: home_visibility::VisibilityMode,
    ) {
        let surface_id = surface.id().protocol_id();

        // Check if we already have a controller for this surface
        if let Some(controller) = self.home_visibility_controllers.get(&surface_id) {
            controller.set_visibility_mode(mode);
            log::info!(
                "Updated visibility mode to {:?} for surface {}",
                mode,
                surface_id
            );
            return;
        }

        // Need to create a new controller
        if let Some(manager) = &self.home_visibility_manager {
            if let Some(unit) = self.units.first() {
                let visibility_data = home_visibility::HomeVisibilityData {
                    surface: surface.clone(),
                };
                let visibility_obj =
                    manager.get_home_visibility(surface, &unit.qh, visibility_data);
                visibility_obj.set_visibility_mode(mode);
                self.home_visibility_controllers
                    .insert(surface_id, visibility_obj);
                log::info!(
                    "Created and set visibility mode to {:?} for surface {}",
                    mode,
                    surface_id
                );
            }
        } else {
            log::warn!(
                "Home visibility manager not available - ensure home_only or hide_on_home was set in settings"
            );
        }
    }

    /// Hide a surface without destroying it (using layer_surface_visibility protocol)
    /// The surface will not be rendered and won't receive input events.
    /// Use show_surface to make it visible again.
    pub fn hide_surface(&mut self, surface: &WlSurface) {
        let surface_id = surface.id().protocol_id();

        // Check if we already have a visibility controller for this surface
        if let Some(controller) = self.layer_surface_visibility_controllers.get(&surface_id) {
            controller.set_hidden();
            // Flush to ensure compositor processes immediately
            if let Some(ref conn) = self.connection {
                let _ = conn.flush();
            }
            log::info!("Hidden surface {} (existing controller)", surface_id);
            return;
        }

        // Need to create a new controller
        if let Some(manager) = &self.layer_surface_visibility_manager {
            if let Some(unit) = self.units.first() {
                let visibility_data = layer_surface_visibility::LayerSurfaceVisibilityData {
                    surface: surface.clone(),
                    hidden: true,
                };
                let controller =
                    manager.get_visibility_controller(surface, &unit.qh, visibility_data);
                controller.set_hidden();
                self.layer_surface_visibility_controllers
                    .insert(surface_id, controller);
                // Flush to ensure compositor processes immediately
                if let Some(ref conn) = self.connection {
                    let _ = conn.flush();
                }
                log::info!("Hidden surface {} (new controller)", surface_id);
            }
        } else {
            log::warn!(
                "Layer surface visibility manager not available - compositor may not support this protocol"
            );
        }
    }

    /// Show a surface that was previously hidden
    pub fn show_surface(&mut self, surface: &WlSurface) {
        let surface_id = surface.id().protocol_id();

        // Check if we have a visibility controller for this surface
        if let Some(controller) = self.layer_surface_visibility_controllers.get(&surface_id) {
            controller.set_visible();
            // Flush to ensure compositor processes immediately
            if let Some(ref conn) = self.connection {
                let _ = conn.flush();
            }
            log::info!("Shown surface {} (existing controller)", surface_id);
            return;
        }

        // Need to create a new controller (surface was never hidden, but make it explicit)
        if let Some(manager) = &self.layer_surface_visibility_manager {
            if let Some(unit) = self.units.first() {
                let visibility_data = layer_surface_visibility::LayerSurfaceVisibilityData {
                    surface: surface.clone(),
                    hidden: false,
                };
                let controller =
                    manager.get_visibility_controller(surface, &unit.qh, visibility_data);
                controller.set_visible();
                self.layer_surface_visibility_controllers
                    .insert(surface_id, controller);
                // Flush to ensure compositor processes immediately
                if let Some(ref conn) = self.connection {
                    let _ = conn.flush();
                }
                log::info!("Shown surface {} (new controller)", surface_id);
            }
        } else {
            log::warn!(
                "Layer surface visibility manager not available - compositor may not support this protocol"
            );
        }
    }

    /// Get or create a dismiss controller for a surface
    /// Returns the controller if available, or None if the protocol is not supported
    fn get_or_create_dismiss_controller(
        &mut self,
        surface: &WlSurface,
    ) -> Option<layer_surface_dismiss::zcosmic_layer_surface_dismiss_v1::ZcosmicLayerSurfaceDismissV1>
    {
        let surface_id = surface.id().protocol_id();

        // Check if we already have a controller for this surface
        if let Some(controller) = self.layer_surface_dismiss_controllers.get(&surface_id) {
            return Some(controller.clone());
        }

        // Need to create a new controller
        let manager = self.layer_surface_dismiss_manager.as_ref()?;
        let unit = self.units.first()?;

        let dismiss_data = layer_surface_dismiss::LayerSurfaceDismissData {
            surface: surface.clone(),
        };
        let controller = manager.get_dismiss_controller(surface, &unit.qh, dismiss_data);
        self.layer_surface_dismiss_controllers
            .insert(surface_id, controller.clone());
        log::debug!("Created dismiss controller for surface {}", surface_id);
        Some(controller)
    }

    /// Arm dismiss notifications for a surface
    /// Once armed, a dismiss_requested event will be sent when the user clicks/touches
    /// outside the surface's dismiss group.
    pub fn arm_dismiss(&mut self, surface: &WlSurface) {
        if let Some(controller) = self.get_or_create_dismiss_controller(surface) {
            controller.arm();
            if let Some(ref conn) = self.connection {
                let _ = conn.flush();
            }
            log::info!("Armed dismiss for surface {}", surface.id().protocol_id());
        } else {
            log::warn!(
                "Layer surface dismiss manager not available - compositor may not support this protocol"
            );
        }
    }

    /// Disarm dismiss notifications for a surface
    /// The surface will no longer receive dismiss_requested events.
    pub fn disarm_dismiss(&mut self, surface: &WlSurface) {
        if let Some(controller) = self.get_or_create_dismiss_controller(surface) {
            controller.disarm();
            if let Some(ref conn) = self.connection {
                let _ = conn.flush();
            }
            log::info!(
                "Disarmed dismiss for surface {}",
                surface.id().protocol_id()
            );
        } else {
            log::warn!(
                "Layer surface dismiss manager not available - compositor may not support this protocol"
            );
        }
    }

    /// Add a surface to the dismiss group of another surface
    /// When the popup_surface is armed, clicks outside both surfaces will trigger dismiss.
    /// The group_surface is typically the parent panel bar.
    pub fn add_to_dismiss_group(&mut self, popup_surface: &WlSurface, group_surface: &WlSurface) {
        if let Some(controller) = self.get_or_create_dismiss_controller(popup_surface) {
            controller.add_to_group(group_surface);
            if let Some(ref conn) = self.connection {
                let _ = conn.flush();
            }
            log::info!(
                "Added surface {} to dismiss group of popup surface {}",
                group_surface.id().protocol_id(),
                popup_surface.id().protocol_id()
            );
        } else {
            log::warn!(
                "Layer surface dismiss manager not available - compositor may not support this protocol"
            );
        }
    }

    /// Remove a surface from the dismiss group of another surface
    pub fn remove_from_dismiss_group(
        &mut self,
        popup_surface: &WlSurface,
        group_surface: &WlSurface,
    ) {
        if let Some(controller) = self.get_or_create_dismiss_controller(popup_surface) {
            controller.remove_from_group(group_surface);
            if let Some(ref conn) = self.connection {
                let _ = conn.flush();
            }
            log::info!(
                "Removed surface {} from dismiss group of popup surface {}",
                group_surface.id().protocol_id(),
                popup_surface.id().protocol_id()
            );
        } else {
            log::warn!(
                "Layer surface dismiss manager not available - compositor may not support this protocol"
            );
        }
    }

    /// Check if dismiss was requested and clear the flag
    /// Returns true if a dismiss was requested since the last check.
    pub fn take_dismiss_requested(&mut self) -> bool {
        std::mem::take(&mut self.dismiss_requested)
    }
}

pub trait ZwpTextInputV3Ext {
    fn set_content_type_by_purpose(&self, purpose: ImePurpose);
}

impl ZwpTextInputV3Ext for ZwpTextInputV3 {
    fn set_content_type_by_purpose(&self, purpose: ImePurpose) {
        let (hint, purpose) = match purpose {
            ImePurpose::Normal => (ContentHint::None, ContentPurpose::Normal),
            ImePurpose::Password => (ContentHint::SensitiveData, ContentPurpose::Password),
            ImePurpose::Terminal => (ContentHint::None, ContentPurpose::Terminal),
        };
        self.set_content_type(hint, purpose);
    }
}

impl WindowWrapper {
    #[inline]
    pub fn raw_window_handle_rwh_06(&self) -> Result<rwh_06::RawWindowHandle, rwh_06::HandleError> {
        Ok(rwh_06::WaylandWindowHandle::new({
            let ptr = self.wl_surface.id().as_ptr();
            std::ptr::NonNull::new(ptr as *mut _).expect("wl_surface will never be null")
        })
        .into())
    }

    #[inline]
    pub fn raw_display_handle_rwh_06(
        &self,
    ) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
        Ok(rwh_06::WaylandDisplayHandle::new({
            let ptr = self.display.id().as_ptr();
            std::ptr::NonNull::new(ptr as *mut _).expect("wl_proxy should never be null")
        })
        .into())
    }
}
impl rwh_06::HasWindowHandle for WindowWrapper {
    fn window_handle(&self) -> Result<rwh_06::WindowHandle<'_>, rwh_06::HandleError> {
        let raw = self.raw_window_handle_rwh_06()?;

        // SAFETY: The window handle will never be deallocated while the window is alive,
        // and the main thread safety requirements are upheld internally by each platform.
        Ok(unsafe { rwh_06::WindowHandle::borrow_raw(raw) })
    }
}

impl rwh_06::HasDisplayHandle for WindowWrapper {
    fn display_handle(&self) -> Result<rwh_06::DisplayHandle<'_>, rwh_06::HandleError> {
        let raw = self.raw_display_handle_rwh_06()?;

        // SAFETY: The window handle will never be deallocated while the window is alive,
        // and the main thread safety requirements are upheld internally by each platform.
        Ok(unsafe { rwh_06::DisplayHandle::borrow_raw(raw) })
    }
}

/// Apply blur effect to a surface using the KDE blur protocol
fn apply_blur_to_surface<T: 'static>(
    blur_manager: &Option<blur::org_kde_kwin_blur_manager::OrgKdeKwinBlurManager>,
    surface: &WlSurface,
    qh: &QueueHandle<WindowState<T>>,
) {
    if let Some(manager) = blur_manager {
        let blur_data = blur::BlurData {
            surface: surface.clone(),
        };
        let blur_obj = manager.create(surface, qh, blur_data);
        // Set region to null (entire surface)
        blur_obj.set_region(None);
        // Commit the blur effect
        blur_obj.commit();
        log::info!("Applied blur effect to layer shell surface");
    }
}

/// Apply corner radius to a surface using the layer corner radius protocol
/// Returns the corner radius surface object so it can be stored for later updates
fn apply_corner_radius_to_surface<T: 'static>(
    corner_radius_manager: &Option<
        corner_radius::layer_corner_radius_manager_v1::LayerCornerRadiusManagerV1,
    >,
    corner_radius_values: Option<[u32; 4]>,
    surface: &WlSurface,
    qh: &QueueHandle<WindowState<T>>,
) -> Option<corner_radius::layer_corner_radius_surface_v1::LayerCornerRadiusSurfaceV1> {
    if let (Some(manager), Some(radii)) = (corner_radius_manager, corner_radius_values) {
        let corner_data = corner_radius::CornerRadiusData {
            surface: surface.clone(),
        };
        let corner_obj = manager.get_corner_radius(surface, qh, corner_data);
        corner_obj.set_radius(radii[0], radii[1], radii[2], radii[3]);
        log::info!("Applied corner radius to layer shell surface: {:?}", radii);
        Some(corner_obj)
    } else {
        None
    }
}

/// Apply shadow to a surface using the layer shadow protocol
fn apply_shadow_to_surface<T: 'static>(
    shadow_manager: &Option<shadow::layer_shadow_manager_v1::LayerShadowManagerV1>,
    surface: &WlSurface,
    qh: &QueueHandle<WindowState<T>>,
) {
    log::debug!(
        "apply_shadow_to_surface called, manager present: {}",
        shadow_manager.is_some()
    );
    if let Some(manager) = shadow_manager {
        let shadow_data = shadow::ShadowData {
            surface: surface.clone(),
        };
        let shadow_obj = manager.get_shadow(surface, qh, shadow_data);
        shadow_obj.enable();
        log::info!(
            "Applied shadow effect to layer shell surface (surface_id: {})",
            surface.id().protocol_id()
        );
    } else {
        log::warn!("Cannot apply shadow: shadow_manager is None");
    }
}

/// Apply home visibility mode to a surface using the home visibility protocol
/// Returns the visibility controller so it can be stored for later mode changes
fn apply_home_visibility_to_surface<T: 'static>(
    home_visibility_manager: &Option<
        home_visibility::zcosmic_home_visibility_manager_v1::ZcosmicHomeVisibilityManagerV1,
    >,
    surface: &WlSurface,
    qh: &QueueHandle<WindowState<T>>,
    mode: home_visibility::VisibilityMode,
) -> Option<home_visibility::zcosmic_home_visibility_v1::ZcosmicHomeVisibilityV1> {
    if let Some(manager) = home_visibility_manager {
        let visibility_data = home_visibility::HomeVisibilityData {
            surface: surface.clone(),
        };
        let visibility_obj = manager.get_home_visibility(surface, qh, visibility_data);
        // Set the requested visibility mode
        visibility_obj.set_visibility_mode(mode);
        log::info!("Applied {:?} visibility to layer shell surface", mode);
        Some(visibility_obj)
    } else {
        None
    }
}

/// Register a surface for voice mode events using the voice mode protocol
/// Returns the voice mode receiver so it can be stored and destroyed later
fn register_voice_mode_for_surface<T: 'static>(
    voice_mode_manager: &Option<
        voice_mode::zcosmic_voice_mode_manager_v1::ZcosmicVoiceModeManagerV1,
    >,
    surface: &WlSurface,
    qh: &QueueHandle<WindowState<T>>,
    is_default: bool,
) -> Option<voice_mode::zcosmic_voice_mode_v1::ZcosmicVoiceModeV1> {
    if let Some(manager) = voice_mode_manager {
        let receiver_data = voice_mode::VoiceModeReceiverData {
            surface: surface.clone(),
            is_default,
        };
        // is_default is passed as uint (1 for default, 0 for window-specific)
        let is_default_uint = if is_default { 1 } else { 0 };
        let receiver = manager.get_voice_mode(surface, is_default_uint, qh, receiver_data);
        if is_default {
            log::info!("Registered surface as default voice mode receiver");
        } else {
            log::info!("Registered surface for voice mode events");
        }
        Some(receiver)
    } else {
        None
    }
}

impl<T> WindowState<T> {
    /// create a WindowState, you need to pass a namespace in
    pub fn new(namespace: &str) -> Self {
        assert_ne!(namespace, "");
        Self {
            namespace: namespace.to_owned(),
            ..Default::default()
        }
    }

    /// suggest to bind to specific output
    /// if there is no such output , it will bind the output which now is focused,
    /// same with when binded_output_name is None
    pub fn with_xdg_output_name(mut self, binded_output_name: String) -> Self {
        self.start_mode = StartMode::TargetScreen(binded_output_name);
        self
    }

    pub fn with_start_mode(mut self, mode: StartMode) -> Self {
        self.start_mode = mode;
        self
    }

    pub fn with_events_transparent(mut self, transparent: bool) -> Self {
        self.events_transparent = transparent;
        self
    }

    /// Request blur effect for surfaces (requires compositor support for org_kde_kwin_blur)
    pub fn with_blur(mut self, blur: bool) -> Self {
        self.blur = blur;
        self
    }

    /// Set corner radius for surfaces (requires compositor support for layer_corner_radius_manager_v1)
    /// Radii are specified as [top_left, top_right, bottom_right, bottom_left]
    pub fn with_corner_radius(mut self, radii: [u32; 4]) -> Self {
        self.corner_radius = Some(radii);
        self
    }

    /// Request shadow effect for surfaces (requires compositor support for layer_shadow_manager_v1)
    pub fn with_shadow(mut self, shadow: bool) -> Self {
        self.shadow = shadow;
        self
    }

    /// Set home-only visibility mode for surfaces (requires compositor support for zcosmic_home_visibility_v1)
    /// When enabled, the surface will only be visible when the compositor is in "home" mode
    /// (no regular windows visible, like the iOS home screen).
    pub fn with_home_only(mut self, home_only: bool) -> Self {
        self.home_only = home_only;
        self
    }

    /// Set hide-on-home visibility mode for surfaces (requires compositor support for zcosmic_home_visibility_v1)
    /// When enabled, the surface will be hidden when the compositor is in "home" mode
    /// (inverse of home_only - visible when windows are present, hidden at home screen).
    pub fn with_hide_on_home(mut self, hide_on_home: bool) -> Self {
        self.hide_on_home = hide_on_home;
        self
    }

    /// Enable foreign toplevel tracking (requires compositor support for zwlr_foreign_toplevel_manager_v1)
    /// When enabled, events will be sent for all opened windows (toplevels) on the system.
    /// Useful for creating taskbars or docks that need to show running applications.
    /// This method is only available when the `foreign-toplevel` feature is enabled.
    #[cfg(feature = "foreign-toplevel")]
    pub fn with_foreign_toplevel(mut self, enabled: bool) -> Self {
        self.foreign_toplevel_enabled = enabled;
        self
    }

    /// Enable voice mode protocol support (requires compositor support for zcosmic_voice_mode_v1)
    /// When enabled, the surface will receive voice mode events from the compositor.
    /// The surface is automatically registered as a voice mode receiver with the compositor.
    pub fn with_voice_mode(mut self, enabled: bool) -> Self {
        self.voice_mode_enabled = enabled;
        self
    }

    /// if the shell is a single one, only display on one screen,
    /// fi true, the layer will binding to current screen
    pub fn with_active(mut self) -> Self {
        self.start_mode = StartMode::Active;
        self
    }

    pub fn with_active_or_xdg_output_name(self, binded_output_name: Option<String>) -> Self {
        match binded_output_name {
            Some(binded_output_name) => self.with_xdg_output_name(binded_output_name),
            None => self.with_active(),
        }
    }

    pub fn with_allscreens_or_xdg_output_name(self, binded_output_name: Option<String>) -> Self {
        match binded_output_name {
            Some(binded_output_name) => self.with_xdg_output_name(binded_output_name),
            None => self.with_allscreens(),
        }
    }
    pub fn with_xdg_output_name_or_not(self, binded_output_name: Option<String>) -> Self {
        let Some(binded_output_name) = binded_output_name else {
            return self;
        };
        self.with_xdg_output_name(binded_output_name)
    }

    pub fn with_allscreens_or_active(mut self, allscreen: bool) -> Self {
        if allscreen {
            self.start_mode = StartMode::AllScreens;
        } else {
            self.start_mode = StartMode::Active;
        }
        self
    }

    pub fn with_allscreens(mut self) -> Self {
        self.start_mode = StartMode::AllScreens;
        self
    }

    pub fn with_background_or_not(self, background_mode: bool) -> Self {
        if !background_mode {
            return self;
        }
        self.with_background()
    }

    pub fn with_background(mut self) -> Self {
        self.start_mode = StartMode::Background;
        self
    }

    /// keyboard_interacivity, please take look at [layer_shell](https://wayland.app/protocols/wlr-layer-shell-unstable-v1)
    pub fn with_keyboard_interacivity(
        mut self,
        keyboard_interacivity: zwlr_layer_surface_v1::KeyboardInteractivity,
    ) -> Self {
        self.keyboard_interactivity = keyboard_interacivity;
        self
    }

    /// set the layer_shell anchor
    pub fn with_anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = anchor;
        self
    }

    /// set the layer_shell layer
    pub fn with_layer(mut self, layer: Layer) -> Self {
        self.layer = layer;
        self
    }

    /// set the layer margin
    pub fn with_margin(mut self, (top, right, bottom, left): (i32, i32, i32, i32)) -> Self {
        self.margin = Some((top, right, bottom, left));
        self
    }

    /// if not set, it will be the size suggested by layer_shell, like anchor to four ways,
    /// and margins to 0,0,0,0 , the size will be the size of screen.
    ///
    /// if set, layer_shell will use the size you set
    pub fn with_size(mut self, size: (u32, u32)) -> Self {
        self.size = Some(size);
        self
    }

    /// set the window size, optional
    pub fn with_option_size(mut self, size: Option<(u32, u32)>) -> Self {
        self.size = size;
        self
    }

    /// exclusive_zone, please take look at [layer_shell](https://wayland.app/protocols/wlr-layer-shell-unstable-v1)
    pub fn with_exclusive_zone(mut self, exclusive_zone: i32) -> Self {
        self.exclusive_zone = Some(exclusive_zone);
        self
    }

    /// set layershellev to use display_handle
    pub fn with_use_display_handle(mut self, use_display_handle: bool) -> Self {
        self.use_display_handle = use_display_handle;
        self
    }

    /// set a callback to create a wayland connection
    pub fn with_connection(mut self, connection_or: Option<Connection>) -> Self {
        self.connection = connection_or;
        self
    }
}

impl<T> Default for WindowState<T> {
    fn default() -> Self {
        Self {
            outputs: Vec::new(),
            current_surface: None,
            active_surfaces: HashMap::new(),
            units: Vec::new(),
            message: Vec::new(),

            background_surface: None,
            display: None,

            connection: None,
            event_queue: None,
            wl_compositor: None,
            shm: None,
            wmbase: None,
            cursor_manager: None,
            viewporter: None,
            xdg_output_manager: None,
            globals: None,
            fractional_scale_manager: None,
            virtual_keyboard: None,

            seat: None,
            keyboard_state: None,
            pointer: None,
            touch: None,

            namespace: "".to_owned(),
            keyboard_interactivity: zwlr_layer_surface_v1::KeyboardInteractivity::OnDemand,
            layer: Layer::Overlay,
            anchor: Anchor::Top | Anchor::Left | Anchor::Right | Anchor::Bottom,
            size: None,
            exclusive_zone: None,
            margin: None,

            use_display_handle: false,
            repeat_delay: None,
            to_remove_tokens: Vec::new(),
            to_be_released_key: None,
            closed_ids: Vec::new(),

            last_wloutput: None,
            last_unit_index: 0,

            return_data: Vec::new(),
            finger_locations: HashMap::new(),
            enter_serial: None,
            // NOTE: if is some, means it is to be binded, but not now it
            // is not binded
            xdg_info_cache: Vec::new(),

            start_mode: StartMode::Active,
            init_finished: false,
            events_transparent: false,
            blur: false,
            blur_manager: None,
            corner_radius: None,
            corner_radius_manager: None,
            corner_radius_surfaces: HashMap::new(),
            shadow: false,
            shadow_manager: None,
            auto_hide_manager: None,
            auto_hide_surfaces: HashMap::new(),
            home_only: false,
            hide_on_home: false,
            home_visibility_manager: None,
            home_visibility_controllers: HashMap::new(),
            is_home: false,

            voice_mode_enabled: false,
            voice_mode_manager: None,
            voice_mode_receivers: HashMap::new(),
            voice_mode_events: Vec::new(),

            layer_surface_visibility_manager: None,
            layer_surface_visibility_controllers: HashMap::new(),

            layer_surface_dismiss_manager: None,
            layer_surface_dismiss_controllers: HashMap::new(),
            dismiss_requested: false,

            #[cfg(feature = "foreign-toplevel")]
            foreign_toplevel_enabled: false,
            #[cfg(feature = "foreign-toplevel")]
            ext_foreign_toplevel_list: None,
            #[cfg(feature = "cosmic-toplevel")]
            cosmic_toplevel_info: None,
            #[cfg(feature = "cosmic-toplevel")]
            cosmic_toplevel_manager: None,
            #[cfg(feature = "cosmic-toplevel")]
            cosmic_to_ext_handle_map: HashMap::new(),
            #[cfg(feature = "cosmic-toplevel")]
            cosmic_toplevel_handles: HashMap::new(),
            #[cfg(feature = "foreign-toplevel")]
            foreign_toplevel_manager: None,
            #[cfg(feature = "foreign-toplevel")]
            foreign_toplevel_data: HashMap::new(),
            #[cfg(feature = "foreign-toplevel")]
            foreign_toplevel_handles: HashMap::new(),

            text_input_manager: None,
            text_input: None,
            text_inputs: Vec::new(),
            ime_purpose: ImePurpose::Normal,
            ime_allowed: false,

            xdg_decoration_manager: None,
        }
    }
}

impl<T> WindowState<T> {
    /// You can save the virtual_keyboard here
    pub fn set_virtual_keyboard(&mut self, keyboard: ZwpVirtualKeyboardV1) {
        self.virtual_keyboard = Some(keyboard);
    }

    /// get the saved virtual_keyboard
    pub fn get_virtual_keyboard(&self) -> Option<&ZwpVirtualKeyboardV1> {
        self.virtual_keyboard.as_ref()
    }

    pub fn set_virtual_key_release(&mut self, key_info: VirtualKeyRelease) {
        self.to_be_released_key = Some(key_info);
    }

    /// use [id::Id] to get the mut [WindowStateUnit]
    fn get_mut_unit_with_id(&mut self, id: id::Id) -> Option<&mut WindowStateUnit<T>> {
        self.units.iter_mut().find(|unit| unit.id == id)
    }

    /// use [id::Id] to get the immutable [WindowStateUnit]
    pub fn get_unit_with_id(&self, id: id::Id) -> Option<&WindowStateUnit<T>> {
        self.units.iter().find(|unit| unit.id == id)
    }

    /// it return the iter of units. you can do loop with it
    pub fn get_unit_iter(&self) -> impl Iterator<Item = &WindowStateUnit<T>> {
        self.units.iter()
    }

    fn surface_pos(&self) -> Option<usize> {
        self.units
            .iter()
            .position(|unit| Some(&unit.wl_surface) == self.current_surface.as_ref())
    }

    /// get the current focused surface id
    pub fn current_surface_id(&self) -> Option<id::Id> {
        self.units
            .iter()
            .find(|unit| Some(&unit.wl_surface) == self.current_surface.as_ref())
            .map(|unit| unit.id())
    }

    fn get_id_from_surface(&self, surface: &WlSurface) -> Option<id::Id> {
        self.units
            .iter()
            .find(|unit| &unit.wl_surface == surface)
            .map(|unit| unit.id())
    }

    pub fn is_mouse_surface(&self, surface_id: id::Id) -> bool {
        self.active_surfaces
            .get(&None)
            .filter(|(_, id)| *id == Some(surface_id))
            .is_some()
    }

    /// update `current_surface` only if a finger is down or a mouse button is clicked or a surface
    /// is created.
    fn update_current_surface(&mut self, surface: Option<WlSurface>) {
        if surface == self.current_surface {
            return;
        }
        if let Some(surface) = surface {
            self.current_surface = Some(surface);

            // reset repeat when surface is changed
            if let Some(keyboard_state) = self.keyboard_state.as_mut() {
                keyboard_state.current_repeat = None;
            }

            let unit = self
                .units
                .iter()
                .find(|unit| Some(&unit.wl_surface) == self.current_surface.as_ref());
            if let Some(unit) = unit {
                self.message
                    .push((Some(unit.id), DispatchMessageInner::Focused(unit.id)));
                self.last_unit_index = self
                    .outputs
                    .iter()
                    .position(|(_, output)| Some(output) == unit.wl_output.as_ref())
                    .unwrap_or(0);
            }
        }
    }

    pub fn request_refresh_all(&mut self, request: RefreshRequest) {
        self.units
            .iter_mut()
            .for_each(|unit| unit.request_refresh(request));
    }

    pub fn request_refresh(&mut self, id: id::Id, request: RefreshRequest) {
        if let Some(unit) = self.get_mut_unit_with_id(id) {
            unit.request_refresh(request);
        }
    }

    /// Flush pending requests to the Wayland compositor.
    /// This ensures that all pending protocol requests are sent immediately.
    pub fn flush(&self) {
        if let Some(ref conn) = self.connection {
            let _ = conn.flush();
        }
    }

    pub fn request_close(&mut self, id: id::Id) {
        self.get_mut_unit_with_id(id)
            .map(WindowStateUnit::request_close);
    }

    pub fn get_binding_mut(&mut self, id: id::Id) -> Option<&mut T> {
        self.get_mut_unit_with_id(id)
            .and_then(WindowStateUnit::get_binding_mut)
    }
}

impl<T: 'static> Dispatch<wl_registry::WlRegistry, ()> for WindowState<T> {
    fn event(
        state: &mut Self,
        proxy: &wl_registry::WlRegistry,
        event: <wl_registry::WlRegistry as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        qh: &wayland_client::QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => {
                if interface == wl_output::WlOutput::interface().name {
                    let output = proxy.bind::<wl_output::WlOutput, _, _>(name, version, qh, ());
                    state.outputs.push((name, output.clone()));
                    state
                        .message
                        .push((None, DispatchMessageInner::NewDisplay(output)));
                }
            }
            wl_registry::Event::GlobalRemove { name } => {
                if state
                    .last_wloutput
                    .as_ref()
                    .is_some_and(|output| !output.is_alive())
                {
                    state.last_wloutput.take();
                }
                state.outputs.retain(|x| x.0 != name);
                let removed_states = state
                    .units
                    .extract_if(.., |unit| !unit.wl_surface.is_alive());
                for deleled in removed_states.into_iter() {
                    state.closed_ids.push(deleled.id);
                }
            }

            _ => {}
        }
    }
}

impl<T: 'static> Dispatch<wl_seat::WlSeat, ()> for WindowState<T> {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: <wl_seat::WlSeat as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        qh: &wayland_client::QueueHandle<Self>,
    ) {
        use xkb_keyboard::KeyboardState;
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        {
            let mut keyboard_installing = true;
            if capabilities.contains(wl_seat::Capability::Keyboard) {
                if state.keyboard_state.is_none() {
                    state.keyboard_state = Some(KeyboardState::new(seat.get_keyboard(qh, ())));
                } else {
                    keyboard_installing = false;
                    let keyboard = state.keyboard_state.take().unwrap();
                    state.keyboard_state = Some(keyboard.update(seat, qh, ()));
                    if let Some(surface_id) = state.current_surface_id() {
                        state
                            .message
                            .push((Some(surface_id), DispatchMessageInner::Unfocus));
                    }
                }
            }
            if capabilities.contains(wl_seat::Capability::Pointer) {
                if state.pointer.is_none() {
                    state.pointer = Some(seat.get_pointer(qh, ()));
                } else {
                    let pointer = state.pointer.take().unwrap();
                    if pointer.version() >= 3 {
                        pointer.release();
                    }
                }
            }
            if capabilities.contains(wl_seat::Capability::Touch) {
                if state.touch.is_none() {
                    state.touch = Some(seat.get_touch(qh, ()));
                } else {
                    let touch = state.touch.take().unwrap();
                    if touch.version() >= 3 {
                        touch.release();
                    }
                }
            }
            if keyboard_installing {
                let text_input = state
                    .text_input_manager
                    .as_ref()
                    .map(|manager| manager.get_text_input(seat, qh, TextInputData::default()));
                state.text_input = text_input;
            } else if let Some(text_input) = state.text_input.take() {
                text_input.destroy();
            }
        }
    }
}

impl<T> Dispatch<wl_keyboard::WlKeyboard, ()> for WindowState<T> {
    fn event(
        state: &mut Self,
        _wl_keyboard: &wl_keyboard::WlKeyboard,
        event: <wl_keyboard::WlKeyboard as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        if state.keyboard_state.is_none() {
            return;
        }

        use keyboard::*;
        use xkb_keyboard::ElementState;
        let surface_id = state.current_surface_id();

        match event {
            wl_keyboard::Event::Keymap { format, fd, size } => match format {
                WEnum::Value(KeymapFormat::XkbV1) => {
                    let keyboard_state = state.keyboard_state.as_mut().unwrap();
                    let context = &mut keyboard_state.xkb_context;
                    context.set_keymap_from_fd(fd, size as usize)
                }
                WEnum::Value(KeymapFormat::NoKeymap) => {
                    log::warn!("non-xkb compatible keymap")
                }
                _ => unreachable!(),
            },
            wl_keyboard::Event::Enter { surface, .. } => {
                log::info!("wl_keyboard::Enter event - keyboard focus entered surface");
                state.update_current_surface(Some(surface));
                let keyboard_state = state.keyboard_state.as_mut().unwrap();
                if let Some(token) = keyboard_state.repeat_token.take() {
                    state.to_remove_tokens.push(token);
                }
            }
            wl_keyboard::Event::Leave { .. } => {
                log::info!(
                    "wl_keyboard::Leave event - emitting Unfocus for surface {:?}",
                    surface_id
                );
                let keyboard_state = state.keyboard_state.as_mut().unwrap();
                keyboard_state.current_repeat = None;
                state.message.push((
                    surface_id,
                    DispatchMessageInner::ModifiersChanged(ModifiersState::empty()),
                ));
                state
                    .message
                    .push((surface_id, DispatchMessageInner::Unfocus));

                if let Some(token) = keyboard_state.repeat_token.take() {
                    state.to_remove_tokens.push(token);
                }
            }
            wl_keyboard::Event::Key {
                state: keystate,
                key,
                ..
            } => {
                let pressed_state = match keystate {
                    WEnum::Value(KeyState::Pressed) => ElementState::Pressed,
                    WEnum::Value(KeyState::Released) => ElementState::Released,
                    _ => {
                        return;
                    }
                };
                let keyboard_state = state.keyboard_state.as_mut().unwrap();
                let key = key + 8;
                if let Some(mut key_context) = keyboard_state.xkb_context.key_context() {
                    let event = key_context.process_key_event(key, pressed_state, false);
                    let event = DispatchMessageInner::KeyboardInput {
                        event,
                        is_synthetic: false,
                    };
                    state.message.push((surface_id, event));
                }

                match pressed_state {
                    ElementState::Pressed => {
                        let delay = match keyboard_state.repeat_info {
                            RepeatInfo::Repeat { delay, .. } => delay,
                            RepeatInfo::Disable => return,
                        };

                        if keyboard_state
                            .xkb_context
                            .keymap_mut()
                            .is_none_or(|keymap| !keymap.key_repeats(key))
                        {
                            return;
                        }

                        keyboard_state.current_repeat = Some(key);

                        if let Some(token) = keyboard_state.repeat_token.take() {
                            state.to_remove_tokens.push(token);
                        }
                        state.repeat_delay = Some(KeyboardTokenState {
                            delay,
                            key,
                            surface_id,
                            pressed_state,
                        });
                    }
                    ElementState::Released => {
                        if keyboard_state.repeat_info != RepeatInfo::Disable
                            && keyboard_state
                                .xkb_context
                                .keymap_mut()
                                .is_some_and(|keymap| keymap.key_repeats(key))
                            && Some(key) == keyboard_state.current_repeat
                        {
                            keyboard_state.current_repeat = None;

                            if let Some(token) = keyboard_state.repeat_token.take() {
                                state.to_remove_tokens.push(token);
                            }
                        }
                    }
                }
            }
            wl_keyboard::Event::Modifiers {
                mods_depressed,
                mods_locked,
                mods_latched,
                group,
                ..
            } => {
                let keyboard_state = state.keyboard_state.as_mut().unwrap();
                let xkb_context = &mut keyboard_state.xkb_context;
                let xkb_state = match xkb_context.state_mut() {
                    Some(state) => state,
                    None => return,
                };
                xkb_state.update_modifiers(mods_depressed, mods_latched, mods_locked, 0, 0, group);
                let modifiers = xkb_state.modifiers();

                state.message.push((
                    state.current_surface_id(),
                    DispatchMessageInner::ModifiersChanged(modifiers.into()),
                ))
            }
            wl_keyboard::Event::RepeatInfo { rate, delay } => {
                let keyboard_state = state.keyboard_state.as_mut().unwrap();
                keyboard_state.repeat_info = if rate == 0 {
                    // Stop the repeat once we get a disable event.
                    keyboard_state.current_repeat = None;

                    if let Some(token) = keyboard_state.repeat_token.take() {
                        state.to_remove_tokens.push(token);
                    }
                    RepeatInfo::Disable
                } else {
                    let gap = Duration::from_micros(1_000_000 / rate as u64);
                    let delay = Duration::from_millis(delay as u64);
                    RepeatInfo::Repeat { gap, delay }
                };
            }
            _ => {}
        }
    }
}

impl<T> Dispatch<wl_touch::WlTouch, ()> for WindowState<T> {
    fn event(
        state: &mut Self,
        _proxy: &wl_touch::WlTouch,
        event: <wl_touch::WlTouch as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wl_touch::Event::Down {
                serial,
                time,
                surface,
                id,
                x,
                y,
            } => {
                state.finger_locations.insert(id, (x, y));
                let surface_id = state.get_id_from_surface(&surface);
                state
                    .active_surfaces
                    .insert(Some(id), (surface.clone(), surface_id));
                state.update_current_surface(Some(surface));
                state.message.push((
                    surface_id,
                    DispatchMessageInner::TouchDown {
                        serial,
                        time,
                        id,
                        x,
                        y,
                    },
                ))
            }
            wl_touch::Event::Cancel => {
                let mut mouse_surface = None;
                for (k, v) in state.active_surfaces.drain() {
                    if let Some(id) = k {
                        let (x, y) = state.finger_locations.remove(&id).unwrap_or_default();
                        state
                            .message
                            .push((v.1, DispatchMessageInner::TouchCancel { id, x, y }));
                    } else {
                        // keep the surface of mouse.
                        mouse_surface = Some(v);
                    }
                }
                if let Some(mouse_surface) = mouse_surface {
                    state.active_surfaces.insert(None, mouse_surface);
                }
            }
            wl_touch::Event::Up { serial, time, id } => {
                let surface_id = state
                    .active_surfaces
                    .remove(&Some(id))
                    .or_else(|| {
                        log::warn!("finger[{id}] hasn't been down.");
                        None
                    })
                    .and_then(|(_, id)| id);
                let (x, y) = state.finger_locations.remove(&id).unwrap_or_default();
                state.message.push((
                    surface_id,
                    DispatchMessageInner::TouchUp {
                        serial,
                        time,
                        id,
                        x,
                        y,
                    },
                ));
            }
            wl_touch::Event::Motion { time, id, x, y } => {
                let surface_id = state
                    .active_surfaces
                    .get(&Some(id))
                    .or_else(|| {
                        log::warn!("finger[{id}] hasn't been down.");
                        None
                    })
                    .and_then(|(_, id)| *id);
                state.finger_locations.insert(id, (x, y));
                state.message.push((
                    surface_id,
                    DispatchMessageInner::TouchMotion { time, id, x, y },
                ));
            }
            _ => {}
        }
    }
}

impl<T> Dispatch<wl_pointer::WlPointer, ()> for WindowState<T> {
    fn event(
        state: &mut Self,
        pointer: &wl_pointer::WlPointer,
        event: <wl_pointer::WlPointer as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
        // All mouse events should be happened on the surface which is hovered by the mouse.
        let (mouse_surface, surface_id) = state
            .active_surfaces
            .get(&None)
            .map(|(surface, id)| (Some(surface), *id))
            .unwrap_or_else(|| {
                match &event {
                    wl_pointer::Event::Enter { .. } => {}
                    _ => {
                        log::warn!("mouse hasn't entered.");
                    }
                }
                (None, None)
            });
        let scale = surface_id
            .and_then(|id| state.get_unit_with_id(id))
            .map(|unit| unit.scale_float())
            .unwrap_or(1.0);
        match event {
            wl_pointer::Event::Axis { time, axis, value } => match axis {
                WEnum::Value(axis) => {
                    let (mut horizontal, mut vertical) = <(AxisScroll, AxisScroll)>::default();
                    match axis {
                        wl_pointer::Axis::VerticalScroll => {
                            vertical.absolute = value;
                        }
                        wl_pointer::Axis::HorizontalScroll => {
                            horizontal.absolute = value;
                        }
                        _ => unreachable!(),
                    };

                    state.message.push((
                        surface_id,
                        DispatchMessageInner::Axis {
                            time,
                            scale,
                            horizontal,
                            vertical,
                            source: None,
                        },
                    ))
                }
                WEnum::Unknown(unknown) => {
                    log::warn!(target: "layershellev", "{}: invalid pointer axis: {:x}", pointer.id(), unknown);
                }
            },
            wl_pointer::Event::AxisStop { time, axis } => match axis {
                WEnum::Value(axis) => {
                    let (mut horizontal, mut vertical) = <(AxisScroll, AxisScroll)>::default();
                    match axis {
                        wl_pointer::Axis::VerticalScroll => vertical.stop = true,
                        wl_pointer::Axis::HorizontalScroll => horizontal.stop = true,

                        _ => unreachable!(),
                    }

                    state.message.push((
                        surface_id,
                        DispatchMessageInner::Axis {
                            time,
                            scale,
                            horizontal,
                            vertical,
                            source: None,
                        },
                    ));
                }

                WEnum::Unknown(unknown) => {
                    log::warn!(target: "layershellev", "{}: invalid pointer axis: {:x}", pointer.id(), unknown);
                }
            },
            wl_pointer::Event::AxisSource { axis_source } => match axis_source {
                WEnum::Value(source) => state.message.push((
                    surface_id,
                    DispatchMessageInner::Axis {
                        horizontal: AxisScroll::default(),
                        vertical: AxisScroll::default(),
                        scale,
                        source: Some(source),
                        time: 0,
                    },
                )),
                WEnum::Unknown(unknown) => {
                    log::warn!(target: "layershellev", "unknown pointer axis source: {unknown:x}");
                }
            },
            wl_pointer::Event::AxisDiscrete { axis, discrete } => match axis {
                WEnum::Value(axis) => {
                    let (mut horizontal, mut vertical) = <(AxisScroll, AxisScroll)>::default();
                    match axis {
                        wl_pointer::Axis::VerticalScroll => {
                            vertical.discrete = discrete;
                        }

                        wl_pointer::Axis::HorizontalScroll => {
                            horizontal.discrete = discrete;
                        }

                        _ => unreachable!(),
                    };

                    state.message.push((
                        surface_id,
                        DispatchMessageInner::Axis {
                            time: 0,
                            scale,
                            horizontal,
                            vertical,
                            source: None,
                        },
                    ));
                }

                WEnum::Unknown(unknown) => {
                    log::warn!(target: "layershellev", "{}: invalid pointer axis: {:x}", pointer.id(), unknown);
                }
            },
            wl_pointer::Event::Button {
                state: btnstate,
                serial,
                button,
                time,
            } => {
                let mouse_surface = mouse_surface.cloned();
                state.update_current_surface(mouse_surface);
                state.message.push((
                    surface_id,
                    DispatchMessageInner::MouseButton {
                        state: btnstate,
                        serial,
                        button,
                        time,
                    },
                ));
            }
            wl_pointer::Event::Leave { .. } => {
                let surface_id = state
                    .active_surfaces
                    .remove(&None)
                    .or_else(|| {
                        log::warn!("mouse hasn't entered.");
                        None
                    })
                    .and_then(|(_, id)| id);
                state
                    .message
                    .push((surface_id, DispatchMessageInner::MouseLeave));
            }
            wl_pointer::Event::Enter {
                serial,
                surface,
                surface_x,
                surface_y,
            } => {
                let surface_id = state.get_id_from_surface(&surface);
                state
                    .active_surfaces
                    .insert(None, (surface.clone(), surface_id));
                state.enter_serial = Some(serial);
                state.message.push((
                    surface_id,
                    DispatchMessageInner::MouseEnter {
                        pointer: pointer.clone(),
                        serial,
                        surface_x,
                        surface_y,
                    },
                ));
            }
            wl_pointer::Event::Motion {
                time,
                surface_x,
                surface_y,
            } => {
                state.message.push((
                    surface_id,
                    DispatchMessageInner::MouseMotion {
                        time,
                        surface_x,
                        surface_y,
                    },
                ));
            }
            _ => {
                // TODO: not now
            }
        }
    }
}

impl<T> Dispatch<xdg_surface::XdgSurface, ()> for WindowState<T> {
    fn event(
        state: &mut Self,
        surface: &xdg_surface::XdgSurface,
        event: <xdg_surface::XdgSurface as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            surface.ack_configure(serial);
            state
                .units
                .iter_mut()
                .filter(|unit| unit.shell == *surface)
                .for_each(|unit| unit.request_refresh(RefreshRequest::NextFrame));
        }
    }
}

impl<T> Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for WindowState<T> {
    fn event(
        state: &mut Self,
        surface: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: <zwlr_layer_surface_v1::ZwlrLayerSurfaceV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
        let unit_index = state.units.iter().position(|unit| unit.shell == *surface);
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                surface.ack_configure(serial);

                let Some(unit_index) = unit_index else {
                    return;
                };
                state.units[unit_index].size = (width, height);

                state.units[unit_index].request_refresh(RefreshRequest::NextFrame);
            }
            zwlr_layer_surface_v1::Event::Closed => {
                if let Some(i) = unit_index {
                    state.units[i].request_close();
                }
            }
            _ => log::info!("ignore zwlr_layer_surface_v1 event: {event:?}"),
        }
    }
}

impl<T> Dispatch<xdg_toplevel::XdgToplevel, ()> for WindowState<T> {
    fn event(
        state: &mut Self,
        surface: &xdg_toplevel::XdgToplevel,
        event: <xdg_toplevel::XdgToplevel as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let unit_index = state.units.iter().position(|unit| unit.shell == *surface);
        match event {
            xdg_toplevel::Event::Configure { width, height, .. } => {
                let Some(unit_index) = unit_index else {
                    return;
                };
                if width != 0 && height != 0 {
                    state.units[unit_index].size = (width as u32, height as u32);
                }

                state.units[unit_index].request_refresh(RefreshRequest::NextFrame);
            }
            xdg_toplevel::Event::Close => {
                let Some(unit_index) = unit_index else {
                    return;
                };
                state.units[unit_index].request_flag.close = true;
            }
            _ => {}
        }
    }
}

impl<T> Dispatch<xdg_popup::XdgPopup, ()> for WindowState<T> {
    fn event(
        state: &mut Self,
        surface: &xdg_popup::XdgPopup,
        event: <xdg_popup::XdgPopup as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        if let xdg_popup::Event::Configure { width, height, .. } = event {
            let Some(unit_index) = state.units.iter().position(|unit| unit.shell == *surface)
            else {
                return;
            };
            state.units[unit_index].size = (width as u32, height as u32);

            state.units[unit_index].request_refresh(RefreshRequest::NextFrame)
        }
    }
}

impl<T> Dispatch<zxdg_output_v1::ZxdgOutputV1, ()> for WindowState<T> {
    fn event(
        state: &mut Self,
        proxy: &zxdg_output_v1::ZxdgOutputV1,
        event: <zxdg_output_v1::ZxdgOutputV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        if state.is_with_target() && !state.init_finished {
            let Some((_, xdg_info)) = state
                .xdg_info_cache
                .iter_mut()
                .find(|(_, info)| info.zxdgoutput == *proxy)
            else {
                return;
            };
            match event {
                zxdg_output_v1::Event::LogicalSize { width, height } => {
                    xdg_info.logical_size = (width, height);
                }
                zxdg_output_v1::Event::LogicalPosition { x, y } => {
                    xdg_info.position = (x, y);
                }
                zxdg_output_v1::Event::Name { name } => {
                    xdg_info.name = name;
                }
                zxdg_output_v1::Event::Description { description } => {
                    xdg_info.description = description;
                }
                _ => {}
            };
            return;
        }
        let Some(index) = state.units.iter().position(|info| {
            info.zxdgoutput
                .as_ref()
                .is_some_and(|zxdgoutput| zxdgoutput.zxdgoutput == *proxy)
        }) else {
            return;
        };
        let info = &mut state.units[index];
        let xdg_info = info.zxdgoutput.as_mut().unwrap();
        let change_type = match event {
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                xdg_info.logical_size = (width, height);
                XdgInfoChangedType::Size
            }
            zxdg_output_v1::Event::LogicalPosition { x, y } => {
                xdg_info.position = (x, y);
                XdgInfoChangedType::Position
            }
            zxdg_output_v1::Event::Name { name } => {
                xdg_info.name = name;
                XdgInfoChangedType::Name
            }
            zxdg_output_v1::Event::Description { description } => {
                xdg_info.description = description;
                XdgInfoChangedType::Description
            }
            _ => {
                return;
            }
        };
        state.message.push((
            Some(state.units[index].id),
            DispatchMessageInner::XdgInfoChanged(change_type),
        ));
    }
}

impl<T> Dispatch<wp_fractional_scale_v1::WpFractionalScaleV1, ()> for WindowState<T> {
    fn event(
        state: &mut Self,
        proxy: &wp_fractional_scale_v1::WpFractionalScaleV1,
        event: <wp_fractional_scale_v1::WpFractionalScaleV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            let Some(unit) = state.units.iter_mut().find(|info| {
                info.fractional_scale
                    .as_ref()
                    .is_some_and(|fractional_scale| fractional_scale == proxy)
            }) else {
                return;
            };
            unit.scale = scale;
            unit.request_refresh(RefreshRequest::NextFrame);
            state.message.push((
                Some(unit.id),
                DispatchMessageInner::PreferredScale {
                    scale_u32: scale,
                    scale_float: scale as f64 / 120.,
                },
            ));
        }
    }
}

#[derive(Default)]
pub struct TextInputData {
    inner: std::sync::Mutex<TextInputDataInner>,
}

#[derive(Default)]
pub struct TextInputDataInner {
    /// The `WlSurface` we're performing input to.
    surface: Option<WlSurface>,

    /// The commit to submit on `done`.
    pending_commit: Option<String>,

    /// The preedit to submit on `done`.
    pending_preedit: Option<Preedit>,
}
/// The state of the preedit.
struct Preedit {
    text: String,
    cursor_begin: Option<usize>,
    cursor_end: Option<usize>,
}

impl<T> Dispatch<zwp_text_input_v3::ZwpTextInputV3, TextInputData> for WindowState<T> {
    fn event(
        state: &mut Self,
        text_input: &zwp_text_input_v3::ZwpTextInputV3,
        event: <zwp_text_input_v3::ZwpTextInputV3 as Proxy>::Event,
        data: &TextInputData,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        use zwp_text_input_v3::Event;
        let mut text_input_data = data.inner.lock().unwrap();

        match event {
            Event::Enter { surface } => {
                let Some(id) = state.get_id_from_surface(&surface) else {
                    return;
                };
                text_input_data.surface = Some(surface);

                if state.ime_allowed() {
                    text_input.enable();
                    text_input.set_content_type_by_purpose(state.ime_purpose());
                    text_input.commit();
                    state
                        .message
                        .push((Some(id), DispatchMessageInner::Ime(events::Ime::Enabled)));
                }
                state.text_input_entered(text_input);
            }
            Event::Leave { surface } => {
                text_input_data.surface = None;

                text_input.disable();
                text_input.commit();
                let Some(id) = state.get_id_from_surface(&surface) else {
                    return;
                };
                state.text_input_left(text_input);
                state
                    .message
                    .push((Some(id), DispatchMessageInner::Ime(events::Ime::Disabled)));
            }
            Event::CommitString { text } => {
                text_input_data.pending_preedit = None;
                text_input_data.pending_commit = text;
            }
            Event::DeleteSurroundingText { .. } => {}
            Event::Done { .. } => {
                let Some(id) = text_input_data
                    .surface
                    .as_ref()
                    .and_then(|surface| state.get_id_from_surface(surface))
                else {
                    return;
                };
                // Clear preedit, unless all we'll be doing next is sending a new preedit.
                if text_input_data.pending_commit.is_some()
                    || text_input_data.pending_preedit.is_none()
                {
                    state.message.push((
                        Some(id),
                        DispatchMessageInner::Ime(Ime::Preedit(String::new(), None)),
                    ));
                }

                // Send `Commit`.
                if let Some(text) = text_input_data.pending_commit.take() {
                    state
                        .message
                        .push((Some(id), DispatchMessageInner::Ime(Ime::Commit(text))));
                }

                // Send preedit.
                if let Some(preedit) = text_input_data.pending_preedit.take() {
                    let cursor_range = preedit
                        .cursor_begin
                        .map(|b| (b, preedit.cursor_end.unwrap_or(b)));

                    state.message.push((
                        Some(id),
                        DispatchMessageInner::Ime(Ime::Preedit(preedit.text, cursor_range)),
                    ));
                }
            }
            Event::PreeditString {
                text,
                cursor_begin,
                cursor_end,
            } => {
                let text = text.unwrap_or_default();
                let cursor_begin = usize::try_from(cursor_begin)
                    .ok()
                    .and_then(|idx| text.is_char_boundary(idx).then_some(idx));
                let cursor_end = usize::try_from(cursor_end)
                    .ok()
                    .and_then(|idx| text.is_char_boundary(idx).then_some(idx));

                text_input_data.pending_preedit = Some(Preedit {
                    text,
                    cursor_begin,
                    cursor_end,
                })
            }

            _ => {}
        }
    }
}

impl<T> Dispatch<WlCallback, (id::Id, PresentAvailableState)> for WindowState<T> {
    fn event(
        state: &mut Self,
        _proxy: &WlCallback,
        event: <WlCallback as Proxy>::Event,
        data: &(id::Id, PresentAvailableState),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        if let WlCallbackEvent::Done { callback_data: _ } = event
            && let Some(unit) = state.get_mut_unit_with_id(data.0)
        {
            unit.present_available_state = data.1;
        }
    }
}

delegate_noop!(@<T> WindowState<T>: ignore WlCompositor); // WlCompositor is need to create a surface
delegate_noop!(@<T> WindowState<T>: ignore WlSurface); // surface is the base needed to show buffer
delegate_noop!(@<T> WindowState<T>: ignore WlOutput); // output is need to place layer_shell, although here
// it is not used
delegate_noop!(@<T> WindowState<T>: ignore WlShm); // shm is used to create buffer pool
delegate_noop!(@<T> WindowState<T>: ignore WlShmPool); // so it is pool, created by wl_shm
delegate_noop!(@<T> WindowState<T>: ignore WlBuffer); // buffer show the picture
delegate_noop!(@<T> WindowState<T>: ignore WlRegion); // region is used to modify input region
delegate_noop!(@<T> WindowState<T>: ignore ZwlrLayerShellV1); // it is similar with xdg_toplevel, also the
// ext-session-shell

delegate_noop!(@<T> WindowState<T>: ignore WpCursorShapeManagerV1);
delegate_noop!(@<T> WindowState<T>: ignore WpCursorShapeDeviceV1);

delegate_noop!(@<T> WindowState<T>: ignore WpViewporter);
delegate_noop!(@<T> WindowState<T>: ignore WpViewport);

delegate_noop!(@<T> WindowState<T>: ignore ZwpVirtualKeyboardV1);
delegate_noop!(@<T> WindowState<T>: ignore ZwpVirtualKeyboardManagerV1);

delegate_noop!(@<T> WindowState<T>: ignore ZxdgOutputManagerV1);
delegate_noop!(@<T> WindowState<T>: ignore WpFractionalScaleManagerV1);
delegate_noop!(@<T> WindowState<T>: ignore XdgPositioner);
delegate_noop!(@<T> WindowState<T>: ignore XdgWmBase);

delegate_noop!(@<T> WindowState<T>: ignore ZwpTextInputManagerV3);
delegate_noop!(@<T> WindowState<T>: ignore ZwpInputPanelSurfaceV1);
delegate_noop!(@<T> WindowState<T>: ignore ZwpInputPanelV1);

delegate_noop!(@<T> WindowState<T>: ignore ZxdgDecorationManagerV1);
delegate_noop!(@<T> WindowState<T>: ignore ZxdgToplevelDecorationV1);

// Blur protocol delegates
delegate_noop!(@<T> WindowState<T>: ignore blur::org_kde_kwin_blur_manager::OrgKdeKwinBlurManager);

// Manual Dispatch impl for blur object since it has custom user data
impl<T: 'static> Dispatch<blur::org_kde_kwin_blur::OrgKdeKwinBlur, blur::BlurData>
    for WindowState<T>
{
    fn event(
        _state: &mut Self,
        _proxy: &blur::org_kde_kwin_blur::OrgKdeKwinBlur,
        _event: <blur::org_kde_kwin_blur::OrgKdeKwinBlur as Proxy>::Event,
        _data: &blur::BlurData,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        // No events for blur objects
    }
}

// Corner radius protocol delegates
delegate_noop!(@<T> WindowState<T>: ignore corner_radius::layer_corner_radius_manager_v1::LayerCornerRadiusManagerV1);

// Manual Dispatch impl for corner radius surface object since it has custom user data
impl<T: 'static>
    Dispatch<
        corner_radius::layer_corner_radius_surface_v1::LayerCornerRadiusSurfaceV1,
        corner_radius::CornerRadiusData,
    > for WindowState<T>
{
    fn event(
        _state: &mut Self,
        _proxy: &corner_radius::layer_corner_radius_surface_v1::LayerCornerRadiusSurfaceV1,
        _event: <corner_radius::layer_corner_radius_surface_v1::LayerCornerRadiusSurfaceV1 as Proxy>::Event,
        _data: &corner_radius::CornerRadiusData,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        // No events for corner radius objects
    }
}

// Shadow protocol delegates
delegate_noop!(@<T> WindowState<T>: ignore shadow::layer_shadow_manager_v1::LayerShadowManagerV1);

// Manual Dispatch impl for shadow surface object since it has custom user data
impl<T: 'static> Dispatch<shadow::layer_shadow_surface_v1::LayerShadowSurfaceV1, shadow::ShadowData>
    for WindowState<T>
{
    fn event(
        _state: &mut Self,
        _proxy: &shadow::layer_shadow_surface_v1::LayerShadowSurfaceV1,
        _event: <shadow::layer_shadow_surface_v1::LayerShadowSurfaceV1 as Proxy>::Event,
        _data: &shadow::ShadowData,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        // No events for shadow objects
    }
}

// Auto-hide protocol delegates
delegate_noop!(@<T> WindowState<T>: ignore layer_auto_hide::layer_auto_hide_manager_v1::LayerAutoHideManagerV1);

// Manual Dispatch impl for auto-hide surface object to handle visibility_changed events
impl<T: 'static>
    Dispatch<
        layer_auto_hide::layer_auto_hide_v1::LayerAutoHideV1,
        layer_auto_hide::LayerAutoHideData,
    > for WindowState<T>
{
    fn event(
        state: &mut Self,
        _proxy: &layer_auto_hide::layer_auto_hide_v1::LayerAutoHideV1,
        event: <layer_auto_hide::layer_auto_hide_v1::LayerAutoHideV1 as Proxy>::Event,
        _data: &layer_auto_hide::LayerAutoHideData,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        use layer_auto_hide::layer_auto_hide_v1::Event;
        match event {
            Event::VisibilityChanged { visible } => {
                let is_visible = visible != 0;
                log::debug!("Auto-hide visibility changed: visible={}", is_visible);
                state.message.push((
                    None,
                    DispatchMessageInner::AutoHideVisibilityChanged(is_visible),
                ));
            }
        }
    }
}

// Home visibility protocol delegates
// Manager has the home_state event, so we need a proper Dispatch impl
impl<T: 'static>
    Dispatch<
        home_visibility::zcosmic_home_visibility_manager_v1::ZcosmicHomeVisibilityManagerV1,
        home_visibility::HomeVisibilityManagerData,
    > for WindowState<T>
{
    fn event(
        state: &mut Self,
        _proxy: &home_visibility::zcosmic_home_visibility_manager_v1::ZcosmicHomeVisibilityManagerV1,
        event: <home_visibility::zcosmic_home_visibility_manager_v1::ZcosmicHomeVisibilityManagerV1 as Proxy>::Event,
        _data: &home_visibility::HomeVisibilityManagerData,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        use home_visibility::zcosmic_home_visibility_manager_v1::Event;
        let Event::HomeState { is_home } = event;
        let is_home = is_home != 0;
        log::debug!("Home state changed: is_home={}", is_home);
        state.is_home = is_home;
        // Add a message to propagate the event
        state
            .message
            .push((None, DispatchMessageInner::HomeStateChanged(is_home)));
    }
}

// Manual Dispatch impl for home visibility controller since it has custom user data
impl<T: 'static>
    Dispatch<
        home_visibility::zcosmic_home_visibility_v1::ZcosmicHomeVisibilityV1,
        home_visibility::HomeVisibilityData,
    > for WindowState<T>
{
    fn event(
        _state: &mut Self,
        _proxy: &home_visibility::zcosmic_home_visibility_v1::ZcosmicHomeVisibilityV1,
        _event: <home_visibility::zcosmic_home_visibility_v1::ZcosmicHomeVisibilityV1 as Proxy>::Event,
        _data: &home_visibility::HomeVisibilityData,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        // No events for visibility controller objects
    }
}

// Layer surface visibility protocol delegates
impl<T: 'static>
    Dispatch<
        layer_surface_visibility::zcosmic_layer_surface_visibility_manager_v1::ZcosmicLayerSurfaceVisibilityManagerV1,
        layer_surface_visibility::LayerSurfaceVisibilityManagerData,
    > for WindowState<T>
{
    fn event(
        _state: &mut Self,
        _proxy: &layer_surface_visibility::zcosmic_layer_surface_visibility_manager_v1::ZcosmicLayerSurfaceVisibilityManagerV1,
        _event: <layer_surface_visibility::zcosmic_layer_surface_visibility_manager_v1::ZcosmicLayerSurfaceVisibilityManagerV1 as Proxy>::Event,
        _data: &layer_surface_visibility::LayerSurfaceVisibilityManagerData,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        // No events for manager
    }
}

impl<T: 'static>
    Dispatch<
        layer_surface_visibility::zcosmic_layer_surface_visibility_v1::ZcosmicLayerSurfaceVisibilityV1,
        layer_surface_visibility::LayerSurfaceVisibilityData,
    > for WindowState<T>
{
    fn event(
        _state: &mut Self,
        _proxy: &layer_surface_visibility::zcosmic_layer_surface_visibility_v1::ZcosmicLayerSurfaceVisibilityV1,
        event: <layer_surface_visibility::zcosmic_layer_surface_visibility_v1::ZcosmicLayerSurfaceVisibilityV1 as Proxy>::Event,
        data: &layer_surface_visibility::LayerSurfaceVisibilityData,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        use layer_surface_visibility::zcosmic_layer_surface_visibility_v1::Event;
        let Event::VisibilityChanged { visible } = event;
        let visible = visible != 0;
        log::debug!(
            "Layer surface visibility changed: surface={:?}, visible={}",
            data.surface.id().protocol_id(),
            visible
        );
        // We just log the event - the client controls visibility, not the compositor
    }
}

// Layer surface dismiss protocol delegates
impl<T: 'static>
    Dispatch<
        layer_surface_dismiss::zcosmic_layer_surface_dismiss_manager_v1::ZcosmicLayerSurfaceDismissManagerV1,
        layer_surface_dismiss::LayerSurfaceDismissManagerData,
    > for WindowState<T>
{
    fn event(
        _state: &mut Self,
        _proxy: &layer_surface_dismiss::zcosmic_layer_surface_dismiss_manager_v1::ZcosmicLayerSurfaceDismissManagerV1,
        _event: <layer_surface_dismiss::zcosmic_layer_surface_dismiss_manager_v1::ZcosmicLayerSurfaceDismissManagerV1 as Proxy>::Event,
        _data: &layer_surface_dismiss::LayerSurfaceDismissManagerData,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        // No events for manager
    }
}

impl<T: 'static>
    Dispatch<
        layer_surface_dismiss::zcosmic_layer_surface_dismiss_v1::ZcosmicLayerSurfaceDismissV1,
        layer_surface_dismiss::LayerSurfaceDismissData,
    > for WindowState<T>
{
    fn event(
        state: &mut Self,
        _proxy: &layer_surface_dismiss::zcosmic_layer_surface_dismiss_v1::ZcosmicLayerSurfaceDismissV1,
        event: <layer_surface_dismiss::zcosmic_layer_surface_dismiss_v1::ZcosmicLayerSurfaceDismissV1 as Proxy>::Event,
        data: &layer_surface_dismiss::LayerSurfaceDismissData,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        use layer_surface_dismiss::zcosmic_layer_surface_dismiss_v1::Event;
        let Event::DismissRequested = event;
        log::debug!(
            "Dismiss requested for surface {:?}",
            data.surface.id().protocol_id()
        );
        state.dismiss_requested = true;
        state
            .message
            .push((None, DispatchMessageInner::DismissRequested));
    }
}

// Voice mode protocol delegates
impl<T: 'static>
    Dispatch<
        voice_mode::zcosmic_voice_mode_manager_v1::ZcosmicVoiceModeManagerV1,
        voice_mode::VoiceModeManagerData,
    > for WindowState<T>
{
    fn event(
        _state: &mut Self,
        _proxy: &voice_mode::zcosmic_voice_mode_manager_v1::ZcosmicVoiceModeManagerV1,
        _event: <voice_mode::zcosmic_voice_mode_manager_v1::ZcosmicVoiceModeManagerV1 as Proxy>::Event,
        _data: &voice_mode::VoiceModeManagerData,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        // No events for manager
    }
}

impl<T: 'static>
    Dispatch<
        voice_mode::zcosmic_voice_mode_v1::ZcosmicVoiceModeV1,
        voice_mode::VoiceModeReceiverData,
    > for WindowState<T>
{
    fn event(
        state: &mut Self,
        _proxy: &voice_mode::zcosmic_voice_mode_v1::ZcosmicVoiceModeV1,
        event: <voice_mode::zcosmic_voice_mode_v1::ZcosmicVoiceModeV1 as Proxy>::Event,
        data: &voice_mode::VoiceModeReceiverData,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        use voice_mode::zcosmic_voice_mode_v1::{Event, OrbState};
        use wayland_client::WEnum;

        log::debug!(
            "Voice mode receiver event: {:?}, is_default: {}",
            event,
            data.is_default
        );

        let voice_event = match event {
            Event::Start { orb_state } => {
                let orb_state = match orb_state {
                    WEnum::Value(s) => s,
                    WEnum::Unknown(v) => {
                        log::warn!("Unknown orb state value: {}", v);
                        OrbState::Hidden
                    }
                };
                // Set voice active immediately so will_stop can respond correctly
                // Don't wait for iced's event loop to process the message
                voice_mode::set_voice_active(true);
                log::info!(
                    "Voice mode started, orb_state: {:?}, voice_active set to true",
                    orb_state
                );
                voice_mode::VoiceModeEvent::Started { orb_state }
            }
            Event::Stop => {
                // Clear voice active on stop
                voice_mode::set_voice_active(false);
                log::info!("Voice mode stopped, voice_active set to false");
                voice_mode::VoiceModeEvent::Stopped
            }
            Event::Cancel => {
                // Clear voice active on cancel
                voice_mode::set_voice_active(false);
                log::info!("Voice mode cancelled, voice_active set to false");
                voice_mode::VoiceModeEvent::Cancelled
            }
            Event::OrbAttached {
                x,
                y,
                width,
                height,
            } => {
                log::debug!(
                    "Voice orb attached: x={}, y={}, width={}, height={}",
                    x,
                    y,
                    width,
                    height
                );
                voice_mode::VoiceModeEvent::OrbAttached {
                    x,
                    y,
                    width,
                    height,
                }
            }
            Event::OrbDetached => {
                log::debug!("Voice orb detached");
                voice_mode::VoiceModeEvent::OrbDetached
            }
            Event::WillStop { serial } => {
                // Immediately respond with cached voice active state
                // This avoids round-trip through iced's event loop
                let freeze = voice_mode::is_voice_active();
                log::info!(
                    "Voice mode will_stop, serial: {}, auto-responding with freeze: {}",
                    serial,
                    freeze
                );
                _proxy.ack_stop(serial, if freeze { 1 } else { 0 });
                voice_mode::VoiceModeEvent::WillStop { serial }
            }
            Event::FocusInput => {
                log::info!("Voice mode focus_input (tap detected)");
                voice_mode::VoiceModeEvent::FocusInput
            }
        };

        // Store the event for later processing
        state.voice_mode_events.push(voice_event.clone());
        // Also push to message queue
        state
            .message
            .push((None, DispatchMessageInner::VoiceMode(voice_event)));
    }
}

// Layer surface dismiss protocol implementation
#[allow(private_interfaces)]
impl<T: 'static> layer_surface_dismiss::LayerSurfaceDismissHandler for WindowState<T> {
    fn dismiss_requested(&mut self, _surface: &WlSurface) {
        log::debug!("Dismiss requested for surface");
        self.dismiss_requested = true;
        self.message
            .push((None, DispatchMessageInner::DismissRequested));
    }
}

// Foreign toplevel protocol implementation
#[cfg(feature = "foreign-toplevel")]
#[allow(private_interfaces)]
impl<T: 'static> foreign_toplevel::ForeignToplevelHandler for WindowState<T> {
    fn foreign_toplevel_event(&mut self, event: foreign_toplevel::ForeignToplevelEvent) {
        log::trace!("Queuing foreign toplevel event: {:?}", event);
        self.message
            .push((None, DispatchMessageInner::ForeignToplevel(event)));
    }

    fn get_toplevel_data(&mut self, id: u32) -> &mut foreign_toplevel::ToplevelHandleData {
        self.foreign_toplevel_data
            .entry(id)
            .or_insert_with(foreign_toplevel::ToplevelHandleData::default)
    }

    fn remove_toplevel_data(&mut self, id: u32) {
        self.foreign_toplevel_data.remove(&id);
    }

    fn store_toplevel_handle(
        &mut self,
        id: u32,
        handle: wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1,
    ) {
        self.foreign_toplevel_handles.insert(id, handle);
    }

    fn remove_toplevel_handle(&mut self, id: u32) {
        self.foreign_toplevel_handles.remove(&id);
    }

    fn get_toplevel_handle(&self, id: u32) -> Option<&wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1>{
        self.foreign_toplevel_handles.get(&id)
    }

    #[cfg(feature = "cosmic-toplevel")]
    fn store_cosmic_toplevel_handle(
        &mut self,
        id: u32,
        handle: cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1,
    ) {
        log::debug!("Storing COSMIC toplevel handle for id {}", id);
        self.cosmic_toplevel_handles.insert(id, handle);
    }

    #[cfg(feature = "cosmic-toplevel")]
    fn remove_cosmic_toplevel_handle(&mut self, id: u32) {
        self.cosmic_toplevel_handles.remove(&id);
    }

    #[cfg(feature = "cosmic-toplevel")]
    fn get_cosmic_toplevel_handle(&self, id: u32) -> Option<&cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1>{
        self.cosmic_toplevel_handles.get(&id)
    }

    #[cfg(feature = "cosmic-toplevel")]
    fn get_cosmic_toplevel_manager(&self) -> Option<&cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1>{
        self.cosmic_toplevel_manager.as_ref()
    }

    #[cfg(feature = "cosmic-toplevel")]
    fn cosmic_toplevel_info(&self) -> Option<&cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1>{
        self.cosmic_toplevel_info.as_ref()
    }
}

// COSMIC toplevel handler implementation
#[cfg(feature = "cosmic-toplevel")]
#[allow(private_interfaces)]
impl<T: 'static> foreign_toplevel::CosmicToplevelHandler for WindowState<T> {
    fn cosmic_toplevel_info(&self) -> Option<&cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1>{
        self.cosmic_toplevel_info.as_ref()
    }

    fn set_cosmic_handle_mapping(&mut self, cosmic_id: u32, ext_id: u32) {
        self.cosmic_to_ext_handle_map.insert(cosmic_id, ext_id);
    }

    fn get_ext_handle_id(&self, cosmic_id: u32) -> Option<u32> {
        self.cosmic_to_ext_handle_map.get(&cosmic_id).copied()
    }
}

// Foreign toplevel manager dispatch
#[cfg(feature = "foreign-toplevel")]
impl<T: 'static>
    Dispatch<
        wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1,
        foreign_toplevel::ForeignToplevelManagerData,
    > for WindowState<T>
{
    fn event(
        state: &mut Self,
        proxy: &wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1,
        event: wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::Event,
        data: &foreign_toplevel::ForeignToplevelManagerData,
        conn: &Connection,
        qhandle: &QueueHandle<Self>,
    ) {
        <() as Dispatch<
            wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1,
            foreign_toplevel::ForeignToplevelManagerData,
            Self,
        >>::event(state, proxy, event, data, conn, qhandle)
    }

    fn event_created_child(
        opcode: u16,
        qhandle: &QueueHandle<Self>,
    ) -> std::sync::Arc<dyn wayland_client::backend::ObjectData> {
        <() as Dispatch<
            wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1,
            foreign_toplevel::ForeignToplevelManagerData,
            Self,
        >>::event_created_child(opcode, qhandle)
    }
}

// Foreign toplevel handle dispatch
#[cfg(feature = "foreign-toplevel")]
impl<T: 'static>
    Dispatch<
        wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1,
        foreign_toplevel::ToplevelHandleUserData,
    > for WindowState<T>
{
    fn event(
        state: &mut Self,
        proxy: &wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1,
        event: wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::Event,
        data: &foreign_toplevel::ToplevelHandleUserData,
        conn: &Connection,
        qhandle: &QueueHandle<Self>,
    ) {
        <() as Dispatch<
            wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1,
            foreign_toplevel::ToplevelHandleUserData,
            Self,
        >>::event(state, proxy, event, data, conn, qhandle)
    }
}

// ext_foreign_toplevel_list_v1 dispatch
#[cfg(feature = "foreign-toplevel")]
impl<T: 'static>
    Dispatch<
        wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1,
        foreign_toplevel::ExtForeignToplevelListData,
    > for WindowState<T>
{
    fn event(
        state: &mut Self,
        proxy: &wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1,
        event: wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_list_v1::Event,
        data: &foreign_toplevel::ExtForeignToplevelListData,
        conn: &Connection,
        qhandle: &QueueHandle<Self>,
    ) {
        <() as Dispatch<
            wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1,
            foreign_toplevel::ExtForeignToplevelListData,
            Self,
        >>::event(state, proxy, event, data, conn, qhandle)
    }

    fn event_created_child(
        opcode: u16,
        qhandle: &QueueHandle<Self>,
    ) -> std::sync::Arc<dyn wayland_client::backend::ObjectData> {
        <() as Dispatch<
            wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1,
            foreign_toplevel::ExtForeignToplevelListData,
            Self,
        >>::event_created_child(opcode, qhandle)
    }
}

// ext_foreign_toplevel_handle_v1 dispatch
#[cfg(feature = "foreign-toplevel")]
impl<T: 'static>
    Dispatch<
        wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
        foreign_toplevel::ExtToplevelHandleData,
    > for WindowState<T>
{
    fn event(
        state: &mut Self,
        proxy: &wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
        event: wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::Event,
        data: &foreign_toplevel::ExtToplevelHandleData,
        conn: &Connection,
        qhandle: &QueueHandle<Self>,
    ) {
        use foreign_toplevel::ForeignToplevelHandler;
        use wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::Event;

        // Check if this is a Done event for a new (uninitialized) handle
        // If so, request the COSMIC extension for state info
        #[cfg(feature = "cosmic-toplevel")]
        if let Event::Done = &event {
            let ext_id = proxy.id().protocol_id();
            let handle_data = state.get_toplevel_data(ext_id);
            if !handle_data.initialized {
                // First Done event - request cosmic extension if available
                if let Some(cosmic_info) = state.cosmic_toplevel_info.as_ref() {
                    log::trace!("Requesting cosmic toplevel handle for ext handle {}", ext_id);
                    let cosmic_handle_data = foreign_toplevel::CosmicToplevelHandleData {
                        ext_handle_id: ext_id,
                    };
                    cosmic_info.get_cosmic_toplevel(proxy, qhandle, cosmic_handle_data);
                }
            }
        }

        // Forward to blanket impl
        <() as Dispatch<
            wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
            foreign_toplevel::ExtToplevelHandleData,
            Self,
        >>::event(state, proxy, event, data, conn, qhandle)
    }
}

// zcosmic_toplevel_info_v1 dispatch
#[cfg(feature = "cosmic-toplevel")]
impl<T: 'static>
    Dispatch<
        cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1,
        foreign_toplevel::CosmicToplevelInfoData,
    > for WindowState<T>
{
    fn event(
        state: &mut Self,
        proxy: &cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1,
        event: cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_info_v1::Event,
        data: &foreign_toplevel::CosmicToplevelInfoData,
        conn: &Connection,
        qhandle: &QueueHandle<Self>,
    ) {
        <() as Dispatch<
            cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1,
            foreign_toplevel::CosmicToplevelInfoData,
            Self,
        >>::event(state, proxy, event, data, conn, qhandle)
    }
}

// zcosmic_toplevel_handle_v1 dispatch
#[cfg(feature = "cosmic-toplevel")]
impl<T: 'static>
    Dispatch<
        cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1,
        foreign_toplevel::CosmicToplevelHandleData,
    > for WindowState<T>
{
    fn event(
        state: &mut Self,
        proxy: &cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1,
        event: cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1::Event,
        data: &foreign_toplevel::CosmicToplevelHandleData,
        conn: &Connection,
        qhandle: &QueueHandle<Self>,
    ) {
        <() as Dispatch<
            cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1,
            foreign_toplevel::CosmicToplevelHandleData,
            Self,
        >>::event(state, proxy, event, data, conn, qhandle)
    }
}

// zcosmic_toplevel_manager_v1 dispatch (for control operations)
#[cfg(feature = "cosmic-toplevel")]
impl<T: 'static>
    Dispatch<
        cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1,
        foreign_toplevel::CosmicToplevelManagerData,
    > for WindowState<T>
{
    fn event(
        state: &mut Self,
        proxy: &cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1,
        event: cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1::Event,
        data: &foreign_toplevel::CosmicToplevelManagerData,
        conn: &Connection,
        qhandle: &QueueHandle<Self>,
    ) {
        <() as Dispatch<
            cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1,
            foreign_toplevel::CosmicToplevelManagerData,
            Self,
        >>::event(state, proxy, event, data, conn, qhandle)
    }
}

impl<T: 'static> WindowState<T> {
    /// build a new WindowState
    pub fn build(mut self) -> Result<Self, LayerEventError> {
        let connection = if let Some(connection) = self.connection.take() {
            connection
        } else {
            Connection::connect_to_env()?
        };
        let (globals, _) = registry_queue_init::<BaseState>(&connection)?;

        self.display = Some(connection.display());
        let mut event_queue = connection.new_event_queue::<WindowState<T>>();
        let qh = event_queue.handle();

        let wmcompositer = globals.bind::<WlCompositor, _, _>(&qh, 1..=5, ())?;

        let shm = globals.bind::<WlShm, _, _>(&qh, 1..=1, ())?;
        self.shm = Some(shm);
        self.seat = Some(globals.bind::<WlSeat, _, _>(&qh, 1..=1, ())?);

        let wmbase = globals.bind::<XdgWmBase, _, _>(&qh, 2..=6, ())?;
        self.wmbase = Some(wmbase);

        let cursor_manager = globals
            .bind::<WpCursorShapeManagerV1, _, _>(&qh, 1..=1, ())
            .ok();
        let viewporter = globals.bind::<WpViewporter, _, _>(&qh, 1..=1, ()).ok();

        let _ = connection.display().get_registry(&qh, ()); // so if you want WlOutput, you need to
        // register this

        let xdg_output_manager = globals.bind::<ZxdgOutputManagerV1, _, _>(&qh, 1..=3, ())?; // bind
        // xdg_output_manager

        let decoration_manager = globals
            .bind::<ZxdgDecorationManagerV1, _, _>(&qh, 1..=1, ())
            .ok();

        self.xdg_decoration_manager = decoration_manager;

        let fractional_scale_manager = globals
            .bind::<WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, ())
            .ok();
        let text_input_manager = globals
            .bind::<ZwpTextInputManagerV3, _, _>(&qh, 1..=1, ())
            .ok();

        self.text_input_manager = text_input_manager;

        // Bind blur manager if blur is enabled
        if self.blur {
            self.blur_manager = globals
                .bind::<blur::org_kde_kwin_blur_manager::OrgKdeKwinBlurManager, _, _>(
                    &qh,
                    1..=1,
                    (),
                )
                .ok();
            if self.blur_manager.is_none() {
                log::warn!(
                    "Blur requested but compositor does not support org_kde_kwin_blur_manager protocol"
                );
            } else {
                log::info!(
                    "Successfully bound org_kde_kwin_blur_manager protocol for blur support"
                );
            }
        }

        // Always try to bind corner radius manager for dynamic corner radius support
        // (allows setting corner radius at runtime even if not set initially)
        self.corner_radius_manager = globals
            .bind::<corner_radius::layer_corner_radius_manager_v1::LayerCornerRadiusManagerV1, _, _>(
                &qh,
                1..=1,
                (),
            )
            .ok();
        if self.corner_radius_manager.is_some() {
            log::info!(
                "Successfully bound layer_corner_radius_manager_v1 protocol for corner radius support"
            );
        }

        // Always try to bind shadow manager for dynamic shadow support
        // (allows requesting shadow on any surface, like popups, even if main window doesn't have shadow)
        self.shadow_manager = globals
            .bind::<shadow::layer_shadow_manager_v1::LayerShadowManagerV1, _, _>(&qh, 1..=1, ())
            .ok();
        if self.shadow_manager.is_some() {
            log::info!("Successfully bound layer_shadow_manager_v1 protocol for shadow support");
        }

        // Always try to bind layer auto-hide manager for compositor-driven auto-hide support
        self.auto_hide_manager = globals
            .bind::<layer_auto_hide::layer_auto_hide_manager_v1::LayerAutoHideManagerV1, _, _>(
                &qh,
                1..=1,
                (),
            )
            .ok();
        if self.auto_hide_manager.is_some() {
            log::info!(
                "Successfully bound layer_auto_hide_manager_v1 protocol for auto-hide support"
            );
        }

        // Always try to bind layer surface visibility manager for hide/show support
        // (allows hiding/showing surfaces without destroying them)
        self.layer_surface_visibility_manager = globals
            .bind::<layer_surface_visibility::zcosmic_layer_surface_visibility_manager_v1::ZcosmicLayerSurfaceVisibilityManagerV1, _, _>(
                &qh,
                1..=1,
                layer_surface_visibility::LayerSurfaceVisibilityManagerData::default(),
            )
            .ok();
        if self.layer_surface_visibility_manager.is_some() {
            log::info!(
                "Successfully bound zcosmic_layer_surface_visibility_manager_v1 protocol for hide/show support"
            );
        }

        // Always try to bind layer surface dismiss manager for dismiss-on-outside-click support
        self.layer_surface_dismiss_manager = globals
            .bind::<layer_surface_dismiss::zcosmic_layer_surface_dismiss_manager_v1::ZcosmicLayerSurfaceDismissManagerV1, _, _>(
                &qh,
                1..=1,
                layer_surface_dismiss::LayerSurfaceDismissManagerData::default(),
            )
            .ok();
        if self.layer_surface_dismiss_manager.is_some() {
            log::info!(
                "Successfully bound zcosmic_layer_surface_dismiss_manager_v1 protocol for dismiss support"
            );
        }

        // Bind home visibility manager if home_only or hide_on_home is enabled
        if self.home_only || self.hide_on_home {
            self.home_visibility_manager = globals
                .bind::<home_visibility::zcosmic_home_visibility_manager_v1::ZcosmicHomeVisibilityManagerV1, _, _>(
                    &qh,
                    1..=1,
                    home_visibility::HomeVisibilityManagerData::default(),
                )
                .ok();
            if self.home_visibility_manager.is_none() {
                log::warn!(
                    "Home visibility mode requested but compositor does not support zcosmic_home_visibility_v1 protocol"
                );
            } else {
                log::info!(
                    "Successfully bound zcosmic_home_visibility_manager_v1 protocol for home visibility support"
                );
            }
        }

        // Bind voice mode manager if voice mode is enabled
        if self.voice_mode_enabled {
            self.voice_mode_manager = globals
                .bind::<voice_mode::zcosmic_voice_mode_manager_v1::ZcosmicVoiceModeManagerV1, _, _>(
                    &qh,
                    1..=1,
                    voice_mode::VoiceModeManagerData::default(),
                )
                .ok();
            if self.voice_mode_manager.is_none() {
                log::warn!(
                    "Voice mode requested but compositor does not support zcosmic_voice_mode_v1 protocol"
                );
            } else {
                log::info!(
                    "Successfully bound zcosmic_voice_mode_manager_v1 protocol for voice mode support"
                );
            }
        }

        // Bind foreign toplevel protocols if enabled
        // We need zwlr_foreign_toplevel_manager for control operations (activate, close, etc.)
        // ext_foreign_toplevel_list + cosmic_toplevel_info provide better info but no control
        #[cfg(feature = "foreign-toplevel")]
        if self.foreign_toplevel_enabled {
            // Try ext_foreign_toplevel_list_v1 first (standard protocol for info)
            self.ext_foreign_toplevel_list = globals
                .bind::<wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1, _, _>(
                    &qh,
                    1..=1,
                    foreign_toplevel::ExtForeignToplevelListData::default(),
                )
                .ok();

            if self.ext_foreign_toplevel_list.is_some() {
                log::info!(
                    "Successfully bound ext_foreign_toplevel_list_v1 protocol for foreign toplevel tracking"
                );
            }

            // Try to bind COSMIC protocols for state info and control
            #[cfg(feature = "cosmic-toplevel")]
            {
                // COSMIC toplevel info (for state info like minimized/maximized)
                self.cosmic_toplevel_info = globals
                    .bind::<cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1, _, _>(
                        &qh,
                        2..=2,  // Version 2+ has get_cosmic_toplevel
                        foreign_toplevel::CosmicToplevelInfoData::default(),
                    )
                    .ok();
                if self.cosmic_toplevel_info.is_some() {
                    log::info!(
                        "Successfully bound zcosmic_toplevel_info_v1 protocol for toplevel state info"
                    );
                } else {
                    log::debug!(
                        "zcosmic_toplevel_info_v1 not available - state info will be limited"
                    );
                }

                // COSMIC toplevel manager (for control - activate, close, etc.)
                // Version 5+ needed for force_close support
                self.cosmic_toplevel_manager = globals
                    .bind::<cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1, _, _>(
                        &qh,
                        1..=5,
                        foreign_toplevel::CosmicToplevelManagerData::default(),
                    )
                    .ok();
                if self.cosmic_toplevel_manager.is_some() {
                    log::info!(
                        "Successfully bound zcosmic_toplevel_manager_v1 protocol for toplevel control"
                    );
                } else {
                    log::debug!(
                        "zcosmic_toplevel_manager_v1 not available - trying wlr fallback for control"
                    );
                }
            }

            // Fall back to wlr_foreign_toplevel_manager_v1 if COSMIC manager not available
            #[cfg(feature = "cosmic-toplevel")]
            let has_cosmic_manager = self.cosmic_toplevel_manager.is_some();
            #[cfg(not(feature = "cosmic-toplevel"))]
            let has_cosmic_manager = false;

            if !has_cosmic_manager {
                self.foreign_toplevel_manager = globals
                    .bind::<wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1, _, _>(
                        &qh,
                        1..=3,
                        foreign_toplevel::ForeignToplevelManagerData::default(),
                    )
                    .ok();

                if self.foreign_toplevel_manager.is_some() {
                    log::info!(
                        "Successfully bound zwlr_foreign_toplevel_manager_v1 protocol for foreign toplevel management"
                    );
                }
            }

            // Check if we have at least one way to track toplevels
            let has_info =
                self.ext_foreign_toplevel_list.is_some() || self.foreign_toplevel_manager.is_some();
            #[cfg(feature = "cosmic-toplevel")]
            let has_control =
                self.cosmic_toplevel_manager.is_some() || self.foreign_toplevel_manager.is_some();
            #[cfg(not(feature = "cosmic-toplevel"))]
            let has_control = self.foreign_toplevel_manager.is_some();

            if !has_info {
                log::warn!(
                    "Foreign toplevel tracking requested but compositor does not support any toplevel info protocols"
                );
            }
            if !has_control {
                log::warn!(
                    "Foreign toplevel control (activate, close, etc.) not available - no supported protocol found"
                );
            }
        }

        event_queue.blocking_dispatch(&mut self)?; // then make a dispatch

        // do the step before, you get empty list

        // so it is the same way, to get surface detach to protocol, first get the shell, like wmbase
        // or layer_shell or session-shell, then get `surface` from the wl_surface you get before, and
        // set it
        // finally thing to remember is to commit the surface, make the shell to init.
        //let (init_w, init_h) = self.size;
        // this example is ok for both xdg_surface and layer_shell
        if self.is_background() {
            let background_surface = wmcompositer.create_surface(&qh, ());
            if self.events_transparent {
                let region = wmcompositer.create_region(&qh, ());
                background_surface.set_input_region(Some(&region));
                region.destroy();
            }
            self.background_surface = Some(background_surface);
        } else if !self.is_allscreens() {
            let mut output = None;

            let (binded_output, binded_xdginfo) = match self.start_mode.clone() {
                StartMode::TargetScreen(name) => {
                    for (_, output_display) in &self.outputs {
                        let zxdgoutput = xdg_output_manager.get_xdg_output(output_display, &qh, ());
                        self.xdg_info_cache
                            .push((output_display.clone(), ZxdgOutputInfo::new(zxdgoutput)));
                    }
                    event_queue.blocking_dispatch(&mut self)?; // then make a dispatch
                    if let Some(cache) = self
                        .xdg_info_cache
                        .iter()
                        .find(|(_, info)| info.name == *name)
                        .cloned()
                    {
                        output = Some(cache.clone());
                    }
                    self.xdg_info_cache.clear();
                    let binded_output = output.as_ref().map(|(output, _)| output).cloned();
                    let binded_xdginfo = output.as_ref().map(|(_, xdginfo)| xdginfo).cloned();
                    (binded_output, binded_xdginfo)
                }
                StartMode::TargetOutput(output) => (Some(output), None),
                _ => (None, None),
            };

            let wl_surface = wmcompositer.create_surface(&qh, ()); // and create a surface. if two or more,
            let layer_shell = globals
                .bind::<ZwlrLayerShellV1, _, _>(&qh, 3..=4, ())
                .unwrap();
            let layer = layer_shell.get_layer_surface(
                &wl_surface,
                binded_output.as_ref(),
                self.layer,
                self.namespace.clone(),
                &qh,
                (),
            );
            layer.set_anchor(self.anchor);
            layer.set_keyboard_interactivity(self.keyboard_interactivity);
            if let Some((init_w, init_h)) = self.size {
                layer.set_size(init_w, init_h);
            }

            if let Some(zone) = self.exclusive_zone {
                layer.set_exclusive_zone(zone);
            }

            if let Some((top, right, bottom, left)) = self.margin {
                layer.set_margin(top, right, bottom, left);
            }

            if self.events_transparent {
                let region = wmcompositer.create_region(&qh, ());
                wl_surface.set_input_region(Some(&region));
                region.destroy();
            }

            // Apply blur effect if enabled
            if self.blur {
                apply_blur_to_surface(&self.blur_manager, &wl_surface, &qh);
            }

            // Apply corner radius if set
            let surface_id = wl_surface.id().protocol_id();
            if self.corner_radius.is_some() {
                if let Some(corner_obj) = apply_corner_radius_to_surface(
                    &self.corner_radius_manager,
                    self.corner_radius,
                    &wl_surface,
                    &qh,
                ) {
                    self.corner_radius_surfaces.insert(surface_id, corner_obj);
                }
            }

            // Apply shadow if enabled
            if self.shadow {
                apply_shadow_to_surface(&self.shadow_manager, &wl_surface, &qh);
            }

            // Apply home visibility mode if enabled
            if self.home_only {
                if let Some(controller) = apply_home_visibility_to_surface(
                    &self.home_visibility_manager,
                    &wl_surface,
                    &qh,
                    home_visibility::VisibilityMode::HomeOnly,
                ) {
                    self.home_visibility_controllers
                        .insert(surface_id, controller);
                }
            } else if self.hide_on_home {
                if let Some(controller) = apply_home_visibility_to_surface(
                    &self.home_visibility_manager,
                    &wl_surface,
                    &qh,
                    home_visibility::VisibilityMode::HideOnHome,
                ) {
                    self.home_visibility_controllers
                        .insert(surface_id, controller);
                }
            }

            // Register surface for voice mode events if enabled
            if self.voice_mode_enabled {
                // First surface is registered as default receiver
                let is_default = self.voice_mode_receivers.is_empty();
                if let Some(receiver) = register_voice_mode_for_surface(
                    &self.voice_mode_manager,
                    &wl_surface,
                    &qh,
                    is_default,
                ) {
                    self.voice_mode_receivers.insert(surface_id, receiver);
                }
            }

            wl_surface.commit();

            let mut fractional_scale = None;
            if let Some(ref fractional_scale_manager) = fractional_scale_manager {
                fractional_scale =
                    Some(fractional_scale_manager.get_fractional_scale(&wl_surface, &qh, ()));
            }
            let viewport = viewporter
                .as_ref()
                .map(|viewport| viewport.get_viewport(&wl_surface, &qh, ()));
            // so during the init Configure of the shell, a buffer, atleast a buffer is needed.
            // and if you need to reconfigure it, you need to commit the wl_surface again
            // so because this is just an example, so we just commit it once
            // like if you want to reset anchor or KeyboardInteractivity or resize, commit is needed
            self.push_window(
                WindowStateUnitBuilder::new(
                    id::Id::unique(),
                    qh.clone(),
                    connection.display(),
                    wl_surface,
                    Shell::LayerShell(layer),
                )
                .viewport(viewport)
                .zxdgoutput(binded_xdginfo)
                .fractional_scale(fractional_scale)
                .wl_output(binded_output.clone())
                .build(),
            );
        } else {
            let displays = self.outputs.clone();
            for (_, output_display) in displays.iter() {
                let wl_surface = wmcompositer.create_surface(&qh, ()); // and create a surface. if two or more,
                let layer_shell = globals
                    .bind::<ZwlrLayerShellV1, _, _>(&qh, 3..=4, ())
                    .unwrap();
                let layer = layer_shell.get_layer_surface(
                    &wl_surface,
                    Some(output_display),
                    self.layer,
                    self.namespace.clone(),
                    &qh,
                    (),
                );
                layer.set_anchor(self.anchor);
                layer.set_keyboard_interactivity(self.keyboard_interactivity);
                if let Some((init_w, init_h)) = self.size {
                    layer.set_size(init_w, init_h);
                }

                if let Some(zone) = self.exclusive_zone {
                    layer.set_exclusive_zone(zone);
                }

                if let Some((top, right, bottom, left)) = self.margin {
                    layer.set_margin(top, right, bottom, left);
                }

                if self.events_transparent {
                    let region = wmcompositer.create_region(&qh, ());
                    wl_surface.set_input_region(Some(&region));
                    region.destroy();
                }

                // Apply blur effect if enabled
                if self.blur {
                    apply_blur_to_surface(&self.blur_manager, &wl_surface, &qh);
                }

                // Apply corner radius if set
                let surface_id = wl_surface.id().protocol_id();
                if self.corner_radius.is_some() {
                    if let Some(corner_obj) = apply_corner_radius_to_surface(
                        &self.corner_radius_manager,
                        self.corner_radius,
                        &wl_surface,
                        &qh,
                    ) {
                        self.corner_radius_surfaces.insert(surface_id, corner_obj);
                    }
                }

                // Apply shadow if enabled
                if self.shadow {
                    apply_shadow_to_surface(&self.shadow_manager, &wl_surface, &qh);
                }

                // Apply home visibility mode if enabled
                if self.home_only {
                    if let Some(controller) = apply_home_visibility_to_surface(
                        &self.home_visibility_manager,
                        &wl_surface,
                        &qh,
                        home_visibility::VisibilityMode::HomeOnly,
                    ) {
                        self.home_visibility_controllers
                            .insert(surface_id, controller);
                    }
                } else if self.hide_on_home {
                    if let Some(controller) = apply_home_visibility_to_surface(
                        &self.home_visibility_manager,
                        &wl_surface,
                        &qh,
                        home_visibility::VisibilityMode::HideOnHome,
                    ) {
                        self.home_visibility_controllers
                            .insert(surface_id, controller);
                    }
                }

                // Register surface for voice mode events if enabled
                if self.voice_mode_enabled {
                    let is_default = self.voice_mode_receivers.is_empty();
                    if let Some(receiver) = register_voice_mode_for_surface(
                        &self.voice_mode_manager,
                        &wl_surface,
                        &qh,
                        is_default,
                    ) {
                        self.voice_mode_receivers.insert(surface_id, receiver);
                    }
                }

                wl_surface.commit();

                let zxdgoutput = xdg_output_manager.get_xdg_output(output_display, &qh, ());
                let mut fractional_scale = None;
                if let Some(ref fractional_scale_manager) = fractional_scale_manager {
                    fractional_scale =
                        Some(fractional_scale_manager.get_fractional_scale(&wl_surface, &qh, ()));
                }
                let viewport = viewporter
                    .as_ref()
                    .map(|viewport| viewport.get_viewport(&wl_surface, &qh, ()));
                // so during the init Configure of the shell, a buffer, atleast a buffer is needed.
                // and if you need to reconfigure it, you need to commit the wl_surface again
                // so because this is just an example, so we just commit it once
                // like if you want to reset anchor or KeyboardInteractivity or resize, commit is needed

                self.push_window(
                    WindowStateUnitBuilder::new(
                        id::Id::unique(),
                        qh.clone(),
                        connection.display(),
                        wl_surface,
                        Shell::LayerShell(layer),
                    )
                    .viewport(viewport)
                    .zxdgoutput(Some(ZxdgOutputInfo::new(zxdgoutput)))
                    .fractional_scale(fractional_scale)
                    .wl_output(Some(output_display.clone()))
                    .build(),
                );
            }
            self.message.clear();
        }
        self.init_finished = true;
        self.viewporter = viewporter;
        self.event_queue = Some(event_queue);
        self.globals = Some(globals);
        self.wl_compositor = Some(wmcompositer);
        self.fractional_scale_manager = fractional_scale_manager;
        self.cursor_manager = cursor_manager;
        self.xdg_output_manager = Some(xdg_output_manager);
        self.connection = Some(connection);

        Ok(self)
    }
    /// main event loop, every time dispatch, it will store the messages, and do callback. it will
    /// pass a LayerShellEvent, with self as mut, the last `Option<usize>` describe which unit the event
    /// happened on, like tell you this time you do a click, what surface it is on. you can use the
    /// index to get the unit, with [WindowState::get_unit_with_id] if the even is not spical on one surface,
    /// it will return [None].
    /// Different with running, it receiver a receiver
    pub fn running_with_proxy<F, Message>(
        self,
        message_receiver: Channel<Message>,
        event_handler: F,
    ) -> Result<(), LayerEventError>
    where
        Message: std::marker::Send + 'static,
        F: FnMut(LayerShellEvent<T, Message>, &mut WindowState<T>, Option<id::Id>) -> ReturnData<T>
            + 'static,
    {
        self.running_with_proxy_option(Some(message_receiver), event_handler)
    }
    /// main event loop, every time dispatch, it will store the messages, and do callback. it will
    /// pass a LayerShellEvent, with self as mut, the last `Option<usize>` describe which unit the event
    /// happened on, like tell you this time you do a click, what surface it is on. you can use the
    /// index to get the unit, with [WindowState::get_unit_with_id] if the even is not spical on one surface,
    /// it will return [None].
    ///
    pub fn running<F>(self, event_handler: F) -> Result<(), LayerEventError>
    where
        F: FnMut(LayerShellEvent<T, ()>, &mut WindowState<T>, Option<id::Id>) -> ReturnData<T>
            + 'static,
    {
        self.running_with_proxy_option(None, event_handler)
    }

    fn running_with_proxy_option<F, Message>(
        mut self,
        message_receiver: Option<Channel<Message>>,
        mut event_handler: F,
    ) -> Result<(), LayerEventError>
    where
        Message: std::marker::Send + 'static,
        F: FnMut(LayerShellEvent<T, Message>, &mut WindowState<T>, Option<id::Id>) -> ReturnData<T>
            + 'static,
    {
        let globals = self.globals.take().unwrap();
        let mut event_queue_origin = self.event_queue.take().unwrap();
        let qh = event_queue_origin.handle();
        let wmcompositer = self.wl_compositor.take().unwrap();
        let shm = self.shm.take().unwrap();
        let fractional_scale_manager = self.fractional_scale_manager.take();
        let cursor_manager: Option<WpCursorShapeManagerV1> = self.cursor_manager.take();
        let xdg_output_manager = self.xdg_output_manager.take().unwrap();
        let connection = self.connection.take().unwrap();
        let mut init_event = None;
        let wmbase = self.wmbase.take().unwrap();
        let viewporter = self.viewporter.take();
        let zxdg_decoration_manager = self.xdg_decoration_manager.take();

        let cursor_update_context = CursorUpdateContext {
            cursor_manager,
            qh: qh.clone(),
            connection: connection.clone(),
            shm: shm.clone(),
            wmcompositer: wmcompositer.clone(),
        };

        while !matches!(init_event, Some(ReturnData::None)) {
            match init_event {
                None => {
                    init_event = Some(event_handler(LayerShellEvent::InitRequest, &mut self, None));
                }
                Some(ReturnData::RequestBind) => {
                    init_event = Some(event_handler(
                        LayerShellEvent::BindProvide(&globals, &qh),
                        &mut self,
                        None,
                    ));
                }
                Some(ReturnData::RequestCompositor) => {
                    init_event = Some(event_handler(
                        LayerShellEvent::CompositorProvide(&wmcompositer, &qh),
                        &mut self,
                        None,
                    ));
                }
                _ => panic!("Not provide server here"),
            }
        }

        struct EventWrapper<Raw, F> {
            raw: Raw,
            fun: F,
            loop_handle: LoopHandle<'static, Self>,
        }

        let mut event_loop: EventLoop<_> =
            EventLoop::try_new().expect("Failed to initialize the event loop");

        let event_queue = connection.new_event_queue::<EventWrapper<Self, F>>();
        WaylandSource::new(connection.clone(), event_queue)
            .insert(event_loop.handle())
            .expect("Failed to init wayland source");
        let mut state = EventWrapper {
            raw: self,
            fun: event_handler,
            loop_handle: event_loop.handle(),
        };

        let signal = event_loop.get_signal();

        // Insert message channel as event source (calloop-style)
        if let Some(channel) = message_receiver {
            event_loop
                .handle()
                .insert_source(channel, |event, _, r_window_state| {
                    let channel::Event::Msg(event) = event else {
                        return;
                    };
                    let window_state = &mut r_window_state.raw;
                    let event_handler = &mut r_window_state.fun;
                    window_state.handle_event(
                        &mut *event_handler,
                        LayerShellEvent::UserEvent(event),
                        None,
                    );
                })
                .expect("Failed to insert message channel source");
        }

        event_loop
            .handle()
            .insert_source(
                Timer::from_duration(Duration::from_millis(50)),
                move |_, _, r_window_state| {
                    let window_state = &mut r_window_state.raw;
                    let event_handler = &mut r_window_state.fun;
                    let mut messages = Vec::new();
                    std::mem::swap(&mut messages, &mut window_state.message);
                    for msg in messages.iter() {
                        match msg {
                            (index_info, DispatchMessageInner::XdgInfoChanged(change_type)) => {
                                window_state.handle_event(
                                     &mut *event_handler,
                                    LayerShellEvent::XdgInfoChanged(*change_type),
                                    *index_info,
                                );
                            }
                            (_, DispatchMessageInner::NewDisplay(output_display)) => {
                                if !window_state.is_allscreens() {
                                    continue;
                                }
                                let wl_surface = wmcompositer.create_surface(&qh, ()); // and create a surface. if two or more,
                                let layer_shell = globals
                                    .bind::<ZwlrLayerShellV1, _, _>(&qh, 3..=4, ())
                                    .unwrap();
                                let layer = layer_shell.get_layer_surface(
                                    &wl_surface,
                                    Some(output_display),
                                    window_state.layer,
                                    window_state.namespace.clone(),
                                    &qh,
                                    (),
                                );
                                layer.set_anchor(window_state.anchor);
                                layer
                                    .set_keyboard_interactivity(window_state.keyboard_interactivity);
                                if let Some((init_w, init_h)) = window_state.size {
                                    layer.set_size(init_w, init_h);
                                }

                                if let Some(zone) = window_state.exclusive_zone {
                                    layer.set_exclusive_zone(zone);
                                }

                                if let Some((top, right, bottom, left)) = window_state.margin {
                                    layer.set_margin(top, right, bottom, left);
                                }

                                if window_state.events_transparent {
                                    let region = wmcompositer.create_region(&qh, ());
                                    wl_surface.set_input_region(Some(&region));
                                    region.destroy();
                                }
                                wl_surface.commit();

                                let zxdgoutput =
                                    xdg_output_manager.get_xdg_output(output_display, &qh, ());
                                let mut fractional_scale = None;
                                if let Some(ref fractional_scale_manager) = fractional_scale_manager
                                {
                                    fractional_scale =
                                        Some(fractional_scale_manager.get_fractional_scale(
                                            &wl_surface,
                                            &qh,
                                            (),
                                        ));
                                }
                                let viewport = viewporter
                                    .as_ref()
                                    .map(|viewport| viewport.get_viewport(&wl_surface, &qh, ()));
                                // so during the init Configure of the shell, a buffer, atleast a buffer is needed.
                                // and if you need to reconfigure it, you need to commit the wl_surface again
                                // so because this is just an example, so we just commit it once
                                // like if you want to reset anchor or KeyboardInteractivity or resize, commit is needed

                                window_state.push_window(
                                    WindowStateUnitBuilder::new(
                                        id::Id::unique(),
                                        qh.clone(),
                                        connection.display(),
                                        wl_surface,
                                        Shell::LayerShell(layer),
                                    )
                                    .viewport(viewport)
                                    .zxdgoutput(Some(ZxdgOutputInfo::new(zxdgoutput)))
                                    .fractional_scale(fractional_scale)
                                    .wl_output(Some(output_display.clone()))
                                    .build(),
                                );
                            }
                            _ => {
                                let (index_message, msg) = msg;

                                let msg: DispatchMessage = msg.clone().into();
                                window_state.handle_event(
                                    &mut *event_handler,
                                    LayerShellEvent::RequestMessages(&msg),
                                    *index_message,
                                );
                            }
                        }
                    }

                    // User events are now processed by the ping source for immediate response.
                    // The timer only handles internal messages and periodic dispatch.
                    window_state.handle_event(
                        &mut *event_handler,
                        LayerShellEvent::NormalDispatch,
                        None,
                    );
                    loop {
                        let mut return_data = vec![];
                        std::mem::swap(&mut window_state.return_data, &mut return_data);

                        for data in return_data {
                            match data {
                                ReturnData::RequestExit => {
                                    signal.stop();
                                    return TimeoutAction::Drop;
                                }
                                ReturnData::RequestSetCursorShape((shape_name, pointer)) => {
                                    let Some(serial) = window_state.enter_serial else {
                                        continue;
                                    };
                                    set_cursor_shape(
                                        &cursor_update_context,
                                        shape_name,
                                        pointer,
                                        serial,
                                    );
                                }
                                ReturnData::NewLayerShell((
                                    NewLayerShellSettings {
                                        size,
                                        layer,
                                        anchor,
                                        exclusive_zone,
                                        margin,
                                        keyboard_interactivity,
                                        output_option: output_type,
                                        events_transparent,
                                        namespace,
                                        blur,
                                        shadow,
                                        corner_radius,
                                        auto_size: _, // Auto-size is handled at the iced level
                                    },
                                    id,
                                    info,
                                )) => {
                                    let output = match output_type {
                                        OutputOption::Output(output) => Some(output),
                                        _ => {
                                            let pos = window_state.surface_pos();

                                            let mut output =
                                                pos.and_then(|p| window_state.units[p].wl_output.as_ref());

                                            if window_state.last_wloutput.is_none()
                                                && window_state.outputs.len() > window_state.last_unit_index
                                            {
                                                window_state.last_wloutput = Some(
                                                    window_state.outputs[window_state.last_unit_index]
                                                        .1
                                                        .clone(),
                                                );
                                            }

                                            if matches!(output_type, events::OutputOption::LastOutput) {
                                                output = window_state.last_wloutput.as_ref();
                                            }

                                            output.cloned()
                                        }
                                    };


                                    let wl_surface = wmcompositer.create_surface(&qh, ()); // and create a surface. if two or more,
                                    let layer_shell = globals
                                        .bind::<ZwlrLayerShellV1, _, _>(&qh, 3..=4, ())
                                        .unwrap();
                                    let layer = layer_shell.get_layer_surface(
                                        &wl_surface,
                                        output.as_ref(),
                                        layer,
                                        namespace.unwrap_or_else(|| window_state.namespace.clone()),
                                        &qh,
                                        (),
                                    );
                                    layer.set_anchor(anchor);
                                    layer.set_keyboard_interactivity(keyboard_interactivity);
                                    if let Some((init_w, init_h)) = size {
                                        layer.set_size(init_w, init_h);
                                    }

                                    if let Some(zone) = exclusive_zone {
                                        layer.set_exclusive_zone(zone);
                                    }

                                    if let Some((top, right, bottom, left)) = margin {
                                        layer.set_margin(top, right, bottom, left);
                                    }

                                    if events_transparent {
                                        let region = wmcompositer.create_region(&qh, ());
                                        wl_surface.set_input_region(Some(&region));
                                        region.destroy();
                                    }

                                    // Apply blur if requested
                                    if blur {
                                        apply_blur_to_surface(&window_state.blur_manager, &wl_surface, &qh);
                                    }

                                    // Apply corner radius if set (per-surface setting takes precedence, then fallback to window_state)
                                    let surface_id = wl_surface.id().protocol_id();
                                    let effective_corner_radius = corner_radius.or(window_state.corner_radius);
                                    log::debug!("NewLayerShell: corner_radius={:?}, effective={:?}", corner_radius, effective_corner_radius);
                                    if effective_corner_radius.is_some() {
                                        if let Some(corner_obj) = apply_corner_radius_to_surface(&window_state.corner_radius_manager, effective_corner_radius, &wl_surface, &qh) {
                                            window_state.corner_radius_surfaces.insert(surface_id, corner_obj);
                                        }
                                    }

                                    // Apply shadow if enabled (per-surface setting takes precedence, then fallback to window_state)
                                    log::debug!("NewLayerShell: shadow={}, window_state.shadow={}, shadow_manager present={}", 
                                        shadow, window_state.shadow, window_state.shadow_manager.is_some());
                                    if shadow || window_state.shadow {
                                        apply_shadow_to_surface(&window_state.shadow_manager, &wl_surface, &qh);
                                    } else {
                                        log::debug!("NewLayerShell: shadow not requested for this surface");
                                    }

                                    // Apply home visibility mode if enabled
                                    if window_state.home_only {
                                        if let Some(controller) = apply_home_visibility_to_surface(
                                            &window_state.home_visibility_manager,
                                            &wl_surface,
                                            &qh,
                                            home_visibility::VisibilityMode::HomeOnly,
                                        ) {
                                            window_state.home_visibility_controllers.insert(surface_id, controller);
                                        }
                                    } else if window_state.hide_on_home {
                                        if let Some(controller) = apply_home_visibility_to_surface(
                                            &window_state.home_visibility_manager,
                                            &wl_surface,
                                            &qh,
                                            home_visibility::VisibilityMode::HideOnHome,
                                        ) {
                                            window_state.home_visibility_controllers.insert(surface_id, controller);
                                        }
                                    }

                                    // Register surface for voice mode events if enabled
                                    if window_state.voice_mode_enabled {
                                        let is_default = window_state.voice_mode_receivers.is_empty();
                                        if let Some(receiver) = register_voice_mode_for_surface(
                                            &window_state.voice_mode_manager,
                                            &wl_surface,
                                            &qh,
                                            is_default,
                                        ) {
                                            window_state.voice_mode_receivers.insert(surface_id, receiver);
                                        }
                                    }

                                    wl_surface.commit();

                                    let mut fractional_scale = None;
                                    if let Some(ref fractional_scale_manager) =
                                        fractional_scale_manager
                                    {
                                        fractional_scale =
                                            Some(fractional_scale_manager.get_fractional_scale(
                                                &wl_surface,
                                                &qh,
                                                (),
                                            ));
                                    }
                                    let viewport = viewporter.as_ref().map(|viewport| {
                                        viewport.get_viewport(&wl_surface, &qh, ())
                                    });
                                    // so during the init Configure of the shell, a buffer, atleast a buffer is needed.
                                    // and if you need to reconfigure it, you need to commit the wl_surface again
                                    // so because this is just an example, so we just commit it once
                                    // like if you want to reset anchor or KeyboardInteractivity or resize, commit is needed

                                    window_state.push_window(
                                        WindowStateUnitBuilder::new(
                                            id,
                                            qh.clone(),
                                            connection.display(),
                                            wl_surface,
                                            Shell::LayerShell(layer),
                                        )
                                        .viewport(viewport)
                                        .fractional_scale(fractional_scale)
                                        .wl_output(output)
                                        .binding(info)
                                        .becreated(true)
                                        .build(),
                                    );
                                }
                                ReturnData::NewPopUp((
                                    NewPopUpSettings {
                                        size: (width, height),
                                        position: (x, y),
                                        id,
                                    },
                                    targetid,
                                    info,
                                )) => {
                                    let Some(index) = window_state
                                        .units
                                        .iter()
                                        .position(|unit| !unit.is_popup() && unit.id == id)
                                    else {
                                        continue;
                                    };
                                    let wl_surface = wmcompositer.create_surface(&qh, ());
                                    let positioner = wmbase.create_positioner(&qh, ());
                                    positioner.set_size(width as i32, height as i32);
                                    positioner.set_anchor_rect(x, y, width as i32, height as i32);
                                    let wl_xdg_surface =
                                        wmbase.get_xdg_surface(&wl_surface, &qh, ());
                                    let popup =
                                        wl_xdg_surface.get_popup(None, &positioner, &qh, ());

                                    let Shell::LayerShell(shell) = &window_state.units[index].shell
                                    else {
                                        unreachable!()
                                    };
                                    shell.get_popup(&popup);

                                    let mut fractional_scale = None;
                                    if let Some(ref fractional_scale_manager) =
                                        fractional_scale_manager
                                    {
                                        fractional_scale =
                                            Some(fractional_scale_manager.get_fractional_scale(
                                                &wl_surface,
                                                &qh,
                                                (),
                                            ));
                                    }
                                    wl_surface.commit();

                                    let viewport = viewporter.as_ref().map(|viewport| {
                                        viewport.get_viewport(&wl_surface, &qh, ())
                                    });
                                    window_state.push_window(
                                        WindowStateUnitBuilder::new(
                                            targetid,
                                            qh.clone(),
                                            connection.display(),
                                            wl_surface,
                                            Shell::PopUp((popup, wl_xdg_surface)),
                                        )
                                        .size((width, height))
                                        .viewport(viewport)
                                        .fractional_scale(fractional_scale)
                                        .binding(info)
                                        .becreated(true)
                                        .build(),
                                    );
                                },
                                ReturnData::NewXdgBase((
                                NewXdgWindowSettings { maximized, title, size },
                                    id,
                                    info,
                                )) => {
                                    let wl_surface = wmcompositer.create_surface(&qh, ());
                                    let wl_xdg_surface =
                                        wmbase.get_xdg_surface(&wl_surface, &qh, ());
                                    let toplevel =
                                        wl_xdg_surface.get_toplevel(&qh, ());

                                    toplevel.set_title(title.unwrap_or("".to_owned()));

                                    if maximized { toplevel.set_maximized(); }
                                    let decoration = if let Some(decoration_manager) = &zxdg_decoration_manager {
                                        let decoration = decoration_manager.get_toplevel_decoration(&toplevel, &qh, ());
                                        use zxdg_toplevel_decoration_v1::Mode;
                                        // Use client-side (no) decorations when maximized, server-side otherwise
                                        if maximized {
                                            decoration.set_mode(Mode::ClientSide);
                                        } else {
                                            decoration.set_mode(Mode::ServerSide);
                                        }
                                        Some(decoration)

                                    } else {
                                        None
                                    };
                                    let mut fractional_scale = None;
                                    if let Some(ref fractional_scale_manager) =
                                        fractional_scale_manager
                                    {
                                        fractional_scale =
                                            Some(fractional_scale_manager.get_fractional_scale(
                                                &wl_surface,
                                                &qh,
                                                (),
                                            ));
                                    }
                                    wl_surface.commit();

                                    let viewport = viewporter.as_ref().map(|viewport| {
                                        viewport.get_viewport(&wl_surface, &qh, ())
                                    });
                                    window_state.push_window(
                                        WindowStateUnitBuilder::new(
                                            id,
                                            qh.clone(),
                                            connection.display(),
                                            wl_surface,
                                            Shell::XdgTopLevel((toplevel, wl_xdg_surface, decoration)),
                                        )
                                        .size(size.unwrap_or((300, 300)))
                                        .viewport(viewport)
                                        .fractional_scale(fractional_scale)
                                        .binding(info)
                                        .becreated(true)
                                        .build(),
                                    );
                                },

                                ReturnData::NewInputPanel((
                                    NewInputPanelSettings {
                                        size: (width, height),
                                        keyboard,
                                        use_last_output,
                                    },
                                    id,
                                    info,
                                )) => {
                                    let pos = window_state.surface_pos();

                                    let mut output = pos.and_then(|p| window_state.units[p].wl_output.as_ref());

                                    if window_state.last_wloutput.is_none()
                                        && window_state.outputs.len() > window_state.last_unit_index
                                    {
                                        window_state.last_wloutput =
                                            Some(window_state.outputs[window_state.last_unit_index].1.clone());
                                    }

                                    if use_last_output {
                                        output = window_state.last_wloutput.as_ref();
                                    }

                                    if output.is_none() {
                                        output = window_state.outputs.first().map(|(_, o)| o);
                                    }

                                    let Some(output) = output else {
                                        log::warn!("no WlOutput, skip creating input panel");
                                        continue;
                                    };

                                    let wl_surface = wmcompositer.create_surface(&qh, ());
                                    let input_panel = globals
                                        .bind::<ZwpInputPanelV1, _, _>(&qh, 1..=1, ())
                                        .unwrap();
                                    let input_panel_surface =
                                        input_panel.get_input_panel_surface(&wl_surface, &qh, ());
                                    if keyboard {
                                        input_panel_surface.set_toplevel(
                                            output,
                                            ZwpInputPanelPosition::CenterBottom as u32,
                                        );
                                    } else {
                                        input_panel_surface.set_overlay_panel();
                                    }
                                    wl_surface.commit();

                                    let mut fractional_scale = None;
                                    if let Some(ref fractional_scale_manager) = fractional_scale_manager {
                                        fractional_scale =
                                            Some(fractional_scale_manager.get_fractional_scale(
                                                &wl_surface,
                                                &qh,
                                                (),
                                            ));
                                    }

                                    let viewport = viewporter
                                        .as_ref()
                                        .map(|viewport| viewport.get_viewport(&wl_surface, &qh, ()));
                                    window_state.push_window(
                                        WindowStateUnitBuilder::new(
                                            id,
                                            qh.clone(),
                                            connection.display(),
                                            wl_surface,
                                            Shell::InputPanel(input_panel_surface),
                                        )
                                        .size((width, height))
                                        .viewport(viewport)
                                        .fractional_scale(fractional_scale)
                                        .binding(info)
                                        .becreated(true)
                                        .build(),
                                    );
                                },
                                _ => {}
                            }
                        }
                        if window_state.return_data.is_empty() {
                            break;
                        }
                    }

                    let to_be_closed_ids: Vec<_> = window_state
                        .units
                        .iter()
                        .filter(|unit| unit.request_flag.close)
                        .map(WindowStateUnit::id)
                        .collect();
                    for id in to_be_closed_ids {
                        window_state.handle_event(
                            &mut *event_handler,
                            LayerShellEvent::RequestMessages(&DispatchMessage::Closed),
                            Some(id),
                        );
                        // event_handler may use unit, only remove it after calling event_handler.
                        window_state.remove_shell(id);
                    }

                    // NOTE: this is for those closed because wl_output is dead.
                    let closed_ids = window_state.closed_ids.clone();
                    for id in closed_ids {
                        window_state.handle_event(
                            &mut *event_handler,
                            LayerShellEvent::RequestMessages(&DispatchMessage::Closed),
                            Some(id),
                        );
                    }
                    window_state.closed_ids.clear();


                    for idx in 0..window_state.units.len() {
                        let unit = &mut window_state.units[idx];
                        let (width, height) = unit.size;
                        if width == 0 || height == 0 {
                            // don't refresh, if size is 0.
                            continue;
                        }
                        if unit.take_present_slot() {
                            let unit_id = unit.id;
                            let is_created = unit.becreated;
                            let scale_float = unit.scale_float();
                            let wl_surface = unit.wl_surface.clone();
                            if unit.buffer.is_none() && !window_state.use_display_handle {
                                let Ok(mut file) = tempfile::tempfile() else {
                                    log::error!("Cannot create new file from tempfile");
                                    return TimeoutAction::Drop;
                                };
                                let ReturnData::WlBuffer(buffer) = event_handler(
                                    LayerShellEvent::RequestBuffer(&mut file, &shm, &qh, width, height),
                                    window_state,
                                    Some(unit_id)) else {
                                    panic!("You cannot return this one");
                                };
                                wl_surface.attach(Some(&buffer), 0, 0);
                                wl_surface.commit();
                                window_state.units[idx].buffer = Some(buffer);
                            }
                            window_state.handle_event(
                                &mut *event_handler,
                                LayerShellEvent::RequestMessages(&DispatchMessage::RequestRefresh {
                                    width,
                                    height,
                                    is_created,
                                    scale_float,
                                }),
                                Some(unit_id),
                            );
                            // reset if the slot is not used
                            window_state.units[idx].reset_present_slot();
                        }
                    }
                    TimeoutAction::ToDuration(std::time::Duration::from_millis(50))
                },
            )
            .expect("Cannot insert_source");
        event_loop
            .run(
                std::time::Duration::from_millis(20),
                &mut state,
                move |r_window_state| {
                    let window_state = &mut r_window_state.raw;
                    let _ = event_queue_origin.roundtrip(window_state);
                    let looph = &r_window_state.loop_handle;
                    for token in window_state.to_remove_tokens.iter() {
                        looph.remove(*token);
                    }
                    window_state.to_remove_tokens.clear();
                    if let Some(VirtualKeyRelease { delay, time, key }) =
                        window_state.to_be_released_key
                    {
                        looph
                            .insert_source(
                                Timer::from_duration(delay),
                                move |_, _, r_window_state| {
                                    let state = &mut r_window_state.raw;
                                    let ky = state.get_virtual_keyboard().unwrap();

                                    ky.key(time, key, KeyState::Released.into());
                                    TimeoutAction::Drop
                                },
                            )
                            .ok();
                    }
                    if let Some(KeyboardTokenState {
                        key,
                        delay,
                        surface_id,
                        pressed_state,
                    }) = window_state.repeat_delay.take()
                    {
                        let timer = Timer::from_duration(delay);
                        let keyboard_state = window_state.keyboard_state.as_mut().unwrap();
                        keyboard_state.repeat_token = looph
                            .insert_source(timer, move |_, _, r_window_state| {
                                let state = &mut r_window_state.raw;
                                let event_handler = &mut r_window_state.fun;
                                let keyboard_state = match state.keyboard_state.as_mut() {
                                    Some(keyboard_state) => keyboard_state,
                                    None => return TimeoutAction::Drop,
                                };
                                let repeat_keycode = match keyboard_state.current_repeat {
                                    Some(repeat_keycode) => repeat_keycode,
                                    None => return TimeoutAction::Drop,
                                };
                                // NOTE: not the same key
                                if repeat_keycode != key {
                                    return TimeoutAction::Drop;
                                }
                                if let Some(mut key_context) =
                                    keyboard_state.xkb_context.key_context()
                                {
                                    let event = key_context.process_key_event(
                                        repeat_keycode,
                                        pressed_state,
                                        false,
                                    );
                                    let event = DispatchMessageInner::KeyboardInput {
                                        event,
                                        is_synthetic: false,
                                    };
                                    state.message.push((surface_id, event));
                                }
                                let repeat_info = keyboard_state.repeat_info;

                                let _ = keyboard_state;
                                state.handle_event(
                                    &mut *event_handler,
                                    LayerShellEvent::NormalDispatch,
                                    None,
                                );
                                match repeat_info {
                                    RepeatInfo::Repeat { gap, .. } => {
                                        TimeoutAction::ToDuration(gap)
                                    }
                                    RepeatInfo::Disable => TimeoutAction::Drop,
                                }
                            })
                            .ok();
                    }
                },
            )
            .expect("Error during event loop!");
        Ok(())
    }

    pub fn request_next_present(&mut self, id: id::Id) {
        self.get_mut_unit_with_id(id)
            .map(WindowStateUnit::request_next_present);
    }

    pub fn reset_present_slot(&mut self, id: id::Id) {
        self.get_mut_unit_with_id(id)
            .map(WindowStateUnit::reset_present_slot);
    }

    pub fn handle_event<F, Message>(
        &mut self,
        mut event_handler: F,
        event: LayerShellEvent<T, Message>,
        unit_id: Option<id::Id>,
    ) where
        Message: std::marker::Send + 'static,
        F: FnMut(LayerShellEvent<T, Message>, &mut WindowState<T>, Option<id::Id>) -> ReturnData<T>,
    {
        let return_data = event_handler(event, self, unit_id);
        if !matches!(return_data, ReturnData::None) {
            self.append_return_data(return_data);
        }
    }
}

fn get_cursor_buffer(
    shape: &str,
    connection: &Connection,
    shm: &WlShm,
) -> Option<CursorImageBuffer> {
    let mut cursor_theme = CursorTheme::load(connection, shm.clone(), 23).ok()?;
    let cursor = cursor_theme.get_cursor(shape);
    Some(cursor?[0].clone())
}

/// avoid too_many_arguments alert in `set_cursor_shape`
struct CursorUpdateContext<T: 'static> {
    cursor_manager: Option<WpCursorShapeManagerV1>,
    qh: QueueHandle<WindowState<T>>,
    connection: Connection,
    shm: WlShm,
    wmcompositer: WlCompositor,
}

fn set_cursor_shape<T: 'static>(
    context: &CursorUpdateContext<T>,
    shape_name: String,
    pointer: WlPointer,
    serial: u32,
) {
    if let Some(cursor_manager) = &context.cursor_manager {
        let Some(shape) = str_to_shape(&shape_name) else {
            log::error!("Not supported shape");
            return;
        };
        let device = cursor_manager.get_pointer(&pointer, &context.qh, ());
        device.set_shape(serial, shape);
        device.destroy();
    } else {
        let Some(cursor_buffer) = get_cursor_buffer(&shape_name, &context.connection, &context.shm)
        else {
            log::error!("Cannot find cursor {shape_name}");
            return;
        };
        let cursor_surface = context.wmcompositer.create_surface(&context.qh, ());
        cursor_surface.attach(Some(&cursor_buffer), 0, 0);
        // and create a surface. if two or more,
        let (hotspot_x, hotspot_y) = cursor_buffer.hotspot();
        pointer.set_cursor(
            serial,
            Some(&cursor_surface),
            hotspot_x as i32,
            hotspot_y as i32,
        );
        cursor_surface.commit();
    }
}
