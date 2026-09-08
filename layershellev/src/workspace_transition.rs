//! Client-side binding for `workspace_transition_manager_v1`.
//!
//! The compositor reports when a workspace switch is animating and for how
//! long, so a shell surface can fade its contents across it.

// Re-export generated types
pub use generated::workspace_transition_manager_v1;

#[allow(
    non_snake_case,
    non_upper_case_globals,
    non_camel_case_types,
    dead_code,
    unused_imports
)]
mod generated {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_backend;
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/workspace-transition.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/workspace-transition.xml");
}

/// A workspace switch, as reported by the compositor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceTransition {
    /// A switch began animating. `duration_ms` is the compositor's own
    /// animation length — the window a client has to fade in.
    Started {
        from: Option<String>,
        to: String,
        duration_ms: u32,
    },
    /// The switch finished; a client mid-fade can settle.
    Finished { to: String },
}
