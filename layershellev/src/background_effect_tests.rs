use super::cursor_tests::requests;
use super::{Shell, WindowState, WindowStateUnitBuilder, background_effect, blur, id};
use std::io::Write;
use std::os::unix::net::UnixStream;
use wayland_client::protocol::{wl_compositor::WlCompositor, wl_surface::WlSurface};
use wayland_client::{Connection, Proxy, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::{
    Layer, ZwlrLayerShellV1,
};

const GET_BACKGROUND_EFFECT: u16 = 1;
const SET_BLUR_REGION: u16 = 1;
const REGION_ADD: u16 = 1;
const DESTROY: u16 = 0;
const KDE_CREATE: u16 = 0;
const DELETE_ID: u16 = 1;

struct Fixture {
    connection: Connection,
    peer: UnixStream,
    qh: QueueHandle<WindowState<()>>,
    state: WindowState<()>,
    compositor: WlCompositor,
    layer_shell: ZwlrLayerShellV1,
    surface: WlSurface,
    effect_manager: u32,
}

/// A layer surface whose state holds the background-effect manager, and the KDE one when
/// `kde`, as the event loop has it once running: `wl_compositor` taken, its cached copy kept.
fn fixture(kde: bool) -> Fixture {
    let (socket, peer) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    let connection = Connection::from_socket(socket).unwrap();
    let queue = connection.new_event_queue::<WindowState<()>>();
    let qh = queue.handle();
    let registry = connection.display().get_registry(&qh, ());
    let compositor = registry.bind::<WlCompositor, _, _>(1, 4, &qh, ());
    let layer_shell = registry.bind::<ZwlrLayerShellV1, _, _>(2, 4, &qh, ());
    let blur_manager = kde.then(|| {
        registry.bind::<blur::org_kde_kwin_blur_manager::OrgKdeKwinBlurManager, _, _>(3, 6, &qh, ())
    });
    let effect_manager = registry
        .bind::<background_effect::ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1, _, _>(
            4,
            1,
            &qh,
            (),
        );
    let surface = compositor.create_surface(&qh, ());
    let mut state = WindowState::<()>::new("background-effect-test");
    let effect_manager_id = effect_manager.id().protocol_id();
    state.blur_manager = blur_manager;
    state.background_effect_manager = Some(effect_manager);
    state.cached_compositor = Some(compositor.clone());
    let mut fixture = Fixture {
        connection,
        peer,
        qh,
        state,
        compositor,
        layer_shell,
        surface: surface.clone(),
        effect_manager: effect_manager_id,
    };
    fixture.map(&surface);
    fixture.sent();
    fixture
}

impl Fixture {
    fn sent(&mut self) -> Vec<(u32, u16, Vec<u32>)> {
        self.connection.flush().unwrap();
        requests(&mut self.peer)
    }

    /// Give `surface` a layer surface and a unit the loop can close.
    fn map(&mut self, surface: &WlSurface) {
        let layer = self.layer_shell.get_layer_surface(
            surface,
            None,
            Layer::Top,
            "bar".into(),
            &self.qh,
            (),
        );
        self.state.push_window(
            WindowStateUnitBuilder::new(
                id::Id::unique(),
                self.qh.clone(),
                self.connection.display(),
                surface.clone(),
                Shell::LayerShell(layer),
            )
            .becreated(true)
            .build(),
        );
    }

    /// The compositor's `wl_display.delete_id` for each of `objects`, read in that order.
    fn delete_ids(&mut self, objects: &[u32]) {
        for object in objects {
            let length = 12u32;
            let mut bytes = 1u32.to_ne_bytes().to_vec();
            bytes.extend((length << 16 | u32::from(DELETE_ID)).to_ne_bytes());
            bytes.extend(object.to_ne_bytes());
            self.peer.write_all(&bytes).unwrap();
        }
        self.connection.prepare_read().unwrap().read().unwrap();
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
    let mut f = fixture(true);
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
    let mut f = fixture(true);
    let surface = f.surface.clone();
    let region = f.compositor.create_region(&f.state.units[0].qh, ());
    f.state
        .set_blur_region_for_surface(&surface, &region, &[], &[], &[], |r| r.add(10, 20, 300, 40));
    let sent = f.sent();
    let (_, sent_region) = effect(&sent, f.effect_manager, surface.id().protocol_id());
    assert_eq!(sent_region, region.id().protocol_id());
}

// A compositor with only the standard protocol blurs the whole surface too, and drops it.
#[test]
fn whole_surface_blur_needs_no_kde_manager() {
    let mut f = fixture(false);
    let surface = f.surface.clone();
    f.state.set_blur_for_surface(&surface, true);
    let sent = f.sent();
    let (effect, region) = effect(&sent, f.effect_manager, surface.id().protocol_id());
    let (x, y, w, h) = background_effect::WHOLE_SURFACE;
    assert!(sent.contains(&(
        region,
        REGION_ADD,
        vec![x as u32, y as u32, w as u32, h as u32]
    )));
    assert_eq!(
        sent.last().map(|(object, _, _)| *object),
        Some(surface.id().protocol_id())
    );

    f.state.set_blur_for_surface(&surface, false);
    assert_eq!(
        f.sent(),
        vec![
            (effect, SET_BLUR_REGION, vec![0]),
            (effect, DESTROY, vec![]),
            (surface.id().protocol_id(), 6, vec![]),
        ],
        "set_blur_region(null), destroy, then wl_surface.commit",
    );
}

// libwayland hands a destroyed object's id to the next object made, so a popover opened after a
// tooltip closed often gets the tooltip's surface id. It must still blur.
#[test]
fn a_surface_on_a_closed_surfaces_id_still_blurs() {
    let mut f = fixture(true);
    let closed = f.surface.clone();
    let id = closed.id().protocol_id();
    f.state.set_blur_for_surface(&closed, true);
    let (old_effect, _) = effect(&f.sent(), f.effect_manager, id);

    let unit = f.state.units[0].id;
    f.state.remove_shell(unit).unwrap();
    let sent = f.sent();
    let destroyed = |object| {
        sent.iter()
            .position(|request| *request == (object, DESTROY, vec![]))
    };
    assert!(
        destroyed(old_effect).is_some_and(|at| Some(at) < destroyed(id)),
        "the effect is destroyed before its surface: {sent:?}",
    );

    // Every request was a destructor. The surface's id is freed last, so it is reused first.
    let mut freed: Vec<u32> = sent.iter().map(|(object, ..)| *object).collect();
    freed.retain(|object| *object != id);
    freed.push(id);
    f.delete_ids(&freed);
    let reopened = f.compositor.create_surface(&f.qh, ());
    assert_eq!(reopened.id().protocol_id(), id, "libwayland reused the id");
    f.map(&reopened);
    f.sent();

    f.state.set_blur_for_surface(&reopened, true);
    let sent = f.sent();
    effect(&sent, f.effect_manager, id);
    let kde = f.state.blur_manager.as_ref().unwrap().id().protocol_id();
    assert!(
        sent.iter()
            .any(|(object, opcode, args)| *object == kde && *opcode == KDE_CREATE && args[1] == id),
        "a KDE blur for the new surface too: {sent:?}",
    );
}
