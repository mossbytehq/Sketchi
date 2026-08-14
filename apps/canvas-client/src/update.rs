//! Cross-platform release checks for the desktop client.

use std::{
    io,
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::{
    StatusCode,
    header::{ACCEPT, USER_AGENT},
};
use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const RELEASES_API_URL: &str =
    "https://api.github.com/repos/SantanuDatta/Sketchi/releases?per_page=30";
const RELEASE_PAGE_PREFIX: &str = "https://github.com/SantanuDatta/Sketchi/releases/";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const AUTOMATIC_CHECK_INTERVAL_SECS: u64 = 4 * 60 * 60;

/// The release stream used when looking for updates.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UpdateChannel {
    /// Final releases only.
    #[default]
    Stable,
    /// Final releases and pre-releases.
    Edge,
}

impl UpdateChannel {
    /// Returns the user-facing channel name.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Stable => "Stable",
            Self::Edge => "Edge (pre-release)",
        }
    }
}

/// The last successful release lookup persisted with user settings.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub(crate) struct UpdateCache {
    /// Unix timestamp of the successful lookup.
    pub(crate) checked_at_epoch: Option<u64>,
    /// Latest stable version as a `SemVer` string.
    pub(crate) latest_stable: Option<String>,
    /// Latest edge version as a `SemVer` string.
    pub(crate) latest_edge: Option<String>,
    /// Stable release page URL.
    pub(crate) latest_stable_url: Option<String>,
    /// Edge release page URL.
    pub(crate) latest_edge_url: Option<String>,
}

/// Release information presented by the settings view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UpdateStatus {
    /// Current application version.
    pub(crate) current: Version,
    /// Latest stable version, when known.
    pub(crate) latest_stable: Option<Version>,
    /// Latest edge version, when known.
    pub(crate) latest_edge: Option<Version>,
    /// Selected release channel.
    pub(crate) channel: UpdateChannel,
    /// Version selected for the current channel, if newer.
    pub(crate) target: Option<Version>,
    /// Release page for the selected target.
    pub(crate) target_url: Option<String>,
    /// Whether this installation is ahead of the latest stable release.
    pub(crate) ahead_of_stable: bool,
    /// Whether the cache contains a successful lookup.
    pub(crate) has_result: bool,
}

/// Errors from a release lookup or browser handoff.
#[derive(Debug, Error)]
pub(crate) enum UpdateError {
    /// The HTTP client could not be created or the request failed.
    #[error("could not check for updates: {0}")]
    Request(#[from] reqwest::Error),
    /// GitHub has temporarily throttled unauthenticated release requests.
    #[error("GitHub is temporarily rate-limiting update checks; try again later")]
    RateLimited,
    /// The release response was not valid JSON.
    #[error("could not decode release information: {0}")]
    Decode(#[from] serde_json::Error),
    /// GitHub returned no usable release entries.
    #[error("no usable releases were found")]
    NoReleases,
    /// The release URL was not one owned by Sketchi.
    #[error("refusing to open an untrusted release URL")]
    InvalidReleaseUrl,
    /// The operating system could not launch its browser.
    #[error("could not open the release page: {0}")]
    Browser(#[source] io::Error),
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    prerelease: bool,
    draft: bool,
}

/// Checks GitHub Releases and returns a cache suitable for persistence.
pub(crate) fn check() -> Result<UpdateCache, UpdateError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()?;
    let response = client
        .get(RELEASES_API_URL)
        .header(ACCEPT, "application/vnd.github+json")
        .header(USER_AGENT, concat!("Sketchi/", env!("CARGO_PKG_VERSION")))
        .send()?;
    if matches!(
        response.status(),
        StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS
    ) {
        return Err(UpdateError::RateLimited);
    }
    let releases = response.error_for_status()?.json::<Vec<GitHubRelease>>()?;

    let mut latest_stable: Option<(Version, String)> = None;
    let mut latest_edge: Option<(Version, String)> = None;
    for release in releases {
        if release.draft || !release.html_url.starts_with(RELEASE_PAGE_PREFIX) {
            continue;
        }
        let Some(version) = parse_tag(&release.tag_name) else {
            continue;
        };
        let entry = (version.clone(), release.html_url);
        if latest_edge
            .as_ref()
            .is_none_or(|current| entry.0 > current.0)
        {
            latest_edge = Some(entry.clone());
        }
        if !release.prerelease
            && entry.0.pre.is_empty()
            && latest_stable
                .as_ref()
                .is_none_or(|current| entry.0 > current.0)
        {
            latest_stable = Some(entry);
        }
    }

    if latest_stable.is_none() && latest_edge.is_none() {
        return Err(UpdateError::NoReleases);
    }

    Ok(UpdateCache {
        checked_at_epoch: Some(now_epoch()),
        latest_stable: latest_stable.as_ref().map(|item| item.0.to_string()),
        latest_edge: latest_edge.as_ref().map(|item| item.0.to_string()),
        latest_stable_url: latest_stable.map(|item| item.1),
        latest_edge_url: latest_edge.map(|item| item.1),
    })
}

/// Derives the current update state from the current cache and channel.
pub(crate) fn status(cache: &UpdateCache, channel: UpdateChannel) -> UpdateStatus {
    let current =
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 0, 0));
    let latest_stable = parse_optional_version(cache.latest_stable.as_deref());
    let latest_edge = parse_optional_version(cache.latest_edge.as_deref());
    let (candidate, target_url) = match channel {
        UpdateChannel::Stable => (latest_stable.clone(), cache.latest_stable_url.clone()),
        UpdateChannel::Edge => (latest_edge.clone(), cache.latest_edge_url.clone()),
    };
    let target = candidate.filter(|version| version > &current);
    let ahead_of_stable = !current.pre.is_empty()
        && latest_stable
            .as_ref()
            .is_some_and(|version| current > *version);

    UpdateStatus {
        current,
        latest_stable,
        latest_edge,
        channel,
        target: target.clone(),
        target_url: target.and(target_url),
        ahead_of_stable,
        has_result: cache.checked_at_epoch.is_some(),
    }
}

/// Returns whether the automatic check should run for the cached timestamp.
pub(crate) fn is_check_due(timestamp: Option<u64>) -> bool {
    timestamp.is_none_or(|checked_at| {
        now_epoch().saturating_sub(checked_at) >= AUTOMATIC_CHECK_INTERVAL_SECS
    })
}

/// Opens a known Sketchi release page using the platform's default browser.
pub(crate) fn open_release_url(url: &str) -> Result<(), UpdateError> {
    if !url.starts_with(RELEASE_PAGE_PREFIX) {
        return Err(UpdateError::InvalidReleaseUrl);
    }

    let mut command = if cfg!(target_os = "windows") {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(url);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }

    command.spawn().map(|_| ()).map_err(UpdateError::Browser)
}

/// Formats the cached lookup time for the settings view.
pub(crate) fn format_last_checked(timestamp: Option<u64>) -> String {
    let Some(timestamp) = timestamp else {
        return "Last checked: never".to_owned();
    };
    let age = now_epoch().saturating_sub(timestamp);
    let label = match age {
        0..=59 => "just now".to_owned(),
        60..=3_599 => format!(
            "{} minute{} ago",
            age / 60,
            if age / 60 == 1 { "" } else { "s" }
        ),
        3_600..=86_399 => format!(
            "{} hour{} ago",
            age / 3_600,
            if age / 3_600 == 1 { "" } else { "s" }
        ),
        _ => format!(
            "{} day{} ago",
            age / 86_400,
            if age / 86_400 == 1 { "" } else { "s" }
        ),
    };
    format!("Last checked {label}")
}

fn parse_tag(tag: &str) -> Option<Version> {
    Version::parse(tag.trim_start_matches(['v', 'V'])).ok()
}

fn parse_optional_version(value: Option<&str>) -> Option<Version> {
    value.and_then(parse_tag)
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::{UpdateCache, UpdateChannel, format_last_checked, is_check_due, now_epoch, status};

    #[test]
    fn stable_channel_ignores_edge_only_updates() {
        let cache = UpdateCache {
            checked_at_epoch: Some(1),
            latest_stable: Some("0.1.2".to_owned()),
            latest_edge: Some("0.2.0-rc.1".to_owned()),
            latest_stable_url: Some(
                "https://github.com/SantanuDatta/Sketchi/releases/tag/v0.1.2".to_owned(),
            ),
            latest_edge_url: Some(
                "https://github.com/SantanuDatta/Sketchi/releases/tag/v0.2.0-rc.1".to_owned(),
            ),
        };
        let stable = status(&cache, UpdateChannel::Stable);
        assert!(stable.target.is_none());
        assert_eq!(
            stable.latest_edge.as_ref().map(ToString::to_string),
            Some("0.2.0-rc.1".to_owned())
        );
    }

    #[test]
    fn edge_channel_selects_a_newer_prerelease() {
        let cache = UpdateCache {
            checked_at_epoch: Some(1),
            latest_stable: Some("0.1.2".to_owned()),
            latest_edge: Some("0.2.0-rc.1".to_owned()),
            latest_stable_url: None,
            latest_edge_url: Some(
                "https://github.com/SantanuDatta/Sketchi/releases/tag/v0.2.0-rc.1".to_owned(),
            ),
        };
        let edge = status(&cache, UpdateChannel::Edge);
        assert_eq!(
            edge.target.as_ref().map(ToString::to_string),
            Some("0.2.0-rc.1".to_owned())
        );
        assert!(edge.target_url.is_some());
    }

    #[test]
    fn missing_timestamp_is_explicit() {
        assert_eq!(format_last_checked(None), "Last checked: never");
    }

    #[test]
    fn automatic_checks_use_a_four_hour_cooldown() {
        assert!(is_check_due(None));
        assert!(!is_check_due(Some(now_epoch())));
        assert!(is_check_due(Some(now_epoch().saturating_sub(4 * 60 * 60))));
    }
}
