//! Rien de privé ne sort d'une retouche de parcours (api 1.17), sur le modèle de `router_sentinel.rs` : des retouches (avec et
//! sans équipe, réussies, refusées, refusées par le contrôleur) traversent le service sous un collecteur de logs JSON (`trace`
//! pour le service) et un exporteur de métriques. Aucune ligne, aucun span, aucune métrique ne contient une coordonnée (brute
//! ou arrondie), un fragment de la polyligne ou du document, la distance visée, la clé d'environnement, la référence
//! d'équipe, ni les noms dérivés.
//!
//! Binaire de test à part : le collecteur et l'exporteur sont globaux au processus.

mod common;

use std::io::Write;
use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use common::generator::*;
use common::*;
use metrics_exporter_prometheus::PrometheusBuilder;
use ord_graph_match::geo::encode_polyline6;
use ord_server::generator::crypto::{decode_key, Credentials};
use serde_json::json;

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_edit_leaves_no_coordinate_document_key_or_team_in_logs_or_metrics() {
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
    let state = ord_server::AppState::new(&config).unwrap();
    let app = ord_server::router(state.clone());
    let (k, team) = (key(7), "team-sentinel-reference");
    assert_eq!(
        send(app.clone(), put_digest(&k, team, "digest-sentinel", team_deposit())).await.status,
        StatusCode::OK
    );

    // Un parcours dont chaque sommet porte des décimales reconnaissables (décalé de 0,000 003°, à moins d'un mètre de la rue).
    let path = square(6, 6, 6);
    let shifted: Vec<[f64; 2]> =
        path_points(&path).iter().map(|p| [p[0] + 0.000_003, p[1] + 0.000_004]).collect();
    let mut body = edit_body(&path, json!({"resize": {"distance_m": 8_640.123_4}}), Some(0.25));
    body["polyline"] = json!(encode_polyline6(&shifted));
    body["network_edges"]["marqueur"] = json!("secret-document-marker");
    let secret_polyline = body["polyline"].as_str().unwrap().to_owned();
    let secret_edge = body["network_edges"]["edges"][2]["g"].as_str().unwrap().to_owned();

    let ok = send(app.clone(), edit(Some(&k), Some(team), &body)).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    assert_eq!(ok.json()["mode"], "team_first");
    // La même, sans équipe ; le déplacement de l'arrivée, avec un point reconnaissable.
    assert_eq!(send(app.clone(), edit(None, None, &body)).await.status, StatusCode::OK);
    let mut path_l = row(10, 2, 14);
    path_l.extend((11..=16).map(|r| (r, 14)));
    let end = [node(16, 17)[0] + 0.000_000_7, node(16, 17)[1] + 0.000_000_3];
    let mut moved = edit_body(&path_l, json!({"move_end": {"point": {"lat": end[0], "lon": end[1]}}}), None);
    moved["network_edges"]["marqueur"] = json!("secret-document-marker");
    let ok = send(app.clone(), edit(Some(&k), Some(team), &moved)).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    // Un refus du moteur (rien à raccourcir), hors couverture, une arrivée trop loin, un document illisible, une clé invalide.
    let straight = edit_body(&row(10, 2, 18), json!({"resize": {"distance_m": 3_600.123_4}}), None);
    assert_eq!(send(app.clone(), edit(None, None, &straight)).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let far = [node(10, 14)[0] + 0.5123456, node(10, 14)[1]];
    let mut out = moved.clone();
    out["operation"] = json!({"move_end": {"point": {"lat": far[0], "lon": far[1]}}});
    assert_eq!(send(app.clone(), edit(None, None, &out)).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let mut broken = body.clone();
    broken["network_edges"] = json!({"edges": []});
    assert_eq!(send(app.clone(), edit(None, None, &broken)).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let mut unreadable = body.clone();
    unreadable["network_edges"] = json!("secret-document-marker");
    assert_eq!(send(app.clone(), edit(None, None, &unreadable)).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        send(app.clone(), edit(Some("pas-une-cle"), Some(team), &body)).await.status,
        StatusCode::BAD_REQUEST
    );
    // Un tronçon refusé par le contrôleur : sa ligne `warn` ne porte rien de la requête.
    let hooks = state.generator().hooks().unwrap();
    hooks.corrupt_routes.store(true, std::sync::atomic::Ordering::Relaxed);
    let refused = send(app.clone(), edit(Some(&k), Some(team), &body)).await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", refused.text);
    hooks.corrupt_routes.store(false, std::sync::atomic::Ordering::Relaxed);

    let logs = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    let rendered = metrics.render();
    assert!(logs.contains("route edit"), "ligne de synthèse d'une retouche");
    assert!(logs.contains("edited route rejected by the independent checker"), "ligne d'un tronçon refusé");
    for m in [
        "ord_generator_edit_requests_total",
        "ord_generator_edit_duration_seconds",
        "ord_generator_edit_stage_seconds",
        "ord_generator_edit_routes_total",
        "ord_generator_edit_routes_rejected_total",
    ] {
        assert!(rendered.contains(m), "{m}");
    }
    assert!(rendered.contains("reason=\"no_economy\""), "le refus normal est compté sous son nom");
    // Une retouche n'incrémente pas les tableaux des générations.
    assert!(!rendered.contains("ord_generator_requests_total"));

    let creds = Credentials::derive(&decode_key(&k).unwrap(), team);
    let mut forbidden = vec![
        k.clone(),
        team.to_owned(),
        "digest-sentinel".to_owned(),
        creds.ns.clone(),
        creds.team.clone(),
        "secret-document-marker".to_owned(),
        secret_polyline[..16].to_owned(),
        secret_edge[..12].to_owned(),
    ];
    let mut values: Vec<f64> = shifted.iter().take(8).flat_map(|p| [p[0], p[1]]).collect();
    values.extend([end[0], end[1], far[0], 8_640.123_4, 3_600.123_4]);
    for v in values {
        number(&mut forbidden, v);
    }
    // Le contrôle voit bien une fuite.
    let leak = format!("lat={} key={k}", shifted[0][0]);
    assert!(forbidden.iter().filter(|f| leak.contains(f.as_str())).count() >= 2);
    for fragment in &forbidden {
        for (what, text) in [("logs", &logs), ("métriques", &rendered)] {
            if let Some(line) = text.lines().find(|l| l.contains(fragment.as_str())) {
                panic!("{what} : « {fragment} » trouvé dans : {line}");
            }
        }
    }
    assert!(!rendered.contains("ns8"), "pas d'espace dans les métriques");
}
