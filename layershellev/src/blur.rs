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
