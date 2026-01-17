//! Client-side implementation of the COSMIC home visibility protocol (zcosmic_home_visibility_v1)
//!
//! This protocol allows layer-shell clients to control their visibility based on
//! whether the compositor is in "home" mode (no regular windows visible).

use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, QueueHandle};

// Re-export the generated protocol types
pub use generated::{zcosmic_home_visibility_manager_v1, zcosmic_home_visibility_v1};

// Re-export the visibility mode enum from the generated code
pub use zcosmic_home_visibility_v1::VisibilityMode;

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
        wayland_scanner::generate_interfaces!("protocols/home-visibility.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/home-visibility.xml");
}

/// User data for the manager - stores a callback for home state changes
#[derive(Debug, Clone)]
pub struct HomeVisibilityManagerData {
    /// Current home state (true = at home, false = windows visible)
    pub is_home: bool,
}

impl Default for HomeVisibilityManagerData {
    fn default() -> Self {
        Self { is_home: false }
    }
}

/// User data for visibility controller - stores the surface reference
#[derive(Debug, Clone)]
pub struct HomeVisibilityData {
    pub surface: WlSurface,
}

/// Blanket implementation for home visibility manager dispatch
impl<D>
    Dispatch<
        zcosmic_home_visibility_manager_v1::ZcosmicHomeVisibilityManagerV1,
        HomeVisibilityManagerData,
        D,
    > for ()
where
    D: Dispatch<
            zcosmic_home_visibility_manager_v1::ZcosmicHomeVisibilityManagerV1,
            HomeVisibilityManagerData,
        > + HomeVisibilityHandler,
{
    fn event(
        state: &mut D,
        _proxy: &zcosmic_home_visibility_manager_v1::ZcosmicHomeVisibilityManagerV1,
        event: zcosmic_home_visibility_manager_v1::Event,
        _data: &HomeVisibilityManagerData,
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        let zcosmic_home_visibility_manager_v1::Event::HomeState { is_home } = event;
        let is_home = is_home != 0;
        state.home_state_changed(is_home);
    }
}

/// Blanket implementation for home visibility controller dispatch
impl<D> Dispatch<zcosmic_home_visibility_v1::ZcosmicHomeVisibilityV1, HomeVisibilityData, D> for ()
where
    D: Dispatch<zcosmic_home_visibility_v1::ZcosmicHomeVisibilityV1, HomeVisibilityData>,
{
    fn event(
        _state: &mut D,
        _proxy: &zcosmic_home_visibility_v1::ZcosmicHomeVisibilityV1,
        _event: zcosmic_home_visibility_v1::Event,
        _data: &HomeVisibilityData,
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        // No events for controller objects
    }
}

/// Trait for handling home visibility events
pub trait HomeVisibilityHandler {
    /// Called when the compositor's home state changes
    fn home_state_changed(&mut self, is_home: bool);
}
