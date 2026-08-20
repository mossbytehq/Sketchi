//! Bounded connection and local-first synchronization primitives.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Display,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use canvas_core::{Operation, OperationId, VersionVector};
use canvas_protocol::{
    ClientMessage, MAX_OPERATIONS_PER_MESSAGE, Participant, PresenceState, ProtocolError, RoomId,
    ServerMessage, decode_server, encode_client,
};
use futures_util::{SinkExt, StreamExt};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use rustls::{
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_tungstenite::{
    Connector, WebSocketStream, connect_async_tls_with_config, tungstenite::Message,
};

use crate::storage::{Journal, StorageError};
use crate::supervisor::{ReadyMessage, ReconnectBackoff, ReconnectState};

/// Bounded channel capacity for UI/network handoff.
pub const CHANNEL_CAPACITY: usize = 128;

/// Connection-level errors visible to the editor loop.
#[derive(Debug, Error)]
pub enum ConnectionError {
    /// The network task has stopped receiving messages.
    #[error("connection channel is closed")]
    Closed,
    /// The bounded outbound queue is temporarily full.
    #[error("connection outbound queue is full")]
    QueueFull,
    /// The durable local operation journal could not be read or written.
    #[error("local journal error: {0}")]
    Storage(#[from] StorageError),
    /// A replay batch size was outside the protocol's bounded range.
    #[error("replay batch size must be between 1 and {MAX_OPERATIONS_PER_MESSAGE}")]
    InvalidBatchSize,
    /// A protocol request ID was zero.
    #[error("request ID must be non-zero")]
    InvalidRequestId,
    /// Request IDs could not be incremented without wrapping.
    #[error("request ID exhausted while creating replay batches")]
    RequestIdExhausted,
    /// The endpoint is not a `ws://` or `wss://` URL.
    #[error("invalid WebSocket endpoint: {0}")]
    InvalidEndpoint(String),
    /// A TLS endpoint did not provide a valid SHA-256 certificate pin.
    #[error("invalid TLS certificate pin")]
    InvalidCertificatePin,
    /// A TLS endpoint was configured without a certificate pin.
    #[error("a certificate pin is required for wss endpoints")]
    MissingCertificatePin,
    /// The configured TLS crypto provider is unavailable.
    #[error("TLS crypto provider is unavailable")]
    TlsProviderUnavailable,
    /// The WebSocket transport failed.
    #[error("WebSocket transport failed: {0}")]
    Transport(#[from] tokio_tungstenite::tungstenite::Error),
    /// A server frame was invalid.
    #[error("invalid server frame: {0}")]
    Protocol(#[from] ProtocolError),
    /// A payload could not be represented as UTF-8 WebSocket text.
    #[error("outbound frame is not UTF-8: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
    /// The remote peer closed the connection.
    #[error("WebSocket connection closed")]
    Disconnected,
    /// The UI stopped receiving inbound messages.
    #[error("inbound connection channel is closed")]
    InboundClosed,
    /// All bounded reconnect attempts were exhausted.
    #[error("reconnect attempts exhausted")]
    ReconnectExhausted,
    /// A room ID entered by the user was not a UUID.
    #[error("invalid room ID")]
    InvalidRoomId,
    /// The client runtime thread could not be created.
    #[error("could not start collaboration runtime: {0}")]
    RuntimeThread(String),
    /// The endpoint and certificate pin in an invite were incomplete or unsafe.
    #[error("invalid collaboration invite: {0}")]
    InvalidInvite(String),
}

/// Validated WebSocket connection configuration.
#[derive(Clone, Debug)]
pub struct ConnectionConfig {
    endpoint: String,
    certificate_pin: Option<[u8; 32]>,
}

impl ConnectionConfig {
    /// Validates an endpoint and its optional pinned certificate.
    ///
    /// `wss://` endpoints require a 64-character hexadecimal SHA-256 pin;
    /// `ws://` is retained for explicit loopback development only.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectionError`] when the endpoint or TLS pin is invalid.
    pub fn new(
        endpoint: impl Into<String>,
        certificate_sha256: Option<&str>,
    ) -> Result<Self, ConnectionError> {
        let endpoint = endpoint.into();
        let is_tls = endpoint.starts_with("wss://");
        if !is_tls && !endpoint.starts_with("ws://") {
            return Err(ConnectionError::InvalidEndpoint(endpoint));
        }
        let certificate_pin = certificate_sha256.map(decode_certificate_pin).transpose()?;
        if is_tls && certificate_pin.is_none() {
            return Err(ConnectionError::MissingCertificatePin);
        }
        Ok(Self {
            endpoint,
            certificate_pin,
        })
    }

    /// Returns the validated endpoint.
    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    fn connector(&self) -> Result<Option<Connector>, ConnectionError> {
        if !self.endpoint.starts_with("wss://") {
            return Ok(None);
        }
        let certificate_pin = self
            .certificate_pin
            .ok_or(ConnectionError::MissingCertificatePin)?;
        let provider = rustls::crypto::CryptoProvider::get_default()
            .ok_or(ConnectionError::TlsProviderUnavailable)?;
        let verifier = PinnedCertificateVerifier {
            certificate_pin,
            algorithms: &provider.signature_verification_algorithms,
        };
        let config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        Ok(Some(Connector::Rustls(Arc::new(config))))
    }
}

/// Runs one bounded, reconnecting WebSocket network task.
///
/// Outbound messages stay in the bounded channel while the server is
/// unavailable. Reconnection does not duplicate the channel or mutate the
/// durable journal; callers replay journaled operations after their join/sync
/// handshake.
///
/// # Errors
///
/// Returns [`ConnectionError`] when the UI closes its channel, the inbound
/// channel is dropped, or the reconnect policy is exhausted.
pub async fn run_reconnecting(
    config: ConnectionConfig,
    endpoints: NetworkEndpoints,
    mut backoff: ReconnectBackoff,
) -> Result<(), ConnectionError> {
    let NetworkEndpoints {
        mut outbound,
        inbound,
        handshake,
    } = endpoints;
    loop {
        let connector = config.connector()?;
        if let Ok((socket, _)) =
            connect_async_tls_with_config(config.endpoint(), None, true, connector).await
        {
            backoff.on_connected();
            match run_socket(socket, &mut outbound, &inbound, &handshake).await {
                Ok(()) => return Ok(()),
                Err(ConnectionError::Disconnected | ConnectionError::Transport(_)) => {}
                Err(error) => return Err(error),
            }
        }
        match backoff.on_disconnect() {
            ReconnectState::Waiting { delay, .. } => sleep(delay).await,
            ReconnectState::Exhausted { .. } => return Err(ConnectionError::ReconnectExhausted),
            ReconnectState::Connected | ReconnectState::Disconnected => {
                return Err(ConnectionError::ReconnectExhausted);
            }
        }
    }
}

async fn run_socket<S>(
    socket: WebSocketStream<S>,
    outbound: &mut mpsc::Receiver<ClientMessage>,
    inbound: &mpsc::Sender<ServerMessage>,
    handshake: &Mutex<Vec<ClientMessage>>,
) -> Result<(), ConnectionError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (mut writer, mut reader) = socket.split();
    let handshake_messages = handshake
        .lock()
        .map(|messages| messages.clone())
        .unwrap_or_default();
    for message in handshake_messages {
        let payload = encode_client(&message)?;
        writer
            .send(Message::Text(String::from_utf8(payload)?.into()))
            .await?;
    }
    loop {
        tokio::select! {
            message = outbound.recv() => {
                let Some(message) = message else {
                    return Ok(());
                };
                let payload = encode_client(&message)?;
                writer.send(Message::Text(String::from_utf8(payload)?.into())).await?;
            }
            message = reader.next() => {
                let Some(message) = message else {
                    return Err(ConnectionError::Disconnected);
                };
                match message? {
                    Message::Text(text) => inbound.send(decode_server(text.as_bytes())?).await.map_err(|_| ConnectionError::InboundClosed)?,
                    Message::Binary(bytes) => inbound.send(decode_server(&bytes)?).await.map_err(|_| ConnectionError::InboundClosed)?,
                    Message::Close(_) => return Err(ConnectionError::Disconnected),
                    Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
                }
            }
        }
    }
}

#[derive(Debug)]
struct PinnedCertificateVerifier {
    certificate_pin: [u8; 32],
    algorithms: &'static rustls::crypto::WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinnedCertificateVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let digest: [u8; 32] = Sha256::digest(end_entity.as_ref()).into();
        if digest == self.certificate_pin {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "server certificate pin did not match".to_owned(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

fn decode_certificate_pin(value: &str) -> Result<[u8; 32], ConnectionError> {
    if value.len() != 64 {
        return Err(ConnectionError::InvalidCertificatePin);
    }
    let mut decoded = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks(2).enumerate() {
        let high = pair.first().and_then(|byte| hex_nibble(*byte));
        let low = pair.get(1).and_then(|byte| hex_nibble(*byte));
        let Some((high, low)) = high.zip(low) else {
            return Err(ConnectionError::InvalidCertificatePin);
        };
        let Some(slot) = decoded.get_mut(index) else {
            return Err(ConnectionError::InvalidCertificatePin);
        };
        *slot = (high << 4) | low;
    }
    Ok(decoded)
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

/// UI-to-network sender and network-to-UI receiver.
pub struct ConnectionChannels {
    /// Outbound client messages.
    pub outbound: mpsc::Sender<ClientMessage>,
    /// Inbound server messages.
    pub inbound: mpsc::Receiver<ServerMessage>,
    /// Shared handshake replay state used before every transport connection.
    pub handshake: Arc<Mutex<Vec<ClientMessage>>>,
}

/// Network task endpoints used to construct a bounded handoff.
pub struct NetworkEndpoints {
    /// Receiver consumed by the network task.
    pub outbound: mpsc::Receiver<ClientMessage>,
    /// Sender used by the network task.
    pub inbound: mpsc::Sender<ServerMessage>,
    /// Shared handshake replay state used before every transport connection.
    pub handshake: Arc<Mutex<Vec<ClientMessage>>>,
}

/// Creates bounded channels between the UI and network tasks.
#[must_use]
pub fn bounded_channels() -> (ConnectionChannels, NetworkEndpoints) {
    let (outbound, outbound_receiver) = mpsc::channel(CHANNEL_CAPACITY);
    let (inbound_sender, inbound) = mpsc::channel(CHANNEL_CAPACITY);
    let handshake = Arc::new(Mutex::new(Vec::new()));
    (
        ConnectionChannels {
            outbound,
            inbound,
            handshake: Arc::clone(&handshake),
        },
        NetworkEndpoints {
            outbound: outbound_receiver,
            inbound: inbound_sender,
            handshake,
        },
    )
}

/// The action selected from the collaboration dialog.
#[derive(Clone, Debug, PartialEq)]
pub enum CollaborationIntent {
    /// Create a new room and automatically join it.
    Create {
        /// Readiness data for the local server hosting the room.
        readiness: ReadyMessage,
        /// Display name shown to other participants.
        display_name: String,
    },
    /// Join an existing room with its capability token.
    Join {
        /// Room identifier entered by the user.
        room_id: RoomId,
        /// Capability token entered by the user.
        capability_token: String,
        /// Validated server readiness data for the room.
        readiness: ReadyMessage,
        /// Display name shown to other participants.
        display_name: String,
    },
}

/// A small immutable view of the collaboration session for the UI.
#[derive(Clone, Debug, PartialEq)]
pub struct CollaborationView {
    /// Human-readable connection or protocol status.
    pub status: String,
    /// Room currently being created, joined, or occupied.
    pub room_id: Option<String>,
    /// Capability token returned for a newly created room.
    pub capability_token: Option<String>,
    /// WebSocket endpoint that collaborators must use.
    pub endpoint: Option<String>,
    /// SHA-256 certificate pin that accompanies the endpoint.
    pub certificate_sha256: Option<String>,
    /// Whether the local server is available for a new session.
    pub server_available: bool,
    /// Current ephemeral participant roster.
    pub participants: Vec<Participant>,
    /// Current ephemeral cursor, selection, and tool state for each participant.
    pub presence: Vec<PresenceState>,
}

impl CollaborationView {
    /// Creates the disconnected view shown before the first collaboration action.
    #[must_use]
    pub fn disconnected(server_available: bool) -> Self {
        Self {
            status: String::from("Not connected"),
            room_id: None,
            capability_token: None,
            endpoint: None,
            certificate_sha256: None,
            server_available,
            participants: Vec::new(),
            presence: Vec::new(),
        }
    }
}

/// Parses the UUID-shaped room identifier used by the share dialog.
///
/// # Errors
///
/// Returns [`ConnectionError::InvalidRoomId`] when `value` is not a serialized
/// [`RoomId`].
pub fn parse_room_id(value: &str) -> Result<RoomId, ConnectionError> {
    serde_json::from_str(&format!("\"{}\"", value.trim()))
        .map_err(|_| ConnectionError::InvalidRoomId)
}

/// A portable room invite containing all connection details needed by a joiner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoomInvite {
    /// Room identifier to join.
    pub room_id: RoomId,
    /// Capability token authorizing the join.
    pub capability_token: String,
    /// Server endpoint included by newer invite copies.
    pub endpoint: Option<String>,
    /// Certificate pin included by newer invite copies.
    pub certificate_sha256: Option<String>,
}

/// Formats a room invite as one copyable value.
#[must_use]
pub fn format_room_invite(
    room_id: impl Display,
    capability_token: &str,
    readiness: &ReadyMessage,
) -> String {
    format!(
        "{room_id}:{capability_token}|{}|{}",
        readiness.endpoint, readiness.certificate_sha256
    )
}

/// Parses a current or legacy room invite.
///
/// Legacy `room_id:token` values remain accepted and use the local or manually
/// entered endpoint. Current invites append `|endpoint|certificate_pin` so a
/// joiner can paste one value without separately copying advanced fields.
///
/// # Errors
///
/// Returns [`ConnectionError`] when the room ID, capability token, endpoint, or
/// certificate pin is malformed.
pub fn parse_room_invite(value: &str) -> Result<RoomInvite, ConnectionError> {
    let mut parts = value.trim().splitn(3, '|');
    let room_and_token = parts.next().unwrap_or_default();
    let (room_id_text, capability_token) = room_and_token.split_once(':').ok_or_else(|| {
        ConnectionError::InvalidInvite(String::from(
            "invite must contain room ID and capability token",
        ))
    })?;
    let capability_token = capability_token.trim();
    if capability_token.is_empty() {
        return Err(ConnectionError::InvalidInvite(String::from(
            "invite capability token cannot be empty",
        )));
    }
    let endpoint = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let certificate_sha256 = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if endpoint.is_some() != certificate_sha256.is_some() {
        return Err(ConnectionError::InvalidInvite(String::from(
            "invite endpoint and certificate pin must be provided together",
        )));
    }
    if let (Some(endpoint), Some(certificate_sha256)) = (endpoint, certificate_sha256) {
        ConnectionConfig::new(endpoint.to_owned(), Some(certificate_sha256))?;
        Ok(RoomInvite {
            room_id: parse_room_id(room_id_text)?,
            capability_token: capability_token.to_owned(),
            endpoint: Some(endpoint.to_owned()),
            certificate_sha256: Some(certificate_sha256.to_owned()),
        })
    } else {
        Ok(RoomInvite {
            room_id: parse_room_id(room_id_text)?,
            capability_token: capability_token.to_owned(),
            endpoint: None,
            certificate_sha256: None,
        })
    }
}

/// Resolves optional invite connection fields against this instance's server.
///
/// Blank endpoint and pin use `local`. Custom invites must provide both fields,
/// use `wss://`, and contain a valid 64-character SHA-256 certificate pin.
///
/// # Errors
///
/// Returns [`ConnectionError::InvalidInvite`] when the pair is incomplete or no
/// local fallback exists, or a connection validation error for malformed data.
pub fn resolve_invite_readiness(
    local: Option<&ReadyMessage>,
    endpoint: &str,
    certificate_sha256: &str,
) -> Result<ReadyMessage, ConnectionError> {
    let endpoint = endpoint.trim();
    let certificate_sha256 = certificate_sha256.trim();
    if endpoint.is_empty() && certificate_sha256.is_empty() {
        return local.cloned().ok_or_else(|| {
            ConnectionError::InvalidInvite(String::from(
                "no local server is available; enter an endpoint and certificate pin",
            ))
        });
    }
    if endpoint.is_empty() || certificate_sha256.is_empty() {
        return Err(ConnectionError::InvalidInvite(String::from(
            "endpoint and certificate pin must be provided together",
        )));
    }
    if !endpoint.starts_with("wss://") {
        return Err(ConnectionError::InvalidInvite(String::from(
            "invite endpoint must use wss://",
        )));
    }
    ConnectionConfig::new(endpoint.to_owned(), Some(certificate_sha256))?;
    Ok(ReadyMessage {
        endpoint: endpoint.to_owned(),
        certificate_sha256: certificate_sha256.to_owned(),
    })
}

/// UI-facing collaboration session that owns the network runtime and sync journal.
pub struct CollaborationClient {
    channels: ConnectionChannels,
    handshake: Arc<Mutex<Vec<ClientMessage>>>,
    runtime: Option<JoinHandle<Result<(), ConnectionError>>>,
    synchronization: SyncController,
    room_id: Option<RoomId>,
    capability_token: Option<String>,
    readiness: ReadyMessage,
    next_request_id: u64,
    sent_operations: BTreeSet<OperationId>,
    status: String,
    server_available: bool,
    client_id: canvas_core::ClientId,
    display_name: String,
    participants: BTreeMap<canvas_core::ClientId, String>,
    presence: BTreeMap<canvas_core::ClientId, PresenceState>,
    presence_throttle: PresenceThrottle,
    create_request_id: Option<u64>,
}

impl CollaborationClient {
    /// Starts the bounded network task and queues the Hello handshake.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectionError`] when the readiness data, journal, or
    /// runtime thread cannot be initialized.
    pub fn start(
        client_id: canvas_core::ClientId,
        journal: Journal,
        intent: CollaborationIntent,
    ) -> Result<Self, ConnectionError> {
        let readiness = match &intent {
            CollaborationIntent::Create { readiness, .. }
            | CollaborationIntent::Join { readiness, .. } => readiness,
        };
        let config = ConnectionConfig::new(
            readiness.endpoint.clone(),
            Some(&readiness.certificate_sha256),
        )?;
        let (channels, endpoints) = bounded_channels();
        let backoff = ReconnectBackoff::default_policy();
        let mut client = Self {
            handshake: Arc::clone(&channels.handshake),
            channels,
            runtime: None,
            synchronization: SyncController::new(journal),
            room_id: None,
            capability_token: None,
            readiness: readiness.clone(),
            next_request_id: 1,
            sent_operations: BTreeSet::new(),
            status: String::from("Connecting to local collaboration server…"),
            server_available: true,
            client_id,
            display_name: match &intent {
                CollaborationIntent::Create { display_name, .. }
                | CollaborationIntent::Join { display_name, .. } => display_name.clone(),
            },
            participants: BTreeMap::new(),
            presence: BTreeMap::new(),
            presence_throttle: PresenceThrottle::new(Duration::from_millis(50))
                .map_err(|error| ConnectionError::InvalidInvite(error.to_string()))?,
            create_request_id: None,
        };
        client
            .participants
            .insert(client_id, client.display_name.clone());
        match intent {
            CollaborationIntent::Create { .. } => {
                client.status = String::from("Creating collaboration room…");
                let request_id = client.next_request_id();
                client.create_request_id = Some(request_id);
            }
            CollaborationIntent::Join {
                room_id,
                capability_token,
                ..
            } => {
                client.room_id = Some(room_id);
                client.capability_token = Some(capability_token.clone());
                client.status = format!("Joining room {room_id}…");
            }
        }
        client.refresh_handshake();

        let runtime = thread::Builder::new()
            .name(String::from("sketchi-collaboration"))
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| ConnectionError::RuntimeThread(error.to_string()))?;
                runtime.block_on(run_reconnecting(config, endpoints, backoff))
            })
            .map_err(|error| ConnectionError::RuntimeThread(error.to_string()))?;
        client.runtime = Some(runtime);
        Ok(client)
    }

    /// Returns the current collaboration state for rendering.
    #[must_use]
    pub fn view(&self) -> CollaborationView {
        CollaborationView {
            status: self.status.clone(),
            room_id: self.room_id.map(|room_id| room_id.to_string()),
            capability_token: self.capability_token.clone(),
            endpoint: Some(self.readiness.endpoint.clone()),
            certificate_sha256: Some(self.readiness.certificate_sha256.clone()),
            server_available: self.server_available,
            participants: self
                .participants
                .iter()
                .map(|(client_id, name)| Participant {
                    client_id: *client_id,
                    name: name.clone(),
                })
                .collect(),
            presence: self.presence.values().cloned().collect(),
        }
    }

    /// Returns the sync controller used by the editor boundary.
    pub const fn synchronization_mut(&mut self) -> &mut SyncController {
        &mut self.synchronization
    }

    /// Returns the room currently associated with this session.
    #[must_use]
    pub const fn room_id(&self) -> Option<RoomId> {
        self.room_id
    }

    /// Persists and sends every newly queued local operation in bounded batches.
    ///
    /// A full outbound channel leaves unsent operations out of `sent_operations`,
    /// so the next event-loop turn retries without blocking the UI.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectionError::Storage`] when the journal cannot be read or
    /// [`ConnectionError::QueueFull`] when the bounded transport queue is full.
    pub fn queue_pending(&mut self) -> Result<(), ConnectionError> {
        let Some(room_id) = self.room_id else {
            return Ok(());
        };
        let pending = self.synchronization.pending_operations()?;
        let unsent = pending
            .into_iter()
            .filter(|operation| !self.sent_operations.contains(&operation.id))
            .collect::<Vec<_>>();
        for operations in unsent.chunks(MAX_OPERATIONS_PER_MESSAGE) {
            let request_id = self.next_request_id();
            let message = ClientMessage::SubmitOperations {
                room_id,
                request_id,
                operations: operations.to_vec(),
            };
            self.send(message)?;
            self.sent_operations
                .extend(operations.iter().map(|operation| operation.id));
        }
        Ok(())
    }

    /// Polls inbound frames without blocking the winit event loop.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectionError`] when the inbound channel closes or the
    /// runtime task finishes with a transport error.
    pub fn poll(&mut self) -> Result<Vec<ServerMessage>, ConnectionError> {
        self.join_finished_runtime()?;
        self.flush_presence()?;
        let mut messages = Vec::new();
        loop {
            match self.channels.inbound.try_recv() {
                Ok(message) => messages.push(message),
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.join_finished_runtime()?;
                    self.status = String::from("Collaboration connection closed");
                    return Err(ConnectionError::InboundClosed);
                }
            }
        }
        Ok(messages)
    }

    fn join_finished_runtime(&mut self) -> Result<(), ConnectionError> {
        if self.runtime.as_ref().is_some_and(JoinHandle::is_finished)
            && let Some(runtime) = self.runtime.take()
        {
            match runtime.join() {
                Ok(Ok(())) => self.status = String::from("Collaboration disconnected"),
                Ok(Err(error)) => {
                    self.status = error.to_string();
                    return Err(error);
                }
                Err(_) => {
                    self.status = String::from("Collaboration runtime stopped unexpectedly");
                    return Err(ConnectionError::Closed);
                }
            }
        }
        Ok(())
    }

    /// Updates connection state and performs the automatic create-to-join step.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectionError::QueueFull`] or [`ConnectionError::Closed`]
    /// when the automatic join message cannot be queued.
    pub fn observe(&mut self, message: &ServerMessage) -> Result<(), ConnectionError> {
        match message {
            ServerMessage::Welcome { .. } => {
                self.status = String::from("Connected; waiting for room sync…");
            }
            ServerMessage::RoomCreated {
                room_id,
                capability_token,
                ..
            } => {
                self.room_id = Some(*room_id);
                self.capability_token = Some(capability_token.clone());
                self.create_request_id = None;
                self.status = format!("Room {room_id} created; joining…");
                self.refresh_handshake();
                self.send(
                    self.synchronization
                        .join_message(*room_id, capability_token.clone()),
                )?;
            }
            ServerMessage::SyncComplete { .. } => {
                self.status = String::from("In collaboration room");
                self.sent_operations.clear();
            }
            ServerMessage::Participants { participants, .. } => {
                self.participants = participants
                    .iter()
                    .map(|participant| (participant.client_id, participant.name.clone()))
                    .collect();
                self.participants
                    .entry(self.client_id)
                    .or_insert_with(|| self.display_name.clone());
            }
            ServerMessage::UserJoined {
                client_id, name, ..
            } => {
                self.participants.insert(*client_id, name.clone());
            }
            ServerMessage::UserLeft { client_id, .. } => {
                self.participants.remove(client_id);
                self.presence.remove(client_id);
            }
            ServerMessage::Presence { state, .. } => {
                self.presence.insert(state.client_id, state.clone());
            }
            ServerMessage::Ack { accepted, .. } => {
                for operation_id in accepted {
                    self.sent_operations.remove(operation_id);
                }
            }
            ServerMessage::Error { code, message, .. } => {
                self.status = if *code == canvas_protocol::ErrorCode::RoomFull {
                    String::from("This room is full (maximum 4 participants).")
                } else {
                    format!("Collaboration error: {message}")
                };
            }
            _ => {}
        }
        Ok(())
    }

    /// Sends one protocol message through the bounded channel.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectionError::QueueFull`] when the bounded channel is full
    /// or [`ConnectionError::Closed`] when the runtime has stopped receiving.
    pub fn send(&self, message: ClientMessage) -> Result<(), ConnectionError> {
        self.channels
            .outbound
            .try_send(message)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => ConnectionError::QueueFull,
                mpsc::error::TrySendError::Closed(_) => ConnectionError::Closed,
            })
    }

    /// Offers local ephemeral state to the bounded presence transport.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectionError`] when the bounded outbound queue is closed or full.
    pub fn offer_presence(
        &mut self,
        room_id: RoomId,
        state: PresenceState,
    ) -> Result<(), ConnectionError> {
        if let Some(message) = self.presence_throttle.offer(room_id, state, Instant::now()) {
            self.send(message)?;
        }
        Ok(())
    }

    fn flush_presence(&mut self) -> Result<(), ConnectionError> {
        if let Some(message) = self.presence_throttle.flush(Instant::now()) {
            self.send(message)?;
        }
        Ok(())
    }

    fn refresh_handshake(&self) {
        let messages = handshake_messages(
            self.client_id,
            &self.display_name,
            self.room_id,
            self.capability_token.as_deref(),
            self.create_request_id,
            self.synchronization.known_version(),
        );
        publish_handshake(&self.handshake, messages);
    }

    /// Records a user-visible transport or protocol error.
    pub fn set_error(&mut self, message: String) {
        self.status = message;
    }

    fn next_request_id(&mut self) -> u64 {
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1).max(1);
        request_id
    }
}

fn publish_handshake(handshake: &Arc<Mutex<Vec<ClientMessage>>>, messages: Vec<ClientMessage>) {
    if let Ok(mut handshake) = handshake.lock() {
        *handshake = messages;
    }
}

fn handshake_messages(
    client_id: canvas_core::ClientId,
    display_name: &str,
    room_id: Option<RoomId>,
    capability_token: Option<&str>,
    create_request_id: Option<u64>,
    known_version: &VersionVector,
) -> Vec<ClientMessage> {
    let mut messages = vec![ClientMessage::Hello {
        client_id,
        client_name: Some(display_name.to_owned()),
    }];
    if let (Some(room_id), Some(capability_token)) = (room_id, capability_token) {
        messages.push(ClientMessage::JoinRoom {
            room_id,
            capability_token: capability_token.to_owned(),
            known_version: known_version.clone(),
        });
    } else if let Some(request_id) = create_request_id {
        messages.push(ClientMessage::CreateRoom { request_id });
    }
    messages
}

impl Drop for CollaborationClient {
    fn drop(&mut self) {
        self.runtime.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_surfaces_finished_runtime_error_before_closed_inbound_channel() {
        let (channels, endpoints) = bounded_channels();
        drop(endpoints);
        let runtime = thread::spawn(|| Err(ConnectionError::ReconnectExhausted));
        while !runtime.is_finished() {
            thread::yield_now();
        }
        let journal = Journal::open_in_memory();
        assert!(journal.is_ok());
        let Ok(journal) = journal else {
            return;
        };
        let handshake = Arc::clone(&channels.handshake);
        let Ok(presence_throttle) = PresenceThrottle::new(Duration::from_millis(50)) else {
            return;
        };
        let mut client = CollaborationClient {
            channels,
            handshake,
            runtime: Some(runtime),
            synchronization: SyncController::new(journal),
            room_id: None,
            capability_token: None,
            readiness: ReadyMessage {
                endpoint: String::from("wss://127.0.0.1:3000/ws"),
                certificate_sha256: "ab".repeat(32),
            },
            next_request_id: 1,
            sent_operations: BTreeSet::new(),
            status: String::from("Connecting"),
            server_available: true,
            client_id: canvas_core::ClientId::from_u128(1),
            display_name: String::from("Test user"),
            participants: BTreeMap::new(),
            presence: BTreeMap::new(),
            presence_throttle,
            create_request_id: None,
        };

        assert!(matches!(
            client.poll(),
            Err(ConnectionError::ReconnectExhausted)
        ));
        assert_eq!(client.status, "reconnect attempts exhausted");
    }

    #[test]
    fn reconnect_handshake_replays_create_until_room_exists_then_rejoins() {
        let client_id = canvas_core::ClientId::from_u128(1);
        let known_version = VersionVector::default();
        let create =
            handshake_messages(client_id, "Test user", None, None, Some(7), &known_version);
        assert!(matches!(create.first(), Some(ClientMessage::Hello { .. })));
        assert_eq!(
            create.get(1),
            Some(&ClientMessage::CreateRoom { request_id: 7 })
        );

        let room_id = RoomId::from_u128(9);
        let joined = handshake_messages(
            client_id,
            "Test user",
            Some(room_id),
            Some("capability"),
            None,
            &known_version,
        );
        assert_eq!(
            joined.get(1),
            Some(&ClientMessage::JoinRoom {
                room_id,
                capability_token: String::from("capability"),
                known_version,
            })
        );
    }

    #[test]
    fn startup_handshake_can_be_published_before_runtime_is_spawned() {
        let (channels, _endpoints) = bounded_channels();
        let known_version = VersionVector::default();
        let messages = handshake_messages(
            canvas_core::ClientId::from_u128(1),
            "Test user",
            None,
            None,
            Some(7),
            &known_version,
        );

        publish_handshake(&channels.handshake, messages.clone());

        let stored = channels
            .handshake
            .lock()
            .map(|handshake| handshake.clone())
            .unwrap_or_default();
        assert_eq!(stored, messages);
    }
}

/// Result of applying a server message to synchronization metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncUpdate {
    /// The server message did not affect durable queue or sync metadata.
    Ignored,
    /// A snapshot replaced the server-side portion of causal knowledge.
    Snapshot,
    /// A delta advanced causal knowledge.
    Operations,
    /// One acknowledgement was applied to the durable queue.
    Acknowledged,
    /// The server reported that a sync response is complete.
    SyncComplete,
}

/// Local-first operation queue backed by a durable [`Journal`].
pub struct SyncController {
    journal: Journal,
    known_version: VersionVector,
}

impl SyncController {
    /// Creates a controller using the supplied durable operation journal.
    #[must_use]
    pub fn new(journal: Journal) -> Self {
        Self {
            journal,
            known_version: VersionVector::default(),
        }
    }

    /// Queues one locally applied operation durably before transport sends it.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectionError::Storage`] when the journal cannot commit the
    /// operation.
    pub fn enqueue(&mut self, operation: &Operation) -> Result<(), ConnectionError> {
        self.journal.append(operation)?;
        self.known_version.merge(&operation.deps);
        self.known_version.observe(operation.id);
        Ok(())
    }

    /// Queues locally applied operations in one durable transaction.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectionError::Storage`] when the journal cannot commit the
    /// batch.
    pub fn enqueue_all(&mut self, operations: &[Operation]) -> Result<(), ConnectionError> {
        self.journal.append_all(operations)?;
        for operation in operations {
            self.known_version.merge(&operation.deps);
            self.known_version.observe(operation.id);
        }
        Ok(())
    }

    /// Returns all operations that have not received an acknowledgement.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectionError::Storage`] when the journal cannot be read.
    pub fn pending_operations(&self) -> Result<Vec<Operation>, ConnectionError> {
        Ok(self.journal.load()?)
    }

    /// Returns the number of durable operations still awaiting acknowledgement.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectionError::Storage`] when the journal cannot be read.
    pub fn pending_count(&self) -> Result<usize, ConnectionError> {
        Ok(self.journal.pending_count()?)
    }

    /// Builds deterministic, protocol-sized replay messages.
    ///
    /// The journal is not modified. Calling this method again with the same
    /// room, request ID, and batch size produces the same operation batches,
    /// which makes a retry safe until an acknowledgement removes their IDs.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectionError::InvalidBatchSize`] for zero or oversized
    /// batches, [`ConnectionError::InvalidRequestId`] for a zero request ID,
    /// [`ConnectionError::RequestIdExhausted`] if IDs would wrap, or
    /// [`ConnectionError::Storage`] when the journal cannot be read.
    pub fn replay_pending(
        &self,
        room_id: RoomId,
        first_request_id: u64,
        batch_size: usize,
    ) -> Result<Vec<ClientMessage>, ConnectionError> {
        if batch_size == 0 || batch_size > MAX_OPERATIONS_PER_MESSAGE {
            return Err(ConnectionError::InvalidBatchSize);
        }
        if first_request_id == 0 {
            return Err(ConnectionError::InvalidRequestId);
        }

        let pending = self.journal.load()?;
        pending
            .chunks(batch_size)
            .enumerate()
            .map(|(index, operations)| {
                let index =
                    u64::try_from(index).map_err(|_| ConnectionError::RequestIdExhausted)?;
                let request_id = first_request_id
                    .checked_add(index)
                    .ok_or(ConnectionError::RequestIdExhausted)?;
                Ok(ClientMessage::SubmitOperations {
                    room_id,
                    request_id,
                    operations: operations.to_vec(),
                })
            })
            .collect()
    }

    /// Removes only the operation IDs explicitly accepted by the server.
    ///
    /// Repeating an acknowledgement is harmless because deleting an absent
    /// journal row is a no-op. Unacknowledged operations remain available for
    /// the next replay.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectionError::Storage`] when the journal cannot be updated.
    pub fn acknowledge(&self, operation_ids: &[OperationId]) -> Result<(), ConnectionError> {
        self.journal.remove(operation_ids)?;
        Ok(())
    }

    /// Applies server acknowledgement and sync metadata without materializing
    /// document state. The caller should separately apply snapshots and
    /// operations through its CRDT/editor boundary.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectionError::Storage`] when an acknowledgement cannot be
    /// persisted locally.
    pub fn apply_server_message(
        &mut self,
        message: &ServerMessage,
    ) -> Result<SyncUpdate, ConnectionError> {
        match message {
            ServerMessage::Ack { accepted, .. } => {
                self.acknowledge(accepted)?;
                Ok(SyncUpdate::Acknowledged)
            }
            ServerMessage::Snapshot { snapshot, .. } => {
                self.known_version = snapshot.version_vector.clone();
                for operation in self.journal.load()? {
                    self.known_version.merge(&operation.deps);
                    self.known_version.observe(operation.id);
                }
                Ok(SyncUpdate::Snapshot)
            }
            ServerMessage::Operations { operations, .. } => {
                for operation in operations {
                    self.known_version.observe(operation.id);
                }
                Ok(SyncUpdate::Operations)
            }
            ServerMessage::SyncComplete { version, .. } => {
                self.known_version.merge(version);
                Ok(SyncUpdate::SyncComplete)
            }
            _ => Ok(SyncUpdate::Ignored),
        }
    }

    /// Builds a capability-token room join request using current causal state.
    #[must_use]
    pub fn join_message(
        &self,
        room_id: RoomId,
        capability_token: impl Into<String>,
    ) -> ClientMessage {
        ClientMessage::JoinRoom {
            room_id,
            capability_token: capability_token.into(),
            known_version: self.known_version.clone(),
        }
    }

    /// Builds a snapshot-plus-delta sync request using current causal state.
    #[must_use]
    pub fn request_sync_message(&self, room_id: RoomId) -> ClientMessage {
        ClientMessage::RequestSync {
            room_id,
            known_version: self.known_version.clone(),
        }
    }

    /// Returns the causal knowledge represented by the local editor plus the
    /// server messages observed by this controller.
    #[must_use]
    pub const fn known_version(&self) -> &VersionVector {
        &self.known_version
    }
}

/// Error raised when a presence throttle cannot be configured.
#[derive(Debug, Eq, Error, PartialEq)]
pub enum PresenceThrottleError {
    /// A zero interval would permit an unbounded update rate.
    #[error("presence throttle interval must be greater than zero")]
    ZeroInterval,
}

/// Coalesces ephemeral presence updates to one outbound message per interval.
pub struct PresenceThrottle {
    interval: Duration,
    next_allowed: Option<Instant>,
    pending: Option<ClientMessage>,
}

impl PresenceThrottle {
    /// Creates a throttle with a strictly positive minimum send interval.
    ///
    /// # Errors
    ///
    /// Returns [`PresenceThrottleError::ZeroInterval`] when `interval` is zero.
    pub fn new(interval: Duration) -> Result<Self, PresenceThrottleError> {
        if interval.is_zero() {
            return Err(PresenceThrottleError::ZeroInterval);
        }
        Ok(Self {
            interval,
            next_allowed: None,
            pending: None,
        })
    }

    /// Offers a presence state, returning it immediately only when the rate
    /// limit allows a send. While limited, only the newest state is retained.
    pub fn offer(
        &mut self,
        room_id: RoomId,
        state: canvas_protocol::PresenceState,
        now: Instant,
    ) -> Option<ClientMessage> {
        let message = ClientMessage::Presence { room_id, state };
        if self.is_allowed(now) {
            self.pending = None;
            self.next_allowed = Some(self.deadline(now));
            Some(message)
        } else {
            self.pending = Some(message);
            None
        }
    }

    /// Flushes the newest coalesced state when the interval has elapsed.
    pub fn flush(&mut self, now: Instant) -> Option<ClientMessage> {
        if self.pending.is_some() && self.is_allowed(now) {
            self.next_allowed = Some(self.deadline(now));
            self.pending.take()
        } else {
            None
        }
    }

    /// Returns whether a coalesced presence state is waiting to be sent.
    #[must_use]
    pub const fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    fn is_allowed(&self, now: Instant) -> bool {
        self.next_allowed.is_none_or(|deadline| now >= deadline)
    }

    fn deadline(&self, now: Instant) -> Instant {
        now.checked_add(self.interval).unwrap_or(now)
    }
}
