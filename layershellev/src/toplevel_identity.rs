//! Authenticated workspace attribution for live foreign handles.

use std::collections::HashMap;

use cosmic_protocols::kora_toplevel_identity::v1::client::{
    kora_toplevel_identity_handle_v1::{Event, KoraToplevelIdentityHandleV1},
    kora_toplevel_identity_v1::KoraToplevelIdentityV1,
};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1;

use crate::WindowState;
use crate::foreign_toplevel::{ForeignToplevelEvent, ForeignToplevelHandler};

#[derive(Default)]
pub(crate) struct State {
    pub manager: Option<KoraToplevelIdentityV1>,
    handles: HashMap<u32, Identity>,
}

struct Identity {
    foreign: ExtForeignToplevelHandleV1,
    handle: KoraToplevelIdentityHandleV1,
    identifier: Option<String>,
    workspace: Option<String>,
    done: bool,
}

impl State {
    pub fn request<T: 'static>(
        &mut self,
        foreign: ExtForeignToplevelHandleV1,
        queue: &QueueHandle<WindowState<T>>,
    ) {
        let Some(manager) = &self.manager else {
            return;
        };
        let id = foreign.id().protocol_id();
        let handle = manager.get_foreign_identity(&foreign, queue, id);
        self.remove(id);
        self.handles.insert(
            id,
            Identity {
                foreign,
                handle,
                identifier: None,
                workspace: None,
                done: false,
            },
        );
    }

    pub fn remove(&mut self, id: u32) {
        if let Some(identity) = self.handles.remove(&id) {
            identity.handle.destroy();
        }
    }
}

impl<T: 'static> WindowState<T> {
    pub(crate) fn refresh_toplevel_identity(&mut self, id: u32, notify: bool) {
        let Some(data) = self.foreign_toplevel_data.get_mut(&id) else {
            return;
        };
        let workspace = self
            .toplevel_identity
            .handles
            .get(&id)
            .and_then(|identity| {
                (identity.done
                    && identity.identifier.is_some()
                    && identity.identifier == data.identifier
                    && self.ext_toplevel_handles.get(&id) == Some(&identity.foreign))
                .then(|| identity.workspace.clone())
                .flatten()
            });
        if data.workspace != workspace {
            data.workspace = workspace;
            if notify && data.initialized {
                let info = data.to_info(id);
                self.foreign_toplevel_event(ForeignToplevelEvent::Changed(info));
            }
        }
    }
}

impl<T: 'static> Dispatch<KoraToplevelIdentityV1, ()> for WindowState<T> {
    fn event(
        _state: &mut Self,
        _proxy: &KoraToplevelIdentityV1,
        _event: <KoraToplevelIdentityV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _queue: &QueueHandle<Self>,
    ) {
    }
}

impl<T: 'static> Dispatch<KoraToplevelIdentityHandleV1, u32> for WindowState<T> {
    fn event(
        state: &mut Self,
        proxy: &KoraToplevelIdentityHandleV1,
        event: Event,
        id: &u32,
        _conn: &Connection,
        _queue: &QueueHandle<Self>,
    ) {
        let Some(identity) = state.toplevel_identity.handles.get_mut(id) else {
            return;
        };
        if &identity.handle != proxy
            || state.ext_toplevel_handles.get(id) != Some(&identity.foreign)
        {
            return;
        }
        match event {
            Event::Identifier { identifier } if !identity.done => {
                identity.identifier = Some(identifier);
            }
            Event::Workspace { workspace } if !identity.done => {
                identity.workspace = Some(workspace);
            }
            Event::Done if !identity.done => {
                identity.done = true;
                state.refresh_toplevel_identity(*id, true);
            }
            Event::Closed => {
                state.toplevel_identity.remove(*id);
                state.refresh_toplevel_identity(*id, true);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DispatchMessageInner;
    use crate::foreign_toplevel::{ExtForeignToplevelListData, ToplevelInfo};
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    use wayland_client::EventQueue;
    use wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1;

    struct Fixture {
        state: WindowState<()>,
        connection: Connection,
        queue: EventQueue<WindowState<()>>,
        server: UnixStream,
        list: ExtForeignToplevelListV1,
    }

    fn string(value: &str) -> Vec<u8> {
        let mut bytes = ((value.len() + 1) as u32).to_ne_bytes().to_vec();
        bytes.extend_from_slice(value.as_bytes());
        bytes.push(0);
        bytes.resize(bytes.len().next_multiple_of(4), 0);
        bytes
    }

    impl Fixture {
        fn new(identity_available: bool) -> Self {
            let (client, server) = UnixStream::pair().unwrap();
            let connection = Connection::from_socket(client).unwrap();
            let queue = connection.new_event_queue();
            let handle = queue.handle();
            let registry = connection.display().get_registry(&handle, ());
            let list = registry.bind(1, 1, &handle, ExtForeignToplevelListData);
            let mut state = WindowState::default();
            if identity_available {
                state.toplevel_identity.manager = Some(registry.bind(2, 1, &handle, ()));
            }
            Self {
                state,
                connection,
                queue,
                server,
                list,
            }
        }

        fn send(&mut self, object: u32, opcode: u32, body: &[u8]) {
            let mut bytes = object.to_ne_bytes().to_vec();
            bytes.extend_from_slice(&(((body.len() + 8) as u32) << 16 | opcode).to_ne_bytes());
            bytes.extend_from_slice(body);
            self.server.write_all(&bytes).unwrap();
            self.queue.blocking_dispatch(&mut self.state).unwrap();
        }

        fn window(&mut self, id: u32) {
            self.send(self.list.id().protocol_id(), 0, &id.to_ne_bytes());
        }

        fn ext_done(&mut self, id: u32, identifier: &str) {
            self.send(id, 4, &string(identifier));
            self.send(id, 1, &[]);
        }

        fn identity_handle(&self, id: u32) -> KoraToplevelIdentityHandleV1 {
            self.state.toplevel_identity.handles[&id].handle.clone()
        }

        fn identity(&mut self, id: u32, identifier: &str, workspace: &str) {
            let handle = self.identity_handle(id).id().protocol_id();
            self.send(handle, 0, &string(identifier));
            self.send(handle, 1, &string(workspace));
            self.send(handle, 2, &[]);
        }

        fn info(&self, id: u32) -> ToplevelInfo {
            self.state.foreign_toplevel_data[&id].to_info(id)
        }

        fn events(&mut self) -> Vec<ForeignToplevelEvent> {
            self.state
                .message
                .drain(..)
                .filter_map(|(_, event)| match event {
                    DispatchMessageInner::ForeignToplevel(event) => Some(event),
                    _ => None,
                })
                .collect()
        }
    }

    const WINDOW: u32 = 0xff00_0000;

    #[test]
    fn an_atomic_pair_changes_an_initialized_window_only_at_done() {
        let mut fixture = Fixture::new(true);
        fixture.window(WINDOW);
        fixture.ext_done(WINDOW, "stable");
        assert!(
            matches!(fixture.events().as_slice(), [ForeignToplevelEvent::Created(info)] if info.workspace.is_none())
        );
        let identity = fixture.identity_handle(WINDOW).id().protocol_id();
        fixture.send(identity, 0, &string("stable"));
        fixture.send(identity, 1, &string("project-a"));
        assert_eq!(fixture.info(WINDOW).workspace, None);
        assert!(fixture.events().is_empty());
        fixture.send(identity, 2, &[]);
        assert!(
            matches!(fixture.events().as_slice(), [ForeignToplevelEvent::Changed(info)] if info.identifier.as_deref() == Some("stable") && info.workspace.as_deref() == Some("project-a"))
        );
    }

    #[test]
    fn an_early_identity_waits_for_the_matching_ext_identifier() {
        let mut fixture = Fixture::new(true);
        fixture.window(WINDOW);
        fixture.identity(WINDOW, "stable", "");
        assert_eq!(fixture.info(WINDOW).workspace, None);
        assert!(fixture.events().is_empty());
        fixture.ext_done(WINDOW, "stable");
        assert!(
            matches!(fixture.events().as_slice(), [ForeignToplevelEvent::Created(info)] if info.workspace.as_deref() == Some(""))
        );
    }

    #[test]
    fn absent_incomplete_and_mismatched_identity_is_unknown() {
        let mut absent = Fixture::new(false);
        absent.window(WINDOW);
        absent.ext_done(WINDOW, "stable");
        assert_eq!(absent.info(WINDOW).workspace, None);
        assert!(absent.state.toplevel_identity.handles.is_empty());

        let mut mismatched = Fixture::new(true);
        mismatched.window(WINDOW);
        mismatched.ext_done(WINDOW, "stable");
        mismatched.identity(WINDOW, "other", "project-a");
        assert_eq!(mismatched.info(WINDOW).workspace, None);

        let mut incomplete = Fixture::new(true);
        incomplete.window(WINDOW);
        incomplete.ext_done(WINDOW, "stable");
        let identity = incomplete.identity_handle(WINDOW).id().protocol_id();
        incomplete.send(identity, 0, &string("stable"));
        incomplete.send(identity, 2, &[]);
        assert_eq!(incomplete.info(WINDOW).workspace, None);
    }

    #[test]
    fn revocation_clears_attribution_and_late_events_cannot_restore_it() {
        let mut fixture = Fixture::new(true);
        fixture.window(WINDOW);
        fixture.ext_done(WINDOW, "stable");
        fixture.identity(WINDOW, "stable", "project-a");
        let handle = fixture.identity_handle(WINDOW);
        fixture.events();
        fixture.send(handle.id().protocol_id(), 3, &[]);
        assert_eq!(fixture.info(WINDOW).workspace, None);
        assert!(
            matches!(fixture.events().as_slice(), [ForeignToplevelEvent::Changed(info)] if info.workspace.is_none())
        );
        <WindowState<()> as Dispatch<KoraToplevelIdentityHandleV1, u32>>::event(
            &mut fixture.state,
            &handle,
            Event::Done,
            &WINDOW,
            &fixture.connection,
            &fixture.queue.handle(),
        );
        assert_eq!(fixture.info(WINDOW).workspace, None);
        assert!(fixture.events().is_empty());
    }

    #[test]
    fn closing_a_foreign_handle_does_not_reanimate_on_late_identity_events() {
        let mut fixture = Fixture::new(true);
        fixture.window(WINDOW);
        fixture.ext_done(WINDOW, "stable");
        let old = fixture.identity_handle(WINDOW);
        fixture.send(old.id().protocol_id(), 0, &string("stable"));
        fixture.send(old.id().protocol_id(), 1, &string("project-a"));
        fixture.events();
        fixture.send(WINDOW, 0, &[]);
        assert!(!fixture.state.foreign_toplevel_data.contains_key(&WINDOW));
        assert!(matches!(
            fixture.events().as_slice(),
            [ForeignToplevelEvent::Closed(WINDOW)]
        ));
        <WindowState<()> as Dispatch<KoraToplevelIdentityHandleV1, u32>>::event(
            &mut fixture.state,
            &old,
            Event::Done,
            &WINDOW,
            &fixture.connection,
            &fixture.queue.handle(),
        );
        assert!(!fixture.state.foreign_toplevel_data.contains_key(&WINDOW));

        // Even reuse of the numeric object ID must obtain new attribution.
        fixture.window(WINDOW);
        fixture.ext_done(WINDOW, "stable");
        <WindowState<()> as Dispatch<KoraToplevelIdentityHandleV1, u32>>::event(
            &mut fixture.state,
            &old,
            Event::Done,
            &WINDOW,
            &fixture.connection,
            &fixture.queue.handle(),
        );
        assert_eq!(fixture.info(WINDOW).workspace, None);
        fixture.identity(WINDOW, "stable", "project-a");
        assert_eq!(fixture.info(WINDOW).workspace.as_deref(), Some("project-a"));
    }
}
