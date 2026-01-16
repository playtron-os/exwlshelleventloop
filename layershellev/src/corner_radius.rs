//! Client-side implementation of the layer corner radius protocol (layer_corner_radius_manager_v1)
//!
//! This protocol allows clients to specify corner radius for layer shell surfaces.

use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, QueueHandle};

// Re-export only the actual code
pub use generated::{layer_corner_radius_manager_v1, layer_corner_radius_surface_v1};

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
        wayland_scanner::generate_interfaces!("protocols/corner-radius.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/corner-radius.xml");
}

/// User data for corner radius surface objects - stores the surface reference
#[derive(Debug, Clone)]
pub struct CornerRadiusData {
    pub surface: WlSurface,
}

/// Blanket implementation for corner radius manager dispatch
impl<D> Dispatch<layer_corner_radius_manager_v1::LayerCornerRadiusManagerV1, (), D> for ()
where
    D: Dispatch<layer_corner_radius_manager_v1::LayerCornerRadiusManagerV1, ()>,
{
    fn event(
        _state: &mut D,
        _proxy: &layer_corner_radius_manager_v1::LayerCornerRadiusManagerV1,
        _event: layer_corner_radius_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        // No events for corner radius manager
    }
}

/// Blanket implementation for corner radius surface dispatch
impl<D> Dispatch<layer_corner_radius_surface_v1::LayerCornerRadiusSurfaceV1, CornerRadiusData, D>
    for ()
where
    D: Dispatch<layer_corner_radius_surface_v1::LayerCornerRadiusSurfaceV1, CornerRadiusData>,
{
    fn event(
        _state: &mut D,
        _proxy: &layer_corner_radius_surface_v1::LayerCornerRadiusSurfaceV1,
        _event: layer_corner_radius_surface_v1::Event,
        _data: &CornerRadiusData,
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        // No events for corner radius surface objects
    }
}
