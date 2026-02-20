use iced_core::clipboard::{Content, Error, Kind};
use layershellev::WindowWrapper;

pub struct LayerShellClipboard {
    state: State,
}

enum State {
    Connected(window_clipboard::Clipboard),
    Unavailable,
}

impl LayerShellClipboard {
    /// Creates a new [`Clipboard`] for the given window.
    pub fn connect(window: &WindowWrapper) -> Self {
        #[allow(unsafe_code)]
        let state = unsafe { window_clipboard::Clipboard::connect(window) }
            .ok()
            .map(State::Connected)
            .unwrap_or(State::Unavailable);

        Self { state }
    }

    /// Creates a new [`Clipboard`] that isn't associated with a window.
    /// This clipboard will never contain a copied value.
    #[allow(unused)]
    pub fn unconnected() -> Self {
        Self {
            state: State::Unavailable,
        }
    }

    /// Reads the current content of the [`Clipboard`].
    pub fn read(
        &self,
        kind: Kind,
        callback: impl FnOnce(Result<Content, Error>) + Send + 'static,
    ) {
        match &self.state {
            State::Connected(clipboard) => {
                let result = match kind {
                    Kind::Text => clipboard.read().map(Content::Text).map_err(|_| Error::ContentNotAvailable),
                    _ => Err(Error::ContentNotAvailable),
                };
                callback(result);
            }
            State::Unavailable => {
                callback(Err(Error::ClipboardUnavailable));
            }
        }
    }

    /// Writes the given content to the [`Clipboard`].
    pub fn write(
        &mut self,
        content: Content,
        callback: impl FnOnce(Result<(), Error>) + Send + 'static,
    ) {
        match &mut self.state {
            State::Connected(clipboard) => {
                let result = match content {
                    Content::Text(text) => clipboard.write(text).map_err(|e| {
                        log::warn!("error writing to clipboard: {e}");
                        Error::ContentNotAvailable
                    }),
                    _ => Err(Error::ContentNotAvailable),
                };
                callback(result);
            }
            State::Unavailable => {
                callback(Err(Error::ClipboardUnavailable));
            }
        }
    }
}
