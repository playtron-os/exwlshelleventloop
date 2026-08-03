//! Client-side implementation of the adaptive foreground protocol
//! (adaptive_foreground_manager_v1)
//!
//! Lets a surface mark zones of itself and be told how light the compositor
//! found the backdrop behind each one, so text drawn straight onto the wallpaper
//! can pick a colour that stays legible against it.
//!
//! The `Dispatch` impls live in `lib.rs` with the rest of `WindowState`'s, since
//! the surface object carries events and has to reach the message queue.

use wayland_client::protocol::wl_surface::WlSurface;

pub use generated::{adaptive_foreground_manager_v1, adaptive_foreground_surface_v1};

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
        wayland_scanner::generate_interfaces!("protocols/adaptive-foreground.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/adaptive-foreground.xml");
}

/// User data for adaptive foreground objects — stores the surface reference.
#[derive(Debug, Clone)]
pub struct AdaptiveForegroundData {
    pub surface: WlSurface,
}

/// Decode the protocol's 8.8 fixed-point luminance to 0.0..=1.0.
///
/// Clamped rather than merely divided: the wire type is a `uint`, so a
/// compositor that sent a value past the 256 the protocol defines as white
/// would otherwise hand the client a luminance above 1.0 and push every
/// contrast decision built on it the wrong way.
pub fn decode_luminance(fixed: u32) -> f32 {
    (fixed as f32 / 256.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::decode_luminance;

    #[test]
    fn decodes_the_documented_endpoints() {
        assert_eq!(decode_luminance(0), 0.0);
        assert_eq!(decode_luminance(256), 1.0);
        assert_eq!(decode_luminance(128), 0.5);
    }

    #[test]
    fn a_value_past_white_clamps_instead_of_exceeding_one() {
        assert_eq!(decode_luminance(u32::MAX), 1.0);
    }
}
