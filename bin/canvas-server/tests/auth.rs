#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use canvas_server::auth::CapabilityToken;

#[test]
fn capability_tokens_verify_without_storing_plaintext() {
    let token = CapabilityToken::generate();
    let hash = token.hash();
    assert!(token.verify(&hash));
    assert!(!CapabilityToken::from_secret("wrong").verify(&hash));
    assert_ne!(token.secret(), hash);
}

#[test]
fn generated_capability_tokens_are_single_uuids() {
    let token = CapabilityToken::generate();
    assert_eq!(token.secret().len(), 36);
    assert!(uuid::Uuid::parse_str(token.secret()).is_ok());
}
