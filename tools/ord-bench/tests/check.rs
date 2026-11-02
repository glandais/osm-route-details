//! `ord-bench check` sur le banc du dépôt, sans traces privées (comme en CI) : empreintes des
//! fichiers versionnés, cohérence criteria.yaml ↔ README, Q1.

use ord_bench::check::check;
use ord_bench::BenchPaths;

#[test]
fn bench_fingerprints_and_q1() {
    let bench = BenchPaths::repo_bench();
    let missing_private = std::env::temp_dir().join("ord-bench-no-private");
    let paths = BenchPaths::new(&bench, None, Some(&missing_private));
    let report = check(&paths).expect("check");
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert!(report.verified > 100, "{}", report.verified);
    assert!(!report.private_present);
    // 4 négatifs du MVP et 3 de V1, en calibration depuis le jugement V1 (README §2, point 3).
    assert_eq!(report.q1.negatives, 7);
    assert_eq!(report.q1_tenue.negatives, 0);
    assert!(report.q1.pass(), "{:?}", report.q1);
}

#[test]
fn held_out_cases_are_excluded_before_any_read() {
    let manifest = ord_bench::manifest::Manifest::load(&BenchPaths::repo_bench()).expect("manifest");
    let all = ord_bench::campaign::select(&manifest, &[], &[]);
    // Série tenue à l'écart du manifest (vide entre le jugement V1 et l'entrée de V1.1) : exclue.
    let held: Vec<&str> = manifest
        .traces
        .iter()
        .filter(|t| t.serie == ord_bench::manifest::Serie::TenueALEcart)
        .map(|t| t.id.as_str())
        .collect();
    assert_eq!(all.excluded_held_out, held);
    assert!(all.cases.iter().all(|t| t.serie == ord_bench::manifest::Serie::Calibration));
    assert!(all.cases.iter().all(|t| t.groupe != "negatif"));
}

/// Retard du baro gelé (README §4 ter.9, manifest `perso_v1.retard_baro_gele`, 48ca44a) : le
/// fichier du dépôt est lu à son empreinte (101 cas, tous estimés) ; `check` le vérifie, sans
/// bench/private/.
#[test]
fn frozen_baro_lag_is_read_at_its_manifest_fingerprint() {
    let bench = BenchPaths::repo_bench();
    let missing_private = std::env::temp_dir().join("ord-bench-no-private");
    let paths = BenchPaths::new(&bench, None, Some(&missing_private));
    let manifest = ord_bench::manifest::Manifest::load(&bench).expect("manifest");
    let entry = manifest.v1.as_ref().and_then(|v| v.retard_gele.clone()).expect("perso_v1.retard_baro_gele");
    assert_eq!(entry.0, "data/retard-baro-v1.json");
    let frozen = ord_bench::retard::RetardGele::load(&paths, Some(&entry)).expect("fichier gelé");
    assert_eq!(frozen.cas.len(), 101);
    assert!(frozen.cas.values().all(Option::is_some));
    let report = check(&paths).expect("check");
    assert!(report.failures.is_empty(), "{:?}", report.failures);
}

/// Un fichier gelé modifié (ou absent) est refusé, au chargement comme par `check`. Copie
/// temporaire du banc : le fichier du dépôt n'est jamais touché.
#[test]
fn an_altered_frozen_baro_lag_is_refused() {
    let repo = BenchPaths::repo_bench();
    let root = tempfile::tempdir().unwrap();
    let bench = root.path().join("bench");
    std::fs::create_dir_all(bench.join("data")).unwrap();
    for f in ["manifest.yaml", "criteria.yaml", "README.md", "data/retard-baro-v1.json"] {
        std::fs::copy(repo.join(f), bench.join(f)).unwrap();
    }
    let private = root.path().join("no-private");
    let paths = BenchPaths::new(&bench, None, Some(&private));
    let manifest = ord_bench::manifest::Manifest::load(&bench).expect("manifest");
    let entry = manifest.v1.as_ref().and_then(|v| v.retard_gele.clone()).unwrap();
    assert!(ord_bench::retard::RetardGele::load(&paths, Some(&entry)).is_ok());
    let label = |r: &ord_bench::check::CheckReport| {
        r.failures.iter().filter(|f| f.starts_with("retard_baro_gele")).cloned().collect::<Vec<_>>()
    };
    assert!(label(&check(&paths).expect("check")).is_empty());
    // Un δ modifié : même structure, empreinte différente.
    let file = bench.join("data/retard-baro-v1.json");
    let text = std::fs::read_to_string(&file).unwrap();
    let altered = text.replacen("\"retard_m\": 15.0", "\"retard_m\": 20.0", 1);
    assert_ne!(altered, text);
    std::fs::write(&file, altered).unwrap();
    let err = ord_bench::retard::RetardGele::load(&paths, Some(&entry)).unwrap_err();
    assert!(err.to_string().contains("empreinte"), "{err}");
    assert_eq!(label(&check(&paths).expect("check")), ["retard_baro_gele : empreinte différente"]);
    // Absent : refusé aussi.
    std::fs::remove_file(&file).unwrap();
    assert!(ord_bench::retard::RetardGele::load(&paths, Some(&entry)).is_err());
    assert_eq!(label(&check(&paths).expect("check")), ["retard_baro_gele : fichier absent"]);
}
