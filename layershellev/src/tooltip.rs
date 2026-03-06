//! Client-side implementation of the compositor-driven tooltip protocol
//! (zcosmic_tooltip_manager_v1)
//!
//! This protocol allows clients to create tooltip surfaces that the compositor
//! positions relative to the pointer cursor automatically, eliminating
//! client-side reposition round-trip latency.

use wayland_client::protocol::wl_surface::WlSurface;

pub use generated::{zcosmic_tooltip_manager_v1, zcosmic_tooltip_v1};

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
        wayland_scanner::generate_interfaces!("protocols/tooltip.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/tooltip.xml");
}

/// User data for tooltip objects - stores the tooltip surface reference
#[derive(Debug, Clone)]
pub struct TooltipData {
    pub tooltip_surface: WlSurface,
    pub parent_surface: WlSurface,
}
