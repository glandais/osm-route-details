//! Appel du routeur en mémoire (`tower::ServiceExt::oneshot`), sans réseau.

#![allow(dead_code)]

use axum::body::Body;
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use ord_server::config::Config;
use ord_server::{router, AppState};
use serde_json::Value;
use tower::ServiceExt;

pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub text: String,
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.text).unwrap_or_else(|e| panic!("JSON attendu ({e}) : {}", self.text))
    }

    pub fn content_type(&self) -> &str {
        self.headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("")
    }
}

/// Adresse sans service (port discard) : aucune URL de test ne sort de la machine.
pub const NOWHERE: &str = "http://127.0.0.1:9";

/// Configuration de test : hors ligne, cache disque dans un dossier temporaire neuf, IGN désactivé
/// et toutes les URL vers `NOWHERE`. Une requête valide y lit des tuiles absentes du disque :
/// 503 `data_unavailable` (typologie et élévation), sans aucun réseau.
pub fn test_config() -> Config {
    let mut config = Config::default();
    config.cache.offline = true;
    config.elevation.ign.enabled = false;
    config.elevation.ign.wms_url = format!("{NOWHERE}/wms");
    config.elevation.mapterhorn.url = format!("{NOWHERE}/{{z}}/{{x}}/{{y}}.webp");
    config.elevation.mapterhorn.tilejson_url = format!("{NOWHERE}/tilejson.json");
    config.elevation.mapterhorn.versions_url = format!("{NOWHERE}/download_urls.json");
    config.typology.url = format!("{NOWHERE}/{{z}}/{{x}}/{{y}}");
    config.coverage.coverage_index_url = None;
    config.coverage.polygons_url = None;
    config.cache.dir = std::env::temp_dir().join(format!("ord-test-{}", uuid_like()));
    config
}

fn uuid_like() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    format!("{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed))
}

/// Réseau des tests : une route revêtue le long du méridien de `START` sur 4 km, avec un pont de
/// 100 m à partir de 600 m, et une route le long du méridien de `KRK`.
pub fn streets() -> ord_testkit::SyntheticStreets {
    use ord_testkit::routes::{meridian, START};
    use ord_testkit::SyntheticStreet;
    let north = |from: f64, to: f64| meridian(ord_testkit::routes::offset_north(START, from), to - from, 2);
    ord_testkit::SyntheticStreets::new(vec![
        SyntheticStreet::new("residential", north(-200.0, 600.0)).with("surface", "paved"),
        SyntheticStreet::new("residential", north(600.0, 700.0))
            .with("surface", "paved")
            .with("bridge", "true"),
        SyntheticStreet::new("residential", north(700.0, 4_000.0)).with("surface", "paved"),
        SyntheticStreet::new("track", meridian(ord_testkit::routes::offset_north(KRK, -200.0), 2_000.0, 2)),
    ])
}

/// Hors de France (Krk, Croatie).
pub const KRK: ord_domain::LatLon = ord_domain::LatLon::new(45.2423, 14.5621);

/// Script de serveur : les tuiles de rues z14 de `streets()`, sinon `script`.
pub fn with_streets(
    script: impl Fn(usize, &ord_testkit::ScriptedRequest) -> Option<ord_testkit::ScriptedResponse>
        + Send
        + Sync
        + 'static,
) -> impl Fn(usize, &ord_testkit::ScriptedRequest) -> Option<ord_testkit::ScriptedResponse> + Send + Sync + 'static
{
    let streets = streets().script();
    move |n, r| script(n, r).or_else(|| streets(n, r))
}

pub fn app() -> Router {
    app_with(&test_config())
}

pub fn app_with(config: &Config) -> Router {
    router(AppState::new(config).expect("état du service"))
}

/// La requête a passé la validation : sans tuile en cache, typologie et élévation sont en panne
/// (503 `both`, même avec `accept_partial`).
pub fn assert_passed_validation(reply: &Reply) {
    let body = assert_problem(reply, StatusCode::SERVICE_UNAVAILABLE, "data_unavailable");
    assert_eq!(body["details"]["source"], "both");
    assert!(reply.headers.get("retry-after").is_some(), "Retry-After toujours présent");
}

pub async fn send(app: Router, req: Request<Body>) -> Reply {
    let response = app.oneshot(req).await.expect("le routeur ne renvoie pas d'erreur");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.expect("corps lisible").to_bytes();
    Reply { status, headers, text: String::from_utf8_lossy(&bytes).into_owned() }
}

pub fn get(uri: &str) -> Request<Body> {
    Request::builder().method(Method::GET).uri(uri).body(Body::empty()).expect("requête valide")
}

pub fn post_raw(content_type: Option<&str>, body: impl Into<Body>) -> Request<Body> {
    let mut builder = Request::builder().method(Method::POST).uri("/v1/route-details");
    if let Some(ct) = content_type {
        builder = builder.header(header::CONTENT_TYPE, ct);
    }
    builder.body(body.into()).expect("requête valide")
}

pub fn post_json(body: &Value) -> Request<Body> {
    post_raw(Some("application/json"), body.to_string())
}

pub async fn post(body: &Value) -> Reply {
    send(app(), post_json(body)).await
}

/// Vérifie la forme RFC 9457 d'un problème et renvoie son corps (T05 §2.5, point 5).
pub fn assert_problem(reply: &Reply, status: StatusCode, code: &str) -> Value {
    assert_eq!(reply.status, status, "{}", reply.text);
    assert_eq!(reply.content_type(), "application/problem+json");
    let body = reply.json();
    assert_eq!(body["type"], format!("urn:osm-route-details:problem:{code}"));
    assert_eq!(body["code"], code);
    assert_eq!(body["status"], status.as_u16());
    assert!(body["title"].as_str().is_some_and(|t| !t.is_empty()));
    assert!(body["detail"].is_string());
    let request_id = reply.headers.get("x-request-id").and_then(|v| v.to_str().ok());
    assert_eq!(body["instance"].as_str(), request_id, "instance = X-Request-Id");
    body
}
