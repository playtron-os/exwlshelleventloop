use crate::reexport::{Anchor, Layer, WlRegion};
use iced_core::window::Id as IcedId;
use layershellev::{NewInputPanelSettings, NewLayerShellSettings, NewXdgWindowSettings};

// Re-export VisibilityMode for consumers
pub use layershellev::home_visibility::VisibilityMode;

// Re-export ToplevelAction for consumers
#[cfg(feature = "foreign-toplevel")]
pub use layershellev::foreign_toplevel::ToplevelAction;

use std::sync::Arc;

#[derive(Debug, PartialEq, Eq, Clone, Copy, Default)]
pub struct IcedXdgWindowSettings {
    pub maximized: bool,
    pub size: Option<(u32, u32)>,
}

impl From<IcedXdgWindowSettings> for NewXdgWindowSettings {
    fn from(val: IcedXdgWindowSettings) -> Self {
        NewXdgWindowSettings {
            maximized: val.maximized,
            title: None,
            size: val.size,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct IcedNewPopupSettings {
    pub size: (u32, u32),
    pub position: (i32, i32),
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum MenuDirection {
    Up,
    Down,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct IcedNewMenuSettings {
    pub size: (u32, u32),
    pub direction: MenuDirection,
}

type Callback = Arc<dyn Fn(&WlRegion) + Send + Sync>;

// Callback wrapper around dyn Fn(&WlRegion)
#[derive(Clone)]
pub struct ActionCallback(pub Callback);

impl std::fmt::Debug for ActionCallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "callback function")
    }
}

impl ActionCallback {
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(&WlRegion) + Send + Sync + 'static,
    {
        ActionCallback(Arc::new(callback))
    }
}

/// NOTE: DO NOT USE THIS ENUM DIERCTLY
/// use macro to_layer_message
#[derive(Debug, Clone)]
pub enum LayershellCustomAction {
    AnchorChange(Anchor),
    LayerChange(Layer),
    AnchorSizeChange(Anchor, (u32, u32)),
    MarginChange((i32, i32, i32, i32)),
    SizeChange((u32, u32)),
    CornerRadiusChange(Option<[u32; 4]>),
    /// Enable compositor-driven auto-hide for the surface. The compositor will
    /// animate hide/show transitions based on the specified mode.
    /// `edge`: which edge to slide off (0 = bottom)
    /// `edge_zone`: hover detection zone in pixels at the screen edge
    /// `mode`: 0 = always hide when cursor leaves, 1 = only hide when maximized/fullscreen exists
    AutoHideChange {
        edge: u32,
        edge_zone: u32,
        mode: u32,
    },
    /// Disable compositor-driven auto-hide for the surface.
    AutoHideUnset,
    ExclusiveZoneChange(i32),
    VirtualKeyboardPressed {
        time: u32,
        key: u32,
    },
    // settings, info, single_tone
    NewLayerShell {
        settings: NewLayerShellSettings,
        id: IcedId,
    },
    SetInputRegion(ActionCallback),
    NewPopUp {
        settings: IcedNewPopupSettings,
        id: IcedId,
    },
    NewBaseWindow {
        settings: IcedXdgWindowSettings,
        id: IcedId,
    },
    NewMenu {
        settings: IcedNewMenuSettings,
        id: IcedId,
    },
    NewInputPanel {
        settings: NewInputPanelSettings,
        id: IcedId,
    },
    /// is same with WindowAction::Close(id)
    RemoveWindow,
    ForgetLastOutput,
    /// Hide the window without destroying it (uses layer_surface_visibility protocol)
    /// The surface is not rendered and doesn't receive input, but maintains its configuration.
    HideWindow,
    /// Show the window if it was previously hidden
    ShowWindow,
    /// Change the home visibility mode for the surface
    VisibilityModeChange(VisibilityMode),
    /// Send audio level to compositor for voice orb visualization (0-1000)
    SetVoiceAudioLevel(u32),
    /// Acknowledge a will_stop event from the compositor.
    /// serial - the serial from the will_stop event
    /// freeze - if true, freeze the orb in place for processing.
    ///          if false, proceed with hiding the orb.
    VoiceAckStop(u32, bool),
    /// Dismiss the frozen voice orb.
    /// This tells the compositor to hide the orb when transcription completes
    /// without spawning a new window (e.g., empty result or error).
    VoiceDismiss,
    /// Execute a toplevel action (activate, close, minimize, etc.)
    #[cfg(feature = "foreign-toplevel")]
    ToplevelAction(ToplevelAction),
    /// Arm dismiss notifications for this window.
    /// Once armed, a DismissRequested event will be sent when the user
    /// clicks/touches outside the window's dismiss group.
    ArmDismiss,
    /// Disarm dismiss notifications for this window.
    DisarmDismiss,
    /// Add the main panel surface to this popup's dismiss group.
    /// When the popup is armed, clicks outside both the popup and the panel
    /// will trigger dismiss.
    AddMainSurfaceToDismissGroup,
    /// Add ALL panel/window surfaces to this popup's dismiss group.
    /// In AllScreens mode each monitor has its own panel surface; this
    /// ensures that clicking any of them won't trigger dismiss.
    AddAllSurfacesToDismissGroup,
    /// Remove the main panel surface from this popup's dismiss group.
    RemoveMainSurfaceFromDismissGroup,
}

/// Please do not use this struct directly
/// Use macro to_layer_message instead
#[derive(Debug, Clone)]
pub struct LayershellCustomActionWithId(pub Option<IcedId>, pub LayershellCustomAction);

impl LayershellCustomActionWithId {
    pub fn new(id: Option<IcedId>, action: LayershellCustomAction) -> Self {
        Self(id, action)
    }
}
