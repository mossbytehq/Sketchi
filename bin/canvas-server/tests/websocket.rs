#![allow(
    clippy::expect_used,
    clippy::float_cmp,
    clippy::match_same_arms,
    clippy::needless_continue,
    clippy::panic,
    clippy::too_many_lines,
    clippy::unwrap_used,
    missing_docs
)]

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{body::Body, http::Request};
use canvas_core::{
    ClientId, Element, ElementId, LamportTimestamp, Operation, OperationId, OperationKind, Point,
    Size, Transform, VersionVector,
};
use canvas_protocol::{ClientMessage, ErrorCode, ServerMessage, decode_server, encode_client};
use canvas_server::{
    room::RoomManager,
    store::RoomStore,
    websocket::{ServerState, router},
};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tower::ServiceExt;

#[tokio::test]
async fn health_route_is_available_without_a_room_session() {
    let store = Arc::new(Mutex::new(RoomStore::open_in_memory().unwrap()));
    let manager = RoomManager::new(store);
    let response = router(ServerState::new(manager))
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn two_websocket_clients_receive_acknowledged_operations() {
    let store = Arc::new(Mutex::new(RoomStore::open_in_memory().unwrap()));
    let manager = RoomManager::new(store);
    let state = ServerState::new(manager);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router(state)).await;
    });

    let (mut first, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    let first_id = ClientId::from_u128(1);
    send_client(
        &mut first,
        ClientMessage::Hello {
            client_id: first_id,
            client_name: None,
        },
    )
    .await;
    assert!(matches!(
        receive_server(&mut first).await,
        ServerMessage::Welcome { .. }
    ));
    send_client(&mut first, ClientMessage::CreateRoom { request_id: 1 }).await;
    let (room_id, token) = match receive_server(&mut first).await {
        ServerMessage::RoomCreated {
            room_id,
            capability_token,
            ..
        } => (room_id, capability_token),
        other => panic!("unexpected room response: {other:?}"),
    };
    send_client(
        &mut first,
        ClientMessage::JoinRoom {
            room_id,
            capability_token: token.clone(),
            known_version: VersionVector::default(),
        },
    )
    .await;
    assert!(matches!(
        receive_server(&mut first).await,
        ServerMessage::Snapshot { .. }
    ));
    assert!(matches!(
        receive_server(&mut first).await,
        ServerMessage::SyncComplete { .. }
    ));
    assert!(matches!(
        receive_server(&mut first).await,
        ServerMessage::Participants { .. }
    ));

    let (mut second, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    let second_id = ClientId::from_u128(2);
    send_client(
        &mut second,
        ClientMessage::Hello {
            client_id: second_id,
            client_name: None,
        },
    )
    .await;
    let _ = receive_server(&mut second).await;
    send_client(
        &mut second,
        ClientMessage::JoinRoom {
            room_id,
            capability_token: token,
            known_version: VersionVector::default(),
        },
    )
    .await;
    assert!(matches!(
        receive_server(&mut second).await,
        ServerMessage::Snapshot { .. }
    ));
    assert!(matches!(
        receive_server(&mut second).await,
        ServerMessage::SyncComplete { .. }
    ));
    assert!(matches!(
        receive_server(&mut second).await,
        ServerMessage::Participants { .. }
    ));
    assert!(matches!(
        receive_server(&mut first).await,
        ServerMessage::UserJoined { .. }
    ));

    let operation = Operation::new(
        OperationId::new(first_id, 1),
        LamportTimestamp::new(1),
        VersionVector::default(),
        OperationKind::Create {
            element: Element::rectangle(
                ElementId::from_u128(9),
                Transform::new(Point::default(), Size::new(20.0, 20.0)),
            ),
        },
    );
    send_client(
        &mut first,
        ClientMessage::SubmitOperations {
            room_id,
            request_id: 2,
            operations: vec![operation],
        },
    )
    .await;
    assert!(matches!(
        receive_server(&mut first).await,
        ServerMessage::Ack { .. }
    ));
    assert!(matches!(
        receive_server(&mut second).await,
        ServerMessage::Operations { .. }
    ));

    server.abort();
}

type ClientSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn hello(
    address: std::net::SocketAddr,
    client_id: ClientId,
) -> (ClientSocket, ServerMessage) {
    let (mut socket, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    send_client(
        &mut socket,
        ClientMessage::Hello {
            client_id,
            client_name: None,
        },
    )
    .await;
    let reply = receive_server(&mut socket).await;
    (socket, reply)
}

#[tokio::test]
async fn silent_session_is_released_so_its_identity_can_reconnect() {
    let store = Arc::new(Mutex::new(RoomStore::open_in_memory().unwrap()));
    let state = ServerState::new(RoomManager::new(store))
        .with_heartbeat(Duration::from_millis(50), Duration::from_millis(300));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router(state)).await;
    });
    let client_id = ClientId::from_u128(21);

    // The first connection goes half-open: it stays open but never reads,
    // so it never answers the server's heartbeat pings.
    let (half_open, reply) = hello(address, client_id).await;
    assert!(matches!(reply, ServerMessage::Welcome { .. }));

    // While the stale session is still alive, the same identity is refused
    // and the socket is closed so the client's reconnect loop retries.
    let (mut refused, reply) = hello(address, client_id).await;
    assert!(matches!(
        reply,
        ServerMessage::Error {
            code: ErrorCode::Unauthorized,
            ..
        }
    ));
    let closed = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match refused.next().await {
                None | Some(Err(_) | Ok(Message::Close(_))) => return,
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert!(closed.is_ok(), "refused handshake should close the socket");

    let welcomed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let (_socket, reply) = hello(address, client_id).await;
            if matches!(reply, ServerMessage::Welcome { .. }) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    assert!(
        welcomed.is_ok(),
        "identity should be released after the idle timeout"
    );
    drop(half_open);
    server.abort();
}

#[tokio::test]
async fn responsive_session_outlives_the_idle_timeout() {
    let store = Arc::new(Mutex::new(RoomStore::open_in_memory().unwrap()));
    let state = ServerState::new(RoomManager::new(store))
        .with_heartbeat(Duration::from_millis(50), Duration::from_millis(200));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router(state)).await;
    });
    let (mut socket, reply) = hello(address, ClientId::from_u128(22)).await;
    assert!(matches!(reply, ServerMessage::Welcome { .. }));

    // Reading lets tungstenite answer the server's pings, which is all an
    // otherwise idle client does. Several idle timeouts pass meanwhile.
    let mut pings = 0;
    let _ = tokio::time::timeout(Duration::from_millis(800), async {
        while let Some(Ok(message)) = socket.next().await {
            if matches!(message, Message::Ping(_)) {
                pings += 1;
            }
        }
    })
    .await;
    assert!(pings > 0, "server should send heartbeat pings");

    send_client(&mut socket, ClientMessage::Ping { nonce: 7 }).await;
    assert!(matches!(
        receive_server(&mut socket).await,
        ServerMessage::Pong { nonce: 7 }
    ));
    server.abort();
}

async fn send_client(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    message: ClientMessage,
) {
    socket
        .send(Message::Text(
            String::from_utf8(encode_client(&message).unwrap())
                .unwrap()
                .into(),
        ))
        .await
        .unwrap();
}

async fn receive_server(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> ServerMessage {
    loop {
        match socket.next().await.unwrap().unwrap() {
            Message::Text(text) => return decode_server(text.as_bytes()).unwrap(),
            Message::Binary(bytes) => return decode_server(&bytes).unwrap(),
            Message::Ping(_) | Message::Pong(_) => continue,
            Message::Frame(_) => continue,
            Message::Close(_) => panic!("server closed socket"),
        }
    }
}
