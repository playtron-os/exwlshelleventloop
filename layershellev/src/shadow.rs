//! Client-side implementation of the layer shadow protocol (layer_shadow_manager_v1)
//!
//! This protocol allows clients to request shadow rendering for layer shell surfaces.

use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, QueueHandle};

// Re-export only the actual code
pub use generated::{layer_shadow_manager_v1, layer_shadow_surface_v1};

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
        wayland_scanner::generate_interfaces!("protocols/layer-shadow.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/layer-shadow.xml");
}

/// User data for shadow surface objects - stores the surface reference
#[derive(Debug, Clone)]
pub struct ShadowData {
    pub surface: WlSurface,
}

/// Blanket implementation for shadow manager dispatch
impl<D> Dispatch<layer_shadow_manager_v1::LayerShadowManagerV1, (), D> for ()
where
    D: Dispatch<layer_shadow_manager_v1::LayerShadowManagerV1, ()>,
{
    fn event(
        _state: &mut D,
        _proxy: &layer_shadow_manager_v1::LayerShadowManagerV1,
        _event: layer_shadow_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        // No events for shadow manager
    }
}

/// Blanket implementation for shadow surface dispatch
impl<D> Dispatch<layer_shadow_surface_v1::LayerShadowSurfaceV1, ShadowData, D> for ()
where
    D: Dispatch<layer_shadow_surface_v1::LayerShadowSurfaceV1, ShadowData>,
{
    fn event(
        _state: &mut D,
        _proxy: &layer_shadow_surface_v1::LayerShadowSurfaceV1,
        _event: layer_shadow_surface_v1::Event,
        _data: &ShadowData,
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        // No events for shadow surface objects
    }
}
