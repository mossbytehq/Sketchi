//! Local collaboration-server supervision boundaries.

use std::{
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};

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
        let (executable, extracted_server) = match names
            .iter()
            .map(|name| directory.join(name))
            .find(|candidate| candidate.is_file())
        {
            Some(executable) => (executable, None),
            None => Self::extract_embedded_server()?,
        };
        let database = ProjectDirs::from("org", "Sketchi", "Sketchi").map_or_else(
            || directory.join("sketchi.sqlite3"),
            |directories| directories.data_dir().join("sketchi.sqlite3"),
        );
        if let Some(parent) = database.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut command = Command::new(executable);
        command
            .arg("--ready")
            .arg("--bind")
            .arg("127.0.0.1:0")
            .arg("--database")
            .arg(database);
        let mut server = Self::spawn(command)?;
        server.extracted_server = extracted_server;
        Ok(server)
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
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let result = Self::readiness_from_child(&mut child);
        match result {
            Ok(readiness) => Ok(Self {
                child,
                readiness,
                extracted_server: None,
            }),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(error)
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
        let mut lines = BufReader::new(stdout).lines();
        let line = lines
            .next()
            .transpose()?
            .ok_or(SupervisorError::ExitedBeforeReadiness)?;
        parse_ready_line(&line)
    }

    fn extract_embedded_server() -> Result<(PathBuf, Option<PathBuf>), SupervisorError> {
        if EMBEDDED_SERVER.is_empty() {
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
        if let Err(error) = std::fs::write(&executable, EMBEDDED_SERVER) {
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

impl Drop for LocalServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(directory) = self.extracted_server.take() {
            let _ = std::fs::remove_dir_all(directory);
        }
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
