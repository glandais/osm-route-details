//! Test de méthode (T05 §3.4) : sur `strava.gpx` de vcyclist, le calcul de D+ d'`ord-bench`
//! redonne le rapport publié par vcyclist, 714 / 637 m ≈ 1,121 au preset `dem` (Mapterhorn z12,
//! centres de pixel ; `elevation.md`, « What the DEM costs »), « à ± 3 points près » : écart
//! absolu ≤ 0,03 sur le rapport.
//!
//! Le fichier est une activité enregistrée : hors git, entrée `vc-strava` de la section `hors_git`
//! du manifest, lue dans `bench/private/` après contrôle de son empreinte ; sans `bench/private/`,
//! le cas est sauté. Elle date de la période d'altitude dégradée (bench README §1.3) : le D+ baro
//! n'est pas une référence de justesse, le test reproduit seulement le chiffre de vcyclist.
//!
//! `ord-bench fill-cache` (inclut `vc-strava`), puis
//! `cargo test -p ord-bench --test method_strava -- --ignored --nocapture`.

use ord_bench::campaign::hors_git_file;
use ord_bench::manifest::Manifest;
use ord_bench::service::{Pt, Service};
use ord_bench::{gpx, metrics, BenchPaths};
use ord_core::rules::Rules;
use ord_data::elevation::PixelConvention;
use ord_domain::geo::haversine_m;
use ord_domain::request::GainPreset;
use ord_domain::LatLon;
use serde_json::json;

#[tokio::test]
#[ignore = "bench/private/ et cache de banc requis (ord-bench fill-cache)"]
async fn dplus_ratio_matches_vcyclist() {
    let bench = BenchPaths::repo_bench();
    let paths = BenchPaths::new(&bench, None, None);
    let manifest = Manifest::load(&bench).expect("manifest");
    let Some(path) = hors_git_file(&paths, &manifest, "vc-strava").expect("empreinte de vc-strava") else {
        println!("vc-strava : skipped (bench/private/ absent)");
        return;
    };
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
    println!("vc-strava : D+ service {dem_gain:.0} m, D+ baro {baro:.0} m, rapport {ratio:.3} (vcyclist 714 / 637 = 1,121)");
    assert!((ratio - 714.0 / 637.0).abs() <= 0.03, "rapport {ratio:.3} hors de 1,121 ± 0,03");
}
