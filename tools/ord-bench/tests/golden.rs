//! Golden hors France et GLO-30 (README technique §3, ligne 2 ; T05 §2.6), sur des ouvrages de
//! calibration du corpus public : `krk` (Mapterhorn n'y a que GLO-30 : `adapted[]` non vide,
//! `dsm_source`) et `liege-belle-liegeoise` (hors France : `outside_primary_zone`).
//!
//! Lit le cache de banc (hors ligne) : `ord-bench fill-cache` d'abord, puis
//! `cargo test -p ord-bench --test golden -- --ignored --nocapture`. `ORD_GOLDEN_UPDATE=1` réécrit
//! les snapshots `tests/golden/<id>.json`.

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
