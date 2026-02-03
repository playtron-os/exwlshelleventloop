//! Client-side implementation of the COSMIC layer surface visibility protocol
//!
//! This protocol allows layer-shell clients to hide and show their surfaces
//! without destroying them. This is useful for popup menus and panels that
//! need instant visibility toggling without surface creation/destruction overhead.

use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, QueueHandle};

// Re-export the generated protocol types
pub use generated::{
    zcosmic_layer_surface_visibility_manager_v1, zcosmic_layer_surface_visibility_v1,
};

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
        wayland_scanner::generate_interfaces!("protocols/layer-surface-visibility.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/layer-surface-visibility.xml");
}

/// User data for the manager
#[derive(Debug, Clone, Default)]
pub struct LayerSurfaceVisibilityManagerData;

/// User data for visibility controller - stores the surface reference
#[derive(Debug, Clone)]
pub struct LayerSurfaceVisibilityData {
    pub surface: WlSurface,
    pub hidden: bool,
}

/// Blanket implementation for visibility manager dispatch
impl<D>
    Dispatch<
        zcosmic_layer_surface_visibility_manager_v1::ZcosmicLayerSurfaceVisibilityManagerV1,
        LayerSurfaceVisibilityManagerData,
        D,
    > for ()
where
    D: Dispatch<
        zcosmic_layer_surface_visibility_manager_v1::ZcosmicLayerSurfaceVisibilityManagerV1,
        LayerSurfaceVisibilityManagerData,
    >,
{
    fn event(
        _state: &mut D,
        _proxy: &zcosmic_layer_surface_visibility_manager_v1::ZcosmicLayerSurfaceVisibilityManagerV1,
        _event: zcosmic_layer_surface_visibility_manager_v1::Event,
        _data: &LayerSurfaceVisibilityManagerData,
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        // No events for manager
    }
}

/// Blanket implementation for visibility controller dispatch
impl<D>
    Dispatch<
        zcosmic_layer_surface_visibility_v1::ZcosmicLayerSurfaceVisibilityV1,
        LayerSurfaceVisibilityData,
        D,
    > for ()
where
    D: Dispatch<
            zcosmic_layer_surface_visibility_v1::ZcosmicLayerSurfaceVisibilityV1,
            LayerSurfaceVisibilityData,
        > + LayerSurfaceVisibilityHandler,
{
    fn event(
        state: &mut D,
        _proxy: &zcosmic_layer_surface_visibility_v1::ZcosmicLayerSurfaceVisibilityV1,
        event: zcosmic_layer_surface_visibility_v1::Event,
        data: &LayerSurfaceVisibilityData,
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        let zcosmic_layer_surface_visibility_v1::Event::VisibilityChanged { visible } = event;
        let visible = visible != 0;
        state.visibility_changed(&data.surface, visible);
    }
}

/// Trait for handling layer surface visibility events
pub trait LayerSurfaceVisibilityHandler {
    /// Called when a surface's visibility changes (from compositor event)
    fn visibility_changed(&mut self, surface: &WlSurface, visible: bool);
}
