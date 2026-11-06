//! `POST /v1/route-details` sur un terrain synthétique servi par un faux serveur local
//! (Mapterhorn seul, IGN désactivé : modes `input` et `distance`). Le parcours est à Nantes, en
//! zone France : origine `mapterhorn_measured` (pas de fichier de couverture, donc pas de GLO-30).

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
    config.typology.url = server.url_template.clone();
    config.elevation.mapterhorn.tile_size = 64;
    config
}

async fn served(
    script: impl Fn(usize, &ord_testkit::ScriptedRequest) -> Option<ScriptedResponse> + Send + Sync + 'static,
) -> (ScriptedServer, tempfile::TempDir) {
    (terrain().serve(with_streets(script)).await, tempfile::tempdir().expect("dossier temporaire"))
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
    assert_eq!(r["meta"]["data_versions"]["typology"]["source"], "versatiles-shortbread");
    let points = r["points"].as_array().expect("points");
    assert_eq!(points.len(), 5);
    for (i, (p, q)) in points.iter().zip(&pts).enumerate() {
        assert_eq!(p["index"], i);
        assert_eq!(p["origin"], "input");
        assert!(p.get("lat").is_none(), "un point d'entrée n'est jamais déplacé");
        // Tuiles de 64 px : pixel de ≈ 150 m, sonde de culée à ≈ 300 m du pont (600–700 m) ; le
        // point de 500 m est sur la rampe du pont interpolé, sur ce plan incliné la même valeur.
        let method = p["ele_method"].as_str().unwrap();
        assert!(method == "dem" || (i == 1 && method == "interpolated_structure"), "{i} : {method}");
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
    assert_eq!(s["terrain_m"], json!({"mapterhorn_measured": 2_000.0}));
    let q = &r["quality"];
    assert_eq!(q["elevation_coverage"], 1.0);
    assert_eq!(
        q["terrain"],
        json!([{"from_m": 0.0, "to_m": 2_000.0, "origin": "mapterhorn_measured", "measured": true, "fallback_after_error": false}])
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

    // Mode par défaut (`input_with_breakpoints`) : les points d'entrée, jamais déplacés, et les
    // extrémités du pont (600–700 m) insérées avec leurs raisons et leur position.
    let r = send(app_with(&config), post_json(&request_body(&pts, None))).await.json();
    let points = r["points"].as_array().unwrap();
    let origins: Vec<&str> = points.iter().map(|p| p["origin"].as_str().unwrap()).collect();
    assert_eq!(origins, ["input", "input", "inserted", "inserted", "input"], "{points:?}");
    let entry = &r["summary"]["structures"][0];
    for (p, reason, bound) in
        [(&points[2], "structure_start", "from_m"), (&points[3], "structure_end", "to_m")]
    {
        assert_eq!(p["reasons"], json!([reason]));
        assert_eq!(p["d_m"], entry[bound], "borne de l'ouvrage portée par un point");
        assert_eq!(p["index"], 1);
        assert!(p["lat"].is_f64() && p["lon"].is_f64() && p["ele"].is_f64());
        assert_eq!(p["ele_method"], "interpolated_structure");
    }
    for (k, i) in [(0, 0), (1, 1), (4, 2)] {
        assert_eq!(points[k]["index"], i);
        assert!(points[k].get("lat").is_none() && points[k].get("reasons").is_none());
    }
    // Mode `input` : aucune insertion, aucune raison.
    let body = request_body(&pts, Some(json!({"sampling": {"mode": "input"}})));
    let r = send(app_with(&config), post_json(&body)).await.json();
    assert!(r["points"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["origin"] == "input" && p.get("reasons").is_none()));
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
    let (server, dir) = served(|_, r| (r.z != 14).then(|| ScriptedResponse::status(400))).await;
    let config = online_config(&server, dir.path());
    let pts = meridian(START, 800.0, 3);
    let reply = send(app_with(&config), post_json(&request_body(&pts, None))).await;
    let problem = assert_problem(&reply, StatusCode::SERVICE_UNAVAILABLE, "data_unavailable");
    assert_eq!(problem["details"]["source"], "elevation");
    // Sans `Retry-After` amont : échéance restante du cache négatif (60 s, T01 §4.4).
    let retry: u64 = reply.headers["retry-after"].to_str().unwrap().parse().unwrap();
    assert!((55..=60).contains(&retry), "{retry}");

    let body = request_body(&pts, Some(json!({"accept_partial": true})));
    let r = send(app_with(&config), post_json(&body)).await.json();
    assert_eq!(r["status"], "partial");
    assert_eq!(
        r["quality"]["warnings"],
        json!([
            {"code": "elevation_unavailable", "message": "elevation sources failed on this range", "from_m": 0.0, "to_m": 800.0},
            {"code": "structure_uncorrected", "message": "structure left uncorrected", "from_m": 599.83, "to_m": 699.89}
        ])
    );
    assert!(r["points"].as_array().unwrap().iter().all(|p| p["ele"].is_null() && p["ele_method"] == "none"));
    assert_eq!(r["quality"]["elevation_coverage"], 0.0);
    assert!(r["summary"].get("ele_min_m").is_none());
    assert_eq!(r["summary"]["gain_params"]["partial"], true);
    assert!(r["meta"]["data_versions"]["elevation"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn absent_tiles_give_elevation_missing() {
    let (server, dir) = served(|_, r| (r.z != 14).then(|| ScriptedResponse::status(404))).await;
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

#[tokio::test]
async fn segments_cover_the_route_with_the_bridge() {
    let (server, dir) = served(|_, _| None).await;
    let pts = meridian(START, 2_000.0, 5);
    let r = send(app_with(&online_config(&server, dir.path())), post_json(&request_body(&pts, None)))
        .await
        .json();
    assert_eq!(r["status"], "complete", "{r}");
    let segs = r["segments"].as_array().expect("segments");
    let kinds: Vec<&str> = segs.iter().map(|s| s["structure"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["ground", "bridge", "ground"], "{segs:?}");
    assert_eq!(segs[0]["from_m"], 0.0);
    assert_eq!(segs[2]["to_m"], 2_000.0);
    let bridge = &segs[1];
    let (from, to) = (bridge["from_m"].as_f64().unwrap(), bridge["to_m"].as_f64().unwrap());
    assert!((from - 600.0).abs() < 10.0 && (to - 700.0).abs() < 10.0, "{bridge}");
    assert_eq!(bridge["structure_source"], "osm");
    assert!(segs[0].get("structure_source").is_none());
    assert_eq!((bridge["from_index"].as_u64(), bridge["to_index"].as_u64()), (Some(1), Some(2)));
    assert_eq!(bridge["surface"], json!({"class": "paved", "source": "tagged"}));
    assert_eq!(bridge["way_kind"], "minor_road");
    assert_eq!(bridge["matched"], true);
    // Mapterhorn seul : pas de MNS, le pont est interpolé entre ses sondes de culée.
    assert_eq!(bridge["ele_method"], "interpolated_structure", "{r}");
    assert!(bridge["ele_start"].is_f64() && bridge["grade_pct"].is_f64());
    let entries = r["summary"]["structures"].as_array().expect("structures");
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0]["kind"], "bridge");
    assert_eq!(entries[0]["method"], "interpolated_structure");
    assert_eq!(entries[0]["structure_source"], "osm");
    let sc = &r["quality"]["structure_correction"];
    assert_eq!((sc["bridges"].as_str(), sc["tunnels"].as_str()), (Some("applied"), Some("none_found")));
    assert_eq!(sc["counts"], json!({"interpolated_structure": 1}));
    let s = &r["summary"];
    assert_eq!(s["unknown_m"], 0.0);
    assert_eq!(s["by_surface_class_m"], json!({"paved": 2_000.0}));
    assert_eq!(s["by_way_kind_m"], json!({"minor_road": 2_000.0}));
    let ground = s["by_structure_m"]["ground"].as_f64().unwrap();
    let deck = s["by_structure_m"]["bridge"].as_f64().unwrap();
    assert!((ground + deck - 2_000.0).abs() < 0.02);
    assert_eq!(r["quality"]["matched_ratio"], 1.0);
    assert_eq!(r["quality"]["inferred_surface_ratio"], 0.0);
}

#[tokio::test]
async fn disabled_structure_correction_keeps_the_dem() {
    let (server, dir) = served(|_, _| None).await;
    let pts = meridian(START, 2_000.0, 5);
    let body = request_body(&pts, Some(json!({"structure_correction": false})));
    let r = send(app_with(&online_config(&server, dir.path())), post_json(&body)).await.json();
    let sc = &r["quality"]["structure_correction"];
    assert_eq!((sc["bridges"].as_str(), sc["tunnels"].as_str()), (Some("disabled"), Some("disabled")));
    assert_eq!(r["summary"]["structures"][0]["method"], "none");
    assert!(r["points"].as_array().unwrap().iter().all(|p| p["ele_method"] == "dem"));
}

#[tokio::test]
async fn off_network_is_unmatched_but_complete() {
    let (server, dir) = served(|_, _| None).await;
    // 1 km plein est depuis le départ : loin de toute rue.
    let east = LatLon::new(START.lat, START.lon + 0.0135);
    let body = request_body(&[START, east], Some(json!({"sections": ["typology", "summary"]})));
    let r = send(app_with(&online_config(&server, dir.path())), post_json(&body)).await.json();
    assert_eq!(r["status"], "complete", "{r}");
    let w: Vec<&str> =
        r["quality"]["warnings"].as_array().unwrap().iter().map(|w| w["code"].as_str().unwrap()).collect();
    assert!(w.contains(&"unmatched_range"), "{w:?}");
    assert!(r["quality"]["matched_ratio"].as_f64().unwrap() < 0.1);
    assert!(r["summary"]["unknown_m"].as_f64().unwrap() > 900.0);
    let last = r["segments"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["structure"], "unknown");
    assert_eq!(last["matched"], false);
    assert_eq!(last["surface"], json!({"class": "unknown", "source": "none"}));
}

#[tokio::test]
async fn failed_street_tiles_give_503_or_typology_unavailable() {
    let (server, dir) = served(|_, r| (r.z == 14).then(|| ScriptedResponse::status(404))).await;
    let config = online_config(&server, dir.path());
    let pts = meridian(START, 800.0, 3);
    let reply = send(app_with(&config), post_json(&request_body(&pts, None))).await;
    let problem = assert_problem(&reply, StatusCode::SERVICE_UNAVAILABLE, "data_unavailable");
    assert_eq!(problem["details"]["source"], "typology", "404 VersaTiles : panne, jamais « pas de rue »");
    let body = request_body(&pts, Some(json!({"accept_partial": true})));
    let r = send(app_with(&config), post_json(&body)).await.json();
    assert_eq!(r["status"], "partial");
    assert_eq!(r["quality"]["warnings"][0]["code"], "typology_unavailable");
    assert_eq!(r["quality"]["matched_ratio"], 0.0);
    assert!(r["points"][0]["ele"].is_f64(), "l'élévation reste servie");
}

/// Revêtement fin (api 1.6) : tag `surface` brut de shortbread@1.1, segment coupé sur un changement
/// de `detail` même à classe égale, répartition `by_surface_detail_m`, absent sur une classe déduite.
#[tokio::test]
async fn surface_detail_splits_segments_and_is_summed() {
    use ord_testkit::routes::offset_north;
    use ord_testkit::{SyntheticStreet, SyntheticStreets};
    let north = |from: f64, to: f64| meridian(offset_north(START, from), to - from, 2);
    let streets = SyntheticStreets::new(vec![
        SyntheticStreet::new("residential", north(-200.0, 700.0)).with("surface", "asphalt"),
        SyntheticStreet::new("residential", north(700.0, 1_400.0)).with("surface", "sett"),
        SyntheticStreet::new("track", north(1_400.0, 2_000.0)).with("surface", "compacted"),
        SyntheticStreet::new("track", north(2_000.0, 2_800.0)),
    ])
    .script();
    let (server, dir) = served(move |n, r| streets(n, r)).await;
    let pts = meridian(START, 2_600.0, 14);
    let r = send(app_with(&online_config(&server, dir.path())), post_json(&request_body(&pts, None)))
        .await
        .json();
    assert_eq!(r["status"], "complete", "{r}");
    let segs = r["segments"].as_array().expect("segments");
    let surfaces: Vec<&Value> = segs.iter().map(|s| &s["surface"]).collect();
    assert_eq!(
        surfaces,
        [
            &json!({"class": "paved", "detail": "asphalt", "source": "tagged"}),
            &json!({"class": "paved", "detail": "sett", "source": "tagged"}),
            &json!({"class": "unpaved", "detail": "compacted", "source": "tagged"}),
            &json!({"class": "unpaved", "source": "inferred_highway"}),
        ],
        "{segs:?}"
    );
    let near = |v: &Value, x: f64| (v.as_f64().unwrap() - x).abs() < 10.0;
    assert!(near(&segs[1]["from_m"], 700.0) && near(&segs[2]["from_m"], 1_400.0), "{segs:?}");
    let by = r["summary"]["by_surface_detail_m"].as_object().expect("by_surface_detail_m");
    assert_eq!(by.keys().collect::<Vec<_>>(), ["asphalt", "compacted", "sett"]);
    let total: f64 = by.values().map(|v| v.as_f64().unwrap()).sum();
    assert!((total - 2_000.0).abs() < 1.0, "{by:?}");
    let caps = send(app_with(&online_config(&server, dir.path())), get("/v1/capabilities")).await.json();
    assert_eq!(caps["data"]["surface_detail"], true);
    assert!(caps["enums"]["surface_detail"].as_array().unwrap().contains(&json!("fine_gravel")));
}
