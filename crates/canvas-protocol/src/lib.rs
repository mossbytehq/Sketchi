//! Versioned, bounded JSON messages shared by Sketchi clients and servers.

#![forbid(unsafe_code)]

mod error;
mod message;
mod validation;

pub use error::ProtocolError;
pub use message::{
    ClientMessage, Envelope, ErrorCode, Participant, PresenceState, RoomId, ServerMessage,
    SessionId, StrokeId, ToolKind,
};

/// Current protocol envelope version.
pub const PROTOCOL_VERSION: u16 = 2;
/// Maximum encoded JSON frame accepted by either endpoint.
///
/// This accommodates a maximum-size embedded image in both a durable
/// operation and the current snapshot representation, which retains the
/// operation log for id-reuse detection, while keeping every network message
/// bounded.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
/// Maximum durable operations in one protocol message.
pub const MAX_OPERATIONS_PER_MESSAGE: usize = 256;
/// Maximum selected elements in a presence message.
pub const MAX_SELECTION: usize = 256;
/// Maximum UTF-8 bytes in a client display name.
pub const MAX_CLIENT_NAME_BYTES: usize = 128;
/// Maximum number of participants in one collaboration room.
pub const MAX_PARTICIPANTS: usize = 4;
/// Maximum UTF-8 bytes in a capability token.
pub const MAX_TOKEN_BYTES: usize = 4096;
/// Maximum points in one ephemeral stroke chunk.
pub const MAX_STROKE_CHUNK_POINTS: usize = 2048;

/// Encodes and validates a client frame.
///
/// # Errors
///
/// Returns a [`ProtocolError`] when validation or JSON encoding fails, or the
/// encoded frame exceeds [`MAX_FRAME_BYTES`].
pub fn encode_client(message: &ClientMessage) -> Result<Vec<u8>, ProtocolError> {
    message.validate()?;
    encode(&Envelope::current(message))
}

/// Decodes and validates a client frame.
///
/// # Errors
///
/// Returns a [`ProtocolError`] for oversized frames, invalid JSON, unsupported
/// versions, or invalid message fields.
pub fn decode_client(bytes: &[u8]) -> Result<ClientMessage, ProtocolError> {
    let envelope: Envelope<ClientMessage> = decode(bytes)?;
    envelope.message.validate()?;
    Ok(envelope.message)
}

/// Encodes and validates a server frame.
///
/// # Errors
///
/// Returns a [`ProtocolError`] when validation or JSON encoding fails, or the
/// encoded frame exceeds [`MAX_FRAME_BYTES`].
pub fn encode_server(message: &ServerMessage) -> Result<Vec<u8>, ProtocolError> {
    message.validate()?;
    encode(&Envelope::current(message))
}

/// Decodes and validates a server frame.
///
/// # Errors
///
/// Returns a [`ProtocolError`] for oversized frames, invalid JSON, unsupported
/// versions, or invalid message fields.
pub fn decode_server(bytes: &[u8]) -> Result<ServerMessage, ProtocolError> {
    let envelope: Envelope<ServerMessage> = decode(bytes)?;
    envelope.message.validate()?;
    Ok(envelope.message)
}

fn encode<T: serde::Serialize>(envelope: &Envelope<T>) -> Result<Vec<u8>, ProtocolError> {
    let encoded = serde_json::to_vec(&envelope)?;
    if encoded.len() > MAX_FRAME_BYTES {
        Err(ProtocolError::FrameTooLarge)
    } else {
        Ok(encoded)
    }
}

fn decode<T: for<'de> serde::Deserialize<'de>>(bytes: &[u8]) -> Result<Envelope<T>, ProtocolError> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge);
    }
    let envelope: Envelope<T> = serde_json::from_slice(bytes)?;
    if envelope.protocol_version != PROTOCOL_VERSION {
        return Err(ProtocolError::UnsupportedVersion(envelope.protocol_version));
    }
    Ok(envelope)
}
