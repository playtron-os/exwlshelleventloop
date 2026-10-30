use super::{
    DispatchMessage, LayerShellEvent, ReturnData, Shell, WindowState, WindowStateUnitBuilder,
    ZxdgOutputInfo, id,
};
use std::os::unix::net::UnixStream;
use wayland_client::protocol::{wl_compositor::WlCompositor, wl_output::WlOutput};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::ZxdgOutputManagerV1,
    zxdg_output_v1::{self, ZxdgOutputV1},
};
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::{
    Layer, ZwlrLayerShellV1,
};

struct Fixture {
    connection: Connection,
    _peer: UnixStream,
    qh: QueueHandle<WindowState<()>>,
    state: WindowState<()>,
    unit: id::Id,
    xdg_output: ZxdgOutputV1,
}

/// A layer surface on an output whose `zxdg_output` is bound, as after `wl_surface.enter`.
fn fixture() -> Fixture {
    let (socket, peer) = UnixStream::pair().unwrap();
    let connection = Connection::from_socket(socket).unwrap();
    let queue = connection.new_event_queue::<WindowState<()>>();
    let qh = queue.handle();
    let registry = connection.display().get_registry(&qh, ());
    let compositor = registry.bind::<WlCompositor, _, _>(1, 4, &qh, ());
    let layer_shell = registry.bind::<ZwlrLayerShellV1, _, _>(2, 4, &qh, ());
    let output = registry.bind::<WlOutput, _, _>(3, 4, &qh, ());
    let xdg_outputs = registry.bind::<ZxdgOutputManagerV1, _, _>(4, 3, &qh, ());
    let surface = compositor.create_surface(&qh, ());
    let layer = layer_shell.get_layer_surface(&surface, None, Layer::Top, "bar".into(), &qh, ());
    let xdg_output = xdg_outputs.get_xdg_output(&output, &qh, ());
    let unit = id::Id::unique();
    let mut state = WindowState::<()>::new("output-info-test");
    let mut window = WindowStateUnitBuilder::new(
        unit,
        qh.clone(),
        connection.display(),
        surface,
        Shell::LayerShell(layer),
    )
    .build();
    window.zxdgoutput = Some(ZxdgOutputInfo::new(xdg_output.clone()));
    state.push_window(window);
    Fixture {
        connection,
        _peer: peer,
        qh,
        state,
        unit,
        xdg_output,
    }
}

impl Fixture {
    fn output_event(&mut self, event: zxdg_output_v1::Event) {
        <WindowState<()> as Dispatch<ZxdgOutputV1, ()>>::event(
            &mut self.state,
            &self.xdg_output,
            event,
            &(),
            &self.connection,
            &self.qh,
        );
    }

    /// The messages a consumer is handed when the surface is drawn.
    fn draw(&mut self) -> Vec<Seen> {
        let mut seen = Vec::new();
        let mut consumer = |event: LayerShellEvent<(), ()>, _: &mut WindowState<()>, _| {
            seen.push(match event {
                LayerShellEvent::RequestMessages(DispatchMessage::RequestRefresh { .. }) => {
                    Seen::Refresh
                }
                LayerShellEvent::RequestMessages(DispatchMessage::XdgInfoChanged {
                    width,
                    height,
                    output_name,
                    output_x,
                    output_y,
                }) => Seen::Output(*width, *height, output_name.clone(), *output_x, *output_y),
                _ => Seen::Other,
            });
            ReturnData::None
        };
        self.state
            .refresh_unit(&mut consumer, self.unit, (400, 300), false, 1.0);
        seen
    }
}

#[derive(Debug, PartialEq)]
enum Seen {
    Refresh,
    Output(i32, i32, String, i32, i32),
    Other,
}

/// The output's size came before the first draw, when the consumer had no window to
/// give it to, so a surface never learned its screen's height; the first draw repeats it.
#[test]
fn the_first_draw_repeats_an_output_size_that_came_before_it() {
    let mut fixture = fixture();
    fixture.output_event(zxdg_output_v1::Event::LogicalPosition { x: 0, y: 0 });
    fixture.output_event(zxdg_output_v1::Event::LogicalSize {
        width: 1280,
        height: 720,
    });
    fixture.output_event(zxdg_output_v1::Event::Name {
        name: "DP-1".into(),
    });

    let output = Seen::Output(1280, 720, "DP-1".into(), 0, 0);
    assert_eq!(fixture.draw(), [Seen::Refresh, output]);
    assert_eq!(
        fixture.draw(),
        [Seen::Refresh],
        "only the first draw repeats it"
    );
}

/// An output size that comes later reaches the window on its own.
#[test]
fn an_unknown_output_is_not_reported() {
    let mut fixture = fixture();
    assert_eq!(fixture.draw(), [Seen::Refresh]);
}
