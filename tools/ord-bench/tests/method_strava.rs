//! Test de méthode (T05 §3.4) : sur `strava.gpx` de vcyclist, le calcul de D+ d'`ord-bench`
//! redonne le rapport publié par vcyclist, 714 / 637 m ≈ 1,12 au preset `dem` (Mapterhorn z12,
//! centres de pixel ; `elevation.md`, « What the DEM costs »), à ± 3 points.
//!
//! `strava.gpx` n'est pas encore copié dans `bench/data/` (T05 §11) : le chemin vient de
//! `ORD_BENCH_STRAVA_GPX`, sinon `bench/data/vcyclist/strava.gpx`. Cache de banc requis :
//! `ord-bench fill-cache --gpx <strava.gpx>`, puis
//! `cargo test -p ord-bench --test method_strava -- --ignored --nocapture`.

use std::path::PathBuf;

use ord_bench::service::{Pt, Service};
use ord_bench::{gpx, metrics, BenchPaths};
use ord_core::rules::Rules;
use ord_data::elevation::PixelConvention;
use ord_domain::geo::haversine_m;
use ord_domain::request::GainPreset;
use ord_domain::LatLon;
use serde_json::json;

#[tokio::test]
#[ignore = "strava.gpx et cache de banc requis (ord-bench fill-cache --gpx …)"]
async fn dplus_ratio_matches_vcyclist() {
    let bench = BenchPaths::repo_bench();
    let paths = BenchPaths::new(&bench, None, None);
    let path = std::env::var_os("ORD_BENCH_STRAVA_GPX")
        .map(PathBuf::from)
        .unwrap_or_else(|| bench.join("data/vcyclist/strava.gpx"));
    let track = gpx::read(&path).expect("strava.gpx");
    let pts: Vec<Pt> = track.iter().map(|p| Pt { lat: p.lat, lon: p.lon }).collect();

    // D+ du baromètre de la trace, même preset `dem`, sur ses propres distances.
    let mut d = vec![0.0];
    for w in track.windows(2) {
        let step = haversine_m(LatLon::new(w[0].lat, w[0].lon), LatLon::new(w[1].lat, w[1].lon));
        d.push(d[d.len() - 1] + step);
    }
    let ele: Vec<f64> = track.iter().map(|p| p.ele.unwrap_or(f64::NAN)).collect();
    let dem = Rules::default().elevation.gain.preset(GainPreset::Dem);
    let baro = metrics::dplus(&d, &ele, dem);

    // D+ du service (MNT Mapterhorn z12, preset dem).
    let service = Service::new(&paths.cache, true, PixelConvention::Center).expect("service");
    let r = service
        .route_details(&pts, json!({"sampling": {"mode": "input"}, "sections": ["summary"]}))
        .await
        .expect("réponse du service");
    let dem_gain = r["summary"]["gain_m"].as_f64().expect("gain_m");
    let ratio = dem_gain / baro;
    println!("strava.gpx : D+ service {dem_gain:.0} m, D+ baro {baro:.0} m, rapport {ratio:.3} (vcyclist 714 / 637 = 1,121)");
    assert!((ratio - 714.0 / 637.0).abs() <= 0.03, "rapport {ratio:.3} hors de 1,12 ± 0,03");
}
