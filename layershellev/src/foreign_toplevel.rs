//! Foreign toplevel management client
//!
//! This module provides client-side support for the `zwlr_foreign_toplevel_manager_v1`
//! Wayland protocol, which allows listing and controlling opened application windows.
//!
//! Use this to create taskbars and docks that need to track open windows.

use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
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
