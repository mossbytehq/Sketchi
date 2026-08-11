#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use canvas_server::tls::LoopbackCertificate;

#[test]
fn loopback_certificate_has_a_stable_pin_for_supervised_clients() {
    let certificate = LoopbackCertificate::generate().unwrap();
    assert!(!certificate.cert_der().is_empty());
    assert!(certificate.verify_pin(certificate.pin()));
    assert!(!certificate.verify_pin("00"));
}
