//! Client-side implementation of the KDE blur protocol (org_kde_kwin_blur_manager)
//!
//! This protocol allows clients to request blur effects for surfaces.

use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, QueueHandle};

// Re-export only the actual code
pub use generated::{org_kde_kwin_blur, org_kde_kwin_blur_manager};

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
        wayland_scanner::generate_interfaces!("protocols/blur.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/blur.xml");
}

/// User data for blur objects - stores the surface reference
#[derive(Debug, Clone)]
pub struct BlurData {
    pub surface: WlSurface,
}

/// Blanket implementation for blur manager dispatch
impl<D> Dispatch<org_kde_kwin_blur_manager::OrgKdeKwinBlurManager, (), D> for ()
where
    D: Dispatch<org_kde_kwin_blur_manager::OrgKdeKwinBlurManager, ()>,
{
    fn event(
        _state: &mut D,
        _proxy: &org_kde_kwin_blur_manager::OrgKdeKwinBlurManager,
        _event: org_kde_kwin_blur_manager::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        // No events for blur manager
    }
}

/// Blanket implementation for blur object dispatch
impl<D> Dispatch<org_kde_kwin_blur::OrgKdeKwinBlur, BlurData, D> for ()
where
    D: Dispatch<org_kde_kwin_blur::OrgKdeKwinBlur, BlurData>,
{
    fn event(
        _state: &mut D,
        _proxy: &org_kde_kwin_blur::OrgKdeKwinBlur,
        _event: org_kde_kwin_blur::Event,
        _data: &BlurData,
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        // No events for blur objects
    }
}

/// Encode per-rect corner radii for `set_region_radii` (blur v4): four
/// native-endian `u32`s per rect — top-left, top-right, bottom-right,
/// bottom-left — in the same order the rects were added to the region.
pub fn encode_region_radii(radii: &[[u32; 4]]) -> Vec<u8> {
    radii
        .iter()
        .flat_map(|corners| corners.iter().flat_map(|r| r.to_ne_bytes()))
        .collect()
}

/// Encode per-rect exact geometry for `set_region_geometry` (blur v5): four
/// native-endian `i32`s per rect — x, y, width, height in surface-local logical
/// px as fixed-point with 8 fractional bits — in the same order the rects were
/// added to the region.
///
/// This is what lets a surface at a fractional position, or one under a scale
/// animation, get a backdrop that matches the shape it draws. The wl_region
/// rects stay whole pixels and remain the conservative bound.
pub fn encode_region_geometry(geometry: &[(f32, f32, f32, f32)]) -> Vec<u8> {
    let fixed = |v: f32| ((v * 256.0).round() as i32).to_ne_bytes();
    geometry
        .iter()
        .flat_map(|(x, y, w, h)| {
            let mut quad = [0u8; 16];
            quad[0..4].copy_from_slice(&fixed(*x));
            quad[4..8].copy_from_slice(&fixed(*y));
            quad[8..12].copy_from_slice(&fixed(*w));
            quad[12..16].copy_from_slice(&fixed(*h));
            quad
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{encode_region_geometry, encode_region_radii};

    #[test]
    fn geometry_round_trips_through_the_fixed_point_encoding() {
        // The pill that started this: 822.3 x 19.54, 275.4 x 46.92. 8 fractional
        // bits resolve ~0.004px, far below anything visible at any scale.
        let bytes = encode_region_geometry(&[(822.3, 19.54, 275.4, 46.92)]);
        assert_eq!(bytes.len(), 16);

        let round_trip: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|b| i32::from_ne_bytes([b[0], b[1], b[2], b[3]]) as f32 / 256.0)
            .collect();

        for (got, want) in round_trip.iter().zip([822.3, 19.54, 275.4, 46.92]) {
            assert!((got - want).abs() < 0.01, "{got} != {want}");
        }
    }

    #[test]
    fn negative_positions_survive_the_encoding() {
        // x and y are signed: a rect may start left of or above the surface
        // origin. Encoding them unsigned would wrap to an enormous positive.
        let bytes = encode_region_geometry(&[(-4.5, -0.25, 10.0, 10.0)]);
        let x = i32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f32 / 256.0;
        let y = i32::from_ne_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as f32 / 256.0;
        assert_eq!((x, y), (-4.5, -0.25));
    }

    #[test]
    fn no_geometry_encodes_to_an_empty_array() {
        assert!(encode_region_geometry(&[]).is_empty());
    }

    #[test]
    fn encodes_four_native_endian_u32_per_rect() {
        let bytes = encode_region_radii(&[[16, 16, 16, 16], [1, 2, 3, 4]]);
        // The compositor reads this back in 16-byte chunks, so the length must be
        // exactly 4 u32 per rect or every later rect's radii shift.
        assert_eq!(bytes.len(), 2 * 4 * 4);
        let round_trip: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|b| u32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        assert_eq!(round_trip, vec![16, 16, 16, 16, 1, 2, 3, 4]);
    }

    #[test]
    fn no_radii_encodes_to_an_empty_array() {
        assert!(encode_region_radii(&[]).is_empty());
    }
}
