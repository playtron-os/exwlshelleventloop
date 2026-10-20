use super::{NewXdgWindowSettings, ReturnData, WindowState, id};

/// A windowless daemon asked to open its window (an IPC "open") has no surface
/// to bring a Wayland event, so the window must not wait for one.
#[test]
fn a_requested_window_runs_the_next_pass_at_once() {
    let mut state = WindowState::<()>::new("requested-surface-test");
    assert!(
        !state.pass_due_now(false),
        "an idle app with no surface sleeps"
    );

    state.append_return_data(ReturnData::NewXdgBase((
        NewXdgWindowSettings::default(),
        id::Id::unique(),
        None,
    )));
    assert!(
        state.pass_due_now(false),
        "the pass that makes the window runs now"
    );
}
