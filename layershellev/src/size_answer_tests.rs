use super::{RefreshRequest, Shell, SizeAnswer, WindowState, WindowStateUnitBuilder, id};
use std::os::unix::net::UnixStream;
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::{
    Layer, ZwlrLayerShellV1,
};

struct Fixture {
    connection: Connection,
    _peer: UnixStream,
    qh: QueueHandle<WindowState<()>>,
    state: WindowState<()>,
    unit: id::Id,
}

fn fixture() -> Fixture {
    let (socket, peer) = UnixStream::pair().unwrap();
    let connection = Connection::from_socket(socket).unwrap();
    let queue = connection.new_event_queue::<WindowState<()>>();
    let qh = queue.handle();
    let registry = connection.display().get_registry(&qh, ());
    let compositor = registry.bind::<WlCompositor, _, _>(1, 4, &qh, ());
    let layer_shell = registry.bind::<ZwlrLayerShellV1, _, _>(2, 4, &qh, ());
    let surface = compositor.create_surface(&qh, ());
    let layer = layer_shell.get_layer_surface(&surface, None, Layer::Top, "pop".into(), &qh, ());
    let unit = id::Id::unique();
    let mut state = WindowState::<()>::new("size-answer-test");
    state.push_window(
        WindowStateUnitBuilder::new(
            unit,
            qh.clone(),
            connection.display(),
            surface,
            Shell::LayerShell(layer),
        )
        .build(),
    );
    Fixture {
        connection,
        _peer: peer,
        qh,
        state,
        unit,
    }
}

impl Fixture {
    /// The compositor's answer to one size request: the `wl_display.sync` sent after it is done.
    fn answer(&mut self) {
        let callback = self
            .connection
            .display()
            .sync(&self.qh, SizeAnswer(self.unit));
        <WindowState<()> as Dispatch<WlCallback, SizeAnswer>>::event(
            &mut self.state,
            &callback,
            wl_callback::Event::Done { callback_data: 0 },
            &SizeAnswer(self.unit),
            &self.connection,
            &self.qh,
        );
    }
}

/// A popover asking for more than fits is kept at the size it has and gets no
/// configure; it waited for one, unseen, until input happened to redraw it.
#[test]
fn a_size_the_compositor_keeps_is_still_answered() {
    let mut fixture = fixture();
    fixture.state.units[0].request_flag.refresh = RefreshRequest::Wait;

    fixture.state.units[0].set_size((380, 900));
    assert!(!fixture.state.units[0].size_answered());
    assert!(!fixture.state.units[0].should_refresh());

    fixture.answer();
    assert!(fixture.state.units[0].size_answered());
    assert!(
        fixture.state.units[0].should_refresh(),
        "the answer draws the surface at the size it has"
    );
}

#[test]
fn every_request_must_be_answered() {
    let mut fixture = fixture();
    assert!(fixture.state.units[0].size_answered(), "nothing asked yet");
    fixture.state.units[0].set_size((380, 900));
    fixture.state.units[0].set_size((380, 720));
    fixture.answer();
    assert!(!fixture.state.units[0].size_answered());
    fixture.answer();
    assert!(fixture.state.units[0].size_answered());
}
