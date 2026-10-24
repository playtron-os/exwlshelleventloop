//! A bar whose yellow tooltip the compositor keeps 16, 20 logical pixels below and right of the
//! pointer while it is over the bar (zcosmic_tooltip_manager_v1, as cosmic-comp offers it).

use std::fs::File;
use std::io::Write;
use std::os::fd::AsFd;

use layershellev::reexport::*;
use layershellev::*;

const BAR: u32 = 0xFF40_4040;
const TOOLTIP: u32 = 0xFFFF_D000;

fn main() {
    let ev: WindowState<()> = WindowState::new("compositor-tooltip")
        .with_size((0, 120))
        .with_layer(Layer::Top)
        .with_anchor(Anchor::Bottom | Anchor::Left | Anchor::Right)
        .with_exclusive_zone(-1)
        .build()
        .unwrap();

    let tooltip = id::Id::unique();
    let mut opened = false;
    ev.running(move |event, _ev, index| match event {
        LayerShellEvent::RequestBuffer(file, shm, qh, width, height) => {
            let colour = if index == Some(tooltip) { TOOLTIP } else { BAR };
            fill(file, (width, height), colour);
            let pool = shm.create_pool(file.as_fd(), (width * height * 4) as i32, qh, ());
            ReturnData::WlBuffer(pool.create_buffer(
                0,
                width as i32,
                height as i32,
                (width * 4) as i32,
                wl_shm::Format::Argb8888,
                qh,
                (),
            ))
        }
        LayerShellEvent::RequestMessages(DispatchMessage::MouseEnter { .. })
            if !opened && index.is_some() && index != Some(tooltip) =>
        {
            opened = true;
            ReturnData::NewPopUp((
                NewPopUpSettings {
                    size: (160, 36),
                    position: (0, 0),
                    id: index.unwrap(),
                    blur: false,
                    shadow: false,
                    corner_radius: None,
                    auto_size: false,
                    anchor_rect_size: Some((1, 1)),
                    anchor: 0,
                    gravity: 0,
                    constraint_adjustment: 0,
                    offset: None,
                    reactive: false,
                    grab: false,
                    input_passthrough: true,
                    tooltip_offset: Some((16, 20)),
                    tooltip_anchor: Some(3),
                    tooltip_delay_ms: None,
                    capture_excluded: false,
                },
                tooltip,
                None,
            ))
        }
        _ => ReturnData::None,
    })
    .unwrap();
}

fn fill(file: &mut File, (width, height): (u32, u32), colour: u32) {
    let mut out = std::io::BufWriter::new(file);
    for _ in 0..width * height {
        out.write_all(&colour.to_ne_bytes()).unwrap();
    }
    out.flush().unwrap();
}
