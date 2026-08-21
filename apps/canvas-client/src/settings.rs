//! Persistent application preferences used by the desktop client.

use std::{collections::BTreeMap, fs, io, path::PathBuf, time::Duration};

use canvas_core::Style;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[allow(unused_imports)]
use crate::{
    theme::ThemeTokens,
    update::{UpdateCache, UpdateChannel},
};

const SETTINGS_VERSION: u32 = 6;

/// Persisted appearance preference.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum Appearance {
    /// Follow the operating-system theme.
    System,
    /// Use the light theme.
    Light,
    /// Use the dark theme.
    Dark,
}

/// Persisted automatic-save interval.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum AutosaveInterval {
    /// Save every 30 seconds.
    ThirtySeconds,
    /// Save every minute.
    OneMinute,
    /// Save every five minutes.
    FiveMinutes,
    /// Save every ten minutes.
    TenMinutes,
    /// Disable automatic saving.
    Never,
}

/// Local canvas surface preference; it is not part of the shared document.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum CanvasBackground {
    /// Show the subtle navigation grid behind the document.
    #[default]
    DotGrid,
    /// Use only the configured canvas color behind the document.
    Clean,
}

/// A serializable keyboard shortcut.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct KeyBinding {
    /// Stable egui key name.
    pub(crate) key: String,
    /// Alt modifier.
    #[serde(default)]
    pub(crate) alt: bool,
    /// Control modifier.
    #[serde(default)]
    pub(crate) ctrl: bool,
    /// Shift modifier.
    #[serde(default)]
    pub(crate) shift: bool,
    /// macOS Command modifier.
    #[serde(default)]
    pub(crate) mac_cmd: bool,
    /// Platform command modifier.
    #[serde(default)]
    pub(crate) command: bool,
}

/// User preferences stored independently from native window geometry.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub(crate) struct Settings {
    /// Settings schema version.
    pub(crate) version: u32,
    /// Canvas appearance mode.
    pub(crate) appearance: Appearance,
    /// Automatic-save interval.
    pub(crate) autosave_interval: AutosaveInterval,
    /// Automatic-save destination.
    pub(crate) autosave_directory: String,
    /// Light canvas background RGBA.
    pub(crate) light_canvas_color: [u8; 4],
    /// Dark canvas background RGBA.
    pub(crate) dark_canvas_color: [u8; 4],
    /// Local canvas surface style.
    pub(crate) canvas_background: CanvasBackground,
    /// Light-mode palette RGBA colors.
    pub(crate) light_palette: Vec<[u8; 4]>,
    /// Dark-mode palette RGBA colors.
    pub(crate) dark_palette: Vec<[u8; 4]>,
    /// Freehand stabilization amount.
    pub(crate) stabilization: f32,
    /// Input pressure sensitivity amount.
    pub(crate) pressure_sensitivity: f32,
    /// Whether the last-used style should be restored for new objects.
    pub(crate) remember_drawing_style: bool,
    /// Last-used style for newly created objects, when persistence is enabled.
    pub(crate) drawing_style: Option<Style>,
    /// Shortcuts keyed by their stable action label.
    pub(crate) keybinds: BTreeMap<String, KeyBinding>,
    /// Release stream used by the update checker.
    pub(crate) update_channel: UpdateChannel,
    /// Last successful update lookup.
    pub(crate) update_cache: UpdateCache,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            appearance: Appearance::System,
            autosave_interval: AutosaveInterval::OneMinute,
            autosave_directory: default_autosave_directory(),
            light_canvas_color: rgba(252, 252, 253, 255),
            dark_canvas_color: rgba(26, 27, 30, 255),
            canvas_background: CanvasBackground::DotGrid,
            light_palette: [
                rgba(31, 31, 31, 255),
                rgba(224, 49, 49, 255),
                rgba(240, 140, 0, 255),
                rgba(255, 236, 153, 255),
                rgba(47, 158, 68, 255),
                rgba(190, 242, 200, 255),
                rgba(25, 113, 194, 255),
                rgba(190, 224, 255, 255),
                rgba(121, 80, 242, 255),
                rgba(255, 201, 201, 255),
                rgba(221, 214, 254, 255),
                rgba(82, 82, 91, 255),
                rgba(145, 145, 155, 255),
                rgba(224, 224, 230, 255),
                rgba(255, 255, 255, 255),
            ]
            .into_iter()
            .collect(),
            dark_palette: [
                rgba(245, 245, 245, 255),
                rgba(224, 49, 49, 255),
                rgba(245, 159, 0, 255),
                rgba(255, 236, 153, 255),
                rgba(82, 196, 104, 255),
                rgba(190, 242, 200, 255),
                rgba(66, 153, 225, 255),
                rgba(190, 224, 255, 255),
                rgba(145, 120, 242, 255),
                rgba(255, 201, 201, 255),
                rgba(221, 214, 254, 255),
                rgba(82, 82, 91, 255),
                rgba(145, 145, 155, 255),
                rgba(224, 224, 230, 255),
                rgba(31, 35, 45, 255),
            ]
            .into_iter()
            .collect(),
            stabilization: 0.5,
            pressure_sensitivity: 0.5,
            remember_drawing_style: true,
            drawing_style: None,
            keybinds: BTreeMap::new(),
            update_channel: UpdateChannel::default(),
            update_cache: UpdateCache::default(),
        }
    }
}

impl AutosaveInterval {
    /// Returns the configured cadence, or `None` when automatic saving is off.
    pub(crate) const fn duration(self) -> Option<Duration> {
        match self {
            Self::ThirtySeconds => Some(Duration::from_secs(30)),
            Self::OneMinute => Some(Duration::from_mins(1)),
            Self::FiveMinutes => Some(Duration::from_mins(5)),
            Self::TenMinutes => Some(Duration::from_mins(10)),
            Self::Never => None,
        }
    }
}

/// Settings persistence failures.
#[derive(Debug, Error)]
pub(crate) enum SettingsError {
    /// Settings file could not be read.
    #[error("could not read settings: {0}")]
    Read(#[source] io::Error),
    /// Settings JSON was invalid.
    #[error("could not decode settings: {0}")]
    Json(#[from] serde_json::Error),
    /// Settings file could not be written.
    #[error("could not write settings: {0}")]
    Write(#[source] io::Error),
}

fn default_autosave_directory() -> String {
    ProjectDirs::from("org", "Sketchi", "Sketchi")
        .map_or_else(
            || PathBuf::from("autosave"),
            |directories| directories.data_local_dir().join("autosave"),
        )
        .to_string_lossy()
        .into_owned()
}

const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> [u8; 4] {
    [red, green, blue, alpha]
}

fn path() -> Option<PathBuf> {
    ProjectDirs::from("org", "Sketchi", "Sketchi")
        .map(|directories| directories.config_dir().join("settings.json"))
}

/// Loads preferences, returning `None` when no settings have been saved yet.
pub(crate) fn load() -> Result<Option<Settings>, SettingsError> {
    let Some(path) = path() else {
        return Ok(None);
    };
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(SettingsError::Read(error)),
    };
    Ok(Some(serde_json::from_slice(&bytes)?))
}

/// Saves preferences to the per-user configuration directory.
pub(crate) fn save(settings: &Settings) -> Result<(), SettingsError> {
    let Some(path) = path() else {
        return Ok(());
    };
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    fs::create_dir_all(parent).map_err(SettingsError::Write)?;
    let bytes = serde_json::to_vec_pretty(settings)?;
    fs::write(path, bytes).map_err(SettingsError::Write)
}

#[cfg(test)]
mod tests {
    use super::{Appearance, AutosaveInterval, CanvasBackground, Settings};
    use crate::update::UpdateChannel;

    #[test]
    fn defaults_are_complete_and_round_trip() {
        let settings = Settings::default();
        assert_eq!(settings.appearance, Appearance::System);
        assert_eq!(settings.autosave_interval, AutosaveInterval::OneMinute);
        assert!(settings.remember_drawing_style);
        assert!(settings.drawing_style.is_none());
        assert_eq!(settings.update_channel, UpdateChannel::Stable);
        assert!(settings.update_cache.checked_at_epoch.is_none());
        assert_eq!(settings.canvas_background, CanvasBackground::DotGrid);
        assert_eq!(settings.light_palette.len(), 15);
        assert_eq!(settings.dark_palette.len(), 15);

        let encoded = serde_json::to_vec(&settings);
        assert!(encoded.is_ok());
        let decoded = serde_json::from_slice::<Settings>(&encoded.unwrap_or_default());
        assert_eq!(decoded.ok(), Some(settings));
    }

    #[test]
    fn missing_optional_fields_use_safe_defaults() {
        let settings = serde_json::from_str::<Settings>(r#"{"version":1}"#);
        assert!(settings.is_ok());
        let settings = settings.unwrap_or_default();
        assert!((settings.stabilization - 0.5).abs() < f32::EPSILON);
        assert!((settings.pressure_sensitivity - 0.5).abs() < f32::EPSILON);
        assert!(settings.remember_drawing_style);
        assert!(settings.drawing_style.is_none());
        assert!(!settings.autosave_directory.is_empty());
        assert_eq!(settings.canvas_background, CanvasBackground::DotGrid);
    }
}
