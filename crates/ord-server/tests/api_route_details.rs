//! `POST /v1/route-details` sur un terrain synthétique servi par un faux serveur local
//! (incrément 1 : profil Mapterhorn seul, modes `input` et `distance`).

mod common;

use axum::http::StatusCode;
use common::*;
use ord_domain::LatLon;
use ord_server::config::Config;
use ord_testkit::routes::{meridian, request_body, START};
use ord_testkit::{ScriptedResponse, ScriptedServer, SyntheticTerrain};
use serde_json::{json, Value};

/// Plan incliné : +1 m tous les ≈ 111 m vers le nord, +0,5 m tous les ≈ 76 m vers l'est.
fn terrain() -> SyntheticTerrain {
    SyntheticTerrain::new(64, |p: LatLon| 50.0 + 1_000.0 * (p.lat - START.lat) + 500.0 * (p.lon - START.lon))
}

fn online_config(server: &ScriptedServer, dir: &std::path::Path) -> Config {
    let mut config = test_config();
    config.cache.offline = false;
    config.cache.dir = dir.to_path_buf();
    config.elevation.mapterhorn.url = server.url_template.clone();
    config.elevation.mapterhorn.tile_size = 64;
    config
}

async fn served(
    script: impl Fn(usize, &ord_testkit::ScriptedRequest) -> Option<ScriptedResponse> + Send + Sync + 'static,
) -> (ScriptedServer, tempfile::TempDir) {
    (terrain().serve(script).await, tempfile::tempdir().expect("dossier temporaire"))
}

fn without_volatile(mut v: Value) -> Value {
    v["meta"]["request_id"] = Value::Null;
    v["meta"]["duration_ms"] = Value::Null;
    v
}

#[tokio::test]
async fn input_mode_profile_and_summary() {
    let (server, dir) = served(|_, _| None).await;
    let app = app_with(&online_config(&server, dir.path()));
    let pts = meridian(START, 2_000.0, 5);
    let body = request_body(&pts, Some(json!({"sampling": {"mode": "input"}})));
    let reply = send(app, post_json(&body)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.content_type(), "application/json");
    let r = reply.json();
    assert_eq!(r["status"], "complete");
    assert_eq!(r["meta"]["api_version"], ord_server::API_VERSION);
    assert_eq!(r["meta"]["rules_version"], ord_core::RULES_VERSION);
    assert_eq!(r["meta"]["data_versions"]["elevation"][0]["source"], "mapterhorn");
    assert!(r["meta"]["data_versions"].get("typology").is_none(), "typologie non lue à l'incrément 1");
    let points = r["points"].as_array().expect("points");
    assert_eq!(points.len(), 5);
    for (i, (p, q)) in points.iter().zip(&pts).enumerate() {
        assert_eq!(p["index"], i);
        assert_eq!(p["origin"], "input");
        assert!(p.get("lat").is_none(), "un point d'entrée n'est jamais déplacé");
        assert_eq!(p["ele_method"], "dem");
        let expected = terrain().at(*q);
        let ele = p["ele"].as_f64().expect("altitude");
        assert!((ele - expected).abs() < 0.02, "point {i} : {ele} vs {expected}");
    }
    let s = &r["summary"];
    assert_eq!(s["length_m"], 2_000.0);
    let climb = terrain().at(pts[4]) - terrain().at(pts[0]);
    // Lissage du preset `dem` renormalisé aux bouts : une pente régulière perd ≈ 2 × 10 m de pente.
    let gain = s["gain_m"].as_f64().unwrap();
    assert!((gain - climb).abs() < 0.5, "{gain} vs {climb}");
    assert_eq!(s["loss_m"], 0.0);
    assert_eq!(
        s["gain_params"],
        json!({"preset": "dem", "threshold_m": 3.0, "smoothing_m": 30.0, "adapted": [], "partial": false})
    );
    assert_eq!(s["terrain_m"], json!({"mapterhorn": 2_000.0}));
    let q = &r["quality"];
    assert_eq!(q["elevation_coverage"], 1.0);
    assert_eq!(
        q["terrain"],
        json!([{"from_m": 0.0, "to_m": 2_000.0, "origin": "mapterhorn", "measured": false, "fallback_after_error": false}])
    );
    assert_eq!(q["warnings"], json!([]));
    assert_eq!(r["attribution"]["url"], "/v1/attribution");
    assert!(r["attribution"]["text"].as_str().unwrap().contains("Mapterhorn"));
}

#[tokio::test]
async fn distance_mode_and_default_mode() {
    let (server, dir) = served(|_, _| None).await;
    let config = online_config(&server, dir.path());
    let pts = meridian(START, 1_010.0, 3);
    let body = request_body(&pts, Some(json!({"sampling": {"mode": "distance", "step_m": 25}})));
    let r = send(app_with(&config), post_json(&body)).await.json();
    let points = r["points"].as_array().expect("points");
    assert_eq!(points.len(), 42, "0, 25, …, 1 000 et la fin à 1 010 m");
    assert_eq!(points[41]["d_m"], 1_010.0);
    assert!(points.iter().all(|p| p.get("origin").is_none() && p["lat"].is_f64() && p["lon"].is_f64()));
    assert_eq!(points[0]["index"], 0);
    assert_eq!(points[21]["index"], 1, "après le point d'entrée du milieu (505 m)");

    // Mode par défaut à l'incrément 1 : les points d'entrée, sans point inséré.
    let r = send(app_with(&config), post_json(&request_body(&pts, None))).await.json();
    let origins: Vec<&str> =
        r["points"].as_array().unwrap().iter().map(|p| p["origin"].as_str().unwrap()).collect();
    assert_eq!(origins, ["input", "input", "input"]);
}

#[tokio::test]
async fn same_request_twice_gives_equal_responses() {
    let (server, dir) = served(|_, _| None).await;
    let app = app_with(&online_config(&server, dir.path()));
    let body = request_body(
        &meridian(START, 3_000.0, 7),
        Some(json!({"sampling": {"mode": "distance", "step_m": 5}})),
    );
    let first = send(app.clone(), post_json(&body)).await;
    let hits = server.hits();
    let second = send(app, post_json(&body)).await;
    assert_eq!(server.hits(), hits, "seconde requête servie par le LRU");
    assert_eq!(without_volatile(first.json()), without_volatile(second.json()));
    assert_ne!(first.json()["meta"]["request_id"], second.json()["meta"]["request_id"]);
}

#[tokio::test]
async fn disk_cache_serves_an_offline_restart() {
    let (server, dir) = served(|_, _| None).await;
    let mut config = online_config(&server, dir.path());
    let body = request_body(&meridian(START, 1_500.0, 4), None);
    let online = send(app_with(&config), post_json(&body)).await.json();
    let hits = server.hits();
    config.cache.offline = true;
    let offline = send(app_with(&config), post_json(&body)).await.json();
    assert_eq!(server.hits(), hits, "aucune requête hors ligne");
    assert_eq!(without_volatile(online), without_volatile(offline));
}

#[tokio::test]
async fn sections_filter_the_response() {
    let (server, dir) = served(|_, _| None).await;
    let body = request_body(
        &meridian(START, 500.0, 2),
        Some(json!({"sections": ["summary"], "elevation": {"smoothing_m": 50}})),
    );
    let r = send(app_with(&online_config(&server, dir.path())), post_json(&body)).await.json();
    assert!(r.get("points").is_none());
    assert!(r["summary"].is_object() && r["quality"].is_object() && r["attribution"].is_object());
    let body = request_body(
        &meridian(START, 500.0, 2),
        Some(json!({"sections": ["elevation"], "elevation": {"smoothing_m": 50}})),
    );
    let r = send(app_with(&online_config(&server, dir.path())), post_json(&body)).await.json();
    assert!(r.get("summary").is_none());
    assert!(r["points"][0]["ele_smoothed"].is_f64(), "lissage demandé : ele_smoothed présent");
}

#[tokio::test]
async fn failed_tiles_give_503_or_a_partial_200() {
    let (server, dir) = served(|_, _| Some(ScriptedResponse::status(400))).await;
    let config = online_config(&server, dir.path());
    let pts = meridian(START, 800.0, 3);
    let reply = send(app_with(&config), post_json(&request_body(&pts, None))).await;
    let problem = assert_problem(&reply, StatusCode::SERVICE_UNAVAILABLE, "data_unavailable");
    assert_eq!(problem["details"]["source"], "elevation");
    assert_eq!(reply.headers["retry-after"], "30");

    let body = request_body(&pts, Some(json!({"accept_partial": true})));
    let r = send(app_with(&config), post_json(&body)).await.json();
    assert_eq!(r["status"], "partial");
    assert_eq!(
        r["quality"]["warnings"],
        json!([{"code": "elevation_unavailable", "message": "elevation sources failed on this range", "from_m": 0.0, "to_m": 800.0}])
    );
    assert!(r["points"].as_array().unwrap().iter().all(|p| p["ele"].is_null() && p["ele_method"] == "none"));
    assert_eq!(r["quality"]["elevation_coverage"], 0.0);
    assert!(r["summary"].get("ele_min_m").is_none());
    assert_eq!(r["summary"]["gain_params"]["partial"], true);
    assert!(r["meta"]["data_versions"]["elevation"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn absent_tiles_give_elevation_missing() {
    let (server, dir) = served(|_, _| Some(ScriptedResponse::status(404))).await;
    let r = send(
        app_with(&online_config(&server, dir.path())),
        post_json(&request_body(&meridian(START, 300.0, 2), None)),
    )
    .await
    .json();
    assert_eq!(r["status"], "partial");
    assert_eq!(r["quality"]["warnings"][0]["code"], "elevation_missing");
    assert_eq!(r["summary"]["terrain_m"], json!({"none": 300.0}));
}
