//! Client-side implementation of the layer edge-resize protocol
//! (layer_edge_resize_manager_v1)
//!
//! Lets a layer-shell surface opt into a compositor-drawn, VSCode-style edge
//! resize sash on its outer (non-anchored) edge, supplying width bounds. The
//! resize itself is fully compositor-driven; this client only creates the object
//! and sets min/max width. See the matching server protocol in cosmic-comp
//! (`resources/protocols/layer-edge-resize.xml`).

use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, QueueHandle};

// Re-export generated types
pub use generated::{layer_edge_resize_manager_v1, layer_edge_resize_v1};

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
        wayland_scanner::generate_interfaces!("protocols/layer-edge-resize.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/layer-edge-resize.xml");
}

/// User data for edge-resize objects — stores the surface reference so the object
/// can be attributed to the right window.
#[derive(Debug, Clone)]
pub struct LayerEdgeResizeData {
    pub surface: WlSurface,
}

/// Blanket implementation for the edge-resize manager dispatch.
impl<D> Dispatch<layer_edge_resize_manager_v1::LayerEdgeResizeManagerV1, (), D> for ()
where
    D: Dispatch<layer_edge_resize_manager_v1::LayerEdgeResizeManagerV1, ()>,
{
    fn event(
        _state: &mut D,
        _proxy: &layer_edge_resize_manager_v1::LayerEdgeResizeManagerV1,
        _event: layer_edge_resize_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        // No events for the edge-resize manager.
    }
}

/// Blanket implementation for the per-surface edge-resize object dispatch.
impl<D> Dispatch<layer_edge_resize_v1::LayerEdgeResizeV1, LayerEdgeResizeData, D> for ()
where
    D: Dispatch<layer_edge_resize_v1::LayerEdgeResizeV1, LayerEdgeResizeData>,
{
    fn event(
        _state: &mut D,
        _proxy: &layer_edge_resize_v1::LayerEdgeResizeV1,
        _event: layer_edge_resize_v1::Event,
        _data: &LayerEdgeResizeData,
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        // No events for edge-resize objects.
    }
}
