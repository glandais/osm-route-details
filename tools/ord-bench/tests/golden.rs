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
