//! Parcours sentinelle (T01 §8) : un parcours aux chiffres reconnaissables traverse tout le
//! pipeline (IGN en panne puis repli, ouvrage, typologie, réponse partielle, erreur de validation)
//! sous un collecteur de logs JSON (niveau `trace` pour les crates du service) et un exporteur de
//! métriques. Aucune ligne de log, aucun span ni aucune métrique ne doit contenir ses coordonnées,
//! brutes ou arrondies, ni les clés des tuiles lues, ni l'emprise d'une requête WMS.
//!
//! Binaire de test à part : le collecteur et l'exporteur sont globaux au processus.

mod common;

use std::io::Write;
use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use common::*;
use metrics_exporter_prometheus::PrometheusBuilder;
use ord_domain::geo::Grid;
use ord_domain::LatLon;
use ord_testkit::routes::{meridian, offset_north, request_body, START};
use ord_testkit::{ScriptedResponse, ScriptedServer, SyntheticTerrain};
use serde_json::json;

/// Départ du parcours sentinelle, à 30 m de `START` (le réseau synthétique de `common` suit le
/// méridien de `START`) : ses décimales ne se retrouvent dans aucun compteur ni aucune durée.
fn sentinel() -> LatLon {
    let p = offset_north(START, 30.0);
    LatLon::new(p.lat + 0.000_000_7, START.lon)
}

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

/// Fragments interdits : chaque coordonnée de 2 à 9 décimales, et sa partie décimale seule (point
/// compris, sans signe ni partie entière) dès 5 décimales, chaque tuile `z/x/y` aux zooms lus, chaque bord
/// d'emprise WMS à 4 décimales et plus.
fn forbidden(points: &[LatLon]) -> Vec<String> {
    let mut out = Vec::new();
    let mut number = |v: f64| {
        let v = v.abs();
        for d in 2..=9 {
            let s = format!("{v:.d$}");
            out.push(s.clone());
            if d >= 5 {
                out.push(format!(".{}", s.split_once('.').unwrap().1));
            }
        }
    };
    for p in points {
        number(p.lat);
        number(p.lon);
    }
    for z in [12u8, 14, 15] {
        for p in points {
            let k = Grid::new(z, 1).tile_of(Grid::new(z, 1).to_units(*p));
            out.push(format!("{}/{}/{}", k.z, k.x, k.y));
            out.push(format!("x={}", k.x));
            out.push(format!("\"x\":{}", k.x));
            out.push(format!("\"y\":{}", k.y));
            if z == 15 {
                let (w, s, e, n) = k.bounds();
                for v in [w, s, e, n] {
                    out.push(format!("{:.4}", v.abs()));
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sentinel_route_leaves_no_coordinate_in_logs_or_metrics() {
    let captured = Captured::default();
    let writer = captured.clone();
    tracing_subscriber::fmt()
        .json()
        // Sans horodatage : ses fractions de seconde (`09:51:31.559878Z`) contiennent au hasard un
        // fragment interdit court (« 1.55 »), faux positif sans rapport avec le parcours.
        .without_time()
        .with_current_span(true)
        .with_span_list(true)
        .with_env_filter("debug,ord_server=trace,ord_pipeline=trace,ord_data=trace,ord_core=trace,ord_adapters=trace,tower_http=trace")
        .with_writer(move || writer.clone())
        .init();
    let metrics = PrometheusBuilder::new().install_recorder().expect("exporteur de métriques");

    // IGN : un échec sur deux (repli Mapterhorn) ; Mapterhorn et rues servis.
    let ign_terrain = SyntheticTerrain::new(64, |p: LatLon| 100.0 + 1_000.0 * (p.lat - START.lat));
    let ign = ign_terrain.serve(|n, _| (n % 2 == 0).then(|| ScriptedResponse::status(503))).await;
    let mt = SyntheticTerrain::new(64, |p: LatLon| 300.0 + 1_000.0 * (p.lat - START.lat))
        .serve(with_streets(|_, _| None))
        .await;
    let dir = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.cache.offline = false;
    config.cache.dir = dir.path().to_path_buf();
    config.elevation.ign.enabled = true;
    config.elevation.ign.wms_url = ign.wms_url.clone();
    config.elevation.ign.max_attempts = 1;
    config.elevation.mapterhorn.url = mt.url_template.clone();
    config.elevation.mapterhorn.tile_size = 64;
    config.typology.url = mt.url_template.clone();
    let app = app_with(&config);

    let route = meridian(sentinel(), 1_500.0, 4);
    let body = request_body(&route, None);
    let reply = send(app.clone(), post_json(&body)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let r = reply.json();
    assert!(r["summary"]["structures"].as_array().is_some_and(|s| !s.is_empty()), "pont traversé");

    // Panne totale de l'élévation, réponse partielle acceptée.
    let down = ScriptedServer::start(|_, _| ScriptedResponse::status(500)).await;
    let mut broken = config.clone();
    broken.elevation.ign.wms_url = down.wms_url.clone();
    broken.elevation.mapterhorn.url = down.url_template.clone();
    let body = request_body(
        &route,
        Some(json!({"accept_partial": true, "sampling": {"mode": "distance", "step_m": 5}})),
    );
    let reply = send(app_with(&broken), post_json(&body)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let reply = send(app_with(&broken), post_json(&request_body(&route, None))).await;
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);

    // Erreur de validation : une coordonnée hors bornes à côté des coordonnées sentinelles.
    let mut bad = request_body(&route, None);
    bad["points"][1]["lat"] = json!(91.0);
    assert_eq!(send(app, post_json(&bad)).await.status, StatusCode::BAD_REQUEST);

    let logs = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    let rendered = metrics.render();
    assert!(logs.lines().count() > 20, "collecteur actif : {logs}");
    assert!(logs.contains("route details"), "ligne de synthèse présente");
    assert!(rendered.contains("ord_http_requests_total"), "métriques actives");
    // Le contrôle voit bien une fuite : la coordonnée brute et la clé de tuile sont reconnues.
    let leak = format!("lat={} tile=14/{}", route[0].lat, {
        let g = Grid::new(14, 1);
        let k = g.tile_of(g.to_units(route[0]));
        format!("{}/{}", k.x, k.y)
    });
    assert!(forbidden(&route).iter().filter(|f| leak.contains(f.as_str())).count() >= 2, "{leak}");
    for fragment in forbidden(&route) {
        for (what, text) in [("logs", &logs), ("métriques", &rendered)] {
            if let Some(line) = text.lines().find(|l| l.contains(&fragment)) {
                panic!("{what} : « {fragment} » trouvé dans : {line}");
            }
        }
    }
}
