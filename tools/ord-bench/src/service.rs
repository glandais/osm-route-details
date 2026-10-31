//! Le service appelé **en processus** (T05 §5.1) : routeur axum complet, couches comprises, sur le
//! cache de banc. Aucune coordonnée n'est écrite par ce module.

use std::path::Path;

use anyhow::{bail, Context};
use axum::body::Body;
use axum::http::{header, Method, Request};
use http_body_util::BodyExt;
use ord_data::elevation::PixelConvention;
use ord_server::config::Config;
use ord_server::{router, AppState, Wiring};
use serde_json::{json, Value};
use tower::ServiceExt;

/// Point envoyé au service.
#[derive(Clone, Copy, PartialEq)]
pub struct Pt {
    pub lat: f64,
    pub lon: f64,
}

impl std::fmt::Debug for Pt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Pt(..)")
    }
}

#[derive(Clone)]
pub struct Service {
    app: axum::Router,
}

impl std::fmt::Debug for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Service(..)")
    }
}

/// Variante du service mesurée par le banc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Variant {
    pub convention: PixelConvention,
    /// IGN LiDAR HD lu (défaut du service) ; `false` : Mapterhorn seul (mesures de l'incrément 1,
    /// test de méthode `vc-strava`, correction du demi-pixel sur Mapterhorn).
    pub ign: bool,
}

impl Variant {
    /// Le service tel qu'il est déployé.
    pub const DEFAULT: Self = Self { convention: PixelConvention::Center, ign: true };
    /// Mapterhorn seul, centres de pixel.
    pub const MAPTERHORN: Self = Self { convention: PixelConvention::Center, ign: false };
}

/// Configuration du service pour le banc : cache disque = cache de banc.
pub fn bench_config(cache_dir: &Path, offline: bool, variant: Variant) -> Config {
    let mut config = Config::default();
    config.cache.dir = cache_dir.to_path_buf();
    config.cache.offline = offline;
    config.elevation.ign.enabled = variant.ign;
    // VersaTiles public : peu de requêtes à la fois, jamais de rafale (exploration 06).
    config.typology.concurrency = 2;
    config
}

impl Service {
    /// Service en processus, après ses lectures de démarrage (version Mapterhorn, index de
    /// couverture), relues dans le cache de banc en mode hors ligne.
    pub async fn start(cache_dir: &Path, offline: bool, variant: Variant) -> anyhow::Result<Self> {
        let config = bench_config(cache_dir, offline, variant);
        let state = AppState::bootstrap(&config, Wiring { pixel_convention: variant.convention }).await?;
        Ok(Self { app: router(state) })
    }

    /// Service en processus sur une configuration donnée (diagnostics : `debug` permis).
    pub async fn start_with(config: &Config) -> anyhow::Result<Self> {
        let state = AppState::bootstrap(config, Wiring { pixel_convention: PixelConvention::Center }).await?;
        Ok(Self { app: router(state) })
    }

    /// `POST /v1/route-details` ; une réponse non 200 est une erreur qui porte le `code` du
    /// problème RFC 9457 (jamais de coordonnée).
    pub async fn route_details(&self, points: &[Pt], options: Value) -> anyhow::Result<Value> {
        let pts: Vec<Value> = points.iter().map(|p| json!({"lat": p.lat, "lon": p.lon})).collect();
        let body = json!({"points": pts, "options": options}).to_string();
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/route-details")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .context("requête")?;
        let response = self.app.clone().oneshot(req).await.context("appel du routeur")?;
        let status = response.status();
        let bytes = response.into_body().collect().await.context("corps de réponse")?.to_bytes();
        let value: Value = serde_json::from_slice(&bytes).context("réponse JSON")?;
        if !status.is_success() {
            bail!("service : HTTP {} `{}`", status.as_u16(), value["code"].as_str().unwrap_or("?"));
        }
        Ok(value)
    }
}
