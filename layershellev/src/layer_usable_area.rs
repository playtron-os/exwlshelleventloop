//! Client-side implementation of the layer usable-area protocol
//! (layer_usable_area_manager_v1)
//!
//! The compositor reports the usable (non-exclusive) area of the output a layer
//! surface is shown on — the output logical geometry minus every exclusive zone
//! reserved by panels/docks. Centered overlays use it to position within the
//! free space rather than the full output. See the matching server protocol in
//! cosmic-comp (`resources/protocols/layer-usable-area.xml`).

use wayland_client::protocol::wl_surface::WlSurface;

// Re-export generated types
pub use generated::{layer_usable_area_manager_v1, layer_usable_area_v1};

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
        wayland_scanner::generate_interfaces!("protocols/layer-usable-area.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/layer-usable-area.xml");
}

/// User data for usable-area objects — stores the surface reference so an
/// incoming `usable_area` event can be attributed to the right window.
#[derive(Debug, Clone)]
pub struct LayerUsableAreaData {
    pub surface: WlSurface,
}
