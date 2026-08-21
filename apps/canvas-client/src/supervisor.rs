//! Local collaboration-server supervision boundaries.

use std::{
    io::{BufRead, BufReader, Read},
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

use flate2::read::GzDecoder;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

include!(concat!(env!("OUT_DIR"), "/embedded_server.rs"));

/// Maximum number of reconnect attempts permitted by the client state machine.
pub const MAX_RECONNECT_ATTEMPTS: u32 = 8;

/// Default delay before the first reconnect attempt.
pub const DEFAULT_RECONNECT_INITIAL_DELAY: Duration = Duration::from_millis(250);

/// Default maximum reconnect delay.
pub const DEFAULT_RECONNECT_MAX_DELAY: Duration = Duration::from_secs(8);

/// Wildcard address used by the supervised server so peers on the local LAN
/// can reach the advertised endpoint.
pub const SUPERVISED_BIND_ADDRESS: &str = "0.0.0.0:0";

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const MAX_SERVER_DIAGNOSTICS_BYTES: usize = 16 * 1024;
const SIDECAR_VERSION_TIMEOUT: Duration = Duration::from_secs(2);
const SERVER_READINESS_TIMEOUT: Duration = Duration::from_secs(10);

/// Readiness payload emitted by a supervised `sketchi-server` process.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ReadyMessage {
    /// WebSocket endpoint accepted by the server.
    pub endpoint: String,
    /// SHA-256 pin for the loopback certificate.
    pub certificate_sha256: String,
}

/// Errors raised while parsing local-server readiness.
#[derive(Debug, Error)]
pub enum SupervisorError {
    /// Readiness output was not valid JSON or did not contain required fields.
    #[error("invalid local server readiness: {0}")]
    InvalidReadiness(String),
    /// Reconnect delays or attempt bounds could not form a finite policy.
    #[error("invalid reconnect backoff: {0}")]
    InvalidBackoff(String),
    /// The supervised server process could not be started or read.
    #[error("local server process failed: {0}")]
    Io(#[from] std::io::Error),
    /// The supervised server exited before readiness and reported a startup error.
    #[error("local server did not become ready: {0}")]
    Startup(String),
    /// The supervised server exited before emitting readiness.
    #[error("local server exited before readiness")]
    ExitedBeforeReadiness,
}

/// Parses one JSON readiness line from a supervised server.
///
/// # Errors
///
/// Returns [`SupervisorError::InvalidReadiness`] when the line is malformed or
/// has an empty endpoint/pin.
pub fn parse_ready_line(line: &str) -> Result<ReadyMessage, SupervisorError> {
    let ready: ReadyMessage = serde_json::from_str(line)
        .map_err(|error| SupervisorError::InvalidReadiness(error.to_string()))?;
    let valid_pin = ready.certificate_sha256.len() == 64
        && ready
            .certificate_sha256
            .chars()
            .all(|character| character.is_ascii_hexdigit());
    if !ready.endpoint.starts_with("wss://") || !valid_pin {
        return Err(SupervisorError::InvalidReadiness(
            "readiness requires a wss endpoint and a 64-character SHA-256 pin".to_owned(),
        ));
    }
    Ok(ready)
}

/// A locally supervised `sketchi-server` child and its pinned endpoint.
#[derive(Debug)]
pub struct LocalServer {
    child: Child,
    readiness: ReadyMessage,
    extracted_server: Option<PathBuf>,
    stderr_thread: Option<JoinHandle<Result<String, std::io::Error>>>,
}

impl LocalServer {
    /// Starts the matching server sidecar next to the current client binary.
    ///
    /// This is the local-room path used by packaged clients. Development
    /// builds accept either the branded or technical binary filename.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError`] when the sidecar cannot be found, its data
    /// directory cannot be created, or readiness cannot be read.
    pub fn spawn_default() -> Result<Self, SupervisorError> {
        let executable = std::env::current_exe()?;
        let directory = executable
            .parent()
            .ok_or_else(|| {
                SupervisorError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "client executable has no parent directory",
                ))
            })?
            .to_owned();
        let names = if cfg!(windows) {
            ["Sketchi-server.exe", "sketchi-server.exe"]
        } else {
            ["Sketchi-server", "sketchi-server"]
        };
        let sidecar = names
            .iter()
            .map(|name| directory.join(name))
            .find(|candidate| candidate.is_file());
        let database = ProjectDirs::from("org", "Sketchi", "Sketchi").map_or_else(
            || directory.join("sketchi.sqlite3"),
            |directories| directories.data_dir().join("sketchi.sqlite3"),
        );
        if let Some(parent) = database.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Some(sidecar) = sidecar {
            let sidecar_matches_client = Self::sidecar_matches_client_version(&sidecar);
            if sidecar_matches_client || EMBEDDED_SERVER_GZIP.is_empty() {
                match Self::spawn(Self::server_command(&sidecar, &database)) {
                    Ok(server) => return Ok(server),
                    Err(error) if !EMBEDDED_SERVER_GZIP.is_empty() => {
                        tracing::warn!(
                            executable = %sidecar.display(),
                            error = %error,
                            "adjacent Sketchi server failed; trying embedded server"
                        );
                    }
                    Err(error) => return Err(error),
                }
            } else {
                tracing::warn!(
                    executable = %sidecar.display(),
                    client_version = env!("CARGO_PKG_VERSION"),
                    "adjacent Sketchi server is stale; trying embedded server"
                );
            }
        }

        let (embedded, extracted_server) = Self::extract_embedded_server()?;
        let mut server = Self::spawn(Self::server_command(&embedded, &database))?;
        server.extracted_server = extracted_server;
        Ok(server)
    }

    fn server_command(executable: &std::path::Path, database: &std::path::Path) -> Command {
        let mut command = Command::new(executable);
        command
            .arg("--ready")
            .arg("--bind")
            .arg(SUPERVISED_BIND_ADDRESS)
            .arg("--database")
            .arg(database);
        command
    }

    fn sidecar_matches_client_version(executable: &std::path::Path) -> bool {
        let mut command = Command::new(executable);
        command.arg("--version");
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        let Some(output) = command_output_with_timeout(command, SIDECAR_VERSION_TIMEOUT) else {
            return false;
        };
        if !output.status.success() {
            return false;
        }
        output_matches_client_version(&output.stdout, &output.stderr)
    }

    /// Spawns a server command and waits for its first readiness line.
    ///
    /// The command should include `--ready`; its stdout is consumed only until
    /// the JSON readiness line is received, while diagnostics remain on stderr.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError`] when the child cannot start, exits early,
    /// or emits malformed readiness data. A failed startup is terminated.
    pub fn spawn(mut command: Command) -> Result<Self, SupervisorError> {
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);

        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stderr_thread = child.stderr.take().map(|stderr| {
            std::thread::Builder::new()
                .name(String::from("sketchi-server-stderr"))
                .spawn(move || {
                    let mut reader = BufReader::new(stderr);
                    let mut output = String::new();
                    let mut line = String::new();
                    loop {
                        line.clear();
                        if reader.read_line(&mut line)? == 0 {
                            break;
                        }
                        tracing::debug!(
                            target: "sketchi-server",
                            output = %line.trim_end(),
                            "supervised server output"
                        );
                        if output.len() < MAX_SERVER_DIAGNOSTICS_BYTES {
                            for character in line.chars() {
                                if output.len() + character.len_utf8()
                                    > MAX_SERVER_DIAGNOSTICS_BYTES
                                {
                                    break;
                                }
                                output.push(character);
                            }
                        }
                    }
                    Ok(output)
                })
        });
        let stderr_thread = match stderr_thread {
            Some(Ok(thread)) => Some(thread),
            Some(Err(error)) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(SupervisorError::Io(error));
            }
            None => None,
        };
        let result = Self::readiness_from_child(&mut child);
        match result {
            Ok(readiness) => Ok(Self {
                child,
                readiness,
                extracted_server: None,
                stderr_thread,
            }),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let diagnostics = stderr_thread
                    .and_then(|thread| thread.join().ok())
                    .and_then(Result::ok)
                    .map(|output| output.trim().to_owned())
                    .filter(|output| !output.is_empty());
                match diagnostics {
                    Some(diagnostics) => Err(SupervisorError::Startup(format!(
                        "{error}; server output: {diagnostics}"
                    ))),
                    None => Err(error),
                }
            }
        }
    }

    /// Returns the pinned endpoint emitted by the server.
    #[must_use]
    pub const fn readiness(&self) -> &ReadyMessage {
        &self.readiness
    }

    /// Returns the operating-system child process ID.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    fn readiness_from_child(child: &mut Child) -> Result<ReadyMessage, SupervisorError> {
        let stdout = child
            .stdout
            .take()
            .ok_or(SupervisorError::ExitedBeforeReadiness)?;
        let (sender, receiver) = mpsc::channel();
        let reader = std::thread::Builder::new()
            .name(String::from("sketchi-server-readiness"))
            .spawn(move || {
                let mut lines = BufReader::new(stdout).lines();
                let result = lines
                    .next()
                    .transpose()
                    .map_err(SupervisorError::Io)
                    .and_then(|line| {
                        line.map_or(Err(SupervisorError::ExitedBeforeReadiness), |line| {
                            parse_ready_line(&line)
                        })
                    });
                let _ = sender.send(result);
            })
            .map_err(SupervisorError::Io)?;
        match receiver.recv_timeout(SERVER_READINESS_TIMEOUT) {
            Ok(result) => {
                let _ = reader.join();
                result
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                Err(SupervisorError::Startup(format!(
                    "server did not emit readiness within {} seconds",
                    SERVER_READINESS_TIMEOUT.as_secs()
                )))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = reader.join();
                Err(SupervisorError::ExitedBeforeReadiness)
            }
        }
    }

    fn extract_embedded_server() -> Result<(PathBuf, Option<PathBuf>), SupervisorError> {
        if EMBEDDED_SERVER_GZIP.is_empty() {
            return Err(SupervisorError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Sketchi server sidecar was not found next to the client",
            )));
        }

        let extraction_directory = std::env::temp_dir()
            .join("Sketchi")
            .join(std::process::id().to_string());
        std::fs::create_dir_all(&extraction_directory)?;
        let executable = extraction_directory.join(if cfg!(windows) {
            "Sketchi-server.exe"
        } else {
            "Sketchi-server"
        });
        let mut decoder = GzDecoder::new(EMBEDDED_SERVER_GZIP);
        let mut server = Vec::new();
        decoder.read_to_end(&mut server)?;
        if let Err(error) = std::fs::write(&executable, server) {
            let _ = std::fs::remove_dir_all(&extraction_directory);
            return Err(SupervisorError::Io(error));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = std::fs::metadata(&executable)?.permissions();
            permissions.set_mode(0o700);
            std::fs::set_permissions(&executable, permissions)?;
        }
        Ok((executable, Some(extraction_directory)))
    }
}

fn command_output_with_timeout(mut command: Command, timeout: Duration) -> Option<Output> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = Instant::now().checked_add(timeout)?;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                child.stdout.take()?.read_to_end(&mut stdout).ok()?;
                child.stderr.take()?.read_to_end(&mut stderr).ok()?;
                return Some(Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

fn output_matches_client_version(stdout: &[u8], stderr: &[u8]) -> bool {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    stdout
        .split_whitespace()
        .chain(stderr.split_whitespace())
        .any(|token| {
            token.trim_matches(|character: char| {
                !character.is_ascii_alphanumeric() && character != '.' && character != '-'
            }) == env!("CARGO_PKG_VERSION")
        })
}

impl Drop for LocalServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(directory) = self.extracted_server.take() {
            let _ = std::fs::remove_dir_all(directory);
        }
        if let Some(thread) = self.stderr_thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::process::Command;
    #[cfg(unix)]
    use std::time::{Duration, Instant};

    #[cfg(unix)]
    use super::command_output_with_timeout;
    use super::output_matches_client_version;

    #[test]
    fn sidecar_version_must_match_the_client() {
        let version = env!("CARGO_PKG_VERSION");
        let current = format!("sketchi-server {version}\n");
        let stale = format!("sketchi-server {version}.1\n");
        assert!(output_matches_client_version(current.as_bytes(), &[]));
        assert!(!output_matches_client_version(stale.as_bytes(), &[]));
    }

    #[test]
    fn sidecar_version_can_be_read_from_stderr() {
        let version = env!("CARGO_PKG_VERSION");
        let output = format!("server version: {version}\n");
        assert!(output_matches_client_version(&[], output.as_bytes()));
    }

    #[cfg(unix)]
    #[test]
    fn version_probe_timeout_terminates_a_stuck_process() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 30"]);
        let started = Instant::now();
        assert!(command_output_with_timeout(command, Duration::from_millis(20)).is_none());
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}

/// State exposed by the bounded reconnect controller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReconnectState {
    /// No connection has been established yet or the caller may start one.
    Disconnected,
    /// A connection is currently usable.
    Connected,
    /// The caller should wait before attempting the numbered retry.
    Waiting {
        /// One-based reconnect attempt number.
        attempt: u32,
        /// Delay selected for this attempt.
        delay: Duration,
    },
    /// The configured retry bound has been reached.
    Exhausted {
        /// Number of reconnect attempts that were permitted.
        attempts: u32,
    },
}

/// Deterministic exponential reconnect/backoff state with hard bounds.
pub struct ReconnectBackoff {
    initial_delay: Duration,
    max_delay: Duration,
    max_attempts: u32,
    attempts: u32,
    state: ReconnectState,
}

impl ReconnectBackoff {
    /// Creates a bounded backoff policy.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError::InvalidBackoff`] when delays are zero or
    /// inverted, or when the attempt bound is zero or exceeds
    /// [`MAX_RECONNECT_ATTEMPTS`].
    pub fn new(
        initial_delay: Duration,
        max_delay: Duration,
        max_attempts: u32,
    ) -> Result<Self, SupervisorError> {
        if initial_delay.is_zero() {
            return Err(SupervisorError::InvalidBackoff(
                "initial delay must be greater than zero".to_owned(),
            ));
        }
        if max_delay < initial_delay {
            return Err(SupervisorError::InvalidBackoff(
                "maximum delay must not be less than initial delay".to_owned(),
            ));
        }
        if max_attempts == 0 || max_attempts > MAX_RECONNECT_ATTEMPTS {
            return Err(SupervisorError::InvalidBackoff(
                "attempt bound is outside the supported range".to_owned(),
            ));
        }
        Ok(Self {
            initial_delay,
            max_delay,
            max_attempts,
            attempts: 0,
            state: ReconnectState::Disconnected,
        })
    }

    /// Creates the default bounded reconnect policy.
    ///
    /// The default is eight attempts from 250 milliseconds through a maximum
    /// delay of eight seconds.
    #[must_use]
    pub fn default_policy() -> Self {
        Self {
            initial_delay: DEFAULT_RECONNECT_INITIAL_DELAY,
            max_delay: DEFAULT_RECONNECT_MAX_DELAY,
            max_attempts: MAX_RECONNECT_ATTEMPTS,
            attempts: 0,
            state: ReconnectState::Disconnected,
        }
    }

    /// Returns the current bounded state.
    #[must_use]
    pub const fn state(&self) -> ReconnectState {
        self.state
    }

    /// Returns the number of reconnect attempts used since the last success.
    #[must_use]
    pub const fn attempts(&self) -> u32 {
        self.attempts
    }

    /// Records a failed connection and selects the next bounded delay.
    pub fn on_disconnect(&mut self) -> ReconnectState {
        if self.attempts >= self.max_attempts {
            self.state = ReconnectState::Exhausted {
                attempts: self.attempts,
            };
            return self.state;
        }
        self.attempts = self.attempts.saturating_add(1);
        let delay = self.delay_for(self.attempts);
        self.state = ReconnectState::Waiting {
            attempt: self.attempts,
            delay,
        };
        self.state
    }

    /// Records a successful connection and resets the retry budget.
    pub fn on_connected(&mut self) {
        self.attempts = 0;
        self.state = ReconnectState::Connected;
    }

    /// Returns to the initial disconnected state and clears retry history.
    pub fn reset(&mut self) {
        self.attempts = 0;
        self.state = ReconnectState::Disconnected;
    }

    fn delay_for(&self, attempt: u32) -> Duration {
        let mut delay = self.initial_delay;
        for _ in 1..attempt {
            delay = delay
                .checked_mul(2)
                .map_or(self.max_delay, |candidate| candidate.min(self.max_delay));
            if delay == self.max_delay {
                break;
            }
        }
        delay
    }
}
