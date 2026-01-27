//! Client-side implementation of the COSMIC voice mode protocol (zcosmic_voice_mode_v1)
//!
//! This protocol allows layer-shell clients to receive voice mode events from
//! the compositor when the user activates voice input through the system.

use log::{debug, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum};

/// Global flag indicating whether voice processing is active.
/// When will_stop arrives, we immediately respond with this value.
/// This avoids round-trip through the iced event loop.
static VOICE_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Set whether voice processing is currently active.
/// Call this when recording starts (true) and when transcription completes (false).
/// This state is used to immediately respond to will_stop from the compositor.
pub fn set_voice_active(active: bool) {
    info!("Voice active state set to: {}", active);
    VOICE_ACTIVE.store(active, Ordering::SeqCst);
}

/// Get whether voice processing is currently active.
pub fn is_voice_active() -> bool {
    VOICE_ACTIVE.load(Ordering::SeqCst)
}

// Re-export the generated protocol types
pub use generated::{zcosmic_voice_mode_manager_v1, zcosmic_voice_mode_v1};

pub use zcosmic_voice_mode_v1::OrbState;

#[allow(
    non_snake_case,
    non_upper_case_globals,
    non_camel_case_types,
    dead_code,
    unused_imports
)]
mod generated {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_backend;
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/voice_mode.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/voice_mode.xml");
}

/// Voice mode event from the compositor
#[derive(Debug, Clone)]
pub enum VoiceModeEvent {
    /// Voice input started
    Started {
        /// Where the orb is displayed
        orb_state: OrbState,
    },
    /// Voice input stopped normally
    Stopped,
    /// Voice input cancelled
    Cancelled,
    /// Orb attached to this receiver's window
    OrbAttached {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
    /// Orb detached from this receiver's window
    OrbDetached,
    /// Voice input is about to stop - client must respond with ack_stop
    WillStop {
        /// Serial to echo back in ack_stop
        serial: u32,
    },
}

/// User data for the voice mode manager
#[derive(Debug, Clone, Default)]
pub struct VoiceModeManagerData;

/// User data for the voice mode receiver
#[derive(Debug, Clone)]
pub struct VoiceModeReceiverData {
    pub surface: WlSurface,
    pub is_default: bool,
}

/// Trait for handling voice mode events
pub trait VoiceModeHandler {
    /// Called when a voice mode event is received
    fn voice_mode_event(&mut self, event: VoiceModeEvent);
}

/// Blanket implementation for voice mode manager dispatch
impl<D>
    Dispatch<
        zcosmic_voice_mode_manager_v1::ZcosmicVoiceModeManagerV1,
        VoiceModeManagerData,
        D,
    > for ()
where
    D: Dispatch<
            zcosmic_voice_mode_manager_v1::ZcosmicVoiceModeManagerV1,
            VoiceModeManagerData,
        >,
{
    fn event(
        _state: &mut D,
        _proxy: &zcosmic_voice_mode_manager_v1::ZcosmicVoiceModeManagerV1,
        _event: zcosmic_voice_mode_manager_v1::Event,
        _data: &VoiceModeManagerData,
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        // No events defined for the manager
        debug!("Voice mode manager event (none expected)");
    }
}

/// Blanket implementation for voice mode receiver dispatch
impl<D> Dispatch<zcosmic_voice_mode_v1::ZcosmicVoiceModeV1, VoiceModeReceiverData, D> for ()
where
    D: Dispatch<zcosmic_voice_mode_v1::ZcosmicVoiceModeV1, VoiceModeReceiverData>
        + VoiceModeHandler,
{
    fn event(
        state: &mut D,
        _proxy: &zcosmic_voice_mode_v1::ZcosmicVoiceModeV1,
        event: zcosmic_voice_mode_v1::Event,
        data: &VoiceModeReceiverData,
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        debug!("Voice mode receiver event: {:?}, is_default: {}", event, data.is_default);
        
        let voice_event = match event {
            zcosmic_voice_mode_v1::Event::Start { orb_state } => {
                let orb_state = match orb_state {
                    WEnum::Value(s) => s,
                    WEnum::Unknown(v) => {
                        warn!("Unknown orb state value: {}", v);
                        OrbState::Hidden
                    }
                };
                info!("Voice mode started, orb_state: {:?}", orb_state);
                VoiceModeEvent::Started { orb_state }
            }
            zcosmic_voice_mode_v1::Event::Stop => {
                info!("Voice mode stopped");
                VoiceModeEvent::Stopped
            }
            zcosmic_voice_mode_v1::Event::Cancel => {
                info!("Voice mode cancelled");
                VoiceModeEvent::Cancelled
            }
            zcosmic_voice_mode_v1::Event::OrbAttached {
                x,
                y,
                width,
                height,
            } => {
                debug!("Voice orb attached: x={}, y={}, width={}, height={}", x, y, width, height);
                VoiceModeEvent::OrbAttached { x, y, width, height }
            }
            zcosmic_voice_mode_v1::Event::OrbDetached => {
                debug!("Voice orb detached");
                VoiceModeEvent::OrbDetached
            }
            zcosmic_voice_mode_v1::Event::WillStop { serial } => {
                // Immediately respond with cached voice active state
                // This avoids round-trip through iced's event loop
                let freeze = is_voice_active();
                info!("Voice mode will_stop, serial: {}, auto-responding with freeze: {}", serial, freeze);
                _proxy.ack_stop(serial, if freeze { 1 } else { 0 });
                VoiceModeEvent::WillStop { serial }
            }
        };
        
        state.voice_mode_event(voice_event);
    }
}
