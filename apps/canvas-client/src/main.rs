//! Sketchi desktop client entry point.

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "sketchi", version, about = "Sketchi collaborative whiteboard")]
struct Args {}

fn main() {
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
