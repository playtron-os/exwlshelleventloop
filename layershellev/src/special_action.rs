//! Client-side implementation of the special action protocol
//! (zcosmic_special_action_v1)
//!
//! The device's special key — the HUMAIN button — is a gesture the compositor
//! resolves, because it is usually bound to a modifier the compositor also
//! needs for its own chords. A surface registers as a receiver and is told the
//! meaning rather than the key.

use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, QueueHandle};

pub use generated::{zcosmic_special_action_manager_v1, zcosmic_special_action_v1};

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
        wayland_scanner::generate_interfaces!("protocols/special-action.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/special-action.xml");
}

/// A resolved gesture on the special key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialActionEvent {
    /// Tapped. Focus the surface's text input; no voice is involved.
    Activate,
    /// A hold began. Start capturing audio.
    HoldStart,
    /// The hold ended. Stop capturing and process what was captured.
    HoldEnd,
    /// The gesture was abandoned. Discard any capture rather than process it.
    Cancel,
}

/// User data for the manager. The manager itself has no events.
#[derive(Debug, Clone, Default)]
pub struct SpecialActionManagerData;

/// User data for a receiver — the surface it speaks for.
#[derive(Debug, Clone)]
pub struct SpecialActionData {
    pub surface: WlSurface,
}

impl<D>
    Dispatch<
        zcosmic_special_action_manager_v1::ZcosmicSpecialActionManagerV1,
        SpecialActionManagerData,
        D,
    > for ()
where
    D: Dispatch<
            zcosmic_special_action_manager_v1::ZcosmicSpecialActionManagerV1,
            SpecialActionManagerData,
        >,
{
    fn event(
        _state: &mut D,
        _proxy: &zcosmic_special_action_manager_v1::ZcosmicSpecialActionManagerV1,
        _event: zcosmic_special_action_manager_v1::Event,
        _data: &SpecialActionManagerData,
        _conn: &Connection,
        _qh: &QueueHandle<D>,
    ) {
        // The manager has no events.
    }
}

/// What a receiver's events are reported to.
pub trait SpecialActionHandler {
    fn special_action(&mut self, surface: &WlSurface, event: SpecialActionEvent);
}

impl<D> Dispatch<zcosmic_special_action_v1::ZcosmicSpecialActionV1, SpecialActionData, D> for ()
where
    D: Dispatch<zcosmic_special_action_v1::ZcosmicSpecialActionV1, SpecialActionData>
        + SpecialActionHandler,
{
    fn event(
        state: &mut D,
        _proxy: &zcosmic_special_action_v1::ZcosmicSpecialActionV1,
        event: zcosmic_special_action_v1::Event,
        data: &SpecialActionData,
        _conn: &Connection,
        _qh: &QueueHandle<D>,
    ) {
        let resolved = match event {
            zcosmic_special_action_v1::Event::Activate => SpecialActionEvent::Activate,
            zcosmic_special_action_v1::Event::HoldStart => SpecialActionEvent::HoldStart,
            zcosmic_special_action_v1::Event::HoldEnd => SpecialActionEvent::HoldEnd,
            zcosmic_special_action_v1::Event::Cancel => SpecialActionEvent::Cancel,
        };
        log::debug!("Special action event: {:?}", resolved);
        state.special_action(&data.surface, resolved);
    }
}
