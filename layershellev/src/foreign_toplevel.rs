//! Foreign toplevel management client
//!
//! This module provides client-side support for tracking opened application windows
//! using Wayland protocols. It supports:
//!
//! - `ext_foreign_toplevel_list_v1` (standard Wayland protocol, preferred)
//! - `zcosmic_toplevel_info_v1` (COSMIC extension for state info like minimized/maximized)
//! - `zwlr_foreign_toplevel_manager_v1` (wlroots fallback)
//!
//! Use this to create taskbars and docks that need to track open windows.

use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};

// ext_foreign_toplevel_list_v1 protocol (standard, minimal)
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::{self, ExtForeignToplevelHandleV1},
    ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
};

// COSMIC toplevel info extension (for state information)
#[cfg(feature = "cosmic-toplevel")]
use cosmic_protocols::toplevel_info::v1::client::{
    zcosmic_toplevel_handle_v1::{self, ZcosmicToplevelHandleV1},
    zcosmic_toplevel_info_v1::{self, ZcosmicToplevelInfoV1},
};

// wlroots fallback protocol
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

/// Information about a foreign toplevel window
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToplevelInfo {
    /// Unique identifier for this toplevel (object ID)
    pub id: u32,
    /// Window title
    pub title: String,
    /// Application ID (app_id)
    pub app_id: String,
    /// Whether the window is currently active/focused
    pub is_activated: bool,
    /// Whether the window is maximized
    pub is_maximized: bool,
    /// Whether the window is minimized
    pub is_minimized: bool,
    /// Whether the window is fullscreen
    pub is_fullscreen: bool,
}

impl ToplevelInfo {
    /// Get a display name for this toplevel
    pub fn display_name(&self) -> &str {
        if self.title.is_empty() {
            &self.app_id
        } else {
            &self.title
        }
    }
}

/// Events from the foreign toplevel manager
#[derive(Debug, Clone)]
pub enum ForeignToplevelEvent {
    /// A new toplevel was created
    Created(ToplevelInfo),
    /// A toplevel's info was updated (title, app_id, or state changed)
    Changed(ToplevelInfo),
    /// A toplevel was closed
    Closed(u32),
    /// The manager has finished (compositor no longer sending events)
    Finished,
}

/// Internal state for a toplevel handle
#[derive(Debug, Default, Clone)]
pub(crate) struct ToplevelHandleData {
    pub title: String,
    pub app_id: String,
    pub is_activated: bool,
    pub is_maximized: bool,
    pub is_minimized: bool,
    pub is_fullscreen: bool,
    /// Whether initial properties have been received (done event received)
    pub initialized: bool,
}

impl ToplevelHandleData {
    pub fn to_info(&self, id: u32) -> ToplevelInfo {
        ToplevelInfo {
            id,
            title: self.title.clone(),
            app_id: self.app_id.clone(),
            is_activated: self.is_activated,
            is_maximized: self.is_maximized,
            is_minimized: self.is_minimized,
            is_fullscreen: self.is_fullscreen,
        }
    }
}

/// User data for the manager - empty, events go through the handler trait
#[derive(Debug, Clone, Default)]
pub struct ForeignToplevelManagerData;

/// User data for toplevel handles - just tracks the object ID
#[derive(Debug, Clone, Default)]
pub struct ToplevelHandleUserData;

/// Trait for handling foreign toplevel events
#[allow(private_interfaces)]
pub trait ForeignToplevelHandler {
    /// Called when a toplevel event occurs
    fn foreign_toplevel_event(&mut self, event: ForeignToplevelEvent);

    /// Get the pending handle data for a toplevel ID (internal use)
    fn get_toplevel_data(&mut self, id: u32) -> &mut ToplevelHandleData;

    /// Remove toplevel data (internal use)
    fn remove_toplevel_data(&mut self, id: u32);
}

/// Blanket implementation for foreign toplevel manager dispatch
impl<D> Dispatch<ZwlrForeignToplevelManagerV1, ForeignToplevelManagerData, D> for ()
where
    D: Dispatch<ZwlrForeignToplevelManagerV1, ForeignToplevelManagerData>
        + Dispatch<ZwlrForeignToplevelHandleV1, ToplevelHandleUserData>
        + ForeignToplevelHandler
        + 'static,
{
    fn event(
        state: &mut D,
        _proxy: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _data: &ForeignToplevelManagerData,
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        match event {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
                // Get the object ID for this toplevel handle
                let id = toplevel.id().protocol_id();
                // Initialize empty state for this toplevel
                let _ = state.get_toplevel_data(id);
            }
            zwlr_foreign_toplevel_manager_v1::Event::Finished => {
                state.foreign_toplevel_event(ForeignToplevelEvent::Finished);
            }
            _ => {}
        }
    }

    fn event_created_child(
        opcode: u16,
        qhandle: &QueueHandle<D>,
    ) -> std::sync::Arc<dyn wayland_client::backend::ObjectData> {
        match opcode {
            // toplevel event (opcode 0)
            0 => qhandle.make_data::<ZwlrForeignToplevelHandleV1, _>(ToplevelHandleUserData),
            _ => panic!("Unknown opcode in event_created_child: {}", opcode),
        }
    }
}

/// Blanket implementation for toplevel handle dispatch
impl<D> Dispatch<ZwlrForeignToplevelHandleV1, ToplevelHandleUserData, D> for ()
where
    D: Dispatch<ZwlrForeignToplevelHandleV1, ToplevelHandleUserData> + ForeignToplevelHandler,
{
    fn event(
        state: &mut D,
        proxy: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _data: &ToplevelHandleUserData,
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        // Use the protocol ID as our identifier
        let id = proxy.id().protocol_id();

        match event {
            zwlr_foreign_toplevel_handle_v1::Event::Title { title } => {
                state.get_toplevel_data(id).title = title;
            }
            zwlr_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                state.get_toplevel_data(id).app_id = app_id;
            }
            zwlr_foreign_toplevel_handle_v1::Event::State {
                state: states_array,
            } => {
                let handle_data = state.get_toplevel_data(id);
                // Reset all states first
                handle_data.is_activated = false;
                handle_data.is_maximized = false;
                handle_data.is_minimized = false;
                handle_data.is_fullscreen = false;

                // Parse state array - it's an array of u32 values
                let bytes: &[u8] = states_array.as_ref();
                for chunk in bytes.chunks_exact(4) {
                    if let Ok(arr) = <[u8; 4]>::try_from(chunk) {
                        let value = u32::from_ne_bytes(arr);
                        match value {
                            0 => handle_data.is_maximized = true,
                            1 => handle_data.is_minimized = true,
                            2 => handle_data.is_activated = true,
                            3 => handle_data.is_fullscreen = true,
                            _ => {}
                        }
                    }
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Done => {
                let handle_data = state.get_toplevel_data(id);
                let info = handle_data.to_info(id);

                if handle_data.initialized {
                    // Update existing toplevel
                    state.foreign_toplevel_event(ForeignToplevelEvent::Changed(info));
                } else {
                    // New toplevel - first done event
                    handle_data.initialized = true;
                    state.foreign_toplevel_event(ForeignToplevelEvent::Created(info));
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Closed => {
                let info = state.get_toplevel_data(id).to_info(id);
                state.remove_toplevel_data(id);
                state.foreign_toplevel_event(ForeignToplevelEvent::Closed(info.id));
                // Destroy the handle
                proxy.destroy();
            }
            zwlr_foreign_toplevel_handle_v1::Event::OutputEnter { .. } => {
                // Could track which outputs the toplevel is on
            }
            zwlr_foreign_toplevel_handle_v1::Event::OutputLeave { .. } => {
                // Could track which outputs the toplevel is on
            }
            zwlr_foreign_toplevel_handle_v1::Event::Parent { .. } => {
                // Could track parent-child relationships
            }
            _ => {}
        }
    }
}

// ============================================================================
// ext_foreign_toplevel_list_v1 protocol (standard Wayland, preferred)
// ============================================================================

/// User data for ext_foreign_toplevel_list_v1
#[derive(Debug, Clone, Default)]
pub struct ExtForeignToplevelListData;

/// User data for ext_foreign_toplevel_handle_v1
#[derive(Debug, Clone, Default)]
pub struct ExtToplevelHandleData;

/// Dispatch for ext_foreign_toplevel_list_v1
impl<D> Dispatch<ExtForeignToplevelListV1, ExtForeignToplevelListData, D> for ()
where
    D: Dispatch<ExtForeignToplevelListV1, ExtForeignToplevelListData>
        + Dispatch<ExtForeignToplevelHandleV1, ExtToplevelHandleData>
        + ForeignToplevelHandler
        + 'static,
{
    fn event(
        state: &mut D,
        _proxy: &ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _data: &ExtForeignToplevelListData,
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        match event {
            ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } => {
                let id = toplevel.id().protocol_id();
                log::trace!("ext_foreign_toplevel_list: new toplevel handle id={}", id);
                // Initialize empty state for this toplevel
                let _ = state.get_toplevel_data(id);
            }
            ext_foreign_toplevel_list_v1::Event::Finished => {
                log::trace!("ext_foreign_toplevel_list: finished");
                state.foreign_toplevel_event(ForeignToplevelEvent::Finished);
            }
            _ => {}
        }
    }

    fn event_created_child(
        opcode: u16,
        qhandle: &QueueHandle<D>,
    ) -> std::sync::Arc<dyn wayland_client::backend::ObjectData> {
        match opcode {
            // toplevel event (opcode 0)
            0 => qhandle.make_data::<ExtForeignToplevelHandleV1, _>(ExtToplevelHandleData),
            _ => panic!(
                "Unknown ext toplevel opcode in event_created_child: {}",
                opcode
            ),
        }
    }
}

/// Dispatch for ext_foreign_toplevel_handle_v1
impl<D> Dispatch<ExtForeignToplevelHandleV1, ExtToplevelHandleData, D> for ()
where
    D: Dispatch<ExtForeignToplevelHandleV1, ExtToplevelHandleData> + ForeignToplevelHandler,
{
    fn event(
        state: &mut D,
        proxy: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _data: &ExtToplevelHandleData,
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        let id = proxy.id().protocol_id();

        match event {
            ext_foreign_toplevel_handle_v1::Event::Title { title } => {
                log::trace!("ext_foreign_toplevel_handle {}: title={}", id, title);
                state.get_toplevel_data(id).title = title;
            }
            ext_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                log::trace!("ext_foreign_toplevel_handle {}: app_id={}", id, app_id);
                state.get_toplevel_data(id).app_id = app_id;
            }
            ext_foreign_toplevel_handle_v1::Event::Identifier { identifier } => {
                // ext protocol uses identifier string instead of tracking state
                log::trace!(
                    "ext_foreign_toplevel_handle {}: identifier={}",
                    id,
                    identifier
                );
            }
            ext_foreign_toplevel_handle_v1::Event::Done => {
                let handle_data = state.get_toplevel_data(id);
                let info = handle_data.to_info(id);
                log::trace!(
                    "ext_foreign_toplevel_handle {}: done, title={}, app_id={}, initialized={}",
                    id,
                    info.title,
                    info.app_id,
                    handle_data.initialized
                );

                if handle_data.initialized {
                    state.foreign_toplevel_event(ForeignToplevelEvent::Changed(info));
                } else {
                    handle_data.initialized = true;
                    state.foreign_toplevel_event(ForeignToplevelEvent::Created(info));
                }
            }
            ext_foreign_toplevel_handle_v1::Event::Closed => {
                let info = state.get_toplevel_data(id).to_info(id);
                state.remove_toplevel_data(id);
                state.foreign_toplevel_event(ForeignToplevelEvent::Closed(info.id));
                proxy.destroy();
            }
            _ => {}
        }
    }
}

// ============================================================================
// zcosmic_toplevel_info_v1 protocol (COSMIC extension for state info)
// ============================================================================

#[cfg(feature = "cosmic-toplevel")]
/// User data for zcosmic_toplevel_info_v1
#[derive(Debug, Clone, Default)]
pub struct CosmicToplevelInfoData;

#[cfg(feature = "cosmic-toplevel")]
/// User data for zcosmic_toplevel_handle_v1 - tracks the ext handle ID this extends
#[derive(Debug, Clone)]
pub struct CosmicToplevelHandleData {
    /// The ext_foreign_toplevel_handle_v1 protocol ID this cosmic handle extends
    pub ext_handle_id: u32,
}

#[cfg(feature = "cosmic-toplevel")]
/// Extended trait for COSMIC toplevel state management
pub trait CosmicToplevelHandler: ForeignToplevelHandler {
    /// Get the COSMIC toplevel info manager (for extending ext handles)
    fn cosmic_toplevel_info(&self) -> Option<&ZcosmicToplevelInfoV1>;

    /// Store mapping from cosmic handle ID to ext handle ID
    fn set_cosmic_handle_mapping(&mut self, cosmic_id: u32, ext_id: u32);

    /// Get the ext handle ID for a cosmic handle ID
    fn get_ext_handle_id(&self, cosmic_id: u32) -> Option<u32>;
}

#[cfg(feature = "cosmic-toplevel")]
/// Dispatch for zcosmic_toplevel_info_v1
impl<D> Dispatch<ZcosmicToplevelInfoV1, CosmicToplevelInfoData, D> for ()
where
    D: Dispatch<ZcosmicToplevelInfoV1, CosmicToplevelInfoData>
        + Dispatch<ZcosmicToplevelHandleV1, CosmicToplevelHandleData>
        + CosmicToplevelHandler
        + 'static,
{
    fn event(
        _state: &mut D,
        _proxy: &ZcosmicToplevelInfoV1,
        event: zcosmic_toplevel_info_v1::Event,
        _data: &CosmicToplevelInfoData,
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        match event {
            zcosmic_toplevel_info_v1::Event::Finished => {
                log::debug!("COSMIC toplevel info finished");
            }
            _ => {}
        }
    }
}

#[cfg(feature = "cosmic-toplevel")]
/// Dispatch for zcosmic_toplevel_handle_v1
impl<D> Dispatch<ZcosmicToplevelHandleV1, CosmicToplevelHandleData, D> for ()
where
    D: Dispatch<ZcosmicToplevelHandleV1, CosmicToplevelHandleData> + CosmicToplevelHandler,
{
    fn event(
        state: &mut D,
        proxy: &ZcosmicToplevelHandleV1,
        event: zcosmic_toplevel_handle_v1::Event,
        data: &CosmicToplevelHandleData,
        _conn: &Connection,
        _qhandle: &QueueHandle<D>,
    ) {
        // Get the ext handle ID that this cosmic handle extends
        let ext_id = data.ext_handle_id;

        match event {
            zcosmic_toplevel_handle_v1::Event::State {
                state: states_array,
            } => {
                let handle_data = state.get_toplevel_data(ext_id);
                // Reset all states first
                handle_data.is_activated = false;
                handle_data.is_maximized = false;
                handle_data.is_minimized = false;
                handle_data.is_fullscreen = false;

                // Parse state array - array of u32 values
                // COSMIC state enum: maximized=0, minimized=1, activated=2, fullscreen=3, sticky=4
                let bytes: &[u8] = states_array.as_ref();
                for chunk in bytes.chunks_exact(4) {
                    if let Ok(arr) = <[u8; 4]>::try_from(chunk) {
                        let value = u32::from_ne_bytes(arr);
                        match value {
                            0 => handle_data.is_maximized = true,
                            1 => handle_data.is_minimized = true,
                            2 => handle_data.is_activated = true,
                            3 => handle_data.is_fullscreen = true,
                            4 => { /* sticky - not tracked */ }
                            _ => {}
                        }
                    }
                }
            }
            zcosmic_toplevel_handle_v1::Event::OutputEnter { .. } => {
                // Could track which outputs the toplevel is on
            }
            zcosmic_toplevel_handle_v1::Event::OutputLeave { .. } => {
                // Could track which outputs the toplevel is on
            }
            zcosmic_toplevel_handle_v1::Event::WorkspaceEnter { .. } => {
                // Could track workspace membership
            }
            zcosmic_toplevel_handle_v1::Event::WorkspaceLeave { .. } => {
                // Could track workspace membership
            }
            zcosmic_toplevel_handle_v1::Event::Closed => {
                // COSMIC handle closed, destroy it
                proxy.destroy();
            }
            _ => {}
        }
    }
}
