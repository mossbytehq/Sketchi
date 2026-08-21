//! Cross-platform release checks for the desktop client.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::Path,
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(target_os = "linux")]
use std::os::unix::fs::PermissionsExt;

#[cfg(target_os = "linux")]
use std::process::Stdio;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use directories::ProjectDirs;
use reqwest::{
    StatusCode,
    header::{ACCEPT, USER_AGENT},
};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

const RELEASES_API_URL: &str =
    "https://api.github.com/repos/mossbytehq/Sketchi/releases?per_page=100";
const RELEASE_PAGE_PREFIX: &str = "https://github.com/mossbytehq/Sketchi/releases/";
const RELEASE_DOWNLOAD_PREFIX: &str = "https://github.com/mossbytehq/Sketchi/releases/download/";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const AUTOMATIC_CHECK_INTERVAL_SECS: u64 = 4 * 60 * 60;
const UPDATE_RESULT_FILENAME: &str = "update-result";

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
    /// Official update asset URL for the latest stable release.
    pub(crate) latest_stable_asset_url: Option<String>,
    /// Official update asset filename for the latest stable release.
    pub(crate) latest_stable_asset_name: Option<String>,
    /// Official update asset SHA-256 digest for the latest stable release.
    pub(crate) latest_stable_asset_digest: Option<String>,
    /// Official update asset URL for the latest edge release.
    pub(crate) latest_edge_asset_url: Option<String>,
    /// Official update asset filename for the latest edge release.
    pub(crate) latest_edge_asset_name: Option<String>,
    /// Official update asset SHA-256 digest for the latest edge release.
    pub(crate) latest_edge_asset_digest: Option<String>,
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
    /// Download URL for the selected target's platform update asset.
    pub(crate) target_asset_url: Option<String>,
    /// Filename for the selected target's platform update asset.
    pub(crate) target_asset_name: Option<String>,
    /// SHA-256 digest for the selected target's platform update asset.
    pub(crate) target_asset_digest: Option<String>,
    /// Whether this installation is ahead of the latest stable release.
    pub(crate) ahead_of_stable: bool,
    /// Whether the cache contains a successful lookup.
    pub(crate) has_result: bool,
}

/// Errors from a release lookup or automatic update installation.
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
    /// The release asset was not a supported official Sketchi update.
    #[error("the release does not contain a supported automatic update asset")]
    UnsupportedAsset,
    /// The update could not be downloaded or staged.
    #[error("could not stage the update: {0}")]
    Io(#[from] io::Error),
    /// The current installation directory cannot be modified by the updater.
    #[error(
        "automatic updates require a user-writable installation directory ({0}); use your package manager to update this installation"
    )]
    InstallLocation(String),
    /// The downloaded update did not match GitHub's advertised digest.
    #[error("the downloaded update failed its SHA-256 verification")]
    ChecksumMismatch,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    prerelease: bool,
    draft: bool,
    assets: Vec<GitHubAsset>,
}

#[derive(Clone, Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

#[derive(Clone, Debug)]
struct ReleaseAsset {
    name: String,
    url: String,
    digest: Option<String>,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LinuxAssetKind {
    Portable,
    Arch,
    Debian,
    Rpm,
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

    #[cfg(target_os = "linux")]
    let asset_kind = linux_asset_kind();
    let mut latest_stable: Option<(Version, String, Option<ReleaseAsset>)> = None;
    let mut latest_edge: Option<(Version, String, Option<ReleaseAsset>)> = None;
    for release in releases {
        if release.draft || !release.html_url.starts_with(RELEASE_PAGE_PREFIX) {
            continue;
        }
        let Some(version) = parse_tag(&release.tag_name) else {
            continue;
        };
        #[cfg(target_os = "linux")]
        let asset = auto_update_asset(&release, asset_kind).and_then(release_asset);
        #[cfg(not(target_os = "linux"))]
        let asset = auto_update_asset(&release).and_then(release_asset);
        let entry = (version.clone(), release.html_url, asset);
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
        latest_stable_url: latest_stable.as_ref().map(|item| item.1.clone()),
        latest_edge_url: latest_edge.as_ref().map(|item| item.1.clone()),
        latest_stable_asset_url: latest_stable
            .as_ref()
            .and_then(|item| item.2.as_ref().map(|asset| asset.url.clone())),
        latest_stable_asset_name: latest_stable
            .as_ref()
            .and_then(|item| item.2.as_ref().map(|asset| asset.name.clone())),
        latest_stable_asset_digest: latest_stable
            .as_ref()
            .and_then(|item| item.2.as_ref().and_then(|asset| asset.digest.clone())),
        latest_edge_asset_url: latest_edge
            .as_ref()
            .and_then(|item| item.2.as_ref().map(|asset| asset.url.clone())),
        latest_edge_asset_name: latest_edge
            .as_ref()
            .and_then(|item| item.2.as_ref().map(|asset| asset.name.clone())),
        latest_edge_asset_digest: latest_edge
            .as_ref()
            .and_then(|item| item.2.as_ref().and_then(|asset| asset.digest.clone())),
    })
}

/// Derives the current update state from the current cache and channel.
pub(crate) fn status(cache: &UpdateCache, channel: UpdateChannel) -> UpdateStatus {
    let current =
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 0, 0));
    let latest_stable = parse_optional_version(cache.latest_stable.as_deref());
    let latest_edge = parse_optional_version(cache.latest_edge.as_deref());
    let (candidate, target_url, target_asset_url, target_asset_name, target_asset_digest) =
        match channel {
            UpdateChannel::Stable => (
                latest_stable.clone(),
                cache.latest_stable_url.clone(),
                cache.latest_stable_asset_url.clone(),
                cache.latest_stable_asset_name.clone(),
                cache.latest_stable_asset_digest.clone(),
            ),
            UpdateChannel::Edge => (
                latest_edge.clone(),
                cache.latest_edge_url.clone(),
                cache.latest_edge_asset_url.clone(),
                cache.latest_edge_asset_name.clone(),
                cache.latest_edge_asset_digest.clone(),
            ),
        };
    let target = candidate.filter(|version| version > &current);
    let has_target = target.is_some();
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
        target_url: target.clone().and(target_url),
        target_asset_url: has_target.then_some(target_asset_url).flatten(),
        target_asset_name: has_target.then_some(target_asset_name).flatten(),
        target_asset_digest: has_target.then_some(target_asset_digest).flatten(),
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

/// Downloads and starts the platform-specific update handoff.
pub(crate) fn install_update(
    url: &str,
    name: &str,
    expected_digest: Option<&str>,
) -> Result<(), UpdateError> {
    if !url.starts_with(RELEASE_DOWNLOAD_PREFIX) || !is_supported_asset_name(name) {
        return Err(UpdateError::UnsupportedAsset);
    }

    #[cfg(target_os = "windows")]
    return install_windows_update(url, name, expected_digest);

    #[cfg(target_os = "linux")]
    return install_linux_update(url, name, expected_digest);

    #[allow(unreachable_code)]
    Err(UpdateError::UnsupportedAsset)
}

fn update_result_path() -> Result<std::path::PathBuf, UpdateError> {
    let directories = ProjectDirs::from("com", "Sketchi", "Sketchi").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "could not determine the Sketchi data directory",
        )
    })?;
    fs::create_dir_all(directories.data_dir())?;
    Ok(directories.data_dir().join(UPDATE_RESULT_FILENAME))
}

#[cfg(target_os = "linux")]
fn mark_update_pending() -> Result<std::path::PathBuf, UpdateError> {
    let path = update_result_path()?;
    fs::write(&path, "pending\n")?;
    Ok(path)
}

fn clear_update_result(path: &Path) {
    let _ = fs::remove_file(path);
}

/// Returns and clears the result written by the previous update handoff.
pub(crate) fn take_update_result() -> Option<String> {
    let path = update_result_path().ok()?;
    let result = fs::read_to_string(&path).ok()?;
    clear_update_result(&path);
    let result = result.trim();
    Some(match result {
        "success" => String::from("Update completed successfully."),
        "pending" => String::from("The previous update did not complete. Please try again."),
        failure if failure.starts_with("failure:") => {
            format!(
                "Update failed before restart: {}",
                failure.trim_start_matches("failure:")
            )
        }
        _ => String::from("The previous update finished with an unknown result."),
    })
}

#[cfg(target_os = "linux")]
fn auto_update_asset(release: &GitHubRelease, asset_kind: LinuxAssetKind) -> Option<&GitHubAsset> {
    let suffix = match asset_kind {
        LinuxAssetKind::Portable => "-linux-x86_64",
        LinuxAssetKind::Arch => ".pkg.tar.zst",
        LinuxAssetKind::Debian => ".deb",
        LinuxAssetKind::Rpm => ".rpm",
    };

    release.assets.iter().find(|asset| {
        asset.name.ends_with(suffix)
            && asset
                .browser_download_url
                .starts_with(RELEASE_DOWNLOAD_PREFIX)
    })
}

#[cfg(not(target_os = "linux"))]
fn auto_update_asset(release: &GitHubRelease) -> Option<&GitHubAsset> {
    #[cfg(target_os = "windows")]
    let suffix = if release.prerelease {
        "-windows-x86_64.zip"
    } else {
        "-windows-x86_64-setup.exe"
    };
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    let suffix = "";

    release.assets.iter().find(|asset| {
        asset.name.ends_with(suffix)
            && asset
                .browser_download_url
                .starts_with(RELEASE_DOWNLOAD_PREFIX)
    })
}

fn release_asset(asset: &GitHubAsset) -> Option<ReleaseAsset> {
    if !asset
        .browser_download_url
        .starts_with(RELEASE_DOWNLOAD_PREFIX)
        || !is_supported_asset_name(&asset.name)
    {
        return None;
    }
    Some(ReleaseAsset {
        name: asset.name.clone(),
        url: asset.browser_download_url.clone(),
        digest: asset.digest.as_deref().and_then(normalize_digest),
    })
}

fn normalize_digest(digest: &str) -> Option<String> {
    let digest = digest.strip_prefix("sha256:").unwrap_or(digest);
    (digest.len() == 64
        && digest
            .chars()
            .all(|character| character.is_ascii_hexdigit()))
    .then(|| digest.to_ascii_lowercase())
}

fn is_supported_asset_name(name: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        return name.ends_with("-windows-x86_64-setup.exe")
            || name.ends_with("-windows-x86_64.zip");
    }
    #[cfg(target_os = "linux")]
    {
        return name.ends_with("-linux-x86_64")
            || name.ends_with(".pkg.tar.zst")
            || has_extension(name, "deb")
            || has_extension(name, "rpm");
    }
    #[allow(unreachable_code)]
    false
}

fn has_extension(name: &str, extension: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|value| value.to_string_lossy().eq_ignore_ascii_case(extension))
}

fn download_asset(url: &str, destination: &std::path::Path) -> Result<(), UpdateError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_mins(10))
        .build()?;
    let mut response = client
        .get(url)
        .header(ACCEPT, "application/octet-stream")
        .header(USER_AGENT, concat!("Sketchi/", env!("CARGO_PKG_VERSION")))
        .send()?
        .error_for_status()?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)?;
    if let Err(error) = io::copy(&mut response, &mut file) {
        let _ = fs::remove_file(destination);
        return Err(UpdateError::Io(error));
    }
    file.flush()?;
    Ok(())
}

fn ensure_writable_directory(directory: &Path) -> Result<(), UpdateError> {
    let probe = directory.join(format!(".Sketchi-update-probe-{}", std::process::id()));
    match OpenOptions::new().create_new(true).write(true).open(&probe) {
        Ok(file) => {
            drop(file);
            fs::remove_file(&probe)?;
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => Err(
            UpdateError::InstallLocation(directory.display().to_string()),
        ),
        Err(error) => Err(UpdateError::Io(error)),
    }
}

fn verify_digest(path: &std::path::Path, expected_digest: Option<&str>) -> Result<(), UpdateError> {
    let Some(expected_digest) = expected_digest.and_then(normalize_digest) else {
        return Ok(());
    };
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected_digest {
        let _ = fs::remove_file(path);
        return Err(UpdateError::ChecksumMismatch);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn install_linux_update(
    url: &str,
    name: &str,
    expected_digest: Option<&str>,
) -> Result<(), UpdateError> {
    if name.ends_with(".pkg.tar.zst") || has_extension(name, "deb") || has_extension(name, "rpm") {
        return install_linux_package_update(url, name, expected_digest);
    }

    let executable = std::env::current_exe()?;
    let parent = executable.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "current executable has no parent")
    })?;
    ensure_writable_directory(parent)?;
    let staged = parent.join(format!(".Sketchi-update-{}-{name}", std::process::id()));
    download_asset(url, &staged)?;
    if let Err(error) = verify_digest(&staged, expected_digest) {
        let _ = fs::remove_file(&staged);
        return Err(error);
    }
    let mut permissions = fs::metadata(&executable)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&staged, permissions)?;

    let mut command = Command::new("sh");
    let result_path = mark_update_pending()?;
    command
        .arg("-c")
        .arg(
            "while kill -0 \"$3\" 2>/dev/null; do sleep 0.1; done; \
             if mv \"$1\" \"$2\"; then \
                 printf 'success\\n' > \"$4\"; \
                 exec \"$2\"; \
             else \
                 printf 'failure: could not replace the client\\n' > \"$4\"; \
                 exit 1; \
             fi",
        )
        .arg("sketchi-update")
        .arg(&staged)
        .arg(&executable)
        .arg(std::process::id().to_string())
        .arg(&result_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Err(error) = command.spawn() {
        clear_update_result(&result_path);
        return Err(error.into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn install_linux_package_update(
    url: &str,
    name: &str,
    expected_digest: Option<&str>,
) -> Result<(), UpdateError> {
    let package_command = if name.ends_with(".pkg.tar.zst") {
        if linux_asset_kind() != LinuxAssetKind::Arch {
            return Err(UpdateError::UnsupportedAsset);
        }
        String::from("pkexec pacman --upgrade --noconfirm \"$1\"")
    } else if has_extension(name, "deb") {
        if linux_asset_kind() != LinuxAssetKind::Debian {
            return Err(UpdateError::UnsupportedAsset);
        }
        String::from("pkexec dpkg --install \"$1\"")
    } else if has_extension(name, "rpm") {
        if linux_asset_kind() != LinuxAssetKind::Rpm {
            return Err(UpdateError::UnsupportedAsset);
        }
        String::from("pkexec rpm --upgrade --replacepkgs \"$1\"")
    } else {
        return Err(UpdateError::UnsupportedAsset);
    };
    let executable = std::env::current_exe()?;
    let staged = std::env::temp_dir().join(format!("Sketchi-update-{}-{name}", std::process::id()));
    download_asset(url, &staged)?;
    if let Err(error) = verify_digest(&staged, expected_digest) {
        let _ = fs::remove_file(&staged);
        return Err(error);
    }

    let result_path = mark_update_pending()?;
    let script = format!(
        "while kill -0 \"$3\" 2>/dev/null; do sleep 0.1; done; \\
         if {package_command}; then \\
             printf 'success\\n' > \"$4\"; \\
             exec \"$2\"; \\
         else \\
             printf 'failure: package manager could not install the update\\n' > \"$4\"; \\
             exit 1; \\
         fi"
    );
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(script)
        .arg("sketchi-update")
        .arg(&staged)
        .arg(&executable)
        .arg(std::process::id().to_string())
        .arg(&result_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Err(error) = command.spawn() {
        clear_update_result(&result_path);
        let _ = fs::remove_file(&staged);
        return Err(error.into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_asset_kind() -> LinuxAssetKind {
    let Ok(executable) = std::env::current_exe() else {
        return LinuxAssetKind::Portable;
    };
    let executable = executable.to_string_lossy();
    if command_succeeds("pacman", &["-Qo", executable.as_ref()]) {
        LinuxAssetKind::Arch
    } else if command_succeeds("dpkg-query", &["--search", executable.as_ref()]) {
        LinuxAssetKind::Debian
    } else if command_succeeds("rpm", &["-qf", executable.as_ref()]) {
        LinuxAssetKind::Rpm
    } else {
        LinuxAssetKind::Portable
    }
}

#[cfg(target_os = "linux")]
fn command_succeeds(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(target_os = "windows")]
fn install_windows_update(
    url: &str,
    name: &str,
    expected_digest: Option<&str>,
) -> Result<(), UpdateError> {
    let executable = std::env::current_exe()?;
    let destination =
        std::env::temp_dir().join(format!("Sketchi-update-{}-{name}", std::process::id()));
    download_asset(url, &destination)?;
    verify_digest(&destination, expected_digest)?;
    if name.ends_with("-setup.exe") {
        return install_windows_setup(&executable, &destination);
    }

    install_windows_archive(&executable, &destination)
}

#[cfg(target_os = "windows")]
fn install_windows_setup(executable: &Path, destination: &Path) -> Result<(), UpdateError> {
    let result_path = update_result_path()?;
    let script_path =
        std::env::temp_dir().join(format!("Sketchi-update-{}-setup.ps1", std::process::id()));
    fs::write(
        &script_path,
        r#"param([string]$Installer, [string]$Exe, [int]$ProcessId, [string]$Result)
$ErrorActionPreference = "Stop"
try {
    Wait-Process -Id $ProcessId -ErrorAction SilentlyContinue
    $install = Start-Process -FilePath $Installer -Wait -PassThru
    if ($install.ExitCode -ne 0) {
        throw "installer exited with code $($install.ExitCode)"
    }
    [System.IO.File]::WriteAllText($Result, "success")
    Start-Process -FilePath $Exe
} catch {
    [System.IO.File]::WriteAllText($Result, ("failure: " + $_.Exception.Message))
} finally {
    Remove-Item -LiteralPath $Installer -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue
}"#,
    )?;
    fs::write(&result_path, "pending\n")?;
    let mut command = windows_powershell_command(&script_path);
    command
        .arg(destination)
        .arg(executable)
        .arg(std::process::id().to_string())
        .arg(&result_path);
    spawn_windows_update_command(command, &result_path)
}

#[cfg(target_os = "windows")]
fn install_windows_archive(executable: &Path, destination: &Path) -> Result<(), UpdateError> {
    let parent = executable.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "current executable has no parent")
    })?;
    ensure_writable_directory(parent)?;

    let script_path =
        std::env::temp_dir().join(format!("Sketchi-update-{}-install.ps1", std::process::id()));
    let result_path = update_result_path()?;
    fs::write(
        &script_path,
        r#"param([string]$Zip, [string]$Exe, [int]$ProcessId, [string]$Result)
$ErrorActionPreference = "Stop"
$parent = Split-Path -Parent $Exe
$stage = Join-Path $env:TEMP ("Sketchi-update-" + $ProcessId)
try {
    Wait-Process -Id $ProcessId -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
    Expand-Archive -LiteralPath $Zip -DestinationPath $stage -Force
    Copy-Item -Path (Join-Path $stage '*') -Destination $parent -Recurse -Force
    [System.IO.File]::WriteAllText($Result, "success")
    Start-Process -FilePath $Exe
} catch {
    [System.IO.File]::WriteAllText($Result, ("failure: " + $_.Exception.Message))
} finally {
    Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $Zip -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue
}"#,
    )?;
    fs::write(&result_path, "pending\n")?;
    let mut command = windows_powershell_command(&script_path);
    command
        .arg(destination)
        .arg(executable)
        .arg(std::process::id().to_string())
        .arg(&result_path);
    spawn_windows_update_command(command, &result_path)
}

#[cfg(target_os = "windows")]
fn windows_powershell_command(script_path: &Path) -> Command {
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script_path)
        .creation_flags(0x0800_0000);
    command
}

#[cfg(target_os = "windows")]
fn spawn_windows_update_command(
    mut command: Command,
    result_path: &Path,
) -> Result<(), UpdateError> {
    if let Err(error) = command.spawn() {
        clear_update_result(result_path);
        return Err(error.into());
    }
    Ok(())
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
    use semver::Version;

    use super::{UpdateCache, UpdateChannel, format_last_checked, is_check_due, now_epoch, status};

    #[cfg(target_os = "linux")]
    use super::{GitHubAsset, GitHubRelease, LinuxAssetKind, auto_update_asset};

    #[test]
    fn stable_channel_ignores_edge_only_updates() {
        let cache = UpdateCache {
            checked_at_epoch: Some(1),
            latest_stable: Some("0.1.2".to_owned()),
            latest_edge: Some("0.2.0-rc.1".to_owned()),
            latest_stable_url: Some(
                "https://github.com/mossbytehq/Sketchi/releases/tag/v0.1.2".to_owned(),
            ),
            latest_edge_url: Some(
                "https://github.com/mossbytehq/Sketchi/releases/tag/v0.2.0-rc.1".to_owned(),
            ),
            ..UpdateCache::default()
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
        let current = Version::parse(env!("CARGO_PKG_VERSION")).ok();
        assert!(current.is_some(), "package version must be valid SemVer");
        let Some(current) = current else {
            return;
        };
        let newer_edge = format!(
            "{}.{}.{}-rc.1",
            current.major,
            current.minor,
            current.patch + 1
        );
        let cache = UpdateCache {
            checked_at_epoch: Some(1),
            latest_stable: Some("0.1.2".to_owned()),
            latest_edge: Some(newer_edge.clone()),
            latest_stable_url: None,
            latest_edge_url: Some(format!(
                "https://github.com/mossbytehq/Sketchi/releases/tag/v{newer_edge}"
            )),
            ..UpdateCache::default()
        };
        let edge = status(&cache, UpdateChannel::Edge);
        assert_eq!(
            edge.target.as_ref().map(ToString::to_string),
            Some(newer_edge)
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

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_asset_selection_matches_the_installed_package_format() {
        let release = GitHubRelease {
            tag_name: String::from("v0.3.1"),
            html_url: String::from("https://github.com/mossbytehq/Sketchi/releases/tag/v0.3.1"),
            prerelease: false,
            draft: false,
            assets: vec![
                GitHubAsset {
                    name: String::from("Sketchi-0.3.1-linux-x86_64"),
                    browser_download_url: String::from(
                        "https://github.com/mossbytehq/Sketchi/releases/download/v0.3.1/Sketchi-0.3.1-linux-x86_64",
                    ),
                    digest: None,
                },
                GitHubAsset {
                    name: String::from("sketchi-0.3.1-1-x86_64.pkg.tar.zst"),
                    browser_download_url: String::from(
                        "https://github.com/mossbytehq/Sketchi/releases/download/v0.3.1/sketchi-0.3.1-1-x86_64.pkg.tar.zst",
                    ),
                    digest: None,
                },
                GitHubAsset {
                    name: String::from("sketchi_0.3.1_amd64.deb"),
                    browser_download_url: String::from(
                        "https://github.com/mossbytehq/Sketchi/releases/download/v0.3.1/sketchi_0.3.1_amd64.deb",
                    ),
                    digest: None,
                },
                GitHubAsset {
                    name: String::from("sketchi-0.3.1-1.x86_64.rpm"),
                    browser_download_url: String::from(
                        "https://github.com/mossbytehq/Sketchi/releases/download/v0.3.1/sketchi-0.3.1-1.x86_64.rpm",
                    ),
                    digest: None,
                },
            ],
        };

        assert!(
            auto_update_asset(&release, LinuxAssetKind::Portable)
                .is_some_and(|asset| asset.name.ends_with("-linux-x86_64"))
        );
        assert!(
            auto_update_asset(&release, LinuxAssetKind::Arch)
                .is_some_and(|asset| asset.name.ends_with(".pkg.tar.zst"))
        );
        assert!(
            auto_update_asset(&release, LinuxAssetKind::Debian)
                .is_some_and(|asset| super::has_extension(&asset.name, "deb"))
        );
        assert!(
            auto_update_asset(&release, LinuxAssetKind::Rpm)
                .is_some_and(|asset| super::has_extension(&asset.name, "rpm"))
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_package_assets_are_supported() {
        assert!(super::is_supported_asset_name("sketchi_0.3.1_amd64.deb"));
        assert!(super::is_supported_asset_name(
            "sketchi-0.3.1-1-x86_64.pkg.tar.zst"
        ));
        assert!(super::is_supported_asset_name("sketchi-0.3.1-1.x86_64.rpm"));
        assert!(!super::is_supported_asset_name(
            "sketchi-0.3.1_amd64.tar.gz"
        ));
    }
}
