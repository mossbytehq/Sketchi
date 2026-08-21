//! Repository automation entry point.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    missing_docs
)]

use std::{
    collections::BTreeSet,
    env,
    fmt::Write as _,
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Parser)]
#[command(name = "xtask", about = "Sketchi development and release tasks")]
struct Cli {
    #[command(subcommand)]
    command: Option<CommandKind>,
}

#[derive(Debug, Subcommand)]
enum CommandKind {
    /// Print the commands planned for the release workflow.
    Check,
    /// Check workspace/tag versions and optional built binaries.
    VersionCheck(VersionCheckArgs),
    /// Set the workspace version and optionally update an existing GitHub release state.
    SetVersion(SetVersionArgs),
    /// Stage a client package containing the server sidecar.
    Package(StageArgs),
    /// Stage a server-only package.
    PackageServer(StageArgs),
    /// Check the exact contents of a staged package directory.
    ArtifactCheck(ArtifactCheckArgs),
    /// Write a deterministic SHA-256 manifest for final artifacts.
    Checksums(ChecksumsArgs),
    /// Verify a staged client or server package can pass its install smoke checks.
    InstallSmoke(InstallSmokeArgs),
}

#[derive(Debug, Args)]
struct VersionCheckArgs {
    /// Release tag to compare with the workspace version, such as v0.1.0.
    #[arg(long)]
    tag: Option<String>,
    /// Optional client binary to invoke with --version.
    #[arg(long)]
    client: Option<PathBuf>,
    /// Optional server binary to invoke with --version.
    #[arg(long)]
    server: Option<PathBuf>,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Args)]
struct SetVersionArgs {
    /// Modify all packages in the workspace.
    #[arg(long)]
    workspace: bool,
    /// Stable Cargo version to write to the workspace manifest.
    version: String,
    /// Mark the existing GitHub release as a draft.
    #[arg(long = "d", conflicts_with_all = ["release_candidate", "latest"])]
    draft: bool,
    /// Mark the existing GitHub release as a pre-release.
    #[arg(long = "rc", conflicts_with_all = ["draft", "latest"])]
    release_candidate: bool,
    /// Mark the existing GitHub release as the latest stable release.
    #[arg(long = "r", conflicts_with_all = ["draft", "release_candidate"])]
    latest: bool,
}

#[derive(Debug, Args)]
struct StageArgs {
    /// Rust target triple used in the artifact name and binary lookup.
    #[arg(long)]
    target: String,
    /// Cargo profile containing the already-built binaries.
    #[arg(long, default_value = "release")]
    profile: String,
    /// Cargo target directory containing the built binaries.
    #[arg(long, default_value = "target")]
    target_dir: PathBuf,
    /// Directory receiving deterministic staging directories.
    #[arg(long, default_value = "dist/staging")]
    out_dir: PathBuf,
    /// Requested final format; native packaging is performed by the matching
    /// runner tool after this deterministic staging step.
    #[arg(long, default_value = "portable")]
    format: String,
}

#[derive(Debug, Args)]
struct ArtifactCheckArgs {
    /// Staging directory to inspect.
    #[arg(long)]
    path: PathBuf,
    /// Rust target triple encoded by the staging command.
    #[arg(long)]
    target: String,
    /// Staged package kind.
    #[arg(long, value_enum, default_value_t = PackageKind::Client)]
    kind: PackageKind,
}

#[derive(Debug, Args)]
struct ChecksumsArgs {
    /// Directory containing only final artifacts to hash.
    #[arg(long)]
    input_dir: PathBuf,
    /// Output checksum manifest. Defaults to input-dir/SHA256SUMS.
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct InstallSmokeArgs {
    /// Staging directory to smoke-test.
    #[arg(long)]
    path: PathBuf,
    /// Rust target triple encoded by the staging command.
    #[arg(long)]
    target: String,
    /// Staged package kind.
    #[arg(long, value_enum, default_value_t = PackageKind::Client)]
    kind: PackageKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum PackageKind {
    Client,
    Server,
}

impl PackageKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Server => "server",
        }
    }
}

#[derive(Debug, Deserialize)]
struct ReadinessSmoke {
    endpoint: String,
    certificate_sha256: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = repository_root()?;
    match cli.command {
        Some(CommandKind::Check) => {
            println!("Sketchi workspace checks are provided by Cargo and CI.");
        }
        Some(CommandKind::VersionCheck(args)) => run_version_check(&root, args)?,
        Some(CommandKind::SetVersion(args)) => run_set_version(&root, &args)?,
        Some(CommandKind::Package(args)) => {
            validate_format(&args.format, &args.target)?;
            let staged = run_stage(&root, &args, PackageKind::Client)?;
            println!(
                "staged client package for {}: {}",
                args.format,
                staged.display()
            );
        }
        Some(CommandKind::PackageServer(args)) => {
            if args.format != "portable" {
                bail!("server staging currently supports only the portable format");
            }
            let staged = run_stage(&root, &args, PackageKind::Server)?;
            println!("staged server package: {}", staged.display());
        }
        Some(CommandKind::ArtifactCheck(args)) => {
            let path = path_from_root(&root, &args.path);
            let version = read_workspace_version(&root)?;
            check_artifact(&path, args.kind, &version, &args.target)?;
            println!("artifact check passed: {}", path.display());
        }
        Some(CommandKind::Checksums(args)) => {
            let input_dir = path_from_root(&root, &args.input_dir);
            let output = args.output.map_or_else(
                || input_dir.join("SHA256SUMS"),
                |path| path_from_root(&root, &path),
            );
            write_checksums(&input_dir, &output)?;
            println!("wrote checksums: {}", output.display());
        }
        Some(CommandKind::InstallSmoke(args)) => run_install_smoke(&root, &args)?,
        None => println!("Run cargo xtask --help for the repository task entry point."),
    }
    Ok(())
}

fn repository_root() -> Result<PathBuf> {
    let current = env::current_dir().context("determine current directory")?;
    for candidate in current.ancestors() {
        if candidate.join("Cargo.toml").is_file() && candidate.join("rust-toolchain.toml").is_file()
        {
            return Ok(candidate.to_path_buf());
        }
    }
    bail!(
        "could not find Sketchi repository root from {}",
        current.display()
    );
}

fn path_from_root(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn validate_format(format: &str, target: &str) -> Result<()> {
    match format {
        "portable" => Ok(()),
        "appimage" | "arch" | "deb" | "rpm" if target.contains("linux") => Ok(()),
        "msi" if target.contains("windows") => Ok(()),
        "appimage" | "arch" | "deb" | "rpm" => bail!("{format} requires a Linux target"),
        "msi" => bail!("msi requires a Windows target"),
        _ => bail!("unknown package format {format:?}"),
    }
}

fn run_version_check(root: &Path, args: VersionCheckArgs) -> Result<()> {
    let version = read_workspace_version(root)?;
    if let Some(tag) = args.tag.as_deref() {
        validate_tag(tag, &version)?;
    }
    if let Some(client) = args.client {
        check_binary_version(&client, &version)
            .with_context(|| format!("check client version at {}", client.display()))?;
    }
    if let Some(server) = args.server {
        check_binary_version(&server, &version)
            .with_context(|| format!("check server version at {}", server.display()))?;
    }
    println!("Sketchi version {version}");
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GitHubReleaseState {
    Draft,
    ReleaseCandidate,
    Latest,
}

impl GitHubReleaseState {
    const fn gh_args(self) -> [&'static str; 3] {
        match self {
            Self::Draft => ["--draft=true", "--prerelease=false", "--latest=false"],
            Self::ReleaseCandidate => ["--draft=false", "--prerelease=true", "--latest=false"],
            Self::Latest => ["--draft=false", "--prerelease=false", "--latest=true"],
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::ReleaseCandidate => "pre-release",
            Self::Latest => "latest stable",
        }
    }
}

fn run_set_version(root: &Path, args: &SetVersionArgs) -> Result<()> {
    if !args.workspace {
        bail!("set-version currently requires --workspace");
    }
    let parsed = semver::Version::parse(&args.version)
        .with_context(|| format!("invalid semantic version {:?}", args.version))?;
    if !parsed.pre.is_empty() {
        bail!("set-version expects a stable Cargo version; use --rc to mark the GitHub release");
    }

    let release_state = match (args.draft, args.release_candidate, args.latest) {
        (true, false, false) => Some(GitHubReleaseState::Draft),
        (false, true, false) => Some(GitHubReleaseState::ReleaseCandidate),
        (false, false, true) => Some(GitHubReleaseState::Latest),
        (false, false, false) => None,
        _ => unreachable!("clap enforces release-state flag conflicts"),
    };
    if let Some(state) = release_state {
        update_github_release_state(&args.version, state)?;
    }
    write_workspace_version(root, &args.version)?;
    println!("workspace version set to {}", args.version);
    if let Some(state) = release_state {
        println!("GitHub release v{} set to {}", args.version, state.label());
    }
    Ok(())
}

fn update_github_release_state(version: &str, state: GitHubReleaseState) -> Result<()> {
    let tag = format!("v{version}");
    let output = Command::new("gh")
        .args(["release", "edit", &tag])
        .args(state.gh_args())
        .output()
        .with_context(|| {
            format!(
                "run gh to set GitHub release {tag} to {}; install and authenticate GitHub CLI",
                state.label()
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "could not set GitHub release {tag} to {}: {}",
            state.label(),
            stderr.trim()
        );
    }
    Ok(())
}

fn write_workspace_version(root: &Path, version: &str) -> Result<()> {
    let path = root.join("Cargo.toml");
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("read workspace manifest at {}", path.display()))?;
    let had_final_newline = contents.ends_with('\n');
    let mut in_workspace_package = false;
    let mut replaced = false;
    let mut lines = Vec::new();
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_workspace_package = trimmed == "[workspace.package]";
        }
        if in_workspace_package && trimmed.starts_with("version") {
            let indentation = &line[..line.len() - line.trim_start().len()];
            lines.push(format!("{indentation}version = \"{version}\""));
            replaced = true;
        } else {
            lines.push(line.to_owned());
        }
    }
    if !replaced {
        bail!(
            "[workspace.package] version is missing from {}",
            path.display()
        );
    }
    let mut updated = lines.join("\n");
    if had_final_newline {
        updated.push('\n');
    }
    fs::write(&path, updated).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn run_stage(root: &Path, args: &StageArgs, kind: PackageKind) -> Result<PathBuf> {
    let version = read_workspace_version(root)?;
    let target_dir = path_from_root(root, &args.target_dir);
    let binary_dir = target_dir.join(&args.target).join(&args.profile);
    let out_dir = path_from_root(root, &args.out_dir);
    match kind {
        PackageKind::Client => stage_client(root, &binary_dir, &out_dir, &version, &args.target),
        PackageKind::Server => stage_server(root, &binary_dir, &out_dir, &version, &args.target),
    }
}

fn run_install_smoke(root: &Path, args: &InstallSmokeArgs) -> Result<()> {
    let version = read_workspace_version(root)?;
    let path = path_from_root(root, &args.path);
    check_artifact(&path, args.kind, &version, &args.target)?;

    let server_name = binary_name("Sketchi-server", &args.target);
    let server = path.join(server_name);
    check_binary_version(&server, &version)?;
    if args.kind == PackageKind::Client {
        let client = path.join(binary_name("Sketchi", &args.target));
        check_binary_version(&client, &version)?;
    }
    let output = Command::new(&server)
        .args(["--check-config", "--insecure-loopback"])
        .output()
        .with_context(|| format!("run server smoke check at {}", server.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "server smoke check failed with {}: {}",
            output.status,
            stderr.trim()
        );
    }
    smoke_test_server(&server)?;
    println!("install smoke passed: {} {version}", path.display());
    Ok(())
}

fn smoke_test_server(server: &Path) -> Result<()> {
    let database = env::temp_dir().join(format!(
        "sketchi-install-smoke-{}.sqlite3",
        std::process::id()
    ));
    let mut child = Command::new(server)
        .args(["--ready", "--bind", "127.0.0.1:0", "--database"])
        .arg(&database)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("start readiness smoke server at {}", server.display()))?;

    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        let _ = fs::remove_file(&database);
        bail!("readiness smoke server did not expose stdout");
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout)
            .read_line(&mut line)
            .map(|bytes| (bytes, line));
        let _ = sender.send(result);
    });
    let readiness = receiver
        .recv_timeout(Duration::from_secs(10))
        .context("timed out waiting for server readiness")
        .and_then(|result| result.context("could not read server readiness"));
    let _ = child.kill();
    let _ = child.wait();
    let _ = fs::remove_file(&database);

    let (bytes, line) = readiness?;
    if bytes == 0 {
        bail!("server exited without a readiness line");
    }
    let readiness: ReadinessSmoke = serde_json::from_str(&line)
        .with_context(|| format!("invalid server readiness JSON: {}", line.trim()))?;
    if !readiness.endpoint.starts_with("wss://") {
        bail!(
            "server readiness endpoint is not secure WebSocket: {}",
            readiness.endpoint
        );
    }
    if readiness.certificate_sha256.len() != 64
        || !readiness
            .certificate_sha256
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        bail!("server readiness certificate pin is invalid");
    }
    Ok(())
}

fn read_workspace_version(root: &Path) -> Result<String> {
    let manifest = fs::read_to_string(root.join("Cargo.toml"))
        .with_context(|| format!("read workspace manifest in {}", root.display()))?;
    let mut in_workspace_package = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_workspace_package = trimmed == "[workspace.package]";
            continue;
        }
        if in_workspace_package {
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if key.trim() == "version" {
                let version = value.trim().trim_matches('"').trim_matches('\'');
                if !version.is_empty() && !version.contains('{') {
                    return Ok(version.to_owned());
                }
            }
        }
    }
    bail!(
        "workspace package version is missing from {}/Cargo.toml",
        root.display()
    );
}

fn validate_tag(tag: &str, version: &str) -> Result<()> {
    let normalized = match tag.strip_prefix('v') {
        Some(value) => value,
        None => tag,
    };
    if normalized != version {
        bail!("release tag {tag:?} does not match workspace version {version}");
    }
    Ok(())
}

fn parse_reported_version(output: &str) -> Result<String> {
    for token in output.split_whitespace() {
        let candidate = token.trim_matches(|character: char| {
            !character.is_ascii_alphanumeric() && character != '.' && character != '-'
        });
        let mut components = candidate.split('.');
        let Some(first) = components.next() else {
            continue;
        };
        if first.chars().all(|character| character.is_ascii_digit())
            && components.next().is_some()
            && candidate
                .chars()
                .any(|character| character.is_ascii_digit())
        {
            return Ok(candidate.to_owned());
        }
    }
    bail!("could not find a version in command output: {output:?}");
}

fn check_binary_version(path: &Path, expected: &str) -> Result<()> {
    if !path.is_file() {
        bail!("binary does not exist: {}", path.display());
    }
    let output = Command::new(path)
        .arg("--version")
        .output()
        .with_context(|| format!("invoke {} --version", path.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{} --version failed: {}", path.display(), stderr.trim());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let reported = parse_reported_version(&format!("{stdout}\n{stderr}"))?;
    if reported != expected {
        bail!(
            "{} reports version {reported}, expected {expected}",
            path.display()
        );
    }
    Ok(())
}

fn package_directory_name(kind: PackageKind, version: &str, target: &str) -> String {
    match kind {
        PackageKind::Client => format!("Sketchi-{version}-{target}"),
        PackageKind::Server => format!("Sketchi-server-{version}-{target}"),
    }
}

fn binary_name(name: &str, target: &str) -> String {
    if target.contains("windows") {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

fn stage_client(
    root: &Path,
    binary_dir: &Path,
    out_dir: &Path,
    version: &str,
    target: &str,
) -> Result<PathBuf> {
    stage_package(
        root,
        binary_dir,
        out_dir,
        version,
        target,
        PackageKind::Client,
    )
}

fn stage_server(
    root: &Path,
    binary_dir: &Path,
    out_dir: &Path,
    version: &str,
    target: &str,
) -> Result<PathBuf> {
    stage_package(
        root,
        binary_dir,
        out_dir,
        version,
        target,
        PackageKind::Server,
    )
}

fn stage_package(
    root: &Path,
    binary_dir: &Path,
    out_dir: &Path,
    version: &str,
    target: &str,
    kind: PackageKind,
) -> Result<PathBuf> {
    let platform = target_platform(target)?;
    let stage = out_dir.join(package_directory_name(kind, version, target));
    if stage.is_dir() {
        fs::remove_dir_all(&stage)
            .with_context(|| format!("clear staging directory {}", stage.display()))?;
    } else if stage.exists() {
        bail!("staging path is not a directory: {}", stage.display());
    }
    fs::create_dir_all(&stage)
        .with_context(|| format!("create staging directory {}", stage.display()))?;

    let server_source = binary_dir.join(binary_name("sketchi-server", target));
    copy_required(
        &server_source,
        &stage.join(binary_name("Sketchi-server", target)),
    )?;
    if kind == PackageKind::Client {
        let client_source = binary_dir.join(binary_name("sketchi", target));
        copy_required(&client_source, &stage.join(binary_name("Sketchi", target)))?;
    }
    copy_required(&root.join("LICENSE.md"), &stage.join("LICENSE.md"))?;
    fs::write(stage.join("VERSION"), format!("{version}\n"))
        .with_context(|| format!("write package version in {}", stage.display()))?;
    if kind == PackageKind::Client && platform == TargetPlatform::Linux {
        copy_required(
            &root.join("packaging/linux/desktop/sketchi.desktop"),
            &stage.join("share/applications/sketchi.desktop"),
        )?;
        copy_required(
            &root.join("packaging/linux/icons/hicolor/512x512/apps/sketchi.png"),
            &stage.join("share/icons/hicolor/512x512/apps/sketchi.png"),
        )?;
    } else if kind == PackageKind::Client && platform == TargetPlatform::Windows {
        copy_required(
            &root.join("packaging/windows/sketchi.ico"),
            &stage.join("Sketchi.ico"),
        )?;
    }
    write_staging_manifest(&stage, kind, version, target)?;
    check_artifact(&stage, kind, version, target)?;
    Ok(stage)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TargetPlatform {
    Linux,
    Windows,
}

fn target_platform(target: &str) -> Result<TargetPlatform> {
    if target.contains("linux") {
        Ok(TargetPlatform::Linux)
    } else if target.contains("windows") {
        Ok(TargetPlatform::Windows)
    } else {
        bail!("unsupported packaging target {target:?}; expected Linux or Windows");
    }
}

fn copy_required(source: &Path, destination: &Path) -> Result<()> {
    if !source.is_file() {
        bail!(
            "required packaging input does not exist: {}",
            source.display()
        );
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create packaging directory {}", parent.display()))?;
    }
    fs::copy(source, destination).with_context(|| {
        format!(
            "copy packaging input {} to {}",
            source.display(),
            destination.display()
        )
    })?;
    let permissions = fs::metadata(source)?.permissions();
    fs::set_permissions(destination, permissions)?;
    Ok(())
}

fn expected_files(kind: PackageKind, target: &str) -> Result<Vec<String>> {
    let platform = target_platform(target)?;
    let mut expected = vec![
        "LICENSE.md".to_owned(),
        "STAGING-MANIFEST.txt".to_owned(),
        "VERSION".to_owned(),
        binary_name("sketchi-server", target).replace("sketchi-server", "Sketchi-server"),
    ];
    if kind == PackageKind::Client {
        expected.push(binary_name("sketchi", target).replace("sketchi", "Sketchi"));
        if platform == TargetPlatform::Linux {
            expected.push("share/applications/sketchi.desktop".to_owned());
            expected.push("share/icons/hicolor/512x512/apps/sketchi.png".to_owned());
        } else {
            expected.push("Sketchi.ico".to_owned());
        }
    }
    expected.sort();
    Ok(expected)
}

fn write_staging_manifest(
    stage: &Path,
    kind: PackageKind,
    version: &str,
    target: &str,
) -> Result<()> {
    let mut files = expected_files(kind, target)?;
    files.sort();
    let mut manifest = format!(
        "kind={}\nversion={version}\ntarget={target}\nfiles:\n",
        kind.as_str()
    );
    for file in files {
        writeln!(&mut manifest, "{file}")?;
    }
    fs::write(stage.join("STAGING-MANIFEST.txt"), manifest)
        .with_context(|| format!("write staging manifest in {}", stage.display()))?;
    Ok(())
}

fn check_artifact(path: &Path, kind: PackageKind, version: &str, target: &str) -> Result<()> {
    if !path.is_dir() {
        bail!(
            "artifact path is not a staging directory: {}",
            path.display()
        );
    }
    let expected: BTreeSet<String> = expected_files(kind, target)?.into_iter().collect();
    let actual: BTreeSet<String> = collect_files(path)?.into_iter().collect();
    let missing: Vec<&String> = expected.difference(&actual).collect();
    let unexpected: Vec<&String> = actual.difference(&expected).collect();
    if !missing.is_empty() || !unexpected.is_empty() {
        bail!("artifact contents differ; missing={missing:?}, unexpected={unexpected:?}");
    }
    let staged_version = fs::read_to_string(path.join("VERSION"))
        .with_context(|| format!("read staged version from {}", path.display()))?;
    if staged_version.trim() != version {
        bail!(
            "staged version {} does not match workspace version {version}",
            staged_version.trim()
        );
    }
    let manifest = fs::read_to_string(path.join("STAGING-MANIFEST.txt"))?;
    let expected_header = format!(
        "kind={}\nversion={version}\ntarget={target}\nfiles:\n",
        kind.as_str()
    );
    if !manifest.starts_with(&expected_header) {
        bail!("staging manifest header is incorrect in {}", path.display());
    }
    for file in actual {
        if fs::metadata(path.join(file))?.len() == 0 {
            bail!("artifact file is empty: {}", path.display());
        }
    }
    Ok(())
}

fn collect_files(root: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    collect_files_from(root, root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_files_from(root: &Path, current: &Path, files: &mut Vec<String>) -> Result<()> {
    let mut entries = fs::read_dir(current)
        .with_context(|| format!("read artifact directory {}", current.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_files_from(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .with_context(|| format!("make {} relative to {}", path.display(), root.display()))?
                .to_string_lossy()
                .replace('\\', "/");
            files.push(relative);
        } else {
            bail!(
                "unsupported filesystem entry in artifact: {}",
                path.display()
            );
        }
    }
    Ok(())
}

fn collect_absolute_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_absolute_files_from(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_absolute_files_from(current: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries = fs::read_dir(current)
        .with_context(|| format!("read checksum directory {}", current.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_absolute_files_from(&path, files)?;
        } else if file_type.is_file() {
            files.push(path);
        } else {
            bail!(
                "unsupported filesystem entry in checksum directory: {}",
                path.display()
            );
        }
    }
    Ok(())
}

fn write_checksums(input_dir: &Path, output: &Path) -> Result<()> {
    if !input_dir.is_dir() {
        bail!("checksum input is not a directory: {}", input_dir.display());
    }
    let mut paths = collect_absolute_files(input_dir)?;
    paths.retain(|path| {
        path != output
            && path
                .file_name()
                .is_none_or(|name| name != std::ffi::OsStr::new("SHA256SUMS"))
    });
    let mut entries = Vec::new();
    for path in paths {
        let relative = path
            .strip_prefix(input_dir)
            .with_context(|| {
                format!(
                    "make {} relative to {}",
                    path.display(),
                    input_dir.display()
                )
            })?
            .to_string_lossy()
            .replace('\\', "/");
        entries.push((relative, sha256_file(&path)?));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let mut manifest = String::new();
    for (path, digest) in entries {
        writeln!(&mut manifest, "{digest}  {path}")?;
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create checksum output directory {}", parent.display()))?;
    }
    fs::write(output, manifest)
        .with_context(|| format!("write checksum file {}", output.display()))?;
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        fs::File::open(path).with_context(|| format!("open {} for hashing", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(buffer.get(..read).context("hash read exceeded buffer")?);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        write!(&mut hex, "{byte:02x}")?;
    }
    Ok(hex)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    fn test_directory(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("sketchi-xtask-{name}-{nonce}"));
        fs::create_dir_all(&path).expect("create test directory");
        path
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create test parent");
        }
        fs::write(path, contents).expect("write test file");
    }

    #[test]
    fn package_directory_names_are_stable() {
        assert_eq!(
            package_directory_name(PackageKind::Client, "0.1.0", "x86_64-unknown-linux-gnu"),
            "Sketchi-0.1.0-x86_64-unknown-linux-gnu"
        );
        assert_eq!(
            package_directory_name(PackageKind::Server, "0.1.0", "x86_64-pc-windows-msvc"),
            "Sketchi-server-0.1.0-x86_64-pc-windows-msvc"
        );
    }

    #[test]
    fn linux_native_formats_require_a_linux_target() {
        for format in ["appimage", "arch", "deb", "rpm"] {
            validate_format(format, "x86_64-unknown-linux-gnu")
                .expect("native Linux format should be accepted");
            assert!(validate_format(format, "x86_64-pc-windows-msvc").is_err());
        }
    }

    #[test]
    fn client_staging_contains_sidecar_license_version_and_linux_metadata() {
        let root = test_directory("client-stage");
        let binary_dir = root.join("target/release");
        let output_dir = root.join("dist");
        write_file(&root.join("LICENSE.md"), "MIT\n");
        write_file(
            &root.join("packaging/linux/desktop/sketchi.desktop"),
            "[Desktop Entry]\nName=Sketchi\n",
        );
        write_file(
            &root.join("packaging/linux/icons/hicolor/512x512/apps/sketchi.png"),
            "png\n",
        );
        write_file(&binary_dir.join("sketchi"), "client");
        write_file(&binary_dir.join("sketchi-server"), "server");

        let staged = stage_client(
            &root,
            &binary_dir,
            &output_dir,
            "0.1.0",
            "x86_64-unknown-linux-gnu",
        )
        .expect("stage client");

        assert_eq!(
            fs::read_to_string(staged.join("Sketchi")).expect("read client"),
            "client"
        );
        assert_eq!(
            fs::read_to_string(staged.join("Sketchi-server")).expect("read sidecar"),
            "server"
        );
        assert_eq!(
            fs::read_to_string(staged.join("VERSION")).expect("read version"),
            "0.1.0\n"
        );
        assert!(staged.join("LICENSE.md").is_file());
        assert!(staged.join("share/applications/sketchi.desktop").is_file());
        assert!(
            staged
                .join("share/icons/hicolor/512x512/apps/sketchi.png")
                .is_file()
        );
        assert!(staged.join("STAGING-MANIFEST.txt").is_file());

        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn artifact_check_rejects_a_client_without_its_sidecar() {
        let root = test_directory("artifact-check");
        let binary_dir = root.join("target/release");
        let output_dir = root.join("dist");
        write_file(&root.join("LICENSE.md"), "MIT\n");
        write_file(
            &root.join("packaging/linux/desktop/sketchi.desktop"),
            "[Desktop Entry]\nName=Sketchi\n",
        );
        write_file(
            &root.join("packaging/linux/icons/hicolor/512x512/apps/sketchi.png"),
            "png\n",
        );
        write_file(&binary_dir.join("sketchi"), "client");
        write_file(&binary_dir.join("sketchi-server"), "server");
        let staged = stage_client(
            &root,
            &binary_dir,
            &output_dir,
            "0.1.0",
            "x86_64-unknown-linux-gnu",
        )
        .expect("stage client");
        fs::remove_file(staged.join("Sketchi-server")).expect("remove sidecar");

        let error = check_artifact(
            &staged,
            PackageKind::Client,
            "0.1.0",
            "x86_64-unknown-linux-gnu",
        )
        .expect_err("missing sidecar must fail artifact check");
        assert!(error.to_string().contains("Sketchi-server"));

        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn checksums_are_sorted_and_exclude_the_output_manifest() {
        let root = test_directory("checksums");
        write_file(&root.join("z-artifact.tar.gz"), "z");
        write_file(&root.join("a-artifact.AppImage"), "a");
        let output = root.join("SHA256SUMS");

        write_checksums(&root, &output).expect("write checksums");

        let lines = fs::read_to_string(output).expect("read checksums");
        let entries: Vec<&str> = lines.lines().collect();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].ends_with("  a-artifact.AppImage"));
        assert!(entries[1].ends_with("  z-artifact.tar.gz"));
        assert!(!lines.contains("SHA256SUMS"));

        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn workspace_version_and_release_tag_must_match() {
        let root = test_directory("version");
        write_file(
            &root.join("Cargo.toml"),
            "[workspace.package]\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        );

        assert_eq!(
            read_workspace_version(&root).expect("read version"),
            "0.1.0"
        );
        assert!(validate_tag("v0.1.0", "0.1.0").is_ok());
        assert!(validate_tag("0.2.0", "0.1.0").is_err());

        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn workspace_version_writer_changes_only_the_workspace_package_version() {
        let root = test_directory("set-version");
        write_file(
            &root.join("Cargo.toml"),
            "[workspace.package]\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\nmembers = []\n",
        );

        write_workspace_version(&root, "0.2.0").expect("write workspace version");

        assert_eq!(
            fs::read_to_string(root.join("Cargo.toml")).expect("read updated manifest"),
            "[workspace.package]\nversion = \"0.2.0\"\nedition = \"2024\"\n\n[workspace]\nmembers = []\n"
        );
        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn github_release_state_maps_to_explicit_flags() {
        assert_eq!(
            GitHubReleaseState::Draft.gh_args(),
            ["--draft=true", "--prerelease=false", "--latest=false"]
        );
        assert_eq!(
            GitHubReleaseState::ReleaseCandidate.gh_args(),
            ["--draft=false", "--prerelease=true", "--latest=false"]
        );
        assert_eq!(
            GitHubReleaseState::Latest.gh_args(),
            ["--draft=false", "--prerelease=false", "--latest=true"]
        );
    }

    #[test]
    fn version_output_parser_accepts_binary_name_and_version() {
        assert_eq!(
            parse_reported_version("sketchi-server 0.1.0\n").expect("parse version"),
            "0.1.0"
        );
        assert!(parse_reported_version("sketchi-server unknown\n").is_err());
    }
}
