//! Client side of `layer_size_transition_v1`: the compositor tells a layer
//! surface once when its size is heading somewhere over an animation.

use wayland_client::protocol::wl_surface::WlSurface;

pub use generated::{layer_size_transition_manager_v1, layer_size_transition_v1};

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
        wayland_scanner::generate_interfaces!("protocols/layer-size-transition.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/layer-size-transition.xml");
}

/// An animated change to the surface's arranged size, in output-logical px.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeTransition {
    /// The size goes from `from` to `to` over `duration_ms`; the surface keeps
    /// the larger of the two until then.
    Started {
        from: (i32, i32),
        to: (i32, i32),
        duration_ms: u32,
    },
    /// The surface has been configured to its final size.
    Finished { size: (i32, i32) },
}

#[derive(Debug, Clone)]
pub struct LayerSizeTransitionData {
    pub surface: WlSurface,
}
