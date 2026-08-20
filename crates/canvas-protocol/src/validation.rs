//! Shared protocol field and batch validation.

use std::collections::BTreeSet;

use canvas_core::{Operation, OperationId};

use crate::{MAX_OPERATIONS_PER_MESSAGE, MAX_TOKEN_BYTES, error::ProtocolError, message::RoomId};

/// Validates a bounded UTF-8 string.
pub(crate) fn validate_text(value: &str, max_bytes: usize) -> Result<(), ProtocolError> {
    if value.len() <= max_bytes {
        Ok(())
    } else {
        Err(ProtocolError::InvalidMessage(
            "text field exceeds the maximum size".to_owned(),
        ))
    }
}

/// Validates a non-empty capability token without logging or normalizing it.
pub(crate) fn validate_token(value: &str) -> Result<(), ProtocolError> {
    if value.is_empty() || value.len() > MAX_TOKEN_BYTES || value.chars().any(char::is_whitespace) {
        Err(ProtocolError::InvalidMessage(
            "capability token is empty, oversized, or contains whitespace".to_owned(),
        ))
    } else {
        Ok(())
    }
}

/// Validates a non-nil room identity.
pub(crate) fn validate_room_id(room_id: RoomId) -> Result<(), ProtocolError> {
    if room_id.is_nil() {
        Err(ProtocolError::InvalidMessage(
            "room id cannot be nil".to_owned(),
        ))
    } else {
        Ok(())
    }
}

/// Validates an operation identity carried outside a durable operation.
pub(crate) fn validate_operation_id(operation_id: OperationId) -> Result<(), ProtocolError> {
    if operation_id.client_id.is_nil() || operation_id.sequence == 0 {
        Err(ProtocolError::InvalidMessage(
            "operation id must contain a client and non-zero sequence".to_owned(),
        ))
    } else {
        Ok(())
    }
}

/// Validates an operation batch and rejects duplicate IDs in one frame.
pub(crate) fn validate_operation_batch(operations: &[Operation]) -> Result<(), ProtocolError> {
    if operations.len() > MAX_OPERATIONS_PER_MESSAGE {
        return Err(ProtocolError::TooManyOperations);
    }
    let mut ids = BTreeSet::new();
    for operation in operations {
        operation.validate()?;
        if !ids.insert(operation.id) {
            return Err(ProtocolError::InvalidMessage(
                "operation batch contains duplicate IDs".to_owned(),
            ));
        }
    }
    Ok(())
}
