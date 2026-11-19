//! `build`, `views` puis `measure` sur le PBF minuscule de `ord-graph` : toute la chaîne, sans gros fichier.

use std::path::PathBuf;

use crate::attractors_cmd::{self, AttractorsArgs};
use crate::build_cmd::{self, BuildArgs};
use crate::measure::{self, MeasureArgs};
use crate::views_cmd::{self, ViewsArgs};

/// Fin de l'en-tête d'un `.ordg` : `build_epoch` et la somme d'en-tête y sont, la table des sections suit.
const HEADER_END: usize = 256;

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
        external: false,
        mem_budget_mb: 1024,
        tmp_dir: None,
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
        external: false,
        mem_budget_mb: 1024,
        tmp_dir: None,
    })
    .expect("views");
    assert_eq!(built.views.len(), 4);
    // Hors mémoire : mêmes octets pour les vues sans élagage ; `-prune` refusé.
    let ext_dir = dir.path().join("ext");
    let ext = views_cmd::run(&ViewsArgs {
        graph: report.output.clone(),
        views: names[..3].iter().map(|s| (*s).to_string()).collect(),
        out_dir: Some(ext_dir.clone()),
        report: None,
        external: true,
        mem_budget_mb: 1,
        tmp_dir: None,
    })
    .expect("views --external");
    for (m, e) in built.views.iter().zip(&ext.views) {
        let (mb, eb) = (std::fs::read(&m.file).unwrap(), std::fs::read(&e.file).unwrap());
        // Deux lancements, deux dates de construction : seuls les octets de l'en-tête diffèrent.
        assert_eq!(mb.len(), eb.len(), "vue {}", m.name);
        assert_eq!(&mb[HEADER_END..], &eb[HEADER_END..], "vue {} : corps différent", m.name);
        assert!(e.tmp_peak_bytes.is_some() && e.runs.is_some());
    }
    let pruned = views_cmd::run(&ViewsArgs {
        graph: report.output.clone(),
        views: vec!["road-prune".into()],
        out_dir: Some(ext_dir),
        report: None,
        external: true,
        mem_budget_mb: 1,
        tmp_dir: None,
    });
    assert!(pruned.unwrap_err().to_string().contains("-prune"));
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

fn attr_pbf() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/ord-graph/tests/data/attr.osm.pbf")
}

#[test]
fn attractors_end_to_end_on_the_tiny_pbf_and_refuse_another_extract() {
    let dir = tempfile::tempdir().unwrap();
    let report = build_cmd::run(&BuildArgs {
        pbf: attr_pbf(),
        out_dir: Some(dir.path().to_path_buf()),
        region: Some("attr".into()),
        zoom: 11,
        threads: None,
        report: None,
        external: false,
        mem_budget_mb: 1024,
        tmp_dir: None,
    })
    .expect("build");
    views_cmd::run(&ViewsArgs {
        graph: report.output.clone(),
        views: vec!["road".into(), "gravel".into()],
        out_dir: None,
        report: None,
        external: false,
        mem_budget_mb: 1024,
        tmp_dir: None,
    })
    .expect("views");
    let args = |pbf: PathBuf, out: Option<PathBuf>| AttractorsArgs {
        pbf,
        graph: report.output.clone(),
        views: vec!["road".into(), "gravel".into()],
        views_dir: None,
        out_dir: out,
        snap_m: 300,
        reach_m: 300,
        check: true,
        report: Some(dir.path().join("attractors.json")),
    };
    let r = attractors_cmd::run(&args(attr_pbf(), None)).expect("attractors --check");
    assert_eq!(r.views.len(), 2);
    assert!(r.views.iter().all(|v| v.checked
        && v.file.exists()
        && v.anchors > 0
        && v.zone_nodes > 0
        && v.zone_nodes <= v.view_nodes));
    // Six objets : café, point de vue et sommet (même nœud), village, col, boulangerie en bâtiment.
    assert_eq!((r.objects, r.ways_tagged), (6, 1));
    assert!(dir.path().join("attractors.json").exists());
    assert_eq!(r.views[0].file.file_name().unwrap().to_str().unwrap(), "attr-inconnue-z11.road.orda");
    let road = ord_graph::attract::AttrFile::open_mmap(&r.views[0].file).unwrap();
    let base = ord_graph::format::GraphFile::open_mmap(&report.output).unwrap();
    let view =
        ord_graph::format::GraphFile::open_mmap(&dir.path().join("attr-inconnue-z11.road.ordv")).unwrap();
    road.check_for(base.header(), &view).expect("identité");
    // Deux écritures : mêmes ancrages et même zone (la date d'écriture est de la provenance, hors de ce que le moteur lit).
    let again =
        attractors_cmd::run(&args(attr_pbf(), Some(dir.path().join("second")))).expect("seconde écriture");
    for (a, b) in r.views.iter().zip(&again.views) {
        let (fa, fb) = (
            ord_graph::attract::AttrFile::open_mmap(&a.file).unwrap(),
            ord_graph::attract::AttrFile::open_mmap(&b.file).unwrap(),
        );
        assert_eq!(fa.anchors().collect::<Vec<_>>(), fb.anchors().collect::<Vec<_>>());
        assert_eq!(fa.zone_nodes(), fb.zone_nodes());
        assert_eq!(fa.zone_masks(), fb.zone_masks());
        assert_eq!(
            (fa.n_anchors(), fa.n_zone(), fa.len_bytes()),
            (fb.n_anchors(), fb.n_zone(), fb.len_bytes())
        );
    }

    // Un extrait d'un octet différent : refus avant tout calcul.
    let mut bytes = std::fs::read(attr_pbf()).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    let other = dir.path().join("other.osm.pbf");
    std::fs::write(&other, bytes).unwrap();
    let err = attractors_cmd::run(&args(other, Some(dir.path().join("refused")))).unwrap_err();
    assert!(err.to_string().contains("l'extrait n'est pas celui du graphe"), "{err}");
    assert!(!dir.path().join("refused").exists());
}

#[test]
fn external_build_writes_the_same_graph() {
    use ord_graph::format::GraphFile;
    let run = |external: bool, dir: &std::path::Path| {
        build_cmd::run(&BuildArgs {
            pbf: mini_pbf(),
            out_dir: Some(dir.to_path_buf()),
            region: None,
            zoom: 11,
            threads: None,
            report: Some(dir.join("build.json")),
            external,
            mem_budget_mb: 1,
            tmp_dir: None,
        })
        .expect("build")
    };
    let (d1, d2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (mem, ext) = (run(false, d1.path()), run(true, d2.path()));
    assert_eq!(
        (ext.ways_total, ext.ways_kept, ext.edges, ext.vertices),
        (mem.ways_total, mem.ways_kept, mem.edges, mem.vertices)
    );
    assert!(ext.tmp_peak_bytes.is_some() && !ext.runs.is_empty());
    let open = |p: &std::path::Path| GraphFile::open(std::fs::read(p).unwrap()).unwrap();
    let (gm, ge) = (open(&mem.output), open(&ext.output));
    assert_eq!(gm.section_digests(), ge.section_digests());
    // Rien d'autre que le graphe et le rapport : le dossier temporaire est parti.
    assert_eq!(std::fs::read_dir(d2.path()).unwrap().count(), 2);
}
