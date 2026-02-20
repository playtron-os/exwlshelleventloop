use iced_core::mouse;
use iced_runtime::Action;
use layershellev::DispatchMessage;
#[cfg(feature = "foreign-toplevel")]
use layershellev::foreign_toplevel::ForeignToplevelEvent;
use layershellev::keyboard::ModifiersState;
use layershellev::reexport::wayland_client::{ButtonState, KeyState, WEnum, WlRegion};
pub use layershellev::voice_mode::VoiceModeEvent;
use layershellev::xkb_keyboard::KeyEvent as LayerShellKeyEvent;
#[cfg(feature = "foreign-toplevel")]
use std::sync::OnceLock;
#[cfg(feature = "foreign-toplevel")]
use std::sync::mpsc;

use iced_core::keyboard::Modifiers as IcedModifiers;

// Global channel for foreign toplevel events
#[cfg(feature = "foreign-toplevel")]
static FOREIGN_TOPLEVEL_CHANNEL: OnceLock<(
    std::sync::Mutex<mpsc::Sender<ForeignToplevelEvent>>,
    std::sync::Mutex<mpsc::Receiver<ForeignToplevelEvent>>,
)> = OnceLock::new();

#[cfg(feature = "foreign-toplevel")]
fn get_foreign_toplevel_channel() -> &'static (
    std::sync::Mutex<mpsc::Sender<ForeignToplevelEvent>>,
    std::sync::Mutex<mpsc::Receiver<ForeignToplevelEvent>>,
) {
    FOREIGN_TOPLEVEL_CHANNEL.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        (std::sync::Mutex::new(tx), std::sync::Mutex::new(rx))
    })
}

/// Send a foreign toplevel event (called by the event loop)
#[cfg(feature = "foreign-toplevel")]
pub(crate) fn send_foreign_toplevel_event(event: ForeignToplevelEvent) {
    let (tx, _) = get_foreign_toplevel_channel();
    if let Ok(tx) = tx.lock() {
        let _ = tx.send(event);
    }
}

/// Subscription for foreign toplevel events
///
/// Use this to receive events about toplevel windows (created, changed, closed)
/// when `foreign_toplevel: true` is set in the layer shell settings.
///
/// This function is only available when the `foreign-toplevel` feature is enabled.
///
/// # Example
/// ```ignore
/// fn subscription(&self) -> Subscription<Message> {
///     iced_layershell::event::foreign_toplevel_subscription()
///         .map(Message::ForeignToplevel)
/// }
/// ```
#[cfg(feature = "foreign-toplevel")]
pub fn foreign_toplevel_subscription() -> iced_futures::Subscription<ForeignToplevelEvent> {
    #[derive(Hash)]
    struct ForeignToplevelSubscription;

    iced_futures::Subscription::run_with(ForeignToplevelSubscription, |_| {
        iced_futures::stream::channel(
            100,
            |mut output: iced_futures::futures::channel::mpsc::Sender<ForeignToplevelEvent>| async move {
                use iced_futures::futures::SinkExt;
                let (_, rx) = get_foreign_toplevel_channel();

                loop {
                    // Try to receive all pending events
                    let events: Vec<ForeignToplevelEvent> = {
                        if let Ok(rx) = rx.lock() {
                            std::iter::from_fn(|| rx.try_recv().ok()).collect()
                        } else {
                            Vec::new()
                        }
                    };

                    if !events.is_empty() {
                        for event in events {
                            let _ = output.send(event).await;
                        }
                    }

                    // Small async delay to avoid busy-waiting (~60fps polling)
                    futures_timer::Delay::new(std::time::Duration::from_millis(16)).await;
                }
            },
        )
    })
}

fn from_u32_to_icedmouse(code: u32) -> mouse::Button {
    match code {
        273 => mouse::Button::Right,
        274 => mouse::Button::Middle,
        _ => mouse::Button::Left,
    }
}

#[derive(Debug, Clone, Copy)]
pub enum IcedButtonState {
    Pressed(mouse::Button),
    Released(mouse::Button),
}

#[derive(Debug, Clone, Copy)]
pub enum IcedKeyState {
    Pressed,
    Released,
}

impl From<WEnum<KeyState>> for IcedKeyState {
    fn from(value: WEnum<KeyState>) -> Self {
        match value {
            WEnum::Value(KeyState::Released) => Self::Released,
            WEnum::Value(KeyState::Pressed) => Self::Pressed,
            _ => unreachable!(),
        }
    }
}

#[allow(unused)]
#[derive(Debug, Clone)]
pub enum WindowEvent {
    ScaleFactorChanged {
        scale_u32: u32,
        scale_float: f64,
    },
    CursorEnter {
        x: f64,
        y: f64,
    },
    CursorMoved {
        x: f64,
        y: f64,
    },
    CursorLeft,
    MouseInput(IcedButtonState),
    Keyboard {
        state: IcedKeyState,
        key: u32,
        modifiers: IcedModifiers,
    },
    KeyBoardInput {
        event: LayerShellKeyEvent,
        is_synthetic: bool,
    },
    Unfocus,
    Focused,
    ModifiersChanged(ModifiersState),
    Axis {
        x: f32,
        y: f32,
    },
    PixelDelta {
        x: f32,
        y: f32,
    },
    ScrollStop,
    TouchDown {
        id: i32,
        x: f64,
        y: f64,
    },
    TouchUp {
        id: i32,
        x: f64,
        y: f64,
    },
    TouchMotion {
        id: i32,
        x: f64,
        y: f64,
    },
    TouchCancel {
        id: i32,
        x: f64,
        y: f64,
    },
    Ime(layershellev::Ime),
    Refresh,
    Closed,
    ThemeChanged(iced_core::theme::Mode),
    /// Home state changed from compositor
    /// is_home: true = at home (no windows visible), false = windows visible
    HomeStateChanged {
        is_home: bool,
    },
    /// Auto-hide visibility changed from compositor
    /// visible: true = surface is fully visible, false = surface is fully hidden
    AutoHideVisibilityChanged {
        visible: bool,
    },
    /// Layer-surface visibility changed via hide/show protocol
    /// visible: true = surface visible, false = surface hidden
    SurfaceVisibilityChanged {
        visible: bool,
    },
    /// Voice mode event from compositor (started, stopped, cancelled, orb attached/detached)
    VoiceMode(VoiceModeEvent),
    /// Foreign toplevel event (window created, changed, or closed)
    #[cfg(feature = "foreign-toplevel")]
    ForeignToplevel(ForeignToplevelEvent),
    /// Dismiss requested - user clicked/touched outside an armed dismiss group
    DismissRequested,
}

#[derive(Debug)]
pub enum IcedLayerShellEvent<Message> {
    UpdateInputRegion(WlRegion),
    Window(WindowEvent),
    UserAction(Action<Message>),
    NormalDispatch,
}

impl From<&DispatchMessage> for WindowEvent {
    fn from(value: &DispatchMessage) -> Self {
        match value {
            DispatchMessage::RequestRefresh { .. } => WindowEvent::Refresh,
            DispatchMessage::Closed => WindowEvent::Closed,
            DispatchMessage::MouseEnter {
                surface_x: x,
                surface_y: y,
                ..
            } => WindowEvent::CursorEnter { x: *x, y: *y },
            DispatchMessage::MouseMotion {
                surface_x: x,
                surface_y: y,
                ..
            } => WindowEvent::CursorMoved { x: *x, y: *y },
            DispatchMessage::MouseLeave => WindowEvent::CursorLeft,
            DispatchMessage::MouseButton { state, button, .. } => {
                let btn = from_u32_to_icedmouse(*button);
                match state {
                    WEnum::Value(ButtonState::Pressed) => {
                        WindowEvent::MouseInput(IcedButtonState::Pressed(btn))
                    }
                    WEnum::Value(ButtonState::Released) => {
                        WindowEvent::MouseInput(IcedButtonState::Released(btn))
                    }
                    _ => unreachable!(),
                }
            }
            DispatchMessage::TouchUp { id, x, y, .. } => WindowEvent::TouchUp {
                id: *id,
                x: *x,
                y: *y,
            },
            DispatchMessage::TouchDown { id, x, y, .. } => WindowEvent::TouchDown {
                id: *id,
                x: *x,
                y: *y,
            },
            DispatchMessage::TouchMotion { id, x, y, .. } => WindowEvent::TouchMotion {
                id: *id,
                x: *x,
                y: *y,
            },
            DispatchMessage::TouchCancel { id, x, y, .. } => WindowEvent::TouchCancel {
                id: *id,
                x: *x,
                y: *y,
            },
            DispatchMessage::PreferredScale {
                scale_u32,
                scale_float,
            } => WindowEvent::ScaleFactorChanged {
                scale_u32: *scale_u32,
                scale_float: *scale_float,
            },

            DispatchMessage::KeyboardInput {
                event,
                is_synthetic,
            } => WindowEvent::KeyBoardInput {
                event: event.clone(),
                is_synthetic: *is_synthetic,
            },
            DispatchMessage::Unfocus => WindowEvent::Unfocus,
            DispatchMessage::Focused(_) => WindowEvent::Focused,
            DispatchMessage::ModifiersChanged(modifiers) => {
                WindowEvent::ModifiersChanged(*modifiers)
            }
            DispatchMessage::Axis {
                horizontal,
                vertical,
                scale,
                ..
            } => {
                if horizontal.stop && vertical.stop {
                    WindowEvent::ScrollStop
                } else if vertical.discrete != 0 || horizontal.discrete != 0 {
                    WindowEvent::Axis {
                        x: (-horizontal.discrete as f64 * scale) as f32,
                        y: (-vertical.discrete as f64 * scale) as f32,
                    }
                } else {
                    WindowEvent::PixelDelta {
                        x: (-horizontal.absolute * scale) as f32,
                        y: (-vertical.absolute * scale) as f32,
                    }
                }
            }
            DispatchMessage::Ime(ime) => WindowEvent::Ime(ime.clone()),
            DispatchMessage::HomeStateChanged { is_home } => {
                WindowEvent::HomeStateChanged { is_home: *is_home }
            }
            DispatchMessage::AutoHideVisibilityChanged { visible } => {
                WindowEvent::AutoHideVisibilityChanged { visible: *visible }
            }
            DispatchMessage::SurfaceVisibilityChanged { visible } => {
                WindowEvent::SurfaceVisibilityChanged { visible: *visible }
            }
            DispatchMessage::VoiceMode(event) => WindowEvent::VoiceMode(event.clone()),
            #[cfg(feature = "foreign-toplevel")]
            DispatchMessage::ForeignToplevel(event) => WindowEvent::ForeignToplevel(event.clone()),
            DispatchMessage::DismissRequested => WindowEvent::DismissRequested,
        }
    }
}
