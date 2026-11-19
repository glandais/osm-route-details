//! Les attracteurs OSM du générateur (lot 4, 4d, §17.13 tests 22 et 23) : fichiers `.orda` posés à côté des vues,
//! capacités, réglage, et ce que la génération envoie au moteur. Graphe, vues et fichiers **synthétiques**.

mod common;

use std::path::Path;

use axum::http::StatusCode;
use axum::Router;
use common::generator::*;
use common::*;
use ord_graph::attract::{build_attr, write_attr, AttrWrite, Object};
use ord_graph::format::GraphFile;
use ord_server::config::{Config, GeneratorAttractors};
use ord_server::{router, AppState};
use serde_json::Value;

/// Écrit `<nom>.road.orda` et `<nom>.gravel.orda` à côté des vues du graphe `files` : des objets semés sur la grille.
fn write_attractors(files: &Files) {
    let base = GraphFile::open(std::fs::read(&files.graph).unwrap()).unwrap();
    for view in [&files.road, &files.gravel] {
        let v = GraphFile::open(std::fs::read(view).unwrap()).unwrap();
        let objs: Vec<Object> = (0..40)
            .map(|i| {
                let p = node(3 + (i * 7) % 15, 3 + (i * 11) % 15);
                Object {
                    cat: (i % 7) as u8,
                    lat: (p[0] * 1e7) as i32,
                    lon: (p[1] * 1e7) as i32,
                    kind: 0,
                    id: i as i64,
                }
            })
            .collect();
        let mut objs = objs;
        objs.sort_unstable();
        let data = build_attr(&v, &objs, 300, 300);
        let w = AttrWrite { snap_m: 300, reach_m: 300, meta: b"{}", built_epoch: 0 };
        write_attr(&view.with_extension("orda"), base.header(), &v, &data, &w)
            .expect("fichier d'attracteurs");
    }
}

struct World {
    state: AppState,
    app: Router,
}

impl World {
    fn new(dir: &Path, files: &Files, tweak: impl FnOnce(&mut Config)) -> Self {
        let mut config = config_with(test_config(), dir, files);
        tweak(&mut config);
        let state = AppState::new(&config).expect("état");
        let app = router(state.clone());
        World { state, app }
    }

    async fn attractors(&self) -> Value {
        let c = send(self.app.clone(), get("/v1/capabilities")).await;
        assert_eq!(c.status, StatusCode::OK, "{}", c.text);
        c.json()["generator"]["attractors"].clone()
    }

    /// Une génération en découverte ; rend les poids que le service a posés dans la requête du moteur.
    async fn weights(&self, ride: &str) -> ord_loops::AttrWeights {
        let mut body = generate_body(1.0);
        body["ride_type"] = ride.into();
        let r = send(self.app.clone(), generate(None, None, &body)).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text);
        let hooks = self.state.generator().hooks().expect("prêt");
        let w = *hooks.last_attr_weights.lock().unwrap();
        w.expect("une requête a été envoyée au moteur")
    }
}

/// Test 22 : fichiers présents, absents, refusés ; le générateur sert dans tous les cas, avec la raison dans les capacités.
#[tokio::test]
async fn the_capabilities_say_whether_the_attractor_files_are_available_and_why_not() {
    let dir = tempfile::tempdir().unwrap();
    let files = write_files(dir.path(), "g1", 21);

    // Sans fichier : indisponibles (`missing`), le générateur sert quand même, aucun poids.
    let w = World::new(dir.path(), &files, |c| c.generator.attractors = GeneratorAttractors::On);
    let a = w.attractors().await;
    assert_eq!(
        (a["available"].as_bool(), a["enabled"].as_bool(), a["reason"].as_str()),
        (Some(false), Some(false), Some("missing"))
    );
    assert!(w.weights("road").await.is_none());

    // Les deux fichiers : disponibles ; `off` par défaut, `on` à la demande.
    write_attractors(&files);
    let off = World::new(dir.path(), &files, |_| {});
    let a = off.attractors().await;
    assert_eq!((a["available"].as_bool(), a["enabled"].as_bool()), (Some(true), Some(false)));
    assert!(a.get("reason").is_none() || a["reason"].is_null());
    let on = World::new(dir.path(), &files, |c| c.generator.attractors = GeneratorAttractors::On);
    let a = on.attractors().await;
    assert_eq!((a["available"].as_bool(), a["enabled"].as_bool()), (Some(true), Some(true)));

    // Un fichier illisible (octets quelconques) : `unreadable`.
    let road_orda = files.road.with_extension("orda");
    let good = std::fs::read(&road_orda).unwrap();
    std::fs::remove_file(&road_orda).unwrap();
    std::fs::write(&road_orda, b"not an attractors file at all, but long enough to have a header....")
        .unwrap();
    let w = World::new(dir.path(), &files, |c| c.generator.attractors = GeneratorAttractors::On);
    assert_eq!(w.attractors().await["reason"], "unreadable");
    assert_eq!(w.attractors().await["enabled"], false);
    assert!(w.weights("road").await.is_none());

    // Le fichier d'un autre graphe (un graphe de 20 × 20 au lieu de 21 × 21) : `mismatch`.
    let other = write_files(dir.path(), "g2", 20);
    write_attractors(&other);
    std::fs::remove_file(&road_orda).unwrap();
    std::fs::copy(other.road.with_extension("orda"), &road_orda).unwrap();
    let w = World::new(dir.path(), &files, |c| c.generator.attractors = GeneratorAttractors::On);
    let a = w.attractors().await;
    assert_eq!((a["available"].as_bool(), a["reason"].as_str()), (Some(false), Some("mismatch")));
    assert!(w.weights("gravel").await.is_none());

    // Un seul fichier bon ne suffit pas : l'autre vue n'a pas le sien, les attracteurs restent éteints.
    std::fs::remove_file(&road_orda).unwrap();
    std::fs::write(&road_orda, &good).unwrap();
    std::fs::remove_file(files.gravel.with_extension("orda")).unwrap();
    let w = World::new(dir.path(), &files, |c| c.generator.attractors = GeneratorAttractors::On);
    let a = w.attractors().await;
    assert_eq!((a["available"].as_bool(), a["reason"].as_str()), (Some(false), Some("missing")));
    assert!(w.weights("road").await.is_none(), "un seul fichier : aucun poids");

    // Chemins explicites : un fichier ailleurs que « à côté de la vue » est lu (les variables d'environnement du déploiement).
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    write_attractors(&files);
    for (view, name) in [(&files.road, "r.orda"), (&files.gravel, "g.orda")] {
        std::fs::rename(view.with_extension("orda"), elsewhere.join(name)).unwrap();
    }
    let w = World::new(dir.path(), &files, |c| {
        c.generator.attractors = GeneratorAttractors::On;
        c.generator.attractors_road = Some(elsewhere.join("r.orda"));
        c.generator.attractors_gravel = Some(elsewhere.join("g.orda"));
    });
    assert_eq!(w.attractors().await["enabled"], true);
}

/// Test 23 : `off` ou fichier absent, la requête porte `NONE` ; `on`, les poids généraux du type.
#[tokio::test]
async fn a_request_carries_the_general_weights_of_its_type_only_when_the_attractors_are_on() {
    let dir = tempfile::tempdir().unwrap();
    let files = write_files(dir.path(), "g1", 21);
    write_attractors(&files);
    let off = World::new(dir.path(), &files, |_| {});
    for ride in ["road", "gravel", "mtb"] {
        assert!(off.weights(ride).await.is_none(), "`off` : aucun poids ({ride})");
    }
    let on = World::new(dir.path(), &files, |c| c.generator.attractors = GeneratorAttractors::On);
    assert_eq!(on.weights("road").await, ord_loops::AttrWeights::general(ord_loops::RideType::Road));
    assert_eq!(on.weights("gravel").await, ord_loops::AttrWeights::general(ord_loops::RideType::Gravel));
    assert!(on.weights("mtb").await.is_none(), "le VTT n'a aucun poids");
    // Une génération en classique ne lit jamais les attracteurs (le moteur ne les lit qu'en organique) : la réponse est celle de
    // toujours.
    let mut body = generate_body(1.0);
    body["figures"] = "classic".into();
    let r = send(on.app.clone(), generate(None, None, &body)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["figures"], "classic");
}
