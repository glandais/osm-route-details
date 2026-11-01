//! Chargement des cas personnels sur données **synthétiques** (aucune trace réelle) : tronçons
//! `.partN` recollés en un cas, segments disjoints `.sN` en cas indépendants. Couvre la forme de
//! `perso-06` (tronçons) et de `perso-13` (segments, série tenue à l'écart : jamais mesurée).

use ord_bench::campaign::load_perso;
use ord_bench::manifest::{Serie, Trace};
use ord_bench::{BenchPaths, BenchRules};

fn trace(id: &str) -> Trace {
    Trace {
        id: id.into(),
        groupe: "dplus".into(),
        serie: Serie::TenueALEcart,
        fichier: format!("private/traces/{id}.gpx"),
        sha256: None,
        sha256_trim1km: None,
        passages_masques: vec![],
        verdict: None,
        dist_km: None,
        gain_baro_m: None,
        gain_mnt_m: None,
    }
}

/// GPX d'un méridien synthétique : `n` points tous les ≈ 111 m à partir de `lat0`.
fn gpx(lat0: f64, n: usize) -> String {
    let pts: String =
        (0..n).map(|i| format!("<trkpt lat=\"{}\" lon=\"0.5\"/>", lat0 + i as f64 * 0.001)).collect();
    format!("<gpx><trk><trkseg>{pts}</trkseg></trk></gpx>")
}

fn baro(s_max: f64) -> String {
    let mut out = String::from("s_recale_m,baro_m,s_origine_m,distance_au_trace_m,insere\n");
    for k in 0..=((s_max / 50.0) as usize) {
        out.push_str(&format!("{},{},0,1,0\n", k as f64 * 50.0, k as f64));
    }
    out
}

fn setup() -> (tempfile::TempDir, BenchPaths) {
    let root = tempfile::tempdir().unwrap();
    let private = root.path().join("private");
    let recales = private.join("recales");
    std::fs::create_dir_all(&recales).unwrap();
    std::fs::create_dir_all(private.join("traces")).unwrap();
    // Tronçons : le second repart du dernier point du premier (raccord en double).
    std::fs::write(recales.join("synth-p.part1.gpx"), gpx(10.0, 11)).unwrap();
    std::fs::write(recales.join("synth-p.part2.gpx"), gpx(10.010, 6)).unwrap();
    std::fs::write(recales.join("synth-p.baro.csv"), baro(1_700.0)).unwrap();
    // Segments disjoints, chacun avec son baro.
    for (k, lat) in [(1, 20.0), (2, 30.0)] {
        std::fs::write(recales.join(format!("synth-s.s{k}.gpx")), gpx(lat, 5)).unwrap();
        std::fs::write(recales.join(format!("synth-s.s{k}.baro.csv")), baro(500.0)).unwrap();
    }
    let paths = BenchPaths::new(root.path(), Some(&root.path().join("cache")), Some(&private));
    (root, paths)
}

#[test]
fn parts_make_one_case_with_a_continuous_abscissa() {
    let (_root, paths) = setup();
    let cases = load_perso(&paths, &trace("synth-p"), &BenchRules::default()).unwrap();
    assert_eq!(cases.len(), 1, "un seul cas pour une trace en tronçons");
    let c = &cases[0];
    assert_eq!(c.id, "synth-p");
    assert_eq!(c.parts.iter().map(Vec::len).collect::<Vec<_>>(), [11, 6]);
    assert_eq!(c.s_ref.len(), 11 + 6 - 1, "point de raccord retiré une fois");
    assert!(c.s_ref.windows(2).all(|w| w[1] > w[0]), "abscisse strictement croissante");
    let step = c.s_ref[1] - c.s_ref[0];
    assert!((c.s_ref[15] - 15.0 * step).abs() < 1e-3, "continue à travers le raccord");
    assert_eq!(c.baro_s.len(), 35, "baro unique");
}

#[test]
fn segments_are_independent_cases() {
    let (_root, paths) = setup();
    let cases = load_perso(&paths, &trace("synth-s"), &BenchRules::default()).unwrap();
    assert_eq!(cases.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["synth-s.s1", "synth-s.s2"]);
    for c in &cases {
        assert_eq!(c.parts.len(), 1);
        assert_eq!(c.s_ref.len(), 5);
        assert_eq!(c.baro_s.len(), 11, "baro propre au segment");
        assert_eq!(c.serie, Serie::TenueALEcart);
    }
    assert!(load_perso(&paths, &trace("absente"), &BenchRules::default()).is_err());
}

#[test]
fn curvilinear_e1_diagnostics_pass_the_privacy_guard() {
    let report = serde_json::json!({"cas": [{"id": "synth-p", "diagnostics": {
        "E1.max_abs_d_m": 5100.0, "E1.plage_10m_debut_d_m": 5000.0,
        "E1.plage_10m_fin_d_m": 5200.0, "E1.plage_10m_longueur_m": 200.0}}]});
    ord_bench::privacy::assert_report_clean(&report, &[]).unwrap();
}
