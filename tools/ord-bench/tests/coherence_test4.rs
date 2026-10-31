//! Correction du demi-pixel sur `test4` de vcyclist (README technique §3, ligne 1) :
//! RMS(service − `T`) ≤ 1,0 m sur le terrain ouvert, Mapterhorn z12. Cohérence avec la vérité
//! de vcyclist (0,58 m aux centres, 2,63 m aux coins), pas une métrique du banc.
//!
//! Lit le cache de banc (hors ligne) : `ord-bench fill-cache --coherence test4` d'abord, puis
//! `cargo test -p ord-bench --test coherence_test4 -- --ignored --nocapture`.

use ord_bench::coherence::measure;
use ord_bench::BenchPaths;

#[tokio::test]
#[ignore = "cache de banc requis (ord-bench fill-cache --coherence test4)"]
async fn half_pixel_correction_on_test4() {
    let bench = BenchPaths::repo_bench();
    let paths = BenchPaths::new(&bench, None, None);
    let c = measure(&paths.cache, &paths.bench, "test4", true).await.expect("mesure sur le cache de banc");
    println!(
        "test4, {} stations de terrain ouvert : RMS {:.2} m aux centres (après correction), {:.2} m aux coins (avant)",
        c.stations, c.rms_center_m, c.rms_corner_m
    );
    assert!(c.stations > 20_000);
    assert!(c.rms_center_m <= 1.0, "RMS {:.2} m > 1,0 m", c.rms_center_m);
    assert!(c.rms_corner_m > c.rms_center_m, "la correction du demi-pixel doit améliorer la cohérence");
}
