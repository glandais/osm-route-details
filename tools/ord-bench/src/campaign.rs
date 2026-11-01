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
use crate::service::{BenchMode, Pt, Service, Variant};
use crate::typology::{
    agreement, disagreements, metrics_json, ours_of, read_stadia_surface, unmatched_ranges, Class, Ref,
};
use crate::{gpx, BenchPaths, BenchRules};

/// Sélection des cas : l'autre série est exclue ici, avant toute lecture. En calibration,
/// `excluded_held_out` liste les traces tenues à l'écart ; au jugement (`judge`), les traces de
/// calibration.
#[derive(Clone, Debug)]
pub struct Selection<'a> {
    pub cases: Vec<&'a Trace>,
    pub excluded_held_out: Vec<String>,
}

/// Sélection de la calibration.
pub fn select<'a>(manifest: &'a Manifest, groupes: &[String], ids: &[String]) -> Selection<'a> {
    select_series(manifest, groupes, ids, Serie::Calibration)
}

/// Sélection des traces de la série `target` ; les traces de l'autre série sont seulement listées.
pub fn select_series<'a>(
    manifest: &'a Manifest,
    groupes: &[String],
    ids: &[String],
    target: Serie,
) -> Selection<'a> {
    let wanted = |t: &Trace| {
        (ids.is_empty() || ids.contains(&t.id))
            && (groupes.is_empty() || groupes.contains(&t.groupe))
            && t.groupe != "negatif"
    };
    let mut cases = Vec::new();
    let mut excluded_held_out = Vec::new();
    for t in manifest.traces.iter().filter(|t| wanted(t)) {
        if t.serie == target {
            cases.push(t);
        } else {
            excluded_held_out.push(t.id.clone());
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
    /// Point d'origine inséré à la place d'un saut (`insere = 1`), aligné sur `baro_s` ; son
    /// baro n'est gardé qu'après `inserted::apply` (README §2.1, étape 3).
    pub baro_inserted: Vec<bool>,
    pub masks: Vec<String>,
    /// Extrait d'une trace tenue à l'écart : points de la trace jusqu'à 3 km de la fenêtre
    /// (abscisse de l'extrait, position, baro transporté), pour le seul repli 1–3 km du décalage
    /// local (baro − MNT, données de référence) ; jamais envoyés au service.
    pub ring: Vec<RingPoint>,
    /// Trace brute (GPS d'origine) et abscisse du cas dans celle de la trace recalée entière
    /// (`shift` : début d'un extrait), pour l'attribution des passages (README §4 ter.2).
    pub raw: Option<RawRef>,
}

/// Trace brute d'un cas (`private/traces/<id>.trim1km.gpx`, segment `.sN` découpé comme par
/// `tools/stadia_match.py`) et correspondance abscisse recalée → abscisse brute (colonnes
/// `s_recale_m` et `s_origine_m` de `<id>.baro.csv`).
#[derive(Clone)]
pub struct RawGps {
    pub pts: Vec<LatLon>,
    pub s: Vec<f64>,
    map_s: Vec<f64>,
    map_o: Vec<f64>,
}

impl std::fmt::Debug for RawGps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawGps").field("points", &self.pts.len()).finish_non_exhaustive()
    }
}

impl RawGps {
    /// Points bruts dont l'abscisse d'origine est entre celles de `s_a` et `s_b` (abscisses
    /// recalées), par interpolation de la correspondance du transport du baro.
    pub fn between(&self, s_a: f64, s_b: f64) -> Vec<LatLon> {
        if self.map_s.is_empty() {
            return Vec::new();
        }
        let (oa, ob) = (interp_one(&self.map_s, &self.map_o, s_a), interp_one(&self.map_s, &self.map_o, s_b));
        let (lo, hi) = (oa.min(ob), oa.max(ob));
        let i = self.s.partition_point(|x| *x < lo);
        let j = self.s.partition_point(|x| *x <= hi);
        self.pts[i..j].to_vec()
    }
}

/// Trace brute d'un cas et décalage de son abscisse (extrait).
#[derive(Clone, Debug)]
pub struct RawRef {
    pub gps: std::sync::Arc<RawGps>,
    pub shift: f64,
}

impl RawRef {
    pub fn between(&self, s_a: f64, s_b: f64) -> Vec<LatLon> {
        self.gps.between(s_a + self.shift, s_b + self.shift)
    }
}

/// Saut entre deux points bruts qui coupe la trace en segments, et longueur minimale d'un
/// segment gardé (`GAP_M`, `MIN_SEG_KM` de `tools/stadia_match.py`).
const RAW_GAP_M: f64 = 2_000.0;
const RAW_MIN_SEGMENT_M: f64 = 5_000.0;

/// Trace brute d'un cas : `<trace>.trim1km.gpx` (points avec altitude), segments coupés aux sauts
/// de plus de 2 km et gardés à partir de 5 km ; `segment` = rang `.sN` (1…), `None` pour une trace
/// d'un seul segment. Correspondance lue dans `baro` (toutes les lignes). `None` si illisible.
fn load_raw(trace: &std::path::Path, baro: &std::path::Path, segment: Option<usize>) -> Option<RawGps> {
    let pts: Vec<LatLon> = gpx::read(trace)
        .ok()?
        .into_iter()
        .filter(|p| p.ele.is_some())
        .map(|p| LatLon::new(p.lat, p.lon))
        .collect();
    let mut segs: Vec<Vec<LatLon>> = vec![Vec::new()];
    for (k, p) in pts.iter().enumerate() {
        if k > 0 && haversine_m(pts[k - 1], *p) > RAW_GAP_M {
            segs.push(Vec::new());
        }
        segs.last_mut()?.push(*p);
    }
    let length = |g: &[LatLon]| g.windows(2).map(|w| haversine_m(w[0], w[1])).sum::<f64>();
    let segs: Vec<Vec<LatLon>> =
        segs.into_iter().filter(|g| g.len() > 1 && length(g) >= RAW_MIN_SEGMENT_M).collect();
    let seg = match (segment, segs.len()) {
        (None, 1) => segs.into_iter().next()?,
        (Some(k), n) if k >= 1 && k <= n && n > 1 => segs.into_iter().nth(k - 1)?,
        _ => return None,
    };
    let mut s = vec![0.0];
    for w in seg.windows(2) {
        s.push(s.last().copied().unwrap_or(0.0) + haversine_m(w[0], w[1]));
    }
    let text = std::fs::read_to_string(baro).ok()?;
    let mut rows: Vec<(f64, f64)> = text
        .lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            Some((f.first()?.trim().parse().ok()?, f.get(2)?.trim().parse().ok()?))
        })
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (map_s, map_o) = rows.into_iter().unzip();
    Some(RawGps { pts: seg, s, map_s, map_o })
}

/// Point de référence du repli 1–3 km d'un extrait tenu à l'écart.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RingPoint {
    pub s: f64,
    pub at: LatLon,
    pub baro: f64,
}

/// Portée du repli du décalage local autour de la fenêtre d'un extrait (1–3 km du milieu).
const HELD_OUT_RING_M: f64 = 3_000.0;

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

/// Baro transporté (`s_recale_m`, `baro_m`, …, `distance_au_trace_m`, `insere`), trié par
/// abscisse : abscisses, altitudes, point inséré.
type Baro = (Vec<f64>, Vec<f64>, Vec<bool>);

fn read_baro(path: &std::path::Path, rules: &BenchRules) -> Result<Baro, String> {
    let text = std::fs::read_to_string(path).map_err(|_| "baro.csv absent".to_owned())?;
    parse_baro(&text, rules)
}

fn parse_baro(text: &str, rules: &BenchRules) -> Result<Baro, String> {
    let header: Vec<&str> = text.lines().next().unwrap_or_default().split(',').map(str::trim).collect();
    if header.get(4) != Some(&"insere") {
        return Err("baro.csv sans colonne insere (recalage antérieur à 53225a8)".into());
    }
    let mut rows: Vec<(f64, f64, bool)> = Vec::new();
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
        let inserted = match f.get(4).map(|x| x.trim()) {
            Some("0") => false,
            Some("1") => true,
            _ => return Err("baro.csv : colonne insere illisible".into()),
        };
        rows.push((s, z, inserted));
    }
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Baro = (Vec::new(), Vec::new(), Vec::new());
    for (s, z, i) in rows {
        out.0.push(s);
        out.1.push(z);
        out.2.push(i);
    }
    Ok(out)
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
    let (baro_s, baro_z, baro_inserted) = read_baro(baro, rules)?;
    Ok(PersoCase {
        id,
        groupe: trace.groupe.clone(),
        serie: trace.serie,
        parts,
        s_ref,
        baro_s,
        baro_z,
        baro_inserted,
        masks: trace.passages_masques.clone(),
        ring: Vec::new(),
        raw: None,
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
    let mut cases = load_perso_geometry(paths, trace, rules)?;
    // Trace brute (attribution des passages, README §4 ter.2) ; absente : passages non tranchés.
    let raw_file = paths.private.join("traces").join(format!("{}.trim1km.gpx", trace.id));
    let several = cases.len() > 1 || cases.first().is_some_and(|c| c.id != trace.id);
    for (k, case) in cases.iter_mut().enumerate() {
        let baro = paths.private.join("recales").join(format!("{}.baro.csv", case.id));
        let segment = several.then_some(k + 1);
        case.raw = load_raw(&raw_file, &baro, segment)
            .map(|gps| RawRef { gps: std::sync::Arc::new(gps), shift: 0.0 });
    }
    Ok(cases)
}

fn load_perso_geometry(
    paths: &BenchPaths,
    trace: &Trace,
    rules: &BenchRules,
) -> Result<Vec<PersoCase>, String> {
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
    matched: Vec<bool>,
    unmatched_warn: Vec<bool>,
    /// `ele_method` de chaque station.
    method: Vec<String>,
    /// Segments structure (`bridge` / `tunnel`), en abscisse de `d` (S1, README §4 ter.1).
    structure_segs: Vec<(f64, f64)>,
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
    let matched = ranges("segments", &|s| s["matched"].as_bool().unwrap_or(true));
    let warns: Vec<(f64, f64)> = r["quality"]["warnings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|w| w["code"] == "unmatched_range")
        .filter_map(|w| Some((w["from_m"].as_f64()?, w["to_m"].as_f64()?)))
        .collect();
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
    let mut p = Profile {
        structure_segs: segs.iter().filter(|s| s.2).map(|s| (s.0, s.1)).collect(),
        ..Profile::default()
    };
    for x in points {
        let d = x["d_m"].as_f64().context("d_m")?;
        p.d.push(d);
        p.pts.push(LatLon::new(x["lat"].as_f64().unwrap_or(f64::NAN), x["lon"].as_f64().unwrap_or(f64::NAN)));
        p.ele.push(x["ele"].as_f64().unwrap_or(f64::NAN));
        p.on_structure.push(at(&segs, d));
        p.corrected.push(matches!(x["ele_method"].as_str(), Some("dsm_deck" | "interpolated_structure")));
        p.method.push(x["ele_method"].as_str().unwrap_or("").to_owned());
        p.ign.push(at(&terrain, d));
        p.matched.push(at(&matched, d));
        p.unmatched_warn.push(warns.iter().any(|w| d >= w.0 - 1e-6 && d <= w.1 + 1e-6));
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
        p.matched.extend(q.matched.iter().skip(skip));
        p.unmatched_warn.extend(q.unmatched_warn.iter().skip(skip));
        p.method.extend(q.method.iter().skip(skip).cloned());
        p.structure_segs.extend(q.structure_segs.iter().map(|&(a, b)| (a + offset, b + offset)));
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

/// Marge de l'extrait d'une trace tenue à l'écart autour de la fenêtre évaluée (effets de bord).
const HELD_OUT_MARGIN_M: f64 = 1_000.0;

/// Extraits d'un cas d'une trace tenue à l'écart autour des passages de ses ouvrages de
/// calibration (`passages_masques`) : la fenêtre de ± 500 m autour de l'ouvrage franchi appartient
/// à la série de l'ouvrage, et chaque ouvrage garde tous ses passages baro (README §2). Chaque
/// extrait couvre la fenêtre ± 1 km ; seule la fenêtre est évaluée. Renvoie l'extrait et la
/// fenêtre évaluée, en abscisse de l'extrait.
pub fn held_out_extracts(
    case: &PersoCase,
    refs: &BTreeMap<String, Vec<LatLon>>,
    rules: &BenchRules,
) -> Vec<(PersoCase, std::ops::Range<f64>)> {
    let pts: Vec<Pt> = case
        .parts
        .iter()
        .enumerate()
        .flat_map(|(k, part)| part.iter().skip(usize::from(k > 0)).cloned())
        .collect();
    if pts.len() != case.s_ref.len() {
        return Vec::new();
    }
    let ll: Vec<LatLon> = pts.iter().map(|q| LatLon::new(q.lat, q.lon)).collect();
    let mut out = Vec::new();
    for (k, w) in held_out_windows(case, refs, rules).into_iter().enumerate() {
        let (lo, hi) = (w.start - HELD_OUT_MARGIN_M, w.end + HELD_OUT_MARGIN_M);
        let idx: Vec<usize> =
            (0..pts.len()).filter(|&i| case.s_ref[i] >= lo && case.s_ref[i] <= hi).collect();
        let (Some(&first), Some(&last)) = (idx.first(), idx.last()) else { continue };
        if last <= first {
            continue;
        }
        let s0 = case.s_ref[first];
        let keep: Vec<usize> =
            (0..case.baro_s.len()).filter(|&i| (lo..=hi).contains(&case.baro_s[i])).collect();
        let baro_s = keep.iter().map(|&i| case.baro_s[i] - s0).collect();
        let baro_z = keep.iter().map(|&i| case.baro_z[i]).collect();
        let baro_inserted = keep.iter().map(|&i| case.baro_inserted[i]).collect();
        let extract = PersoCase {
            id: format!("{}#extrait{k}", case.id),
            groupe: case.groupe.clone(),
            serie: case.serie,
            parts: vec![pts[first..=last].to_vec()],
            s_ref: case.s_ref[first..=last].iter().map(|x| x - s0).collect(),
            baro_s,
            baro_z,
            baro_inserted,
            // Les ouvrages de calibration de l'extrait sont mesurés, pas masqués.
            masks: Vec::new(),
            ring: ring_points(case, &ll, (w.start - HELD_OUT_RING_M)..(w.end + HELD_OUT_RING_M), s0, rules),
            raw: case.raw.as_ref().map(|r| RawRef { gps: r.gps.clone(), shift: r.shift + s0 }),
        };
        out.push((extract, (w.start - s0)..(w.end - s0)));
    }
    out
}

/// Fenêtres évaluées d'un cas d'une trace tenue à l'écart (abscisses du cas) : ± 500 m autour de
/// chaque passage d'un ouvrage de calibration (`passages_masques`), fusionnées.
pub fn held_out_windows(
    case: &PersoCase,
    refs: &BTreeMap<String, Vec<LatLon>>,
    rules: &BenchRules,
) -> Vec<std::ops::Range<f64>> {
    let ll: Vec<LatLon> = case
        .parts
        .iter()
        .enumerate()
        .flat_map(|(k, part)| part.iter().skip(usize::from(k > 0)).map(|q| LatLon::new(q.lat, q.lon)))
        .collect();
    if ll.len() != case.s_ref.len() {
        return Vec::new();
    }
    let mut windows: Vec<std::ops::Range<f64>> = case
        .masks
        .iter()
        .filter_map(|id| refs.get(id))
        .flat_map(|r| passage_windows(&case.s_ref, &ll, r, rules.passage_tol_m, rules.mask_half_m))
        .collect();
    windows.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut merged: Vec<std::ops::Range<f64>> = Vec::new();
    for w in windows {
        match merged.last_mut() {
            Some(m) if w.start <= m.end => m.end = m.end.max(w.end),
            _ => merged.push(w),
        }
    }
    merged
}

/// Plages d'un cas tenu à l'écart où son baro est lu : fenêtres ± 3 km (repli du décalage
/// local), plus l'écart d'interpolation maximal. La règle des points insérés n'y regarde que là.
pub fn held_out_baro_ranges(
    case: &PersoCase,
    refs: &BTreeMap<String, Vec<LatLon>>,
    rules: &BenchRules,
) -> Vec<std::ops::Range<f64>> {
    let reach = HELD_OUT_RING_M + rules.baro_gap_max_m;
    held_out_windows(case, refs, rules).into_iter().map(|w| (w.start - reach)..(w.end + reach)).collect()
}

/// Points de la trace dans `range` (abscisse de la trace), en abscisse de l'extrait (`s0`), avec
/// le baro transporté (`NaN` au-delà d'un trou de baro).
fn ring_points(
    case: &PersoCase,
    ll: &[LatLon],
    range: std::ops::Range<f64>,
    s0: f64,
    rules: &BenchRules,
) -> Vec<RingPoint> {
    let idx: Vec<usize> = (0..ll.len()).filter(|&i| range.contains(&case.s_ref[i])).collect();
    let s: Vec<f64> = idx.iter().map(|&i| case.s_ref[i]).collect();
    let baro = interpolate(&case.baro_s, &case.baro_z, &s, rules.baro_gap_max_m);
    idx.iter().zip(s).zip(baro).map(|((&i, s), baro)| RingPoint { s: s - s0, at: ll[i], baro }).collect()
}

/// Repli 1–3 km d'un extrait : `(abscisse de l'extrait, baro − MNT LiDAR)` des points de la
/// trace hors pont et tunnel des tuiles (MNT IGN lu directement, jamais la sortie du service).
async fn ring_reference(
    service: &Service,
    cache: &Path,
    ring: &[RingPoint],
) -> anyhow::Result<Vec<(f64, f64)>> {
    let pts: Vec<LatLon> = ring.iter().map(|r| r.at).collect();
    let ctx = ord_domain::ctx::RequestCtx::new(
        ord_domain::budget::RequestId("bench".into()),
        ord_domain::budget::Deadline::after(std::time::Duration::from_secs(600)),
    );
    let dtm = service.state().orchestrator().sources().terrain.sample_ign_dtm(&pts, &ctx).await;
    let tiles = crate::ouvrages::Tiles::load(cache, &pts).await?;
    let on_structure = tiles.structure_mask(&pts);
    Ok(ring
        .iter()
        .zip(dtm)
        .zip(on_structure)
        .filter(|(_, on)| !on)
        .filter_map(|((r, z), _)| z.map(|z| (r.s, r.baro - f64::from(z))))
        .filter(|(_, v)| v.is_finite())
        .collect())
}

/// Mesures d'ouvrage d'une trace tenue à l'écart, sur ses extraits : seuls les passages de ses
/// ouvrages de calibration dans la fenêtre évaluée, sans position (distances retirées) ; aucune
/// métrique de trace ni sortie du service hors fenêtre.
#[allow(clippy::too_many_arguments)]
pub async fn measure_held_out(
    service: &Service,
    trace: &Trace,
    case: &PersoCase,
    refs: &BTreeMap<String, Vec<LatLon>>,
    ouvrages: &[crate::ouvrages::Ouvrage],
    rules: &BenchRules,
    rules_core: &Rules,
    cache: &Path,
) -> anyhow::Result<Vec<Value>> {
    let mut items = Vec::new();
    for (extract, window) in held_out_extracts(case, refs, rules) {
        let ring = ring_reference(service, cache, &extract.ring).await?;
        let eval = OuvrageEval { cache, window: Some(window), ring: Some(ring) };
        let (_trace_metrics, _, its) =
            measure_case(service, &extract, None, refs, ouvrages, rules, rules_core, &eval).await?;
        items.extend(held_out_items(its, &trace.id, &trace.passages_masques));
    }
    Ok(items)
}

/// Items d'un extrait tenu à l'écart : ouvrages de `passages_masques` seulement, cas = la trace,
/// distances curvilignes de l'extrait retirées.
pub fn held_out_items(items: Vec<Value>, trace: &str, calibration: &[String]) -> Vec<Value> {
    items
        .into_iter()
        .filter(|i| i["ouvrage"].as_str().is_some_and(|o| calibration.iter().any(|c| c == o)))
        .map(|mut i| {
            if let Some(m) = i.as_object_mut() {
                for k in ["d_milieu_m", "ecart_max_d_m", "emprise_d_m", "_emprise_d_m"] {
                    m.remove(k);
                }
                m.insert("cas".into(), json!(trace));
                m.insert("extrait_tenu_a_l_ecart".into(), json!(true));
            }
            i
        })
        .collect()
}

/// Contexte des mesures d'ouvrage d'un cas : cache de banc (tuiles de rues de la campagne) et,
/// pour un extrait de trace tenue à l'écart, la fenêtre évaluée (abscisses de l'extrait).
#[derive(Clone, Debug)]
pub struct OuvrageEval<'a> {
    pub cache: &'a Path,
    pub window: Option<std::ops::Range<f64>>,
    /// Extrait : repli 1–3 km lu sur la trace (`(abscisse, baro − MNT)`), hors service.
    pub ring: Option<Vec<(f64, f64)>>,
}

/// Profils d'un cas sur la grille de la campagne (mode `distance`), pour un vidage local et privé
/// (`dump-profils`) : mêmes calculs que `measure_case` (baro transporté, fenêtres masquées,
/// décalage global d'E1). Aucune coordonnée n'en sort.
pub(crate) async fn case_dump(
    service: &Service,
    case: &PersoCase,
    refs: &BTreeMap<String, Vec<LatLon>>,
    ouvrages: &[crate::ouvrages::Ouvrage],
    rules: &BenchRules,
    rules_core: &Rules,
) -> anyhow::Result<crate::dump::CaseProfiles> {
    let dem = rules_core.elevation.gain.preset(GainPreset::Dem);
    let (p, _, _) = recolle(service, case, true, rules, dem.threshold_m).await?;
    let (p_raw, _, _) = recolle(service, case, false, rules, dem.threshold_m).await?;
    if p_raw.d.len() != p.d.len() {
        bail!("profils de longueurs différentes");
    }
    let d_srv = request_distances(case);
    let baro_d = interpolate(&case.s_ref, &d_srv, &case.baro_s, f64::INFINITY);
    let (bd, bz): (Vec<f64>, Vec<f64>) =
        baro_d.iter().zip(&case.baro_z).filter(|(d, _)| !d.is_nan()).map(|(d, z)| (*d, *z)).unzip();
    let baro = interpolate(&bd, &bz, &p.d, rules.baro_gap_max_m);
    let mut windows = Vec::new();
    for id in &case.masks {
        if let Some(pts) = refs.get(id) {
            windows.extend(passage_windows(&p.d, &p.pts, pts, rules.passage_tol_m, rules.mask_half_m));
        }
    }
    let n = p.d.len();
    let masked: Vec<bool> = p.d.iter().map(|d| windows.iter().any(|w| w.contains(d))).collect();
    // Référence E2 : baro sur les stations non masquées où le service a une altitude.
    let bar: Vec<f64> = (0..n)
        .map(|i| if masked[i] || p.ele[i].is_nan() || baro[i].is_nan() { f64::NAN } else { baro[i] })
        .collect();
    let svc: Vec<f64> = (0..n).map(|i| if bar[i].is_nan() { f64::NAN } else { p.ele[i] }).collect();
    let off = |v: &[f64]| -> Vec<f64> {
        (0..n).map(|i| if p.on_structure[i] || p.corrected[i] { f64::NAN } else { v[i] }).collect()
    };
    let offset = e1(&p.d, &off(&svc), &off(&bar), rules.detrend_window_m, rules.detrend_step_m).offset_m;
    // Ouvrage du corpus de chaque suite de stations corrigées : milieu à moins de la
    // demi-longueur + tolérance de passage d'une station de la suite.
    let mut ouvrage = vec![String::new(); n];
    let mut i = 0;
    while i < n {
        if !p.corrected[i] {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && p.corrected[i] {
            i += 1;
        }
        let best = ouvrages
            .iter()
            .filter_map(|o| {
                let dist = (start..i).map(|k| haversine_m(p.pts[k], o.pts[1])).fold(f64::INFINITY, f64::min);
                (dist <= 0.5 * o.length_m + rules.passage_tol_m).then_some((dist, &o.id))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let id = best.map_or_else(|| "hors_corpus".to_owned(), |(_, id)| id.clone());
        ouvrage[start..i].fill(id);
    }
    Ok(crate::dump::CaseProfiles {
        id: case.id.clone(),
        s: p.d,
        baro_e2: bar,
        offset_m: offset,
        ele: p.ele,
        ele_raw: p_raw.ele,
        ouvrage,
        method: p.method,
        masked,
    })
}

/// Mesure E1 et E2 d'un cas (service appelé deux fois : corrections actives, puis MNT brut).
#[allow(clippy::too_many_arguments)]
pub async fn measure_case(
    service: &Service,
    case: &PersoCase,
    surface_csv: Option<String>,
    refs: &BTreeMap<String, Vec<LatLon>>,
    ouvrages: &[crate::ouvrages::Ouvrage],
    rules: &BenchRules,
    rules_core: &Rules,
    eval: &OuvrageEval<'_>,
) -> anyhow::Result<(Value, Value, Vec<Value>)> {
    let dem = rules_core.elevation.gain.preset(GainPreset::Dem);
    let (mut p, tau, responses) = recolle(service, case, true, rules, dem.threshold_m).await?;
    let (p_raw, tau_raw, _) = recolle(service, case, false, rules, dem.threshold_m).await?;
    if p_raw.d.len() != p.d.len() {
        bail!("profils de longueurs différentes");
    }
    // Références des ouvrages (MNT des approches, du niveau, d'E5, des portails ; repli 1–3 km)
    // et positions des passages : toujours lues en mode `distance`, quel que soit le mode testé.
    // E2b garde la sortie non corrigée du mode testé (`p_raw`).
    let (ref_svc, ref_raw) = if service.mode() == BenchMode::Reference {
        (p.ele.clone(), p_raw.ele.clone())
    } else {
        let reference = service.with_mode(BenchMode::Reference);
        let (q, _, _) = recolle(&reference, case, true, rules, dem.threshold_m).await?;
        let (q_raw, _, _) = recolle(&reference, case, false, rules, dem.threshold_m).await?;
        // Même grille, aux arrondis du cm près ; la dernière station du mode `distance` peut être
        // doublée (fin du parcours à moins d'un cm du dernier pas) : profils de référence lus
        // aux abscisses de la grille testée.
        if (q.d.len() as i64 - p.d.len() as i64).abs() > 1
            || (q_raw.d.len() as i64 - p.d.len() as i64).abs() > 1
        {
            bail!("grilles de référence et testée différentes ({} / {} stations)", q.d.len(), p.d.len());
        }
        // Station à moins d'un cm : sa valeur telle quelle (jamais `0 × NaN` d'un voisin absent).
        let on = |xs: &[f64], ys: &[f64]| -> Vec<f64> {
            p.d.iter()
                .map(|&x| {
                    let i = xs.partition_point(|&v| v < x - 0.01);
                    match xs.get(i) {
                        Some(&v) if (v - x).abs() <= 0.01 => ys[i],
                        _ => interp_one(xs, ys, x),
                    }
                })
                .collect()
        };
        let lat: Vec<f64> = q.pts.iter().map(|l| l.lat).collect();
        let lon: Vec<f64> = q.pts.iter().map(|l| l.lon).collect();
        let (lat, lon) = (on(&q.d, &lat), on(&q.d, &lon));
        let (svc_ref, raw_ref) = (on(&q.d, &q.ele), on(&q_raw.d, &q_raw.ele));
        p.pts = lat.iter().zip(&lon).map(|(&a, &b)| LatLon::new(a, b)).collect();
        (svc_ref, raw_ref)
    };
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
    // E2b (criteria.yaml `preset_baro: dem`, README §4 bis.5) : D+ baro au preset `dem`, le même
    // traitement que le service et sa référence sans correction ; E2 reste en `barometric`.
    let dplus_baro_dem = dplus(&p.d, &bar, dem);
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
    // M1 hors réseau (README §4 bis.6) : plages de la trace recalée sans voie `streets` à 30 m,
    // lues sur la trace et les tuiles seulement ; hors de M1, rapportées à part. Un extrait de
    // trace tenue à l'écart n'a pas de métrique de trace : rien n'y est lu.
    let off_network = if eval.window.is_none() {
        match crate::off_network::ranges(case, eval.cache, rules.grid_step_m).await? {
            Ok(r) => r,
            Err(reason) => bail!(reason),
        }
    } else {
        Vec::new()
    };
    // M1 hors graphe vélo (README §4 ter.3) : stations dont la voie la plus proche (≤ 30 m) est
    // fermée au vélo par la règle de tags ; hors de M1, rapportées à part, avec leur contrôle
    // (matched = false et unmatched_range attendus à chaque station).
    let (closed, closed_at) = if eval.window.is_none() {
        match crate::off_network::closed(case, eval.cache, rules.grid_step_m).await? {
            Ok(r) => r,
            Err(reason) => bail!(reason),
        }
    } else {
        (Vec::new(), Vec::new())
    };
    let warn_ranges: Vec<(f64, f64)> = responses
        .iter()
        .zip(&offsets)
        .flat_map(|(x, off)| {
            x["quality"]["warnings"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|w| w["code"] == "unmatched_range")
                .filter_map(|w| {
                    Some((to_ref(w["from_m"].as_f64()? + off), to_ref(w["to_m"].as_f64()? + off)))
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let closed_matched = closed_at
        .iter()
        .filter(|&&st| ours.iter().find(|o| o.from <= st && st < o.to).is_some_and(|o| o.matched))
        .count();
    let closed_unwarned = closed_at
        .iter()
        .filter(|&&st| !warn_ranges.iter().any(|w| w.0 - 1e-6 <= st && st <= w.1 + 1e-6))
        .count();
    let ours_m1 = crate::typology::without(&ours, &crate::off_network::union(&off_network, &closed));
    let m1_agr =
        agreement(&ours_m1, &[Ref { from: f64::NEG_INFINITY, to: f64::INFINITY, class: Class::Unknown }]);
    let mut typo = if stadia.is_empty() {
        json!({"R3": "reference_absente"})
    } else {
        metrics_json(&agreement(&ours, &stadia), "R3")
    };
    typo["M1.non_apparie"] = json!(round(m1_agr.unmatched, 4));
    typo["M1.unknown"] = json!(round(m1_agr.unknown, 4));
    typo["M1.hors_reseau_m"] = json!(round(crate::off_network::length(&off_network), 1));
    typo["M1.hors_graphe_velo_m"] = json!(round(crate::off_network::length(&closed), 1));
    typo["M1.hors_graphe_velo_controle"] = json!({
        "stations": closed_at.len(),
        "appariees": closed_matched,
        "sans_unmatched_range": closed_unwarned,
        "ok": closed_matched == 0 && closed_unwarned == 0,
    });
    let to_d = |s: f64| interp_one(&case.s_ref, &d_srv, s);
    let mut off_diag: Vec<&std::ops::Range<f64>> = off_network.iter().collect();
    off_diag.sort_by(|a, b| (b.end - b.start).total_cmp(&(a.end - a.start)));
    let typo_diag = json!({
        "M1.plages_non_appariees": unmatched_ranges(&ours_m1, 10, &to_d),
        "M1.plages_hors_reseau": off_diag.iter().take(10).map(|r| {
            let (a, b) = (to_d(r.start), to_d(r.end));
            json!({"d_debut_m": round(a, 1), "d_fin_m": round(b, 1), "longueur_plage_m": round(b - a, 1)})
        }).collect::<Vec<_>>(),
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
            "E2.ratio_mnt_brut": round(dplus_raw / dplus_baro, 4),
            "E2b.dplus_baro_dem_m": round(dplus_baro_dem, 1),
            "E2b.ratio": round(dplus_svc / dplus_baro_dem, 4),
            "E2b.ratio_mnt_brut": round(dplus_raw / dplus_baro_dem, 4)
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
    // Extrait de trace tenue à l'écart : rien n'est lu hors de la fenêtre évaluée (service,
    // MNT, baro), le reste de l'extrait ne sert qu'à éviter les effets de bord.
    let inside = |i: usize| eval.window.as_ref().is_none_or(|w| w.contains(&p.d[i]));
    let keep = |v: &[f64]| -> Vec<f64> { (0..n).map(|i| if inside(i) { v[i] } else { f64::NAN }).collect() };
    let baro_unmasked: Vec<f64> =
        (0..n).map(|i| if masked[i] || !inside(i) { f64::NAN } else { baro[i] }).collect();
    let (svc_eval, raw_eval, ref_svc_eval) = (keep(&p.ele), keep(&ref_raw), keep(&ref_svc));
    // Ponts et tunnels des tuiles de la campagne (§4 bis.2, §4 bis.3) : tuiles des abords des
    // ouvrages du corpus seulement.
    let near_ouvrage =
        |q: &LatLon| ouvrages.iter().any(|o| haversine_m(*q, o.pts[1]) <= o.length_m + 2_500.0);
    let mut near: Vec<LatLon> = p.pts.iter().copied().filter(near_ouvrage).collect();
    // Trace brute sur l'étendue du cas (attribution des passages, README §4 ter.2).
    let raw_span: Vec<LatLon> = match (&case.raw, case.s_ref.first(), case.s_ref.last()) {
        (Some(r), Some(&a), Some(&b)) => r.between(a, b),
        _ => Vec::new(),
    };
    near.extend(raw_span.iter().copied().filter(near_ouvrage));
    let tiles = crate::ouvrages::Tiles::load(eval.cache, &near).await?;
    let raw_gps = |da: f64, db: f64| -> Vec<LatLon> {
        case.raw.as_ref().map_or_else(Vec::new, |r| r.between(to_ref(da), to_ref(db)))
    };
    let tile_structure = tiles.structure_mask(&p.pts);
    let cp = crate::ouvrages::CaseProfile {
        d: &p.d,
        pts: &p.pts,
        svc: &svc_eval,
        raw: &raw_eval,
        ref_svc: &ref_svc_eval,
        baro: &baro_unmasked,
        on_structure: &p.on_structure,
        ign: &p.ign,
        matched: &p.matched,
        unmatched_warn: &p.unmatched_warn,
        tile_structure: &tile_structure,
        ring: eval.ring.as_deref(),
        structure_segs: &p.structure_segs,
        raw_gps: Some(&raw_gps),
        tiles: Some(&tiles),
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
            // E2b : médiane |ln(service / baro)| ≤ médiane |ln(MNT brut / baro)|, D+ baro au preset
            // `dem` (`E2b.ratio*`, criteria.yaml `preset_baro: dem`).
            let ln_of = |k: &str| -> Vec<f64> { list.iter().map(|c| metric(c, k).ln().abs()).collect() };
            let (ln_svc, ln_raw) = (ln_of("E2b.ratio"), ln_of("E2b.ratio_mnt_brut"));
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
        // M1 hors graphe vélo (README §4 ter.3) : une station appariée (ou sans unmatched_range)
        // sur une voie fermée au vélo est un échec de M1.
        let closed_ok = c["typologie"]["M1.hors_graphe_velo_controle"]["ok"] != false;
        if !(m(c, "M1.non_apparie") <= M1_UNMATCHED_MAX && m(c, "M1.unknown") <= M1_UNKNOWN_MAX && closed_ok)
        {
            fails_m1.push(c["id"].clone());
        }
    }
    let mut agg = serde_json::Map::new();
    // M1 hors réseau (README §4 bis.6) : longueur par trace, rapportée à part.
    let off: serde_json::Map<String, Value> = perso
        .iter()
        .filter_map(|c| Some((c["id"].as_str()?.to_owned(), c["typologie"]["M1.hors_reseau_m"].as_f64()?)))
        .map(|(id, m)| (id, json!(m)))
        .collect();
    agg.insert("M1.hors_reseau_m".into(), Value::Object(off));
    // M1 hors graphe vélo (README §4 ter.3) : longueur et contrôle par trace, rapportés à part.
    let closed: serde_json::Map<String, Value> = perso
        .iter()
        .filter_map(|c| {
            let t = &c["typologie"];
            Some((
                c["id"].as_str()?.to_owned(),
                json!({"longueur_m": t["M1.hors_graphe_velo_m"].as_f64()?, "controle": t["M1.hors_graphe_velo_controle"]}),
            ))
        })
        .collect();
    agg.insert("M1.hors_graphe_velo".into(), Value::Object(closed));
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

/// Mesures d'une passe de la campagne, dans un mode de lecture du service.
#[derive(Debug, Default)]
struct Pass {
    cases: Vec<Value>,
    items: Vec<Value>,
    skipped: Vec<Value>,
    data_versions: Value,
    inserted_report: Vec<Value>,
    held_out_measured: Vec<Value>,
    os_cases: Vec<Value>,
    warm: Vec<Value>,
}

/// Contexte commun aux passes.
struct PassCtx<'a> {
    paths: &'a BenchPaths,
    manifest: &'a Manifest,
    selection: &'a Selection<'a>,
    refs: &'a BTreeMap<String, Vec<LatLon>>,
    ouvrages: &'a [crate::ouvrages::Ouvrage],
    tiles: &'a crate::ouvrages::Tiles,
    groupes: &'a [String],
    ids: &'a [String],
    /// Série mesurée : calibration (`run`) ou tenue à l'écart (`judge`).
    target: Serie,
}

/// Bilan des points insérés d'un cas (sans position) : points par km et densité maximale.
fn inserted_stats(stats: &[crate::service::InsertedStats]) -> Value {
    let length: f64 = stats.iter().map(|s| s.length_m).sum();
    let inserted: usize = stats.iter().map(|s| s.inserted).sum();
    json!({
        "requetes": stats.len(),
        "longueur_km": round(length / 1_000.0, 3),
        "points_entree": stats.iter().map(|s| s.input).sum::<usize>(),
        "points_inseres": inserted,
        "inseres_par_km": round(inserted as f64 / (length / 1_000.0).max(1e-9), 3),
        "max_sur_un_km": stats.iter().map(|s| s.max_per_km).max().unwrap_or(0),
    })
}

/// Une passe : traces perso, extraits tenus à l'écart, parcours d'osm-surface, ouvrages des
/// parcours vcyclist et témoins, lus dans le mode du service.
async fn measure_pass(service: &Service, ctx: &PassCtx<'_>) -> anyhow::Result<Pass> {
    let PassCtx { paths, manifest, selection, refs, ouvrages, tiles, groupes, ids, target } = *ctx;
    let rules = BenchRules::default();
    let rules_core = Rules::default();
    let structures_wanted = ids.is_empty();
    let mut items: Vec<Value> = Vec::new();
    let mut cases = Vec::new();
    let mut skipped = Vec::new();
    let mut data_versions = Value::Null;
    let mut inserted_report: Vec<Value> = Vec::new();
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
        for mut case in loaded {
            service.take_stats();
            // Baro des points insérés (README §2.1, étape 3) : tuiles de la campagne.
            match crate::inserted::apply(&mut case, &paths.cache, None, &rules).await? {
                Ok(o) => {
                    inserted_report.push(json!({"id": case.id, "inseres": o.inserted, "retires": o.removed}))
                }
                Err(reason) => {
                    skipped.push(json!({"id": case.id, "raison": reason}));
                    continue;
                }
            }
            let csv = std::fs::read_to_string(
                paths.private.join("recales").join(format!("{}.surface.csv", case.id)),
            )
            .ok();
            let eval = OuvrageEval { cache: &paths.cache, window: None, ring: None };
            match measure_case(service, &case, csv, refs, ouvrages, &rules, &rules_core, &eval).await {
                Ok((mut c, dv, its)) => {
                    eprintln!("{} : mesuré ({})", case.id, service.mode().as_str());
                    c["mode"] = json!(service.mode().as_str());
                    if service.mode() == BenchMode::Breakpoints {
                        c["points_inseres"] = inserted_stats(&service.take_stats());
                    }
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
    // Traces de l'autre série (tenues à l'écart en calibration, de calibration au jugement) :
    // passages des ouvrages de la série mesurée seulement (`passages_masques`), sur des extraits
    // (README §2 ; aucune métrique de trace, rien hors des fenêtres de ± 500 m).
    let mut held_out_measured = Vec::new();
    for t in manifest.traces.iter().filter(|t| {
        selection.excluded_held_out.contains(&t.id)
            && !t.passages_masques.is_empty()
            && paths.private_present()
    }) {
        let Ok(loaded) = load_perso(paths, t, &rules) else {
            skipped.push(json!({"id": t.id, "raison": "extraits tenus à l'écart : trace illisible"}));
            continue;
        };
        for mut case in loaded {
            // Points insérés : seulement là où le baro de l'extrait est lu ; bilan non rapporté.
            let ranges = held_out_baro_ranges(&case, refs, &rules);
            if let Err(reason) =
                crate::inserted::apply(&mut case, &paths.cache, Some(&ranges), &rules).await?
            {
                skipped
                    .push(json!({"id": case.id, "raison": format!("extraits tenus à l'écart : {reason}")}));
                continue;
            }
            let measured =
                measure_held_out(service, t, &case, refs, ouvrages, &rules, &rules_core, &paths.cache).await;
            // Aucune sortie du service hors des fenêtres : bilans des extraits jamais rapportés.
            service.take_stats();
            match measured {
                Ok(its) => {
                    eprintln!("{} : extraits mesurés ({} mesures d'ouvrage)", case.id, its.len());
                    held_out_measured.push(json!({"id": case.id, "mesures": its.len()}));
                    items.extend(its);
                }
                Err(_) => skipped
                    .push(json!({"id": case.id, "raison": "extraits tenus à l'écart : échec du service"})),
            }
        }
    }
    // Parcours d'osm-surface : R1, R2, M1 et typologie à chaud.
    let os_wanted = |id: &str| {
        (ids.is_empty() && groupes.iter().any(|g| g == "osm_surface")) || ids.iter().any(|i| i == id)
    };
    let mut os_cases = Vec::new();
    let mut warm = Vec::new();
    for entry in manifest.osm_surface.iter().filter(|e| os_wanted(&e.id) && e.serie == target) {
        let route = crate::osm_surface::load(&paths.bench, entry)?;
        match crate::osm_surface::measure(service, &route).await {
            Ok(c) => {
                eprintln!("{} : mesuré", route.id);
                os_cases.push(c);
                if service.mode() == BenchMode::Reference {
                    let (ms, stats) = crate::osm_surface::typology_warm_ms(&paths.cache, &route, 5).await?;
                    warm.push(json!({"id": route.id, "typologie_a_chaud_ms": round(ms, 1), "stats": stats}));
                }
            }
            Err(e) => skipped.push(json!({"id": route.id, "raison": e.to_string()})),
        }
    }
    // Ouvrages : parcours `test1..4` de vcyclist et témoins (toujours calibration : « déjà vu »).
    if structures_wanted && target == Serie::Calibration {
        items.extend(crate::ouvrages::vc_items(service, &paths.bench, tiles, ouvrages, &mut skipped).await?);
    }
    items.extend(crate::ouvrages::perimeter_items(ouvrages));
    Ok(Pass { cases, items, skipped, data_versions, inserted_report, held_out_measured, os_cases, warm })
}

/// Verdicts d'une passe : `(cas, ouvrages publics, agrégats, verdicts)`.
fn judge_pass(pass: Pass, limits: &[(String, String)]) -> (Vec<Value>, Vec<Value>, Value, Value, Pass) {
    let Pass {
        mut cases,
        mut items,
        skipped,
        data_versions,
        inserted_report,
        held_out_measured,
        os_cases,
        warm,
    } = pass;
    crate::ouvrages::annotate_data_limits(&mut items, limits);
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
    let public = crate::ouvrages::public(&items);
    let rest = Pass { skipped, data_versions, inserted_report, held_out_measured, ..Pass::default() };
    (cases, public, agregats, verdicts, rest)
}

/// `run --offline` : campagne de calibration ; renvoie le contenu de `results.json`.
pub async fn run(paths: &BenchPaths, groupes: &[String], ids: &[String]) -> anyhow::Result<Value> {
    run_series(paths, groupes, ids, Serie::Calibration).await
}

/// Campagne sur la série `target` : calibration (`run`) ou tenue à l'écart (`judge`, seul appelant
/// de cette série). L'autre série est exclue avant toute lecture ; ses traces ne servent qu'aux
/// extraits de ± 500 m autour des ouvrages de la série mesurée.
pub async fn run_series(
    paths: &BenchPaths,
    groupes: &[String],
    ids: &[String],
    target: Serie,
) -> anyhow::Result<Value> {
    let report = check(paths)?;
    ensure_check(&report)?;
    let manifest = Manifest::load(&paths.bench)?;
    let selection = select_series(&manifest, groupes, ids, target);
    eprintln!(
        "cas retenus : {} ; exclus (autre série) avant toute lecture : {}",
        selection.cases.len(),
        selection.excluded_held_out.len()
    );
    let rules = BenchRules::default();
    let rules_core = Rules::default();
    let refs = ouvrage_refs(paths)?;
    let mut ouvrages = crate::ouvrages::series_ouvrages(paths, &manifest.ouvrages_tenus, target)?;
    // Tuiles de rues du cache : périmètre vélo des ouvrages du corpus (§4 bis.1) et spans vcyclist.
    let structures_wanted = ids.is_empty() && target == Serie::Calibration;
    let geoms = crate::ouvrages::way_geometries(&paths.bench);
    let mut tile_points: Vec<LatLon> =
        ouvrages.iter().flat_map(|o| crate::ouvrages::perimeter_stations(o, &geoms)).collect();
    if structures_wanted {
        tile_points.extend(crate::ouvrages::vc_points(&paths.bench)?);
    }
    let tiles = crate::ouvrages::Tiles::load(&paths.cache, &tile_points).await?;
    crate::ouvrages::apply_perimeter(&mut ouvrages, &tiles, &geoms);
    let limits = crate::ouvrages::corpus_data_limits(&ouvrages, &crate::ouvrages::data_limits(&paths.bench)?);
    let service = Service::start(&paths.cache, true, Variant::DEFAULT).await?;
    let ctx = PassCtx {
        paths,
        manifest: &manifest,
        selection: &selection,
        refs: &refs,
        ouvrages: &ouvrages,
        tiles: &tiles,
        groupes,
        ids,
        target,
    };
    // Deux passes (README §2.2, étape 2) : `distance` au pas de la grille, puis le mode par défaut
    // `input_with_breakpoints`, rééchantillonné sur la même grille (étape 3).
    let reference = measure_pass(&service, &ctx).await?;
    let breakpoints = measure_pass(&service.with_mode(BenchMode::Breakpoints), &ctx).await?;
    let (cases, ouvrages_public, agregats, mut verdicts, rest) = judge_pass(reference, &limits);
    let (bp_cases, bp_ouvrages, bp_agregats, mut bp_verdicts, bp_rest) = judge_pass(breakpoints, &limits);
    let Pass { skipped, data_versions, inserted_report, held_out_measured, .. } = rest;
    let bp_inserted: Vec<&Value> = bp_cases.iter().filter_map(|c| c.get("points_inseres")).collect();
    let max_per_km = bp_inserted.iter().filter_map(|v| v["max_sur_un_km"].as_u64()).max().unwrap_or(0);
    let (bp_len, bp_ins) = bp_inserted.iter().fold((0.0, 0u64), |(l, n), v| {
        (l + v["longueur_km"].as_f64().unwrap_or(0.0), n + v["points_inseres"].as_u64().unwrap_or(0))
    });
    verdicts["Q1"] = json!(if report.q1.pass() { "pass" } else { "fail" });
    bp_verdicts["Q1"] = verdicts["Q1"].clone();
    let fingerprint = cache_fingerprint(&paths.cache).context("empreinte du cache de banc")?;
    let rules_fingerprint = sha256_bytes(serde_json::to_string(&rules_core)?.as_bytes());
    let bench_rules_sha256 = sha256_bytes(serde_json::to_string(&rules)?.as_bytes());
    // Traces de l'autre série : exclues (et lues en extraits autour des ouvrages mesurés).
    let (exclus_key, extraits_key) = match target {
        Serie::Calibration => ("exclus_serie_tenue_a_l_ecart", "extraits_tenus_a_l_ecart"),
        Serie::TenueALEcart => ("exclus_serie_calibration", "extraits_calibration"),
    };
    let results = json!({
        "schema": 1,
        "campagne": {
            "date": today_utc(),
            "type": if target == Serie::Calibration { "calibration" } else { "jugement" },
            "serie": target,
            "increment": "V1",
            "increment_plan": "V1",
            "modes": ["distance", "input_with_breakpoints"],
            "regles_banc": "README §4 bis (décision de l'utilisateur, 2026-10-31 ; complétée en 40a989f, 657d2c8, 022710f, 4304838, e48170f, adbf6f1, 53225a8 et 405f956 ; décision de l'utilisateur, 2026-11-01 : 2f3254b, 02874b5 et 01272dc) ; README §4 ter, règles V1 (décision de l'utilisateur, 2026-11-01 : 304f767, 16f6097 et c1d231c ; 6c664b6 non appliqué en calibration)",
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
            exclus_key: selection.excluded_held_out,
            extraits_key: held_out_measured,
            "baro_points_inseres": inserted_report,
            "notes": [
                "Deux modes (README §2.2, étape 2) : cas, ouvrages, agrégats et verdicts de premier niveau en sampling.mode = distance au pas de 5 m (les mesures point par point des témoins et des parcours vcyclist en mode input) ; clé input_with_breakpoints : mêmes requêtes envoyées dans le mode par défaut, sortie rééchantillonnée sur la même grille de 5 m (étape 3 ; altitude interpolée linéairement entre points consécutifs, null si l'un est sans altitude ; position des points d'entrée = position envoyée ; méthode de la station = none si l'un des deux points est none, sinon dsm_deck ou interpolated_structure si l'un des deux la porte, sinon celle du plus proche), et réduite aux points d'entrée pour les mesures point par point ; références des ouvrages (MNT des approches E3, test de niveau, E5, bandes de portail E4, repli 1–3 km du décalage local) et positions des passages toujours lues en mode distance, seule la sortie testée venant du mode input_with_breakpoints ; E2b de ce mode compare ses sorties corrigée et non corrigée ; points_inseres = bilan des points insérés par cas de calibration (sans position), jamais pour les extraits tenus à l'écart ; typologie à chaud mesurée dans la première passe seulement",
                "Baro des points insérés (README §2.1, étape 3 ; criteria.yaml preparation.baro_points_inseres) : point insere = 1 gardé seulement si sa position sur la géométrie recalée est à ≤ 30 m d'une ligne de la couche streets (toutes classes, tuile z14 et 8 voisines, cache de la campagne) ; tuile absente à la source = sans voie ; tuile absente du cache : cas sauté ; baro_points_inseres = bilan par cas de calibration ; traces tenues à l'écart : règle appliquée sur les seules plages lues des extraits, bilan non rapporté",
                "E1 : stations hors fenêtres masquées et hors ouvrage (segment pont ou tunnel, ou altitude dsm_deck / interpolated_structure, rampes comprises)",
                "E3 : baro corrigé localement sur les traces perso, décalage = médiane (baro − MNT LiDAR, profil du service sans correction d'ouvrage) sur les approches de 50 à 300 m de chaque côté du pont, hors autres ouvrages (service ou tuiles) et fenêtres masquées ; moins de 20 stations d'un côté : repli sur la médiane baro − service au sol entre 1 et 3 km du milieu (decalage_repli_1_3km) ; côtés différant de plus de 2 m : reference_incertaine, rapportée, non comptée (README §4 bis.3) ; médiane MNS 25 m de vcyclist (B, cohérence) sur test1..3 ; par pont, médiane des passages de l'écart au milieu et de l'écart max (README §4 bis.3)",
                "E4 : par tunnel OSM entre ses propres portails (tunnels d'ouvrages.yaml sur test4 ou un témoin, tunnels N des témoins, tunnels perso) ; altitude de portail = médiane du MNT brut entre 10 et 30 m hors portail ; bande invalide (station dans un autre ouvrage : tunnel, galerie, balcon S ; ou moins de 3 stations ; ou sur un tronçon à l'air libre de moins de 120 m entre deux tunnels OSM ; ou sur un pont ou un tunnel des tuiles de la campagne, station à moins de 10 m à l'intérieur de sa ligne) remplacée par la bande du portail extérieur suivant du même côté, à moins de 1 km, sinon référence indisponible, rapportée et non comptée ; bande_remplacee : par côté, distance curviligne au portail, bandes sautées, altitude retenue (distance retirée pour les ouvrages perso) ; droite passant par chaque portail retenu à sa place ; sensibilité 5/10/20/30 m, référence ambiguë si le verdict bascule ; pente implicite entre portails retenus (pente_implicite_portails) au-delà de 25 % : référence indisponible, rapportée, non comptée (README §4 bis.2) ; D+ fictif = D+ dem du service − D+ dem de la droite",
                "Périmètre vélo (README §4 bis.1) : tags kind/bicycle/bridge/tunnel, dans les tuiles de la campagne (data_versions, couche streets), de la voie de l'ouvrage = élément de même nature (pont, tunnel, voie au sol d'un passage dessous) de plus petite médiane de distance aux stations du corpus (tous les 5 m sur la géométrie Overpass, sinon A–milieu–B), accepté à ≤ 15 m, sinon toute voie avec le même critère ; deux candidats à moins de 3 m de médiane et de périmètres différents : perimetre_ambigu, rapporté ; jamais le corpus Overpass ; voie exclue du graphe vélo : sortie de S1/E3/E4/E5, rapportée en hors_graphe_velo ; pont ou tunnel sans voie : absent_des_donnees, rapporté en E3_non_couvert/E4_non_couvert, non compté ; négatifs à trace hors graphe gardés en S2/E5",
                "hors_graphe_velo : contrôle satisfait si la portion est non appariée avec unmatched_range (controle_par = non_apparie) ou appariée sur toute sa longueur à une voie du graphe vélo, nécessairement distincte de la voie de l'ouvrage exclue du graphe (controle_par = voie_parallele_du_graphe) ; ecart_max_m = altitude rendue contre la référence (baro corrigé local pour les traces perso, vérité T de vcyclist) sur les stations non appariées, ou sur toute la portion sur une voie parallèle ; null avec raison_ecart sans référence",
                "Limite des données (README §4 bis.4, 2026-11-01) : ouvrages de criteria.yaml perimetre.limite_donnees, et toute galerie tunnel=avalanche_protector du corpus (absente du schéma Shortbread) : note limite_donnees, hors du décompte d'E4 (E3 pour un pont) et de S1, rapportés à part (agrégat ouvrages.limite_donnees : mesures et erreur d'altitude contre leur référence, sans seuil)",
                "S3 : stn-acces-nord, balcons de test4 et balcons entre tunnels d'une série, contre vcyclist S",
                "E5 : |service − MNT brut| au milieu des passages dessous (perso) et sur les stations O des témoins négatifs",
                "S1 (README §4 ter.1, V1) : longueur typée = recouvrement continu de l'emprise [s_A, s_B] par les segments bridge/tunnel du service (abscisses from_m/to_m), un segment arrêté à 2,5 m au plus d'une borne, à l'intérieur de l'emprise, prolongé jusqu'à elle ; rappel = typé ÷ (s_B − s_A) ; s_A, s_B = projection continue de A et B sur la trace (polyligne des stations du mode testé) pour les traces perso, abscisses des stations bornes des plages pour les parcours vcyclist (structures.csv bridge/tunnel/missed, témoins D/N, tunnels d'ouvrages.yaml entre portails) ; S2 : témoins négatifs n*, tn*, hn*, longueur typée hors plages X/x ; p06-normandie exclu (ouvrage tenu à l'écart)",
                "Traces perso tenues à l'écart (README §2) : seuls des extraits sont envoyés, fenêtre de ± 500 m autour de chaque ouvrage de calibration franchi (passages_masques) ± 1 km de marge ; service, MNT et baro lus dans la fenêtre seulement ; mesures d'ouvrage des seuls ouvrages de calibration, sans distance curviligne (extrait_tenu_a_l_ecart) ; aucune métrique de trace, aucune sortie hors fenêtre",
                "Passage en hauteur sur un pont (README §2.1, passage_pont.niveau) : MNH au milieu du corpus (médiane MNS 25 m − MNT LiDAR) ≥ 10 m : passage compté seulement si la médiane (baro corrigé − MNT) à ± 12,5 m du milieu (≥ 3 stations) atteint ½ MNH, sinon rapporté en passage_retire_niveau et retiré de E3, S1 et hors_graphe_velo ; MNH < 10 m : niveau_non_discriminant ; moins de 3 stations : niveau_sans_baro",
                "Extraits tenus à l'écart : repli 1–3 km du décalage local lu sur la trace (baro − MNT IGN LiDAR lu directement, hors ponts et tunnels des tuiles), jamais la sortie du service ; seul le décalage est écrit ; decalage_repli_stations_par_cote = stations d'approche avant / après le pont",
                "Attribution d'un passage (README §4 ter.2, V1) : trace brute = private/traces/<trace>.trim1km.gpx (points avec altitude, segments coupés aux sauts > 2 km et gardés dès 5 km, comme tools/stadia_match.py), points dont l'abscisse d'origine est entre celles de s_A et s_B (correspondance s_recale_m → s_origine_m de <id>.baro.csv, interpolée) ; médiane des distances de ces points à la voie de l'ouvrage (élément de la tuile qui suit le mieux la géométrie du corpus, §4 bis.1) et à chaque autre élément bridge/tunnel des tuiles de la campagne (tuile de chaque point, éléments = lignes d'attributs identiques) ; voie de l'ouvrage pas strictement la plus proche : passage_sur_ouvrage_parallele, rapporté, compté ni dans E3, E4, S1 ni dans hors_graphe_velo ; sans point brut ou sans voie de l'ouvrage dans ces tuiles : attribution_indeterminee, passage compté ; E5 non concerné",
                "Passages d'ouvrage (traces perso) : un passage par traversée du disque de 40 m autour du milieu (suite de stations contiguës), A et B à moins de 40 m cherchés sur cette traversée seulement ; E3 sans référence (pas de baro transporté sur l'emprise) : écarts null avec raison_ecart, hors de la médiane du pont",
                "R1/R2/R3 : classe Stadia ramenée à paved/unpaved/unknown (revêtu = paved_smooth, paved, paved_rough ; non revêtu = pire que paved_rough) ; R1 total et hors inconnu Stadia, R2 sur les mètres classables des deux côtés",
                "M1 : non apparié = longueur matched=false ; unknown = longueur surface.class=unknown ; parts de la longueur du service ; traces recalées (README §4 bis.6, 2026-11-01) : plages dont aucune station (pas de 5 m de la grille, sur la géométrie recalée) n'a de ligne de la couche streets à ≤ 30 m (tuile z14 et 8 voisines, cache de la campagne ; tuile absente à la source = sans voie ; tuile absente du cache : cas sauté), élargies d'un demi-pas de chaque côté, sans longueur minimale, retirées de M1 (non apparié, longueur totale, unknown) et rapportées à part (M1.hors_reseau_m par cas, agrégat typologie M1.hors_reseau_m) ; lues sur la trace et les tuiles seulement ; M1 hors graphe vélo (README §4 ter.3, V1) : stations (même grille) dont la ligne la plus proche de la couche streets à ≤ 30 m (toutes lignes, comme hors réseau) est une voie fermée au vélo par la règle de tags du service (kind routable hors rail, bicycle_routable faux : motorway, trunk, pedestrian, bridleway, busway, bicycle=no… ; bicycle=* prime), plages élargies d'un demi-pas, retirées de M1 et rapportées (M1.hors_graphe_velo_m par cas, agrégat typologie M1.hors_graphe_velo) ; contrôle M1.hors_graphe_velo_controle : à chaque station, segment du service matched = false et avertissement unmatched_range ; une station appariée ou sans unmatched_range = échec de M1",
                "R3 : accord entre deux dérivés d'OSM (VersaTiles vs Stadia), pas une vérité terrain ; mesuré aux mêmes seuils, non bloquant (README §4 bis.7, 2026-11-01) : un échec est rapporté sans refuser l'incrément",
                "E2b : D+ baro au preset dem (3 m, 30 m ; criteria.yaml preset_baro: dem, README §4 bis.5), E2b.dplus_baro_dem_m et E2b.ratio* par cas ; E2 (E2.dplus_baro_m, E2.ratio) reste en barometric ; non_discriminant quand le service et sa référence structure_correction=false sont identiques sur tous les cas d'un groupe (ni réussite ni échec ; aucune correction d'ouvrage avant l'incrément 4)",
                "Tronçons .partN recollés en un cas (E1 sur la grille entière, D+ du profil recollé) ; segments .sN : un cas chacun"
            ]
        },
        "cas": cases,
        "ouvrages": ouvrages_public,
        "cas_sautes": skipped,
        "agregats": agregats,
        "verdicts": verdicts,
        "input_with_breakpoints": {
            "mode": "input_with_breakpoints",
            "points_inseres": {
                "longueur_km": round(bp_len, 3),
                "points_inseres": bp_ins,
                "inseres_par_km": round(bp_ins as f64 / bp_len.max(1e-9), 3),
                "max_sur_un_km": max_per_km
            },
            "cas": bp_cases,
            "ouvrages": bp_ouvrages,
            "cas_sautes": bp_rest.skipped,
            "agregats": bp_agregats,
            "verdicts": bp_verdicts
        },
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
    target: Serie,
) -> anyhow::Result<Value> {
    let report = check(paths)?;
    ensure_check(&report)?;
    let manifest = Manifest::load(&paths.bench)?;
    let selection = select_series(&manifest, groupes, ids, target);
    // Série tenue à l'écart (avant `judge`) : ses traces entières et les extraits des traces de
    // calibration autour de ses ouvrages ; rien d'autre (cohérences, golden, témoins : calibration).
    let calibration = target == Serie::Calibration;
    let none: [String; 0] = [];
    let (coherence_routes, hors_git, goldens, extra_gpx): (&[String], &[String], &[String], &[PathBuf]) =
        if calibration {
            (coherence_routes, hors_git, goldens, extra_gpx)
        } else {
            (&none, &none, &none, &[])
        };
    let rules = BenchRules::default();
    let service = Service::start(&paths.cache, false, Variant::DEFAULT).await?;
    let mapterhorn = Service::start(&paths.cache, false, Variant::MAPTERHORN).await?;
    let mut filled = Vec::new();
    if paths.private_present() {
        for t in &selection.cases {
            let Ok(loaded) = load_perso(paths, t, &rules) else { continue };
            for case in loaded {
                crate::inserted::fill(&case, &paths.cache, None).await?;
                // M1 hors réseau (README §4 bis.6) : voisinage de toute la trace recalée.
                crate::off_network::fill(&case, &paths.cache, rules.grid_step_m).await?;
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
    // Traces de l'autre série : tuiles des seuls extraits autour des ouvrages de la série remplie
    // (fenêtre ± 1 km, README §2), jamais de la trace entière.
    if paths.private_present() {
        let refs = ouvrage_refs(paths)?;
        for t in manifest
            .traces
            .iter()
            .filter(|t| selection.excluded_held_out.contains(&t.id) && !t.passages_masques.is_empty())
        {
            let Ok(loaded) = load_perso(paths, t, &rules) else { continue };
            for case in loaded {
                crate::inserted::fill(&case, &paths.cache, Some(&held_out_baro_ranges(&case, &refs, &rules)))
                    .await?;
                for (extract, _) in held_out_extracts(&case, &refs, &rules) {
                    for part in &extract.parts {
                        for sc in [true, false] {
                            service.route_details(part, distance_options(sc, rules.grid_step_m)).await?;
                        }
                    }
                    // Repli 1–3 km : MNT IGN et tuiles de rues seulement, jamais le service.
                    let pts: Vec<LatLon> = extract.ring.iter().map(|r| r.at).collect();
                    let ctx = ord_domain::ctx::RequestCtx::new(
                        ord_domain::budget::RequestId("bench".into()),
                        ord_domain::budget::Deadline::after(std::time::Duration::from_secs(600)),
                    );
                    service.state().orchestrator().sources().terrain.sample_ign_dtm(&pts, &ctx).await;
                    crate::ouvrages::Tiles::load_online(&paths.cache, &pts).await?;
                }
                eprintln!("{} : tuiles des extraits en cache", case.id);
                filled.push(format!("{}#extraits", case.id));
            }
        }
    }
    for entry in manifest.osm_surface.iter().filter(|e| {
        e.serie == target
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
    if ids.is_empty() && calibration {
        for (dir, id, _) in crate::ouvrages::controls(&paths.bench)? {
            let pts = crate::ouvrages::control_points(&paths.bench, &dir, &id)?;
            for sc in [true, false] {
                let _ = crate::ouvrages::query_points(&service, &pts, sc).await?;
            }
            eprintln!("{id} : tuiles en cache");
            filled.push(id);
        }
    }
    for id in crate::golden::STRUCTURE_SCENARIOS.iter().filter(|_| calibration) {
        let scenario = crate::golden::structure_scenario(paths, &manifest, id)?;
        crate::golden::response_with(&service, &scenario, crate::golden::structure_options()).await?;
        filled.push(format!("golden-{id}"));
    }
    if calibration {
        let id = crate::golden::MODES_SCENARIO;
        let scenario = crate::golden::structure_scenario(paths, &manifest, id)?;
        crate::golden::modes_response(&service, &scenario).await?;
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
        "serie": target,
        "exclus_autre_serie": selection.excluded_held_out
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

/// L2 (latence à chaud, hors ligne) : un service sur le cache de banc, la requête `distance` 5 m
/// envoyée une fois pour chauffer les caches mémoire, puis rejouée `trials` fois ; p50 et p95 en
/// ms. Le mode par défaut (`input_with_breakpoints`) est mesuré de même, en diagnostic. Aucune
/// sortie du service n'est écrite, seulement des durées.
pub async fn latency_warm(paths: &BenchPaths, ids: &[String], trials: usize) -> anyhow::Result<Value> {
    let report = check(paths)?;
    ensure_check(&report)?;
    let manifest = Manifest::load(&paths.bench)?;
    let selection = select(&manifest, &[], ids);
    let rules = BenchRules::default();
    let service = Service::start(&paths.cache, true, Variant::DEFAULT).await?;
    let mut out = Vec::new();
    for t in &selection.cases {
        for case in load_perso(paths, t, &rules).map_err(|e| anyhow::anyhow!("{} : {e}", t.id))? {
            if case.parts.len() != 1 {
                bail!("{} : cas en tronçons, hors du protocole L2", case.id);
            }
            let mut modes = serde_json::Map::new();
            for (mode, options) in [
                ("distance", distance_options(true, rules.grid_step_m)),
                ("input_with_breakpoints", json!({"sampling": {"mode": "input_with_breakpoints"}})),
            ] {
                let first = std::time::Instant::now();
                let r = service.route_details(&case.parts[0], options.clone()).await?;
                let first_ms = first.elapsed().as_secs_f64() * 1e3;
                let mut ms = Vec::new();
                for _ in 0..trials.max(1) {
                    let start = std::time::Instant::now();
                    service.route_details(&case.parts[0], options.clone()).await?;
                    ms.push(start.elapsed().as_secs_f64() * 1e3);
                }
                ms.sort_by(f64::total_cmp);
                modes.insert(
                    mode.into(),
                    json!({
                        "longueur_service_m": r["summary"]["length_m"],
                        "points_sortie": r["points"].as_array().map_or(0, Vec::len),
                        "premier_ms": round(first_ms, 1),
                        "p50_ms": round(crate::metrics::percentile(&ms, 50.0), 1),
                        "p95_ms": round(crate::metrics::percentile(&ms, 95.0), 1),
                        "max_ms": round(ms.last().copied().unwrap_or(0.0), 1)
                    }),
                );
            }
            eprintln!("{} : L2 mesuré", case.id);
            out.push(json!({"id": case.id, "groupe": case.groupe, "points_entree": case.parts[0].len(), "modes": modes}));
        }
    }
    let result = json!({
        "date": today_utc(),
        "rules_version": ord_core::RULES_VERSION,
        "profil": if cfg!(debug_assertions) { "debug" } else { "release" },
        "essais": trials,
        "cpus": std::thread::available_parallelism().map_or(1, usize::from),
        "L2": out
    });
    crate::privacy::assert_report_clean(&result, &crate::privacy::private_names(paths))?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// E2b lit les ratios au D+ baro `dem` (`E2b.ratio*`), jamais ceux d'E2 (`barometric`) :
    /// ici, E2 dirait « échec » et E2b, au même preset que le service, « réussite ».
    #[test]
    fn e2b_uses_the_dem_baro_gain_and_e2_stays_barometric() {
        let case = |id: &str, e2: (f64, f64), e2b: (f64, f64)| {
            json!({"id": id, "groupe": "plaine", "metriques": {
                "E1.rmse_m": 1.0, "E1.p95_m": 2.0,
                "E2.dplus_service_m": 100.0, "E2.dplus_mnt_brut_m": 110.0,
                "E2.ratio": e2.0, "E2.ratio_mnt_brut": e2.1,
                "E2b.ratio": e2b.0, "E2b.ratio_mnt_brut": e2b.1}})
        };
        // Baro barometric plus haut : le service paraît plus loin que sa référence brute.
        let cases = [case("a", (0.80, 0.88), (1.01, 1.11)), case("b", (0.82, 0.90), (0.99, 1.09))];
        let (agg, verdicts) = judge(&cases);
        assert_eq!(verdicts["E2b"], "pass", "{agg}");
        assert_eq!(agg["E2b"]["plaine"]["verdict"], "pass");
        assert!(
            (agg["E2b"]["plaine"]["mediane_ecart_log_service"].as_f64().unwrap() - 0.01005).abs() < 1e-4,
            "{agg}"
        );
        // E2 indicatif reste lu en barometric.
        assert_eq!(agg["E2"]["plaine.ratio_mediane"], 0.81);
        // Ratios E2b inversés : échec.
        let cases = [case("a", (0.80, 0.88), (1.11, 1.01)), case("b", (0.82, 0.90), (1.09, 0.99))];
        assert_eq!(judge(&cases).1["E2b"], "fail");
    }

    /// Trace synthétique est-ouest de 10 km (un point tous les 10 m), baro partout, qui franchit un
    /// ouvrage de calibration au kilomètre 5.
    fn straight_case() -> (PersoCase, BTreeMap<String, Vec<LatLon>>) {
        let m_per_deg = haversine_m(LatLon::new(47.0, 0.0), LatLon::new(47.0, 1.0));
        let pts: Vec<Pt> = (0..=1_000).map(|k| Pt { lat: 47.0, lon: k as f64 * 10.0 / m_per_deg }).collect();
        let s_ref = cumulative(&pts);
        let refs = BTreeMap::from([("pont".to_owned(), vec![LatLon::new(47.0, 5_000.0 / m_per_deg)])]);
        let case = PersoCase {
            id: "perso-99".into(),
            groupe: "ouvrage".into(),
            serie: Serie::TenueALEcart,
            parts: vec![pts],
            baro_s: s_ref.clone(),
            baro_z: vec![10.0; s_ref.len()],
            baro_inserted: vec![false; s_ref.len()],
            s_ref,
            masks: vec!["pont".into()],
            ring: Vec::new(),
            raw: None,
        };
        (case, refs)
    }

    #[test]
    fn held_out_extract_covers_the_window_and_a_margin_only() {
        let (case, refs) = straight_case();
        let rules = BenchRules::default();
        let ex = held_out_extracts(&case, &refs, &rules);
        assert_eq!(ex.len(), 1);
        let (e, w) = &ex[0];
        // Fenêtre : ± 500 m autour des stations à moins de 40 m de l'ouvrage (≈ 1 040 à 1 080 m) ;
        // extrait : ± 1 km de plus (à un pas de points près), la fenêtre commence à 1 km du début.
        let len = e.s_ref.last().unwrap() - e.s_ref[0];
        let width = w.end - w.start;
        assert!((1_030.0..=1_090.0).contains(&width), "{w:?}");
        assert!((len - (width + 2_000.0)).abs() <= 25.0, "{len} {w:?}");
        assert!((w.start - 1_000.0).abs() <= 15.0, "{w:?}");
        assert_eq!(e.s_ref[0], 0.0);
        assert!(e.baro_s.iter().all(|s| (-1e-6..=len + 1e-6).contains(s)), "baro de l'extrait seulement");
        assert!(e.masks.is_empty(), "l'ouvrage de calibration est mesuré, pas masqué");
        // Sans ouvrage de calibration franchi : aucun extrait.
        let none = PersoCase { masks: Vec::new(), ..case };
        assert!(held_out_extracts(&none, &refs, &rules).is_empty());
    }

    #[test]
    fn baro_csv_reads_the_inserted_column() {
        let rules = BenchRules::default();
        let text = "s_recale_m,baro_m,s_origine_m,distance_au_trace_m,insere\n\
                    50.0,101.0,50.0,3.0,0\n0.0,100.0,0.0,2.0,0\n25.0,100.5,25.0,0.0,1\n75.0,102.0,75.0,31.0,0\n";
        let (s, z, ins) = parse_baro(text, &rules).unwrap();
        assert_eq!(s, vec![0.0, 25.0, 50.0], "trié, apparié à plus de 30 m écarté");
        assert_eq!(z, vec![100.0, 100.5, 101.0]);
        assert_eq!(ins, vec![false, true, false]);
        let old = "s_recale_m,baro_m,s_origine_m,distance_au_trace_m\n0.0,100.0,0.0,2.0\n";
        assert!(parse_baro(old, &rules).is_err(), "recalage sans colonne insere refusé");
        let bad = "s_recale_m,baro_m,s_origine_m,distance_au_trace_m,insere\n0.0,100.0,0.0,2.0,x\n";
        assert!(parse_baro(bad, &rules).is_err());
    }

    #[test]
    fn held_out_baro_ranges_cover_the_ring_only() {
        let (case, refs) = straight_case();
        let rules = BenchRules::default();
        let w = held_out_windows(&case, &refs, &rules);
        let r = held_out_baro_ranges(&case, &refs, &rules);
        assert_eq!((w.len(), r.len()), (1, 1));
        assert!((w[0].start - r[0].start - 3_100.0).abs() < 1e-9);
        assert!((r[0].end - w[0].end - 3_100.0).abs() < 1e-9);
    }

    #[test]
    fn held_out_items_keep_calibration_structures_without_positions() {
        let items = vec![
            json!({"ouvrage": "pont", "cas": "perso-99#extrait0", "metrique": "E3", "d_milieu_m": 1500.0,
                "ecart_max_d_m": 1510.0, "emprise_d_m": [1450.0, 1550.0], "ecart_milieu_m": 0.5}),
            json!({"ouvrage": "nv-99", "cas": "perso-99#extrait0", "metrique": "E3", "ecart_milieu_m": 9.0}),
        ];
        let out = held_out_items(items, "perso-99", &["pont".to_owned()]);
        assert_eq!(out.len(), 1, "seuls les ouvrages de calibration de la trace");
        assert_eq!(
            out[0],
            json!({"ouvrage": "pont", "cas": "perso-99", "metrique": "E3", "ecart_milieu_m": 0.5,
                "extrait_tenu_a_l_ecart": true})
        );
    }
}
