//! Parcours d'osm-surface (`os-*`, bench §1.2) : R1, R2 et M1 contre les classes Stadia/Valhalla
//! de tribly (`*.reference.json`), et latence de la typologie à chaud. Ces parcours sont publics
//! (géométrie OSM) ; les rapports n'en écrivent pourtant aucune coordonnée.
//!
//! Entrée : la ligne de la référence (`tracks[0].line`, celle qu'envoyait l'éval d'osm-surface),
//! dont la 4e coordonnée est l'abscisse des plages `tracks[0].surfaces`.

use std::path::Path;

use anyhow::Context;
use ord_domain::budget::{Deadline, RequestId};
use ord_domain::ctx::RequestCtx;
use ord_domain::LatLon;
use serde_json::{json, Value};

use crate::campaign::interp_one;
use crate::manifest::FileEntry;
use crate::service::{Pt, Service};
use crate::typology::{
    agreement, disagreements, metrics_json, ours_of, r_pass, unmatched_ranges, Class, Ref,
};

/// Référence d'un parcours : points envoyés, abscisse de chacun, plages de classe Stadia.
#[derive(Clone)]
pub struct OsRoute {
    pub id: String,
    pub points: Vec<Pt>,
    pub dist: Vec<f64>,
    pub reference: Vec<Ref>,
}

impl std::fmt::Debug for OsRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OsRoute").field("id", &self.id).field("n", &self.points.len()).finish_non_exhaustive()
    }
}

/// `data/osm-surface/<nom>.reference.json`, à côté du GPX du manifest.
pub fn load(bench: &Path, entry: &FileEntry) -> anyhow::Result<OsRoute> {
    let path = bench.join(entry.fichier.trim_end_matches(".gpx").to_owned() + ".reference.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).context("référence osm-surface")?)?;
    let track = &v["tracks"][0];
    let mut points = Vec::new();
    let mut dist = Vec::new();
    for c in track["line"]["coordinates"].as_array().context("ligne de référence")? {
        let (Some(lon), Some(lat), Some(d)) = (c[0].as_f64(), c[1].as_f64(), c[3].as_f64()) else { continue };
        points.push(Pt { lat, lon });
        dist.push(d);
    }
    let mut reference: Vec<Ref> = track["surfaces"]
        .as_array()
        .context("plages de référence")?
        .iter()
        .filter_map(|s| {
            Some(Ref {
                from: s["from"].as_f64()?,
                to: s["to"].as_f64()?,
                class: Class::of_tribly(s["surface"].as_str().unwrap_or("")),
            })
        })
        .collect();
    reference.sort_by(|a, b| a.from.total_cmp(&b.from));
    Ok(OsRoute { id: entry.id.clone(), points, dist, reference })
}

/// Distance cumulée du service (haversine, comme `prepare`).
fn cumulative(points: &[Pt]) -> Vec<f64> {
    let mut out = vec![0.0];
    for w in points.windows(2) {
        let d =
            ord_domain::geo::haversine_m(LatLon::new(w[0].lat, w[0].lon), LatLon::new(w[1].lat, w[1].lon));
        out.push(out.last().copied().unwrap_or(0.0) + d);
    }
    out
}

pub fn options() -> Value {
    json!({"sampling": {"mode": "input"}, "sections": ["typology", "summary"]})
}

/// R1, R2, M1 d'un parcours.
pub async fn measure(service: &Service, route: &OsRoute) -> anyhow::Result<Value> {
    let r = service.route_details(&route.points, options()).await?;
    let cum = cumulative(&route.points);
    let to_ref = |x: f64| interp_one(&cum, &route.dist, x);
    let to_d = |x: f64| interp_one(&route.dist, &cum, x);
    let ours = ours_of(std::slice::from_ref(&r), &[0.0], &to_ref);
    let a = agreement(&ours, &route.reference);
    let mut warnings: std::collections::BTreeMap<String, usize> = Default::default();
    for c in r["quality"]["warnings"].as_array().into_iter().flatten().filter_map(|w| w["code"].as_str()) {
        *warnings.entry(c.to_owned()).or_default() += 1;
    }
    Ok(json!({
        "id": route.id,
        "serie": "calibration",
        "groupe": "osm_surface",
        "mode": "input",
        "typologie": metrics_json(&a, "R1"),
        "verdicts": {"R1_R2": if r_pass(&a) { "pass" } else { "fail" }, "M1": if crate::typology::m1_pass(&a) { "pass" } else { "fail" }},
        "diagnostics": {
            "status": r["status"],
            "segments": r["segments"].as_array().map_or(0, Vec::len),
            "matched_ratio": r["quality"]["matched_ratio"],
            "inferred_surface_ratio": r["quality"]["inferred_surface_ratio"],
            "by_surface_class_m": r["summary"]["by_surface_class_m"],
            "by_structure_m": r["summary"]["by_structure_m"],
            "avertissements": warnings,
            "typologie": {
                "M1.plages_non_appariees": unmatched_ranges(&ours, 10, &to_d),
                "R2.desaccords_revetu": disagreements(&ours, &route.reference, 10, &to_d)
            }
        }
    }))
}

/// Typologie à chaud (chaîne seule, tuiles en mémoire) : médiane de `trials` appels après un
/// appel de chauffe, en millisecondes. Mesure du test nommé « < 160 ms sur 180 km ».
pub async fn typology_warm_ms(cache: &Path, route: &OsRoute, trials: usize) -> anyhow::Result<(f64, Value)> {
    use ord_pipeline::typology::TypologyChain;
    let config = crate::service::bench_config(cache, true, crate::service::Variant::DEFAULT);
    let chain = ord_server::wiring::typology_chain(&config)?;
    let pts: Vec<LatLon> = route.points.iter().map(|p| LatLon::new(p.lat, p.lon)).collect();
    let prepared = std::sync::Arc::new(ord_core::prepare::prepare(
        &pts,
        ord_core::Rules::default().elevation.station_step_m,
    ));
    let ctx =
        || RequestCtx::new(RequestId("bench".into()), Deadline::after(std::time::Duration::from_secs(90)));
    let warm = chain.run(std::sync::Arc::clone(&prepared), &ctx()).await?;
    let mut times = Vec::new();
    for _ in 0..trials.max(1) {
        let start = std::time::Instant::now();
        chain.run(std::sync::Arc::clone(&prepared), &ctx()).await?;
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let stats = json!({
        "tuiles": warm.stats.tiles,
        "segments_graphe": warm.stats.segments,
        "observations": warm.stats.observations,
        "observations_appariees": warm.stats.matched_observations,
        "km": (prepared.length_m / 100.0).round() / 10.0
    });
    Ok((crate::metrics::median(&times), stats))
}
