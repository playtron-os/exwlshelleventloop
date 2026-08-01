//! Client-side implementation of the background effect protocol
//! (`ext_background_effect_v1`).
//!
//! This is upstream's staging protocol: the client marks a region and the
//! compositor decides everything else -- there is no blur strength, no corner
//! rounding, no frosted-glass appearance and no whole-surface mode.
//!
//! It therefore does not replace the KDE blur protocol, which carries all of
//! those. Compositors commonly implement only one of the two, so a client
//! should bind both and set both; whichever the compositor understands takes
//! effect.

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

/// Stands in for the whole surface. The protocol has no whole-surface request,
/// so "everything" is an oversized region and the compositor clips it to the
/// surface -- which keeps it correct across resizes without a resend.
pub const WHOLE_SURFACE: (i32, i32, i32, i32) = (0, 0, i32::MAX, i32::MAX);

/// User data for background effect objects - stores the surface reference
#[derive(Debug, Clone)]
pub struct BackgroundEffectData {
    pub surface: WlSurface,
}

/// Applies blur state to a surface's effect object.
///
/// The region is in surface-local coordinates and is the only thing this
/// protocol carries; strength, corner rounding and appearance go over the KDE
/// blur protocol instead. Passing `None` removes the effect. The state is
/// double-buffered, so it lands with the next surface commit.
pub fn apply(effect: &ExtBackgroundEffectSurfaceV1, region: Option<&WlRegion>) {
    effect.set_blur_region(region);
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
