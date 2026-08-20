#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use canvas_client::supervisor::{
    LocalServer, ReconnectBackoff, ReconnectState, SUPERVISED_BIND_ADDRESS,
};
use canvas_client::supervisor::{SupervisorError, parse_ready_line};
use std::process::Command;
use std::time::Duration;

#[test]
fn readiness_requires_endpoint_and_certificate_pin() {
    let ready = parse_ready_line(
        r#"{"endpoint":"wss://127.0.0.1:1234","certificate_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
    )
    .unwrap();
    assert_eq!(ready.endpoint, "wss://127.0.0.1:1234");
    assert!(matches!(
        parse_ready_line(
            r#"{"endpoint":"","certificate_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#
        ),
        Err(SupervisorError::InvalidReadiness(_))
    ));
    assert!(matches!(
        parse_ready_line(
            r#"{"endpoint":"ws://127.0.0.1:1234","certificate_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
        ),
        Err(SupervisorError::InvalidReadiness(_))
    ));
    assert!(matches!(
        parse_ready_line(r#"{"endpoint":"wss://127.0.0.1:1234","certificate_sha256":"abc"}"#,),
        Err(SupervisorError::InvalidReadiness(_))
    ));
}

#[test]
fn supervised_server_requests_a_wildcard_bind_for_lan_reachability() {
    assert_eq!(SUPERVISED_BIND_ADDRESS, "0.0.0.0:0");
}

#[test]
fn reconnect_backoff_is_bounded_and_resets_after_connection() {
    let mut backoff =
        ReconnectBackoff::new(Duration::from_millis(10), Duration::from_millis(25), 3).unwrap();

    assert_eq!(
        backoff.on_disconnect(),
        ReconnectState::Waiting {
            attempt: 1,
            delay: Duration::from_millis(10),
        }
    );
    assert_eq!(
        backoff.on_disconnect(),
        ReconnectState::Waiting {
            attempt: 2,
            delay: Duration::from_millis(20),
        }
    );
    assert_eq!(
        backoff.on_disconnect(),
        ReconnectState::Waiting {
            attempt: 3,
            delay: Duration::from_millis(25),
        }
    );
    assert_eq!(
        backoff.on_disconnect(),
        ReconnectState::Exhausted { attempts: 3 }
    );

    backoff.on_connected();
    assert_eq!(backoff.state(), ReconnectState::Connected);
    assert_eq!(
        backoff.on_disconnect(),
        ReconnectState::Waiting {
            attempt: 1,
            delay: Duration::from_millis(10),
        }
    );
}

#[test]
fn reconnect_backoff_rejects_unbounded_or_zero_configuration() {
    assert!(ReconnectBackoff::new(Duration::ZERO, Duration::from_secs(1), 3).is_err());
    assert!(ReconnectBackoff::new(Duration::from_secs(2), Duration::from_secs(1), 3).is_err());
    assert!(ReconnectBackoff::new(Duration::from_millis(1), Duration::from_secs(1), 0).is_err());
}

#[test]
fn local_server_reports_a_spawn_failure_without_panicking() {
    let error = LocalServer::spawn(Command::new("sketchi-server-that-does-not-exist"))
        .expect_err("missing supervised server must fail");
    assert!(matches!(error, SupervisorError::Io(_)));
}
