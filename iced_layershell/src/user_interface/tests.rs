use super::*;
use iced::widget::{auto_focus, column, text_input};
use iced_core::widget::operation::{self, Outcome, focusable};
use iced_core::{Element, Theme, keyboard, window};

struct Discard;

impl UserInterfaceReclaim<(), Theme, ()> for Discard {
    fn reclaim(&mut self, _ui: IcedUserInterface<'static, (), Theme, ()>) {}
}

type Guard = UserInterfaceMutGuard<'static, (), Theme, (), Discard>;

fn dialog(generation: u64) -> Element<'static, (), Theme, ()> {
    column![
        text_input("First", "").id("first").on_input(|_| ()),
        auto_focus(text_input("Safe", "").id("safe").on_input(|_| ())).key(generation),
    ]
    .into()
}

fn build(generation: u64, cache: Cache) -> Guard {
    Guard {
        reclaim: Discard,
        ui: Some(IcedUserInterface::build(
            dialog(generation),
            Size::new(640.0, 480.0),
            cache,
            &mut (),
        )),
    }
}

fn focused(ui: &mut Guard) -> Option<iced_core::widget::Id> {
    let mut find = focusable::find_focused();
    ui.operate(&(), &mut operation::black_box(&mut find));
    match find.finish() {
        Outcome::Some(id) => Some(id),
        Outcome::None => None,
        Outcome::Chain(_) => panic!("focus query did not finish"),
    }
}

fn redraw(ui: &mut Guard) {
    ui.update(
        &[Event::Window(window::Event::RedrawRequested(
            std::time::Instant::now(),
        ))],
        Cursor::Unavailable,
        &mut (),
        &mut Vec::new(),
    );
}

#[test]
fn a_dialog_takes_autofocus_on_its_first_frame() {
    let mut ui = build(0, Cache::default());
    redraw(&mut ui);
    assert_eq!(focused(&mut ui), Some("safe".into()));

    ui.operate(&(), &mut focusable::focus::<()>("first".into()));
    let mut ui = build(0, ui.into_cache());
    redraw(&mut ui);
    assert_eq!(
        focused(&mut ui),
        Some("first".into()),
        "ordinary rebuilds preserve navigation"
    );

    let mut ui = build(1, ui.into_cache());
    redraw(&mut ui);
    assert_eq!(
        focused(&mut ui),
        Some("safe".into()),
        "the next dialog takes focus again"
    );
}

#[test]
fn an_input_event_services_only_its_own_window_autofocus() {
    let mut active = build(0, Cache::default());
    let mut other = build(0, Cache::default());
    active.update(
        &[Event::Keyboard(keyboard::Event::ModifiersChanged(
            keyboard::Modifiers::empty(),
        ))],
        Cursor::Unavailable,
        &mut (),
        &mut Vec::new(),
    );
    assert_eq!(focused(&mut active), Some("safe".into()));
    assert_eq!(
        focused(&mut other),
        None,
        "an untouched window stays unfocused"
    );
}
