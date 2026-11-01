//! Binaire `osm-route-details`.

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use ord_server::config::Config;
use ord_server::{internal_router, observability, render_contract, router, AppState};

#[derive(Debug, Parser)]
#[command(version, about = "Elevation and typology along a route")]
struct Args {
    /// Fichier TOML de configuration (sinon `ORD_CONFIG`) ; l'environnement l'emporte.
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Démarre le service (défaut).
    Serve,
    /// Écrit le contrat OpenAPI.
    Openapi {
        #[arg(long, default_value = "contract/openapi.json")]
        out: PathBuf,
    },
    /// Sonde de vie du conteneur (image distroless, sans `curl`) : `GET /health/live` sur le port
    /// public local ; code de sortie 0 si 200.
    Healthcheck,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    match args.command.unwrap_or(Command::Serve) {
        Command::Openapi { out } => {
            std::fs::write(&out, render_contract()).with_context(|| format!("writing {}", out.display()))?;
            Ok(())
        }
        Command::Serve => serve(Config::load(args.config.as_deref())?).await,
        Command::Healthcheck => healthcheck(&Config::load(args.config.as_deref())?),
    }
}

/// HTTP/1.0 minimal sur la boucle locale, délai de 2 s : pas de client HTTP complet pour une sonde.
fn healthcheck(config: &Config) -> anyhow::Result<()> {
    use std::io::{Read, Write};
    use std::time::Duration;
    let port = config.server.listen_addr.port();
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(2))
        .with_context(|| format!("connecting to {addr}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(b"GET /health/live HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
    let mut head = [0u8; 16];
    let n = stream.read(&mut head)?;
    let status = String::from_utf8_lossy(&head[..n]);
    anyhow::ensure!(status.starts_with("HTTP/1.") && status.get(9..12) == Some("200"), "not live: {status}");
    Ok(())
}

async fn serve(config: Config) -> anyhow::Result<()> {
    observability::init_tracing(&config.log);
    tracing::info!(config = %serde_json::to_string(&config)?, "configuration");
    let metrics = observability::install_metrics()?;
    let state = AppState::bootstrap(&config, ord_server::Wiring::default()).await?;

    let public = tokio::net::TcpListener::bind(config.server.listen_addr)
        .await
        .with_context(|| format!("binding {}", config.server.listen_addr))?;
    let internal = tokio::net::TcpListener::bind(config.server.metrics_addr)
        .await
        .with_context(|| format!("binding {}", config.server.metrics_addr))?;
    tracing::info!(public = %config.server.listen_addr, internal = %config.server.metrics_addr, "listening");

    let internal_task =
        tokio::spawn(async move { axum::serve(internal, internal_router(Some(metrics))).await });
    // Arrêt : plus d'admission, requêtes en vol attendues au plus `shutdown_grace_s` (T01 §10).
    let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
    let mut graceful_rx = stop_rx.clone();
    let server = axum::serve(public, router(state)).with_graceful_shutdown(async move {
        let _ = graceful_rx.wait_for(|stop| *stop).await;
    });
    let grace = std::time::Duration::from_secs(config.server.shutdown_grace_s);
    tokio::spawn(async move {
        shutdown_signal().await;
        let _ = stop_tx.send(true);
    });
    tokio::select! {
        result = server => result?,
        () = async {
            let _ = stop_rx.wait_for(|stop| *stop).await;
            tokio::time::sleep(grace).await;
        } => tracing::warn!(grace_s = grace.as_secs(), "shutdown grace elapsed: in-flight requests dropped"),
    }
    internal_task.abort();
    tracing::info!("stopped");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutdown requested");
}
