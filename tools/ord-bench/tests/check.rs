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
    assert_eq!(report.q1.negatives, 4);
    assert!(report.q1.pass(), "{:?}", report.q1);
}

#[test]
fn held_out_cases_are_excluded_before_any_read() {
    let manifest = ord_bench::manifest::Manifest::load(&BenchPaths::repo_bench()).expect("manifest");
    let all = ord_bench::campaign::select(&manifest, &[], &[]);
    assert!(!all.excluded_held_out.is_empty());
    assert!(all.cases.iter().all(|t| t.serie == ord_bench::manifest::Serie::Calibration));
    assert!(all.cases.iter().all(|t| t.groupe != "negatif"));
}
