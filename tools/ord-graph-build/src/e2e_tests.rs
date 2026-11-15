//! `build`, `views` puis `measure` sur le PBF minuscule de `ord-graph` : toute la chaîne, sans gros fichier.

use std::path::PathBuf;

use crate::build_cmd::{self, BuildArgs};
use crate::measure::{self, MeasureArgs};
use crate::views_cmd::{self, ViewsArgs};

fn mini_pbf() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/ord-graph/tests/data/mini.osm.pbf")
}

#[test]
fn build_views_then_measure_on_the_tiny_pbf() {
    let dir = tempfile::tempdir().unwrap();
    let report = build_cmd::run(&BuildArgs {
        pbf: mini_pbf(),
        out_dir: Some(dir.path().to_path_buf()),
        region: None,
        zoom: 11,
        threads: None,
        report: Some(dir.path().join("build.json")),
    })
    .expect("build");
    assert_eq!((report.ways_total, report.ways_kept, report.edges), (5, 3, 4));
    assert_eq!(report.output.file_name().unwrap().to_str().unwrap(), "mini-inconnue-z11.ordg");
    assert!(report.phases.iter().any(|p| p.name.starts_with("écriture")));
    assert!(dir.path().join("build.json").exists());

    let names = ["road", "gravel", "all", "road-prune"];
    let built = views_cmd::run(&ViewsArgs {
        graph: report.output.clone(),
        views: names.iter().map(|s| (*s).to_string()).collect(),
        out_dir: None,
        report: Some(dir.path().join("views.json")),
    })
    .expect("views");
    assert_eq!(built.views.len(), 4);
    assert!(built.views.iter().all(|v| v.file.exists() && v.file_bytes > 0));
    // `all` ne filtre rien : tout le graphe de base est couvert.
    assert_eq!(built.views[2].base_edges_covered, 4);

    let out = dir.path().join("measure.json");
    let m = measure::run(&MeasureArgs {
        graph: report.output.clone(),
        starts: 3,
        seed: 1,
        radii: vec![5.0],
        poly: None,
        state: vec!["map".into(), "dense".into(), "local".into()],
        views: names.iter().map(|s| (*s).to_string()).collect(),
        view_dir: None,
        warm_runs: 3,
        flat: true,
        check: true,
        check_starts: 3,
        skip_timing: false,
        out: Some(out.clone()),
    })
    .expect("measure");
    drop(m);
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap();
    let targets = json["targets"].as_array().unwrap();
    assert_eq!(targets.len(), 5);
    assert_eq!(targets[0]["name"], "base");
    assert_eq!(targets[0]["graph_stats"]["edges"], 4);
    assert_eq!(targets[0]["states"].as_array().unwrap().len(), 3);
    let dense = &targets[0]["states"][1];
    assert_eq!(dense["state"], "dense");
    assert_eq!(dense["radii"][0]["all"]["starts"], 3);
    // La référence à plat a tourné et fixé les mêmes états que le lecteur (sinon `measure` échoue).
    assert_eq!(dense["radii"][0]["all"]["flat_ms"]["n"], 3);
    // Sans `.poly`, aucun départ n'est dit intérieur.
    assert_eq!(dense["radii"][0]["interior"]["starts"], 0);
    assert_eq!(dense["radii"][0]["interior"]["unreliable"], true);
    // Le contrôle a tourné sur chaque vue et sur la base.
    assert_eq!(json["check"]["ok"], true);
    assert_eq!(json["check"]["states_agree_on"].as_array().unwrap().len(), 5);
    // L'état local est plus petit que le tableau dense du graphe entier (ou égal : graphe minuscule).
    let local = &targets[0]["states"][2]["radii"][0]["all"]["state_mb"]["median"];
    assert!(local.as_f64().unwrap() > 0.0);
}
