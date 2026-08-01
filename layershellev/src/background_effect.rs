//! Client-side implementation of the background effect protocol
//! (`ext_background_effect_v1`).
//!
//! Version 1 is the upstream staging protocol: the client marks a region and the
//! compositor decides everything else. Version 2 additionally carries a blur
//! strength, per-area corner radii, and a whole-surface mode that the compositor
//! keeps matched as the surface resizes.
//!
//! This does not replace the KDE blur protocol. Compositors commonly implement
//! only one of the two, so a client should bind both and set both; whichever the
//! compositor understands takes effect.

use wayland_client::protocol::wl_region::WlRegion;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, QueueHandle};

// Re-export only the actual code
pub use generated::{ext_background_effect_manager_v1, ext_background_effect_surface_v1};

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
        wayland_scanner::generate_interfaces!("protocols/ext-background-effect-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/ext-background-effect-v1.xml");
}

use ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1;
use ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1;

/// The version at which the strength, corner and whole-surface requests appear.
pub const VERSION_WITH_RADIUS: u32 = 2;
/// The version at which the frosted-glass appearance requests appear
/// (saturation, tint, border).
pub const VERSION_WITH_APPEARANCE: u32 = 3;

/// User data for background effect objects - stores the surface reference
#[derive(Debug, Clone)]
pub struct BackgroundEffectData {
    pub surface: WlSurface,
}

/// Applies blur state to a surface's effect object.
///
/// The region is in surface-local coordinates. Passing `None` removes the
/// effect. Everything here is double-buffered by the protocol, so it lands with
/// the next surface commit rather than immediately.
///
/// The strength and corner radii are only sent on version 2, so a version 1
/// compositor silently gets the region alone rather than a protocol error.
#[allow(clippy::too_many_arguments)]
pub fn apply(
    effect: &ExtBackgroundEffectSurfaceV1,
    version: u32,
    region: Option<&WlRegion>,
    whole_surface: bool,
    radius: Option<u32>,
    region_radii: &[[u32; 4]],
    saturation: Option<f64>,
    tint: Option<f64>,
    border: Option<f64>,
) {
    if whole_surface && version >= VERSION_WITH_RADIUS {
        // Lets the compositor keep the blurred area matched to the surface. A
        // client would otherwise have to recompute and resend a region on every
        // resize, and would flash unblurred whenever it lost that race.
        effect.blur_whole_surface();
    } else {
        effect.set_blur_region(region);
    }

    if version >= VERSION_WITH_RADIUS {
        if let Some(radius) = radius {
            effect.set_blur_radius(radius);
        }
        if !region_radii.is_empty() {
            effect.set_region_radii(encode_region_radii(region_radii));
        }
    }

    // Appearance. Each is only sent when the caller asked for it: there is no
    // "reset to default" request, because 0 is a real value for all three
    // (greyscale, no tint, no border) and cannot double as "unset".
    if version >= VERSION_WITH_APPEARANCE {
        if let Some(saturation) = saturation {
            effect.set_saturation(saturation);
        }
        if let Some(tint) = tint {
            effect.set_tint(tint);
        }
        if let Some(border) = border {
            effect.set_border(border);
        }
    }
}

/// Encode per-rect corner radii for the wire: four native-endian u32 per rect,
/// clockwise from top-left, index-matched to the region's rectangles.
pub fn encode_region_radii(radii: &[[u32; 4]]) -> Vec<u8> {
    radii
        .iter()
        .flat_map(|corners| corners.iter().flat_map(|r| r.to_ne_bytes()))
        .collect()
}

/// Blanket implementation for background effect manager dispatch
impl<D> Dispatch<ExtBackgroundEffectManagerV1, (), D> for ()
where
    D: Dispatch<ExtBackgroundEffectManagerV1, ()>,
{
    fn event(
        _state: &mut D,
        _proxy: &ExtBackgroundEffectManagerV1,
        _event: <ExtBackgroundEffectManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        // The only event is `capabilities`, and blur is the only capability the
        // protocol defines, so there is nothing to branch on yet.
    }
}

/// Blanket implementation for background effect surface dispatch
impl<D> Dispatch<ExtBackgroundEffectSurfaceV1, BackgroundEffectData, D> for ()
where
    D: Dispatch<ExtBackgroundEffectSurfaceV1, BackgroundEffectData>,
{
    fn event(
        _state: &mut D,
        _proxy: &ExtBackgroundEffectSurfaceV1,
        _event: <ExtBackgroundEffectSurfaceV1 as wayland_client::Proxy>::Event,
        _data: &BackgroundEffectData,
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        // This object has no events.
    }
}
