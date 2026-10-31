//! Points de contrôle live sur Mapterhorn (T05 §2.7), à ± 50 m : feature `live-tests`, hors CI
//! bloquante. Points de vcyclist (`ElevationProviderIntegrationTest.kt:24-56`).
//!
//! `cargo test -p ord-server --features live-tests --test live_mapterhorn`

#![cfg(feature = "live-tests")]

mod common;

use axum::http::StatusCode;
use common::*;
use ord_domain::LatLon;
use ord_testkit::routes::{meridian, request_body};
use serde_json::json;

async fn elevation_at(p: LatLon) -> f64 {
    let mut config = test_config();
    config.cache.offline = false;
    let body = request_body(&meridian(p, 10.0, 2), Some(json!({"sampling": {"mode": "input"}})));
    let reply = send(app_with(&config), post_json(&body)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    reply.json()["points"][0]["ele"].as_f64().expect("altitude")
}

#[tokio::test]
async fn control_points_within_50_m() {
    for (name, p, expected) in [
        ("Mont Blanc", LatLon::new(45.8326, 6.8652), 4_805.0),
        ("Dead Sea", LatLon::new(31.5, 35.5), -430.0),
        ("Badwater", LatLon::new(36.250, -116.832), -85.0),
    ] {
        let ele = elevation_at(p).await;
        println!("{name}: {ele:.1} m (attendu {expected} ± 50 m)");
        assert!((ele - expected).abs() <= 50.0, "{name}: {ele} m, attendu {expected} ± 50 m");
    }
}
