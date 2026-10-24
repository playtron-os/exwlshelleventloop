use super::cursor_tests::requests;
use super::{BaseState, Shell, WindowState, WindowStateUnitBuilder, id};
use std::io::Write;
use std::os::unix::net::UnixStream;
use wayland_client::globals::{GlobalList, registry_queue_init};
use wayland_client::protocol::{
    wl_compositor::WlCompositor, wl_seat::WlSeat, wl_surface::WlSurface,
};
use wayland_client::{Connection, Proxy, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::{
    Layer, ZwlrLayerShellV1,
};

const MANAGER: &str = "zwp_keyboard_shortcuts_inhibit_manager_v1";
const INHIBIT_SHORTCUTS: u16 = 1;
const DESTROY: u16 = 0;

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
    state: WindowState<()>,
    surface: WlSurface,
    seat: WlSeat,
}

/// A layer surface and seat on a connection whose compositor announces `announced` (name 7),
/// with the window state as the running loop has it: its own `globals` already taken.
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
    let registry = globals.registry();
    let compositor = registry.bind::<WlCompositor, _, _>(9, 4, &qh, ());
    let seat = registry.bind::<WlSeat, _, _>(10, 7, &qh, ());
    let layer_shell = registry.bind::<ZwlrLayerShellV1, _, _>(11, 4, &qh, ());
    let surface = compositor.create_surface(&qh, ());
    let layer =
        layer_shell.get_layer_surface(&surface, None, Layer::Overlay, "alt-tab".into(), &qh, ());
    let mut state = WindowState::<()>::new("shortcuts-inhibit-test");
    state.push_window(
        WindowStateUnitBuilder::new(
            id::Id::unique(),
            qh.clone(),
            connection.display(),
            surface.clone(),
            Shell::LayerShell(layer),
        )
        .build(),
    );
    state.seat = Some(seat.clone());
    assert!(state.globals.is_none());
    peer.set_nonblocking(true).unwrap();
    let mut fixture = Fixture {
        connection,
        peer,
        globals,
        qh,
        state,
        surface,
        seat,
    };
    fixture.sent();
    fixture
}

impl Fixture {
    fn sent(&mut self) -> Vec<(u32, u16, Vec<u32>)> {
        self.connection.flush().unwrap();
        requests(&mut self.peer)
    }
}

#[test]
fn the_switcher_surface_is_inhibited_through_the_manager_bound_at_startup() {
    let mut f = fixture(MANAGER);
    f.state
        .bind_keyboard_shortcuts_inhibit_manager(&f.globals, &f.qh);
    let manager = f
        .state
        .keyboard_shortcuts_inhibit_manager
        .as_ref()
        .expect("the compositor announced the manager")
        .id()
        .protocol_id();
    let surface = f.surface.clone();
    f.state
        .set_keyboard_shortcuts_inhibit_for_surface(&surface, true);
    f.state
        .set_keyboard_shortcuts_inhibit_for_surface(&surface, true);
    let surface = surface.id().protocol_id();
    let inhibitor = f.state.keyboard_shortcuts_inhibitors[&surface]
        .id()
        .protocol_id();

    assert_eq!(
        f.sent(),
        vec![
            (2, 0, [vec![7], string(MANAGER), vec![1, manager]].concat()),
            (
                manager,
                INHIBIT_SHORTCUTS,
                vec![inhibitor, surface, f.seat.id().protocol_id()]
            ),
        ],
        "one wl_registry.bind, then one inhibit_shortcuts for the surface and seat",
    );

    let surface = f.surface.clone();
    f.state
        .set_keyboard_shortcuts_inhibit_for_surface(&surface, false);
    assert_eq!(f.sent(), vec![(inhibitor, DESTROY, vec![])]);
    assert!(f.state.keyboard_shortcuts_inhibitors.is_empty());
}

#[test]
fn nothing_is_sent_when_the_compositor_lacks_the_manager() {
    let mut f = fixture("wl_output");
    f.state
        .bind_keyboard_shortcuts_inhibit_manager(&f.globals, &f.qh);
    assert!(f.state.keyboard_shortcuts_inhibit_manager.is_none());
    let surface = f.surface.clone();
    f.state
        .set_keyboard_shortcuts_inhibit_for_surface(&surface, true);
    f.state
        .set_keyboard_shortcuts_inhibit_for_surface(&surface, false);
    assert_eq!(f.sent(), vec![], "nothing to bind, so nothing is sent");
    assert!(f.state.keyboard_shortcuts_inhibitors.is_empty());
}
