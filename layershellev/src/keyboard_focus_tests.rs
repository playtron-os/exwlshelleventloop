use super::{DispatchMessageInner, Shell, WindowState, WindowStateUnitBuilder, id, xkb_keyboard};
use std::os::unix::net::UnixStream;
use wayland_client::protocol::{wl_compositor::WlCompositor, wl_keyboard, wl_seat::WlSeat};
use wayland_client::{Connection, Dispatch};
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::{
    Layer, ZwlrLayerShellV1,
};

#[test]
fn keyboard_enter_reports_focus_after_a_new_window_was_preselected() {
    for preselected in [true, false] {
        let (socket, _peer) = UnixStream::pair().unwrap();
        let connection = Connection::from_socket(socket).unwrap();
        let queue = connection.new_event_queue::<WindowState<()>>();
        let qh = queue.handle();
        let registry = connection.display().get_registry(&qh, ());
        let compositor = registry.bind::<WlCompositor, _, _>(1, 4, &qh, ());
        let layer_shell = registry.bind::<ZwlrLayerShellV1, _, _>(2, 4, &qh, ());
        let seat = registry.bind::<WlSeat, _, _>(3, 5, &qh, ());
        let keyboard = seat.get_keyboard(&qh, ());
        let mut state = WindowState::new("keyboard-focus-test");
        state.keyboard_state = Some(xkb_keyboard::KeyboardState::new(keyboard.clone()));

        let mut create = || {
            let surface = compositor.create_surface(&qh, ());
            let shell = layer_shell.get_layer_surface(
                &surface,
                None,
                Layer::Top,
                "keyboard-focus-test".into(),
                &qh,
                (),
            );
            let id = id::Id::unique();
            state.push_window(
                WindowStateUnitBuilder::new(
                    id,
                    qh.clone(),
                    connection.display(),
                    surface.clone(),
                    Shell::LayerShell(shell),
                )
                .build(),
            );
            (id, surface)
        };
        let (_, main) = create();
        let (drawer_id, drawer) = create();
        if !preselected {
            state.update_current_surface(Some(main.clone()));
        }
        // Creation's event can precede the consumer registering its window alias.
        state.message.clear();
        <WindowState<()> as Dispatch<wl_keyboard::WlKeyboard, ()>>::event(
            &mut state,
            &keyboard,
            wl_keyboard::Event::Leave {
                serial: 1,
                surface: main,
            },
            &(),
            &connection,
            &qh,
        );
        <WindowState<()> as Dispatch<wl_keyboard::WlKeyboard, ()>>::event(
            &mut state,
            &keyboard,
            wl_keyboard::Event::Enter {
                serial: 2,
                surface: drawer.clone(),
                keys: Vec::new(),
            },
            &(),
            &connection,
            &qh,
        );

        let focused: Vec<_> = state
            .message
            .iter()
            .filter_map(|(target, message)| match message {
                DispatchMessageInner::Focused(id) => Some((*target, *id)),
                _ => None,
            })
            .collect();
        assert_eq!(
            focused,
            [(Some(drawer_id), drawer_id)],
            "each real Enter reports focus once, even after preselection={preselected}"
        );
        assert_eq!(state.current_surface, Some(drawer));
    }
}
