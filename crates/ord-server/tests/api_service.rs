//! Endpoints de service : santé, capacités, attribution, identifiant de requête.

mod common;

use axum::http::StatusCode;
use common::*;

#[tokio::test]
async fn health_endpoints() {
    let live = send(app(), get("/health/live")).await;
    assert_eq!(live.status, StatusCode::OK);
    assert_eq!(live.json()["status"], "ok");
    let ready = send(app(), get("/health/ready")).await;
    assert_eq!(ready.status, StatusCode::OK);
    assert_eq!(ready.json()["ready"], true);
}

#[tokio::test]
async fn capabilities_publish_bounds_and_defaults() {
    let reply = send(app(), get("/v1/capabilities")).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.content_type(), "application/json");
    let caps = reply.json();
    assert_eq!(caps["api_version"], ord_server::API_VERSION);
    assert_eq!(caps["rules_version"], ord_core::RULES_VERSION);
    assert_eq!(caps["limits"]["max_points"], 16_000);
    assert_eq!(caps["limits"]["max_length_m"], 200_000.0);
    assert_eq!(caps["limits"]["densified_max"], 56_000);
    assert_eq!(caps["option_bounds"]["step_m"]["min"], 5.0);
    assert_eq!(caps["option_bounds"]["step_m"]["max"], 1_000.0);
    assert_eq!(caps["defaults"]["sampling"]["step_m"], 25.0);
    assert_eq!(caps["defaults"]["sampling"]["mode"], "input_with_breakpoints");
    assert_eq!(caps["option_bounds"]["smoothing_m"]["min"], 10.0);
    assert_eq!(caps["defaults"]["elevation"]["smoothing_m"], 0.0);
    assert_eq!(caps["defaults"]["elevation"]["gain_preset"], "dem");
    assert_eq!(caps["primary_zone"], "FR-metropole+corse");
    assert_eq!(caps["data"]["bridge_dsm"], true);
    let sources: Vec<&str> = caps["elevation_sources"]
        .as_array()
        .expect("sources")
        .iter()
        .filter_map(|s| s["source"].as_str())
        .collect();
    assert_eq!(sources, ["ign-lidar-hd", "mapterhorn"]);
    assert_eq!(caps["elevation_sources"][1]["role"], "fallback");
    assert!(caps["enums"]["warning_codes"]
        .as_array()
        .is_some_and(|c| c.iter().any(|v| v == "terrain_fallback")));
}

#[tokio::test]
async fn capabilities_follow_configuration() {
    let mut config = test_config();
    config.limits.max_points = 1_000;
    config.elevation.ign.enabled = false;
    let caps = send(app_with(&config), get("/v1/capabilities")).await.json();
    assert_eq!(caps["limits"]["max_points"], 1_000);
    assert_eq!(caps["data"]["bridge_dsm"], false);
    assert_eq!(caps["elevation_sources"].as_array().map(Vec::len), Some(1));
    let attribution = send(app_with(&config), get("/v1/attribution")).await.json();
    assert!(!attribution["text"].as_str().unwrap_or("").contains("IGN"));
}

#[tokio::test]
async fn attribution_lists_configured_sources() {
    let reply = send(app(), get("/v1/attribution")).await;
    assert_eq!(reply.status, StatusCode::OK);
    let doc = reply.json();
    let text = doc["text"].as_str().expect("text");
    for mention in ["OpenStreetMap", "IGN", "Mapterhorn"] {
        assert!(text.contains(mention), "{text}");
    }
    assert!(doc["sources"].as_array().is_some_and(|s| s.len() >= 3));
}

#[tokio::test]
async fn request_id_is_propagated_or_generated() {
    let mut req = get("/health/live");
    req.headers_mut().insert("x-request-id", "abc-123".parse().expect("en-tête"));
    let reply = send(app(), req).await;
    assert_eq!(reply.headers["x-request-id"], "abc-123");

    let mut req = get("/nope");
    req.headers_mut().insert("x-request-id", "x".repeat(129).parse().expect("en-tête"));
    let reply = send(app(), req).await;
    let generated = reply.headers["x-request-id"].to_str().expect("ASCII");
    assert_eq!(generated.len(), 36, "UUID attendu : {generated}");
    assert_eq!(reply.json()["instance"], generated);
}
