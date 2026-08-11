//! Per-room serialized command actor.

use canvas_core::{ClientId, Operation, VersionVector};
use canvas_protocol::{PresenceState, RoomId};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::room::{Room, RoomError, SubmitOutcome, SyncPayload};

/// Commands processed in one room's serialized task.
pub enum RoomCommand {
    /// Add a room member.
    Join {
        /// Client identity.
        client_id: ClientId,
        /// Completion channel.
        response: oneshot::Sender<Result<(), RoomError>>,
    },
    /// Remove a room member.
    Leave {
        /// Client identity.
        client_id: ClientId,
        /// Completion channel.
        response: oneshot::Sender<Result<(), RoomError>>,
    },
    /// Submit durable operations.
    Submit {
        /// Client identity.
        client_id: ClientId,
        /// Candidate operations.
        operations: Vec<Operation>,
        /// Completion channel.
        response: oneshot::Sender<Result<SubmitOutcome, RoomError>>,
    },
    /// Request snapshot-plus-delta sync.
    Sync {
        /// Client causal knowledge.
        known_version: VersionVector,
        /// Completion channel.
        response: oneshot::Sender<Result<SyncPayload, RoomError>>,
    },
    /// Update ephemeral presence.
    Presence {
        /// Presence state.
        state: PresenceState,
        /// Completion channel.
        response: oneshot::Sender<Result<(), RoomError>>,
    },
}

/// Handle for sending commands to one room actor.
#[derive(Clone)]
pub struct RoomActorHandle {
    room_id: RoomId,
    sender: mpsc::Sender<RoomCommand>,
}

impl RoomActorHandle {
    /// Returns the room identity.
    #[must_use]
    pub const fn room_id(&self) -> RoomId {
        self.room_id
    }

    /// Joins a client to the serialized room state.
    ///
    /// # Errors
    ///
    /// Returns [`RoomError`] when the actor has stopped.
    pub async fn join(&self, client_id: ClientId) -> Result<(), RoomError> {
        let (response, receiver) = oneshot::channel();
        self.sender
            .send(RoomCommand::Join {
                client_id,
                response,
            })
            .await
            .map_err(|_| RoomError::ActorStopped)?;
        receiver.await.map_err(|_| RoomError::ActorStopped)?
    }

    /// Leaves a client from the serialized room state.
    ///
    /// # Errors
    ///
    /// Returns [`RoomError`] when the actor has stopped.
    pub async fn leave(&self, client_id: ClientId) -> Result<(), RoomError> {
        let (response, receiver) = oneshot::channel();
        self.sender
            .send(RoomCommand::Leave {
                client_id,
                response,
            })
            .await
            .map_err(|_| RoomError::ActorStopped)?;
        receiver.await.map_err(|_| RoomError::ActorStopped)?
    }

    /// Sends ephemeral presence to the serialized room state.
    ///
    /// # Errors
    ///
    /// Returns [`RoomError`] when the actor has stopped or the client is not a
    /// room member.
    pub async fn update_presence(&self, state: PresenceState) -> Result<(), RoomError> {
        let (response, receiver) = oneshot::channel();
        self.sender
            .send(RoomCommand::Presence { state, response })
            .await
            .map_err(|_| RoomError::ActorStopped)?;
        receiver.await.map_err(|_| RoomError::ActorStopped)?
    }

    /// Sends a submission to the room actor.
    ///
    /// # Errors
    ///
    /// Returns [`RoomError`] when the actor is stopped or room validation fails.
    pub async fn submit(
        &self,
        client_id: ClientId,
        operations: Vec<Operation>,
    ) -> Result<SubmitOutcome, RoomError> {
        let (response, receiver) = oneshot::channel();
        self.sender
            .send(RoomCommand::Submit {
                client_id,
                operations,
                response,
            })
            .await
            .map_err(|_| RoomError::ActorStopped)?;
        receiver.await.map_err(|_| RoomError::ActorStopped)?
    }

    /// Requests synchronization from the room actor.
    ///
    /// # Errors
    ///
    /// Returns [`RoomError`] when the actor is stopped.
    pub async fn sync(&self, known_version: VersionVector) -> Result<SyncPayload, RoomError> {
        let (response, receiver) = oneshot::channel();
        self.sender
            .send(RoomCommand::Sync {
                known_version,
                response,
            })
            .await
            .map_err(|_| RoomError::ActorStopped)?;
        receiver.await.map_err(|_| RoomError::ActorStopped)?
    }
}

/// Spawns one serialized room command loop.
#[must_use]
pub fn spawn(room: Room) -> (RoomActorHandle, JoinHandle<()>) {
    let room_id = room.id();
    let (sender, mut receiver) = mpsc::channel(128);
    let handle = RoomActorHandle { room_id, sender };
    let task = tokio::spawn(async move {
        let mut room = room;
        while let Some(command) = receiver.recv().await {
            match command {
                RoomCommand::Join {
                    client_id,
                    response,
                } => {
                    room.join(client_id);
                    let _ = response.send(Ok(()));
                }
                RoomCommand::Leave {
                    client_id,
                    response,
                } => {
                    room.leave(client_id);
                    let _ = response.send(Ok(()));
                }
                RoomCommand::Submit {
                    client_id,
                    operations,
                    response,
                } => {
                    let result = room.submit(client_id, &operations);
                    let _ = response.send(result);
                }
                RoomCommand::Sync {
                    known_version,
                    response,
                } => {
                    let result = Ok(room.sync(&known_version));
                    let _ = response.send(result);
                }
                RoomCommand::Presence { state, response } => {
                    let result = room.update_presence(state);
                    let _ = response.send(result);
                }
            }
        }
    });
    (handle, task)
}
