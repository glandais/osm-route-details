//! Rien de privé ne sort d'un itinéraire de A à B (api 1.15 et 1.16), sur le modèle de `generator_sentinel.rs` : des itinéraires
//! (avec et sans équipe, en échec) traversent le service sous un collecteur de logs JSON (`trace` pour le service) et un
//! exporteur de métriques. Aucune ligne, aucun span, aucune métrique ne contient une coordonnée (brute ou arrondie), la
//! clé d'environnement, la référence d'équipe, ni les noms dérivés.
//!
//! Binaire de test à part : le collecteur et l'exporteur sont globaux au processus.

mod common;

use std::io::Write;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use common::generator::*;
use common::*;
use metrics_exporter_prometheus::PrometheusBuilder;
use ord_server::generator::crypto::{decode_key, Credentials};
use serde_json::{json, Value};

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Fragments interdits d'un nombre : de 2 à 9 décimales, et sa partie décimale seule dès 5 décimales.
fn number(out: &mut Vec<String>, v: f64) {
    let v = v.abs();
    for d in 2..=9 {
        let s = format!("{v:.d$}");
        out.push(s.clone());
        if d >= 5 {
            out.push(format!(".{}", s.split_once('.').unwrap().1));
        }
    }
}

fn route(key: Option<&str>, team: Option<&str>, body: &Value) -> Request<Body> {
    let mut b = Request::builder()
        .method(Method::POST)
        .uri("/v1/router/route")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(k) = key {
        b = b.header("ord-namespace-key", k);
    }
    if let Some(t) = team {
        b = b.header("ord-team", t);
    }
    b.body(Body::from(body.to_string())).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn router_leaves_no_coordinate_key_or_team_in_logs_or_metrics() {
    let captured = Captured::default();
    let writer = captured.clone();
    tracing_subscriber::fmt()
        .json()
        .without_time()
        .with_current_span(true)
        .with_span_list(true)
        .with_env_filter("debug,ord_server=trace,tower_http=trace")
        .with_writer(move || writer.clone())
        .init();
    let metrics = PrometheusBuilder::new().install_recorder().expect("exporteur de métriques");

    let dir = tempfile::tempdir().unwrap();
    let files = write_files(dir.path(), "g", 21);
    let config = config_with(test_config(), dir.path(), &files);
    let app = ord_server::router(ord_server::AppState::new(&config).unwrap());

    let (k, team) = (key(7), "team-sentinel-reference");
    // Départ et arrivée sentinelles : décimales reconnaissables, à moins d'un mètre des carrefours.
    let (a, b) = (node(6, 6), node(6, 14));
    let (lat, lon) = (a[0] + 0.000_000_7, a[1] + 0.000_000_3);
    let (lat2, lon2) = (b[0] + 0.000_000_9, b[1] + 0.000_000_1);
    let body = json!({"start": {"lat": lat, "lon": lon}, "end": {"lat": lat2, "lon": lon2}, "ride_type": "road",
        "cursor": 0.25});

    assert_eq!(
        send(app.clone(), put_digest(&k, team, "digest-sentinel", team_deposit())).await.status,
        StatusCode::OK
    );
    let ok = send(app.clone(), route(Some(&k), Some(team), &body)).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    assert_eq!(ok.json()["mode"], "team_first");
    assert_eq!(send(app.clone(), route(None, None, &body)).await.status, StatusCode::OK);
    // Une préférence de voie aux valeurs reconnaissables : seul son sens (`less`) peut sortir.
    let (roads, roads_neutral) = (0.137_245_9, 0.862_754_1);
    let mut pref = body.clone();
    pref["use_roads"] = json!(roads);
    pref["use_roads_neutral"] = json!(roads_neutral);
    pref["avoid_main_roads"] = json!(true);
    assert_eq!(send(app.clone(), route(Some(&k), Some(team), &pref)).await.status, StatusCode::OK);
    // Équipe sans surcouche, hors couverture, trop loin d'une voie, clé invalide.
    assert_eq!(send(app.clone(), route(Some(&key(8)), Some(team), &body)).await.status, StatusCode::OK);
    let mut out = body.clone();
    out["end"] = json!({"lat": 48.5123456, "lon": 2.3123456});
    assert_eq!(send(app.clone(), route(None, None, &out)).await.status, StatusCode::BAD_REQUEST);
    out["end"] = json!({"lat": lat2 + 0.0723456, "lon": lon2});
    assert_eq!(send(app.clone(), route(None, None, &out)).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let mut far = body.clone();
    far["start"] = json!({"lat": node(0, 10)[0] - 0.007_234_5, "lon": node(0, 10)[1]});
    assert_eq!(send(app.clone(), route(None, None, &far)).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        send(app.clone(), route(Some("pas-une-cle"), Some(team), &body)).await.status,
        StatusCode::BAD_REQUEST
    );

    let logs = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    let rendered = metrics.render();
    assert!(logs.contains("route from A to B"), "ligne de synthèse d'un itinéraire");
    assert!(rendered.contains("ord_router_requests_total"), "métriques actives");
    assert!(rendered.contains("ord_router_duration_seconds"));
    assert!(rendered.contains("ord_router_routes_total"));
    assert!(rendered.contains("use_roads=\"less\""), "le sens de la préférence, jamais sa valeur");
    assert!(rendered.contains("avoid_main_roads=\"true\""));

    let creds = Credentials::derive(&decode_key(&k).unwrap(), team);
    let mut forbidden =
        vec![k.clone(), team.to_owned(), "digest-sentinel".to_owned(), creds.ns.clone(), creds.team.clone()];
    for v in [lat, lon, lat2, lon2, 48.5123456, 2.3123456, lat2 + 0.0723456, node(0, 10)[0] - 0.007_234_5] {
        number(&mut forbidden, v);
    }
    // Les décimales des réglages, pas leurs préfixes (« 0.14 » pourrait être une durée).
    forbidden.extend([
        roads.to_string(),
        roads_neutral.to_string(),
        "1372459".to_owned(),
        "8627541".to_owned(),
    ]);
    for fragment in &forbidden {
        for (what, text) in [("logs", &logs), ("métriques", &rendered)] {
            if let Some(line) = text.lines().find(|l| l.contains(fragment.as_str())) {
                panic!("{what} : « {fragment} » trouvé dans : {line}");
            }
        }
    }
    assert!(!rendered.contains("ns8"), "pas d'espace dans les métriques");
}
