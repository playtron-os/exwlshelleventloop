//! The desktops of every Kora workspace: `ext_workspace_v1` for the desktops
//! themselves, plus `kora_workspace_realm_v1` for which workspace each group
//! of them belongs to.

use std::collections::HashMap;

use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_group_handle_v1::{self, ExtWorkspaceGroupHandleV1, GroupCapabilities},
    ext_workspace_handle_v1::{self, ExtWorkspaceHandleV1, State, WorkspaceCapabilities},
    ext_workspace_manager_v1::{self, ExtWorkspaceManagerV1},
};

use crate::{DispatchMessageInner, WindowState};

pub use realm::kora_workspace_realm_manager_v1;
use realm::kora_workspace_realm_manager_v1::KoraWorkspaceRealmManagerV1;

#[allow(
    non_snake_case,
    non_upper_case_globals,
    non_camel_case_types,
    dead_code,
    unused_imports
)]
mod realm {
    use wayland_client;
    use wayland_client::protocol::*;
    use wayland_protocols::ext::workspace::v1::client::*;

    pub mod __interfaces {
        use wayland_backend;
        use wayland_client::protocol::__interfaces::*;
        use wayland_protocols::ext::workspace::v1::client::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/kora-workspace-realm.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/kora-workspace-realm.xml");
}

/// One group of desktops: a Kora workspace on one output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceGroupInfo {
    /// The group object's protocol id.
    pub id: u32,
    /// The Kora workspace registry id, once the compositor has said.
    pub realm: Option<String>,
    /// Whether the group is on an output right now (the active workspace's are).
    pub on_screen: bool,
    pub can_create: bool,
}

/// One desktop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceInfo {
    /// The handle's protocol id; what a capture or an action names.
    pub id: u32,
    pub group: Option<u32>,
    pub name: String,
    pub ext_id: Option<String>,
    /// Position within the group, in the protocol's own axes.
    pub coordinates: Vec<u32>,
    pub active: bool,
    pub urgent: bool,
    pub hidden: bool,
    pub can_activate: bool,
    pub can_remove: bool,
}

/// Everything known, sent whole after every batch of changes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspacesSnapshot {
    pub groups: Vec<WorkspaceGroupInfo>,
    pub workspaces: Vec<WorkspaceInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceEvent {
    Changed(WorkspacesSnapshot),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceAction {
    /// Show this desktop.
    Activate(u32),
    /// Add a desktop to a group.
    Create { group: u32, name: String },
    /// Remove a desktop.
    Remove(u32),
}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceManagerData;
#[derive(Debug, Clone, Default)]
pub struct WorkspaceGroupData;
#[derive(Debug, Clone, Default)]
pub struct WorkspaceHandleData;
#[derive(Debug, Clone, Default)]
pub struct RealmData;

#[derive(Debug, Default)]
pub(crate) struct WorkspacesState {
    pub manager: Option<ExtWorkspaceManagerV1>,
    pub realm_manager: Option<KoraWorkspaceRealmManagerV1>,
    groups: HashMap<u32, (ExtWorkspaceGroupHandleV1, WorkspaceGroupInfo)>,
    handles: HashMap<u32, (ExtWorkspaceHandleV1, WorkspaceInfo)>,
    outputs: HashMap<u32, usize>,
}

impl WorkspacesState {
    pub fn snapshot(&self) -> WorkspacesSnapshot {
        let mut groups: Vec<_> = self.groups.values().map(|(_, info)| info.clone()).collect();
        groups.sort_by_key(|g| g.id);
        let mut workspaces: Vec<_> = self
            .handles
            .values()
            .map(|(_, info)| info.clone())
            .collect();
        workspaces.sort_by_key(|w| w.id);
        WorkspacesSnapshot { groups, workspaces }
    }

    pub fn handle(&self, id: u32) -> Option<&ExtWorkspaceHandleV1> {
        self.handles.get(&id).map(|(handle, _)| handle)
    }
}

impl<T> WindowState<T> {
    fn emit_workspaces(&mut self) {
        let snapshot = self.workspaces.snapshot();
        self.message.push((
            None,
            DispatchMessageInner::Workspaces(WorkspaceEvent::Changed(snapshot)),
        ));
    }

    /// Carry out `action`, committing it in the same round trip.
    pub fn execute_workspace_action(&mut self, action: WorkspaceAction) -> bool {
        let Some(manager) = self.workspaces.manager.as_ref() else {
            log::warn!("Workspace action dropped: ext_workspace_manager_v1 not bound");
            return false;
        };
        match action {
            WorkspaceAction::Activate(id) => {
                let Some(handle) = self.workspaces.handle(id) else {
                    return false;
                };
                handle.activate();
            }
            WorkspaceAction::Create { group, name } => {
                let Some((group, _)) = self.workspaces.groups.get(&group) else {
                    return false;
                };
                group.create_workspace(name);
            }
            WorkspaceAction::Remove(id) => {
                let Some(handle) = self.workspaces.handle(id) else {
                    return false;
                };
                handle.remove();
            }
        }
        manager.commit();
        true
    }
}

impl<T: 'static> Dispatch<ExtWorkspaceManagerV1, WorkspaceManagerData> for WindowState<T> {
    fn event(
        state: &mut Self,
        _proxy: &ExtWorkspaceManagerV1,
        event: ext_workspace_manager_v1::Event,
        _data: &WorkspaceManagerData,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_manager_v1::Event::WorkspaceGroup { workspace_group } => {
                let id = workspace_group.id().protocol_id();
                let info = WorkspaceGroupInfo {
                    id,
                    realm: None,
                    on_screen: false,
                    can_create: false,
                };
                state.workspaces.groups.insert(id, (workspace_group, info));
            }
            ext_workspace_manager_v1::Event::Workspace { workspace } => {
                let id = workspace.id().protocol_id();
                let info = WorkspaceInfo {
                    id,
                    group: None,
                    name: String::new(),
                    ext_id: None,
                    coordinates: Vec::new(),
                    active: false,
                    urgent: false,
                    hidden: false,
                    can_activate: false,
                    can_remove: false,
                };
                state.workspaces.handles.insert(id, (workspace, info));
            }
            ext_workspace_manager_v1::Event::Done => state.emit_workspaces(),
            ext_workspace_manager_v1::Event::Finished => {
                state.workspaces.groups.clear();
                state.workspaces.handles.clear();
                state.workspaces.manager = None;
                state.emit_workspaces();
            }
            _ => {}
        }
    }

    fn event_created_child(
        opcode: u16,
        qh: &QueueHandle<Self>,
    ) -> std::sync::Arc<dyn wayland_client::backend::ObjectData> {
        match opcode {
            0 => qh.make_data::<ExtWorkspaceGroupHandleV1, _>(WorkspaceGroupData),
            1 => qh.make_data::<ExtWorkspaceHandleV1, _>(WorkspaceHandleData),
            _ => panic!("Unknown ext_workspace_manager_v1 opcode {opcode}"),
        }
    }
}

impl<T: 'static> Dispatch<ExtWorkspaceGroupHandleV1, WorkspaceGroupData> for WindowState<T> {
    fn event(
        state: &mut Self,
        proxy: &ExtWorkspaceGroupHandleV1,
        event: ext_workspace_group_handle_v1::Event,
        _data: &WorkspaceGroupData,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let id = proxy.id().protocol_id();
        match event {
            ext_workspace_group_handle_v1::Event::Capabilities { capabilities } => {
                if let Some((_, info)) = state.workspaces.groups.get_mut(&id) {
                    info.can_create = matches!(capabilities, WEnum::Value(c) if c.contains(GroupCapabilities::CreateWorkspace));
                }
            }
            ext_workspace_group_handle_v1::Event::OutputEnter { .. } => {
                let n = state.workspaces.outputs.entry(id).or_insert(0);
                *n += 1;
                if let Some((_, info)) = state.workspaces.groups.get_mut(&id) {
                    info.on_screen = true;
                }
            }
            ext_workspace_group_handle_v1::Event::OutputLeave { .. } => {
                let n = state.workspaces.outputs.entry(id).or_insert(0);
                *n = n.saturating_sub(1);
                let on_screen = *n > 0;
                if let Some((_, info)) = state.workspaces.groups.get_mut(&id) {
                    info.on_screen = on_screen;
                }
            }
            ext_workspace_group_handle_v1::Event::WorkspaceEnter { workspace } => {
                let wid = workspace.id().protocol_id();
                if let Some((_, info)) = state.workspaces.handles.get_mut(&wid) {
                    info.group = Some(id);
                }
            }
            ext_workspace_group_handle_v1::Event::WorkspaceLeave { workspace } => {
                let wid = workspace.id().protocol_id();
                if let Some((_, info)) = state.workspaces.handles.get_mut(&wid) {
                    if info.group == Some(id) {
                        info.group = None;
                    }
                }
            }
            ext_workspace_group_handle_v1::Event::Removed => {
                if let Some((group, _)) = state.workspaces.groups.remove(&id) {
                    group.destroy();
                }
                state.workspaces.outputs.remove(&id);
            }
            _ => {}
        }
    }
}

impl<T: 'static> Dispatch<ExtWorkspaceHandleV1, WorkspaceHandleData> for WindowState<T> {
    fn event(
        state: &mut Self,
        proxy: &ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        _data: &WorkspaceHandleData,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let id = proxy.id().protocol_id();
        let Some((_, info)) = state.workspaces.handles.get_mut(&id) else {
            return;
        };
        match event {
            ext_workspace_handle_v1::Event::Id { id } => info.ext_id = Some(id),
            ext_workspace_handle_v1::Event::Name { name } => info.name = name,
            ext_workspace_handle_v1::Event::Coordinates { coordinates } => {
                info.coordinates = coordinates
                    .chunks_exact(4)
                    .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();
            }
            ext_workspace_handle_v1::Event::State { state: bits } => {
                let bits = match bits {
                    WEnum::Value(s) => s,
                    WEnum::Unknown(_) => State::empty(),
                };
                info.active = bits.contains(State::Active);
                info.urgent = bits.contains(State::Urgent);
                info.hidden = bits.contains(State::Hidden);
            }
            ext_workspace_handle_v1::Event::Capabilities { capabilities } => {
                let caps = match capabilities {
                    WEnum::Value(c) => c,
                    WEnum::Unknown(_) => WorkspaceCapabilities::empty(),
                };
                info.can_activate = caps.contains(WorkspaceCapabilities::Activate);
                info.can_remove = caps.contains(WorkspaceCapabilities::Remove);
            }
            ext_workspace_handle_v1::Event::Removed => {
                if let Some((handle, _)) = state.workspaces.handles.remove(&id) {
                    handle.destroy();
                }
            }
            _ => {}
        }
    }
}

impl<T: 'static> Dispatch<KoraWorkspaceRealmManagerV1, RealmData> for WindowState<T> {
    fn event(
        state: &mut Self,
        _proxy: &KoraWorkspaceRealmManagerV1,
        event: kora_workspace_realm_manager_v1::Event,
        _data: &RealmData,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let kora_workspace_realm_manager_v1::Event::Realm { group, id } = event;
        let gid = group.id().protocol_id();
        if let Some((_, info)) = state.workspaces.groups.get_mut(&gid) {
            info.realm = Some(id);
        }
        // Not batched under the workspace protocol's `done`, so it is its own change.
        state.emit_workspaces();
    }
}
