//! Client-side implementation of the COSMIC layer surface dismiss protocol
//!
//! This protocol allows layer-shell clients to receive notifications when
//! user interaction occurs outside their surfaces. This enables "close on
//! click outside" behavior for popup menus without changing keyboard focus.

use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, QueueHandle};

// Re-export the generated protocol types
pub use generated::{zcosmic_layer_surface_dismiss_manager_v1, zcosmic_layer_surface_dismiss_v1};

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
        wayland_scanner::generate_interfaces!("protocols/layer-surface-dismiss.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/layer-surface-dismiss.xml");
}

/// User data for the manager
#[derive(Debug, Clone, Default)]
pub struct LayerSurfaceDismissManagerData;

/// User data for dismiss controller - stores the surface reference
#[derive(Debug, Clone)]
pub struct LayerSurfaceDismissData {
    pub surface: WlSurface,
}

/// Blanket implementation for dismiss manager dispatch
impl<D>
    Dispatch<
        zcosmic_layer_surface_dismiss_manager_v1::ZcosmicLayerSurfaceDismissManagerV1,
        LayerSurfaceDismissManagerData,
        D,
    > for ()
where
    D: Dispatch<
        zcosmic_layer_surface_dismiss_manager_v1::ZcosmicLayerSurfaceDismissManagerV1,
        LayerSurfaceDismissManagerData,
    >,
{
    fn event(
        _state: &mut D,
        _proxy: &zcosmic_layer_surface_dismiss_manager_v1::ZcosmicLayerSurfaceDismissManagerV1,
        _event: zcosmic_layer_surface_dismiss_manager_v1::Event,
        _data: &LayerSurfaceDismissManagerData,
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        // No events for manager
    }
}

/// Blanket implementation for dismiss controller dispatch
impl<D>
    Dispatch<
        zcosmic_layer_surface_dismiss_v1::ZcosmicLayerSurfaceDismissV1,
        LayerSurfaceDismissData,
        D,
    > for ()
where
    D: Dispatch<
            zcosmic_layer_surface_dismiss_v1::ZcosmicLayerSurfaceDismissV1,
            LayerSurfaceDismissData,
        > + LayerSurfaceDismissHandler,
{
    fn event(
        state: &mut D,
        _proxy: &zcosmic_layer_surface_dismiss_v1::ZcosmicLayerSurfaceDismissV1,
        event: zcosmic_layer_surface_dismiss_v1::Event,
        data: &LayerSurfaceDismissData,
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        let zcosmic_layer_surface_dismiss_v1::Event::DismissRequested = event;
        state.dismiss_requested(&data.surface);
    }
}

/// Trait for handling layer surface dismiss events
pub trait LayerSurfaceDismissHandler {
    /// Called when a dismiss is requested for a surface
    /// (user clicked/touched outside the dismiss group while armed)
    fn dismiss_requested(&mut self, surface: &WlSurface);
}
