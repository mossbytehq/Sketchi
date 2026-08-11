#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use std::fs;

#[test]
fn protocol_manifest_has_no_runtime_or_platform_dependencies() {
    let manifest_path = format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(manifest_path).unwrap();
    for forbidden in ["tokio", "wgpu", "winit", "egui", "rusqlite", "axum"] {
        assert!(
            !manifest.contains(forbidden),
            "found forbidden dependency: {forbidden}"
        );
    }
}
