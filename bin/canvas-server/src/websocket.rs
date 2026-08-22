//! Axum WebSocket session and room broadcast handling.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

use crate::{
    auth::CapabilityToken,
    room::{CreatedRoom, RoomError, RoomManager},
};

const CREATE_REQUEST_CACHE_CAPACITY: usize = 1024;
const CREATE_REQUEST_RETENTION: Duration = Duration::from_mins(10);

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
    client_name: Option<String>,
    room_id: Option<RoomId>,
    sender: mpsc::Sender<ServerMessage>,
    outbound: Arc<AsyncMutex<()>>,
}

struct CreateRequestRecord {
    request_id: u64,
    created: CreatedRoom,
    stored_at: Instant,
}

/// Shared state for HTTP health and WebSocket sessions.
#[derive(Clone)]
pub struct ServerState {
    manager: Arc<AsyncMutex<RoomManager>>,
    sessions: Arc<AsyncMutex<BTreeMap<SessionId, SessionPeer>>>,
    create_requests: Arc<AsyncMutex<BTreeMap<ClientId, CreateRequestRecord>>>,
    server_version: String,
}

impl ServerState {
    /// Creates transport state around a room manager.
    #[must_use]
    pub fn new(manager: RoomManager) -> Self {
        Self {
            manager: Arc::new(AsyncMutex::new(manager)),
            sessions: Arc::new(AsyncMutex::new(BTreeMap::new())),
            create_requests: Arc::new(AsyncMutex::new(BTreeMap::new())),
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
        let endpoint = readiness_endpoint(listener.local_addr()?);
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
            client_name: None,
            room_id: None,
            sender: sender.clone(),
            outbound: Arc::new(AsyncMutex::new(())),
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
                send_to_session(
                    &state,
                    session_id,
                    ServerMessage::Error {
                        request_id: None,
                        code: ErrorCode::InvalidMessage,
                        message: error.to_string(),
                    },
                )
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
    let Some(outbound) = peer_outbound(state, session_id).await else {
        return Err(());
    };

    match message {
        ClientMessage::Hello {
            client_id,
            client_name,
        } => {
            if client_id.is_nil() {
                send_error(
                    &outbound,
                    sender,
                    None,
                    ErrorCode::InvalidMessage,
                    "client id cannot be nil",
                )
                .await;
                return Ok(());
            }
            if peer_client(state, session_id)
                .await
                .is_some_and(|bound_client_id| bound_client_id != client_id)
            {
                send_error(
                    &outbound,
                    sender,
                    None,
                    ErrorCode::Unauthorized,
                    "session identity is already bound",
                )
                .await;
                return Ok(());
            }
            if client_is_bound_elsewhere(state, session_id, client_id).await {
                send_error(
                    &outbound,
                    sender,
                    None,
                    ErrorCode::Unauthorized,
                    "client identity is already connected",
                )
                .await;
                return Ok(());
            }
            let client_name = client_name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| String::from("Sketchi"));
            update_peer(state, session_id, |peer| {
                peer.client_id = Some(client_id);
                peer.client_name = Some(client_name);
            })
            .await;
            send_direct(
                &outbound,
                sender,
                ServerMessage::Welcome {
                    session_id,
                    server_version: state.server_version.clone(),
                },
            )
            .await;
        }
        ClientMessage::CreateRoom { request_id } => {
            let Some(client_id) = peer_client(state, session_id).await else {
                send_error(
                    &outbound,
                    sender,
                    Some(request_id),
                    ErrorCode::Unauthorized,
                    "send hello first",
                )
                .await;
                return Ok(());
            };
            let result = create_room_for_request(state, client_id, request_id).await;
            match result {
                Ok(created) => {
                    send_direct(
                        &outbound,
                        sender,
                        ServerMessage::RoomCreated {
                            request_id,
                            room_id: created.room_id,
                            capability_token: created.token.secret().to_owned(),
                            creator_token: created.creator_token.secret().to_owned(),
                            expires_at_epoch: Some(created.expires_at_epoch),
                        },
                    )
                    .await;
                }
                Err(error) => send_room_error(&outbound, sender, Some(request_id), error).await,
            }
        }
        ClientMessage::JoinRoom {
            room_id,
            capability_token,
            known_version,
        } => {
            let Some((client_id, previous_room)) = peer_snapshot(state, session_id).await else {
                send_error(
                    &outbound,
                    sender,
                    None,
                    ErrorCode::Unauthorized,
                    "send hello first",
                )
                .await;
                return Ok(());
            };
            let previous_name = peer_name(state, session_id).await;
            let client_name = previous_name
                .clone()
                .unwrap_or_else(|| String::from("Sketchi"));
            let switching_rooms = previous_room.is_some() && previous_room != Some(room_id);
            let token = crate::auth::CapabilityToken::from_secret(capability_token);
            let result = {
                let _join_guard = outbound.lock().await;
                let result = {
                    let mut manager = state.manager.lock().await;
                    let result = manager
                        .join_named(room_id, &token, client_id, client_name.clone())
                        .and_then(|()| manager.sync(room_id, &known_version));
                    match (result, previous_room) {
                        (Ok(sync), Some(previous_room)) if previous_room != room_id => {
                            manager.leave(previous_room, client_id).map(|()| sync)
                        }
                        (result, _) => result,
                    }
                };
                if result.is_ok() {
                    update_peer(state, session_id, |peer| peer.room_id = None).await;
                }
                result
            };
            match result {
                Ok(sync) => {
                    let version = sync.version;
                    let presence = sync.presence;
                    let participants = sync.participants;
                    if switching_rooms && let Some(previous_room) = previous_room {
                        broadcast_room(
                            state,
                            previous_room,
                            session_id,
                            ServerMessage::UserLeft {
                                room_id: previous_room,
                                client_id,
                            },
                        )
                        .await;
                    }
                    {
                        let _sync_guard = outbound.lock().await;
                        send_sync_frames_locked(
                            sender,
                            room_id,
                            sync.snapshot,
                            sync.operations,
                            presence,
                            participants,
                            version,
                        )
                        .await;
                    }
                    update_peer(state, session_id, |peer| peer.room_id = Some(room_id)).await;
                    if previous_room != Some(room_id)
                        || previous_name.as_deref() != Some(client_name.as_str())
                    {
                        broadcast_room(
                            state,
                            room_id,
                            session_id,
                            ServerMessage::UserJoined {
                                room_id,
                                client_id,
                                name: client_name,
                            },
                        )
                        .await;
                    }
                }
                Err(error) => send_room_error(&outbound, sender, None, error).await,
            }
        }
        ClientMessage::SubmitOperations {
            room_id,
            request_id,
            operations,
        } => {
            let Some(client_id) = peer_client(state, session_id).await else {
                send_error(
                    &outbound,
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
                    &outbound,
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
                    send_direct(
                        &outbound,
                        sender,
                        ServerMessage::Ack {
                            room_id,
                            request_id,
                            accepted: outcome.acknowledged,
                        },
                    )
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
                Err(error) => send_room_error(&outbound, sender, Some(request_id), error).await,
            }
        }
        ClientMessage::RequestSync {
            room_id,
            known_version,
        } => {
            if peer_room(state, session_id).await != Some(room_id) {
                send_error(
                    &outbound,
                    sender,
                    None,
                    ErrorCode::NotInRoom,
                    "join the room first",
                )
                .await;
                return Ok(());
            }
            let result = {
                let _sync_guard = outbound.lock().await;
                let result = {
                    let mut manager = state.manager.lock().await;
                    manager.sync(room_id, &known_version)
                };
                match result {
                    Ok(sync) => {
                        let version = sync.version;
                        let presence = sync.presence;
                        let participants = sync.participants;
                        send_sync_frames_locked(
                            sender,
                            room_id,
                            sync.snapshot,
                            sync.operations,
                            presence,
                            participants,
                            version,
                        )
                        .await;
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            };
            if let Err(error) = result {
                send_room_error(&outbound, sender, None, error).await;
            }
        }
        ClientMessage::Presence {
            room_id,
            state: presence,
        } => {
            if peer_room(state, session_id).await != Some(room_id) {
                send_error(
                    &outbound,
                    sender,
                    None,
                    ErrorCode::NotInRoom,
                    "join the room first",
                )
                .await;
                return Ok(());
            }
            if peer_client(state, session_id).await != Some(presence.client_id) {
                send_error(
                    &outbound,
                    sender,
                    None,
                    ErrorCode::Unauthorized,
                    "presence client id does not match the session",
                )
                .await;
                return Ok(());
            }
            let mut manager = state.manager.lock().await;
            let result = manager.update_presence(room_id, presence.clone());
            drop(manager);
            match result {
                Ok(()) => {
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
                Err(error) => send_room_error(&outbound, sender, None, error).await,
            }
        }
        ClientMessage::Ping { nonce } => {
            send_direct(&outbound, sender, ServerMessage::Pong { nonce }).await;
        }
        ClientMessage::LeaveRoom { room_id } => {
            let Some((client_id, current_room)) = peer_snapshot(state, session_id).await else {
                send_error(
                    &outbound,
                    sender,
                    None,
                    ErrorCode::Unauthorized,
                    "send hello first",
                )
                .await;
                return Ok(());
            };
            if current_room != Some(room_id) {
                send_error(
                    &outbound,
                    sender,
                    None,
                    ErrorCode::NotInRoom,
                    "join the room first",
                )
                .await;
                return Ok(());
            }
            let mut manager = state.manager.lock().await;
            let result = manager.leave(room_id, client_id);
            drop(manager);
            match result {
                Ok(()) => {
                    update_peer(state, session_id, |peer| peer.room_id = None).await;
                    broadcast_room(
                        state,
                        room_id,
                        session_id,
                        ServerMessage::UserLeft { room_id, client_id },
                    )
                    .await;
                }
                Err(error) => send_room_error(&outbound, sender, None, error).await,
            }
        }
        ClientMessage::CancelRoom {
            room_id,
            creator_token,
        } => {
            let Some(client_id) = peer_client(state, session_id).await else {
                send_error(
                    &outbound,
                    sender,
                    None,
                    ErrorCode::Unauthorized,
                    "send hello first",
                )
                .await;
                return Ok(());
            };
            let result = state
                .manager
                .lock()
                .await
                .cancel(room_id, &CapabilityToken::from_secret(creator_token));
            match result {
                Ok(_) => {
                    state.create_requests.lock().await.remove(&client_id);
                    cancel_room_sessions(state, room_id, session_id).await;
                }
                Err(error) => send_room_error(&outbound, sender, None, error).await,
            }
        }
        ClientMessage::StrokeStart {
            room_id,
            stroke_id,
            start,
        } => {
            if peer_room(state, session_id).await != Some(room_id) {
                send_error(
                    &outbound,
                    sender,
                    None,
                    ErrorCode::NotInRoom,
                    "join the room first",
                )
                .await;
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
                send_error(
                    &outbound,
                    sender,
                    None,
                    ErrorCode::NotInRoom,
                    "join the room first",
                )
                .await;
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
                send_error(
                    &outbound,
                    sender,
                    None,
                    ErrorCode::NotInRoom,
                    "join the room first",
                )
                .await;
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

async fn create_room_for_request(
    state: &ServerState,
    client_id: ClientId,
    request_id: u64,
) -> Result<CreatedRoom, RoomError> {
    let mut requests = state.create_requests.lock().await;
    let now = Instant::now();
    prune_create_requests(&mut requests, now);
    if let Some(record) = requests.get(&client_id)
        && record.request_id == request_id
    {
        return Ok(record.created.clone());
    }

    let created = state.manager.lock().await.create_room_for(client_id)?;
    while requests.len() >= CREATE_REQUEST_CACHE_CAPACITY {
        let Some(oldest_client) = requests
            .iter()
            .min_by_key(|(_, record)| record.stored_at)
            .map(|(client_id, _)| *client_id)
        else {
            break;
        };
        requests.remove(&oldest_client);
    }
    requests.insert(
        client_id,
        CreateRequestRecord {
            request_id,
            created: created.clone(),
            stored_at: now,
        },
    );
    Ok(created)
}

fn prune_create_requests(requests: &mut BTreeMap<ClientId, CreateRequestRecord>, now: Instant) {
    requests.retain(|_, record| {
        now.saturating_duration_since(record.stored_at) < CREATE_REQUEST_RETENTION
    });
}

async fn peer_client(state: &ServerState, session_id: SessionId) -> Option<ClientId> {
    state
        .sessions
        .lock()
        .await
        .get(&session_id)
        .and_then(|peer| peer.client_id)
}

async fn peer_name(state: &ServerState, session_id: SessionId) -> Option<String> {
    state
        .sessions
        .lock()
        .await
        .get(&session_id)
        .and_then(|peer| peer.client_name.clone())
}

async fn client_is_bound_elsewhere(
    state: &ServerState,
    session_id: SessionId,
    client_id: ClientId,
) -> bool {
    let sessions = state.sessions.lock().await;
    sessions.iter().any(|(other_session_id, peer)| {
        *other_session_id != session_id && peer.client_id == Some(client_id)
    })
}

async fn peer_snapshot(
    state: &ServerState,
    session_id: SessionId,
) -> Option<(ClientId, Option<RoomId>)> {
    state
        .sessions
        .lock()
        .await
        .get(&session_id)
        .and_then(|peer| peer.client_id.map(|client_id| (client_id, peer.room_id)))
}

async fn peer_room(state: &ServerState, session_id: SessionId) -> Option<RoomId> {
    state
        .sessions
        .lock()
        .await
        .get(&session_id)
        .and_then(|peer| peer.room_id)
}

async fn peer_outbound(state: &ServerState, session_id: SessionId) -> Option<Arc<AsyncMutex<()>>> {
    state
        .sessions
        .lock()
        .await
        .get(&session_id)
        .map(|peer| Arc::clone(&peer.outbound))
}

async fn update_peer<F>(state: &ServerState, session_id: SessionId, update: F)
where
    F: FnOnce(&mut SessionPeer),
{
    if let Some(peer) = state.sessions.lock().await.get_mut(&session_id) {
        update(peer);
    }
}

async fn cancel_room_sessions(state: &ServerState, room_id: RoomId, creator_session_id: SessionId) {
    let session_ids = {
        let mut sessions = state.sessions.lock().await;
        sessions
            .iter_mut()
            .filter_map(|(session_id, peer)| {
                (peer.room_id == Some(room_id) || *session_id == creator_session_id).then(|| {
                    peer.room_id = None;
                    *session_id
                })
            })
            .collect::<Vec<_>>()
    };
    for session_id in session_ids {
        send_to_session(state, session_id, ServerMessage::RoomCancelled { room_id }).await;
    }
}

fn readiness_endpoint(bound: SocketAddr) -> String {
    readiness_endpoint_for_host(bound, detect_advertised_host(bound.ip()))
}

fn readiness_endpoint_for_host(bound: SocketAddr, detected_host: Option<IpAddr>) -> String {
    let host = advertised_host(bound.ip(), detected_host);
    format!("wss://{}/ws", SocketAddr::new(host, bound.port()))
}

fn advertised_host(bind_ip: IpAddr, detected_host: Option<IpAddr>) -> IpAddr {
    if !bind_ip.is_unspecified() {
        return bind_ip;
    }
    detected_host
        .filter(|host| is_usable_advertised_host(*host))
        .unwrap_or(match bind_ip {
            IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::LOCALHOST),
        })
}

fn detect_advertised_host(bind_ip: IpAddr) -> Option<IpAddr> {
    let (local_bind, route_probe) = match bind_ip {
        IpAddr::V4(_) => ("0.0.0.0:0", "8.8.8.8:80"),
        IpAddr::V6(_) => ("[::]:0", "[2001:4860:4860::8888]:80"),
    };
    let socket = UdpSocket::bind(local_bind).ok()?;
    // UDP connect selects the local route without sending a packet.
    socket.connect(route_probe).ok()?;
    let host = socket.local_addr().ok()?.ip();
    is_usable_advertised_host(host).then_some(host)
}

fn is_usable_advertised_host(host: IpAddr) -> bool {
    match host {
        IpAddr::V4(host) => !host.is_unspecified() && !host.is_loopback() && !host.is_multicast(),
        IpAddr::V6(host) => !host.is_unspecified() && !host.is_loopback() && !host.is_multicast(),
    }
}

async fn broadcast_room(
    state: &ServerState,
    room_id: RoomId,
    except: SessionId,
    message: ServerMessage,
) {
    let session_ids = {
        let sessions = state.sessions.lock().await;
        sessions
            .iter()
            .filter(|(session_id, peer)| **session_id != except && peer.room_id == Some(room_id))
            .map(|(session_id, _)| *session_id)
            .collect::<Vec<_>>()
    };
    for session_id in session_ids {
        let Some(outbound) = peer_outbound(state, session_id).await else {
            continue;
        };
        let _outbound_guard = outbound.lock().await;
        let sender = {
            let sessions = state.sessions.lock().await;
            sessions
                .get(&session_id)
                .and_then(|peer| (peer.room_id == Some(room_id)).then(|| peer.sender.clone()))
        };
        if let Some(sender) = sender {
            let _ = sender.send(message.clone()).await;
        }
    }
}

async fn send_to_session(state: &ServerState, session_id: SessionId, message: ServerMessage) {
    let Some(outbound) = peer_outbound(state, session_id).await else {
        return;
    };
    let _outbound_guard = outbound.lock().await;
    let sender = state
        .sessions
        .lock()
        .await
        .get(&session_id)
        .map(|peer| peer.sender.clone());
    if let Some(sender) = sender {
        let _ = sender.send(message).await;
    }
}

async fn send_direct(
    outbound: &Arc<AsyncMutex<()>>,
    sender: &mpsc::Sender<ServerMessage>,
    message: ServerMessage,
) {
    let _outbound_guard = outbound.lock().await;
    let _ = sender.send(message).await;
}

async fn send_sync_frames_locked(
    sender: &mpsc::Sender<ServerMessage>,
    room_id: RoomId,
    snapshot: canvas_core::CrdtSnapshot,
    operations: Vec<canvas_core::Operation>,
    presence: Vec<canvas_protocol::PresenceState>,
    participants: Vec<canvas_protocol::Participant>,
    version: canvas_core::VersionVector,
) {
    let _ = sender
        .send(ServerMessage::Snapshot { room_id, snapshot })
        .await;
    if !operations.is_empty() {
        let _ = sender
            .send(ServerMessage::Operations {
                room_id,
                operations,
            })
            .await;
    }
    for state in presence {
        let _ = sender
            .send(ServerMessage::Presence { room_id, state })
            .await;
    }
    let _ = sender
        .send(ServerMessage::SyncComplete { room_id, version })
        .await;
    let _ = sender
        .send(ServerMessage::Participants {
            room_id,
            participants,
        })
        .await;
}

async fn send_error(
    outbound: &Arc<AsyncMutex<()>>,
    sender: &mpsc::Sender<ServerMessage>,
    request_id: Option<u64>,
    code: ErrorCode,
    message: &str,
) {
    send_direct(
        outbound,
        sender,
        ServerMessage::Error {
            request_id,
            code,
            message: message.to_owned(),
        },
    )
    .await;
}

async fn send_room_error(
    outbound: &Arc<AsyncMutex<()>>,
    sender: &mpsc::Sender<ServerMessage>,
    request_id: Option<u64>,
    error: RoomError,
) {
    let code = match error {
        RoomError::Unauthorized => ErrorCode::Unauthorized,
        RoomError::TokenExpired => ErrorCode::TokenExpired,
        RoomError::NotInRoom => ErrorCode::NotInRoom,
        RoomError::RoomNotFound => ErrorCode::RoomNotFound,
        RoomError::RoomFull => ErrorCode::RoomFull,
        _ => ErrorCode::Internal,
    };
    send_error(outbound, sender, request_id, code, &error.to_string()).await;
}

/// A small response type used by health checks that need explicit status.
#[allow(dead_code)]
fn _not_found_response() -> Response {
    StatusCode::NOT_FOUND.into_response()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use canvas_core::{
        Element, ElementId, LamportTimestamp, Operation, OperationId, OperationKind, Point, Size,
        Transform, VersionVector,
    };
    use canvas_protocol::{PresenceState, ToolKind};
    use std::net::{IpAddr, Ipv4Addr};

    async fn add_peer(
        state: &ServerState,
        session_id: SessionId,
        client_id: Option<ClientId>,
        room_id: Option<RoomId>,
    ) -> mpsc::Receiver<ServerMessage> {
        let (sender, receiver) = mpsc::channel(8);
        state.sessions.lock().await.insert(
            session_id,
            SessionPeer {
                client_id,
                client_name: None,
                room_id,
                sender,
                outbound: Arc::new(AsyncMutex::new(())),
            },
        );
        receiver
    }

    async fn next_message(receiver: &mut mpsc::Receiver<ServerMessage>) -> ServerMessage {
        receiver
            .recv()
            .await
            .expect("test peer should receive a server message")
    }

    #[tokio::test]
    async fn room_full_maps_to_the_bounded_protocol_error_code() {
        let (sender, mut receiver) = mpsc::channel(2);
        let outbound = Arc::new(AsyncMutex::new(()));
        send_room_error(&outbound, &sender, None, RoomError::RoomFull).await;
        assert!(matches!(
            next_message(&mut receiver).await,
            ServerMessage::Error {
                code: ErrorCode::RoomFull,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn create_room_retry_returns_the_original_room_for_the_same_request() {
        let store = std::sync::Arc::new(std::sync::Mutex::new(
            crate::store::RoomStore::open_in_memory().expect("in-memory store"),
        ));
        let state = ServerState::new(RoomManager::new(store));
        let session_id = SessionId::new();
        let client_id = ClientId::from_u128(42);
        let mut receiver = add_peer(&state, session_id, Some(client_id), None).await;
        let sender = state
            .sessions
            .lock()
            .await
            .get(&session_id)
            .expect("peer was inserted")
            .sender
            .clone();

        handle_message(
            session_id,
            ClientMessage::CreateRoom { request_id: 9 },
            &state,
            &sender,
        )
        .await
        .expect("create should be handled");

        let second_session_id = SessionId::new();
        let mut second_receiver = add_peer(&state, second_session_id, Some(client_id), None).await;
        let second_sender = state
            .sessions
            .lock()
            .await
            .get(&second_session_id)
            .expect("second peer was inserted")
            .sender
            .clone();
        handle_message(
            second_session_id,
            ClientMessage::CreateRoom { request_id: 9 },
            &state,
            &second_sender,
        )
        .await
        .expect("retry should be handled");

        let first = next_message(&mut receiver).await;
        let second = next_message(&mut second_receiver).await;
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn cancelling_a_room_clears_create_history_for_the_next_room() {
        let store = std::sync::Arc::new(std::sync::Mutex::new(
            crate::store::RoomStore::open_in_memory().expect("in-memory store"),
        ));
        let state = ServerState::new(RoomManager::new(store));
        let session_id = SessionId::new();
        let client_id = ClientId::from_u128(44);
        let mut receiver = add_peer(&state, session_id, Some(client_id), None).await;
        let sender = state
            .sessions
            .lock()
            .await
            .get(&session_id)
            .expect("peer was inserted")
            .sender
            .clone();

        handle_message(
            session_id,
            ClientMessage::CreateRoom { request_id: 1 },
            &state,
            &sender,
        )
        .await
        .expect("create should be handled");
        let first_message = next_message(&mut receiver).await;
        assert!(matches!(first_message, ServerMessage::RoomCreated { .. }));
        let ServerMessage::RoomCreated {
            room_id: first_room_id,
            creator_token: first_creator_token,
            ..
        } = first_message
        else {
            return;
        };
        handle_message(
            session_id,
            ClientMessage::CancelRoom {
                room_id: first_room_id,
                creator_token: first_creator_token,
            },
            &state,
            &sender,
        )
        .await
        .expect("cancel should be handled");
        let first_cancel_message = next_message(&mut receiver).await;
        assert!(
            matches!(
                first_cancel_message,
                ServerMessage::RoomCancelled { room_id } if room_id == first_room_id
            ),
            "received {first_cancel_message:?}"
        );

        handle_message(
            session_id,
            ClientMessage::CreateRoom { request_id: 1 },
            &state,
            &sender,
        )
        .await
        .expect("second create should be handled");
        let second_message = next_message(&mut receiver).await;
        assert!(matches!(second_message, ServerMessage::RoomCreated { .. }));
        let ServerMessage::RoomCreated {
            room_id: second_room_id,
            creator_token: second_creator_token,
            ..
        } = second_message
        else {
            return;
        };
        assert_ne!(first_room_id, second_room_id);
        handle_message(
            session_id,
            ClientMessage::CancelRoom {
                room_id: second_room_id,
                creator_token: second_creator_token,
            },
            &state,
            &sender,
        )
        .await
        .expect("second cancel should be handled");
        assert!(matches!(
            next_message(&mut receiver).await,
            ServerMessage::RoomCancelled { room_id } if room_id == second_room_id
        ));
    }

    #[tokio::test]
    async fn expired_create_request_entries_are_pruned() {
        let store = std::sync::Arc::new(std::sync::Mutex::new(
            crate::store::RoomStore::open_in_memory().expect("in-memory store"),
        ));
        let state = ServerState::new(RoomManager::new(store));
        let client_id = ClientId::from_u128(43);
        let created = state.manager.lock().await.create_room().expect("room");
        state.create_requests.lock().await.insert(
            client_id,
            CreateRequestRecord {
                request_id: 1,
                created,
                stored_at: Instant::now()
                    .checked_sub(CREATE_REQUEST_RETENTION + Duration::from_secs(1))
                    .expect("test instant"),
            },
        );

        let replacement = create_room_for_request(&state, client_id, 2)
            .await
            .expect("replacement room");
        let requests = state.create_requests.lock().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests.get(&client_id).map(|record| record.request_id),
            Some(2)
        );
        assert_eq!(
            requests.get(&client_id).map(|record| &record.created),
            Some(&replacement)
        );
    }

    #[test]
    fn readiness_endpoint_uses_the_bound_host_for_explicit_addresses() {
        let bound = SocketAddr::from(([192, 168, 1, 42], 4321));

        assert_eq!(readiness_endpoint(bound), "wss://192.168.1.42:4321/ws");
    }

    #[test]
    fn readiness_endpoint_uses_the_detected_lan_host_for_a_wildcard_bind() {
        let bind_ip = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
        let detected_host = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 42));

        assert_eq!(advertised_host(bind_ip, Some(detected_host)), detected_host);
    }

    #[test]
    fn readiness_endpoint_falls_back_to_loopback_without_a_detected_host() {
        let bound = SocketAddr::from(([0, 0, 0, 0], 4321));

        assert_eq!(
            readiness_endpoint_for_host(bound, None),
            "wss://127.0.0.1:4321/ws"
        );
    }

    #[tokio::test]
    async fn hello_cannot_rebind_a_session_to_another_client() {
        let store = std::sync::Arc::new(std::sync::Mutex::new(
            crate::store::RoomStore::open_in_memory().expect("in-memory store"),
        ));
        let state = ServerState::new(RoomManager::new(store));
        let session_id = SessionId::new();
        let mut receiver = add_peer(&state, session_id, None, None).await;
        let sender = state
            .sessions
            .lock()
            .await
            .get(&session_id)
            .expect("peer was inserted")
            .sender
            .clone();

        handle_message(
            session_id,
            ClientMessage::Hello {
                client_id: ClientId::from_u128(1),
                client_name: None,
            },
            &state,
            &sender,
        )
        .await
        .expect("hello should be handled");
        assert!(matches!(
            next_message(&mut receiver).await,
            ServerMessage::Welcome { .. }
        ));

        handle_message(
            session_id,
            ClientMessage::Hello {
                client_id: ClientId::from_u128(2),
                client_name: None,
            },
            &state,
            &sender,
        )
        .await
        .expect("rebind should be reported as a protocol error");
        assert!(matches!(
            next_message(&mut receiver).await,
            ServerMessage::Error {
                code: ErrorCode::Unauthorized,
                ..
            }
        ));
        assert_eq!(
            peer_client(&state, session_id).await,
            Some(ClientId::from_u128(1))
        );
    }

    #[tokio::test]
    async fn switching_rooms_removes_membership_from_the_previous_room() {
        let store = std::sync::Arc::new(std::sync::Mutex::new(
            crate::store::RoomStore::open_in_memory().expect("in-memory store"),
        ));
        let state = ServerState::new(RoomManager::new(store));
        let (first_room, second_room) = {
            let mut manager = state.manager.lock().await;
            (
                manager.create_room().expect("first room"),
                manager.create_room().expect("second room"),
            )
        };
        let client_id = ClientId::from_u128(3);
        {
            let mut manager = state.manager.lock().await;
            manager
                .join(first_room.room_id, &first_room.token, client_id)
                .expect("first join");
        }
        let session_id = SessionId::new();
        let mut receiver = add_peer(
            &state,
            session_id,
            Some(client_id),
            Some(first_room.room_id),
        )
        .await;
        let sender = state
            .sessions
            .lock()
            .await
            .get(&session_id)
            .expect("peer was inserted")
            .sender
            .clone();

        handle_message(
            session_id,
            ClientMessage::JoinRoom {
                room_id: second_room.room_id,
                capability_token: second_room.token.secret().to_owned(),
                known_version: canvas_core::VersionVector::default(),
            },
            &state,
            &sender,
        )
        .await
        .expect("room switch should be handled");
        assert!(matches!(
            next_message(&mut receiver).await,
            ServerMessage::Snapshot { .. }
        ));
        assert!(matches!(
            next_message(&mut receiver).await,
            ServerMessage::SyncComplete { .. }
        ));
        assert_eq!(
            peer_room(&state, session_id).await,
            Some(second_room.room_id)
        );

        let mut manager = state.manager.lock().await;
        assert!(matches!(
            manager.submit(first_room.room_id, client_id, &[]),
            Err(RoomError::NotInRoom)
        ));
        assert!(manager.submit(second_room.room_id, client_id, &[]).is_ok());
    }

    #[tokio::test]
    async fn leave_for_another_room_does_not_clear_the_current_session() {
        let store = std::sync::Arc::new(std::sync::Mutex::new(
            crate::store::RoomStore::open_in_memory().expect("in-memory store"),
        ));
        let state = ServerState::new(RoomManager::new(store));
        let room = {
            let mut manager = state.manager.lock().await;
            manager.create_room().expect("room")
        };
        let other_room = RoomId::from_u128(999);
        let client_id = ClientId::from_u128(4);
        {
            let mut manager = state.manager.lock().await;
            manager
                .join(room.room_id, &room.token, client_id)
                .expect("join");
        }
        let session_id = SessionId::new();
        let mut receiver = add_peer(&state, session_id, Some(client_id), Some(room.room_id)).await;
        let sender = state
            .sessions
            .lock()
            .await
            .get(&session_id)
            .expect("peer was inserted")
            .sender
            .clone();

        handle_message(
            session_id,
            ClientMessage::LeaveRoom {
                room_id: other_room,
            },
            &state,
            &sender,
        )
        .await
        .expect("invalid leave should be handled");
        assert!(matches!(
            next_message(&mut receiver).await,
            ServerMessage::Error {
                code: ErrorCode::NotInRoom,
                ..
            }
        ));
        assert_eq!(peer_room(&state, session_id).await, Some(room.room_id));
        assert!(
            state
                .manager
                .lock()
                .await
                .submit(room.room_id, client_id, &[])
                .is_ok()
        );
    }

    #[tokio::test]
    async fn joining_replays_current_presence_before_sync_complete() {
        let store = std::sync::Arc::new(std::sync::Mutex::new(
            crate::store::RoomStore::open_in_memory().expect("in-memory store"),
        ));
        let state = ServerState::new(RoomManager::new(store));
        let room = {
            let mut manager = state.manager.lock().await;
            manager.create_room().expect("room")
        };
        let existing_client = ClientId::from_u128(5);
        {
            let mut manager = state.manager.lock().await;
            manager
                .join(room.room_id, &room.token, existing_client)
                .expect("existing join");
            manager
                .update_presence(
                    room.room_id,
                    PresenceState {
                        client_id: existing_client,
                        cursor: Some(Point::new(10.0, 20.0)),
                        selected_elements: Vec::new(),
                        active_tool: ToolKind::Select,
                    },
                )
                .expect("presence update");
        }
        let session_id = SessionId::new();
        let mut receiver = add_peer(&state, session_id, Some(ClientId::from_u128(6)), None).await;
        let sender = state
            .sessions
            .lock()
            .await
            .get(&session_id)
            .expect("peer was inserted")
            .sender
            .clone();

        handle_message(
            session_id,
            ClientMessage::JoinRoom {
                room_id: room.room_id,
                capability_token: room.token.secret().to_owned(),
                known_version: canvas_core::VersionVector::default(),
            },
            &state,
            &sender,
        )
        .await
        .expect("join should be handled");
        assert!(matches!(
            next_message(&mut receiver).await,
            ServerMessage::Snapshot { .. }
        ));
        assert!(matches!(
            next_message(&mut receiver).await,
            ServerMessage::Presence {
                state: PresenceState { client_id, .. },
                ..
            } if client_id == existing_client
        ));
        assert!(matches!(
            next_message(&mut receiver).await,
            ServerMessage::SyncComplete { .. }
        ));
    }

    #[tokio::test]
    async fn broadcast_waits_for_direct_sync_frames_on_the_same_session() {
        let store = std::sync::Arc::new(std::sync::Mutex::new(
            crate::store::RoomStore::open_in_memory().expect("in-memory store"),
        ));
        let state = ServerState::new(RoomManager::new(store));
        let room_id = RoomId::from_u128(7);
        let session_id = SessionId::new();
        let mut receiver = add_peer(
            &state,
            session_id,
            Some(ClientId::from_u128(7)),
            Some(room_id),
        )
        .await;
        let sender = state
            .sessions
            .lock()
            .await
            .get(&session_id)
            .expect("peer was inserted")
            .sender
            .clone();
        let outbound = peer_outbound(&state, session_id)
            .await
            .expect("peer has an outbound lock");
        let guard = outbound.lock().await;
        let broadcast = tokio::spawn({
            let state = state.clone();
            async move {
                broadcast_room(
                    &state,
                    room_id,
                    SessionId::new(),
                    ServerMessage::Operations {
                        room_id,
                        operations: Vec::new(),
                    },
                )
                .await;
            }
        });
        tokio::task::yield_now().await;
        assert!(receiver.try_recv().is_err());

        sender
            .send(ServerMessage::Snapshot {
                room_id,
                snapshot: canvas_core::CrdtDocument::new().snapshot(),
            })
            .await
            .expect("snapshot should fit the channel");
        sender
            .send(ServerMessage::SyncComplete {
                room_id,
                version: canvas_core::VersionVector::default(),
            })
            .await
            .expect("sync completion should fit the channel");
        drop(guard);
        broadcast.await.expect("broadcast should finish");

        assert!(matches!(
            next_message(&mut receiver).await,
            ServerMessage::Snapshot { .. }
        ));
        assert!(matches!(
            next_message(&mut receiver).await,
            ServerMessage::SyncComplete { .. }
        ));
        assert!(matches!(
            next_message(&mut receiver).await,
            ServerMessage::Operations { .. }
        ));
    }

    #[tokio::test]
    async fn source_lock_is_released_before_broadcasting_to_a_backpressured_peer() {
        let store = std::sync::Arc::new(std::sync::Mutex::new(
            crate::store::RoomStore::open_in_memory().expect("in-memory store"),
        ));
        let state = ServerState::new(RoomManager::new(store));
        let room = {
            let mut manager = state.manager.lock().await;
            manager.create_room().expect("room")
        };
        let client_a = ClientId::from_u128(10);
        let client_b = ClientId::from_u128(11);
        {
            let mut manager = state.manager.lock().await;
            manager
                .join(room.room_id, &room.token, client_a)
                .expect("client A join");
            manager
                .join(room.room_id, &room.token, client_b)
                .expect("client B join");
        }
        let session_a = SessionId::new();
        let session_b = SessionId::new();
        let mut receiver_a = add_peer(&state, session_a, Some(client_a), Some(room.room_id)).await;
        let _receiver_b = add_peer(&state, session_b, Some(client_b), Some(room.room_id)).await;
        let sender_a = state
            .sessions
            .lock()
            .await
            .get(&session_a)
            .expect("client A peer was inserted")
            .sender
            .clone();
        let outbound_b = peer_outbound(&state, session_b)
            .await
            .expect("client B has an outbound lock");
        let guard_b = outbound_b.lock().await;
        let operation = Operation::new(
            OperationId::new(client_a, 1),
            LamportTimestamp::new(1),
            VersionVector::default(),
            OperationKind::Create {
                element: Element::rectangle(
                    ElementId::from_u128(10),
                    Transform::new(Point::default(), Size::new(10.0, 10.0)),
                ),
            },
        );
        let submission_state = state.clone();
        let submission_sender = sender_a.clone();
        let submission = tokio::spawn(async move {
            handle_message(
                session_a,
                ClientMessage::SubmitOperations {
                    room_id: room.room_id,
                    request_id: 1,
                    operations: vec![operation],
                },
                &submission_state,
                &submission_sender,
            )
            .await
        });

        assert!(matches!(
            next_message(&mut receiver_a).await,
            ServerMessage::Ack { .. }
        ));
        tokio::task::yield_now().await;
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            send_to_session(&state, session_a, ServerMessage::Pong { nonce: 2 }),
        )
        .await
        .expect("source outbound lock must be released before peer broadcast");
        assert!(matches!(
            next_message(&mut receiver_a).await,
            ServerMessage::Pong { nonce: 2 }
        ));

        drop(guard_b);
        let _ = submission.await.expect("submission should finish");
    }
}
