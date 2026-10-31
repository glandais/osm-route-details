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
    }
}

async fn serve(config: Config) -> anyhow::Result<()> {
    observability::init_tracing(&config.log);
    tracing::info!(config = %serde_json::to_string(&config)?, "configuration");
    let metrics = observability::install_metrics()?;
    if config.elevation.ign.enabled {
        tracing::warn!("IGN LiDAR HD is configured but not read yet (increment 1): Mapterhorn only");
    }
    let state = AppState::new(&config)?;

    let public = tokio::net::TcpListener::bind(config.server.listen_addr)
        .await
        .with_context(|| format!("binding {}", config.server.listen_addr))?;
    let internal = tokio::net::TcpListener::bind(config.server.metrics_addr)
        .await
        .with_context(|| format!("binding {}", config.server.metrics_addr))?;
    tracing::info!(public = %config.server.listen_addr, internal = %config.server.metrics_addr, "listening");

    let internal_task =
        tokio::spawn(async move { axum::serve(internal, internal_router(Some(metrics))).await });
    axum::serve(public, router(state)).with_graceful_shutdown(shutdown_signal()).await?;
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
