//! `diagnose` : explication des plages non appariées (M1) et des désaccords de revêtement (R2,
//! R3) d'un cas, à partir des seules tuiles VersaTiles du cache de banc (hors ligne). Sortie sur
//! la console, jamais un rapport : distances curvilignes, distances aux voies et valeurs d'enum
//! ou de tag (`kind`, `surface`, `tracktype`) seulement ; ni coordonnée, ni nom, ni identifiant.

use std::collections::BTreeMap;
use std::sync::Arc;

use ord_adapters::Shortbread;
use ord_core::classification::tables::bicycle_routable;
use ord_domain::budget::{Deadline, RequestId};
use ord_domain::ctx::RequestCtx;
use ord_domain::geo::{dist_units, project_on_segment};
use ord_domain::street::{PavedGroup, StreetTile, StructureTag, SurfaceTag, WayAttrs};
use ord_domain::tiles::{TileKey, TileOutcome};
use ord_domain::LatLon;
use serde_json::{json, Value};

use crate::service::{Pt, Service};

/// Parcours à diagnostiquer : tronçons envoyés.
#[derive(Debug)]
pub struct Parts {
    pub parts: Vec<Vec<Pt>>,
}

fn cumulative(p: &[Pt]) -> Vec<f64> {
    let mut out = vec![0.0];
    for w in p.windows(2) {
        let d =
            ord_domain::geo::haversine_m(LatLon::new(w[0].lat, w[0].lon), LatLon::new(w[1].lat, w[1].lon));
        out.push(out.last().copied().unwrap_or(0.0) + d);
    }
    out
}

/// Position à la distance `d` d'un tronçon.
fn at(points: &[Pt], cum: &[f64], d: f64) -> LatLon {
    let i = cum.partition_point(|x| *x <= d).clamp(1, cum.len() - 1);
    let (a, b) = (points[i - 1], points[i]);
    let span = cum[i] - cum[i - 1];
    let t = if span > 0.0 { ((d - cum[i - 1]) / span).clamp(0.0, 1.0) } else { 0.0 };
    LatLon::new(a.lat + t * (b.lat - a.lat), a.lon + t * (b.lon - a.lon))
}

fn tag(a: &WayAttrs) -> String {
    let surface = match a.surface {
        SurfaceTag::Grouped(PavedGroup::Paved) => "paved",
        SurfaceTag::Grouped(PavedGroup::Unpaved) => "unpaved",
        _ => "-",
    };
    let tt = a.tracktype.map_or("-".to_owned(), |t| format!("grade{}", t as u8 + 1));
    let st = match a.structure {
        StructureTag::Bridge => " bridge",
        StructureTag::Tunnel => " tunnel",
        _ => "",
    };
    let bike = a.bicycle.as_deref().map(|b| format!(" bicycle={b}")).unwrap_or_default();
    format!("{} surface={surface} tracktype={tt}{st}{bike}", a.highway)
}

/// Voies proches d'un point : (distance m, attributs, ouvert au vélo), triées par distance.
fn near(
    tiles: &BTreeMap<TileKey, Option<Arc<StreetTile>>>,
    p: LatLon,
    radius_m: f64,
) -> (bool, Vec<(f64, String, bool)>) {
    let grid = Shortbread::GRID;
    let u = grid.to_units(p);
    let mpu = grid.metres_per_unit(p.lat);
    let k = grid.tile_of(u);
    let mut missing = false;
    let mut out: BTreeMap<String, (f64, bool)> = BTreeMap::new();
    for dx in -1i64..=1 {
        for dy in -1i64..=1 {
            let key = TileKey { z: k.z, x: (k.x as i64 + dx) as u32, y: (k.y as i64 + dy) as u32 };
            match tiles.get(&key) {
                Some(Some(t)) => {
                    for l in &t.lines {
                        let a = &t.attrs[l.attr as usize];
                        for w in l.points.windows(2) {
                            let d = dist_units(u, project_on_segment(u, w[0], w[1]).point) * mpu;
                            if d <= radius_m {
                                let e = out.entry(tag(a)).or_insert((d, bicycle_routable(a)));
                                e.0 = e.0.min(d);
                            }
                        }
                    }
                }
                Some(None) => {}
                None => missing |= dx == 0 && dy == 0,
            }
        }
    }
    let mut v: Vec<(f64, String, bool)> = out.into_iter().map(|(t, (d, r))| (d, t, r)).collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    (missing, v)
}

/// Référence de revêtement et passages abscisse du service ↔ abscisse de la référence.
pub type Reference<'a> = (&'a [crate::typology::Ref], &'a dyn Fn(f64) -> f64, &'a dyn Fn(f64) -> f64);

/// Diagnostic d'un cas : plages non appariées et désaccords de revêtement, avec leurs causes.
pub async fn diagnose(
    cache: &std::path::Path,
    id: &str,
    parts: &Parts,
    reference: Option<Reference<'_>>,
) -> anyhow::Result<Value> {
    let mut config = crate::service::bench_config(cache, true, crate::service::Variant::DEFAULT);
    config.limits.debug_allowed = true;
    let service = Service::start_with(&config).await?;
    let chain = ord_server::wiring::typology_chain(&config)?;
    let ctx = RequestCtx::new(RequestId("diag".into()), Deadline::after(std::time::Duration::from_secs(600)));
    let opts = json!({"sampling": {"mode": "input"}, "sections": ["typology"], "debug": true});
    let mut responses = Vec::new();
    let mut offsets = Vec::new();
    let mut acc = 0.0;
    let mut cums = Vec::new();
    for p in &parts.parts {
        let r = service.route_details(p, opts.clone()).await?;
        offsets.push(acc);
        let c = cumulative(p);
        acc += c.last().copied().unwrap_or(0.0);
        cums.push(c);
        responses.push(r);
    }
    // Position et tronçon à une distance du parcours recollé.
    let locate = |d: f64| -> (LatLon, f64) {
        let k = offsets.partition_point(|o| *o <= d).saturating_sub(1);
        (at(&parts.parts[k], &cums[k], d - offsets[k]), offsets[k])
    };
    // Tuiles du cache (hors ligne) autour des points.
    let mut keys = std::collections::BTreeSet::new();
    for p in parts.parts.iter().flatten() {
        let k = Shortbread::GRID.tile_of(Shortbread::GRID.to_units(LatLon::new(p.lat, p.lon)));
        for dx in -1i64..=1 {
            for dy in -1i64..=1 {
                keys.insert(TileKey { z: k.z, x: (k.x as i64 + dx) as u32, y: (k.y as i64 + dy) as u32 });
            }
        }
    }
    let keys: Vec<TileKey> = keys.into_iter().collect();
    let outcomes = chain.store().get_many(&keys, &ctx).await;
    let tiles: BTreeMap<TileKey, Option<Arc<StreetTile>>> = keys
        .into_iter()
        .zip(outcomes)
        .filter_map(|(k, o)| match o {
            TileOutcome::Data { tile, .. } => Some((k, Some(tile))),
            TileOutcome::Empty(_) => Some((k, None)),
            _ => None,
        })
        .collect();

    let identity = |x: f64| x;
    let to_ref: &dyn Fn(f64) -> f64 = reference.map_or(&identity as &dyn Fn(f64) -> f64, |r| r.1);
    let to_d: &dyn Fn(f64) -> f64 = reference.map_or(&identity as &dyn Fn(f64) -> f64, |r| r.2);
    let ours = crate::typology::ours_of(&responses, &offsets, to_ref);
    let um = crate::typology::unmatched_ranges(&ours, 10, to_d);
    let dis = reference.map(|r| crate::typology::disagreements(&ours, r.0, 10, to_d)).unwrap_or_default();

    // Segment du service (debug `osm`) à une distance.
    let seg_at = |d: f64| -> Value {
        let k = offsets.partition_point(|o| *o <= d).saturating_sub(1);
        let local = d - offsets[k];
        responses[k]["segments"]
            .as_array()
            .and_then(|s| {
                s.iter().find(|s| {
                    s["from_m"].as_f64().unwrap_or(0.0) <= local && local <= s["to_m"].as_f64().unwrap_or(0.0)
                })
            })
            .map(|s| json!({"osm": s["osm"], "way_kind": s["way_kind"], "structure": s["structure"]}))
            .unwrap_or(Value::Null)
    };

    let mut m1 = Vec::new();
    for r in &um {
        let (a, b) = (r["d_debut_m"].as_f64().unwrap_or(0.0), r["d_fin_m"].as_f64().unwrap_or(0.0));
        let n = ((b - a) / 10.0).ceil().max(1.0) as usize;
        let (mut miss, mut none, mut routable_near, mut filtered) = (0, 0, 0, 0);
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        let mut dist_routable = Vec::new();
        let raccord = offsets.iter().skip(1).any(|o| *o >= a - 50.0 && *o <= b + 50.0);
        for i in 0..=n {
            let d = a + (b - a) * i as f64 / n as f64;
            let (p, _) = locate(d);
            let (m, v) = near(&tiles, p, 40.0);
            if m {
                miss += 1;
            }
            match v.iter().find(|x| x.2) {
                Some(x) => {
                    routable_near += 1;
                    dist_routable.push(x.0);
                    *kinds.entry(x.1.clone()).or_default() += 1;
                }
                None if v.is_empty() => none += 1,
                None => {
                    filtered += 1;
                    *kinds.entry(format!("[fermée au vélo] {}", v[0].1)).or_default() += 1;
                }
            }
        }
        let total = n + 1;
        // Au milieu : voie la plus proche dans 300 m, distance au bord de tuile le plus proche.
        let (pm, _) = locate(0.5 * (a + b));
        let (_, far) = near(&tiles, pm, 300.0);
        let u = Shortbread::GRID.to_units(pm);
        let e = f64::from(Shortbread::GRID.extent);
        let fx = u[0].rem_euclid(e).min(e - u[0].rem_euclid(e));
        let fy = u[1].rem_euclid(e).min(e - u[1].rem_euclid(e));
        let border_m = fx.min(fy) * Shortbread::GRID.metres_per_unit(pm.lat);
        let cause = if miss * 2 > total {
            "tuile_absente"
        } else if raccord {
            "raccord_troncons"
        } else if none * 2 > total {
            "aucune_voie_dans_les_tuiles"
        } else if filtered * 2 > total {
            "voie_fermee_au_velo"
        } else {
            "voie_presente_rejetee_par_le_hmm"
        };
        let mut r = r.clone();
        r["cause"] = json!(cause);
        r["echantillons"] = json!({"sans_voie": none, "voie_fermee_velo": filtered, "voie_velo": routable_near, "tuile_absente": miss});
        r["distance_voie_velo_mediane_m"] =
            json!((crate::metrics::median(&dist_routable) * 10.0).round() / 10.0);
        r["voies_proches"] = json!(kinds);
        r["milieu_voie_la_plus_proche"] = json!(far
            .first()
            .map(|x| json!({"distance_m": (x.0 * 10.0).round() / 10.0, "voie": x.1, "velo": x.2})));
        r["milieu_bord_de_tuile_m"] = json!((border_m * 10.0).round() / 10.0);
        r["milieu_tuile"] = json!(match tiles.get(&Shortbread::GRID.tile_of(u)) {
            Some(Some(t)) => format!("data, {} lignes", t.lines.len()),
            Some(None) => "vide (204)".to_owned(),
            None => "absente ou en panne".to_owned(),
        });
        m1.push(r);
    }
    let mut r3 = Vec::new();
    for r in &dis {
        let (a, b) = (r["d_debut_m"].as_f64().unwrap_or(0.0), r["d_fin_m"].as_f64().unwrap_or(0.0));
        let seg = seg_at(0.5 * (a + b));
        let source = r["source_service"].as_str().unwrap_or("");
        let cause = match source {
            "tagged" => "surface_taguee_en_desaccord",
            "inferred_tracktype" => "inference_tracktype_en_desaccord",
            "inferred_highway" => "inference_highway_en_desaccord",
            _ => "autre",
        };
        let mut r = r.clone();
        r["cause"] = json!(cause);
        r["segment_service"] = seg;
        r3.push(r);
    }
    let share = |list: &[Value]| -> BTreeMap<String, f64> {
        let mut m: BTreeMap<String, f64> = BTreeMap::new();
        let tot: f64 = list.iter().filter_map(|x| x["longueur_plage_m"].as_f64()).sum();
        for x in list {
            *m.entry(x["cause"].as_str().unwrap_or("").to_owned()).or_default() +=
                x["longueur_plage_m"].as_f64().unwrap_or(0.0) / tot.max(1e-9);
        }
        m.into_iter().map(|(k, v)| (k, (v * 1000.0).round() / 1000.0)).collect()
    };
    Ok(json!({
        "id": id,
        "raccords_d_m": offsets.iter().skip(1).copied().collect::<Vec<_>>(),
        "M1": m1, "M1_parts_par_cause": share(&m1),
        "R": r3, "R_parts_par_cause": share(&r3)
    }))
}

/// Diagnostic d'un cas du manifest (`perso-NN`, `perso-NN.sN`, `os-*`), garde-fou de vie privée
/// appliqué à la sortie.
pub async fn diagnose_case(paths: &crate::BenchPaths, id: &str) -> anyhow::Result<Value> {
    use crate::campaign::{interp_one, load_perso};
    let manifest = crate::manifest::Manifest::load(&paths.bench)?;
    let out = if let Some(entry) = manifest.osm_surface.iter().find(|e| e.id == id) {
        let route = crate::osm_surface::load(&paths.bench, entry)?;
        let cum = cumulative(&route.points);
        let to_ref = |x: f64| interp_one(&cum, &route.dist, x);
        let to_d = |x: f64| interp_one(&route.dist, &cum, x);
        let parts = Parts { parts: vec![route.points.clone()] };
        diagnose(&paths.cache, id, &parts, Some((&route.reference, &to_ref, &to_d))).await?
    } else {
        let base = id.split(".s").next().unwrap_or(id);
        let trace = manifest
            .traces
            .iter()
            .find(|t| t.id == base)
            .ok_or_else(|| anyhow::anyhow!("{id} : cas inconnu"))?;
        if trace.serie == crate::manifest::Serie::TenueALEcart {
            anyhow::bail!("{id} : série tenue à l'écart, jamais diagnostiquée");
        }
        let cases = load_perso(paths, trace, &crate::BenchRules::default())
            .map_err(|e| anyhow::anyhow!("{id} : {e}"))?;
        let case =
            cases.into_iter().find(|c| c.id == id).ok_or_else(|| anyhow::anyhow!("{id} : cas absent"))?;
        let mut d_srv = Vec::new();
        let mut off = 0.0;
        for (k, p) in case.parts.iter().enumerate() {
            let c = cumulative(p);
            d_srv.extend(c.iter().skip(usize::from(k > 0)).map(|x| x + off));
            off += c.last().copied().unwrap_or(0.0);
        }
        let csv = std::fs::read_to_string(paths.private.join("recales").join(format!("{id}.surface.csv")))
            .unwrap_or_default();
        let reference = crate::typology::read_stadia_surface(&csv);
        let to_ref = |x: f64| interp_one(&d_srv, &case.s_ref, x);
        let to_d = |x: f64| interp_one(&case.s_ref, &d_srv, x);
        let parts = Parts { parts: case.parts.clone() };
        diagnose(&paths.cache, id, &parts, Some((&reference, &to_ref, &to_d))).await?
    };
    crate::privacy::assert_report_clean(&out, &crate::privacy::private_names(paths))?;
    Ok(out)
}
