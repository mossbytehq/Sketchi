//! Sketchi collaboration server entry point.

use std::{
    fs::File,
    io::BufReader,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use canvas_server::{
    config::{ServerConfig, TlsMode},
    room::RoomManager,
    store::RoomStore,
    tls::LoopbackCertificate,
    websocket::{ServerState, serve_http, serve_tls, serve_tls_with_readiness},
};
use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "sketchi-server",
    version,
    about = "Sketchi collaboration server"
)]
struct Args {
    /// Listening address.
    #[arg(long, default_value = "127.0.0.1:3210")]
    bind: SocketAddr,
    /// `SQLite` database path.
    #[arg(long, default_value = "sketchi.sqlite3")]
    database: PathBuf,
    /// PEM certificate path for non-loopback TLS.
    #[arg(long)]
    certificate: Option<PathBuf>,
    /// PEM private key path for non-loopback TLS.
    #[arg(long)]
    private_key: Option<PathBuf>,
    /// Explicitly allow only the loopback development escape hatch.
    #[arg(long)]
    insecure_loopback: bool,
    /// Validate configuration and exit without binding.
    #[arg(long)]
    check_config: bool,
    /// Emit a JSON readiness line for a supervised local client.
    #[arg(long)]
    ready: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let tls_mode = if args.insecure_loopback
        || (args.bind.ip().is_loopback()
            && args.certificate.is_none()
            && args.private_key.is_none())
    {
        TlsMode::Disabled
    } else {
        TlsMode::Required
    };
    let config = ServerConfig {
        bind: args.bind,
        database: args.database,
        tls_mode,
        certificate: args.certificate,
        private_key: args.private_key,
        emit_readiness: args.ready,
    };
    config.validate().map_err(anyhow::Error::msg)?;
    if args.check_config {
        println!("configuration ok: {}", config.bind);
        return Ok(());
    }

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();
    let store = RoomStore::open(&config.database)
        .with_context(|| format!("open database {}", config.database.display()))?;
    let manager = RoomManager::new(Arc::new(Mutex::new(store)));
    let state = ServerState::new(manager);
    let generated_loopback = args.ready
        && config.bind.ip().is_loopback()
        && config.certificate.is_none()
        && config.private_key.is_none();
    if generated_loopback {
        let certificate =
            LoopbackCertificate::generate().context("generate loopback readiness certificate")?;
        let tls = certificate
            .server_config()
            .context("configure loopback readiness TLS")?;
        serve_tls_with_readiness(state, config.bind, tls, Some(certificate.pin())).await?;
        return Ok(());
    }
    match config.tls_mode {
        TlsMode::Disabled => serve_http(state, config.bind).await?,
        TlsMode::Required => {
            let certificate = config
                .certificate
                .as_ref()
                .context("certificate path missing")?;
            let private_key = config
                .private_key
                .as_ref()
                .context("private key path missing")?;
            let (tls, pin) = load_tls_config(certificate, private_key)?;
            if config.emit_readiness {
                serve_tls_with_readiness(state, config.bind, tls, Some(&pin)).await?;
            } else {
                serve_tls(state, config.bind, tls).await?;
            }
        }
    }
    Ok(())
}

fn load_tls_config(
    certificate_path: &PathBuf,
    private_key_path: &PathBuf,
) -> Result<(rustls::ServerConfig, String)> {
    let mut certificate_reader = BufReader::new(File::open(certificate_path)?);
    let certificates = rustls_pemfile::certs(&mut certificate_reader)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let certificate = certificates.first().context("certificate missing")?;
    let pin = LoopbackCertificate::pin_for_der(certificate.as_ref());
    let mut key_reader = BufReader::new(File::open(private_key_path)?);
    let private_key =
        rustls_pemfile::private_key(&mut key_reader)?.context("private key missing")?;
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certificates, private_key)?;
    Ok((config, pin))
}
