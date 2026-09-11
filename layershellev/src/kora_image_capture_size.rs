//! Client-side implementation of the capture-size protocol
//! (kora_image_capture_size_manager_v1)
//!
//! Asks the compositor to render captures of a source to fit within a size of
//! the client's choosing — a preview's — instead of the source's own, so a live
//! preview costs a preview's worth per frame. See the matching server protocol
//! in cosmic-comp (`resources/protocols/kora-image-capture-size.xml`).

// Re-export generated types
pub use generated::kora_image_capture_size_manager_v1;

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
    use wayland_protocols::ext::image_capture_source::v1::client::*;

    pub mod __interfaces {
        use wayland_backend;
        use wayland_client::protocol::__interfaces::*;
        use wayland_protocols::ext::image_capture_source::v1::client::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/kora-image-capture-size.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/kora-image-capture-size.xml");
}
