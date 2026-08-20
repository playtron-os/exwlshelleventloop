//! Holding back the reveal of a newly started session.
//!
//! At login the compositor keeps the outgoing session's final frame on screen
//! and cross-fades out of it as soon as there is something to show — normally
//! the moment the wallpaper is opaque. An application that means to cover the
//! session before the user sees it, such as a first-run experience, has not
//! drawn anything by then, so without help the user sees the desktop for a beat
//! and then the app drops on top of it.
//!
//! `cosmic_session_hold_v1` asks the compositor to keep waiting. The compositor
//! never learns which application this is: a hold is just an object, so an
//! application that decides it has nothing to show simply never takes one and
//! nothing waits for it.
//!
//! ```no_run
//! // In `main`, before anything that can take time or exit.
//! let hold = iced_layershell::session_hold::claim(std::time::Duration::from_secs(3));
//! // ...then drop `hold` once your surfaces are actually on screen.
//! ```
//!
//! Deliberately on its own Wayland connection rather than the one the runtime
//! uses. A hold is only worth anything if it is claimed within milliseconds of
//! `main` — the compositor is deciding whether to fade right now — and the
//! runtime's connection does not exist until an application has been built and
//! started, most of a second later. A second `wl_display` costs about a
//! millisecond.

use std::time::Duration;

use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
    protocol::wl_registry,
};

pub mod protocol {
    //! Generated client bindings for `session-hold.xml`.
    use wayland_client;

    pub mod __interfaces {
        use wayland_backend;
        wayland_scanner::generate_interfaces!("./protocols/session-hold.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("./protocols/session-hold.xml");
}

use protocol::{cosmic_session_hold_manager_v1, cosmic_session_hold_v1};

/// An outstanding request that the session not be revealed yet.
///
/// Releases on drop, which is the point: every early exit in `main` — already
/// seen, `--check`, a failed bundle lookup, another instance holding the lock,
/// a panic — releases the hold without having to remember to.
pub struct Hold {
    // Dropped in declaration order: the hold object, then the connection it
    // belongs to.
    hold: cosmic_session_hold_v1::CosmicSessionHoldV1,
    _conn: Connection,
}

impl Drop for Hold {
    fn drop(&mut self) {
        self.hold.release();
        // The release is a request like any other; without a flush it would sit
        // in the buffer and leave the compositor waiting for a process that has
        // already gone.
        let _ = self._conn.flush();
    }
}

struct State;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<cosmic_session_hold_manager_v1::CosmicSessionHoldManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &cosmic_session_hold_manager_v1::CosmicSessionHoldManagerV1,
        _: cosmic_session_hold_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<cosmic_session_hold_v1::CosmicSessionHoldV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &cosmic_session_hold_v1::CosmicSessionHoldV1,
        event: cosmic_session_hold_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The only event this interface has. Not an error: the compositor is
        // entitled to stop waiting, and says so rather than leaving us to
        // wonder why the desktop appeared underneath.
        let cosmic_session_hold_v1::Event::Expired = event;
        tracing::debug!(
            "session hold: compositor stopped waiting; the welcome will appear over the session"
        );
    }
}

/// Ask the compositor to keep the previous session's frame up.
///
/// `expected` is how long the caller believes it needs, as a hint; the
/// compositor applies its own cap regardless, so a hold cannot strand a stale
/// frame.
///
/// Returns `None` when there is nothing to hold — a compositor without this
/// protocol, or no Wayland connection at all. That is not a failure worth
/// reporting upwards: the application still shows, just possibly after a flash
/// of whatever was underneath.
pub fn claim(expected: Duration) -> Option<Hold> {
    let conn = match Connection::connect_to_env() {
        Ok(conn) => conn,
        Err(e) => {
            tracing::debug!("session hold: no wayland connection ({e})");
            return None;
        }
    };
    let (globals, queue) = match registry_queue_init::<State>(&conn) {
        Ok(pair) => pair,
        Err(e) => {
            tracing::debug!("session hold: could not read the registry ({e})");
            return None;
        }
    };
    let qh = queue.handle();

    let manager: cosmic_session_hold_manager_v1::CosmicSessionHoldManagerV1 =
        match globals.bind(&qh, 1..=1, ()) {
            Ok(manager) => manager,
            Err(e) => {
                // The usual case on a compositor that does not implement this,
                // which is not a problem: the welcome still shows.
                tracing::debug!("session hold: compositor does not offer one ({e})");
                return None;
            }
        };

    let hold = manager.hold(expected.as_millis().min(u32::MAX as u128) as u32, &qh, ());
    // The claim has to reach the compositor now, not whenever something else
    // happens to flush — it is racing the fade it exists to prevent.
    conn.flush().ok()?;

    tracing::debug!("asked the compositor to hold the session until the welcome is up");
    Some(Hold { hold, _conn: conn })
}
