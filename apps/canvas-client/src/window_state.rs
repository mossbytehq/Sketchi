//! Persistent native-window state for the desktop client.

use std::{fs, io, path::PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const DEFAULT_WIDTH: u32 = 1_280;
const DEFAULT_HEIGHT: u32 = 720;
const MIN_WIDTH: u32 = 320;
const MIN_HEIGHT: u32 = 240;

/// The geometry and presentation state restored for the next client launch.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub(crate) struct WindowState {
    /// Last known outer position, when the window system exposes one.
    pub(crate) position: Option<[i32; 2]>,
    /// Last known unmaximized inner size in physical pixels.
    pub(crate) inner_size: [u32; 2],
    /// Whether the window was maximized when the session ended.
    pub(crate) maximized: bool,
    /// Whether the saved geometry should be restored on the next launch.
    pub(crate) restore_session: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            position: None,
            inner_size: [DEFAULT_WIDTH, DEFAULT_HEIGHT],
            maximized: false,
            restore_session: true,
        }
    }
}

impl WindowState {
    /// Returns whether the state contains a usable window size.
    pub(crate) const fn is_valid(&self) -> bool {
        self.inner_size[0] >= MIN_WIDTH && self.inner_size[1] >= MIN_HEIGHT
    }
}

/// Errors raised while reading or writing the native-window configuration.
#[derive(Debug, Error)]
pub(crate) enum WindowStateError {
    /// The JSON file could not be read.
    #[error("could not read window state: {0}")]
    Read(#[source] io::Error),
    /// The JSON file could not be decoded or encoded.
    #[error("could not decode window state: {0}")]
    Json(#[from] serde_json::Error),
    /// The JSON file could not be written.
    #[error("could not write window state: {0}")]
    Write(#[source] io::Error),
}

fn path() -> Option<PathBuf> {
    ProjectDirs::from("org", "Sketchi", "Sketchi")
        .map(|directories| directories.config_dir().join("window-state.json"))
}

/// Loads the last saved window state.
pub(crate) fn load() -> Result<Option<WindowState>, WindowStateError> {
    let Some(path) = path() else {
        return Ok(None);
    };
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(WindowStateError::Read(error)),
    };
    let state: WindowState = serde_json::from_slice(&bytes)?;
    Ok(state.is_valid().then_some(state))
}

/// Persists the window state for the next launch.
pub(crate) fn save(state: &WindowState) -> Result<(), WindowStateError> {
    let Some(path) = path() else {
        return Ok(());
    };
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    fs::create_dir_all(parent).map_err(WindowStateError::Write)?;
    let bytes = serde_json::to_vec_pretty(state)?;
    fs::write(path, bytes).map_err(WindowStateError::Write)
}

#[cfg(test)]
mod tests {
    use super::WindowState;

    #[test]
    fn window_state_round_trips_and_preserves_restore_preferences() {
        let state = WindowState {
            position: Some([42, 84]),
            inner_size: [1_440, 900],
            maximized: true,
            restore_session: false,
        };
        let encoded = serde_json::to_vec(&state);
        assert!(encoded.is_ok());
        let encoded = encoded.unwrap_or_default();
        let decoded = serde_json::from_slice::<WindowState>(&encoded);
        assert!(decoded.is_ok());
        let decoded = decoded.unwrap_or_default();

        assert_eq!(decoded, state);
        assert!(decoded.is_valid());
    }

    #[test]
    fn invalid_window_sizes_are_not_restored() {
        let state = WindowState {
            inner_size: [1, 1],
            ..WindowState::default()
        };

        assert!(!state.is_valid());
    }

    #[test]
    fn missing_restore_preference_defaults_to_enabled() {
        let state = serde_json::from_str::<WindowState>(
            r#"{"position":null,"inner_size":[800,600],"maximized":false}"#,
        );
        assert!(state.is_ok());
        let state = state.unwrap_or_default();

        assert!(state.restore_session);
    }
}
