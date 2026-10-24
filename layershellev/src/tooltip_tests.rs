use super::cursor_tests::requests;
use super::{BaseState, TooltipPlacement, WindowState, attach_tooltip};
use std::io::Write;
use std::os::unix::net::UnixStream;
use wayland_client::globals::{GlobalList, registry_queue_init};
use wayland_client::protocol::{wl_compositor::WlCompositor, wl_surface::WlSurface};
use wayland_client::{Connection, Proxy, QueueHandle};

const MANAGER: &str = "zcosmic_tooltip_manager_v1";

/// One event from the compositor: its object, opcode and words.
fn event(object: u32, opcode: u16, words: &[u32]) -> Vec<u8> {
    let length = 8 + 4 * words.len() as u32;
    let mut bytes = object.to_ne_bytes().to_vec();
    bytes.extend((length << 16 | u32::from(opcode)).to_ne_bytes());
    bytes.extend(words.iter().flat_map(|word| word.to_ne_bytes()));
    bytes
}

/// A string argument: its length with the NUL, then its bytes padded to words.
fn string(text: &str) -> Vec<u32> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    bytes.resize(bytes.len().next_multiple_of(4), 0);
    let mut words = vec![text.len() as u32 + 1];
    words.extend(
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| u32::from_ne_bytes(*word)),
    );
    words
}

struct Fixture {
    connection: Connection,
    peer: UnixStream,
    globals: GlobalList,
    qh: QueueHandle<WindowState<()>>,
    compositor: WlCompositor,
}

/// A connection whose compositor announces `announced` (name 7) in the first roundtrip.
fn fixture(announced: &str) -> Fixture {
    let (socket, mut peer) = UnixStream::pair().unwrap();
    // Queued before the client asks: the registry is object 2 and the roundtrip's callback 3.
    let global = [vec![7], string(announced), vec![1]].concat();
    peer.write_all(&event(2, 0, &global)).unwrap();
    peer.write_all(&event(3, 0, &[0])).unwrap();
    let connection = Connection::from_socket(socket).unwrap();
    let (globals, _) = registry_queue_init::<BaseState>(&connection).unwrap();
    let queue = connection.new_event_queue::<WindowState<()>>();
    let qh = queue.handle();
    let compositor = globals.registry().bind::<WlCompositor, _, _>(9, 4, &qh, ());
    peer.set_nonblocking(true).unwrap();
    Fixture {
        connection,
        peer,
        globals,
        qh,
        compositor,
    }
}

impl Fixture {
    fn surface(&self) -> WlSurface {
        self.compositor.create_surface(&self.qh, ())
    }

    fn sent(&mut self) -> Vec<(u32, u16, Vec<u32>)> {
        self.connection.flush().unwrap();
        requests(&mut self.peer)
    }
}

#[test]
fn a_tooltip_popup_binds_the_manager_from_the_loop_globals_once() {
    let mut f = fixture(MANAGER);
    let parent = f.surface();
    let (first, second) = (f.surface(), f.surface());
    f.sent();

    let mut manager = None;
    let placement = TooltipPlacement {
        offset: Some((12, -8)),
        anchor: Some(3),
        delay_ms: Some(400),
    };
    let one = attach_tooltip(&mut manager, &f.globals, &f.qh, &first, &parent, placement)
        .expect("the compositor announced the manager");
    let only_offset = TooltipPlacement {
        offset: Some((0, 16)),
        ..TooltipPlacement::default()
    };
    let two = attach_tooltip(
        &mut manager,
        &f.globals,
        &f.qh,
        &second,
        &parent,
        only_offset,
    )
    .expect("the bound manager is reused");
    let manager = manager.expect("bound").id().protocol_id();
    let (one, two) = (one.id().protocol_id(), two.id().protocol_id());
    let (first, second, parent) = (
        first.id().protocol_id(),
        second.id().protocol_id(),
        parent.id().protocol_id(),
    );

    assert_eq!(
        f.sent(),
        vec![
            (2, 0, [vec![7], string(MANAGER), vec![1, manager]].concat()),
            (manager, 1, vec![one, first, parent]),
            (one, 1, vec![12, (-8i32) as u32]),
            (one, 2, vec![3]),
            (one, 3, vec![400]),
            (manager, 1, vec![two, second, parent]),
            (two, 1, vec![0, 16]),
        ],
        "one wl_registry.bind, then get_tooltip and the placement for each popup",
    );
}

#[test]
fn other_popups_and_compositors_without_the_manager_get_no_tooltip() {
    let mut f = fixture(MANAGER);
    let (parent, popup) = (f.surface(), f.surface());
    f.sent();
    let mut manager = None;
    let plain = TooltipPlacement::default();
    assert!(attach_tooltip(&mut manager, &f.globals, &f.qh, &popup, &parent, plain).is_none());
    assert!(
        manager.is_none(),
        "a popup without tooltip settings binds nothing"
    );
    assert_eq!(f.sent(), vec![]);

    let mut f = fixture("wl_seat");
    let (parent, popup) = (f.surface(), f.surface());
    f.sent();
    let placement = TooltipPlacement {
        offset: Some((4, 4)),
        ..TooltipPlacement::default()
    };
    assert!(attach_tooltip(&mut manager, &f.globals, &f.qh, &popup, &parent, placement).is_none());
    assert!(manager.is_none());
    assert_eq!(f.sent(), vec![], "nothing to bind, so nothing is sent");
}
