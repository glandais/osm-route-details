//! Scénarios de pannes de T05 §2.4 (incrément 6) sur des serveurs scriptés locaux : décision
//! 200 / 503 et `Retry-After`, cache périmé servi en secours, refus avant toute lecture, budget
//! dépassé (504), file saturée (503 `overloaded`), client parti. Les autres scénarios de la liste
//! sont dans `api_ign.rs` (IGN en 503, hors contour), `api_route_details.rs` (typologie en panne)
//! et `api_validation.rs` (`debug` sans droit).

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use common::*;
use ord_domain::LatLon;
use ord_server::config::Config;
use ord_testkit::routes::{meridian, request_body, START};
use ord_testkit::{ScriptedRequest, ScriptedResponse, ScriptedServer, SyntheticTerrain};
use serde_json::{json, Value};

fn terrain() -> SyntheticTerrain {
    SyntheticTerrain::new(64, |p: LatLon| 50.0 + 1_000.0 * (p.lat - START.lat))
}

/// Tuile Terrarium du terrain, telle que la sert `SyntheticTerrain::serve`.
fn terrain_tile(r: &ScriptedRequest) -> ScriptedResponse {
    ScriptedResponse::ok(terrain().tile(r.z, r.x, r.y)).header("content-type", "image/webp")
}

/// Mapterhorn et VersaTiles sur le même serveur scripté ; IGN désactivé.
fn online(server: &ScriptedServer, dir: &std::path::Path) -> Config {
    let mut config = test_config();
    config.cache.offline = false;
    config.cache.dir = dir.to_path_buf();
    config.elevation.mapterhorn.url = server.url_template.clone();
    config.typology.url = server.url_template.clone();
    config.elevation.mapterhorn.tile_size = 64;
    config
}

fn codes(r: &Value) -> Vec<String> {
    r["quality"]["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["code"].as_str().unwrap().to_owned())
        .collect()
}

/// IGN et Mapterhorn en panne → 503 `elevation` ; `Retry-After` = le plus grand indice amont
/// (Mapterhorn : 503 avec `Retry-After: 100`, au-delà de l'échéance, donc sans relance), borné à
/// 120 s. Avec `accept_partial` → 200 `partial`, `elevation_unavailable`.
#[tokio::test]
async fn ign_and_mapterhorn_down_give_503_with_retry_after_or_partial() {
    let ign = ScriptedServer::start(|_, _| ScriptedResponse::status(503)).await;
    let mt = terrain()
        .serve(with_streets(|_, r| {
            (r.z != 14).then(|| ScriptedResponse::status(503).header("retry-after", "100"))
        }))
        .await;
    let dir = tempfile::tempdir().unwrap();
    let mut config = online(&mt, dir.path());
    config.elevation.ign.enabled = true;
    config.elevation.ign.wms_url = ign.wms_url.clone();
    config.elevation.ign.max_attempts = 2;
    let app = app_with(&config);
    let pts = meridian(START, 800.0, 3);
    let reply = send(app.clone(), post_json(&request_body(&pts, None))).await;
    let problem = assert_problem(&reply, StatusCode::SERVICE_UNAVAILABLE, "data_unavailable");
    assert_eq!(problem["details"]["source"], "elevation");
    assert_eq!(reply.headers["retry-after"], "100");
    assert!(ign.hits() > 0, "IGN essayé avant le repli");

    let reply = send(app, post_json(&request_body(&pts, Some(json!({"accept_partial": true}))))).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let r = reply.json();
    assert_eq!(r["status"], "partial");
    assert_eq!(codes(&r)[0], "elevation_unavailable");
    assert!(r["points"].as_array().unwrap().iter().all(|p| p["ele"].is_null()));
    assert_eq!(r["segments"][0]["matched"], true, "la typologie reste servie");
}

/// Typologie et élévation en panne → 503 `both`, même avec `accept_partial` ; sans indice amont,
/// `Retry-After` = échéance restante du cache négatif (60 s).
#[tokio::test]
async fn both_sides_down_give_503_even_with_accept_partial() {
    let server = ScriptedServer::start(|_, _| ScriptedResponse::status(500)).await;
    let dir = tempfile::tempdir().unwrap();
    let mut config = online(&server, dir.path());
    // Pas de relance : la règle de relance est commune à toutes les sources.
    config.elevation.ign.max_attempts = 1;
    let body = request_body(&meridian(START, 500.0, 2), Some(json!({"accept_partial": true})));
    let reply = send(app_with(&config), post_json(&body)).await;
    let problem = assert_problem(&reply, StatusCode::SERVICE_UNAVAILABLE, "data_unavailable");
    assert_eq!(problem["details"]["source"], "both");
    let retry: u64 = reply.headers["retry-after"].to_str().unwrap().parse().unwrap();
    assert!((55..=60).contains(&retry), "cache négatif de 60 s : {retry}");
}

/// Cache périmé servi en secours : les tuiles de rues expirent tout de suite (TTL 0) ; la source
/// tombe ; la réponse reste 200 `complete`, identique, et la source a bien été réessayée.
#[tokio::test]
async fn stale_cache_is_served_when_the_source_fails() {
    let failing = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&failing);
    let server = terrain()
        .serve(with_streets(move |_, r| {
            (r.z == 14 && flag.load(Ordering::SeqCst)).then(|| ScriptedResponse::status(503))
        }))
        .await;
    let dir = tempfile::tempdir().unwrap();
    let mut config = online(&server, dir.path());
    config.cache.ttl_streets_days = 0;
    let body = request_body(&meridian(START, 2_000.0, 5), None);
    let fresh = send(app_with(&config), post_json(&body)).await.json();
    assert_eq!(fresh["status"], "complete");
    failing.store(true, Ordering::SeqCst);
    let hits = server.hits();
    // Nouveau processus (LRU vide), même cache disque.
    let reply = send(app_with(&config), post_json(&body)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let stale = reply.json();
    assert!(server.hits() > hits, "source réessayée avant le secours");
    assert_eq!(stale["status"], "complete");
    assert_eq!(stale["segments"], fresh["segments"]);
    assert_eq!(codes(&stale), codes(&fresh));
}

/// Option inconnue → 400 avant toute lecture de tuile (compteur du serveur à 0).
#[tokio::test]
async fn unknown_option_is_refused_before_any_tile_read() {
    let server = terrain().serve(with_streets(|_, _| None)).await;
    let dir = tempfile::tempdir().unwrap();
    let body = request_body(&meridian(START, 1_000.0, 3), Some(json!({"foo": 1, "sampling": {"bar": 2}})));
    let reply = send(app_with(&online(&server, dir.path())), post_json(&body)).await;
    let problem = assert_problem(&reply, StatusCode::BAD_REQUEST, "invalid_input");
    assert_eq!(problem["details"]["unknown_keys"], json!(["/options/foo", "/options/sampling/bar"]));
    assert_eq!(server.hits(), 0, "aucune tuile lue");
}

/// Budget dépassé : le serveur ne répond qu'après le budget (1 s ici, en temps réel : le serveur
/// est un vrai serveur local ; le même cas en temps virtuel est un test de l'orchestrateur) → 504
/// `timeout`, sans `Retry-After`, jamais de réponse tronquée.
#[tokio::test]
async fn source_slower_than_the_budget_gives_504() {
    let server = ScriptedServer::start(|_, r| terrain_tile(r).delayed(Duration::from_secs(3))).await;
    let dir = tempfile::tempdir().unwrap();
    let mut config = online(&server, dir.path());
    config.budget.request_budget_s = 1;
    // Construit avant la mesure (client HTTP et certificats racines).
    let app = app_with(&config);
    let started = Instant::now();
    let reply = send(app, post_json(&request_body(&meridian(START, 500.0, 2), None))).await;
    assert_problem(&reply, StatusCode::GATEWAY_TIMEOUT, "timeout");
    assert!(reply.headers.get("retry-after").is_none());
    assert!(started.elapsed() < Duration::from_millis(2_500), "{:?}", started.elapsed());
}

/// Une seule requête dans le pipeline et aucune place en file : pendant qu'une requête lente est en
/// vol, la suivante reçoit aussitôt 503 `overloaded` avec `Retry-After: 2`.
#[tokio::test]
async fn full_queue_gives_503_overloaded() {
    let slow = Arc::new(AtomicBool::new(true));
    let flag = Arc::clone(&slow);
    let server = terrain()
        .serve(with_streets(move |_, r| {
            (r.z != 14 && flag.load(Ordering::SeqCst))
                .then(|| terrain_tile(r).delayed(Duration::from_secs(1)))
        }))
        .await;
    let dir = tempfile::tempdir().unwrap();
    let mut config = online(&server, dir.path());
    config.budget.max_in_flight = Some(1);
    config.budget.max_queued = Some(0);
    let app = app_with(&config);
    let body = request_body(&meridian(START, 500.0, 2), None);
    let first = tokio::spawn(send(app.clone(), post_json(&body)));
    while server.hits() == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let started = Instant::now();
    let reply = send(app.clone(), post_json(&body)).await;
    assert_problem(&reply, StatusCode::SERVICE_UNAVAILABLE, "overloaded");
    assert_eq!(reply.headers["retry-after"], "2");
    assert!(started.elapsed() < Duration::from_millis(500), "refus immédiat");
    assert_eq!(first.await.unwrap().status, StatusCode::OK);
}

/// Client parti en cours de calcul : son jeton est rendu, la requête suivante passe.
#[tokio::test]
async fn disconnected_client_frees_its_slot() {
    let slow = Arc::new(AtomicBool::new(true));
    let flag = Arc::clone(&slow);
    let server = terrain()
        .serve(with_streets(move |_, r| {
            (r.z != 14 && flag.load(Ordering::SeqCst))
                .then(|| terrain_tile(r).delayed(Duration::from_secs(1)))
        }))
        .await;
    let dir = tempfile::tempdir().unwrap();
    let mut config = online(&server, dir.path());
    config.budget.max_in_flight = Some(1);
    config.budget.max_queued = Some(0);
    let app = app_with(&config);
    let body = request_body(&meridian(START, 500.0, 2), None);
    let gone = tokio::time::timeout(Duration::from_millis(200), send(app.clone(), post_json(&body))).await;
    assert!(gone.is_err(), "client parti avant la réponse");
    slow.store(false, Ordering::SeqCst);
    let reply = send(app, post_json(&body)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
}
