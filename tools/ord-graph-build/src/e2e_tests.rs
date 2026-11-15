//! `build` puis `measure` sur le PBF minuscule de `ord-graph` : toute la chaîne, sans gros fichier.

use std::path::PathBuf;

use crate::build_cmd::{self, BuildArgs};
use crate::measure::{self, MeasureArgs};

fn mini_pbf() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/ord-graph/tests/data/mini.osm.pbf")
}

#[test]
fn build_then_measure_on_the_tiny_pbf() {
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

    let out = dir.path().join("measure.json");
    let m = measure::run(&MeasureArgs {
        graph: report.output.clone(),
        starts: 3,
        seed: 1,
        radii: vec![5.0],
        poly: None,
        state: "dense".into(),
        warm_runs: 3,
        flat: true,
        out: Some(out.clone()),
    })
    .expect("measure");
    drop(m);
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap();
    assert_eq!(json["graph_stats"]["edges"], 4);
    assert_eq!(json["radii"][0]["all"]["starts"], 3);
    // La référence à plat a tourné et fixé les mêmes états que le lecteur (sinon `measure` échoue).
    assert_eq!(json["radii"][0]["all"]["flat_ms"]["n"], 3);
    // Sans `.poly`, aucun départ n'est dit intérieur.
    assert_eq!(json["radii"][0]["interior"]["starts"], 0);
    assert_eq!(json["radii"][0]["interior"]["unreliable"], true);
}
