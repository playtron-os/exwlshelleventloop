use super::{Shell, WindowState, WindowStateUnitBuilder, id};
use std::os::unix::net::UnixStream;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::{Connection, Dispatch};
use wayland_protocols::xdg::shell::client::{
    xdg_surface::{self, XdgSurface},
    xdg_toplevel::{self, XdgToplevel},
    xdg_wm_base::XdgWmBase,
};

/// A base window is an xdg toplevel. Once its first configure is acked it may
/// present; it used to stay unconfigured, so it never drew and never mapped.
#[test]
fn a_configured_toplevel_presents_its_first_frame() {
    let (socket, _peer) = UnixStream::pair().unwrap();
    let connection = Connection::from_socket(socket).unwrap();
    let queue = connection.new_event_queue::<WindowState<()>>();
    let qh = queue.handle();
    let registry = connection.display().get_registry(&qh, ());
    let compositor = registry.bind::<WlCompositor, _, _>(1, 4, &qh, ());
    let wm_base = registry.bind::<XdgWmBase, _, _>(2, 6, &qh, ());
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg.get_toplevel(&qh, ());
    let mut state = WindowState::<()>::new("toplevel-configure-test");
    state.push_window(
        WindowStateUnitBuilder::new(
            id::Id::unique(),
            qh.clone(),
            connection.display(),
            surface,
            Shell::XdgTopLevel((toplevel.clone(), xdg.clone(), None)),
        )
        .build(),
    );
    assert!(
        !state.has_due_frame(),
        "nothing presents before the configure"
    );

    <WindowState<()> as Dispatch<XdgToplevel, ()>>::event(
        &mut state,
        &toplevel,
        xdg_toplevel::Event::Configure {
            width: 1280,
            height: 800,
            states: Vec::new(),
        },
        &(),
        &connection,
        &qh,
    );
    <WindowState<()> as Dispatch<XdgSurface, ()>>::event(
        &mut state,
        &xdg,
        xdg_surface::Event::Configure { serial: 1 },
        &(),
        &connection,
        &qh,
    );

    assert!(
        state.has_due_frame(),
        "the configured toplevel has a frame due"
    );
    assert!(state.units[0].take_present_slot());
}
