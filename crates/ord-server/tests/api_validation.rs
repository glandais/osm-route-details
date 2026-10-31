//! Validation de l'entrée (T01 §4.2) : tests nommés de l'incrément 0 et cas voisins.

mod common;

use std::io::Write;

use axum::http::StatusCode;
use common::*;
use ord_domain::LatLon;
use ord_testkit::routes::{meridian, request_body, START};
use serde_json::{json, Value};

fn route(length_m: f64, n: usize) -> Vec<LatLon> {
    meridian(START, length_m, n)
}

fn field_errors(body: &Value) -> Vec<(String, String)> {
    body["details"]["errors"]
        .as_array()
        .expect("details.errors")
        .iter()
        .map(|e| {
            (e["pointer"].as_str().unwrap_or("").to_owned(), e["message"].as_str().unwrap_or("").to_owned())
        })
        .collect()
}

/// Une option inconnue donne un 400 qui liste **toutes** les clés inconnues, à tout niveau.
#[tokio::test]
async fn unknown_options_are_all_listed() {
    let mut body = request_body(
        &route(1_000.0, 3),
        Some(json!({
            "foo": 1,
            "sampling": {"mode": "distance", "stepm": 10},
            "elevation": {"smoothing": 30, "gain_preset": "dem"},
            "bar": {"baz": true}
        })),
    );
    body["points"][1]["speed"] = json!(3.2);
    body["extra"] = json!(null);
    let reply = post(&body).await;
    let problem = assert_problem(&reply, StatusCode::BAD_REQUEST, "invalid_input");
    let mut keys: Vec<&str> = problem["details"]["unknown_keys"]
        .as_array()
        .expect("details.unknown_keys")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "/extra",
            "/options/bar",
            "/options/elevation/smoothing",
            "/options/foo",
            "/options/sampling/stepm",
            "/points/1/speed"
        ]
    );
}

/// Une coordonnée hors Web Mercator donne un 400 avec un pointeur JSON, sans recopier la valeur.
#[tokio::test]
async fn coordinate_outside_web_mercator_is_pointed_without_value() {
    let body = json!({"points": [
        {"lat": 47.2184, "lon": -1.5536},
        {"lat": 85.0612345, "lon": -1.5536},
        {"lat": 47.2201, "lon": 181.5432}
    ]});
    let reply = post(&body).await;
    let problem = assert_problem(&reply, StatusCode::BAD_REQUEST, "invalid_input");
    let pointers: Vec<String> = field_errors(&problem).into_iter().map(|(p, _)| p).collect();
    assert_eq!(pointers, ["/points/1/lat", "/points/2/lon"]);
    for digits in ["85.06", "0612345", "181.5", "5432", "47.2", "1.5536"] {
        assert!(!reply.text.contains(digits), "la réponse recopie une valeur ({digits}) : {}", reply.text);
    }
}

/// Un parcours de 201 km donne un 413 avec `details.limit = "length_m"`.
#[tokio::test]
async fn route_of_201_km_is_too_large() {
    let reply = post(&request_body(&route(201_000.0, 202), None)).await;
    let problem = assert_problem(&reply, StatusCode::PAYLOAD_TOO_LARGE, "route_too_large");
    assert_eq!(problem["details"]["limit"], "length_m");

    // 199 km passent la validation (hors ligne, sans tuile : 503 `data_unavailable`).
    let reply = post(&request_body(&route(199_000.0, 200), None)).await;
    assert_passed_validation(&reply);
}

#[tokio::test]
async fn too_many_distinct_points_is_too_large() {
    let reply = post(&request_body(&route(10_000.0, 16_001), None)).await;
    let problem = assert_problem(&reply, StatusCode::PAYLOAD_TOO_LARGE, "route_too_large");
    assert_eq!(problem["details"]["limit"], "points");

    // Les doublons consécutifs ne comptent pas.
    let mut pts = route(10_000.0, 16_000);
    pts.insert(1, pts[0]);
    let reply = post(&request_body(&pts, None)).await;
    assert_passed_validation(&reply);
}

#[tokio::test]
async fn body_limit_applies_after_decompression() {
    let mut config = test_config();
    config.limits.max_body_bytes = 4_096;
    let body = request_body(&route(1_000.0, 200), None).to_string();
    assert!(body.len() > 4_096);
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gz.write_all(body.as_bytes()).expect("compression");
    let compressed = gz.finish().expect("compression");
    assert!(compressed.len() < 4_096);
    let mut req = post_raw(Some("application/json"), compressed);
    req.headers_mut().insert("content-encoding", "gzip".parse().expect("en-tête"));
    let problem =
        assert_problem(&send(app_with(&config), req).await, StatusCode::PAYLOAD_TOO_LARGE, "route_too_large");
    assert_eq!(problem["details"]["limit"], "body_bytes");
}

#[tokio::test]
async fn gzip_request_body_is_accepted() {
    let body = request_body(&route(1_000.0, 3), None).to_string();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(body.as_bytes()).expect("compression");
    let mut req = post_raw(Some("application/json"), gz.finish().expect("compression"));
    req.headers_mut().insert("content-encoding", "gzip".parse().expect("en-tête"));
    assert_passed_validation(&send(app(), req).await);
}

#[tokio::test]
async fn route_shape_rules() {
    let pts = request_body(&route(1_000.0, 3), None)["points"].clone();
    let cases = [
        (json!({}), ""),
        (json!({"points": pts, "polyline": "_p~iF~ps|U_ulLnnqC"}), ""),
        (json!({"points": pts, "polyline_precision": 5}), "/polyline_precision"),
        (json!({"polyline": "_p~iF~ps|U_ulLnnqC"}), "/polyline_precision"),
        (json!({"polyline": "_p~iF~ps|U_ulLnnqC", "polyline_precision": 7}), "/polyline_precision"),
        (json!({"polyline": "_p~iF~ps|", "polyline_precision": 5}), "/polyline"),
        (json!({"points": [{"lat": 47.0, "lon": -1.0}, {"lat": 47.0, "lon": -1.0}]}), "/points"),
        (json!({"points": [{"lat": "47.0", "lon": -1.0}, {"lat": 47.1, "lon": -1.0}]}), "/points/0/lat"),
    ];
    for (body, pointer) in cases {
        let reply = post(&body).await;
        let problem = assert_problem(&reply, StatusCode::BAD_REQUEST, "invalid_input");
        assert_eq!(field_errors(&problem)[0].0, pointer, "{body}");
    }
}

#[tokio::test]
async fn polyline_and_tolerated_altitude_pass_validation() {
    // (38.5, -120.2) puis 0,001° plus au nord.
    let polyline = json!({"polyline": "_p~iF~ps|UgE?", "polyline_precision": 5});
    assert_passed_validation(&post(&polyline).await);
    let with_ele = json!({"points": [
        {"lat": 47.2184, "lon": -1.5536, "ele": 12.5},
        {"lat": 47.2201, "lon": -1.5490, "alt": null}
    ]});
    assert_passed_validation(&post(&with_ele).await);
}

#[tokio::test]
async fn option_bounds_are_enforced() {
    let pts = route(1_000.0, 3);
    let cases = [
        (json!({"sampling": {"mode": "distance", "step_m": 4.9}}), "/options/sampling/step_m"),
        (json!({"sampling": {"step_m": 1000.5}}), "/options/sampling/step_m"),
        (json!({"elevation": {"smoothing_m": 5}}), "/options/elevation/smoothing_m"),
        (json!({"elevation": {"smoothing_m": 1001}}), "/options/elevation/smoothing_m"),
        (json!({"debug": true}), "/options/debug"),
        (json!({"sampling": {"mode": "every_meter"}}), "/options/sampling/mode"),
        (json!({"sections": ["elevation", "map"]}), "/options/sections/1"),
    ];
    for (options, pointer) in cases {
        let reply = post(&request_body(&pts, Some(options.clone()))).await;
        let problem = assert_problem(&reply, StatusCode::BAD_REQUEST, "invalid_input");
        assert_eq!(field_errors(&problem)[0].0, pointer, "{options}");
    }
    let accepted = json!({
        "sections": ["elevation"],
        "sampling": {"mode": "distance", "step_m": 5},
        "elevation": {"smoothing_m": 0, "gain_preset": "barometric"},
        "structure_correction": false,
        "accept_partial": true,
        "debug": false
    });
    let reply = post(&request_body(&pts, Some(accepted))).await;
    assert_passed_validation(&reply);
}

#[tokio::test]
async fn debug_is_accepted_when_allowed() {
    let mut config = test_config();
    config.limits.debug_allowed = true;
    let body = request_body(&route(1_000.0, 3), Some(json!({"debug": true})));
    let reply = send(app_with(&config), post_json(&body)).await;
    assert_passed_validation(&reply);
}
