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

/// Traces de l'autre série lues en extraits autour des éléments de la série mesurée
/// (`passages_masques`, README §2) : traces exclues de la sélection qui en ont. En calibration,
/// jamais une trace d'une série nouvelle tenue à l'écart (V1, V1.1, V1.2) : seul `judge` l'ouvre.
pub fn extract_sources<'a>(
    manifest: &'a Manifest,
    selection: &Selection<'_>,
    target: Serie,
) -> Vec<&'a Trace> {
    manifest
        .traces
        .iter()
        .filter(|t| {
            selection.excluded_held_out.contains(&t.id)
                && !t.passages_masques.is_empty()
                && !(target == Serie::Calibration && manifest.is_new_series(&t.id))
        })
        .collect()
}

/// Points de référence des ouvrages (publics, puis extension perso si présente).
pub fn ouvrage_refs(paths: &BenchPaths) -> anyhow::Result<BTreeMap<String, Vec<LatLon>>> {
    let mut out = BTreeMap::new();
    let mut files = vec![paths.bench.join("data/ouvrages-geom.json")];
    if paths.private_present() {
        files.push(paths.private.join("ouvrages-perso.json"));
    }
    if paths.private_present() {
        // Séries V1, V1.1 et V1.2 : ouvrages, croisements et extrémités des témoins des deux
        // séries (masques), index seulement.
        let manifest = Manifest::load(&paths.bench)?;
        for sec in manifest.sections() {
            out.extend(crate::v1::refs(paths, sec)?);
        }
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
    /// Trace perso fiable (verdict `fiable` de `qualify_baro.py`) : référence baro d'E5
    /// (README §4 ter.7).
    pub baro_fiable: bool,
    /// Bruit baro de `qualify_baro.py` (`noise_m`, E1 plaine, README §4 ter.8).
    pub baro_noise_m: Option<f64>,
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
    /// Masques ajoutés par le banc (éléments V1 de l'autre série trouvés sur le parcours, sans
    /// liste au manifest) : pas rapportés quand ils ne sont pas retrouvés.
    pub implicit_masks: Vec<String>,
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
    /// « Trace brute » d'un itinéraire planifié : l'itinéraire lui-même, abscisses identiques.
    pub fn identity(pts: Vec<LatLon>, s: Vec<f64>) -> Self {
        Self { map_s: s.clone(), map_o: s.clone(), pts, s }
    }

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
        baro_fiable: trace.verdict.as_deref() == Some("fiable"),
        baro_noise_m: trace.noise_m,
        parts,
        s_ref,
        baro_s,
        baro_z,
        baro_inserted,
        masks: trace.passages_masques.clone(),
        implicit_masks: Vec::new(),
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
    held_out_extracts_lagged(case, refs, rules, 0.0)
}

/// `held_out_extracts`, baro du repli 1–3 km lu corrigé du retard `shift` de la trace
/// (baro(s + δ), abscisse de la trace, README §4 ter.9) : δ gelé d'un extrait de trace de
/// calibration au jugement, 0 sinon (baro brut). Le baro de l'extrait reste brut ; `measure_case`
/// le corrige du même δ (`OuvrageEval::retard`).
pub fn held_out_extracts_lagged(
    case: &PersoCase,
    refs: &BTreeMap<String, Vec<LatLon>>,
    rules: &BenchRules,
    shift: f64,
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
            baro_fiable: case.baro_fiable,
            baro_noise_m: case.baro_noise_m,
            parts: vec![pts[first..=last].to_vec()],
            s_ref: case.s_ref[first..=last].iter().map(|x| x - s0).collect(),
            baro_s,
            baro_z,
            baro_inserted,
            // Les ouvrages de calibration de l'extrait sont mesurés, pas masqués.
            masks: Vec::new(),
            implicit_masks: Vec::new(),
            ring: ring_points(
                case,
                &ll,
                (w.start - HELD_OUT_RING_M)..(w.end + HELD_OUT_RING_M),
                s0,
                shift,
                rules,
            ),
            raw: case.raw.as_ref().map(|r| RawRef { gps: r.gps.clone(), shift: r.shift + s0 }),
        };
        out.push((extract, (w.start - s0)..(w.end - s0)));
    }
    out
}

/// Fenêtres masquées d'un élément d'une autre série sur un profil (README §2) : ± 500 m autour de
/// chaque passage ; un témoin V1 masque tout son tronçon, du premier au dernier passage près de
/// ses extrémités.
pub fn mask_windows(
    d: &[f64],
    pts: &[LatLon],
    id: &str,
    refs: &BTreeMap<String, Vec<LatLon>>,
    rules: &BenchRules,
) -> Vec<std::ops::Range<f64>> {
    let Some(r) = refs.get(id) else { return Vec::new() };
    let w = passage_windows(d, pts, r, rules.passage_tol_m, rules.mask_half_m);
    if crate::v1::is_temoin_id(id) && w.len() > 1 {
        let start = w.iter().map(|x| x.start).fold(f64::INFINITY, f64::min);
        let end = w.iter().map(|x| x.end).fold(f64::NEG_INFINITY, f64::max);
        return vec![start..end];
    }
    w
}

/// Stations du mode `distance` d'un cas au pas `step`, calculées sans le service comme il les
/// place (`ord_core::prepare` et `sample_output`) : sur chaque tronçon, points d'entrée
/// dédoublonnés, distance cumulée (haversine), stations à `k × step` puis à la fin du tronçon,
/// position interpolée sur la géométrie envoyée, arrondis de sortie (distance au cm, coordonnées à
/// 1e-6°) ; tronçons recollés comme `recolle` (station de raccord retirée, distances du tronçon
/// N+1 décalées de la longueur des précédents). Ce sont les stations où la calibration calcule ses
/// fenêtres masquées (`mask_windows(&p.d, &p.pts, …)`).
pub fn distance_stations(case: &PersoCase, step: f64) -> (Vec<f64>, Vec<LatLon>) {
    let (mut d, mut pts) = (Vec::new(), Vec::new());
    let mut offset = 0.0;
    for (k, part) in case.parts.iter().enumerate() {
        let (pd, pp) = part_stations(part, step);
        let skip = usize::from(k > 0);
        d.extend(pd.iter().skip(skip).map(|x| x + offset));
        pts.extend(pp.iter().skip(skip));
        offset += pd.last().copied().unwrap_or(0.0);
    }
    (d, pts)
}

/// Stations `distance` d'un tronçon (voir `distance_stations`).
fn part_stations(part: &[Pt], step: f64) -> (Vec<f64>, Vec<LatLon>) {
    const EPS: f64 = ord_core::prepare::DUPLICATE_EPS_DEG;
    // Dernier pas confondu avec la fin du tronçon (`LAST_STEP_EPS_M` du service).
    const LAST_STEP_EPS_M: f64 = 1e-3;
    let mut kept: Vec<LatLon> = Vec::with_capacity(part.len());
    for p in part {
        let q = LatLon::new(p.lat, p.lon);
        if !kept.last().is_some_and(|l| (q.lat - l.lat).abs() <= EPS && (q.lon - l.lon).abs() <= EPS) {
            kept.push(q);
        }
    }
    let Some(&first) = kept.first() else { return (Vec::new(), Vec::new()) };
    let mut cum = vec![0.0];
    for w in kept.windows(2) {
        cum.push(cum[cum.len() - 1] + haversine_m(w[0], w[1]));
    }
    let length = cum[cum.len() - 1];
    let at = |x: f64| -> LatLon {
        if kept.len() < 2 {
            return first;
        }
        let i = cum.partition_point(|v| *v <= x).clamp(1, cum.len() - 1);
        let (a, b) = (kept[i - 1], kept[i]);
        let span = cum[i] - cum[i - 1];
        let t = if span > 0.0 { ((x - cum[i - 1]) / span).clamp(0.0, 1.0) } else { 0.0 };
        LatLon::new(a.lat + t * (b.lat - a.lat), a.lon + t * (b.lon - a.lon))
    };
    let step = if step.is_finite() && step > 0.0 { step } else { f64::INFINITY };
    let (mut d, mut pts) = (Vec::new(), Vec::new());
    let mut k: u64 = 0;
    loop {
        let x = k as f64 * step;
        if x > length + LAST_STEP_EPS_M || (k > 0 && !step.is_finite()) {
            break;
        }
        d.push(x.min(length));
        pts.push(at(x.min(length)));
        k += 1;
    }
    if d.last().is_some_and(|x| length - x > LAST_STEP_EPS_M) {
        d.push(length);
        pts.push(at(length));
    }
    // Arrondis de sortie du service (`ord_core::round`) : distances au cm, coordonnées à 1e-6°.
    let coord = |v: f64| (v * 1_000_000.0).round() / 1_000_000.0;
    (
        d.into_iter().map(|x| (x * 100.0).round() / 100.0).collect(),
        pts.into_iter().map(|p| LatLon::new(coord(p.lat), coord(p.lon))).collect(),
    )
}

/// Abscisse du cas (`s_ref`) d'une distance du service (`request_distances`) : interpolation
/// entre les points envoyés, prolongée au pas de 1 m pour 1 m au-delà des extrémités (une fenêtre
/// de ± 500 m déborde du parcours sans être rognée).
fn service_to_case(d_srv: &[f64], s_ref: &[f64], x: f64) -> f64 {
    match (d_srv.first(), d_srv.last(), s_ref.first(), s_ref.last()) {
        (Some(&d0), _, Some(&s0), _) if x <= d0 => s0 + (x - d0),
        (_, Some(&d1), _, Some(&s1)) if x >= d1 => s1 + (x - d1),
        (Some(_), Some(_), Some(_), Some(_)) => interp_one(d_srv, s_ref, x),
        _ => x,
    }
}

/// Fenêtres masquées de chaque élément de `case.masks` (abscisses du cas), calculées exactement
/// comme en calibration : `mask_windows` sur les stations du mode `distance` au pas de la grille
/// (`distance_stations`), mêmes tolérance (`passage_tol_m`), demi-largeur, regroupement des
/// passages et tronçon entier d'un témoin ; distances du service ramenées à l'abscisse du cas.
/// Un élément sans passage a une liste vide.
pub fn held_out_windows_by_id(
    case: &PersoCase,
    refs: &BTreeMap<String, Vec<LatLon>>,
    rules: &BenchRules,
) -> Vec<(String, Vec<std::ops::Range<f64>>)> {
    let d_srv = request_distances(case);
    if d_srv.len() != case.s_ref.len() {
        return case.masks.iter().map(|id| (id.clone(), Vec::new())).collect();
    }
    let (d, pts) = distance_stations(case, rules.grid_step_m);
    let to_case = |x: f64| service_to_case(&d_srv, &case.s_ref, x);
    case.masks
        .iter()
        .map(|id| {
            let w = mask_windows(&d, &pts, id, refs, rules);
            (id.clone(), w.into_iter().map(|w| to_case(w.start)..to_case(w.end)).collect())
        })
        .collect()
}

/// Fenêtres évaluées d'un cas d'une trace tenue à l'écart (abscisses du cas) : les fenêtres
/// masquées en calibration de ses ouvrages de calibration (`passages_masques`,
/// `held_out_windows_by_id`), fusionnées.
pub fn held_out_windows(
    case: &PersoCase,
    refs: &BTreeMap<String, Vec<LatLon>>,
    rules: &BenchRules,
) -> Vec<std::ops::Range<f64>> {
    let mut windows: Vec<std::ops::Range<f64>> =
        held_out_windows_by_id(case, refs, rules).into_iter().flat_map(|(_, w)| w).collect();
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

/// Contrôle des extraits d'une trace de l'autre série (`passages_masques`, tous ses cas `.sN`),
/// sur les stations du mode `distance` recalculées (`distance_stations`, celles de la calibration),
/// sans le service. Raisons sans position.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeldOutLosses {
    /// Cas sautés (un jugement n'en tolère aucun) : élément retrouvé (station à `passage_tol_m`
    /// au plus d'un de ses points) mais sans fenêtre, élément absent du corpus, témoin V1 dont
    /// aucun cas n'a de station près de chacune de ses extrémités, fenêtre sans extrait.
    pub skipped: Vec<String>,
    /// Éléments qu'aucune station n'approche à `passage_tol_m` : pas un passage, comme en
    /// calibration (`passages_non_retrouves`) ; rapportés (`passage_non_retrouve`), sans
    /// interrompre le jugement (décision de l'utilisateur, 2026-11-02).
    pub not_found: Vec<String>,
}

pub fn held_out_losses(
    passages: &[String],
    cases: &[PersoCase],
    refs: &BTreeMap<String, Vec<LatLon>>,
    rules: &BenchRules,
) -> HeldOutLosses {
    let mut out = HeldOutLosses::default();
    let by_case: Vec<Vec<(String, Vec<std::ops::Range<f64>>)>> =
        cases.iter().map(|c| held_out_windows_by_id(c, refs, rules)).collect();
    let stations: Vec<Vec<LatLon>> =
        cases.iter().map(|c| distance_stations(c, rules.grid_step_m).1).collect();
    let near = |pts: &[LatLon], e: &LatLon| pts.iter().any(|p| haversine_m(*p, *e) <= rules.passage_tol_m);
    for id in passages {
        let Some(r) = refs.get(id).filter(|r| !r.is_empty()) else {
            out.skipped.push(format!("{id} : absent du corpus"));
            continue;
        };
        let windowed = by_case.iter().flatten().any(|(x, w)| x == id && !w.is_empty());
        if crate::v1::is_temoin_id(id) {
            let whole = r.len() == 2 && stations.iter().any(|pts| r.iter().all(|e| near(pts, e)));
            if !whole {
                out.skipped.push(format!("{id} : témoin aux extrémités non retrouvées"));
            } else if !windowed {
                out.skipped.push(format!("{id} : témoin retrouvé sans fenêtre"));
            }
            continue;
        }
        let approached = stations.iter().any(|pts| r.iter().any(|e| near(pts, e)));
        if !approached {
            out.not_found.push(id.clone());
        } else if !windowed {
            out.skipped.push(format!("{id} : passage retrouvé sans fenêtre"));
        }
    }
    for c in cases {
        let (w, e) = (held_out_windows(c, refs, rules).len(), held_out_extracts(c, refs, rules).len());
        if w != e {
            out.skipped.push(format!("{} : {} fenêtres, {} extraits", c.id, w, e));
        }
    }
    out
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
/// le baro transporté corrigé du retard `shift`, baro(s + δ) (`NaN` au-delà d'un trou de baro).
fn ring_points(
    case: &PersoCase,
    ll: &[LatLon],
    range: std::ops::Range<f64>,
    s0: f64,
    shift: f64,
    rules: &BenchRules,
) -> Vec<RingPoint> {
    let idx: Vec<usize> = (0..ll.len()).filter(|&i| range.contains(&case.s_ref[i])).collect();
    let s: Vec<f64> = idx.iter().map(|&i| case.s_ref[i]).collect();
    let lagged: Vec<f64> = s.iter().map(|x| x + shift).collect();
    let baro = interpolate(&case.baro_s, &case.baro_z, &lagged, rules.baro_gap_max_m);
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

/// Retard du baro des extraits d'un cas de la trace `trace` (README §4 ter.9) : trace de
/// calibration (extraits lus au jugement), δ gelé de `retard-baro-v1.json` pour le cas, sans
/// réestimation (30e8024, 87b0ea8) ; trace de l'autre série (extraits en calibration), baro brut
/// (`extrait_tenu_a_l_ecart`).
pub fn extract_retard(
    trace: &Trace,
    case_id: &str,
    frozen: &crate::retard::RetardGele,
) -> crate::retard::Retard {
    if trace.serie == Serie::Calibration {
        frozen.extract(case_id)
    } else {
        crate::retard::Retard::not_estimated("extrait_tenu_a_l_ecart")
    }
}

/// Mesures d'ouvrage d'une trace de l'autre série, sur ses extraits : seuls les passages de ses
/// ouvrages de la série mesurée dans la fenêtre évaluée, sans position (distances retirées) ;
/// aucune métrique de trace ni sortie du service hors fenêtre. Renvoie aussi, par extrait (id
/// opaque), la source et la valeur de son δ (`extract_retard`).
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
    temoins: &[crate::v1::TemoinV1],
    frozen: &crate::retard::RetardGele,
) -> anyhow::Result<(Vec<Value>, Vec<Value>)> {
    let mut items = Vec::new();
    let mut extracts = Vec::new();
    let retard = extract_retard(trace, &case.id, frozen);
    for (extract, window) in held_out_extracts_lagged(case, refs, rules, retard.shift_m()) {
        let ring = ring_reference(service, cache, &extract.ring).await?;
        let eval =
            OuvrageEval { cache, window: Some(window), ring: Some(ring), retard: Some(retard.clone()) };
        let (_trace_metrics, _, its) =
            measure_case(service, &extract, None, refs, ouvrages, rules, rules_core, &eval, temoins).await?;
        extracts.push(json!({"id": extract.id, "retard_baro": retard.to_json()}));
        items.extend(held_out_items(its, &trace.id, &trace.passages_masques));
    }
    Ok((items, extracts))
}

/// Items d'un extrait tenu à l'écart : ouvrages de `passages_masques` seulement, cas = la trace,
/// distances curvilignes de l'extrait retirées.
pub fn held_out_items(items: Vec<Value>, trace: &str, calibration: &[String]) -> Vec<Value> {
    items
        .into_iter()
        .filter(|i| {
            i["ouvrage"]
                .as_str()
                .or_else(|| i["temoin"].as_str())
                .is_some_and(|o| calibration.iter().any(|c| c == o))
        })
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
    /// Extrait : retard du baro reçu de sa trace, jamais réestimé (`extract_retard` : δ gelé au
    /// jugement, baro brut en calibration) ; `None` sur un extrait : baro brut.
    pub retard: Option<crate::retard::Retard>,
}

impl<'a> OuvrageEval<'a> {
    /// Cas entier : trace de la série mesurée (calibration au `run`, série tenue à l'écart au
    /// `judge`) ou itinéraire planifié ; ni fenêtre ni repli d'extrait, retard du baro estimé.
    pub fn whole(cache: &'a Path) -> Self {
        Self { cache, window: None, ring: None, retard: None }
    }
}

/// Retard du baro d'un cas (README §4 ter.9) : δ estimé sur le cas entier (tronçons `.partN`
/// recousus, segment `.sN` seul), MNT = sortie sans correction du mode `distance`, hors fenêtres
/// masquées (points insérés écartés déjà retirés de `bd`, `bz`). Extrait (`eval.window`) : plus
/// court que la fenêtre de la dérive, jamais estimé ; il reçoit le retard de sa trace
/// (`eval.retard`) : δ gelé pour une trace de calibration au jugement (30e8024), baro brut sinon
/// (`extrait_tenu_a_l_ecart`, 7dab276). Au jugement, les traces de la série tenue à l'écart sont
/// des cas entiers (`OuvrageEval::whole`) : leur δ est estimé normalement.
pub fn case_retard(
    eval: &OuvrageEval<'_>,
    d: &[f64],
    bd: &[f64],
    bz: &[f64],
    mnt: &[f64],
    masked: &[bool],
    rules: &BenchRules,
) -> crate::retard::Retard {
    if eval.window.is_some() {
        return eval
            .retard
            .clone()
            .unwrap_or_else(|| crate::retard::Retard::not_estimated("extrait_tenu_a_l_ecart"));
    }
    if bd.is_empty() {
        return crate::retard::Retard::not_estimated("sans_baro");
    }
    let usable: Vec<bool> = masked.iter().map(|m| !m).collect();
    crate::retard::estimate(
        d,
        bd,
        bz,
        mnt,
        &usable,
        rules.baro_gap_max_m,
        rules.detrend_window_m,
        rules.detrend_step_m,
    )
}

/// Points bruts de baro d'E5 (README §4 ter.7) : abscisses corrigées du retard `bd_corr`
/// (s_p − δ, 085d26e), hors fenêtres masquées et hors de la fenêtre évaluée d'un extrait, lues
/// à cette même abscisse (celle de la station où le point entre dans le décalage).
pub fn e5_baro_points(
    bd_corr: &[f64],
    masks: &[std::ops::Range<f64>],
    window: Option<&std::ops::Range<f64>>,
) -> Vec<f64> {
    bd_corr
        .iter()
        .copied()
        .filter(|s| !masks.iter().any(|w| w.contains(s)))
        .filter(|s| window.is_none_or(|w| w.contains(s)))
        .collect()
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
        windows.extend(mask_windows(&p.d, &p.pts, id, refs, rules));
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

/// Contrôle M1 hors graphe vélo (README §4 ter.3 ; criteria `controle.passe_si_un_de`, 62bd46e) :
/// une station de la plage passe si le service l'apparie (`matched`) — la route fermée étant exclue
/// du graphe vélo par la même règle de tags que le service, c'est une voie du graphe distincte de
/// la route fermée (voie parallèle) —, ou si elle est non appariée sous un `unmatched_range`.
/// Une station non appariée sans `unmatched_range` est un échec de M1 (une station appariée à la
/// route fermée elle-même serait `matched = false`, avec `bicycle_forbidden`, api 1.1).
pub fn closed_control(
    closed_at: &[f64],
    ours: &[crate::typology::Ours],
    warn_ranges: &[(f64, f64)],
) -> Value {
    let matched = |st: f64| ours.iter().find(|o| o.from <= st && st < o.to).is_some_and(|o| o.matched);
    let warned = |st: f64| warn_ranges.iter().any(|w| w.0 - 1e-6 <= st && st <= w.1 + 1e-6);
    let parallel = closed_at.iter().filter(|&&st| matched(st)).count();
    let unwarned = closed_at.iter().filter(|&&st| !matched(st) && !warned(st)).count();
    json!({
        "stations": closed_at.len(),
        "appariees_voie_parallele": parallel,
        "sans_unmatched_range": unwarned,
        "ok": unwarned == 0,
    })
}

/// Mesure E1 et E2 d'un cas (service appelé deux fois : corrections actives, puis MNT brut).
#[allow(clippy::too_many_arguments)]
pub async fn measure_case(
    service: &Service,
    case: &PersoCase,
    surface: Option<crate::reference_error::Exclusion>,
    refs: &BTreeMap<String, Vec<LatLon>>,
    ouvrages: &[crate::ouvrages::Ouvrage],
    rules: &BenchRules,
    rules_core: &Rules,
    eval: &OuvrageEval<'_>,
    temoins: &[crate::v1::TemoinV1],
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
        let w = mask_windows(&p.d, &p.pts, id, refs, rules);
        if w.is_empty() && !case.implicit_masks.contains(id) {
            not_found.push(id.clone());
        }
        windows.extend(w);
    }
    let masked: Vec<bool> = p.d.iter().map(|d| windows.iter().any(|w| w.contains(d))).collect();
    // Retard du baro (README §4 ter.9, `case_retard`) : baro corrigé(s) = baro(s + δ) pour E1, E3
    // et E5 (décalages et baro au croisement), baro brut pour E2 et E2b.
    let retard = case_retard(eval, &p.d, &bd, &bz, &ref_raw, &masked, rules);
    let shift = retard.shift_m();
    let bd_corr: Vec<f64> = bd.iter().map(|s| s - shift).collect();
    let baro_corr = interpolate(&bd_corr, &bz, &p.d, rules.baro_gap_max_m);
    let keep_with = |b: &[f64], v: &[f64], i: usize| {
        if masked[i] || p.ele[i].is_nan() || b[i].is_nan() {
            f64::NAN
        } else {
            v[i]
        }
    };
    let keep = |v: &[f64], i: usize| keep_with(&baro, v, i);
    let n = p.d.len();
    let svc: Vec<f64> = (0..n).map(|i| keep(&p.ele, i)).collect();
    let svc_raw: Vec<f64> = (0..n).map(|i| keep(&p_raw.ele, i)).collect();
    let bar: Vec<f64> = (0..n).map(|i| keep(&baro, i)).collect();
    // E1 au baro corrigé de son retard (stations où il existe).
    let svc_e1: Vec<f64> = (0..n).map(|i| keep_with(&baro_corr, &p.ele, i)).collect();
    let bar_e1: Vec<f64> = (0..n).map(|i| keep_with(&baro_corr, &baro_corr, i)).collect();

    // Zones de pente du baro sans relief (README §4 quater.3, 2744983, 633255f) : après δ, avant la
    // marche ; baro corrigé et MNT du mode distance sans correction, stations hors ouvrage et
    // fenêtre masquée. Masquées pour E1 (décalage, tendance, RMSE, P95, km comparés).
    let excluded_e1: Vec<bool> = (0..n).map(|i| masked[i] || p.on_structure[i] || p.corrected[i]).collect();
    let slope_zones = crate::marche::slope_zones(&p.d, &baro_corr, &ref_raw, &excluded_e1);
    let in_slope_zone = crate::marche::in_zones(&p.d, &slope_zones);
    // Marche du baro (README §4 ter.11, 3f8a810) : après δ et les zones de pente, avant le décalage
    // et la tendance d'E1 ; résidu baro(s + δ) − MNT (mode distance sans correction), hors ouvrage,
    // masque, zone de pente et sans baro en s comme en s + δ. Une marche rend la référence E1
    // incertaine (rapportée, non jugée).
    let excluded_marche: Vec<bool> = (0..n).map(|i| excluded_e1[i] || in_slope_zone[i]).collect();
    let marche = crate::marche::find(
        &p.d,
        &crate::marche::residual(&p.d, &baro_corr, &ref_raw, &excluded_marche, shift),
        &ref_raw,
    );
    // E1 hors ouvrage (stations `ground`) : ni segment pont ou tunnel, ni altitude d'ouvrage
    // (tablier, interpolation, rampes jusqu'aux sondes) ; hors zones de pente du baro.
    let off_structure = |v: &[f64]| -> Vec<f64> {
        (0..n)
            .map(|i| if p.on_structure[i] || p.corrected[i] || in_slope_zone[i] { f64::NAN } else { v[i] })
            .collect()
    };
    let m1 = e1(
        &p.d,
        &off_structure(&svc_e1),
        &off_structure(&bar_e1),
        rules.detrend_window_m,
        rules.detrend_step_m,
    );
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
    // Référence Valhalla privée des plages d'erreur de la référence (README §4 bis.8, 562d729).
    let stadia = surface.as_ref().map(|x| x.reference.clone()).unwrap_or_default();
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
    let (closed, closed_at) = if eval.window.is_none() || !temoins.is_empty() {
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
    let ours_m1 = crate::typology::without(&ours, &crate::off_network::union(&off_network, &closed));
    let m1_agr =
        agreement(&ours_m1, &[Ref { from: f64::NEG_INFINITY, to: f64::INFINITY, class: Class::Unknown }]);
    let mut typo = if stadia.is_empty() {
        json!({"R3": "reference_absente"})
    } else {
        metrics_json(&agreement(&ours, &stadia), "R3")
    };
    if let Some(x) = surface.as_ref().filter(|_| !stadia.is_empty()) {
        x.json_into(&mut typo, "R3");
    }
    typo["M1.non_apparie"] = json!(round(m1_agr.unmatched, 4));
    typo["M1.unknown"] = json!(round(m1_agr.unknown, 4));
    typo["M1.hors_reseau_m"] = json!(round(crate::off_network::length(&off_network), 1));
    typo["M1.hors_graphe_velo_m"] = json!(round(crate::off_network::length(&closed), 1));
    typo["M1.hors_graphe_velo_controle"] = closed_control(&closed_at, &ours, &warn_ranges);
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
    // Stations comparées (baro brut) des zones de pente : hors des km comparés d'E1.
    let compared_in_zones = (0..n).filter(|&i| in_slope_zone[i] && !bar[i].is_nan()).count();
    let case_json = json!({
        "id": case.id,
        "serie": case.serie,
        "groupe": case.groupe,
        "bruit_baro_m": case.baro_noise_m,
        "marche_baro": marche.map(|m| m.to_json()),
        "zones_pente_baro": crate::marche::zones_json(&slope_zones, compared_in_zones),
        "retard_baro": retard.to_json(),
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
            "passages_masques": case.masks.iter().filter(|m| !case.implicit_masks.contains(m)).collect::<Vec<_>>(),
            "masques_v1_implicites": case.masks.iter().filter(|m| case.implicit_masks.contains(m)).count(),
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
        (0..n).map(|i| if masked[i] || !inside(i) { f64::NAN } else { baro_corr[i] }).collect();
    // Points bruts de baro (E5, 6bc77a5) à leur abscisse corrigée du retard, s_p − δ (085d26e :
    // là où chacun entre dans le décalage du côté), hors fenêtres masquées et hors de la fenêtre
    // évaluée d'un extrait, lues à cette même abscisse.
    let baro_points = e5_baro_points(&bd_corr, &windows, eval.window.as_ref());
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
    // Témoins V1 : ponts et tunnels des tuiles sur tout le cas (abords exclus de S2).
    if !temoins.is_empty() {
        near.extend(p.pts.iter().copied());
    }
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
        baro_points: &baro_points,
        on_structure: &p.on_structure,
        ign: &p.ign,
        matched: &p.matched,
        unmatched_warn: &p.unmatched_warn,
        tile_structure: &tile_structure,
        ring: eval.ring.as_deref(),
        structure_segs: &p.structure_segs,
        raw_gps: Some(&raw_gps),
        tiles: Some(&tiles),
        baro_fiable: case.baro_fiable,
        baro_noise_m: case.baro_noise_m,
    };
    let mut items = crate::ouvrages::perso_items(&case.id, &cp, ouvrages, rules.passage_tol_m);
    // Témoins négatifs V1 portés par ce cas (README §4 ter.4–6) ; plages hors graphe vélo en
    // abscisse du profil.
    let closed_d: Vec<std::ops::Range<f64>> = closed.iter().map(|c| to_d(c.start)..to_d(c.end)).collect();
    for t in temoins {
        if let Some(it) = crate::ouvrages::temoin_v1_item(
            t,
            &p.d,
            &p.pts,
            &p.on_structure,
            &tile_structure,
            &closed_d,
            eval.window.as_ref(),
            rules.passage_tol_m,
        ) {
            items.extend(it);
        }
    }
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
/// E1 plaine (README §4 ter.8, criteria `reference_incertaine_si`) : bruit baro de
/// `qualify_baro.py` au-delà duquel la référence est incertaine, rapportée, non jugée.
const E1_PLAINE_NOISE_MAX_M: f64 = 1.0;

/// Référence E1 incertaine : trace `plaine` au bruit baro > 1,0 m (README §4 ter.8), ou trace de
/// toute strate avec une marche du baro (`marche_baro`, README §4 ter.11, 3f8a810).
pub fn e1_uncertain(c: &Value) -> bool {
    (c["groupe"] == "plaine" && c["bruit_baro_m"].as_f64().is_some_and(|b| b > E1_PLAINE_NOISE_MAX_M))
        || c["marche_baro"].is_object()
}
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
    let mut e1_uncertain_list = Vec::new();
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
                let (r, q) = (metric(c, "E1.rmse_m"), metric(c, "E1.p95_m"));
                if e1_uncertain(c) {
                    // Rapportée, non jugée (README §4 ter.8).
                    e1_uncertain_list.push(
                        json!({"id": c["id"], "groupe": g, "bruit_baro_m": c["bruit_baro_m"],
                        "marche_baro": c["marche_baro"], "rmse_m": r, "p95_m": q,
                        "pass_indicatif": r <= rmse_max && q <= p95_max}),
                    );
                    continue;
                }
                e1_judged += 1;
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
        "E1_references_incertaines": e1_uncertain_list,
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
        per_group.entry(c["groupe"].as_str().unwrap_or("osm_surface").to_owned()).or_default().push(c);
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
    // Erreur de la référence Valhalla (README §4 bis.8, 562d729) : hors décompte de R1, R2 et R3,
    // longueur et part par cas rapportées à part (parcours planifiés R1/R2, traces perso R3).
    let mut err = serde_json::Map::new();
    let (mut err_m, mut ref_m) = (0.0, 0.0);
    for c in os_cases.iter().chain(perso) {
        let t = &c["typologie"];
        let Some((p, m)) =
            ["R1", "R3"].into_iter().find_map(|p| Some((p, t[format!("{p}.erreur_reference_m")].as_f64()?)))
        else {
            continue;
        };
        let Some(id) = c["id"].as_str() else { continue };
        err_m += m;
        ref_m += t[format!("{p}.reference_m")].as_f64().unwrap_or(0.0);
        err.insert(
            id.to_owned(),
            json!({"critere": p, "longueur_m": m, "part": t[format!("{p}.erreur_reference_part")]}),
        );
    }
    agg.insert(
        "erreur_reference".into(),
        json!({"par_cas": err, "longueur_m": round(err_m, 1), "reference_m": round(ref_m, 1),
            "part": round(if ref_m > 0.0 { err_m / ref_m } else { 0.0 }, 4)}),
    );
    // Voies aux tags différents entre dates (README §4 quater.1) : parcours planifiés R1/R2, par
    // parcours (longueur, part) ; `instantanes_absents_du_manifest` si la règle n'a pas tourné.
    let tags: serde_json::Map<String, Value> = os_cases
        .iter()
        .filter(|c| c["groupe"] == "planifie_R1_R2")
        .filter(|c| !c["typologie"]["R1.tags_differents"].is_null())
        .filter_map(|c| Some((c["id"].as_str()?.to_owned(), c["typologie"]["R1.tags_differents"].clone())))
        .collect();
    if !tags.is_empty() {
        let total: f64 = tags.values().filter_map(|v| v["longueur_m"].as_f64()).sum();
        agg.insert("tags_differents".into(), json!({"par_cas": tags, "longueur_m": round(total, 1)}));
    }
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
    /// Itinéraires planifiés E4 / S2 (README §4 ter.4) : sans baro, ni E1 ni E2.
    planned: Vec<Value>,
}

/// Séries V1, V1.1 et V1.2 d'une campagne (manifest `perso_v1`, `perso_v11`, `perso_v12`) : séries par cas, témoins
/// de la série mesurée, itinéraires planifiés retenus. Vide sans `bench/private/` ou sans section.
#[derive(Clone, Debug, Default)]
pub struct V1Ctx {
    pub rows: Vec<crate::v1::SeriesRow>,
    pub temoins: Vec<crate::v1::TemoinV1>,
    pub planned: crate::v1::PlannedSelection,
}

/// Contexte V1, V1.1 et V1.2 d'une campagne de la série `target` : l'autre série est retirée des
/// index ici, avant tout calcul ; chaque section est lue avec son propre index (`series-v1.csv`,
/// `series-v11.csv`, `series-v12.csv`), empreinte vérifiée.
pub fn v1_ctx(
    paths: &BenchPaths,
    manifest: &Manifest,
    ids: &[String],
    target: Serie,
) -> anyhow::Result<V1Ctx> {
    let mut ctx = V1Ctx::default();
    if !paths.private_present() {
        return Ok(ctx);
    }
    for sec in manifest.sections() {
        let rows = crate::v1::series_rows(paths, sec)?;
        crate::v1::select_planned_in(sec, &rows, ids, target, &mut ctx.planned);
        ctx.temoins.extend(crate::v1::temoins(paths, sec, target)?);
        ctx.rows.extend(rows);
    }
    Ok(ctx)
}

impl V1Ctx {
    /// Témoins de la série mesurée portés par un parcours (trace ou itinéraire).
    pub fn temoins_on(&self, trace: &str) -> Vec<crate::v1::TemoinV1> {
        self.temoins.iter().filter(|t| t.trace == trace).cloned().collect()
    }
}

/// Cas d'une série tenue à l'écart entière (V1.1 : README §4 ter.10, 22dbba7 ; V1.2 : README
/// §4 quinquies, manifest `perso_v12.regle_serie`) à son jugement : jugé entier, sans aucun
/// masque ; les fenêtres des éléments de calibration qu'il franchit ne sont ni masquées ni tirées
/// en extraits (`fenetres_calibration` de `series-v11.csv` ou `series-v12.csv` n'est jamais lue).
/// En calibration, ses éléments restent masqués sur les traces de calibration. Une série déjà jugée
/// (calibration) et V1 (jugée avec masques et extraits) ne sont pas concernées.
pub fn judged_whole(manifest: &Manifest, target: Serie, id: &str) -> bool {
    target == Serie::TenueALEcart
        && [manifest.v11.as_ref(), manifest.v12.as_ref()]
            .into_iter()
            .flatten()
            .filter(|v| !v.judged)
            .any(|v| v.traces.contains(id) || v.planifies.iter().any(|p| p.id == id))
}

/// Masques ajoutés à un cas : éléments V1 de l'autre série sur son parcours (index seulement).
fn add_implicit_masks(
    case: &mut PersoCase,
    refs: &BTreeMap<String, Vec<LatLon>>,
    v1: &V1Ctx,
    target: Serie,
    own: &str,
) {
    if v1.rows.is_empty() {
        return;
    }
    let pts = crate::v1::case_points(case);
    for id in crate::v1::other_series_on(&pts, refs, &v1.rows, target, own) {
        if !case.masks.contains(&id) {
            case.masks.push(id.clone());
            case.implicit_masks.push(id);
        }
    }
}

/// Instantanés des tags OSM lus pendant une passe, par date de recalage (README §4 bis.8).
#[derive(Debug, Default)]
pub struct SnapshotCache(BTreeMap<String, crate::reference_error::OsmTags>);

impl SnapshotCache {
    /// Instantané de la date du recalage du cas `id` (`Manifest::recalage_section`), lu et vérifié
    /// une fois par date ; absent ou d'empreinte différente : refus de la campagne.
    pub fn for_case(
        &mut self,
        paths: &BenchPaths,
        manifest: &Manifest,
        id: &str,
    ) -> anyhow::Result<&crate::reference_error::OsmTags> {
        let date = manifest
            .recalage_section(id)
            .and_then(|s| s.donnees_serveur.clone())
            .map(|d| crate::reference_error::date_key(&d))
            .unwrap_or_default();
        if !self.0.contains_key(&date) {
            let tags = crate::reference_error::OsmTags::load_case(paths, manifest, id)?;
            self.0.insert(date.clone(), tags);
        }
        Ok(&self.0[&date])
    }
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
    v1: &'a V1Ctx,
    /// Retard du baro gelé de la calibration (`perso_v1.retard_baro_gele`, empreinte vérifiée).
    frozen: &'a crate::retard::RetardGele,
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
    let PassCtx { paths, manifest, selection, refs, ouvrages, tiles, groupes, ids, target, v1, frozen } =
        *ctx;
    let rules = BenchRules::default();
    let rules_core = Rules::default();
    let structures_wanted = ids.is_empty();
    let mut items: Vec<Value> = Vec::new();
    let mut cases = Vec::new();
    let mut skipped = Vec::new();
    // Instantanés des tags OSM des références Valhalla (README §4 bis.8), un par date de recalage,
    // lus au premier cas qui en a besoin : chaque cas prend celui de la date de son recalage.
    let mut osm_tags = SnapshotCache::default();
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
        let temoins = v1.temoins_on(&t.id);
        for mut case in loaded {
            if !judged_whole(manifest, target, &t.id) {
                add_implicit_masks(&mut case, refs, v1, target, &t.id);
            }
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
            // R3 contre Valhalla : plages d'erreur de la référence hors décompte (README §4 bis.8),
            // instantané des tags OSM lu une fois, empreinte vérifiée.
            let csv = match csv {
                Some(text) => {
                    let tags = osm_tags.for_case(paths, manifest, &t.id)?;
                    let cut = crate::reference_error::error_ranges(&text, tags);
                    Some(crate::reference_error::exclude(read_stadia_surface(&text), &cut))
                }
                None => None,
            };
            // Trace de la série mesurée, cas entier : au `judge`, une trace de la série tenue à
            // l'écart l'est aussi, son δ estimé normalement (README §4 ter.9, 7dab276).
            let eval = OuvrageEval::whole(&paths.cache);
            match measure_case(service, &case, csv, refs, ouvrages, &rules, &rules_core, &eval, &temoins)
                .await
            {
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
    // Traces V1, V1.1 et V1.2 tenues à l'écart : jamais ouvertes en calibration (seul `judge` les lit),
    // leurs ouvrages de calibration perdent ces passages.
    for t in extract_sources(manifest, selection, target).into_iter().filter(|_| paths.private_present()) {
        let Ok(loaded) = load_perso(paths, t, &rules) else {
            skipped.push(json!({"id": t.id, "raison": "extraits tenus à l'écart : trace illisible"}));
            continue;
        };
        // Aucun passage perdu en silence (`held_out_losses`) : élément retrouvé sans fenêtre,
        // témoin incomplet, fenêtre sans extrait → cas sauté ; élément qu'aucune station
        // n'approche (non retrouvé, comme en calibration) → rapporté.
        let losses = held_out_losses(&t.passages_masques, &loaded, refs, &rules);
        for reason in losses.skipped {
            skipped.push(json!({"id": t.id, "raison": format!("extraits tenus à l'écart : {reason}")}));
        }
        if !losses.not_found.is_empty() {
            held_out_measured.push(json!({"id": t.id, "passage_non_retrouve": losses.not_found}));
        }
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
            let temoins: Vec<crate::v1::TemoinV1> =
                v1.temoins_on(&t.id).into_iter().filter(|x| t.passages_masques.contains(&x.id)).collect();
            let measured = measure_held_out(
                service,
                t,
                &case,
                refs,
                ouvrages,
                &rules,
                &rules_core,
                &paths.cache,
                &temoins,
                frozen,
            )
            .await;
            // Aucune sortie du service hors des fenêtres : bilans des extraits jamais rapportés.
            service.take_stats();
            match measured {
                Ok((its, extraits)) => {
                    eprintln!("{} : extraits mesurés ({} mesures d'ouvrage)", case.id, its.len());
                    held_out_measured
                        .push(json!({"id": case.id, "mesures": its.len(), "extraits": extraits}));
                    items.extend(its);
                }
                Err(_) => skipped
                    .push(json!({"id": case.id, "raison": "extraits tenus à l'écart : échec du service"})),
            }
        }
    }
    // Itinéraires planifiés E4 / S2 (README §4 ter.4) : GPX tel quel rééchantillonné à 25 m, sans
    // baro ; éléments de l'autre série masqués ; ouvrages, croisements et témoins de la série.
    let mut planned_cases = Vec::new();
    for (pl, _) in v1.planned.cases.iter().filter(|(pl, _)| pl.role != "R1_R2") {
        let mut case = match crate::v1::load_planned(paths, pl, target) {
            Ok(c) => c,
            Err(reason) => {
                skipped.push(json!({"id": pl.id, "raison": reason}));
                continue;
            }
        };
        if !judged_whole(manifest, target, &pl.id) {
            add_implicit_masks(&mut case, refs, v1, target, &pl.id);
        }
        let temoins = v1.temoins_on(&pl.id);
        let eval = OuvrageEval::whole(&paths.cache);
        match measure_case(service, &case, None, refs, ouvrages, &rules, &rules_core, &eval, &temoins).await {
            Ok((mut c, dv, its)) => {
                eprintln!("{} : mesuré ({})", case.id, service.mode().as_str());
                c["mode"] = json!(service.mode().as_str());
                if let Some(m) = c.as_object_mut() {
                    m.remove("metriques");
                    m.remove("retard_baro");
                    m.remove("zones_pente_baro");
                }
                if let Some(d) = c["diagnostics"].as_object_mut() {
                    d.retain(|k, _| {
                        !k.starts_with("E1.") && k != "stations_comparees" && k != "part_comparee"
                    });
                }
                c["role"] = json!(pl.role);
                planned_cases.push(c);
                items.extend(its);
                if data_versions.is_null() {
                    data_versions = dv;
                }
            }
            Err(e) => skipped.push(json!({"id": case.id, "raison": e.to_string()})),
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
    // Parcours planifiés R1/R2 (README §4 ter.4) : comme les parcours d'osm-surface, référence
    // Valhalla FOSSGIS au format osm-surface, entrée rééchantillonnée à 25 m.
    for (pl, _) in v1.planned.cases.iter().filter(|(pl, _)| pl.role == "R1_R2") {
        let tags = osm_tags.for_case(paths, manifest, &pl.id)?;
        let mut route = match crate::osm_surface::load_planned(paths, pl) {
            Ok(r) => r,
            Err(e) => {
                skipped.push(json!({"id": pl.id, "raison": e.to_string()}));
                continue;
            }
        };
        // Voies aux tags différents entre la date des tuiles et celle de la référence (README §4
        // quater.1) : avant l'erreur de la référence. Instantanés absents du manifest : règle non
        // appliquée en calibration (rapporté), refus au jugement.
        let between = match manifest.recalage_section(&pl.id) {
            Some(sec) => crate::reference_error::TagsBetweenDates::load_for(paths, manifest, sec)?,
            None => None,
        };
        match &between {
            Some(t) => crate::osm_surface::exclude_tags_differents(paths, pl, &mut route, t)?,
            None if target == Serie::TenueALEcart => bail!(
                "{} : tags différents entre dates (README §4 quater.1) : instantanés osm_tags_toutes \
                 absents du manifest, jugement refusé",
                pl.id
            ),
            None => {}
        }
        // Empreintes de surface.csv et baro.csv, cohérence avec la référence : refus de la campagne.
        crate::osm_surface::exclude_reference_errors(paths, pl, &mut route, tags)?;
        match crate::osm_surface::measure(service, &route).await {
            Ok(mut c) => {
                eprintln!("{} : mesuré", pl.id);
                if between.is_none() {
                    c["typologie"]["R1.tags_differents"] = json!("instantanes_absents_du_manifest");
                }
                c["serie"] = json!(target);
                c["groupe"] = json!("planifie_R1_R2");
                os_cases.push(c);
            }
            Err(e) => skipped.push(json!({"id": pl.id, "raison": e.to_string()})),
        }
    }
    // Témoins V1 de la série mesurée sans mesure S2 (extrémités non retrouvées, parcours non
    // mesuré) : sautés, rapportés.
    for t in &v1.temoins {
        if !items.iter().any(|i| i["metrique"] == "S2" && i["temoin"] == t.id.as_str()) {
            skipped.push(
                json!({"id": t.id, "raison": "témoin V1 : extrémités non retrouvées sur un cas mesuré"}),
            );
        }
    }
    // Ouvrages : parcours `test1..4` de vcyclist et témoins (toujours calibration : « déjà vu »).
    if structures_wanted && target == Serie::Calibration {
        items.extend(
            crate::ouvrages::vc_items(service, &paths.bench, &paths.cache, tiles, ouvrages, &mut skipped)
                .await?,
        );
    }
    items.extend(crate::ouvrages::perimeter_items(ouvrages));
    Ok(Pass {
        cases,
        items,
        skipped,
        data_versions,
        inserted_report,
        held_out_measured,
        os_cases,
        warm,
        planned: planned_cases,
    })
}

/// Verdicts d'une passe : `(cas, ouvrages publics, agrégats, verdicts)`.
fn judge_pass(
    pass: Pass,
    limits: &[(String, String)],
    info: &crate::coverage::OuvrageInfo,
    q1: usize,
    frozen: Option<&crate::retard::RetardGele>,
) -> (Vec<Value>, Vec<Value>, Value, Value, Pass) {
    let Pass {
        mut cases,
        mut items,
        skipped,
        data_versions,
        inserted_report,
        held_out_measured,
        os_cases,
        warm,
        planned,
    } = pass;
    crate::ouvrages::annotate_data_limits(&mut items, limits);
    let (ouvrages_agg, ouvrages_verdicts) = crate::ouvrages::judge(&items);
    let (agregats, mut verdicts) = judge(&cases);
    // Couverture après exclusions (README §4 ter.4) : jugée au jugement seulement.
    let routes: Vec<Value> = os_cases.iter().filter(|c| c["groupe"] == "planifie_R1_R2").cloned().collect();
    let couverture =
        crate::coverage::measure(&cases, &routes, &items, info, q1, BenchRules::default().grid_step_m);
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
    agregats["couverture"] = couverture;
    // Retard du baro des traces perso (README §4 ter.9) : id opaque et δ seulement.
    agregats["retard_baro"] = crate::retard::summary(&cases);
    if let Some(frozen) = frozen {
        agregats["retard_baro"]["controle_gele"] = frozen.control(&cases);
    }
    agregats["planifies"] = json!(planned
        .iter()
        .map(|c| json!({"id": c["id"], "role": c["role"], "M1.non_apparie": c["typologie"]["M1.non_apparie"],
            "M1.unknown": c["typologie"]["M1.unknown"], "M1.hors_graphe_velo_controle": c["typologie"]["M1.hors_graphe_velo_controle"]}))
        .collect::<Vec<_>>());
    for (k, v) in ouvrages_verdicts.as_object().into_iter().flatten() {
        verdicts[k] = v.clone();
    }
    cases.extend(os_cases);
    cases.extend(planned);
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
    let report = crate::check::check_series(paths, target)?;
    ensure_check(&report)?;
    let manifest = Manifest::load(&paths.bench)?;
    let selection = select_series(&manifest, groupes, ids, target);
    let v1 = v1_ctx(paths, &manifest, ids, target)?;
    eprintln!(
        "cas retenus : {} traces, {} itinéraires ; exclus (autre série) avant toute lecture : {} traces, {} itinéraires",
        selection.cases.len(),
        v1.planned.cases.len(),
        selection.excluded_held_out.len(),
        v1.planned.excluded.len()
    );
    let rules = BenchRules::default();
    let rules_core = Rules::default();
    let refs = ouvrage_refs(paths)?;
    let mut ouvrages = crate::ouvrages::series_ouvrages(paths, &manifest, target)?;
    let info: crate::coverage::OuvrageInfo =
        ouvrages.iter().map(|o| (o.id.clone(), (o.length_m, o.mnh_milieu))).collect();
    let q1 = if target == Serie::Calibration { &report.q1 } else { &report.q1_tenue };
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
    // Retard du baro gelé (README §4 ter.9, 48ca44a) : empreinte vérifiée, fichier modifié refusé ;
    // lu pour les extraits des traces de calibration au jugement, contrôlé en calibration.
    let frozen =
        crate::retard::RetardGele::load(paths, manifest.v1.as_ref().and_then(|v| v.retard_gele.as_ref()))?;
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
        v1: &v1,
        frozen: &frozen,
    };
    // Deux passes (README §2.2, étape 2) : `distance` au pas de la grille, puis le mode par défaut
    // `input_with_breakpoints`, rééchantillonné sur la même grille (étape 3).
    let reference = measure_pass(&service, &ctx).await?;
    let breakpoints = measure_pass(&service.with_mode(BenchMode::Breakpoints), &ctx).await?;
    // Calibration : δ calculé de chaque cas contre le fichier gelé (contrôle, sans effet).
    let control = (target == Serie::Calibration).then_some(&frozen);
    let (cases, ouvrages_public, agregats, mut verdicts, rest) =
        judge_pass(reference, &limits, &info, q1.negatives, control);
    let (bp_cases, bp_ouvrages, bp_agregats, mut bp_verdicts, bp_rest) =
        judge_pass(breakpoints, &limits, &info, q1.negatives, control);
    let Pass { skipped, data_versions, inserted_report, held_out_measured, .. } = rest;
    let bp_inserted: Vec<&Value> = bp_cases.iter().filter_map(|c| c.get("points_inseres")).collect();
    let max_per_km = bp_inserted.iter().filter_map(|v| v["max_sur_un_km"].as_u64()).max().unwrap_or(0);
    let (bp_len, bp_ins) = bp_inserted.iter().fold((0.0, 0u64), |(l, n), v| {
        (l + v["longueur_km"].as_f64().unwrap_or(0.0), n + v["points_inseres"].as_u64().unwrap_or(0))
    });
    verdicts["Q1"] = json!(match (q1.negatives, q1.pass()) {
        (0, _) => "skipped",
        (_, true) => "pass",
        _ => "fail",
    });
    bp_verdicts["Q1"] = verdicts["Q1"].clone();
    let fingerprint = cache_fingerprint(&paths.cache).context("empreinte du cache de banc")?;
    let rules_fingerprint = sha256_bytes(serde_json::to_string(&rules_core)?.as_bytes());
    let bench_rules_sha256 = sha256_bytes(serde_json::to_string(&rules)?.as_bytes());
    // Traces de l'autre série : exclues (et lues en extraits autour des ouvrages mesurés).
    // Incrément : V1 en calibration ; au jugement, la série nouvelle tenue à l'écart (V1.2).
    let increment = match target {
        Serie::Calibration => "V1",
        Serie::TenueALEcart => {
            manifest.named_sections().filter(|(_, v)| !v.judged).map(|(n, _)| n).last().unwrap_or("V1")
        }
    };
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
            "increment": increment,
            "increment_plan": "V1",
            "modes": ["distance", "input_with_breakpoints"],
            "regles_banc": "README §4 bis (décision de l'utilisateur, 2026-10-31 ; complétée en 40a989f, 657d2c8, 022710f, 4304838, e48170f, adbf6f1, 53225a8 et 405f956 ; décision de l'utilisateur, 2026-11-01 : 2f3254b, 02874b5 et 01272dc) ; README §4 ter, règles V1 (décision de l'utilisateur, 2026-11-01 : 304f767, 16f6097 et c1d231c ; 6c664b6 non appliqué en calibration ; après la calibration 2026.11.2 : ec72f13, 09dcda0 et 0b590ff ; ouvrage réel sur la voie d'un témoin : 32f6867, précisé en eb4c4cc, covered mesuré contre le MNT : cc59b18) ; série V1 (README §4 ter.4 : 13e0ed1, 930f6e8, 305abd7, 64b7067, 00e500f ; manifest perso_v1 e4af0f7, 71781e7 ; recalage Valhalla FOSSGIS 6aba29e) ; E5 au croisement (README §4 ter.7 : 800d4e0 ; après la calibration 2026.11.6 : f1bfda3, e477dd4, 504c800, e9430ac), E1 plaine incertaine (§4 ter.8 : 4423741), ouvrages réels des témoins V1 (§4 ter.6 : 57e9d95 ; manifest temoins_v1_tags 6ed1da7) ; après la calibration 2026.11.6-bench1 (décisions de l'utilisateur, 2026-11-02) : E5 tronçons recousus, bosse de 1,0 m, côté sans baro sans repli (1477b5f), retard du baro estimé et corrigé (§4 ter.9 : b922ce4), points de baro d'E5 comptés bruts (6bc77a5) ; après la calibration 2026.11.6-bench2 : points de baro d'E5 à l'abscisse corrigée du retard (décision de l'utilisateur, 2026-11-02 : 085d26e), lectures du retard et d'E5 (7dab276) ; après la calibration 2026.11.6-bench3 : extraits des traces de calibration au jugement au δ gelé de leur trace, sans réestimation (décision de l'utilisateur, 2026-11-02 : 30e8024 ; data/retard-baro-v1.json, manifest perso_v1.retard_baro_gele : 48ca44a ; lectures des extraits et des fenêtres masquées d'E5 : 87b0ea8) ; après le jugement V1 (décisions de l'utilisateur, 2026-11-02) : E5, approche sur un autre ouvrage (§4 ter.7 : f38b745, validé en 2e511e5), erreur de la référence Valhalla de R1, R2 et R3 hors décompte (§4 bis.8 : 562d729) ; README §4 quater, règles pour V1.2 (décisions de l'utilisateur, 2026-11-03, après le jugement V1.1) : référence R1/R2, tags différents entre dates et sauts de s_origine (§4 quater.1 : d6fd42c), E5 baro bruité (§4 quater.2 : f8f81a6), E1 zones de pente du baro avant la marche (§4 quater.3 : 2744983, 633255f), E5 par site (§4 quater.4 : 8559a21)",
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
            "itineraires_exclus": v1.planned.excluded,
            "q1": q1,
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
                "S3 : stn-acces-nord, balcons de test4 et balcons entre tunnels d'une série, contre vcyclist S ; V1 (README §3) : detecte = station du milieu typée pont ou tunnel par le service (stn-acces-nord), part_typee_ouvrage = part des stations de l'emprise (balcons : stations S) typées pont ou tunnel, rapportées sans seuil",
                "E5 : |service − référence| au croisement des passages dessous (perso, itinéraires) ; max sur les stations O des témoins négatifs. Référence (README §4 ter.7 : 800d4e0, f1bfda3, e477dd4, 504c800, e9430ac), sans le service, dans l'ordre : retiré (e5_trace_sur_ouvrage, agregats.ouvrages.E5.trace_sur_ouvrage) si une ligne bridge ou tunnel de la couche streets (tuile du point de croisement et ses 8 voisines, toutes classes) passe à ≤ 10 m du point de croisement en faisant < 20° avec la trace (direction = stations à ± 10 m du croisement) ; sinon baro corrigé localement (baro corrigé de son retard, README §4 ter.9, au croisement comme sur les côtés ; baro absent au croisement : MNT, 7dab276) si la trace est perso au verdict fiable (qualify_baro.py ; segments .sN = cas distincts, chacun avec son baro ; tronçons .partN recousus par abscisse, README §2.2) et que son baro existe à la station du croisement (interpolation sur la grille de 5 m sans pas brut > 100 m) : baro − médiane (baro − MNT) des stations à 200–1 500 m de part et d'autre réunies, tolérance 2 m ; incertaine (e5_reference_incertaine, agregats.ouvrages.E5.references_incertaines, non comptée, écart indicatif, jamais de repli sur le MNT) si un côté compte moins de 3 points bruts de baro.csv entre 200 et 1 500 m, hors points insérés écartés et fenêtres masquées, à leur abscisse corrigée du retard s_p − δ, fenêtres masquées lues à cette abscisse (points_baro_insuffisants ; 6bc77a5 : pas les stations de la grille ; 085d26e : abscisse corrigée), si un côté a ses 3 points mais aucune station avec baro et MNT (decalage_d_un_cote_absent, sans repli ; 7dab276) ou si les médianes (baro − MNT) des deux côtés pris à part diffèrent de plus de 2 m (decalages_des_cotes) ; décalages par côté rapportés (decalages_cotes_m, points_baro_cotes) ; sinon MNT LiDAR (sortie sans correction du mode distance), tolérance 1 m, indisponible (e5_reference_indisponible) si une station à ≤ 10 m est sans MNT ou hors IGN LiDAR HD (mnt_sans_donnee), si la bosse MNT(croisement) − moyenne des médianes du MNT à 20–50 m de part et d'autre le long de la trace dépasse 2,0 m (mnt_lit_le_tablier), ou si un côté n'a pas de station à 20–50 m (approche_sans_mnt) ; incertaine (approche_sur_ouvrage, testée après le MNT sans donnée et avant la bosse ; f38b745, 2e511e5) si une station d'une approche 20–50 m (depuis les bouts de la plage O pour les témoins) est à ≤ 10 m d'une ligne bridge ou tunnel de la couche streets (tuile de la station et ses 8 voisines, toutes classes) qui fait < 20° avec la trace (direction = stations à ± 10 m), hors voies de l'ouvrage du dessus (lignes bridge ou tunnel à ≤ 10 m d'une station de la plage ou du croisement et à ≥ 20° de la trace, et les morceaux du même élément qui les prolongent d'une tuile à l'autre) ; incertaine (bosse_mnt) si la bosse est entre 1,0 et 2,0 m inclus ; témoins négatifs vcyclist : croisement = station du milieu de la plage O, référence MNT, approches 20–50 m mesurées depuis les bouts de la plage, écart = max sur la plage ; bosse_mnt_m, ecart_mnt_m et seuil_m rapportés",
                "Retard du baro (README §4 ter.9, b922ce4 ; criteria preparation.retard_baro) : par cas (tronçons .partN recousus, segments .sN chacun), δ de −50 à +200 m par pas de 5 m minimisant 1,4826 × MAD du résidu baro(s + δ) − MNT(s) sur la grille de 5 m, MNT = sortie sans correction du mode distance (IGN LiDAR HD, Mapterhorn hors IGN ; identique dans les deux modes), hors points insérés écartés, fenêtres masquées et stations sans baro (pas brut > 100 m), dérive retirée par médiane glissante de 5 km (pas de calcul 50 m, comme E1) ; à écart égal, le plus petit |δ| ; baro corrigé(s) = baro(s + δ) pour E1, E3 (décalage d'approche, milieu, max, test de niveau, repli 1–3 km), hors_graphe_velo et E5 ; E2, E2b, stations_comparees et couverture au baro brut ; non estimable (moins de 400 stations à δ = 0 : stations_insuffisantes ; minimum à −50 ou +200 m : minimum_a_une_borne ; extrait d'une trace de l'autre série, plus court que la fenêtre de la dérive : extrait_tenu_a_l_ecart) : baro brut, mention retard_non_estimable ; au jugement, traces de la série tenue à l'écart lues en cas entiers, δ estimé normalement (7dab276) ; au jugement, extraits des traces de calibration au δ gelé de leur cas (data/retard-baro-v1.json, cas.<id>.retard_m, manifest perso_v1.retard_baro_gele, empreinte vérifiée, fichier modifié refusé ; 30e8024, 48ca44a, 87b0ea8), sans réestimation : baro(s + δ) dans l'abscisse de la trace, avant tout décalage, pour le repli 1–3 km et toutes les références d'ouvrage de l'extrait ; cas absent du fichier ou retard_m null : baro brut, retard_non_estimable ; source et valeur rapportées par extrait (extraits_calibration[].extraits[].retard_baro) ; en calibration, δ calculé de chaque cas contrôlé contre le fichier gelé (agregats.retard_baro.controle_gele, sans effet) ; cas.retard_baro (retard_m, minimum_m, ecart_robuste_0_m, ecart_robuste_min_m, stations) et agregats.retard_baro",
                "E1 plaine (README §4 ter.8, 4423741) : trace plaine dont le bruit baro de qualify_baro.py (noise_m du manifest) dépasse 1,0 m = référence incertaine, rapportée (verdicts.E1_references_incertaines, avec un verdict indicatif), non jugée et hors couverture E1 ; dplus et E2b inchangés",
                "E1, marche du baro (README §4 ter.11, 3f8a810), toutes strates et séries : résidu r(s) = baro(s + δ) − MNT(s) sur la grille de 5 m (δ de la trace, MNT = sortie sans correction du mode distance), stations sur un ouvrage (segment pont ou tunnel, altitude d'ouvrage), masquées ou sans baro exclues en s comme en s + δ ; Δ court = médiane de r sur [s + 100, s + 300] − médiane sur [s − 300, s − 100], Δ long sur [s + 100, s + 2 100] et [s − 2 100, s − 100] bornées à la trace, au moins 20 stations par fenêtre ; marche si |Δ court| > 10 m et |Δ long| > 10 m de même signe et amplitude du MNT ≤ 30 m sur [s − 300, s + 300] ; plus grande |Δ court| rapportée (cas.marche_baro : abscisse_km, delta_court_m, delta_long_m, amplitude_mnt_m) ; référence E1 incertaine, rapportée dans verdicts.E1_references_incertaines avec un verdict indicatif, non jugée, hors couverture E1 ; E2 et E2b inchangés",
                "Témoins S2 V1, ouvrages réels sur leur voie (README §4 ter.6, 57e9d95) : private/v1/temoins-v1-tags.json (perso_v1.temoins_v1_tags du manifest, 6ed1da7, empreinte vérifiée ; entrée absente du manifest : aucune plage), entrées de la série mesurée seulement ; extrémités de chaque plage projetées sur la station la plus proche du tronçon du témoin (≤ 40 m, sinon non_retrouvee) ; plage ± 35 m retirée des ouvrages fictifs, de la longueur typée et de la longueur totale (ouvrage_reel_masque_m), rapportée (S2_ouvrage_reel, longueur et part du témoin lues dans le fichier ; avalanche_protector et covered : limite_donnees, erreur d'altitude non mesurée) ; témoin sorti (S2_temoin_sorti) si une plage dépasse la moitié de longueur_temoin_m",
                "S1 (README §4 ter.1, V1) : longueur typée = recouvrement continu de l'emprise [s_A, s_B] par les segments bridge/tunnel du service (abscisses from_m/to_m), un segment arrêté à 2,5 m au plus d'une borne, à l'intérieur de l'emprise, prolongé jusqu'à elle ; rappel = typé ÷ (s_B − s_A) ; s_A, s_B = projection continue de A et B sur la trace (polyligne des stations du mode testé) pour les traces perso, abscisses des stations bornes des plages pour les parcours vcyclist (structures.csv bridge/tunnel/missed, témoins D/N, tunnels d'ouvrages.yaml entre portails) ; S2 : témoins négatifs n*, tn*, hn*, longueur typée hors plages X/x ; plages hors graphe vélo du témoin (règle de M1 resserrée, README §4 ter.5, 09dcda0) retirées des ouvrages fictifs, de la longueur typée et de la longueur totale, hors_graphe_velo_m par témoin ; ouvrage réel sur la voie même d'un témoin (README §4 ter.6, 32f6867, précisé en eb4c4cc : suites de stations dont la way, colonne way des fichiers de stations, plages X et O comprises, porte à la base OSM du témoin bridge, tunnel, dont avalanche_protector = galerie, ou covered=yes ; tags lus par Overpass daté dans le cache de banc, temoins-tags/) : plage A–B ± 35 m retirée des ouvrages fictifs, de la longueur typée et de la longueur totale (ouvrage_reel_masque_m), rapportée (S2_ouvrage_reel ; galerie : limite_donnees, mesurée comme un tunnel entre ses portails) ; témoin sorti de S2 (S2_temoin_sorti) si sa catégorie nomme la nature de l'ouvrage (galerie : « galler… », sinon « tunnel », « bridge ») ou si un ouvrage couvre à lui seul plus de la moitié de sa longueur ; objet = catégorie vcyclist nommant la nature d'un ouvrage emprunté (galler…, tunnel, bridge, viaduc…, sans under/above/beside/over avant le nom) ; covered : limite_donnees (shortbread sans covered), non mesuré ; tags absents du cache : S2 du témoin sauté ; p06-normandie exclu (ouvrage tenu à l'écart)",
                "Traces perso tenues à l'écart (README §2) : seuls des extraits sont envoyés, fenêtre de ± 500 m autour de chaque ouvrage de calibration franchi (passages_masques) ± 1 km de marge ; service, MNT et baro lus dans la fenêtre seulement ; mesures d'ouvrage des seuls ouvrages de calibration, sans distance curviligne (extrait_tenu_a_l_ecart) ; aucune métrique de trace, aucune sortie hors fenêtre",
                "Passage en hauteur sur un pont (README §2.1, passage_pont.niveau) : MNH au milieu du corpus (médiane MNS 25 m − MNT LiDAR) ≥ 10 m : passage compté seulement si la médiane (baro corrigé − MNT) à ± 12,5 m du milieu (≥ 3 stations) atteint ½ MNH, sinon rapporté en passage_retire_niveau et retiré de E3, S1 et hors_graphe_velo ; MNH < 10 m : niveau_non_discriminant ; moins de 3 stations : niveau_sans_baro",
                "Extraits tenus à l'écart : repli 1–3 km du décalage local lu sur la trace (baro − MNT IGN LiDAR lu directement, hors ponts et tunnels des tuiles), jamais la sortie du service ; seul le décalage est écrit ; decalage_repli_stations_par_cote = stations d'approche avant / après le pont",
                "Attribution d'un passage (README §4 ter.2, V1) : trace brute = private/traces/<trace>.trim1km.gpx (points avec altitude, segments coupés aux sauts > 2 km et gardés dès 5 km, comme tools/stadia_match.py), points dont l'abscisse d'origine est entre celles de s_A et s_B (correspondance s_recale_m → s_origine_m de <id>.baro.csv, interpolée) ; médiane des distances de ces points à la voie de l'ouvrage (élément de la tuile qui suit le mieux la géométrie du corpus, §4 bis.1) et à chaque autre élément bridge/tunnel des tuiles de la campagne (tuile de chaque point, éléments = lignes d'attributs identiques) ; voies du même pont réunies à la voie de l'ouvrage (« un tablier, un ouvrage », 0b590ff, ponts seulement : points les plus proches du milieu dans un même polygone de la couche bridges des tuiles, morceaux recouvrants réunis ; deux polygones = deux ponts ; sinon, si l'une est hors de tout polygone, concurrente à ≤ 10 m de la voie de l'ouvrage sur 21 points de A à B ; médiane de l'ouvrage = la plus petite de ses voies) ; voie de l'ouvrage pas strictement la plus proche : passage_sur_ouvrage_parallele, rapporté, compté ni dans E3, E4, S1 ni dans hors_graphe_velo ; sans point brut ou sans voie de l'ouvrage dans ces tuiles : attribution_indeterminee, passage compté ; E5 non concerné",
                "Passages d'ouvrage (traces perso) : un passage par traversée du disque de 40 m autour du milieu (suite de stations contiguës), A et B à moins de 40 m cherchés sur cette traversée seulement ; E3 sans référence (pas de baro transporté sur l'emprise) : écarts null avec raison_ecart, hors de la médiane du pont",
                "R1/R2/R3 : classe Stadia ramenée à paved/unpaved/unknown (revêtu = paved_smooth, paved, paved_rough ; non revêtu = pire que paved_rough) ; R1 total et hors inconnu Stadia, R2 sur les mètres classables des deux côtés",
                "M1 : non apparié = longueur matched=false ; unknown = longueur surface.class=unknown ; parts de la longueur du service ; traces recalées (README §4 bis.6, 2026-11-01) : plages dont aucune station (pas de 5 m de la grille, sur la géométrie recalée) n'a de ligne de la couche streets à ≤ 30 m (tuile z14 et 8 voisines, cache de la campagne ; tuile absente à la source = sans voie ; tuile absente du cache : cas sauté), élargies d'un demi-pas de chaque côté, sans longueur minimale, retirées de M1 (non apparié, longueur totale, unknown) et rapportées à part (M1.hors_reseau_m par cas, agrégat typologie M1.hors_reseau_m) ; lues sur la trace et les tuiles seulement ; M1 hors graphe vélo (README §4 ter.3, V1, resserrée en ec72f13) : stations (même grille) dont la ligne la plus proche de la couche streets à ≤ 30 m (rails exclus) est une route motorisée fermée au vélo (motorway, trunk, route bicycle=no ; bicycle=* prime ; busway exclue), plages continues élargies d'un demi-pas et gardées à partir de 50 m, retirées de M1 et rapportées (M1.hors_graphe_velo_m par cas, agrégat typologie M1.hors_graphe_velo) ; contrôle M1.hors_graphe_velo_controle (62bd46e) : à chaque station, segment du service apparié à une voie du graphe vélo, distincte de la route fermée exclue du graphe (appariees_voie_parallele), ou matched = false sous un avertissement unmatched_range ; une station non appariée sans unmatched_range = échec de M1",
                "Erreur de la référence (README §4 bis.8, 562d729) : sur R1/R2 des parcours planifiés et R3 des traces perso (référence Valhalla trace_attributes ; jamais osm-surface), plage de <id>.surface.csv où Valhalla rend compacted, dirt, gravel, path ou impassable et dont la voie porte, dans l'instantané OSM à la date des données du serveur (perso_v1.osm_tags_reference, empreinte vérifiée, sans réseau), surface = asphalt, concrete, concrete:lanes, concrete:plates ou paving_stones : retirée de la référence (ni accord, ni désaccord, ni longueur comparée), rapportée à part (typologie.<R1|R3>.erreur_reference_m et _part, agregats.typologie.erreur_reference) ; voie absente de l'instantané : pas d'exclusion ; parcours planifiés : plages ramenées à l'abscisse de l'itinéraire par <id>.baro.csv, contrôlées sur les plages non revêtues de la référence empreintée",
                "R3 : accord entre deux dérivés d'OSM (VersaTiles vs Stadia), pas une vérité terrain ; mesuré aux mêmes seuils, non bloquant (README §4 bis.7, 2026-11-01) : un échec est rapporté sans refuser l'incrément",
                "E2b : D+ baro au preset dem (3 m, 30 m ; criteria.yaml preset_baro: dem, README §4 bis.5), E2b.dplus_baro_dem_m et E2b.ratio* par cas ; E2 (E2.dplus_baro_m, E2.ratio) reste en barometric ; non_discriminant quand le service et sa référence structure_correction=false sont identiques sur tous les cas d'un groupe (ni réussite ni échec ; aucune correction d'ouvrage avant l'incrément 4)",
                "Tronçons .partN recollés en un cas (E1 sur la grille entière, D+ du profil recollé) ; segments .sN : un cas chacun",
                "Série V1 (README §4 ter.4) : série tenue à l'écart du MVP jugée (judgements.md), devenue calibration avec ses ouvrages, fenêtres masquées du MVP retirées ; traces perso_v1 selon leur série ; ouvrages et croisements de ouvrages-v1.json, témoins de temoins-v1.json et itinéraires de series-v1.csv (par_cas : cas de la calibration pour ses seuls éléments de calibration) ; index lus, vérifiés par empreinte et l'autre série retirée avant tout calcul ; fichiers des cas de l'autre série jamais ouverts en calibration (traces, recalages, itinéraires, références ; ni extraits des traces V1 tenues à l'écart) ; éléments V1 de l'autre série dont tous les points sont à ≤ 40 m d'un cas masqués comme les passages_masques (masques_v1_implicites) ; fenêtre d'un témoin = tout son tronçon ± 500 m",
                "Série V1.1 (manifest perso_v11, serie_v11 ; README §4 ter.10, 7b1a4e9, 22dbba7), tenue à l'écart entière : en calibration, traces, itinéraires, témoins et extraits V1.1 jamais ouverts (index ouvrages-v11.json, temoins-v11.json, series-v11.csv lus, vérifiés par empreinte, la série retirée avant tout calcul) ; éléments V1.1 (nv-606 et suivants) dont tous les points sont à ≤ 40 m d'une trace ou d'un itinéraire de calibration masqués ; au jugement V1.1, traces et itinéraires V1.1 jugés entiers, sans aucun masque, sans extrait de calibration, E3 et E5 sur les seuls éléments V1.1 ; colonne fenetres_calibration jamais lue ; R1, R2 et R3 : instantané OSM de la date du recalage de chaque cas (perso_v1.osm_tags_reference pour MVP et V1, perso_v11 ou serie_v11.osm_tags_reference pour V1.1), empreinte vérifiée, absent ou faux = refus ; au jugement V1.1, E3 rapporté non bloquant et minimum E2b sur dplus et plaine seulement (criteria serie_tenue_a_l_ecart.serie_v11)",
                "Série V1.2 (manifest perso_v12, serie_v12 ; README §4 quinquies), tenue à l'écart entière, MVP, V1 et V1.1 en calibration (ouvrages complement_v11 nv-840 à nv-947 compris, V1.1 jugée) : en calibration, traces, itinéraires, témoins et extraits V1.2 jamais ouverts (index ouvrages-v12.json, temoins-v12.json, temoins-v12-tags.json, series-v12.csv lus, vérifiés par empreinte, la série retirée avant tout calcul) ; éléments V1.2 (nv-948 et suivants) dont tous les points sont à ≤ 40 m d'une trace ou d'un itinéraire de calibration masqués ; au jugement V1.2, traces et itinéraires V1.2 jugés entiers, sans aucun masque, sans extrait de calibration, E3 et E5 sur les seuls éléments V1.2 ; colonne fenetres_calibration jamais lue ; instantanés de la section (recalage et osm_tags_reference de serie_v12, osm_tags_toutes, campagne tuiles_2026_11_03) ; au jugement V1.2, Q1 rapporté non bloquant, E3 bloquant si la couverture comptée avant toute mesure (manifest serie_v12.couverture.E3, couverture_v1.py) atteint le minimum de jugement, sinon rapporté sans refus (criteria serie_tenue_a_l_ecart.serie_v12 : bloquant si_couverture_atteinte)",
                "Itinéraires planifiés (README §4 ter.4) : GPX tel quel, sans baro ni recalage, rééchantillonné à 25 m ; E4 / S2 mesurés comme une trace sans baro (pas d'E1, E2, E2b ; M1 rapporté dans agregats.planifies, non jugé) ; trace brute de l'attribution = l'itinéraire ; R1/R2 comme les parcours d'osm-surface (référence Valhalla FOSSGIS au format osm-surface, ligne rééchantillonnée à 25 m), M1 jugé comme pour osm-surface",
                "Croisements E5 V1 (point de croisement seul, voie du dessous lue dans les tuiles : périmètre et voie = voie au sol la plus proche, §4 bis.1) : passage = trace à ≤ 40 m du croisement ; compté seulement si, à ± 25 m du croisement, la trace suit la voie du dessous au moins aussi bien que tout pont ou tunnel des tuiles (même lecture que l'attribution, §4 ter.2), sinon E5_passage_dessus, rapporté (agregats.ouvrages.E5.passages_dessus), non compté ; voie ou tuile absente : attribution_indeterminee, compté",
                "S2 des témoins V1 (README §4 ter.4–6, lecture du banc) : tronçon entre les stations les plus proches (≤ 40 m) de debut et fin, sur une sortie comme sur un itinéraire planifié, ou itinéraire planifié entier pour un témoin sans extrémités (README §4 ter.6, affc041) ; retirés de la longueur, des ouvrages fictifs et de la longueur typée : plages hors graphe vélo (§4 ter.5) et abords à 100 m de tout pont ou tunnel des tuiles de la campagne (ouvrage réel, jamais fictif) ; ouvrage fictif = suite de stations typées pont ou tunnel par le service hors de ces plages ; covered et avalanche_protector (absents du schéma des tuiles) non lus sur ces témoins ; témoin non retrouvé : cas_sautes",
                "S2_ouvrage_reel covered=yes (README §4 ter.6, cc59b18) : ecart_max_m = max |service − MNT| aux stations de la plage (reference mnt_aux_stations), rapporté sans seuil dans agregats.ouvrages.limite_donnees",
                "couverture (par mode) : comptes après exclusions du banc par critère à minimum de jugement (criteria.yaml serie_tenue_a_l_ecart.couverture.minimums) ; informative en calibration ; au jugement V1, un critère sous son minimum prend le verdict sous_minimum (refus, README §2 point 3)",
                "E1, zones de pente du baro sans relief (README §4 quater.3, 2744983, 633255f), toutes strates : sur la grille de 5 m, baro corrigé du δ et MNT du mode distance sans correction, là où |baro(s + 50) − baro(s)| / 50 > 0,15 et |MNT(s + 50) − MNT(s)| / 50 < 0,10 (deux stations avec baro et MNT, hors ouvrage et fenêtre masquée), stations de [s − 300, s + 350] masquées pour E1 (décalage, tendance, RMSE, P95, km comparés de la couverture), zones réunies, bornées à la trace ; ordre δ, zones, marche (détectée hors zones, stations exclues en s comme en s + δ), E1 ; cas.zones_pente_baro (zones, km_masques, abscisses_km, stations_comparees_masquees) ; E2, E2b, E3, E5 inchangés",
                "E5 baro bruité (README §4 quater.2, f8f81a6) : bruit baro de la trace (noise_m de qualify_baro.py, trace entière pour ses segments .sN) > 1,0 m, tous groupes : référence baro corrigé local incertaine (e5_reference_incertaine, raison baro_bruite), non comptée, sans repli sur le MNT ; testée après le retrait, avant les côtés",
                "E5 par site (README §4 quater.4, 8559a21) : passages E5 et e5_* (comptés ou non) réunis en sites, deux ids du même site s'ils sont le même ouvrage ou croisés par un même cas à 40 m au plus l'un de l'autre (d_milieu_m), de proche en proche, toutes traces ; site en échec si un passage compté échoue, réussi si tous ses passages comptés réussissent, sans passage compté non compté (marques) ; verdicts.E5 et E5_echecs par site (ids joints par +), agregats.ouvrages.E5.par_site (ouvrages, cas, passages, passages_comptes, ecart_max_m) ; couverture.E5.croisements = sites avec un passage compté",
                "R1/R2 des parcours planifiés (README §4 quater.1, d6fd42c) : voies de <id>.surface.csv (toutes classes, hors discontinuités) dont highway, surface ou tracktype diffèrent entre l'instantané à la date OSM des tuiles (tuiles_*.date_osm) et celui de la date des données du serveur Valhalla, ou présentes à une seule date, retirées de R1 et R2 avant l'erreur de la référence (qui ne les voit plus) et rapportées par parcours (typologie R1.tags_differents : longueur_m, part de la référence classée) ; instantanés osm_tags_toutes de la section du manifest (voies_retenues: toutes, empreinte vérifiée) ; absents du manifest : non appliqué en calibration (R1.tags_differents = instantanes_absents_du_manifest), refus au jugement ; plages ramenées à l'itinéraire coupées aux sauts de s_origine (Δs_origine − Δs_recale > 100 m)",
                "fill-cache : campagne de tuiles datée du manifest (tuiles_*, espace de cache des tuiles de rues) verrouillée : tuiles de rues lues dans le cache seulement, sans revalidation ni téléchargement ; une tuile manquante refuse le remplissage",
                "Q1 : négatifs de la série mesurée ; négatif d'une série nouvelle compté seulement si son défaut est attesté (D+ enregistré < 0,6 ou > 1,6 × D+ MNT, 930f6e8, 305abd7) ; campagne.q1"
            ]
        },
        "cas": cases,
        "ouvrages": ouvrages_public,
        "cas_sautes": skipped,
        "couverture": agregats["couverture"].clone(),
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
            "couverture": bp_agregats["couverture"].clone(),
            "agregats": bp_agregats,
            "verdicts": bp_verdicts
        },
        "diagnostics": {"cache": {"tuiles": fingerprint.tiles, "octets": fingerprint.bytes}}
    });
    crate::privacy::assert_report_clean(&results, &crate::privacy::private_names(paths))?;
    Ok(results)
}

/// Témoins négatifs vcyclist (S2).
pub fn negative_controls(bench: &std::path::Path) -> anyhow::Result<Vec<String>> {
    Ok(crate::ouvrages::controls(bench)?
        .into_iter()
        .filter(|c| c.2.contains("negative"))
        .map(|c| c.1)
        .collect())
}

/// `fill-cache` : mêmes requêtes que `run`, en ligne, plus les cohérences `test1..4` (service
/// complet ; `test4` aussi en Mapterhorn seul, deux conventions), les fixtures hors git
/// (Mapterhorn seul, test de méthode), les scénarios golden et des GPX supplémentaires. Écrit
/// `cache.lock` dans le cache. En calibration, aussi les tags de la voie des témoins négatifs.
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
    let report = crate::check::check_series(paths, target)?;
    ensure_check(&report)?;
    let manifest = Manifest::load(&paths.bench)?;
    // Campagne de tuiles datée du manifest (`tuiles_*`) : tuiles de rues verrouillées, lues dans le
    // cache seulement, jamais téléchargées ni revalidées ; une tuile manquante refuse le remplissage.
    let campaign = crate::service::locked_campaign(&manifest, &paths.cache)?.map(|c| c.id.clone());
    if let Some(id) = &campaign {
        crate::service::lock_streets();
        eprintln!("campagne de tuiles {id} verrouillée : aucune tuile de rues téléchargée");
    }
    fill_cache_inner(paths, &manifest, groupes, ids, coherence_routes, hors_git, goldens, extra_gpx, target)
        .await
        .with_context(|| match &campaign {
            Some(id) => format!(
                "fill-cache, campagne de tuiles {id} verrouillée (une réponse 503 data_unavailable \
                 signale une tuile de rues hors de la campagne)"
            ),
            None => "fill-cache".to_owned(),
        })
}

#[allow(clippy::too_many_arguments)]
async fn fill_cache_inner(
    paths: &BenchPaths,
    manifest: &Manifest,
    groupes: &[String],
    ids: &[String],
    coherence_routes: &[String],
    hors_git: &[String],
    goldens: &[String],
    extra_gpx: &[PathBuf],
    target: Serie,
) -> anyhow::Result<Value> {
    let manifest = manifest.clone();
    let selection = select_series(&manifest, groupes, ids, target);
    let v1 = v1_ctx(paths, &manifest, ids, target)?;
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
    // Tags de la voie des témoins négatifs à leur base OSM (README §4 ter.6), calibration.
    if calibration {
        crate::temoin_tags::fill(&paths.bench, &paths.cache, &negative_controls(&paths.bench)?).await?;
    }
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
        // Traces V1, V1.1 et V1.2 tenues à l'écart : jamais ouvertes en calibration.
        for t in extract_sources(&manifest, &selection, target) {
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
    // Itinéraires planifiés de la série (README §4 ter.4) : E4 / S2 comme une trace sans baro
    // (tuiles du voisinage, deux appels `distance`), R1/R2 comme un parcours d'osm-surface.
    for (pl, _) in &v1.planned.cases {
        if pl.role == "R1_R2" {
            let route = crate::osm_surface::load_planned(paths, pl)?;
            service.route_details(&route.points, crate::osm_surface::options()).await?;
        } else {
            let case =
                crate::v1::load_planned(paths, pl, target).map_err(|e| anyhow::anyhow!("{}: {e}", pl.id))?;
            crate::off_network::fill(&case, &paths.cache, rules.grid_step_m).await?;
            for part in &case.parts {
                for sc in [true, false] {
                    service.route_details(part, distance_options(sc, rules.grid_step_m)).await?;
                }
            }
        }
        eprintln!("{} : tuiles en cache", pl.id);
        filled.push(pl.id.clone());
    }
    // Périmètre vélo des ouvrages de la série (§4 bis.1) : tuiles de leurs stations.
    {
        let ouvrages = crate::ouvrages::series_ouvrages(paths, &manifest, target)?;
        let geoms = crate::ouvrages::way_geometries(&paths.bench);
        let pts: Vec<LatLon> =
            ouvrages.iter().flat_map(|o| crate::ouvrages::perimeter_stations(o, &geoms)).collect();
        crate::ouvrages::Tiles::load_online(&paths.cache, &pts).await?;
    }
    // S2 hors graphe vélo (README §4 ter.5) : voisinage des tuiles de rues des témoins négatifs.
    if ids.is_empty() && target == Serie::Calibration {
        for (dir, id, role) in crate::ouvrages::controls(&paths.bench)? {
            if !role.contains("negative") {
                continue;
            }
            let pts = crate::ouvrages::control_points(&paths.bench, &dir, &id)?;
            let mut st: Vec<(f64, LatLon)> = Vec::with_capacity(pts.len());
            for p in &pts {
                let at = LatLon::new(p.lat, p.lon);
                let d = st.last().map_or(0.0, |(d, q)| d + haversine_m(*q, at));
                st.push((d, at));
            }
            let _ = crate::off_network::closed_on(&st, &paths.cache, 5.0, true).await?;
            eprintln!("{id} : voisinage des tuiles de rues en cache");
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
        "exclus_autre_serie": selection.excluded_held_out,
        "itineraires_exclus": v1.planned.excluded
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

    /// 562d729 (README §4 bis.8) : l'erreur de la référence est rapportée à part, par cas, pour
    /// les parcours planifiés (R1) et les traces perso (R3) ; jamais pour osm-surface.
    #[test]
    fn reference_errors_are_reported_apart_per_case() {
        let os = vec![
            json!({"id": "os-a", "groupe": "osm_surface", "typologie": {"R1.accord_classe": 0.9,
                "R1.accord_classe_hors_inconnu": 0.9, "R1.accord_revetu_non_revetu": 0.99,
                "M1.non_apparie": 0.0, "M1.unknown": 0.0}}),
            json!({"id": "pl-r-a", "groupe": "planifie_R1_R2", "typologie": {"R1.accord_classe": 0.9,
                "R1.accord_classe_hors_inconnu": 0.9, "R1.accord_revetu_non_revetu": 0.99,
                "R1.erreur_reference_m": 300.0, "R1.erreur_reference_part": 0.03, "R1.reference_m": 10_000.0,
                "M1.non_apparie": 0.0, "M1.unknown": 0.0}}),
        ];
        let perso = vec![json!({"id": "perso-01", "groupe": "dplus", "typologie": {"R3.accord_classe": 0.9,
            "R3.accord_classe_hors_inconnu": 0.9, "R3.accord_revetu_non_revetu": 0.99,
            "R3.erreur_reference_m": 0.0, "R3.erreur_reference_part": 0.0, "R3.reference_m": 5_000.0,
            "M1.non_apparie": 0.0, "M1.unknown": 0.0}})];
        let (agg, verdicts) = judge_typology(&os, &perso);
        let e = &agg["erreur_reference"];
        assert_eq!(e["par_cas"].as_object().unwrap().len(), 2, "{e}");
        assert_eq!(e["par_cas"]["pl-r-a"], json!({"critere": "R1", "longueur_m": 300.0, "part": 0.03}));
        assert_eq!(e["par_cas"]["perso-01"]["critere"], "R3");
        assert!(e["par_cas"].get("os-a").is_none());
        assert_eq!((e["longueur_m"].as_f64(), e["reference_m"].as_f64()), (Some(300.0), Some(15_000.0)));
        assert_eq!((verdicts["R1_R2"].as_str(), verdicts["R3"].as_str()), (Some("pass"), Some("pass")));
        assert!(agg.get("tags_differents").is_none(), "osm-surface et parcours sans la règle : rien");
        // Tags différents entre dates (README §4 quater.1) : rapportés par parcours planifié.
        let mut os = os;
        os[1]["typologie"]["R1.tags_differents"] = json!({"longueur_m": 120.0, "part": 0.012});
        let (agg, _) = judge_typology(&os, &perso);
        assert_eq!(agg["tags_differents"]["par_cas"]["pl-r-a"]["longueur_m"], 120.0);
        assert_eq!(agg["tags_differents"]["longueur_m"], 120.0);
    }

    /// Point 6 de 7dab276 (README §4 ter.9) : au jugement, une trace de la série tenue à l'écart
    /// est un cas entier de la sélection (`OuvrageEval::whole`, jamais un extrait) et son retard
    /// est estimé normalement ; seuls les extraits d'une trace de l'autre série gardent leur baro
    /// brut. Données synthétiques : aucun fichier de la série tenue à l'écart n'est ouvert.
    #[test]
    fn held_out_traces_are_whole_cases_with_their_lag_estimated_at_judgement() {
        let trace = |id: &str, serie, masques: &[&str]| Trace {
            id: id.into(),
            groupe: "dplus".into(),
            serie,
            fichier: format!("private/traces/{id}.gpx"),
            sha256: None,
            sha256_trim1km: None,
            passages_masques: masques.iter().map(|m| (*m).to_owned()).collect(),
            verdict: None,
            dist_km: None,
            gain_baro_m: None,
            gain_mnt_m: None,
            noise_m: None,
        };
        let m = Manifest {
            sha256: String::new(),
            tuiles: Vec::new(),
            vcyclist: Vec::new(),
            osm_surface: Vec::new(),
            copies: Vec::new(),
            ouvrages_tenus: Default::default(),
            ouvrages_perso: None,
            traces: vec![
                trace("cal", Serie::Calibration, &["nv-x"]),
                trace("held", Serie::TenueALEcart, &["nv-y"]),
            ],
            hors_git: Vec::new(),
            v1: None,
            v11: None,
            v12: None,
            mvp_judged: false,
        };
        // Jugement : la trace tenue à l'écart est dans les cas mesurés en entier, pas dans la
        // liste des traces lues par extraits.
        let judged = select_series(&m, &[], &[], Serie::TenueALEcart);
        assert_eq!(judged.cases.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["held"]);
        assert!(!judged.excluded_held_out.contains(&"held".to_owned()));
        // Relief sinusoïdal sur 20 km, baro en retard de 40 m.
        let relief = |s: f64| 200.0 + 40.0 * (s * std::f64::consts::TAU / 2_000.0).sin();
        let d: Vec<f64> = (0..=4_000).map(|k| k as f64 * 5.0).collect();
        let mnt: Vec<f64> = d.iter().map(|&s| relief(s)).collect();
        let bd: Vec<f64> = (0..=800).map(|k| k as f64 * 25.0).collect();
        let bz: Vec<f64> = bd.iter().map(|&s| relief(s - 40.0) + 5.0).collect();
        let masked = vec![false; d.len()];
        let rules = BenchRules::default();
        let cache = Path::new("/nonexistent");
        // Cas entier (chemin de `measure_pass` pour les cas de la sélection) : δ estimé.
        let whole = OuvrageEval::whole(cache);
        assert!(whole.window.is_none() && whole.ring.is_none());
        let r = case_retard(&whole, &d, &bd, &bz, &mnt, &masked, &rules);
        assert_eq!((r.retard_m, r.raison), (Some(40.0), None), "{r:?}");
        // Extrait (fenêtre évaluée) : baro brut, mention.
        let extract =
            OuvrageEval { cache, window: Some(9_500.0..10_500.0), ring: Some(Vec::new()), retard: None };
        let r = case_retard(&extract, &d, &bd, &bz, &mnt, &masked, &rules);
        assert_eq!((r.retard_m, r.raison), (None, Some("extrait_tenu_a_l_ecart")));
    }

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
            baro_fiable: true,
            baro_noise_m: None,
            parts: vec![pts],
            baro_s: s_ref.clone(),
            baro_z: vec![10.0; s_ref.len()],
            baro_inserted: vec![false; s_ref.len()],
            s_ref,
            masks: vec!["pont".into()],
            implicit_masks: Vec::new(),
            ring: Vec::new(),
            raw: None,
        };
        (case, refs)
    }

    /// Trace de calibration `id` (extraits lus au jugement), sans fichier.
    fn calibration_trace(id: &str) -> Trace {
        Trace {
            id: id.into(),
            groupe: "ouvrage".into(),
            serie: Serie::Calibration,
            fichier: format!("private/traces/{id}.gpx"),
            sha256: None,
            sha256_trim1km: None,
            passages_masques: vec!["pont".into()],
            verdict: None,
            dist_km: None,
            gain_baro_m: None,
            gain_mnt_m: None,
            noise_m: None,
        }
    }

    /// Relief sinusoïdal sur 20 km, baro en retard de 40 m (δ réel du profil).
    #[allow(clippy::type_complexity)]
    fn lagged_profile() -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
        let relief = |s: f64| 200.0 + 40.0 * (s * std::f64::consts::TAU / 2_000.0).sin();
        let d: Vec<f64> = (0..=4_000).map(|k| k as f64 * 5.0).collect();
        let mnt: Vec<f64> = d.iter().map(|&s| relief(s)).collect();
        let bd: Vec<f64> = (0..=800).map(|k| k as f64 * 25.0).collect();
        let bz: Vec<f64> = bd.iter().map(|&s| relief(s - 40.0) + 5.0).collect();
        (d, mnt, bd, bz)
    }

    /// README §4 ter.9 (30e8024, 87b0ea8) : au jugement, un extrait d'une trace de calibration
    /// reçoit le δ gelé de sa trace, sans réestimation (ici 15 m, quand le profil de l'extrait
    /// donnerait 40 m) ; le repli 1–3 km lit le baro corrigé baro(s + δ) dans l'abscisse de la
    /// trace. Données synthétiques, aucun fichier de la série tenue à l'écart.
    #[test]
    fn a_calibration_extract_at_judgement_takes_the_frozen_lag() {
        let frozen =
            crate::retard::RetardGele::parse(r#"{"cas": {"perso-99": {"retard_m": 15.0}}}"#).unwrap();
        let trace = calibration_trace("perso-99");
        let r = extract_retard(&trace, "perso-99", &frozen);
        assert_eq!((r.retard_m, r.raison, r.source), (Some(15.0), None, Some("gele")));
        assert_eq!(r.to_json(), json!({"source": "gele", "retard_m": 15.0}));
        // Pas de réestimation : le δ reçu l'emporte sur celui que donnerait le profil.
        let (d, mnt, bd, bz) = lagged_profile();
        let rules = BenchRules::default();
        let cache = Path::new("/nonexistent");
        let masked = vec![false; d.len()];
        let eval =
            OuvrageEval { cache, window: Some(9_500.0..10_500.0), ring: Some(Vec::new()), retard: Some(r) };
        let got = case_retard(&eval, &d, &bd, &bz, &mnt, &masked, &rules);
        assert_eq!((got.retard_m, got.shift_m(), got.source), (Some(15.0), 15.0, Some("gele")));
        // Repli 1–3 km : baro(s + δ) de la trace (baro linéaire, 1 m tous les 100 m).
        let (mut case, refs) = straight_case();
        case.serie = Serie::Calibration;
        case.baro_z = case.baro_s.iter().map(|s| s / 100.0).collect();
        let raw = held_out_extracts(&case, &refs, &rules);
        let lagged = held_out_extracts_lagged(&case, &refs, &rules, 15.0);
        assert_eq!(raw.len(), 1);
        assert_eq!(lagged.len(), 1);
        let (r0, r1) = (&raw[0].0.ring, &lagged[0].0.ring);
        assert_eq!(r0.len(), r1.len());
        let pairs: Vec<(f64, f64)> = r0
            .iter()
            .zip(r1)
            .filter(|(a, b)| a.baro.is_finite() && b.baro.is_finite())
            .map(|(a, b)| (a.baro, b.baro))
            .collect();
        assert!(pairs.len() > 100);
        assert!(pairs.iter().all(|(a, b)| (b - a - 0.15).abs() < 1e-9), "baro(s + 15 m)");
        // Le baro de l'extrait reste brut ; `measure_case` le corrige du δ reçu.
        assert_eq!(raw[0].0.baro_z, lagged[0].0.baro_z);
    }

    /// Trace de calibration absente du fichier gelé, ou `retard_m` null : l'extrait garde son
    /// baro brut (`retard_non_estimable`), sans estimation ; extrait d'une trace de l'autre série
    /// (calibration) : baro brut, `extrait_tenu_a_l_ecart`.
    #[test]
    fn an_extract_without_a_frozen_lag_keeps_its_raw_baro() {
        let frozen =
            crate::retard::RetardGele::parse(r#"{"cas": {"perso-98": {"retard_m": null}}}"#).unwrap();
        let (d, mnt, bd, bz) = lagged_profile();
        let rules = BenchRules::default();
        let cache = Path::new("/nonexistent");
        let masked = vec![false; d.len()];
        for (id, raison) in [("perso-97", "absent_du_retard_gele"), ("perso-98", "retard_gele_non_estime")] {
            let r = extract_retard(&calibration_trace(id), id, &frozen);
            assert_eq!((r.retard_m, r.raison, r.shift_m()), (None, Some(raison), 0.0));
            let v = r.to_json();
            assert_eq!(
                (&v["source"], &v["retard_m"], &v["mention"]),
                (&json!("gele"), &Value::Null, &json!("retard_non_estimable"))
            );
            let eval = OuvrageEval {
                cache,
                window: Some(9_500.0..10_500.0),
                ring: Some(Vec::new()),
                retard: Some(r),
            };
            let got = case_retard(&eval, &d, &bd, &bz, &mnt, &masked, &rules);
            assert_eq!((got.retard_m, got.shift_m(), got.raison), (None, 0.0, Some(raison)), "baro brut");
        }
        let held = Trace { serie: Serie::TenueALEcart, ..calibration_trace("perso-98") };
        let r = extract_retard(&held, "perso-98", &frozen);
        assert_eq!((r.retard_m, r.raison, r.source), (None, Some("extrait_tenu_a_l_ecart"), None));
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

    /// Trace synthétique nord-sud (méridien : la distance haversine y est exacte) de `len_m`, un
    /// sommet tous les `vertex_m`, coupée en tronçons `.partN` aux sommets `cuts` (le tronçon N+1
    /// repart du dernier point du tronçon N) ; masques `masks`.
    fn meridian_case(len_m: f64, vertex_m: f64, cuts: &[usize], masks: &[&str]) -> PersoCase {
        let deg = |m: f64| (m / ord_domain::geo::EARTH_RADIUS_M).to_degrees();
        let n = (len_m / vertex_m).round() as usize;
        let all: Vec<Pt> = (0..=n).map(|k| Pt { lat: 45.0 + deg(k as f64 * vertex_m), lon: 6.0 }).collect();
        let mut bounds = vec![0];
        bounds.extend_from_slice(cuts);
        bounds.push(n);
        let parts: Vec<Vec<Pt>> = bounds.windows(2).map(|b| all[b[0]..=b[1]].to_vec()).collect();
        let s_ref = cumulative(&all);
        PersoCase {
            id: "perso-99".into(),
            groupe: "ouvrage".into(),
            serie: Serie::Calibration,
            baro_fiable: true,
            baro_noise_m: None,
            parts,
            baro_s: s_ref.clone(),
            baro_z: vec![10.0; s_ref.len()],
            baro_inserted: vec![false; s_ref.len()],
            s_ref,
            masks: masks.iter().map(|m| (*m).to_owned()).collect(),
            implicit_masks: Vec::new(),
            ring: Vec::new(),
            raw: None,
        }
    }

    /// Point à `along` m du début de `meridian_case`, décalé de `aside` m vers l'est.
    fn meridian_point(along: f64, aside: f64) -> LatLon {
        let deg = |m: f64| (m / ord_domain::geo::EARTH_RADIUS_M).to_degrees();
        LatLon::new(45.0 + deg(along), 6.0 + deg(aside) / 45f64.to_radians().cos())
    }

    /// perso-44-s0 (jugement V1 interrompu) : une extrémité de témoin à quelques mètres de la
    /// géométrie, au milieu de deux sommets espacés de plus de 80 m, est à plus de 40 m de tout
    /// sommet. Les stations de 5 m du mode `distance` (celles de la calibration) la trouvent : la
    /// fenêtre couvre tout le témoin, ± 500 m, et l'extrait le contient entier, ± 1 km.
    #[test]
    fn a_temoin_end_between_distant_vertices_is_found() {
        let case = meridian_case(12_000.0, 100.0, &[], &["perso-1-s0"]);
        let (a, b) = (meridian_point(3_050.0, 3.6), meridian_point(8_550.0, 3.6));
        // Aucun sommet à 40 m : la lecture sur les sommets de la géométrie (avant bench5) ne
        // voyait aucune des deux extrémités.
        let ll: Vec<LatLon> = case.parts[0].iter().map(|q| LatLon::new(q.lat, q.lon)).collect();
        assert!(ll.iter().all(|p| haversine_m(*p, a) > 40.0 && haversine_m(*p, b) > 40.0));
        let refs = BTreeMap::from([("perso-1-s0".to_owned(), vec![a, b])]);
        let rules = BenchRules::default();
        let w = held_out_windows_by_id(&case, &refs, &rules);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].1.len(), 1, "{w:?}");
        let r = &w[0].1[0];
        // Stations à 40 m au plus : de 3 015 à 3 085 m, de 8 515 à 8 585 m (pas de 5 m).
        assert!((r.start - 2_515.0).abs() < 1e-6 && (r.end - 9_085.0).abs() < 1e-6, "{r:?}");
        assert_eq!(
            held_out_losses(&case.masks, std::slice::from_ref(&case), &refs, &rules),
            HeldOutLosses::default()
        );
        let ex = held_out_extracts(&case, &refs, &rules);
        assert_eq!(ex.len(), 1);
        let (e, ew) = &ex[0];
        let (s0, s1) = (e.s_ref[0], e.s_ref[e.s_ref.len() - 1]);
        assert!(s0 <= ew.start - 1_000.0 + 100.0 && s1 >= ew.end + 1_000.0 - 100.0, "{ew:?}");
        // Les deux extrémités du témoin sont dans la fenêtre évaluée de l'extrait.
        assert!(ew.contains(&(3_050.0 - 1_515.0)) && ew.contains(&(8_550.0 - 1_515.0)), "{ew:?}");
    }

    /// La fenêtre de jugement est le masque de calibration : `mask_windows` sur la grille de 5 m
    /// du service (`d = 5 k`, position sur le méridien), ici sur un cas en deux tronçons
    /// `.partN` (raccord à 4 200 m, hors de la grille) et un ouvrage, un témoin, un croisement.
    #[test]
    fn the_judgement_window_is_the_calibration_mask() {
        let case = meridian_case(10_030.0, 85.0, &[50], &["nv-1", "perso-1-s0", "nv-2"]);
        let total = case.s_ref[case.s_ref.len() - 1];
        let refs = BTreeMap::from([
            (
                "nv-1".to_owned(),
                vec![
                    meridian_point(1_210.0, 6.0),
                    meridian_point(1_260.0, 6.0),
                    meridian_point(1_310.0, 6.0),
                ],
            ),
            ("perso-1-s0".to_owned(), vec![meridian_point(3_990.0, 2.0), meridian_point(6_642.0, -1.0)]),
            ("nv-2".to_owned(), vec![meridian_point(9_900.0, 20.0)]),
        ]);
        // Grille du service, recollée : premier tronçon de 0 à 4 250 m (50 × 85) au pas de 5 m,
        // second repartant de 4 250 m.
        let cut = 50.0 * 85.0;
        let mut d: Vec<f64> = (0..=850).map(|k| k as f64 * 5.0).collect();
        d.extend((1..).map(|k| cut + k as f64 * 5.0).take_while(|x| *x < total - 1e-3));
        d.push(total);
        // Sortie du service : distances au cm, coordonnées à 1e-6°.
        let coord = |v: f64| (v * 1e6).round() / 1e6;
        let pts: Vec<LatLon> = d
            .iter()
            .map(|&x| meridian_point(x, 0.0))
            .map(|p| LatLon::new(coord(p.lat), coord(p.lon)))
            .collect();
        let d: Vec<f64> = d.iter().map(|x| (x * 100.0).round() / 100.0).collect();
        let rules = BenchRules::default();
        let (sd, sp) = distance_stations(&case, rules.grid_step_m);
        assert_eq!(sd.len(), d.len());
        assert!(sd.iter().zip(&d).all(|(a, b)| (a - b).abs() < 1e-6));
        assert!(sp.iter().zip(&pts).all(|(a, b)| haversine_m(*a, *b) < 1e-3));
        let judged = held_out_windows_by_id(&case, &refs, &rules);
        for (id, w) in &judged {
            let calib = mask_windows(&d, &pts, id, &refs, &rules);
            assert!(!calib.is_empty(), "{id}");
            assert_eq!(w.len(), calib.len(), "{id}");
            for (a, b) in w.iter().zip(&calib) {
                assert!((a.start - b.start).abs() < 1e-6 && (a.end - b.end).abs() < 1e-6, "{id} {a:?} {b:?}");
            }
        }
        // La fenêtre d'un passage en bout de parcours n'est pas rognée (comme en calibration).
        let tail = &judged.iter().find(|(id, _)| id == "nv-2").unwrap().1[0];
        assert!(tail.end > total + 400.0, "{tail:?}");
    }

    /// Décision de l'utilisateur (2026-11-02) : un élément de `passages_masques` qu'aucune station
    /// n'approche à 40 m n'est pas un passage (non retrouvé en calibration aussi) : rapporté, sans
    /// interrompre le jugement. Un élément retrouvé mais perdu au jugement (sans fenêtre), un témoin
    /// aux extrémités non retrouvées, un élément absent du corpus : cas sautés, jamais une perte
    /// silencieuse.
    #[test]
    fn a_passage_not_found_is_reported_and_a_lost_one_is_skipped() {
        let case = meridian_case(10_000.0, 100.0, &[], &["nv-1", "perso-1-s0", "nv-3"]);
        let refs = BTreeMap::from([
            ("nv-1".to_owned(), vec![meridian_point(2_000.0, 5.0)]),
            ("perso-1-s0".to_owned(), vec![meridian_point(4_000.0, 1.0), meridian_point(6_000.0, 300.0)]),
            ("nv-3".to_owned(), vec![meridian_point(8_000.0, 45.0)]),
        ]);
        let rules = BenchRules::default();
        let losses = held_out_losses(&case.masks, std::slice::from_ref(&case), &refs, &rules);
        assert_eq!(losses.skipped, vec!["perso-1-s0 : témoin aux extrémités non retrouvées".to_owned()]);
        assert_eq!(losses.not_found, vec!["nv-3".to_owned()], "non retrouvé : rapporté");
        // Cas `.sN` d'une trace : l'élément est cherché sur tous ; élément absent du corpus : sauté.
        let other = meridian_case(3_000.0, 100.0, &[], &["nv-1"]);
        let losses = held_out_losses(&["nv-1".to_owned(), "nv-9".to_owned()], &[other, case], &refs, &rules);
        assert_eq!(losses.skipped, vec!["nv-9 : absent du corpus".to_owned()]);
        assert!(losses.not_found.is_empty());
        // Perdu au jugement : une station approche l'ouvrage (retrouvé), mais le cas ne donne
        // aucune fenêtre (abscisses et points envoyés désaccordés) → sauté, pas rapporté.
        let mut broken = meridian_case(10_000.0, 100.0, &[], &["nv-1", "nv-3"]);
        broken.s_ref.pop();
        let losses = held_out_losses(&broken.masks.clone(), &[broken], &refs, &rules);
        assert_eq!(losses.skipped, vec!["nv-1 : passage retrouvé sans fenêtre".to_owned()]);
        assert_eq!(losses.not_found, vec!["nv-3".to_owned()]);
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

#[cfg(test)]
mod e1_tests {
    use super::*;

    fn case(id: &str, groupe: &str, noise: f64, rmse: f64) -> Value {
        json!({"id": id, "groupe": groupe, "bruit_baro_m": noise,
            "metriques": {"E1.rmse_m": rmse, "E1.p95_m": 2.0, "E2.ratio": 1.0, "E2.ratio_mnt_brut": 1.0,
                "E2b.ratio": 1.0, "E2b.ratio_mnt_brut": 1.0, "E2.dplus_service_m": 1.0, "E2.dplus_mnt_brut_m": 1.0}})
    }

    /// README §4 ter.8 : trace plaine au bruit baro > 1,0 m, référence incertaine, non jugée ; dplus
    /// non concerné.
    #[test]
    fn a_noisy_plain_baro_is_an_uncertain_e1_reference() {
        let cases =
            [case("a", "plaine", 1.44, 1.77), case("b", "plaine", 0.6, 0.5), case("c", "dplus", 1.4, 3.5)];
        let (_, v) = judge(&cases);
        assert_eq!(v["E1_echecs"].as_array().unwrap().len(), 1, "{v}");
        assert_eq!(v["E1_echecs"][0]["id"], "c");
        assert_eq!(v["E1_references_incertaines"][0]["id"], "a");
        let (_, v) = judge(&cases[..2]);
        assert_eq!(v["E1"], "pass", "{v}");
    }
}

#[cfg(test)]
mod closed_control_tests {
    use super::*;
    use crate::typology::{Class, Ours, Source};

    /// M1 hors graphe vélo (62bd46e) : station appariée (voie parallèle du graphe, distincte de la
    /// route fermée) ou non appariée sous unmatched_range = contrôle satisfait ; non appariée sans
    /// unmatched_range = échec.
    #[test]
    fn a_parallel_way_of_the_graph_satisfies_the_control() {
        let seg = |from: f64, to: f64, matched: bool| Ours {
            from,
            to,
            class: Class::Paved,
            matched,
            source: Source::of_service(""),
        };
        let ours = [seg(0.0, 100.0, true), seg(100.0, 200.0, false)];
        let c = closed_control(&[10.0, 50.0, 120.0], &ours, &[(110.0, 150.0)]);
        assert_eq!(
            c,
            json!({"stations": 3, "appariees_voie_parallele": 2, "sans_unmatched_range": 0, "ok": true})
        );
        let c = closed_control(&[10.0, 180.0], &ours, &[(110.0, 150.0)]);
        assert_eq!(c["ok"], false);
        assert_eq!(c["sans_unmatched_range"], 1);
    }
}
