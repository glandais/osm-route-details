//! Logs et métriques (T01 §8) : jamais de coordonnées, ni de corps de requête.

use std::time::{Duration, Instant};

use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::Response;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::config::{LogConfig, LogFormat};
use crate::problem::ProblemCode;

pub fn init_tracing(config: &LogConfig) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.filter));
    let registry = tracing_subscriber::registry().with(filter);
    match config.format {
        LogFormat::Json => registry.with(tracing_subscriber::fmt::layer().json()).init(),
        LogFormat::Text => registry.with(tracing_subscriber::fmt::layer()).init(),
    }
}

/// Installe l'exporteur Prometheus global ; à n'appeler qu'une fois, dans le binaire.
pub fn install_metrics() -> anyhow::Result<PrometheusHandle> {
    let handle = PrometheusBuilder::new().install_recorder()?;
    let upkeep = handle.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        loop {
            tick.tick().await;
            upkeep.run_upkeep();
        }
    });
    Ok(handle)
}

/// `ord_http_requests_total` et `ord_http_request_duration_seconds{route, status, code}`.
pub async fn record_http_metrics(req: Request, next: Next) -> Response {
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| "unmatched".to_owned(), |p| p.as_str().to_owned());
    let start = Instant::now();
    let response = next.run(req).await;
    let code = response.extensions().get::<ProblemCode>().map_or("", |c| c.as_str());
    let labels =
        [("route", route), ("status", response.status().as_u16().to_string()), ("code", code.to_owned())];
    metrics::counter!("ord_http_requests_total", &labels).increment(1);
    metrics::histogram!("ord_http_request_duration_seconds", &labels).record(start.elapsed().as_secs_f64());
    response
}
