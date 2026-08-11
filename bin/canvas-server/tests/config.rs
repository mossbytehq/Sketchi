#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use canvas_server::config::{ServerConfig, TlsMode};
use canvas_server::websocket::Readiness;

#[test]
fn non_loopback_servers_require_tls() {
    let config = ServerConfig {
        bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 8080),
        tls_mode: TlsMode::Disabled,
        ..ServerConfig::default()
    };
    assert!(config.validate().is_err());

    let loopback = ServerConfig {
        bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080),
        tls_mode: TlsMode::Disabled,
        ..ServerConfig::default()
    };
    assert!(loopback.validate().is_ok());
}

#[test]
fn readiness_is_versionless_json_for_supervised_clients() {
    let readiness = Readiness::new("wss://127.0.0.1:3210/ws", "deadbeef");
    let encoded = serde_json::to_string(&readiness).unwrap();
    assert_eq!(
        encoded,
        r#"{"endpoint":"wss://127.0.0.1:3210/ws","certificate_sha256":"deadbeef"}"#
    );
}
