use super::{CursorUpdateContext, WindowState, WpCursorShapeManagerV1, set_cursor_shape};
use std::io::{ErrorKind, Read};
use std::os::unix::net::UnixStream;
use wayland_client::protocol::{wl_compositor::WlCompositor, wl_seat::WlSeat, wl_shm::WlShm};
use wayland_client::{Connection, Proxy};

/// Read the actual Wayland requests from the local peer; no compositor or
/// announced globals are needed to inspect client-side request encoding.
fn requests(peer: &mut UnixStream) -> Vec<(u32, u16, Vec<u32>)> {
    let mut bytes = Vec::new();
    loop {
        let mut buffer = [0; 512];
        match peer.read(&mut buffer) {
            Ok(0) => break,
            Ok(length) => bytes.extend_from_slice(&buffer[..length]),
            Err(error) if error.kind() == ErrorKind::WouldBlock => break,
            Err(error) => panic!("read requests: {error}"),
        }
    }
    let mut messages = Vec::new();
    while !bytes.is_empty() {
        let object = u32::from_ne_bytes(bytes[..4].try_into().unwrap());
        let header = u32::from_ne_bytes(bytes[4..8].try_into().unwrap());
        let length = (header >> 16) as usize;
        assert!(length >= 8 && bytes.len() >= length);
        let arguments = bytes[8..length]
            .chunks_exact(4)
            .map(|word| u32::from_ne_bytes(word.try_into().unwrap()))
            .collect();
        messages.push((object, header as u16, arguments));
        bytes.drain(..length);
    }
    messages
}

#[test]
fn hiding_sends_a_null_surface_with_or_without_the_shape_protocol() {
    for shape_protocol in [false, true] {
        let (socket, mut peer) = UnixStream::pair().unwrap();
        peer.set_nonblocking(true).unwrap();
        let connection = Connection::from_socket(socket).unwrap();
        let queue = connection.new_event_queue::<WindowState<()>>();
        let qh = queue.handle();
        let registry = connection.display().get_registry(&qh, ());
        let seat = registry.bind::<WlSeat, _, _>(1, 5, &qh, ());
        let pointer = seat.get_pointer(&qh, ());
        let context = CursorUpdateContext {
            cursor_manager: shape_protocol
                .then(|| registry.bind::<WpCursorShapeManagerV1, _, _>(2, 1, &qh, ())),
            shm: registry.bind::<WlShm, _, _>(3, 1, &qh, ()),
            wmcompositer: registry.bind::<WlCompositor, _, _>(4, 1, &qh, ()),
            qh,
            connection: connection.clone(),
        };
        connection.flush().unwrap();
        requests(&mut peer);

        set_cursor_shape(&context, "none".into(), pointer.clone(), 71);
        connection.flush().unwrap();
        assert_eq!(
            requests(&mut peer),
            vec![(pointer.id().protocol_id(), 0, vec![71, 0, 0, 0])],
            "hiding sends wl_pointer.set_cursor(serial, null, 0, 0)",
        );

        if let Some(manager) = &context.cursor_manager {
            set_cursor_shape(&context, "pointer".into(), pointer.clone(), 71);
            connection.flush().unwrap();
            let sent = requests(&mut peer);
            assert_eq!(sent.len(), 3, "get_pointer, set_shape, destroy");
            assert_eq!(sent[0].0, manager.id().protocol_id());
            assert_eq!(sent[1].2, vec![71, 4], "restore the pointer shape");
        }
    }
}
