//! Cohérence avec la vérité `T` de vcyclist (README technique §3, ligne 2, ex-A8) : service
//! complet (IGN LiDAR HD z15 et repli Mapterhorn), RMS(service − `T`) sur le terrain ouvert
//! ≤ 1,0 m sur `test1..3` et ≤ 1,5 m sur `test4`. Cohérence, pas une métrique du banc.
//!
//! Lit le cache de banc (hors ligne) : `ord-bench fill-cache --coherence test1,test2,test3,test4`
//! d'abord, puis `cargo test -p ord-bench --test coherence_ign -- --ignored --nocapture`.

use ord_bench::coherence::measure;
use ord_bench::BenchPaths;

#[tokio::test]
#[ignore = "cache de banc requis (ord-bench fill-cache --coherence test1,test2,test3,test4)"]
async fn ign_profile_matches_vcyclist_truth() {
    let bench = BenchPaths::repo_bench();
    let paths = BenchPaths::new(&bench, None, None);
    let mut failures = Vec::new();
    for (route, max) in [("test1", 1.0), ("test2", 1.0), ("test3", 1.0), ("test4", 1.5)] {
        let c = measure(&paths.cache, &paths.bench, route, true, true, false)
            .await
            .expect("mesure sur le cache de banc");
        println!(
            "{route}, {} stations de terrain ouvert : RMS {:.3} m (seuil {max} m)",
            c.stations, c.rms_center_m
        );
        assert!(c.stations > 1_000, "{route} : trop peu de stations");
        if c.rms_center_m > max {
            failures.push(format!("{route} : RMS {:.3} m > {max} m", c.rms_center_m));
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}
