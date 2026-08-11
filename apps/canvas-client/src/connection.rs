//! Bounded connection and local-first synchronization primitives.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use canvas_core::{Operation, OperationId, VersionVector};
use canvas_protocol::{
    ClientMessage, MAX_OPERATIONS_PER_MESSAGE, ProtocolError, RoomId, ServerMessage, decode_server,
    encode_client,
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
use crate::supervisor::{ReconnectBackoff, ReconnectState};

/// Bounded channel capacity for UI/network handoff.
pub const CHANNEL_CAPACITY: usize = 128;

/// Connection-level errors visible to the editor loop.
#[derive(Debug, Error)]
pub enum ConnectionError {
    /// The network task has stopped receiving messages.
    #[error("connection channel is closed")]
    Closed,
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
    } = endpoints;
    loop {
        let connector = config.connector()?;
        if let Ok((socket, _)) =
            connect_async_tls_with_config(config.endpoint(), None, true, connector).await
        {
            backoff.on_connected();
            match run_socket(socket, &mut outbound, &inbound).await {
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
) -> Result<(), ConnectionError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (mut writer, mut reader) = socket.split();
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
}

/// Network task endpoints used to construct a bounded handoff.
pub struct NetworkEndpoints {
    /// Receiver consumed by the network task.
    pub outbound: mpsc::Receiver<ClientMessage>,
    /// Sender used by the network task.
    pub inbound: mpsc::Sender<ServerMessage>,
}

/// Creates bounded channels between the UI and network tasks.
#[must_use]
pub fn bounded_channels() -> (ConnectionChannels, NetworkEndpoints) {
    let (outbound, outbound_receiver) = mpsc::channel(CHANNEL_CAPACITY);
    let (inbound_sender, inbound) = mpsc::channel(CHANNEL_CAPACITY);
    (
        ConnectionChannels { outbound, inbound },
        NetworkEndpoints {
            outbound: outbound_receiver,
            inbound: inbound_sender,
        },
    )
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
                    self.known_version.merge(&operation.deps);
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
