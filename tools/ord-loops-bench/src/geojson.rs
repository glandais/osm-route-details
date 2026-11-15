//! GeoJSON d'une requête : départ, candidats et leurs violations, arêtes d'équipe du disque. **Privé** : écrit
//! seulement sous `~/.cache/osm-route-details/graph/lot2/geojson/`, jamais dans le dépôt.

use std::path::Path;

use anyhow::Result;
use ord_graph::format::GraphFile;
use ord_graph::overlay::Overlay;
use ord_loops::{route_geometry, Generation, Request};
use serde_json::{json, Value};

use crate::input::crow_m;
use crate::record::Rec;

fn e7(p: (i32, i32)) -> (f64, f64) {
    (f64::from(p.0) * 1e-7, f64::from(p.1) * 1e-7)
}

/// Arêtes d'équipe dans le disque de rayon `radius_m` autour de `centre` (au plus `cap`).
fn team_edges<B: AsRef<[u8]>>(
    g: &GraphFile<B>,
    overlay: &Overlay,
    centre: (f64, f64),
    radius_m: f64,
    cap: usize,
) -> Vec<Value> {
    let mut out = Vec::new();
    for (e, _) in overlay.iter() {
        let (a, b) = g.edge_ends(e);
        let pa = e7(g.node_pos(a));
        if crow_m(centre, pa) > radius_m {
            continue;
        }
        let mut line = vec![pa];
        line.extend(g.geometry(e).into_iter().map(e7));
        line.push(e7(g.node_pos(b)));
        out.push(json!({
            "type": "Feature",
            "properties": {"role": "team_edge"},
            "geometry": {"type": "LineString", "coordinates": line.iter().map(|p| json!([p.1, p.0])).collect::<Vec<_>>()},
        }));
        if out.len() >= cap {
            break;
        }
    }
    out
}

pub fn write<B: AsRef<[u8]>>(
    dir: &Path,
    g: &GraphFile<B>,
    overlay: &Overlay,
    req: &Request,
    rec: &Rec,
    generation: Option<&Generation>,
) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut features = vec![json!({
        "type": "Feature",
        "properties": {"role": "start", "snap_m": rec.start_snap_m},
        "geometry": {"type": "Point", "coordinates": [req.start.lon, req.start.lat]},
    })];
    if let Some(e) = req.end {
        features.push(json!({
            "type": "Feature",
            "properties": {"role": "end"},
            "geometry": {"type": "Point", "coordinates": [e.lon, e.lat]},
        }));
    }
    if let Some(gen) = generation {
        for (k, r) in gen.routes.iter().enumerate() {
            let rr = &rec.routes[k];
            let coords: Vec<Value> = route_geometry(g, r).iter().map(|p| json!([p.lon, p.lat])).collect();
            features.push(json!({
                "type": "Feature",
                "properties": {
                    "role": "candidate", "index": k, "length_km": rr.length_km, "over_target": rr.over_target,
                    "team_share": rr.team_share, "ok": rr.violations.is_empty(), "violations": rr.violations,
                },
                "geometry": {"type": "LineString", "coordinates": coords},
            }));
        }
    }
    features.extend(team_edges(g, overlay, (req.start.lat, req.start.lon), req.distance_m / 2.0, 20_000));
    let doc = json!({"type": "FeatureCollection", "features": features});
    let name = format!("{}-{}-{}.geojson", rec.team, rec.i, rec.mode);
    std::fs::write(dir.join(name), serde_json::to_vec(&doc)?)?;
    Ok(())
}
