//! Versioned collaboration message definitions.

use std::fmt;

use canvas_core::{
    ClientId, CrdtSnapshot, ElementId, Operation, OperationId, Point, VersionVector,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    MAX_CLIENT_NAME_BYTES, MAX_SELECTION, MAX_STROKE_CHUNK_POINTS, PROTOCOL_VERSION,
    error::ProtocolError,
    validation::{validate_operation_batch, validate_room_id, validate_text, validate_token},
};

/// Stable room identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct RoomId(Uuid);

impl RoomId {
    /// Creates a random room ID.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Creates a deterministic room ID for fixtures and tests.
    #[must_use]
    pub const fn from_u128(value: u128) -> Self {
        Self(Uuid::from_u128(value))
    }

    /// Returns whether this is the nil room ID.
    #[must_use]
    pub const fn is_nil(self) -> bool {
        self.0.is_nil()
    }
}

impl Default for RoomId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for RoomId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Server-side identity for one connected session.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SessionId(Uuid);

impl SessionId {
    /// Creates a random session ID.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

/// Identity for an ephemeral in-progress freehand stroke.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct StrokeId(Uuid);

impl StrokeId {
    /// Creates a random stroke ID.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Creates a deterministic stroke ID for fixtures and tests.
    #[must_use]
    pub const fn from_u128(value: u128) -> Self {
        Self(Uuid::from_u128(value))
    }
}

impl Default for StrokeId {
    fn default() -> Self {
        Self::new()
    }
}

/// Tools that may be advertised through ephemeral presence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    /// Select and manipulate existing elements.
    Select,
    /// Create rectangles.
    Rectangle,
    /// Create triangles.
    Triangle,
    /// Create ellipses.
    Ellipse,
    /// Create straight lines.
    Line,
    /// Create arrows.
    Arrow,
    /// Create freehand paths.
    Freehand,
    /// Pan the camera.
    Pan,
}

/// Ephemeral cursor, selection, and tool state.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PresenceState {
    /// Client advertising the state.
    pub client_id: ClientId,
    /// Current cursor in world coordinates, when visible.
    pub cursor: Option<Point>,
    /// Currently selected element IDs.
    pub selected_elements: Vec<ElementId>,
    /// Active editor tool.
    pub active_tool: ToolKind,
}

impl PresenceState {
    /// Validates ephemeral state bounds.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError`] when IDs, cursor coordinates, or selection
    /// bounds are invalid.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.client_id.is_nil() {
            return Err(ProtocolError::InvalidMessage(
                "presence client id cannot be nil".to_owned(),
            ));
        }
        if let Some(cursor) = self.cursor {
            cursor.validate()?;
        }
        if self.selected_elements.len() > MAX_SELECTION {
            return Err(ProtocolError::InvalidMessage(
                "presence selection exceeds the maximum size".to_owned(),
            ));
        }
        if self
            .selected_elements
            .iter()
            .any(|element_id| element_id.is_nil())
        {
            return Err(ProtocolError::InvalidMessage(
                "presence selection contains a nil element id".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Messages sent from a desktop client to a server.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Initiates a protocol session.
    Hello {
        /// Stable client identity.
        client_id: ClientId,
        /// Optional human-readable name.
        client_name: Option<String>,
    },
    /// Requests a new capability-token room.
    CreateRoom {
        /// Request correlation number.
        request_id: u64,
    },
    /// Joins an existing room with a capability token.
    JoinRoom {
        /// Room to join.
        room_id: RoomId,
        /// Capability token supplied by the room creator.
        capability_token: String,
        /// Highest version knowledge already held by the client.
        known_version: VersionVector,
    },
    /// Submits durable operations for the joined room.
    SubmitOperations {
        /// Room receiving the operations.
        room_id: RoomId,
        /// Request correlation number.
        request_id: u64,
        /// Operations to validate and apply.
        operations: Vec<Operation>,
    },
    /// Requests a snapshot plus operations after a known version.
    RequestSync {
        /// Room to synchronize.
        room_id: RoomId,
        /// Highest version knowledge already held by the client.
        known_version: VersionVector,
    },
    /// Sends ephemeral cursor and selection state.
    Presence {
        /// Room receiving the state.
        room_id: RoomId,
        /// Ephemeral presence payload.
        state: PresenceState,
    },
    /// Requests a heartbeat response.
    Ping {
        /// Caller-provided nonce.
        nonce: u64,
    },
    /// Leaves a room without destroying it.
    LeaveRoom {
        /// Room to leave.
        room_id: RoomId,
    },
    /// Starts an ephemeral freehand preview.
    StrokeStart {
        /// Room receiving the preview.
        room_id: RoomId,
        /// Preview identity.
        stroke_id: StrokeId,
        /// First world point.
        start: Point,
    },
    /// Adds points to an ephemeral freehand preview.
    StrokeChunk {
        /// Room receiving the preview.
        room_id: RoomId,
        /// Preview identity.
        stroke_id: StrokeId,
        /// Bounded chunk of world points.
        points: Vec<Point>,
    },
    /// Completes an ephemeral preview; the durable operation follows separately.
    StrokeEnd {
        /// Room receiving the preview.
        room_id: RoomId,
        /// Preview identity.
        stroke_id: StrokeId,
    },
}

/// Messages sent from a server to connected desktop clients.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Confirms the protocol session.
    Welcome {
        /// Server-assigned session identity.
        session_id: SessionId,
        /// Human-readable server version.
        server_version: String,
    },
    /// Returns the newly created room and capability token.
    RoomCreated {
        /// Request correlation number.
        request_id: u64,
        /// New room identity.
        room_id: RoomId,
        /// Capability needed to join.
        capability_token: String,
    },
    /// Sends a full room snapshot.
    Snapshot {
        /// Room represented by the snapshot.
        room_id: RoomId,
        /// CRDT state, including tombstones and causal metadata.
        snapshot: CrdtSnapshot,
    },
    /// Sends durable operations accepted after a snapshot.
    Operations {
        /// Room represented by the operations.
        room_id: RoomId,
        /// Accepted operations in server log order.
        operations: Vec<Operation>,
    },
    /// Acknowledges durable operations after commit.
    Ack {
        /// Room receiving the operations.
        room_id: RoomId,
        /// Request correlation number.
        request_id: u64,
        /// Operation IDs accepted or already known by the server.
        accepted: Vec<OperationId>,
    },
    /// Broadcasts ephemeral presence.
    Presence {
        /// Room represented by the state.
        room_id: RoomId,
        /// Ephemeral state from one client.
        state: PresenceState,
    },
    /// Announces a new room participant.
    UserJoined {
        /// Room containing the participant.
        room_id: RoomId,
        /// Participant identity.
        client_id: ClientId,
    },
    /// Announces a participant departure.
    UserLeft {
        /// Room containing the participant.
        room_id: RoomId,
        /// Participant identity.
        client_id: ClientId,
    },
    /// Heartbeat response.
    Pong {
        /// Echoed caller nonce.
        nonce: u64,
    },
    /// Indicates that a sync response is complete.
    SyncComplete {
        /// Room synchronized.
        room_id: RoomId,
        /// Server's resulting causal knowledge.
        version: VersionVector,
    },
    /// Structured protocol or authorization error.
    Error {
        /// Request correlation number, when available.
        request_id: Option<u64>,
        /// Stable machine-readable code.
        code: ErrorCode,
        /// Human-readable diagnostic.
        message: String,
    },
    /// Echoes ephemeral stroke start.
    StrokeStart {
        /// Room receiving the preview.
        room_id: RoomId,
        /// Preview identity.
        stroke_id: StrokeId,
        /// First world point.
        start: Point,
    },
    /// Echoes ephemeral stroke chunk.
    StrokeChunk {
        /// Room receiving the preview.
        room_id: RoomId,
        /// Preview identity.
        stroke_id: StrokeId,
        /// Bounded chunk of world points.
        points: Vec<Point>,
    },
    /// Echoes ephemeral stroke end.
    StrokeEnd {
        /// Room receiving the preview.
        room_id: RoomId,
        /// Preview identity.
        stroke_id: StrokeId,
    },
}

/// Stable server error categories.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Message fields are invalid.
    InvalidMessage,
    /// Capability token is absent or invalid.
    Unauthorized,
    /// Room does not exist.
    RoomNotFound,
    /// The session is not joined to the requested room.
    NotInRoom,
    /// The server could not persist or apply the operation.
    Internal,
    /// The server is rate limiting the session.
    RateLimited,
}

impl ClientMessage {
    /// Validates one client message before transport or allocation-heavy work.
    ///
    /// # Errors
    ///
    /// Returns a [`ProtocolError`] when any message field is malformed or
    /// exceeds its configured bound.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            Self::Hello {
                client_id,
                client_name,
            } => {
                if client_id.is_nil() {
                    return Err(ProtocolError::InvalidMessage(
                        "hello client id cannot be nil".to_owned(),
                    ));
                }
                if let Some(client_name) = client_name {
                    validate_text(client_name, MAX_CLIENT_NAME_BYTES)?;
                }
            }
            Self::CreateRoom { request_id } => validate_request_id(*request_id)?,
            Self::JoinRoom {
                room_id,
                capability_token,
                known_version,
            } => {
                validate_room_id(*room_id)?;
                validate_token(capability_token)?;
                known_version.validate()?;
            }
            Self::SubmitOperations {
                room_id,
                request_id,
                operations,
            } => {
                validate_room_id(*room_id)?;
                validate_request_id(*request_id)?;
                validate_operation_batch(operations)?;
            }
            Self::RequestSync {
                room_id,
                known_version,
            } => {
                validate_room_id(*room_id)?;
                known_version.validate()?;
            }
            Self::Presence { room_id, state } => {
                validate_room_id(*room_id)?;
                state.validate()?;
            }
            Self::Ping { .. } => {}
            Self::LeaveRoom { room_id } | Self::StrokeEnd { room_id, .. } => {
                validate_room_id(*room_id)?;
            }
            Self::StrokeStart { room_id, start, .. } => {
                validate_room_id(*room_id)?;
                start.validate()?;
            }
            Self::StrokeChunk {
                room_id, points, ..
            } => {
                validate_room_id(*room_id)?;
                if points.len() > MAX_STROKE_CHUNK_POINTS {
                    return Err(ProtocolError::InvalidMessage(
                        "stroke chunk exceeds the maximum size".to_owned(),
                    ));
                }
                for point in points {
                    point.validate()?;
                }
            }
        }
        Ok(())
    }
}

impl ServerMessage {
    /// Validates one server message before encoding or broadcast.
    ///
    /// # Errors
    ///
    /// Returns a [`ProtocolError`] when a message contains malformed IDs,
    /// oversized operation batches, invalid presence, or invalid stroke data.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            Self::Welcome { server_version, .. } => validate_text(server_version, 128)?,
            Self::RoomCreated {
                request_id,
                room_id,
                capability_token,
            } => {
                validate_request_id(*request_id)?;
                validate_room_id(*room_id)?;
                validate_token(capability_token)?;
            }
            Self::Snapshot { room_id, snapshot } => {
                validate_room_id(*room_id)?;
                canvas_core::CrdtDocument::from_snapshot(snapshot.clone())?;
            }
            Self::Operations {
                room_id,
                operations,
            } => {
                validate_room_id(*room_id)?;
                validate_operation_batch(operations)?;
            }
            Self::Ack {
                room_id,
                request_id,
                accepted,
            } => {
                validate_room_id(*room_id)?;
                validate_request_id(*request_id)?;
                if accepted.len() > crate::MAX_OPERATIONS_PER_MESSAGE {
                    return Err(ProtocolError::TooManyOperations);
                }
            }
            Self::Presence { room_id, state } => {
                validate_room_id(*room_id)?;
                state.validate()?;
            }
            Self::UserJoined { room_id, client_id } | Self::UserLeft { room_id, client_id } => {
                validate_room_id(*room_id)?;
                if client_id.is_nil() {
                    return Err(ProtocolError::InvalidMessage(
                        "participant client id cannot be nil".to_owned(),
                    ));
                }
            }
            Self::Pong { .. } => {}
            Self::SyncComplete { room_id, version } => {
                validate_room_id(*room_id)?;
                version.validate()?;
            }
            Self::Error { message, .. } => validate_text(message, 1024)?,
            Self::StrokeStart { room_id, start, .. } => {
                validate_room_id(*room_id)?;
                start.validate()?;
            }
            Self::StrokeChunk {
                room_id, points, ..
            } => {
                validate_room_id(*room_id)?;
                if points.len() > MAX_STROKE_CHUNK_POINTS {
                    return Err(ProtocolError::InvalidMessage(
                        "stroke chunk exceeds the maximum size".to_owned(),
                    ));
                }
                for point in points {
                    point.validate()?;
                }
            }
            Self::StrokeEnd { room_id, .. } => validate_room_id(*room_id)?,
        }
        Ok(())
    }
}

fn validate_request_id(request_id: u64) -> Result<(), ProtocolError> {
    if request_id == 0 {
        Err(ProtocolError::InvalidMessage(
            "request id must be non-zero".to_owned(),
        ))
    } else {
        Ok(())
    }
}

/// Versioned JSON envelope used for every client and server frame.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Envelope<T> {
    /// Protocol version selected by the sender.
    pub protocol_version: u16,
    /// Tagged message payload.
    pub message: T,
}

impl<T> Envelope<T> {
    /// Wraps a payload in the current protocol version.
    #[must_use]
    pub const fn current(message: T) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            message,
        }
    }
}
