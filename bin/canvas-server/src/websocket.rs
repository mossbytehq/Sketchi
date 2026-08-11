//! Axum WebSocket session and room broadcast handling.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use canvas_core::ClientId;
use canvas_protocol::{
    ClientMessage, ErrorCode, RoomId, ServerMessage, SessionId, decode_client, encode_server,
};
use futures_util::{SinkExt, StreamExt};
use hyper::{Request, body::Incoming};
use hyper_util::{
    rt::{TokioExecutor, TokioIo},
    server::conn::auto::Builder,
    service::TowerToHyperService,
};
use serde::Serialize;
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::{Mutex as AsyncMutex, mpsc};
use tokio_rustls::TlsAcceptor;
use tower::{ServiceExt, service_fn};

use crate::room::{RoomError, RoomManager};

/// Errors returned by the HTTP/WebSocket server.
#[derive(Debug, Error)]
pub enum TransportError {
    /// TCP listener or axum serving failed.
    #[error("transport I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// Axum server failed while serving requests.
    #[error("HTTP server failed: {0}")]
    Axum(#[from] axum::Error),
    /// Readiness JSON could not be encoded.
    #[error("readiness JSON failed: {0}")]
    Json(#[from] serde_json::Error),
}

/// JSON emitted when a supervised server has bound its listener.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Readiness {
    /// WebSocket endpoint accepted by the client.
    pub endpoint: String,
    /// SHA-256 digest of the server certificate in lowercase hexadecimal.
    pub certificate_sha256: String,
}

impl Readiness {
    /// Creates a readiness payload for a supervised endpoint.
    #[must_use]
    pub fn new(endpoint: impl Into<String>, certificate_sha256: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            certificate_sha256: certificate_sha256.into(),
        }
    }
}

#[derive(Clone)]
struct SessionPeer {
    client_id: Option<ClientId>,
    room_id: Option<RoomId>,
    sender: mpsc::Sender<ServerMessage>,
}

/// Shared state for HTTP health and WebSocket sessions.
#[derive(Clone)]
pub struct ServerState {
    manager: Arc<AsyncMutex<RoomManager>>,
    sessions: Arc<AsyncMutex<BTreeMap<SessionId, SessionPeer>>>,
    server_version: String,
}

impl ServerState {
    /// Creates transport state around a room manager.
    #[must_use]
    pub fn new(manager: RoomManager) -> Self {
        Self {
            manager: Arc::new(AsyncMutex::new(manager)),
            sessions: Arc::new(AsyncMutex::new(BTreeMap::new())),
            server_version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    }
}

/// Builds the health and WebSocket routes.
pub fn router(state: ServerState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/ws", get(websocket_upgrade))
        .with_state(state)
}

/// Serves the non-TLS loopback development endpoint.
///
/// # Errors
///
/// Returns [`TransportError`] when binding or serving the listener fails.
pub async fn serve_http(state: ServerState, bind: SocketAddr) -> Result<(), TransportError> {
    let listener = TcpListener::bind(bind).await?;
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Serves the WebSocket router behind a rustls acceptor.
///
/// # Errors
///
/// Returns [`TransportError`] when the listener cannot bind or accept a TCP
/// connection. Individual TLS/HTTP connection failures are logged and do not
/// stop the listener.
pub async fn serve_tls(
    state: ServerState,
    bind: SocketAddr,
    config: rustls::ServerConfig,
) -> Result<(), TransportError> {
    serve_tls_with_readiness(state, bind, config, None).await
}

/// Serves the WebSocket router and optionally emits supervised-server
/// readiness after the listener has successfully bound.
///
/// # Errors
///
/// Returns [`TransportError`] when the listener cannot bind, readiness JSON
/// cannot be encoded, or an accept loop fails.
pub async fn serve_tls_with_readiness(
    state: ServerState,
    bind: SocketAddr,
    config: rustls::ServerConfig,
    certificate_sha256: Option<&str>,
) -> Result<(), TransportError> {
    let listener = TcpListener::bind(bind).await?;
    if let Some(certificate_sha256) = certificate_sha256 {
        let endpoint = format!("wss://{}/ws", listener.local_addr()?);
        let readiness = Readiness::new(endpoint, certificate_sha256);
        println!("{}", serde_json::to_string(&readiness)?);
    }
    let acceptor = TlsAcceptor::from(Arc::new(config));
    loop {
        let (stream, _) = tokio::select! {
            result = listener.accept() => result?,
            () = shutdown_signal() => break,
        };
        let acceptor = acceptor.clone();
        let app = router(state.clone());
        tokio::spawn(async move {
            let Ok(stream) = acceptor.accept(stream).await else {
                return;
            };
            let service = service_fn(move |request: Request<Incoming>| {
                let app = app.clone();
                async move { app.oneshot(request.map(axum::body::Body::new)).await }
            });
            let io = TokioIo::new(stream);
            let hyper_service = TowerToHyperService::new(service);
            let builder = Builder::new(TokioExecutor::new());
            if let Err(error) = builder
                .serve_connection_with_upgrades(io, hyper_service)
                .await
            {
                tracing::debug!(%error, "TLS connection closed with an error");
            }
        });
    }
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn websocket_upgrade(ws: WebSocketUpgrade, State(state): State<ServerState>) -> Response {
    ws.on_upgrade(move |socket| session(socket, state))
        .into_response()
}

async fn session(socket: WebSocket, state: ServerState) {
    let session_id = SessionId::new();
    let (mut socket_sender, mut socket_receiver) = socket.split();
    let (sender, mut outbound) = mpsc::channel(128);
    state.sessions.lock().await.insert(
        session_id,
        SessionPeer {
            client_id: None,
            room_id: None,
            sender: sender.clone(),
        },
    );

    let writer = tokio::spawn(async move {
        while let Some(message) = outbound.recv().await {
            let Ok(payload) = encode_server(&message) else {
                continue;
            };
            let Ok(text) = String::from_utf8(payload) else {
                continue;
            };
            if socket_sender
                .send(Message::Text(text.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    while let Some(Ok(message)) = socket_receiver.next().await {
        let bytes = match message {
            Message::Text(text) => text.as_bytes().to_vec(),
            Message::Binary(bytes) => bytes.to_vec(),
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) => continue,
        };
        let decoded = match decode_client(&bytes) {
            Ok(message) => message,
            Err(error) => {
                let _ = sender
                    .send(ServerMessage::Error {
                        request_id: None,
                        code: ErrorCode::InvalidMessage,
                        message: error.to_string(),
                    })
                    .await;
                continue;
            }
        };
        if handle_message(session_id, decoded, &state, &sender)
            .await
            .is_err()
        {
            break;
        }
    }

    let peer = state.sessions.lock().await.remove(&session_id);
    if let Some(peer) = peer
        && let (Some(room_id), Some(client_id)) = (peer.room_id, peer.client_id)
    {
        let mut manager = state.manager.lock().await;
        let _ = manager.leave(room_id, client_id);
        drop(manager);
        broadcast_room(
            &state,
            room_id,
            session_id,
            ServerMessage::UserLeft { room_id, client_id },
        )
        .await;
    }
    drop(sender);
    let _ = writer.await;
}

#[allow(clippy::too_many_lines)]
async fn handle_message(
    session_id: SessionId,
    message: ClientMessage,
    state: &ServerState,
    sender: &mpsc::Sender<ServerMessage>,
) -> Result<(), ()> {
    match message {
        ClientMessage::Hello {
            client_id,
            client_name: _,
        } => {
            if client_id.is_nil() {
                send_error(
                    sender,
                    None,
                    ErrorCode::InvalidMessage,
                    "client id cannot be nil",
                )
                .await;
                return Ok(());
            }
            update_peer(state, session_id, |peer| peer.client_id = Some(client_id)).await;
            let _ = sender
                .send(ServerMessage::Welcome {
                    session_id,
                    server_version: state.server_version.clone(),
                })
                .await;
        }
        ClientMessage::CreateRoom { request_id } => {
            if peer_client(state, session_id).await.is_none() {
                send_error(
                    sender,
                    Some(request_id),
                    ErrorCode::Unauthorized,
                    "send hello first",
                )
                .await;
                return Ok(());
            }
            let mut manager = state.manager.lock().await;
            let result = manager.create_room();
            drop(manager);
            match result {
                Ok(created) => {
                    let _ = sender
                        .send(ServerMessage::RoomCreated {
                            request_id,
                            room_id: created.room_id,
                            capability_token: created.token.secret().to_owned(),
                        })
                        .await;
                }
                Err(error) => send_room_error(sender, Some(request_id), error).await,
            }
        }
        ClientMessage::JoinRoom {
            room_id,
            capability_token,
            known_version,
        } => {
            let Some(client_id) = peer_client(state, session_id).await else {
                send_error(sender, None, ErrorCode::Unauthorized, "send hello first").await;
                return Ok(());
            };
            let token = crate::auth::CapabilityToken::from_secret(capability_token);
            let mut manager = state.manager.lock().await;
            let result = manager
                .join(room_id, &token, client_id)
                .and_then(|()| manager.sync(room_id, &known_version));
            drop(manager);
            match result {
                Ok(sync) => {
                    let version = sync.snapshot.version_vector.clone();
                    update_peer(state, session_id, |peer| peer.room_id = Some(room_id)).await;
                    let _ = sender
                        .send(ServerMessage::Snapshot {
                            room_id,
                            snapshot: sync.snapshot,
                        })
                        .await;
                    if !sync.operations.is_empty() {
                        let _ = sender
                            .send(ServerMessage::Operations {
                                room_id,
                                operations: sync.operations,
                            })
                            .await;
                    }
                    let _ = sender
                        .send(ServerMessage::SyncComplete { room_id, version })
                        .await;
                    broadcast_room(
                        state,
                        room_id,
                        session_id,
                        ServerMessage::UserJoined { room_id, client_id },
                    )
                    .await;
                }
                Err(error) => send_room_error(sender, None, error).await,
            }
        }
        ClientMessage::SubmitOperations {
            room_id,
            request_id,
            operations,
        } => {
            let Some(client_id) = peer_client(state, session_id).await else {
                send_error(
                    sender,
                    Some(request_id),
                    ErrorCode::Unauthorized,
                    "send hello first",
                )
                .await;
                return Ok(());
            };
            if peer_room(state, session_id).await != Some(room_id) {
                send_error(
                    sender,
                    Some(request_id),
                    ErrorCode::NotInRoom,
                    "join the room first",
                )
                .await;
                return Ok(());
            }
            let mut manager = state.manager.lock().await;
            let result = manager.submit(room_id, client_id, &operations);
            drop(manager);
            match result {
                Ok(outcome) => {
                    let _ = sender
                        .send(ServerMessage::Ack {
                            room_id,
                            request_id,
                            accepted: outcome.acknowledged,
                        })
                        .await;
                    if !outcome.applied.is_empty() {
                        broadcast_room(
                            state,
                            room_id,
                            session_id,
                            ServerMessage::Operations {
                                room_id,
                                operations: outcome.applied,
                            },
                        )
                        .await;
                    }
                }
                Err(error) => send_room_error(sender, Some(request_id), error).await,
            }
        }
        ClientMessage::RequestSync {
            room_id,
            known_version,
        } => {
            if peer_room(state, session_id).await != Some(room_id) {
                send_error(sender, None, ErrorCode::NotInRoom, "join the room first").await;
                return Ok(());
            }
            let mut manager = state.manager.lock().await;
            let result = manager.sync(room_id, &known_version);
            drop(manager);
            match result {
                Ok(sync) => {
                    let version = sync.snapshot.version_vector.clone();
                    let _ = sender
                        .send(ServerMessage::Snapshot {
                            room_id,
                            snapshot: sync.snapshot,
                        })
                        .await;
                    if !sync.operations.is_empty() {
                        let _ = sender
                            .send(ServerMessage::Operations {
                                room_id,
                                operations: sync.operations,
                            })
                            .await;
                    }
                    let _ = sender
                        .send(ServerMessage::SyncComplete { room_id, version })
                        .await;
                }
                Err(error) => send_room_error(sender, None, error).await,
            }
        }
        ClientMessage::Presence {
            room_id,
            state: presence,
        } => {
            if peer_room(state, session_id).await != Some(room_id) {
                return Ok(());
            }
            if peer_client(state, session_id).await != Some(presence.client_id) {
                send_error(
                    sender,
                    None,
                    ErrorCode::Unauthorized,
                    "presence client id does not match the session",
                )
                .await;
                return Ok(());
            }
            let mut manager = state.manager.lock().await;
            if manager.update_presence(room_id, presence.clone()).is_ok() {
                drop(manager);
                broadcast_room(
                    state,
                    room_id,
                    session_id,
                    ServerMessage::Presence {
                        room_id,
                        state: presence,
                    },
                )
                .await;
            }
        }
        ClientMessage::Ping { nonce } => {
            let _ = sender.send(ServerMessage::Pong { nonce }).await;
        }
        ClientMessage::LeaveRoom { room_id } => {
            if let Some(client_id) = peer_client(state, session_id).await {
                let mut manager = state.manager.lock().await;
                let _ = manager.leave(room_id, client_id);
                drop(manager);
                update_peer(state, session_id, |peer| peer.room_id = None).await;
                broadcast_room(
                    state,
                    room_id,
                    session_id,
                    ServerMessage::UserLeft { room_id, client_id },
                )
                .await;
            }
        }
        ClientMessage::StrokeStart {
            room_id,
            stroke_id,
            start,
        } => {
            if peer_room(state, session_id).await != Some(room_id) {
                send_error(sender, None, ErrorCode::NotInRoom, "join the room first").await;
                return Ok(());
            }
            broadcast_room(
                state,
                room_id,
                session_id,
                ServerMessage::StrokeStart {
                    room_id,
                    stroke_id,
                    start,
                },
            )
            .await;
        }
        ClientMessage::StrokeChunk {
            room_id,
            stroke_id,
            points,
        } => {
            if peer_room(state, session_id).await != Some(room_id) {
                send_error(sender, None, ErrorCode::NotInRoom, "join the room first").await;
                return Ok(());
            }
            broadcast_room(
                state,
                room_id,
                session_id,
                ServerMessage::StrokeChunk {
                    room_id,
                    stroke_id,
                    points,
                },
            )
            .await;
        }
        ClientMessage::StrokeEnd { room_id, stroke_id } => {
            if peer_room(state, session_id).await != Some(room_id) {
                send_error(sender, None, ErrorCode::NotInRoom, "join the room first").await;
                return Ok(());
            }
            broadcast_room(
                state,
                room_id,
                session_id,
                ServerMessage::StrokeEnd { room_id, stroke_id },
            )
            .await;
        }
    }
    Ok(())
}

async fn peer_client(state: &ServerState, session_id: SessionId) -> Option<ClientId> {
    state
        .sessions
        .lock()
        .await
        .get(&session_id)
        .and_then(|peer| peer.client_id)
}

async fn peer_room(state: &ServerState, session_id: SessionId) -> Option<RoomId> {
    state
        .sessions
        .lock()
        .await
        .get(&session_id)
        .and_then(|peer| peer.room_id)
}

async fn update_peer<F>(state: &ServerState, session_id: SessionId, update: F)
where
    F: FnOnce(&mut SessionPeer),
{
    if let Some(peer) = state.sessions.lock().await.get_mut(&session_id) {
        update(peer);
    }
}

async fn broadcast_room(
    state: &ServerState,
    room_id: RoomId,
    except: SessionId,
    message: ServerMessage,
) {
    let senders = state
        .sessions
        .lock()
        .await
        .iter()
        .filter(|(session_id, peer)| **session_id != except && peer.room_id == Some(room_id))
        .map(|(_, peer)| peer.sender.clone())
        .collect::<Vec<_>>();
    for sender in senders {
        let _ = sender.send(message.clone()).await;
    }
}

async fn send_error(
    sender: &mpsc::Sender<ServerMessage>,
    request_id: Option<u64>,
    code: ErrorCode,
    message: &str,
) {
    let _ = sender
        .send(ServerMessage::Error {
            request_id,
            code,
            message: message.to_owned(),
        })
        .await;
}

async fn send_room_error(
    sender: &mpsc::Sender<ServerMessage>,
    request_id: Option<u64>,
    error: RoomError,
) {
    let code = match error {
        RoomError::Unauthorized => ErrorCode::Unauthorized,
        RoomError::NotInRoom => ErrorCode::NotInRoom,
        RoomError::RoomNotFound => ErrorCode::RoomNotFound,
        _ => ErrorCode::Internal,
    };
    send_error(sender, request_id, code, &error.to_string()).await;
}

/// A small response type used by health checks that need explicit status.
#[allow(dead_code)]
fn _not_found_response() -> Response {
    StatusCode::NOT_FOUND.into_response()
}
