//! Client side of `kora_capture_exclusion_v1`: a surface drawn on screen but left out of
//! screen captures, such as a panel during a screenshot taken from it.

pub use generated::{kora_capture_exclusion_manager_v1, kora_surface_capture_exclusion_v1};

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
        wayland_scanner::generate_interfaces!("protocols/kora-capture-exclusion.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/kora-capture-exclusion.xml");
}
