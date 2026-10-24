use super::cursor_tests::requests;
use super::{Shell, WindowState, WindowStateUnitBuilder, background_effect, blur, id};
use std::os::unix::net::UnixStream;
use wayland_client::protocol::{wl_compositor::WlCompositor, wl_surface::WlSurface};
use wayland_client::{Connection, Proxy};
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::{
    Layer, ZwlrLayerShellV1,
};

const GET_BACKGROUND_EFFECT: u16 = 1;
const SET_BLUR_REGION: u16 = 1;
const REGION_ADD: u16 = 1;

struct Fixture {
    connection: Connection,
    peer: UnixStream,
    state: WindowState<()>,
    compositor: WlCompositor,
    surface: WlSurface,
    effect_manager: u32,
}

/// A layer surface whose state holds both blur managers, as the event loop has it once
/// running: `wl_compositor` taken, its cached copy kept.
fn fixture() -> Fixture {
    let (socket, mut peer) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    let connection = Connection::from_socket(socket).unwrap();
    let queue = connection.new_event_queue::<WindowState<()>>();
    let qh = queue.handle();
    let registry = connection.display().get_registry(&qh, ());
    let compositor = registry.bind::<WlCompositor, _, _>(1, 4, &qh, ());
    let layer_shell = registry.bind::<ZwlrLayerShellV1, _, _>(2, 4, &qh, ());
    let blur_manager = registry
        .bind::<blur::org_kde_kwin_blur_manager::OrgKdeKwinBlurManager, _, _>(3, 6, &qh, ());
    let effect_manager = registry
        .bind::<background_effect::ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1, _, _>(
            4,
            1,
            &qh,
            (),
        );
    let surface = compositor.create_surface(&qh, ());
    let layer = layer_shell.get_layer_surface(&surface, None, Layer::Top, "bar".into(), &qh, ());
    let mut state = WindowState::<()>::new("background-effect-test");
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
    let effect_manager_id = effect_manager.id().protocol_id();
    state.blur_manager = Some(blur_manager);
    state.background_effect_manager = Some(effect_manager);
    state.cached_compositor = Some(compositor.clone());
    connection.flush().unwrap();
    requests(&mut peer);
    Fixture {
        connection,
        peer,
        state,
        compositor,
        surface,
        effect_manager: effect_manager_id,
    }
}

impl Fixture {
    fn sent(&mut self) -> Vec<(u32, u16, Vec<u32>)> {
        self.connection.flush().unwrap();
        requests(&mut self.peer)
    }
}

/// The effect object made for the surface and the region it was given, from the requests.
fn effect(sent: &[(u32, u16, Vec<u32>)], manager: u32, surface: u32) -> (u32, u32) {
    let effect = sent
        .iter()
        .find(|(object, opcode, args)| {
            *object == manager && *opcode == GET_BACKGROUND_EFFECT && args[1] == surface
        })
        .map(|(_, _, args)| args[0])
        .expect("get_background_effect for the surface");
    let region = sent
        .iter()
        .find(|(object, opcode, _)| *object == effect && *opcode == SET_BLUR_REGION)
        .map(|(_, _, args)| args[0])
        .expect("set_blur_region on its effect");
    (effect, region)
}

#[test]
fn whole_surface_blur_reaches_the_standard_protocol_while_the_loop_runs() {
    let mut f = fixture();
    let surface = f.surface.clone();
    f.state.set_blur_for_surface(&surface, true);
    let sent = f.sent();
    let (_, region) = effect(&sent, f.effect_manager, surface.id().protocol_id());
    let (x, y, w, h) = background_effect::WHOLE_SURFACE;
    assert!(
        sent.contains(&(
            region,
            REGION_ADD,
            vec![x as u32, y as u32, w as u32, h as u32]
        )),
        "the oversized region the compositor clips to the surface"
    );
}

#[test]
fn a_blur_region_is_sent_on_both_protocols() {
    let mut f = fixture();
    let surface = f.surface.clone();
    let region = f.compositor.create_region(&f.state.units[0].qh, ());
    f.state
        .set_blur_region_for_surface(&surface, &region, &[], &[], &[], |r| r.add(10, 20, 300, 40));
    let sent = f.sent();
    let (_, sent_region) = effect(&sent, f.effect_manager, surface.id().protocol_id());
    assert_eq!(sent_region, region.id().protocol_id());
}
