//! Client-side implementation of the layer surface placement protocol
//! (layer_surface_placement_manager_v1)
//!
//! Lets a layer shell surface ask the compositor to position it within its
//! output's usable (non-exclusive) area instead of computing margins itself and
//! applying them after the surface is already mapped (which causes a visible
//! jump when the surface first appears on an output). See the matching server
//! protocol in cosmic-comp (`resources/protocols/layer-surface-placement.xml`).

use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, QueueHandle};

// Re-export generated types
pub use generated::{layer_surface_placement_manager_v1, layer_surface_placement_v1};

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
        wayland_scanner::generate_interfaces!("protocols/layer-surface-placement.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/layer-surface-placement.xml");
}

/// User data for placement objects — stores the surface reference so the object
/// can be attributed to the right window.
#[derive(Debug, Clone)]
pub struct LayerSurfacePlacementData {
    pub surface: WlSurface,
}

/// Blanket implementation for the placement manager dispatch.
impl<D> Dispatch<layer_surface_placement_manager_v1::LayerSurfacePlacementManagerV1, (), D> for ()
where
    D: Dispatch<layer_surface_placement_manager_v1::LayerSurfacePlacementManagerV1, ()>,
{
    fn event(
        _state: &mut D,
        _proxy: &layer_surface_placement_manager_v1::LayerSurfacePlacementManagerV1,
        _event: layer_surface_placement_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        // No events for the placement manager.
    }
}

/// Blanket implementation for the per-surface placement object dispatch.
impl<D> Dispatch<layer_surface_placement_v1::LayerSurfacePlacementV1, LayerSurfacePlacementData, D>
    for ()
where
    D: Dispatch<layer_surface_placement_v1::LayerSurfacePlacementV1, LayerSurfacePlacementData>,
{
    fn event(
        _state: &mut D,
        _proxy: &layer_surface_placement_v1::LayerSurfacePlacementV1,
        _event: layer_surface_placement_v1::Event,
        _data: &LayerSurfacePlacementData,
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        // No events for placement objects.
    }
}
