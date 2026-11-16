//! Rien de privé ne sort du générateur (note lot 3a §10) : un dépôt, un état, des générations (dont une dont un
//! parcours est refusé par le contrôleur) et des effacements traversent le service sous un collecteur de logs JSON
//! (`trace` pour le service) et un exporteur de métriques. Aucune ligne, aucun span, aucune métrique ne doit contenir
//! la coordonnée de départ (brute ou arrondie), la clé d'environnement, la référence d'équipe, ni les noms dérivés
//! (dossier, fichier).
//!
//! Binaire de test à part : le collecteur et l'exporteur sont globaux au processus.

mod common;

use std::io::Write;
use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use common::generator::*;
use common::*;
use metrics_exporter_prometheus::PrometheusBuilder;
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
async fn generator_leaves_no_coordinate_key_or_team_in_logs_or_metrics() {
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
    // Départ sentinelle : décimales reconnaissables, à moins d'un mètre du carrefour central.
    let c = node(10, 10);
    let (lat, lon) = (c[0] + 0.000_000_7, c[1] + 0.000_000_3);
    let body =
        json!({"start": {"lat": lat, "lon": lon}, "distance_m": 3600.0, "ride_type": "road", "cursor": 0.5});

    assert_eq!(
        send(app.clone(), put_digest(&k, team, "digest-sentinel", team_deposit())).await.status,
        StatusCode::OK
    );
    assert_eq!(send(app.clone(), status(&k, team)).await.status, StatusCode::OK);
    let ok = send(app.clone(), generate(Some(&k), Some(team), &body)).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    // Un parcours refusé par le contrôleur : sa ligne `warn` ne porte aucune coordonnée.
    let hooks = state.generator().hooks().unwrap();
    hooks.corrupt_routes.store(true, std::sync::atomic::Ordering::Relaxed);
    let refused = send(app.clone(), generate(Some(&k), Some(team), &body)).await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", refused.text);
    hooks.corrupt_routes.store(false, std::sync::atomic::Ordering::Relaxed);
    // Hors couverture, clé invalide, équipe inconnue, dépôt invalide, effacements.
    let mut out = body.clone();
    out["start"] = json!({"lat": 48.5123456, "lon": 2.3123456});
    out["cursor"] = json!(1.0);
    assert_eq!(send(app.clone(), generate(None, None, &out)).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        send(app.clone(), generate(Some("pas-une-cle"), Some(team), &body)).await.status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(app.clone(), generate(Some(&key(8)), Some(team), &body)).await.status,
        StatusCode::CONFLICT
    );
    assert_eq!(send(app.clone(), put(&k, team, "{pas du json\n")).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(send(app.clone(), delete(&k, team)).await.status, StatusCode::NO_CONTENT);
    assert_eq!(send(app.clone(), delete_all(&k)).await.status, StatusCode::NO_CONTENT);

    let logs = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    let rendered = metrics.render();
    assert!(logs.lines().count() > 10, "collecteur actif : {logs}");
    assert!(logs.contains("route generation"), "ligne de synthèse d'une génération");
    assert!(logs.contains("overlay deposited"), "ligne de synthèse d'un dépôt");
    assert!(logs.contains("rejected by the independent checker"), "ligne d'un parcours refusé");
    assert!(rendered.contains("ord_generator_requests_total"), "métriques actives");
    assert!(rendered.contains("ord_generator_routes_rejected_total"), "refus comptés");
    assert!(rendered.contains("ord_overlay_deposits_total"));

    let creds = Credentials::derive(&decode_key(&k).unwrap(), team);
    let mut forbidden = vec![
        k.clone(),
        team.to_owned(),
        "digest-sentinel".to_owned(),
        creds.ns.clone(),
        creds.team.clone(),
        ".ordo.enc".to_owned(),
    ];
    for v in [lat, lon, 48.5123456, 2.3123456] {
        number(&mut forbidden, v);
    }
    // Le contrôle voit bien une fuite.
    let leak = format!("lat={lat} key={k}");
    assert!(forbidden.iter().filter(|f| leak.contains(f.as_str())).count() >= 2);
    for fragment in &forbidden {
        for (what, text) in [("logs", &logs), ("métriques", &rendered)] {
            if let Some(line) = text.lines().find(|l| l.contains(fragment.as_str())) {
                panic!("{what} : « {fragment} » trouvé dans : {line}");
            }
        }
    }
    // L'étiquette d'espace n'est pas une étiquette de métrique.
    assert!(!rendered.contains("ns8"), "pas d'espace dans les métriques");
}
