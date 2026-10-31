//! IGN LiDAR HD et repli (incrément 2) sur des serveurs scriptés locaux : WMS BIL32 et Mapterhorn
//! Terrarium servent deux terrains différents, pour savoir d'où vient chaque altitude.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::http::StatusCode;
use common::*;
use ord_domain::LatLon;
use ord_server::config::Config;
use ord_testkit::routes::{meridian, request_body, START};
use ord_testkit::{ScriptedResponse, ScriptedServer, SyntheticTerrain};
use serde_json::{json, Value};

/// IGN : plan à 100 m ; Mapterhorn : plan à 300 m (repli reconnaissable).
fn ign_terrain() -> SyntheticTerrain {
    SyntheticTerrain::new(64, |p: LatLon| 100.0 + 1_000.0 * (p.lat - START.lat))
}

fn mapterhorn_terrain() -> SyntheticTerrain {
    SyntheticTerrain::new(64, |p: LatLon| 300.0 + 1_000.0 * (p.lat - START.lat))
}

fn config(ign: &ScriptedServer, mapterhorn: &ScriptedServer, dir: &std::path::Path) -> Config {
    let mut config = test_config();
    config.cache.offline = false;
    config.cache.dir = dir.to_path_buf();
    config.elevation.ign.enabled = true;
    config.elevation.ign.wms_url = ign.wms_url.clone();
    // Une relance (1 s) : le repli s'applique après les relances, sans attendre 31 s.
    config.elevation.ign.max_attempts = 2;
    config.elevation.mapterhorn.url = mapterhorn.url_template.clone();
    config.elevation.mapterhorn.tile_size = 64;
    config
}

async fn post_route(config: &Config, pts: &[LatLon]) -> (StatusCode, Value) {
    let body = request_body(pts, Some(json!({"sampling": {"mode": "input"}})));
    let reply = send(app_with(config), post_json(&body)).await;
    (reply.status, reply.json())
}

fn codes(r: &Value) -> Vec<String> {
    r["quality"]["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["code"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn ign_is_read_first_in_france() {
    let ign = ign_terrain().serve(|_, _| None).await;
    let mt = mapterhorn_terrain().serve(|_, _| None).await;
    let dir = tempfile::tempdir().unwrap();
    let pts = meridian(START, 1_000.0, 3);
    let (status, r) = post_route(&config(&ign, &mt, dir.path()), &pts).await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!(r["status"], "complete");
    for (p, q) in r["points"].as_array().unwrap().iter().zip(&pts) {
        let expected = ign_terrain().at(*q);
        assert!((p["ele"].as_f64().unwrap() - expected).abs() < 0.05, "{p} vs {expected}");
    }
    assert_eq!(r["summary"]["terrain_m"], json!({"ign_lidar_hd": 1_000.0}));
    assert_eq!(r["meta"]["data_versions"]["elevation"][0]["source"], "ign-lidar-hd");
    assert_eq!(codes(&r), Vec::<String>::new());
    assert!(ign.hits() > 0);
}

/// Scénario T05 §2.4 : IGN en 503 → 200 avec `terrain_fallback`, altitudes lues dans Mapterhorn.
#[tokio::test]
async fn ign_503_gives_200_with_terrain_fallback() {
    let ign = ScriptedServer::start(|_, _| ScriptedResponse::status(503)).await;
    let mt = mapterhorn_terrain().serve(|_, _| None).await;
    let dir = tempfile::tempdir().unwrap();
    let pts = meridian(START, 1_000.0, 3);
    let (status, r) = post_route(&config(&ign, &mt, dir.path()), &pts).await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!(r["status"], "complete", "un repli absorbé n'est pas une panne");
    assert_eq!(codes(&r), ["terrain_fallback"]);
    let w = &r["quality"]["warnings"][0];
    assert_eq!((w["from_m"].as_f64(), w["to_m"].as_f64()), (Some(0.0), Some(1_000.0)));
    assert_eq!(
        r["quality"]["terrain"],
        json!([{"from_m": 0.0, "to_m": 1_000.0, "origin": "mapterhorn_measured", "measured": true, "fallback_after_error": true}])
    );
    let first = r["points"][0]["ele"].as_f64().unwrap();
    assert!((first - mapterhorn_terrain().at(pts[0])).abs() < 0.05, "{first}");
    assert!(ign.hits() >= 2, "relance avant le repli");
}

/// Scénario T05 §2.4 : hors du contour LiDAR HD, aucune requête IGN ; `outside_primary_zone`.
#[tokio::test]
async fn outside_the_hull_no_ign_request() {
    let ign_hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&ign_hits);
    let ign = ScriptedServer::start(move |_, _| {
        counter.fetch_add(1, Ordering::SeqCst);
        ScriptedResponse::status(500)
    })
    .await;
    let krk = LatLon::new(45.2423, 14.5621);
    let mt = SyntheticTerrain::new(64, move |p: LatLon| 20.0 + 1_000.0 * (p.lat - krk.lat))
        .serve(|_, _| None)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let pts = meridian(krk, 1_000.0, 3);
    let (status, r) = post_route(&config(&ign, &mt, dir.path()), &pts).await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!(ign_hits.load(Ordering::SeqCst), 0, "aucune requête IGN hors contour");
    assert_eq!(codes(&r), ["outside_primary_zone"]);
    assert_eq!(r["summary"]["terrain_m"], json!({"mapterhorn": 1_000.0}));
    assert_eq!(r["status"], "complete");
    assert!(r["meta"]["data_versions"]["elevation"]
        .as_array()
        .unwrap()
        .iter()
        .all(|v| v["source"] != "ign-lidar-hd"));
}

/// No-data IGN (mer, ZICAD) : repli normal, sans `terrain_fallback`.
#[tokio::test]
async fn ign_nodata_falls_back_without_warning() {
    let ign = SyntheticTerrain::new(64, |_| f64::NAN).serve(|_, _| None).await;
    let mt = mapterhorn_terrain().serve(|_, _| None).await;
    let dir = tempfile::tempdir().unwrap();
    let (status, r) = post_route(&config(&ign, &mt, dir.path()), &meridian(START, 500.0, 2)).await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!(codes(&r), Vec::<String>::new());
    assert_eq!(r["quality"]["terrain"][0]["fallback_after_error"], false);
    assert_eq!(r["summary"]["terrain_m"], json!({"mapterhorn_measured": 500.0}));
}
