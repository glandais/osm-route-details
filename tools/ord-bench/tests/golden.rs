//! Golden hors France et GLO-30 (README technique §3, ligne 2 ; T05 §2.6), sur des ouvrages de
//! calibration du corpus public : `krk` (Mapterhorn n'y a que GLO-30 : `adapted[]` non vide,
//! `dsm_source`) et `liege-belle-liegeoise` (hors France : `outside_primary_zone`).
//!
//! Lit le cache de banc (hors ligne) : `ord-bench fill-cache` d'abord, puis
//! `cargo test -p ord-bench --test golden -- --ignored --nocapture`. `ORD_GOLDEN_UPDATE=1` réécrit
//! les snapshots `tests/golden/<id>.json`. Scénario des trois modes de sortie : `golden_three_modes`.

use ord_bench::golden::{diff, response, scenario, snapshot_path, SCENARIOS};
use ord_bench::manifest::Manifest;
use ord_bench::service::{Service, Variant};
use ord_bench::BenchPaths;
use serde_json::Value;

fn codes(r: &Value) -> Vec<&str> {
    r["quality"]["warnings"].as_array().into_iter().flatten().filter_map(|w| w["code"].as_str()).collect()
}

#[tokio::test]
#[ignore = "cache de banc requis (ord-bench fill-cache)"]
async fn golden_outside_france_and_glo30() {
    let bench = BenchPaths::repo_bench();
    let paths = BenchPaths::new(&bench, None, None);
    let manifest = Manifest::load(&bench).expect("manifest");
    let service = Service::start(&paths.cache, true, Variant::DEFAULT).await.expect("service");
    let update = std::env::var_os("ORD_GOLDEN_UPDATE").is_some();
    for id in SCENARIOS {
        let s = scenario(&paths, &manifest, id).expect("scénario");
        let r = response(&service, &s).await.expect("réponse");
        // Propriétés attendues, indépendantes du snapshot.
        assert!(codes(&r).contains(&"outside_primary_zone"), "{id} : {:?}", codes(&r));
        assert!(r["summary"]["terrain_m"].get("ign_lidar_hd").is_none(), "{id} : aucune lecture IGN");
        if id == "krk" {
            let adapted = r["summary"]["gain_params"]["adapted"].as_array().expect("adapted");
            assert!(!adapted.is_empty(), "krk : adapted[] vide");
            assert!(codes(&r).contains(&"dsm_source"));
            assert_eq!(r["summary"]["terrain_m"]["mapterhorn"], r["summary"]["length_m"]);
        }
        let path = snapshot_path(id);
        if update || !path.exists() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, serde_json::to_string_pretty(&r).unwrap() + "\n").unwrap();
            println!("{id} : snapshot écrit");
            continue;
        }
        let expected: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let mut out = Vec::new();
        diff("", &expected, &r, &mut out);
        assert!(out.is_empty(), "{id} : {out:#?}");
        println!("{id} : conforme au snapshot");
    }
}

/// Golden d'ouvrage (T05 §2.6, incrément 4) : trace qui commence sur le tablier, pont parcouru
/// deux fois, demi-tour sur un pont, sur des ouvrages de calibration.
#[tokio::test]
#[ignore = "cache de banc requis (ord-bench fill-cache)"]
async fn golden_structures() {
    use ord_bench::golden::{response_with, structure_options, structure_scenario, STRUCTURE_SCENARIOS};
    let bench = BenchPaths::repo_bench();
    let paths = BenchPaths::new(&bench, None, None);
    let manifest = Manifest::load(&bench).expect("manifest");
    let service = Service::start(&paths.cache, true, Variant::DEFAULT).await.expect("service");
    let update = std::env::var_os("ORD_GOLDEN_UPDATE").is_some();
    for id in STRUCTURE_SCENARIOS {
        let s = structure_scenario(&paths, &manifest, id).expect("scénario");
        let r = response_with(&service, &s, structure_options()).await.expect("réponse");
        let entries = r["summary"]["structures"].as_array().expect("structures");
        assert!(!entries.is_empty(), "{id} : aucun ouvrage");
        assert!(
            entries.iter().all(|e| e["kind"] == "bridge" && e["method"] == "dsm_deck"),
            "{id} : {entries:?}"
        );
        assert_eq!(r["quality"]["structure_correction"]["bridges"], "applied", "{id}");
        let deck: Vec<(f64, f64)> = r["points"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["ele_method"] == "dsm_deck")
            .map(|p| (p["d_m"].as_f64().unwrap(), p["ele"].as_f64().unwrap()))
            .collect();
        assert!(!deck.is_empty(), "{id}");
        if id == "nantes-pirmil-deux-fois" {
            // Inv. 8 : même altitude aux deux passages, à distances symétriques.
            assert_eq!(entries.len(), 2, "{entries:?}");
            let l = r["summary"]["length_m"].as_f64().unwrap();
            let at =
                |d: f64| deck.iter().min_by(|a, b| (a.0 - d).abs().total_cmp(&(b.0 - d).abs())).unwrap().1;
            let (f, t) = (entries[0]["from_m"].as_f64().unwrap(), entries[0]["to_m"].as_f64().unwrap());
            let mid = 0.5 * (f + t);
            assert!((at(mid) - at(l - mid)).abs() < 0.5, "{} vs {}", at(mid), at(l - mid));
        }
        if id == "test1-depart-sur-tablier" {
            assert_eq!(r["points"][0]["ele_method"], "dsm_deck", "premier point sur le tablier");
        }
        let path = snapshot_path(id);
        if update || !path.exists() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, serde_json::to_string_pretty(&r).unwrap() + "\n").unwrap();
            println!("{id} : snapshot écrit");
            continue;
        }
        let expected: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let mut out = Vec::new();
        diff("", &expected, &r, &mut out);
        assert!(out.is_empty(), "{id} : {out:#?}");
        println!("{id} : conforme au snapshot");
    }
}

/// Les trois modes de sortie sur le même parcours (T05 §2.6) : traversée du pont de Pirmil
/// (calibration), points d'entrée espacés autour du pont.
#[tokio::test]
#[ignore = "cache de banc requis (ord-bench fill-cache)"]
async fn golden_three_modes() {
    use ord_bench::golden::{modes_response, structure_scenario, MODES_SCENARIO};
    let bench = BenchPaths::repo_bench();
    let paths = BenchPaths::new(&bench, None, None);
    let manifest = Manifest::load(&bench).expect("manifest");
    let service = Service::start(&paths.cache, true, Variant::DEFAULT).await.expect("service");
    let s = structure_scenario(&paths, &manifest, MODES_SCENARIO).expect("scénario");
    let r = modes_response(&service, &s).await.expect("réponses");
    let pts = |m: &str| r[m]["points"].as_array().expect("points").clone();
    let (input, distance, bp) = (pts("input"), pts("distance"), pts("input_with_breakpoints"));
    assert_eq!(input.len(), 3);
    // Points d'entrée identiques en `input` et en `input_with_breakpoints`.
    let bp_inputs: Vec<&Value> = bp.iter().filter(|p| p["origin"] == "input").collect();
    assert_eq!(bp_inputs.len(), 3);
    for (a, b) in input.iter().zip(&bp_inputs) {
        assert_eq!((&a["d_m"], &a["ele"], &a["index"]), (&b["d_m"], &b["ele"], &b["index"]));
    }
    // Bornes du pont portées par des points insérés.
    let entry = &r["input_with_breakpoints"]["summary"]["structures"][0];
    for (k, reason) in [("from_m", "structure_start"), ("to_m", "structure_end")] {
        assert!(
            bp.iter().any(|p| p["d_m"] == entry[k]
                && p["reasons"].as_array().is_some_and(|x| x.contains(&reason.into()))),
            "{k}"
        );
    }
    // Même résumé dans les trois modes ; `distance` au pas de 25 m, sans origine.
    assert_eq!(r["input"]["summary"], r["distance"]["summary"]);
    assert_eq!(r["input"]["summary"], r["input_with_breakpoints"]["summary"]);
    assert!(distance.iter().all(|p| p.get("origin").is_none()));
    assert!(r.to_string().find("\"lat\"").is_none(), "aucune coordonnée dans le snapshot");
    let path = snapshot_path(MODES_SCENARIO);
    if std::env::var_os("ORD_GOLDEN_UPDATE").is_some() || !path.exists() {
        std::fs::write(&path, serde_json::to_string_pretty(&r).unwrap() + "\n").unwrap();
        println!("{MODES_SCENARIO} : snapshot écrit");
        return;
    }
    let expected: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut out = Vec::new();
    diff("", &expected, &r, &mut out);
    assert!(out.is_empty(), "{out:#?}");
    println!("{MODES_SCENARIO} : conforme au snapshot");
}

/// Série de tunnels (règles 2026.10.10, T04 §3.1) : tunnels des Petits Goulets (calibration, `test4`)
/// séparés par des balcons de moins de 120 m, une droite entre les portails extérieurs ; les balcons
/// restent des routes (ni segment ni entrée d'ouvrage inventés), leur profil suit la droite.
#[tokio::test]
#[ignore = "cache de banc requis (ord-bench fill-cache)"]
async fn golden_tunnel_series() {
    use ord_bench::golden::{response_with, structure_options, structure_scenario, SERIES_SCENARIO};
    let bench = BenchPaths::repo_bench();
    let paths = BenchPaths::new(&bench, None, None);
    let manifest = Manifest::load(&bench).expect("manifest");
    let service = Service::start(&paths.cache, true, Variant::DEFAULT).await.expect("service");
    let s = structure_scenario(&paths, &manifest, SERIES_SCENARIO).expect("scénario");
    let mut r = response_with(&service, &s, structure_options()).await.expect("réponse");
    for p in r["points"].as_array_mut().into_iter().flatten() {
        if let Some(m) = p.as_object_mut() {
            m.remove("lat");
            m.remove("lon");
        }
    }
    let entries = r["summary"]["structures"].as_array().expect("structures").clone();
    assert_eq!(entries.len(), 4, "{entries:?}");
    assert!(entries.iter().all(|e| e["kind"] == "tunnel" && e["method"] == "interpolated_structure"));
    assert_eq!(r["quality"]["structure_correction"]["tunnels"], "applied");
    let span = |e: &Value| (e["from_m"].as_f64().unwrap(), e["to_m"].as_f64().unwrap());
    let (first, last) = (span(&entries[0]).0, span(&entries[3]).1);
    // Balcons : entre deux entrées, aucun segment d'ouvrage.
    let balconies: Vec<(f64, f64)> = entries.windows(2).map(|w| (span(&w[0]).1, span(&w[1]).0)).collect();
    for seg in r["segments"].as_array().expect("segments") {
        let (f, t) = (seg["from_m"].as_f64().unwrap(), seg["to_m"].as_f64().unwrap());
        for &(a, b) in &balconies {
            if f < b - 1.0 && t > a + 1.0 {
                assert_eq!(seg["structure"], "ground", "balcon typé ouvrage : {seg}");
            }
        }
    }
    // Profil : une droite de portail extérieur à portail extérieur, balcons compris.
    let pts: Vec<(f64, f64, String)> = r["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (p["d_m"].as_f64().unwrap(), p["ele"].as_f64().unwrap(), p["ele_method"].as_str().unwrap().into())
        })
        .filter(|p| p.0 > first && p.0 < last)
        .collect();
    assert!(pts.iter().all(|p| p.2 == "interpolated_structure"), "{pts:?}");
    let (a, b) = (&pts[0], &pts[pts.len() - 1]);
    for p in &pts {
        let line = a.1 + (b.1 - a.1) * (p.0 - a.0) / (b.0 - a.0);
        assert!((p.1 - line).abs() < 0.05, "{} : {} ≠ {line}", p.0, p.1);
    }
    let path = snapshot_path(SERIES_SCENARIO);
    if std::env::var_os("ORD_GOLDEN_UPDATE").is_some() || !path.exists() {
        std::fs::write(&path, serde_json::to_string_pretty(&r).unwrap() + "\n").unwrap();
        println!("{SERIES_SCENARIO} : snapshot écrit");
        return;
    }
    let expected: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut out = Vec::new();
    diff("", &expected, &r, &mut out);
    assert!(out.is_empty(), "{out:#?}");
    println!("{SERIES_SCENARIO} : conforme au snapshot");
}

/// Raccord entre un pont OSM et son viaduc d'accès non tagué détecté (règles 2026.11.2, T04 §4.3
/// bis) : `test1` de vcyclist, fin nord du pont de Saint-Nazaire (calibration). Le profil du
/// tablier est continu : aucune station corrigée ne plonge vers la sonde au sol du pont OSM.
#[tokio::test]
#[ignore = "cache de banc requis (ord-bench fill-cache)"]
async fn golden_heuristic_junction() {
    use ord_bench::golden::{response_with, structure_options, structure_scenario, HEURISTIC_SCENARIO};
    let bench = BenchPaths::repo_bench();
    let paths = BenchPaths::new(&bench, None, None);
    let manifest = Manifest::load(&bench).expect("manifest");
    let service = Service::start(&paths.cache, true, Variant::DEFAULT).await.expect("service");
    let id = HEURISTIC_SCENARIO;
    let s = structure_scenario(&paths, &manifest, id).expect("scénario");
    let r = response_with(&service, &s, structure_options()).await.expect("réponse");
    let entries = r["summary"]["structures"].as_array().expect("structures");
    assert!(entries.iter().any(|e| e["structure_source"] == "heuristic"), "{entries:?}");
    let deck: Vec<f64> = r["points"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["ele_method"] == "dsm_deck")
        .map(|p| p["ele"].as_f64().unwrap())
        .collect();
    assert!(deck.len() > 10, "{id}");
    // Pas de 10 m : un tablier plafonné à 10 % ne varie pas de plus de 1 m d'un point au suivant
    // (marge : 3 m).
    let jump = deck.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f64::max);
    assert!(jump < 3.0, "{id} : saut de {jump} m sur le tablier");
    let path = snapshot_path(id);
    if std::env::var_os("ORD_GOLDEN_UPDATE").is_some() || !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string_pretty(&r).unwrap() + "\n").unwrap();
        println!("{id} : snapshot écrit");
        return;
    }
    let expected: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut out = Vec::new();
    diff("", &expected, &r, &mut out);
    assert!(out.is_empty(), "{id} : {out:#?}");
}

/// Galerie pare-avalanche absente du schéma des tuiles (T04 §4.2 bis) : `arly-galerie`, témoin
/// `tt08-arly-cliets` de vcyclist (calibration). Le plateau du MNT est détecté et corrigé en
/// altitude seule (règles 2026.11.5) : droite entre portails, `gap_filled`,
/// `elevation_cover_corrected`, sans ouvrage.
#[tokio::test]
#[ignore = "cache de banc requis (ord-bench fill-cache)"]
async fn golden_gallery() {
    use ord_bench::golden::{response_with, structure_options, structure_scenario, GALLERY_SCENARIO};
    let bench = BenchPaths::repo_bench();
    let paths = BenchPaths::new(&bench, None, None);
    let manifest = Manifest::load(&bench).expect("manifest");
    let service = Service::start(&paths.cache, true, Variant::DEFAULT).await.expect("service");
    let id = GALLERY_SCENARIO;
    let s = structure_scenario(&paths, &manifest, id).expect("scénario");
    let mut r = response_with(&service, &s, structure_options()).await.expect("réponse");
    for p in r["points"].as_array_mut().into_iter().flatten() {
        if let Some(m) = p.as_object_mut() {
            m.remove("lat");
            m.remove("lon");
        }
    }
    // Altitude seule (règles 2026.11.5) : aucun ouvrage, segments au sol.
    let entries = r["summary"]["structures"].as_array().expect("structures").clone();
    assert!(entries.is_empty(), "{entries:?}");
    assert!(r["segments"].as_array().unwrap().iter().all(|g| g["structure"] == "ground"));
    assert!(codes(&r).contains(&"elevation_cover_corrected"), "{:?}", codes(&r));
    assert!(!codes(&r).contains(&"structure_untagged"), "{:?}", codes(&r));
    assert!(r["points"].as_array().unwrap().iter().any(|p| p["ele_method"] == "gap_filled"));
    // Profil : aucune marche dans la galerie, pente de route (≤ 15 %) d'un point au suivant.
    let pts: Vec<(f64, f64)> = r["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["d_m"].as_f64().unwrap(), p["ele"].as_f64().unwrap()))
        .collect();
    let steepest = pts.windows(2).map(|w| ((w[1].1 - w[0].1) / (w[1].0 - w[0].0)).abs()).fold(0.0, f64::max);
    assert!(steepest <= 0.15, "{id} : pente de {steepest}");
    let path = snapshot_path(id);
    if std::env::var_os("ORD_GOLDEN_UPDATE").is_some() || !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string_pretty(&r).unwrap() + "\n").unwrap();
        println!("{id} : snapshot écrit");
        return;
    }
    let expected: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut out = Vec::new();
    diff("", &expected, &r, &mut out);
    assert!(out.is_empty(), "{id} : {out:#?}");
}
