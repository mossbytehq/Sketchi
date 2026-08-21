//! Sketchi desktop client entry point.

// Release builds are launched by the Windows shell as a GUI application, so
// Windows does not allocate a console window for the client's diagnostics.
// Keep debug builds attached to a console to preserve `cargo run` logging.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "sketchi", version, about = "Sketchi collaborative whiteboard")]
struct Args {}

fn main() {
    // Rustls has multiple crypto providers in the dependency graph. Install
    // the provider explicitly so release builds do not depend on feature
    // unification choosing one for us.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,wgpu=warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .try_init();
    tracing::info!("starting Sketchi desktop client");
    let _ = Args::parse();
    canvas_client::app::run();
}
