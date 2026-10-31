//! Campagne de calibration (`run --offline`) et remplissage du cache de banc (`fill-cache`),
//! T05 §3.3, §5.1, §5.3. Incrément 2 : E1 jugé sur `dplus` et `plaine` (seuils de
//! `criteria.yaml`), E2 rapporté (bornes indicatives), E2b jugé par groupe (médiane de
//! |ln(D+ service / D+ baro)| non dégradée face au service appelé avec
//! `structure_correction: false`, le « MNT brut » de T05 §3.1), `hors_france` rapporté.
//! Incrément 3 : R1, R2 et M1 sur les parcours d'osm-surface (`os-*`), R3 et M1 sur les traces
//! perso recalées (contre `trace_attributes` de Stadia), typologie à chaud sur `os-*`.
//!
//! Ordre imposé : empreintes vérifiées, puis cas `tenue_a_l_ecart` exclus, **avant** toute lecture
//! de tuile. Aucune coordonnée n'est écrite : identifiants `perso-NN`, distances curvilignes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use ord_core::rules::Rules;
use ord_domain::geo::haversine_m;
use ord_domain::request::GainPreset;
use ord_domain::LatLon;
use serde_json::{json, Value};

use crate::check::{check, CheckReport};
use crate::hash::{cache_fingerprint, sha256_bytes};
use crate::manifest::{Manifest, Serie, Trace};
use crate::metrics::{dplus, dplus_with, e1, interpolate, median, passage_windows};
use crate::service::{Pt, Service, Variant};
use crate::typology::{
    agreement, disagreements, metrics_json, ours_of, read_stadia_surface, unmatched_ranges, Class, Ref,
};
use crate::{gpx, BenchPaths, BenchRules};

/// Sélection des cas : la série tenue à l'écart est exclue ici, avant toute lecture.
#[derive(Clone, Debug)]
pub struct Selection<'a> {
    pub cases: Vec<&'a Trace>,
    pub excluded_held_out: Vec<String>,
}

pub fn select<'a>(manifest: &'a Manifest, groupes: &[String], ids: &[String]) -> Selection<'a> {
    let wanted = |t: &Trace| {
        (ids.is_empty() || ids.contains(&t.id))
            && (groupes.is_empty() || groupes.contains(&t.groupe))
            && t.groupe != "negatif"
    };
    let mut cases = Vec::new();
    let mut excluded_held_out = Vec::new();
    for t in manifest.traces.iter().filter(|t| wanted(t)) {
        if t.serie == Serie::TenueALEcart {
            excluded_held_out.push(t.id.clone());
        } else {
            cases.push(t);
        }
    }
    Selection { cases, excluded_held_out }
}

/// Points de référence des ouvrages (publics, puis extension perso si présente).
pub fn ouvrage_refs(paths: &BenchPaths) -> anyhow::Result<BTreeMap<String, Vec<LatLon>>> {
    let mut out = BTreeMap::new();
    let mut files = vec![paths.bench.join("data/ouvrages-geom.json")];
    if paths.private_present() {
        files.push(paths.private.join("ouvrages-perso.json"));
    }
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let v: Value = serde_json::from_str(&text).context("géométrie des ouvrages")?;
        for o in v.as_array().into_iter().flatten() {
            let Some(id) = o["id"].as_str() else { continue };
            let pts: Vec<LatLon> = o["pts"]
                .as_object()
                .into_iter()
                .flat_map(|m| m.values())
                .filter_map(|p| Some(LatLon::new(p[0].as_f64()?, p[1].as_f64()?)))
                .collect();
            out.insert(id.to_owned(), pts);
        }
    }
    Ok(out)
}

/// Requête d'un cas personnel : géométrie recalée, abscisse de chaque point envoyé sur cette
/// géométrie (`s` de `baro.csv`) et altitude baro transportée.
pub struct PersoCase {
    /// Identifiant du cas : celui de la trace, ou `<trace>.sN` pour un segment disjoint.
    pub id: String,
    pub groupe: String,
    pub serie: Serie,
    /// Requêtes envoyées, une par tronçon (`.partN` : bornes d'entrée de 200 km) ; le tronçon
    /// N+1 repart du dernier point du tronçon N.
    pub parts: Vec<Vec<Pt>>,
    /// Abscisse sur la géométrie recalée entière de chaque point envoyé, tronçons recollés
    /// (point de raccord en double retiré).
    pub s_ref: Vec<f64>,
    pub baro_s: Vec<f64>,
    pub baro_z: Vec<f64>,
    pub masks: Vec<String>,
}

impl std::fmt::Debug for PersoCase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersoCase")
            .field("id", &self.id)
            .field("parts", &self.parts.len())
            .field("points", &self.s_ref.len())
            .finish_non_exhaustive()
    }
}

fn cumulative(pts: &[Pt]) -> Vec<f64> {
    let mut out = Vec::with_capacity(pts.len());
    let mut s = 0.0;
    for (i, p) in pts.iter().enumerate() {
        if i > 0 {
            s += haversine_m(LatLon::new(pts[i - 1].lat, pts[i - 1].lon), LatLon::new(p.lat, p.lon));
        }
        out.push(s);
    }
    out
}

/// Points envoyés pour une géométrie recalée : tels quels, ou rééchantillonnés à 10 m au-delà des
/// bornes d'entrée (T05 §3.4) ; avec leur abscisse sur cette géométrie.
fn request_of(vertices: Vec<Pt>, rules: &BenchRules) -> (Vec<Pt>, Vec<f64>) {
    let cum = cumulative(&vertices);
    if vertices.len() <= 16_000 {
        return (vertices, cum);
    }
    let total = *cum.last().unwrap_or(&0.0);
    let mut s: Vec<f64> =
        (0..).map(|k| k as f64 * rules.request_resample_m).take_while(|x| *x < total).collect();
    s.push(total);
    let lat = interpolate(&cum, &vertices.iter().map(|p| p.lat).collect::<Vec<_>>(), &s, f64::INFINITY);
    let lon = interpolate(&cum, &vertices.iter().map(|p| p.lon).collect::<Vec<_>>(), &s, f64::INFINITY);
    (lat.iter().zip(&lon).map(|(&lat, &lon)| Pt { lat, lon }).collect(), s)
}

fn read_geometry(path: &std::path::Path) -> Result<Vec<Pt>, String> {
    let v: Vec<Pt> =
        gpx::read(path).map_err(|e| e.to_string())?.iter().map(|p| Pt { lat: p.lat, lon: p.lon }).collect();
    if v.len() < 2 {
        return Err("géométrie recalée vide".into());
    }
    Ok(v)
}

/// Baro transporté (`s_recale_m`, `baro_m`, …, `distance_au_trace_m`), trié par abscisse.
fn read_baro(path: &std::path::Path, rules: &BenchRules) -> Result<(Vec<f64>, Vec<f64>), String> {
    let text = std::fs::read_to_string(path).map_err(|_| "baro.csv absent".to_owned())?;
    let mut rows: Vec<(f64, f64)> = Vec::new();
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split(',').collect();
        let (Some(s), Some(z)) =
            (f.first().and_then(|x| x.parse::<f64>().ok()), f.get(1).and_then(|x| x.parse::<f64>().ok()))
        else {
            continue;
        };
        let dist = f.get(3).and_then(|x| x.trim().parse::<f64>().ok());
        if dist.is_some_and(|d| d > rules.match_max_m) {
            continue;
        }
        rows.push((s, z));
    }
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(rows.into_iter().unzip())
}

/// Un cas : géométrie en un ou plusieurs tronçons, baro unique sur l'abscisse recollée.
fn build_case(
    id: String,
    trace: &Trace,
    geometries: Vec<Vec<Pt>>,
    baro: &std::path::Path,
    rules: &BenchRules,
) -> Result<PersoCase, String> {
    let mut parts = Vec::new();
    let mut s_ref: Vec<f64> = Vec::new();
    let mut offset = 0.0;
    for (k, g) in geometries.into_iter().enumerate() {
        let total = *cumulative(&g).last().unwrap_or(&0.0);
        let (request, s) = request_of(g, rules);
        // Le tronçon N+1 repart du dernier point du tronçon N : raccord retiré une fois.
        let skip = usize::from(k > 0);
        s_ref.extend(s.iter().skip(skip).map(|x| x + offset));
        offset += total;
        parts.push(request);
    }
    let (baro_s, baro_z) = read_baro(baro, rules)?;
    Ok(PersoCase {
        id,
        groupe: trace.groupe.clone(),
        serie: trace.serie,
        parts,
        s_ref,
        baro_s,
        baro_z,
        masks: trace.passages_masques.clone(),
    })
}

/// Charge les cas d'une trace personnelle ; `Err` = trace sautée (raison sans coordonnée).
///
/// - `<id>.gpx` : un cas ;
/// - `<id>.part1.gpx`, `.part2.gpx`… : un cas en tronçons (bornes d'entrée), `<id>.baro.csv`
///   unique, abscisse continue sur toute la géométrie recalée ;
/// - `<id>.s1.gpx`, `.s2.gpx`… : segments disjoints (transport coupé), un cas indépendant
///   `<id>.sN` chacun, avec son `<id>.sN.baro.csv`.
pub fn load_perso(paths: &BenchPaths, trace: &Trace, rules: &BenchRules) -> Result<Vec<PersoCase>, String> {
    let dir = paths.private.join("recales");
    let id = &trace.id;
    let file = |name: String| dir.join(name);
    let numbered = |infix: &str| -> Vec<std::path::PathBuf> {
        (1..).map(|n| file(format!("{id}.{infix}{n}.gpx"))).take_while(|p| p.exists()).collect()
    };
    if file(format!("{id}.gpx")).exists() {
        let g = read_geometry(&file(format!("{id}.gpx")))?;
        return Ok(vec![build_case(id.clone(), trace, vec![g], &file(format!("{id}.baro.csv")), rules)?]);
    }
    let parts = numbered("part");
    if !parts.is_empty() {
        let geometries = parts.iter().map(|p| read_geometry(p)).collect::<Result<Vec<_>, _>>()?;
        return Ok(vec![build_case(id.clone(), trace, geometries, &file(format!("{id}.baro.csv")), rules)?]);
    }
    let segments = numbered("s");
    if !segments.is_empty() {
        return segments
            .iter()
            .enumerate()
            .map(|(k, p)| {
                let sid = format!("{id}.s{}", k + 1);
                build_case(
                    sid.clone(),
                    trace,
                    vec![read_geometry(p)?],
                    &file(format!("{sid}.baro.csv")),
                    rules,
                )
            })
            .collect();
    }
    Err("géométrie recalée absente".into())
}

fn distance_options(structure_correction: bool, step: f64) -> Value {
    json!({
        "sampling": {"mode": "distance", "step_m": step},
        "sections": ["elevation", "typology", "summary"],
        "structure_correction": structure_correction
    })
}

/// Profil `distance` d'une réponse : distances, positions, altitudes (`NaN` = absente), station
/// sur un ouvrage (segment pont ou tunnel, ou altitude corrigée d'ouvrage) et terrain IGN.
#[derive(Default)]
struct Profile {
    d: Vec<f64>,
    pts: Vec<LatLon>,
    ele: Vec<f64>,
    on_structure: Vec<bool>,
    corrected: Vec<bool>,
    ign: Vec<bool>,
}

fn profile(r: &Value) -> anyhow::Result<Profile> {
    let points = r["points"].as_array().context("points")?;
    let ranges = |key: &str, f: &dyn Fn(&Value) -> bool| -> Vec<(f64, f64, bool)> {
        r[key]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| (s["from_m"].as_f64().unwrap_or(0.0), s["to_m"].as_f64().unwrap_or(0.0), f(s)))
            .collect()
    };
    let segs = ranges("segments", &|s| matches!(s["structure"].as_str(), Some("bridge" | "tunnel")));
    let terrain: Vec<(f64, f64, bool)> = r["quality"]["terrain"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| {
            (
                s["from_m"].as_f64().unwrap_or(0.0),
                s["to_m"].as_f64().unwrap_or(0.0),
                s["origin"] == "ign_lidar_hd",
            )
        })
        .collect();
    let at = |v: &[(f64, f64, bool)], d: f64| {
        v.iter().find(|s| d >= s.0 && d < s.1).or(v.last()).is_some_and(|s| s.2)
    };
    let mut p = Profile::default();
    for x in points {
        let d = x["d_m"].as_f64().context("d_m")?;
        p.d.push(d);
        p.pts.push(LatLon::new(x["lat"].as_f64().unwrap_or(f64::NAN), x["lon"].as_f64().unwrap_or(f64::NAN)));
        p.ele.push(x["ele"].as_f64().unwrap_or(f64::NAN));
        p.on_structure.push(at(&segs, d));
        p.corrected.push(matches!(x["ele_method"].as_str(), Some("dsm_deck" | "interpolated_structure")));
        p.ign.push(at(&terrain, d));
    }
    Ok(p)
}

/// Seuil de D+ par station de la grille : celui du preset, ou celui des plages `adapted[]` que
/// le service a appliqué (GLO-30, T04 §5.4). Le D+ du banc reproduit ainsi celui du service.
fn thresholds(d: &[f64], r: &Value, preset_threshold: f64) -> Vec<f64> {
    let adapted: Vec<(f64, f64, f64)> = r["summary"]["gain_params"]["adapted"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| Some((a["from_m"].as_f64()?, a["to_m"].as_f64()?, a["threshold_m"].as_f64()?)))
        .collect();
    d.iter()
        .map(|&x| adapted.iter().find(|(a, b, _)| x >= *a && x <= *b).map_or(preset_threshold, |t| t.2))
        .collect()
}

fn round(x: f64, digits: i32) -> f64 {
    let k = 10f64.powi(digits);
    (x * k).round() / k
}

/// Profil `distance` d'un cas en tronçons : chaque tronçon envoyé séparément, `d_m` du tronçon
/// N+1 décalés de la longueur cumulée des précédents, point de raccord en double retiré. Renvoie
/// aussi le seuil de D+ par station et les réponses brutes.
async fn recolle(
    service: &Service,
    case: &PersoCase,
    structure_correction: bool,
    rules: &BenchRules,
    preset_threshold: f64,
) -> anyhow::Result<(Profile, Vec<f64>, Vec<Value>)> {
    let mut p = Profile::default();
    let (mut tau, mut responses) = (Vec::new(), Vec::new());
    let mut offset = 0.0;
    for (k, part) in case.parts.iter().enumerate() {
        let r =
            service.route_details(part, distance_options(structure_correction, rules.grid_step_m)).await?;
        let q = profile(&r)?;
        let t = thresholds(&q.d, &r, preset_threshold);
        let skip = usize::from(k > 0);
        p.d.extend(q.d.iter().skip(skip).map(|x| x + offset));
        p.pts.extend(q.pts.iter().skip(skip));
        p.ele.extend(q.ele.iter().skip(skip));
        p.on_structure.extend(q.on_structure.iter().skip(skip));
        p.corrected.extend(q.corrected.iter().skip(skip));
        p.ign.extend(q.ign.iter().skip(skip));
        tau.extend(t.iter().skip(skip));
        offset += q.d.last().copied().unwrap_or(0.0);
        responses.push(r);
    }
    if responses.is_empty() {
        bail!("cas sans tronçon");
    }
    Ok((p, tau, responses))
}

/// Interpolation linéaire en un point (`xs` croissants), bornée aux extrémités.
pub fn interp_one(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    if xs.is_empty() {
        return x;
    }
    let i = xs.partition_point(|v| *v <= x);
    if i == 0 {
        return ys[0];
    }
    if i >= xs.len() {
        return ys[ys.len() - 1];
    }
    let (x0, x1) = (xs[i - 1], xs[i]);
    if x1 <= x0 {
        return ys[i];
    }
    ys[i - 1] + (x - x0) / (x1 - x0) * (ys[i] - ys[i - 1])
}

/// Distance du service de chaque point envoyé, tronçons recollés (raccord retiré).
fn request_distances(case: &PersoCase) -> Vec<f64> {
    let mut out = Vec::new();
    let mut offset = 0.0;
    for (k, part) in case.parts.iter().enumerate() {
        let c = cumulative(part);
        out.extend(c.iter().skip(usize::from(k > 0)).map(|x| x + offset));
        offset += c.last().copied().unwrap_or(0.0);
    }
    out
}

/// Mesure E1 et E2 d'un cas (service appelé deux fois : corrections actives, puis MNT brut).
pub async fn measure_case(
    service: &Service,
    case: &PersoCase,
    surface_csv: Option<String>,
    refs: &BTreeMap<String, Vec<LatLon>>,
    ouvrages: &[crate::ouvrages::Ouvrage],
    rules: &BenchRules,
    rules_core: &Rules,
) -> anyhow::Result<(Value, Value, Vec<Value>)> {
    let dem = rules_core.elevation.gain.preset(GainPreset::Dem);
    let (p, tau, responses) = recolle(service, case, true, rules, dem.threshold_m).await?;
    let (p_raw, tau_raw, _) = recolle(service, case, false, rules, dem.threshold_m).await?;
    if p_raw.d.len() != p.d.len() {
        bail!("profils de longueurs différentes");
    }
    let r = &responses[0];
    // Abscisse recalée → distance du service (identiques sans rééchantillonnage), tronçons
    // recollés comme les profils.
    let d_srv = request_distances(case);
    let baro_d = interpolate(&case.s_ref, &d_srv, &case.baro_s, f64::INFINITY);
    let (bd, bz): (Vec<f64>, Vec<f64>) =
        baro_d.iter().zip(&case.baro_z).filter(|(d, _)| !d.is_nan()).map(|(d, z)| (*d, *z)).unzip();
    let baro = interpolate(&bd, &bz, &p.d, rules.baro_gap_max_m);

    // Fenêtres masquées (ouvrages d'une autre série que la trace).
    let mut windows = Vec::new();
    let mut not_found = Vec::new();
    for id in &case.masks {
        let w = refs
            .get(id)
            .map(|pts| passage_windows(&p.d, &p.pts, pts, rules.passage_tol_m, rules.mask_half_m))
            .unwrap_or_default();
        if w.is_empty() {
            not_found.push(id.clone());
        }
        windows.extend(w);
    }
    let masked: Vec<bool> = p.d.iter().map(|d| windows.iter().any(|w| w.contains(d))).collect();
    let keep = |v: &[f64], i: usize| {
        if masked[i] || p.ele[i].is_nan() || baro[i].is_nan() {
            f64::NAN
        } else {
            v[i]
        }
    };
    let n = p.d.len();
    let svc: Vec<f64> = (0..n).map(|i| keep(&p.ele, i)).collect();
    let svc_raw: Vec<f64> = (0..n).map(|i| keep(&p_raw.ele, i)).collect();
    let bar: Vec<f64> = (0..n).map(|i| keep(&baro, i)).collect();

    // E1 hors ouvrage (stations `ground`) : ni segment pont ou tunnel, ni altitude d'ouvrage
    // (tablier, interpolation, rampes jusqu'aux sondes).
    let off_structure = |v: &[f64]| -> Vec<f64> {
        (0..n).map(|i| if p.on_structure[i] || p.corrected[i] { f64::NAN } else { v[i] }).collect()
    };
    let m1 =
        e1(&p.d, &off_structure(&svc), &off_structure(&bar), rules.detrend_window_m, rules.detrend_step_m);
    let g = &rules_core.elevation.gain;
    // D+ sur le profil recollé, jamais somme des D+ des tronçons.
    let (dplus_svc, dplus_raw, dplus_baro) = (
        dplus_with(&p.d, &svc, dem, &tau),
        dplus_with(&p.d, &svc_raw, dem, &tau_raw),
        dplus(&p.d, &bar, g.preset(GainPreset::Barometric)),
    );
    let mut warning_codes: BTreeMap<String, usize> = BTreeMap::new();
    let mut terrain_m: BTreeMap<String, f64> = BTreeMap::new();
    let mut adapted = 0;
    for x in &responses {
        for c in x["quality"]["warnings"].as_array().into_iter().flatten().filter_map(|w| w["code"].as_str())
        {
            *warning_codes.entry(c.to_owned()).or_default() += 1;
        }
        for (k, v) in x["summary"]["terrain_m"].as_object().into_iter().flatten() {
            *terrain_m.entry(k.clone()).or_default() += v.as_f64().unwrap_or(0.0);
        }
        adapted += x["summary"]["gain_params"]["adapted"].as_array().map_or(0, Vec::len);
    }
    let per_part = |f: &dyn Fn(&Value) -> Value| -> Value {
        if responses.len() == 1 {
            f(r)
        } else {
            Value::Array(responses.iter().map(f).collect())
        }
    };
    // R3 et M1 : segments du service contre `<id>.surface.csv` (Stadia), en abscisse recalée.
    let mut offsets = Vec::new();
    let mut acc = 0.0;
    for x in &responses {
        offsets.push(acc);
        acc += x["summary"]["length_m"].as_f64().unwrap_or(0.0);
    }
    let to_ref = |x: f64| interp_one(&d_srv, &case.s_ref, x);
    let ours = ours_of(&responses, &offsets, &to_ref);
    let stadia = surface_csv.map(|text| read_stadia_surface(&text)).unwrap_or_default();
    let typo = if stadia.is_empty() {
        let a =
            agreement(&ours, &[Ref { from: f64::NEG_INFINITY, to: f64::INFINITY, class: Class::Unknown }]);
        json!({"M1.non_apparie": round(a.unmatched, 4), "M1.unknown": round(a.unknown, 4), "R3": "reference_absente"})
    } else {
        metrics_json(&agreement(&ours, &stadia), "R3")
    };
    let to_d = |s: f64| interp_one(&case.s_ref, &d_srv, s);
    let typo_diag = json!({
        "M1.plages_non_appariees": unmatched_ranges(&ours, 10, &to_d),
        "R3.desaccords_revetu": disagreements(&ours, &stadia, 10, &to_d),
        "raccords_troncons_d_m": offsets.iter().skip(1).map(|o| round(*o, 1)).collect::<Vec<_>>()
    });
    let step = rules.grid_step_m;
    let masked_m = masked.iter().filter(|m| **m).count() as f64 * step;
    let compared = bar.iter().filter(|x| !x.is_nan()).count();
    let case_json = json!({
        "id": case.id,
        "serie": case.serie,
        "groupe": case.groupe,
        "mode": "distance",
        "cache": "disque",
        "metriques": {
            "E1.rmse_m": round(m1.rmse_m, 3),
            "E1.p95_m": round(m1.p95_m, 3),
            "E2.dplus_service_m": round(dplus_svc, 1),
            "E2.dplus_baro_m": round(dplus_baro, 1),
            "E2.ratio": round(dplus_svc / dplus_baro, 4),
            "E2.dplus_mnt_brut_m": round(dplus_raw, 1),
            "E2.ratio_mnt_brut": round(dplus_raw / dplus_baro, 4)
        },
        "typologie": typo,
        "diagnostics": {
            "E1.offset_baro_moins_service_m": round(m1.offset_m, 2),
            "E1.max_abs_m": round(m1.max_abs_m, 2),
            "E1.max_abs_d_m": round(m1.max_abs_d_m, 1),
            "E1.part_au_dela_de_10m": round(m1.share_over_10m, 4),
            "E1.plage_10m_debut_d_m": m1.longest_over_10m.map(|r| round(r.0, 1)),
            "E1.plage_10m_fin_d_m": m1.longest_over_10m.map(|r| round(r.1, 1)),
            "E1.plage_10m_longueur_m": m1.longest_over_10m.map(|r| round(r.1 - r.0, 1)),
            "stations_comparees": compared,
            "stations": n,
            "part_comparee": round(compared as f64 / n.max(1) as f64, 4),
            "masque_m": masked_m,
            "passages_masques": case.masks,
            "passages_non_retrouves": not_found,
            "troncons": case.parts.len(),
            "points_envoyes": case.parts.iter().map(Vec::len).sum::<usize>(),
            "summary_gain_m": per_part(&|x| x["summary"]["gain_m"].clone()),
            "summary_length_m": per_part(&|x| x["summary"]["length_m"].clone()),
            "status": per_part(&|x| x["status"].clone()),
            "terrain_m": terrain_m.into_iter().map(|(k, v)| (k, json!(round(v, 2)))).collect::<serde_json::Map<_, _>>(),
            "plages_glo30_adaptees": adapted,
            "avertissements": warning_codes,
            "typologie": typo_diag
        }
    });
    // Ouvrages du corpus franchis : fenêtres masquées exclues (série de l'ouvrage différente).
    let baro_unmasked: Vec<f64> = (0..n).map(|i| if masked[i] { f64::NAN } else { baro[i] }).collect();
    let cp = crate::ouvrages::CaseProfile {
        d: &p.d,
        pts: &p.pts,
        svc: &p.ele,
        raw: &p_raw.ele,
        baro: &baro_unmasked,
        on_structure: &p.on_structure,
        ign: &p.ign,
    };
    let items = crate::ouvrages::perso_items(&case.id, &cp, ouvrages, rules.passage_tol_m);
    Ok((case_json, r["meta"]["data_versions"].clone(), items))
}

/// Fixture hors git du manifest, empreinte vérifiée ; `None` (cas sauté) sans `bench/private/`.
pub fn hors_git_file(paths: &BenchPaths, manifest: &Manifest, id: &str) -> anyhow::Result<Option<PathBuf>> {
    let entry =
        manifest.hors_git(id).with_context(|| format!("{id} absent de la section hors_git du manifest"))?;
    if !paths.private_present() {
        return Ok(None);
    }
    let path = paths.resolve(&entry.fichier);
    let actual = crate::hash::sha256_file(&path).with_context(|| format!("{id} : fichier absent"))?;
    if actual != entry.sha256 {
        bail!("{id} : empreinte différente de celle du manifest");
    }
    Ok(Some(path))
}

/// Date UTC du jour (`AAAA-MM-JJ`), sans dépendance de calendrier.
pub fn today_utc() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn git_commit(repo: &Path) -> String {
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    let head = run(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "inconnu".into());
    let dirty = run(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    if dirty {
        format!("{head}-dirty")
    } else {
        head
    }
}

/// Emplacement de `results.json` prévu par T05 §5.2 : `<date>-<rules_version>`, suivi de
/// `-<révision du banc>` quand seul l'outil de mesure a changé (règles du service inchangées).
pub fn default_report_dir(paths: &BenchPaths) -> PathBuf {
    let mut name = format!("{}-{}", today_utc(), ord_core::RULES_VERSION);
    if let Some(rev) = crate::BENCH_REVISION {
        name.push('-');
        name.push_str(rev);
    }
    paths.bench.join("reports").join(name)
}

pub fn ensure_check(report: &CheckReport) -> anyhow::Result<()> {
    if !report.failures.is_empty() {
        bail!("empreintes du banc non conformes : {}", report.failures.join(" ; "));
    }
    Ok(())
}

/// Seuils E1 d'une strate (`criteria.yaml`, E1) : `(rmse_m_max, p95_m_max)`.
fn e1_thresholds(groupe: &str) -> Option<(f64, f64)> {
    match groupe {
        "plaine" => Some((1.5, 4.0)),
        "dplus" => Some((3.0, 8.0)),
        _ => None,
    }
}

/// Bornes indicatives d'E2 (`criteria.yaml`) : par trace, et médiane du groupe.
const E2_TRACE: (f64, f64) = (0.90, 1.20);
const E2_GROUP_MEDIAN: (f64, f64) = (0.95, 1.12);
/// Groupes jugés « France » pour E2 et E2b (`criteria.yaml`, `terrain: france`).
const E2_GROUPS: [&str; 3] = ["dplus", "plaine", "ouvrage"];

fn within(x: f64, (lo, hi): (f64, f64)) -> bool {
    x >= lo && x <= hi
}

/// Verdicts E1, E2 (indicatif), E2b (bloquant) à partir des cas mesurés.
fn judge(cases: &[Value]) -> (Value, Value) {
    let metric = |c: &Value, k: &str| c["metriques"][k].as_f64().unwrap_or(f64::NAN);
    let groupe = |c: &Value| c["groupe"].as_str().unwrap_or("").to_owned();
    let mut groups: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for c in cases {
        groups.entry(groupe(c)).or_default().push(c);
    }
    let mut agg_e1 = serde_json::Map::new();
    let mut agg_e2 = serde_json::Map::new();
    let mut agg_e2b = serde_json::Map::new();
    let mut e1_fail = Vec::new();
    let mut e1_judged = 0usize;
    let mut e2b_fail = Vec::new();
    let mut e2b_judged = 0usize;
    let mut e2b_neutral = 0usize;
    let mut e2_out = Vec::new();
    for (g, list) in &groups {
        let rmse: Vec<f64> = list.iter().map(|c| metric(c, "E1.rmse_m")).collect();
        let p95: Vec<f64> = list.iter().map(|c| metric(c, "E1.p95_m")).collect();
        let ratio: Vec<f64> = list.iter().map(|c| metric(c, "E2.ratio")).collect();
        let ratio_raw: Vec<f64> = list.iter().map(|c| metric(c, "E2.ratio_mnt_brut")).collect();
        agg_e1.insert(format!("{g}.rmse_m_mediane"), json!(round(median(&rmse), 3)));
        agg_e1.insert(format!("{g}.p95_m_mediane"), json!(round(median(&p95), 3)));
        agg_e1.insert(
            format!("{g}.rmse_m_max"),
            json!(round(rmse.iter().copied().fold(f64::NAN, f64::max), 3)),
        );
        agg_e1
            .insert(format!("{g}.p95_m_max"), json!(round(p95.iter().copied().fold(f64::NAN, f64::max), 3)));
        if let Some((rmse_max, p95_max)) = e1_thresholds(g) {
            for c in list {
                e1_judged += 1;
                let (r, q) = (metric(c, "E1.rmse_m"), metric(c, "E1.p95_m"));
                if !(r <= rmse_max && q <= p95_max) {
                    e1_fail.push(json!({"id": c["id"], "groupe": g, "rmse_m": r, "p95_m": q}));
                }
            }
        }
        let med = median(&ratio);
        agg_e2.insert(format!("{g}.ratio_mediane"), json!(round(med, 4)));
        agg_e2.insert(format!("{g}.ratio_mnt_brut_mediane"), json!(round(median(&ratio_raw), 4)));
        if E2_GROUPS.contains(&g.as_str()) {
            let traces_out: Vec<Value> = list
                .iter()
                .filter(|c| !within(metric(c, "E2.ratio"), E2_TRACE))
                .map(|c| json!({"id": c["id"], "ratio": metric(c, "E2.ratio")}))
                .collect();
            e2_out.push(json!({
                "groupe": g,
                "mediane": round(med, 4),
                "mediane_dans_bornes": within(med, E2_GROUP_MEDIAN),
                "traces_hors_bornes": traces_out
            }));
            // E2b : médiane |ln(service / baro)| ≤ médiane |ln(MNT brut / baro)|.
            let ln_svc: Vec<f64> = ratio.iter().map(|r| r.ln().abs()).collect();
            let ln_raw: Vec<f64> = ratio_raw.iter().map(|r| r.ln().abs()).collect();
            let (ms, mr) = (median(&ln_svc), median(&ln_raw));
            // Service et référence identiques sur tous les cas : le critère ne mesure rien (ni
            // réussite ni échec), ce qui est le cas tant qu'aucun ouvrage n'est corrigé.
            let identical =
                list.iter().all(|c| metric(c, "E2.dplus_service_m") == metric(c, "E2.dplus_mnt_brut_m"));
            let v = if identical {
                e2b_neutral += 1;
                "non_discriminant"
            } else if ms <= mr + 1e-12 {
                e2b_judged += 1;
                "pass"
            } else {
                e2b_judged += 1;
                e2b_fail.push(g.clone());
                "fail"
            };
            agg_e2b.insert(
                g.clone(),
                json!({
                    "mediane_ecart_log_service": round(ms, 5),
                    "mediane_ecart_log_mnt_brut": round(mr, 5),
                    "verdict": v
                }),
            );
        }
    }
    let verdict = |judged: usize, fails: usize| match (judged, fails) {
        (0, _) => "skipped",
        (_, 0) => "pass",
        _ => "fail",
    };
    let verdicts = json!({
        "E1": verdict(e1_judged, e1_fail.len()),
        "E1_echecs": e1_fail,
        "E2": if e2_out.is_empty() { "skipped" } else { "reported" },
        "E2_indicatif": e2_out,
        "E2b": match (e2b_judged, e2b_neutral) {
            (0, n) if n > 0 => "non_discriminant",
            (j, _) => verdict(j, e2b_fail.len()),
        },
        "E2b_groupes_en_echec": e2b_fail,
    });
    (json!({"E1": agg_e1, "E2": agg_e2, "E2b": agg_e2b}), verdicts)
}

/// Verdicts R1, R2 (`os-*`), R3 (traces perso) et M1 (les deux), par parcours (`criteria.yaml`).
fn judge_typology(os_cases: &[Value], perso: &[Value]) -> (Value, Value) {
    use crate::typology::{M1_UNKNOWN_MAX, M1_UNMATCHED_MAX, R_CLASS_MIN, R_PAVED_MIN};
    let m = |c: &Value, k: &str| c["typologie"][k].as_f64().unwrap_or(f64::NAN);
    let mut fails_r12 = Vec::new();
    let mut fails_r3 = Vec::new();
    let mut fails_m1 = Vec::new();
    let mut per_group: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    let mut r3_judged = 0;
    for c in os_cases {
        let ok = m(c, "R1.accord_classe") >= R_CLASS_MIN
            && m(c, "R1.accord_classe_hors_inconnu") >= R_CLASS_MIN
            && m(c, "R1.accord_revetu_non_revetu") >= R_PAVED_MIN;
        if !ok {
            fails_r12.push(c["id"].clone());
        }
        per_group.entry("osm_surface".into()).or_default().push(c);
    }
    for c in perso {
        per_group.entry(c["groupe"].as_str().unwrap_or("").to_owned()).or_default().push(c);
        if c["typologie"]["R3"].is_string() {
            continue;
        }
        r3_judged += 1;
        let ok = m(c, "R3.accord_classe") >= R_CLASS_MIN
            && m(c, "R3.accord_classe_hors_inconnu") >= R_CLASS_MIN
            && m(c, "R3.accord_revetu_non_revetu") >= R_PAVED_MIN;
        if !ok {
            fails_r3.push(c["id"].clone());
        }
    }
    for c in os_cases.iter().chain(perso) {
        if !(m(c, "M1.non_apparie") <= M1_UNMATCHED_MAX && m(c, "M1.unknown") <= M1_UNKNOWN_MAX) {
            fails_m1.push(c["id"].clone());
        }
    }
    let mut agg = serde_json::Map::new();
    for (g, list) in &per_group {
        let mut a = serde_json::Map::new();
        for k in [
            "R1.accord_classe",
            "R1.accord_classe_hors_inconnu",
            "R1.accord_revetu_non_revetu",
            "R3.accord_classe",
            "R3.accord_classe_hors_inconnu",
            "R3.accord_revetu_non_revetu",
            "M1.non_apparie",
            "M1.unknown",
        ] {
            let v: Vec<f64> = list.iter().map(|c| m(c, k)).filter(|x| !x.is_nan()).collect();
            if v.is_empty() {
                continue;
            }
            a.insert(format!("{k}.mediane"), json!(round(median(&v), 4)));
            a.insert(format!("{k}.min"), json!(round(v.iter().copied().fold(f64::INFINITY, f64::min), 4)));
            a.insert(
                format!("{k}.max"),
                json!(round(v.iter().copied().fold(f64::NEG_INFINITY, f64::max), 4)),
            );
        }
        a.insert("cas".into(), json!(list.len()));
        agg.insert(g.clone(), Value::Object(a));
    }
    let verdict = |judged: usize, fails: usize| match (judged, fails) {
        (0, _) => "skipped",
        (_, 0) => "pass",
        _ => "fail",
    };
    let verdicts = json!({
        "R1_R2": verdict(os_cases.len(), fails_r12.len()),
        "R1_R2_echecs": fails_r12,
        "R3": verdict(r3_judged, fails_r3.len()),
        "R3_echecs": fails_r3,
        "M1": verdict(os_cases.len() + perso.len(), fails_m1.len()),
        "M1_echecs": fails_m1,
    });
    (Value::Object(agg), verdicts)
}

/// `run --offline` : campagne de calibration ; renvoie le contenu de `results.json`.
pub async fn run(paths: &BenchPaths, groupes: &[String], ids: &[String]) -> anyhow::Result<Value> {
    let report = check(paths)?;
    ensure_check(&report)?;
    let manifest = Manifest::load(&paths.bench)?;
    let selection = select(&manifest, groupes, ids);
    eprintln!(
        "cas retenus : {} ; exclus (série tenue à l'écart) avant toute lecture : {}",
        selection.cases.len(),
        selection.excluded_held_out.len()
    );
    let rules = BenchRules::default();
    let rules_core = Rules::default();
    let refs = ouvrage_refs(paths)?;
    let ouvrages = crate::ouvrages::calibration_ouvrages(paths, &manifest.ouvrages_tenus)?;
    let service = Service::start(&paths.cache, true, Variant::DEFAULT).await?;
    let mut items: Vec<Value> = Vec::new();
    let mut cases = Vec::new();
    let mut skipped = Vec::new();
    let mut data_versions = Value::Null;
    for t in &selection.cases {
        if !paths.private_present() {
            skipped.push(json!({"id": t.id, "raison": "bench/private/ absent"}));
            continue;
        }
        let loaded = match load_perso(paths, t, &rules) {
            Ok(c) => c,
            Err(reason) => {
                skipped.push(json!({"id": t.id, "raison": reason}));
                continue;
            }
        };
        for case in loaded {
            let csv = std::fs::read_to_string(
                paths.private.join("recales").join(format!("{}.surface.csv", case.id)),
            )
            .ok();
            match measure_case(&service, &case, csv, &refs, &ouvrages, &rules, &rules_core).await {
                Ok((c, dv, its)) => {
                    eprintln!("{} : mesuré", case.id);
                    cases.push(c);
                    items.extend(its);
                    if data_versions.is_null() {
                        data_versions = dv;
                    }
                }
                Err(e) => skipped.push(json!({"id": case.id, "raison": e.to_string()})),
            }
        }
    }
    // Parcours d'osm-surface : R1, R2, M1 et typologie à chaud.
    let os_wanted = |id: &str| {
        (ids.is_empty() && groupes.iter().any(|g| g == "osm_surface")) || ids.iter().any(|i| i == id)
    };
    let mut os_cases = Vec::new();
    let mut warm = Vec::new();
    for entry in manifest.osm_surface.iter().filter(|e| os_wanted(&e.id) && e.serie == Serie::Calibration) {
        let route = crate::osm_surface::load(&paths.bench, entry)?;
        match crate::osm_surface::measure(&service, &route).await {
            Ok(c) => {
                eprintln!("{} : mesuré", route.id);
                os_cases.push(c);
                let (ms, stats) = crate::osm_surface::typology_warm_ms(&paths.cache, &route, 5).await?;
                warm.push(json!({"id": route.id, "typologie_a_chaud_ms": round(ms, 1), "stats": stats}));
            }
            Err(e) => skipped.push(json!({"id": route.id, "raison": e.to_string()})),
        }
    }
    // Ouvrages : parcours `test1..4` de vcyclist et témoins (calibration).
    let structures_wanted = ids.is_empty();
    if structures_wanted {
        for route in ["test1", "test2", "test3", "test4"] {
            match crate::ouvrages::vc_route_items(&service, &paths.bench, route).await {
                Ok(its) => items.extend(its),
                Err(e) => skipped.push(json!({"id": format!("vc-{route}"), "raison": e.to_string()})),
            }
        }
        for (dir, id, role) in crate::ouvrages::controls(&paths.bench)? {
            match crate::ouvrages::control_items(&service, &paths.bench, &dir, &id, &role).await {
                Ok(its) => {
                    eprintln!("{id} : mesuré");
                    items.extend(its);
                }
                Err(e) => skipped.push(json!({"id": id, "raison": e.to_string()})),
            }
        }
    }
    let (ouvrages_agg, ouvrages_verdicts) = crate::ouvrages::judge(&items);
    let (agregats, mut verdicts) = judge(&cases);
    let (typo_agregats, typo_verdicts) = judge_typology(&os_cases, &cases);
    for (k, v) in typo_verdicts.as_object().into_iter().flatten() {
        verdicts[k] = v.clone();
    }
    let warm_total: f64 = warm.iter().filter_map(|w| w["typologie_a_chaud_ms"].as_f64()).sum();
    let warm_km: f64 = warm.iter().filter_map(|w| w["stats"]["km"].as_f64()).sum();
    let mut agregats = agregats;
    agregats["typologie"] = typo_agregats;
    agregats["typologie_a_chaud"] = json!({
        "parcours": warm,
        "total_ms": round(warm_total, 1),
        "total_km": round(warm_km, 1),
        "seuil_ms": 160.0,
        "profil": if cfg!(debug_assertions) { "debug" } else { "release" }
    });
    agregats["ouvrages"] = ouvrages_agg;
    for (k, v) in ouvrages_verdicts.as_object().into_iter().flatten() {
        verdicts[k] = v.clone();
    }
    cases.extend(os_cases);
    verdicts["Q1"] = json!(if report.q1.pass() { "pass" } else { "fail" });
    let fingerprint = cache_fingerprint(&paths.cache).context("empreinte du cache de banc")?;
    let rules_fingerprint = sha256_bytes(serde_json::to_string(&rules_core)?.as_bytes());
    let bench_rules_sha256 = sha256_bytes(serde_json::to_string(&rules)?.as_bytes());
    let results = json!({
        "schema": 1,
        "campagne": {
            "date": today_utc(),
            "type": "calibration",
            "increment": "MVP",
            "increment_plan": 4,
            "revision_banc": crate::BENCH_REVISION,
            "commit": git_commit(&paths.bench),
            "api_version": ord_server::API_VERSION,
            "rules_version": ord_core::RULES_VERSION,
            "rules_fingerprint": rules_fingerprint,
            "data_fingerprint": fingerprint.sha256,
            "data_versions": data_versions,
            "manifest_sha256": report.manifest_sha256,
            "criteria_sha256": report.criteria_sha256,
            "bench_rules_sha256": bench_rules_sha256,
            "bench_rules": rules,
            "private_present": paths.private_present(),
            "groupes": groupes,
            "exclus_serie_tenue_a_l_ecart": selection.excluded_held_out,
            "notes": [
                "E1 : stations hors fenêtres masquées et hors ouvrage (segment pont ou tunnel, ou altitude dsm_deck / interpolated_structure, rampes comprises)",
                "E3 : baro corrigé localement (médiane baro − service au sol entre 1 et 3 km du milieu) sur les traces perso ; médiane MNS 25 m de vcyclist (B, cohérence) sur test1..3 ; un pont passe si tous ses passages passent",
                "E4 : droite entre les MNT bruts (structure_correction=false) juste hors des portails (perso, témoins tt*/th*) ; vérité N de vcyclist (test4) ; D+ fictif = D+ dem du service − D+ dem de la droite sur la plage",
                "E5 : |service − MNT brut| au milieu des passages dessous (perso) et sur les stations O des témoins négatifs",
                "S1 : longueur typée pont/tunnel sur l'emprise des ouvrages tagués (perso, structures.csv bridge/tunnel/missed, témoins D/N) ; S2 : témoins négatifs n*, tn*, hn*, longueur typée hors plages X/x ; p06-normandie exclu (ouvrage tenu à l'écart)",
                "Traces perso tenues à l'écart non envoyées : leurs passages d'ouvrages de calibration ne sont pas mesurés",
                "R1/R2/R3 : classe Stadia ramenée à paved/unpaved/unknown (revêtu = paved_smooth, paved, paved_rough ; non revêtu = pire que paved_rough) ; R1 total et hors inconnu Stadia, R2 sur les mètres classables des deux côtés",
                "M1 : non apparié = longueur matched=false ; unknown = longueur surface.class=unknown ; parts de la longueur du service",
                "R3 : accord entre deux dérivés d'OSM (VersaTiles vs Stadia), pas une vérité terrain",
                "E2b : non_discriminant quand le service et sa référence structure_correction=false sont identiques sur tous les cas d'un groupe (ni réussite ni échec ; aucune correction d'ouvrage avant l'incrément 4)",
                "Tronçons .partN recollés en un cas (E1 sur la grille entière, D+ du profil recollé) ; segments .sN : un cas chacun"
            ]
        },
        "cas": cases,
        "ouvrages": crate::ouvrages::public(&items),
        "cas_sautes": skipped,
        "agregats": agregats,
        "verdicts": verdicts,
        "diagnostics": {"cache": {"tuiles": fingerprint.tiles, "octets": fingerprint.bytes}}
    });
    crate::privacy::assert_report_clean(&results, &crate::privacy::private_names(paths))?;
    Ok(results)
}

/// `fill-cache` : mêmes requêtes que `run`, en ligne, plus les cohérences `test1..4` (service
/// complet ; `test4` aussi en Mapterhorn seul, deux conventions), les fixtures hors git
/// (Mapterhorn seul, test de méthode), les scénarios golden et des GPX supplémentaires. Écrit
/// `cache.lock` dans le cache.
#[allow(clippy::too_many_arguments)]
pub async fn fill_cache(
    paths: &BenchPaths,
    groupes: &[String],
    ids: &[String],
    coherence_routes: &[String],
    hors_git: &[String],
    goldens: &[String],
    extra_gpx: &[PathBuf],
) -> anyhow::Result<Value> {
    let report = check(paths)?;
    ensure_check(&report)?;
    let manifest = Manifest::load(&paths.bench)?;
    let selection = select(&manifest, groupes, ids);
    let rules = BenchRules::default();
    let service = Service::start(&paths.cache, false, Variant::DEFAULT).await?;
    let mapterhorn = Service::start(&paths.cache, false, Variant::MAPTERHORN).await?;
    let mut filled = Vec::new();
    if paths.private_present() {
        for t in &selection.cases {
            let Ok(loaded) = load_perso(paths, t, &rules) else { continue };
            for case in loaded {
                for part in &case.parts {
                    for sc in [true, false] {
                        service.route_details(part, distance_options(sc, rules.grid_step_m)).await?;
                    }
                }
                eprintln!("{} : tuiles en cache", case.id);
                filled.push(case.id.clone());
            }
        }
    }
    for entry in manifest.osm_surface.iter().filter(|e| {
        e.serie == Serie::Calibration
            && ((ids.is_empty() && groupes.iter().any(|g| g == "osm_surface")) || ids.contains(&e.id))
    }) {
        let route = crate::osm_surface::load(&paths.bench, entry)?;
        service.route_details(&route.points, crate::osm_surface::options()).await?;
        eprintln!("{} : tuiles en cache", route.id);
        filled.push(route.id.clone());
    }
    for route in coherence_routes {
        crate::coherence::measure(&paths.cache, &paths.bench, route, false, true, false).await?;
        if route == "test4" {
            crate::coherence::measure(&paths.cache, &paths.bench, route, false, false, true).await?;
        }
        let gpx_path = paths.bench.join(format!("data/vcyclist/{route}.gpx"));
        let pts: Vec<Pt> = gpx::read(&gpx_path)?.iter().map(|p| Pt { lat: p.lat, lon: p.lon }).collect();
        service.route_details(&pts, distance_options(true, rules.grid_step_m)).await?;
        eprintln!("vc-{route} : tuiles en cache (cohérence)");
        filled.push(format!("vc-{route}"));
    }
    for id in hors_git {
        match hors_git_file(paths, &manifest, id)? {
            Some(path) => {
                let pts: Vec<Pt> = gpx::read(&path)?.iter().map(|p| Pt { lat: p.lat, lon: p.lon }).collect();
                crate::coherence::sample_points(&mapterhorn, &pts).await?;
                eprintln!("{id} : tuiles en cache");
                filled.push(id.clone());
            }
            None => eprintln!("{id} : sauté (bench/private/ absent)"),
        }
    }
    if ids.is_empty() {
        for (dir, id, _) in crate::ouvrages::controls(&paths.bench)? {
            let pts = crate::ouvrages::control_points(&paths.bench, &dir, &id)?;
            for sc in [true, false] {
                crate::ouvrages::query_points(&service, &pts, sc).await?;
            }
            eprintln!("{id} : tuiles en cache");
            filled.push(id);
        }
    }
    for id in crate::golden::STRUCTURE_SCENARIOS {
        let scenario = crate::golden::structure_scenario(paths, &manifest, id)?;
        crate::golden::response_with(&service, &scenario, crate::golden::structure_options()).await?;
        filled.push(format!("golden-{id}"));
    }
    for id in goldens {
        let scenario = crate::golden::scenario(paths, &manifest, id)?;
        crate::golden::response(&service, &scenario).await?;
        eprintln!("golden {id} : tuiles en cache");
        filled.push(format!("golden-{id}"));
    }
    for path in extra_gpx {
        let pts: Vec<Pt> = gpx::read(path)?.iter().map(|p| Pt { lat: p.lat, lon: p.lon }).collect();
        crate::coherence::sample_points(&service, &pts).await?;
        eprintln!("GPX supplémentaire : tuiles en cache");
        filled.push("gpx".into());
    }
    let fingerprint = cache_fingerprint(&paths.cache)?;
    let lock = json!({
        "date": today_utc(),
        "data_fingerprint": fingerprint.sha256,
        "tuiles": fingerprint.tiles,
        "octets": fingerprint.bytes,
        "cas": filled,
        "exclus_serie_tenue_a_l_ecart": selection.excluded_held_out
    });
    crate::privacy::assert_report_clean(&lock, &crate::privacy::private_names(paths))?;
    std::fs::create_dir_all(&paths.cache)?;
    std::fs::write(paths.cache.join("cache.lock"), serde_json::to_string_pretty(&lock)? + "\n")?;
    Ok(lock)
}

/// L1 (latence à froid, live) : chaque essai part d'un cache disque neuf, temporaire, et d'un
/// service neuf (mémoire vide). Durée de la requête `distance` 5 m, comme la campagne.
pub async fn latency_cold(paths: &BenchPaths, ids: &[String], trials: usize) -> anyhow::Result<Value> {
    let report = check(paths)?;
    ensure_check(&report)?;
    let manifest = Manifest::load(&paths.bench)?;
    let selection = select(&manifest, &[], ids);
    let rules = BenchRules::default();
    let mut out = Vec::new();
    let cases: Vec<PersoCase> = selection
        .cases
        .iter()
        .map(|t| load_perso(paths, t, &rules).map_err(|e| anyhow::anyhow!("{} : {e}", t.id)))
        .collect::<anyhow::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();
    for case in &cases {
        // L1 porte sur une requête : un cas en tronçons n'est pas mesurable tel quel.
        if case.parts.len() != 1 {
            bail!("{} : cas en tronçons, hors du protocole L1", case.id);
        }
        let mut durations = Vec::new();
        let mut terrain = Value::Null;
        let mut length = Value::Null;
        for _ in 0..trials.max(1) {
            let dir = tempfile::tempdir().context("cache temporaire")?;
            let service = Service::start(dir.path(), false, Variant::DEFAULT).await?;
            let start = std::time::Instant::now();
            let r = service.route_details(&case.parts[0], distance_options(true, rules.grid_step_m)).await?;
            durations.push(start.elapsed().as_secs_f64());
            terrain = r["summary"]["terrain_m"].clone();
            length = r["summary"]["length_m"].clone();
        }
        let mut sorted = durations.clone();
        sorted.sort_by(f64::total_cmp);
        out.push(json!({
            "id": case.id,
            "groupe": case.groupe,
            "serie": case.serie,
            "longueur_service_m": length,
            "terrain_m": terrain,
            "essais": durations.iter().map(|d| round(*d, 2)).collect::<Vec<_>>(),
            "p95_s": round(crate::metrics::percentile(&sorted, 95.0), 2)
        }));
    }
    let result = json!({"date": today_utc(), "rules_version": ord_core::RULES_VERSION, "L1": out});
    crate::privacy::assert_report_clean(&result, &crate::privacy::private_names(paths))?;
    Ok(result)
}
