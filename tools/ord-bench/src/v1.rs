//! Série V1 (README §4 ter.4 ; manifest, section `perso_v1`) : index des ouvrages, croisements et
//! témoins V1, séries par cas (`series-v1.csv`), itinéraires planifiés.
//!
//! Discipline (README §2) : la campagne de calibration ne lit que la calibration. Les index
//! (`ouvrages-v1.json`, `temoins-v1.json`, `series-v1.csv`) portent les deux séries : ils sont lus,
//! vérifiés par empreinte, et l'autre série en est retirée **avant** tout calcul ; ses points ne
//! servent qu'à masquer ses fenêtres. Les fichiers propres d'un cas de l'autre série (trace,
//! recalage, itinéraire, référence) ne sont jamais ouverts : seul `judge` lit la série tenue à
//! l'écart.

use anyhow::{bail, Context};
use ord_domain::geo::haversine_m;
use ord_domain::LatLon;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use crate::campaign::{PersoCase, RawGps, RawRef};
use crate::manifest::{Manifest, PersoV1, Planned, Serie};
use crate::service::Pt;
use crate::BenchPaths;

/// Pas de l'entrée d'un itinéraire planifié (GPX tel quel, rééchantillonné ; données de
/// data-init, `perso_v1.planifies`).
pub const PLANNED_STEP_M: f64 = 25.0;

/// Tolérance d'un élément « sur » un parcours : tous ses points à moins de 40 m (`passage_tol_m`,
/// règle de `tools/build_v1_corpus.py`).
pub const ON_TRACK_TOL_M: f64 = 40.0;

/// Série d'un itinéraire planifié (`series-v1.csv`) : un itinéraire `par_cas` est un cas de
/// calibration pour ses éléments de calibration, et de la série tenue à l'écart pour les autres.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlannedSerie {
    Serie(Serie),
    ParCas,
}

impl PlannedSerie {
    /// L'itinéraire entre-t-il dans une campagne de la série `target` ?
    pub fn in_series(self, target: Serie) -> bool {
        match self {
            Self::Serie(s) => s == target,
            Self::ParCas => true,
        }
    }
}

/// Ligne de `series-v1.csv` : `id, type, groupe, serie, passages_masques`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeriesRow {
    pub id: String,
    pub kind: String,
    pub groupe: String,
    pub serie: String,
}

/// Lit un fichier d'index V1 et vérifie son empreinte (manifest).
fn read_verified(paths: &BenchPaths, (fichier, sha): &(String, String)) -> anyhow::Result<String> {
    let path = paths.resolve(fichier);
    let bytes = std::fs::read(&path).with_context(|| format!("{fichier} : fichier absent"))?;
    if crate::hash::sha256_bytes(&bytes) != *sha {
        bail!("{fichier} : empreinte différente de celle du manifest");
    }
    String::from_utf8(bytes).with_context(|| format!("{fichier} illisible"))
}

pub fn parse_series(text: &str) -> Vec<SeriesRow> {
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').map(str::trim).collect();
            Some(SeriesRow {
                id: f.first()?.to_string(),
                kind: f.get(1)?.to_string(),
                groupe: f.get(2)?.to_string(),
                serie: f.get(3)?.to_string(),
            })
        })
        .filter(|r| !r.id.is_empty())
        .collect()
}

/// `series-v1.csv`, empreinte vérifiée.
pub fn series_rows(paths: &BenchPaths, v1: &PersoV1) -> anyhow::Result<Vec<SeriesRow>> {
    let mut rows = parse_series(&read_verified(paths, &v1.series)?);
    if v1.judged {
        judged_rows(&mut rows);
    }
    Ok(rows)
}

/// Série V1 jugée (README §2, point 3) : les lignes tenues à l'écart passent en calibration ; un
/// itinéraire `par_cas` le reste (tous ses éléments sont désormais de calibration).
pub fn judged_rows(rows: &mut [SeriesRow]) {
    for r in rows.iter_mut().filter(|r| r.serie == "tenue_a_l_ecart") {
        r.serie = "calibration".into();
    }
}

/// Série d'une entrée d'index V1 (`temoins-v1.json`, `temoins-v1-tags.json`) : calibration pour
/// toute entrée une fois la série V1 jugée.
fn index_serie(v1: &PersoV1, s: Option<&str>) -> Option<Serie> {
    let serie = Serie::parse(s?).ok()?;
    Some(if v1.judged { Serie::Calibration } else { serie })
}

/// Série d'un itinéraire planifié ; `None` s'il est absent de `series-v1.csv`.
pub fn planned_serie(rows: &[SeriesRow], id: &str) -> Option<PlannedSerie> {
    let r = rows.iter().find(|r| r.id == id && r.kind == "planifie")?;
    match r.serie.as_str() {
        "par_cas" => Some(PlannedSerie::ParCas),
        s => Serie::parse(s).ok().map(PlannedSerie::Serie),
    }
}

/// Série d'un élément (ouvrage, croisement, témoin) de `series-v1.csv`.
pub fn element_serie(rows: &[SeriesRow], id: &str) -> Option<Serie> {
    rows.iter()
        .find(|r| r.id == id && matches!(r.kind.as_str(), "ouvrage" | "temoin"))
        .and_then(|r| Serie::parse(&r.serie).ok())
}

/// Itinéraires planifiés d'une campagne de la série `target` : `cases` (avec leur série) ; les
/// autres sont listés dans `excluded`, sans rien en lire.
#[derive(Clone, Debug, Default)]
pub struct PlannedSelection {
    pub cases: Vec<(Planned, PlannedSerie)>,
    pub excluded: Vec<String>,
}

pub fn select_planned(
    manifest: &Manifest,
    rows: &[SeriesRow],
    ids: &[String],
    target: Serie,
) -> PlannedSelection {
    let mut out = PlannedSelection::default();
    let Some(v1) = &manifest.v1 else { return out };
    for p in v1.planifies.iter().filter(|p| ids.is_empty() || ids.contains(&p.id)) {
        match planned_serie(rows, &p.id) {
            Some(s) if s.in_series(target) => out.cases.push((p.clone(), s)),
            _ => out.excluded.push(p.id.clone()),
        }
    }
    out
}

/// Témoin négatif V1 (`temoins-v1.json`) : tronçon d'une sortie entre `debut` et `fin`, ou
/// itinéraire planifié entier.
#[derive(Clone, Debug, PartialEq)]
pub struct TemoinV1 {
    pub id: String,
    pub trace: String,
    pub planifie: bool,
    pub nature: String,
    pub serie: Serie,
    pub ends: Option<[LatLon; 2]>,
    /// Ouvrages réels sur la voie du témoin (README §4 ter.6 ; `temoins-v1-tags.json`, 57e9d95) :
    /// plages taguées `covered`, `avalanche_protector`, `bridge` ou `tunnel` de la base OSM V1.
    pub real: Vec<RealRange>,
    /// Longueur du témoin lue par `tools/temoins_v1_tags.py` (part d'une plage).
    pub length_m: Option<f64>,
}

/// Plage d'ouvrage réel sur la voie d'un témoin V1 (extrémités projetées sur le cas par ord-bench).
#[derive(Clone, Debug, PartialEq)]
pub struct RealRange {
    pub tag: String,
    pub ends: [LatLon; 2],
    pub length_m: f64,
}

/// Plages d'ouvrages réels d'un témoin et sa longueur.
type TemoinTags = (Vec<RealRange>, Option<f64>);

/// Plages d'ouvrages réels des témoins de la série `target` (`perso_v1.temoins_v1_tags` du
/// manifest, 6ed1da7, empreinte vérifiée) ; les entrées de l'autre série sont retirées à la
/// lecture, avant tout calcul. Entrée absente du manifest : aucune plage.
fn real_ranges(
    paths: &BenchPaths,
    v1: &PersoV1,
    target: Serie,
) -> anyhow::Result<BTreeMap<String, TemoinTags>> {
    let Some(entry) = &v1.temoins_tags else {
        return Ok(BTreeMap::new());
    };
    let text = read_verified(paths, entry)?;
    let v: Value = serde_json::from_str(&text).context("temoins-v1-tags.json")?;
    let mut out = BTreeMap::new();
    for (id, t) in v.as_object().into_iter().flatten() {
        if index_serie(v1, t["serie"].as_str()) != Some(target) {
            continue;
        }
        let ranges = t["plages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                Some(RealRange {
                    tag: p["tag"].as_str()?.to_owned(),
                    ends: [point(&p["debut"])?, point(&p["fin"])?],
                    length_m: p["longueur_m"].as_f64()?,
                })
            })
            .collect();
        out.insert(id.clone(), (ranges, t["longueur_temoin_m"].as_f64()));
    }
    Ok(out)
}

fn point(v: &Value) -> Option<LatLon> {
    Some(LatLon::new(v[0].as_f64()?, v[1].as_f64()?))
}

/// Témoins V1 de la série `target` ; les autres sont retirés à la lecture de l'index.
pub fn temoins(paths: &BenchPaths, v1: &PersoV1, target: Serie) -> anyhow::Result<Vec<TemoinV1>> {
    let v: Value = serde_json::from_str(&read_verified(paths, &v1.temoins)?).context("temoins-v1.json")?;
    let mut real = real_ranges(paths, v1, target)?;
    Ok(v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let serie = index_serie(v1, t["serie"].as_str())?;
            (serie == target).then(|| TemoinV1 {
                id: t["id"].as_str().unwrap_or_default().to_owned(),
                trace: t["trace"].as_str().unwrap_or_default().to_owned(),
                planifie: t["planifie"].as_bool().unwrap_or(false),
                nature: t["nature"].as_str().unwrap_or_default().to_owned(),
                serie,
                ends: point(&t["debut"]).zip(point(&t["fin"])).map(|(a, b)| [a, b]),
                real: Vec::new(),
                length_m: None,
            })
        })
        .map(|mut t: TemoinV1| {
            if let Some((r, l)) = real.remove(&t.id) {
                t.real = r;
                t.length_m = l;
            }
            t
        })
        .collect())
}

/// Points de référence des éléments V1, des deux séries (masques) : A, milieu, B (ou point de
/// croisement) des ouvrages ; extrémités des témoins de sortie.
pub fn refs(paths: &BenchPaths, v1: &PersoV1) -> anyhow::Result<BTreeMap<String, Vec<LatLon>>> {
    let mut out = BTreeMap::new();
    let o: Value = serde_json::from_str(&read_verified(paths, &v1.ouvrages)?).context("ouvrages-v1.json")?;
    for x in o.as_array().into_iter().flatten() {
        let Some(id) = x["id"].as_str() else { continue };
        let pts: Vec<LatLon> =
            x["pts"].as_object().into_iter().flat_map(|m| m.values()).filter_map(point).collect();
        out.insert(id.to_owned(), pts);
    }
    let t: Value = serde_json::from_str(&read_verified(paths, &v1.temoins)?).context("temoins-v1.json")?;
    for x in t.as_array().into_iter().flatten() {
        let Some(id) = x["id"].as_str() else { continue };
        let pts: Vec<LatLon> = [&x["debut"], &x["fin"]].into_iter().filter_map(point).collect();
        if !pts.is_empty() {
            out.insert(id.to_owned(), pts);
        }
    }
    Ok(out)
}

/// Identifiant d'un témoin V1 (`<trace>-sN`, trace `perso-NN` ou itinéraire `pl-…`) : sa fenêtre
/// masquée couvre tout le tronçon, de son début à sa fin (± 500 m), pas seulement ses extrémités.
pub fn is_temoin_id(id: &str) -> bool {
    let Some((head, n)) = id.rsplit_once("-s") else { return false };
    !n.is_empty()
        && n.chars().all(|c| c.is_ascii_digit())
        && (head.starts_with("perso-") || head.starts_with("pl-"))
}

/// Grille grossière (≈ 100 m) des points d'un parcours, pour « tous les points à moins de 40 m ».
struct Grid(BTreeMap<(i64, i64), Vec<LatLon>>);

impl Grid {
    const CELL: f64 = 0.001;

    fn key(p: LatLon) -> (i64, i64) {
        ((p.lat / Self::CELL).floor() as i64, (p.lon / Self::CELL).floor() as i64)
    }

    fn new(pts: &[LatLon]) -> Self {
        let mut m: BTreeMap<(i64, i64), Vec<LatLon>> = BTreeMap::new();
        for p in pts {
            m.entry(Self::key(*p)).or_default().push(*p);
        }
        Self(m)
    }

    fn near(&self, p: LatLon, tol: f64) -> bool {
        let (a, b) = Self::key(p);
        (a - 1..=a + 1)
            .flat_map(|x| (b - 2..=b + 2).map(move |y| (x, y)))
            .filter_map(|k| self.0.get(&k))
            .flatten()
            .any(|q| haversine_m(*q, p) <= tol)
    }
}

/// Éléments V1 de l'autre série que `target` sur un parcours (masques, README §2) : ouvrages et
/// croisements dont tous les points sont à moins de 40 m, comme `build_v1_corpus.py` ; témoins
/// de l'autre série portés par ce parcours (`own`). Ni service ni fichier de cas : index seulement.
pub fn other_series_on(
    pts: &[LatLon],
    refs: &BTreeMap<String, Vec<LatLon>>,
    rows: &[SeriesRow],
    target: Serie,
    own: &str,
) -> Vec<String> {
    let grid = Grid::new(pts);
    let mut out = BTreeSet::new();
    for r in rows.iter().filter(|r| matches!(r.kind.as_str(), "ouvrage" | "temoin")) {
        if Serie::parse(&r.serie).ok() == Some(target) {
            continue;
        }
        let mine = r.kind == "temoin"
            && (r.id == own || r.id.strip_prefix(own).is_some_and(|x| x.starts_with("-s")));
        let on =
            refs.get(&r.id).is_some_and(|p| !p.is_empty() && p.iter().all(|q| grid.near(*q, ON_TRACK_TOL_M)));
        if mine || on {
            out.insert(r.id.clone());
        }
    }
    out.into_iter().collect()
}

/// Rééchantillonne une polyligne au pas `step` (abscisse haversine), extrémités comprises ;
/// `extra` (une valeur par point d'entrée, ex. abscisse d'une référence) est interpolé aux mêmes
/// abscisses.
pub fn resample(pts: &[LatLon], extra: &[f64], step: f64) -> (Vec<LatLon>, Vec<f64>, Vec<f64>) {
    let mut cum = vec![0.0];
    for w in pts.windows(2) {
        cum.push(cum.last().copied().unwrap_or(0.0) + haversine_m(w[0], w[1]));
    }
    let total = cum.last().copied().unwrap_or(0.0);
    let mut s: Vec<f64> = (0..).map(|k| k as f64 * step).take_while(|x| *x < total).collect();
    if s.last().is_none_or(|l| total - l > 1e-6) {
        s.push(total);
    }
    let lat: Vec<f64> = pts.iter().map(|p| p.lat).collect();
    let lon: Vec<f64> = pts.iter().map(|p| p.lon).collect();
    let at = |ys: &[f64], x: f64| crate::campaign::interp_one(&cum, ys, x);
    let out: Vec<LatLon> = s.iter().map(|&x| LatLon::new(at(&lat, x), at(&lon, x))).collect();
    let ex: Vec<f64> =
        if extra.len() == pts.len() { s.iter().map(|&x| at(extra, x)).collect() } else { Vec::new() };
    // Abscisse haversine des points rééchantillonnés eux-mêmes (celle que calcule le service).
    let mut sd = vec![0.0];
    for w in out.windows(2) {
        sd.push(sd.last().copied().unwrap_or(0.0) + haversine_m(w[0], w[1]));
    }
    (out, sd, ex)
}

/// Itinéraire planifié chargé comme un cas sans baro : GPX tel quel, rééchantillonné à 25 m ; la
/// « trace brute » de l'attribution des passages (README §4 ter.2) est l'itinéraire lui-même.
pub fn load_planned(paths: &BenchPaths, p: &Planned, serie: Serie) -> Result<PersoCase, String> {
    let path = paths.resolve(&p.fichier);
    let bytes = std::fs::read(&path).map_err(|_| "itinéraire planifié absent".to_owned())?;
    if crate::hash::sha256_bytes(&bytes) != p.sha256 {
        return Err("itinéraire planifié : empreinte différente du manifest".into());
    }
    let text = String::from_utf8(bytes).map_err(|_| "itinéraire planifié illisible".to_owned())?;
    let raw: Vec<LatLon> = crate::gpx::parse(&text)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|q| LatLon::new(q.lat, q.lon))
        .collect();
    if raw.len() < 2 {
        return Err("itinéraire planifié vide".into());
    }
    let (pts, s, _) = resample(&raw, &[], PLANNED_STEP_M);
    let gps = RawGps::identity(pts.clone(), s.clone());
    Ok(PersoCase {
        id: p.id.clone(),
        groupe: format!("planifie_{}", p.role),
        serie,
        baro_fiable: false,
        baro_noise_m: None,
        parts: vec![pts.iter().map(|q| Pt { lat: q.lat, lon: q.lon }).collect()],
        s_ref: s,
        baro_s: Vec::new(),
        baro_z: Vec::new(),
        baro_inserted: Vec::new(),
        masks: Vec::new(),
        implicit_masks: Vec::new(),
        ring: Vec::new(),
        raw: Some(RawRef { gps: std::sync::Arc::new(gps), shift: 0.0 }),
    })
}

/// Positions d'un cas (points envoyés, tronçons recollés).
pub fn case_points(case: &PersoCase) -> Vec<LatLon> {
    case.parts
        .iter()
        .enumerate()
        .flat_map(|(k, part)| part.iter().skip(usize::from(k > 0)).map(|q| LatLon::new(q.lat, q.lon)))
        .collect()
}

/// Contrôles de cohérence du manifest V1 sans fichier privé (`check`) : groupe E2b d'une trace
/// nouvelle (README §4 ter.4, 13e0ed1 : `dplus` si et seulement si D+ MNT ≥ 11 m/km).
pub fn e2b_group_failures(manifest: &Manifest) -> Vec<String> {
    manifest
        .traces
        .iter()
        .filter(|t| manifest.is_v1(&t.id) && t.groupe != "negatif")
        .filter_map(|t| {
            let rate = t.gain_mnt_m? / t.dist_km?;
            ((t.groupe == "dplus") != (rate >= DPLUS_M_PER_KM)).then(|| {
                format!(
                    "{} : groupe {} contraire à la règle E2b d'une trace nouvelle (13e0ed1)",
                    t.id, t.groupe
                )
            })
        })
        .collect()
}

/// Seuil `dplus` du groupe E2b d'une trace nouvelle (`criteria.yaml`, `gain_dem_m_par_km_min`).
pub const DPLUS_M_PER_KM: f64 = 11.0;

/// Défaut attesté d'un négatif Q1 d'une série nouvelle (README §4 ter.4, 930f6e8 et 305abd7) :
/// D+ enregistré < 60 % ou > 1,6 × D+ MNT (`qualify_baro.py`, stream complet).
pub fn q1_attested(gain_baro: Option<f64>, gain_mnt: Option<f64>) -> bool {
    match (gain_baro, gain_mnt) {
        (Some(b), Some(m)) if m > 0.0 => b / m < 0.6 || b / m > 1.6,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temoin_ids_are_recognised() {
        assert!(is_temoin_id("perso-56-s1"));
        assert!(is_temoin_id("pl-e4-cajarc-s0"));
        assert!(!is_temoin_id("nv-231"));
        assert!(!is_temoin_id("pl-s2-tarn"));
        assert!(!is_temoin_id("combe-laval"));
    }

    #[test]
    fn resampling_keeps_both_ends_and_the_step() {
        let pts = [LatLon::new(45.0, 5.0), LatLon::new(45.001, 5.0)];
        let (out, s, ex) = resample(&pts, &[0.0, 100.0], 25.0);
        assert_eq!(out.len(), 6);
        assert!((s[1] - 25.0).abs() < 0.01);
        assert!((s[5] - haversine_m(pts[0], pts[1])).abs() < 1e-6);
        assert!((ex[5] - 100.0).abs() < 1e-9);
    }

    #[test]
    fn planned_series_and_selection() {
        let rows = parse_series(
            "id,type,groupe,serie,passages_masques\npl-a,planifie,E4,par_cas,\npl-b,planifie,S2,tenue_a_l_ecart,\npl-c,planifie,S2,calibration,\n",
        );
        let m = Manifest {
            v1: Some(PersoV1 {
                planifies: ["pl-a", "pl-b", "pl-c"]
                    .iter()
                    .map(|i| Planned { id: (*i).into(), ..Planned::default() })
                    .collect(),
                ..PersoV1::default()
            }),
            ..Manifest::default()
        };
        let cal = select_planned(&m, &rows, &[], Serie::Calibration);
        assert_eq!(cal.cases.iter().map(|c| c.0.id.as_str()).collect::<Vec<_>>(), ["pl-a", "pl-c"]);
        assert_eq!(cal.excluded, ["pl-b"]);
        let held = select_planned(&m, &rows, &[], Serie::TenueALEcart);
        assert_eq!(held.cases.iter().map(|c| c.0.id.as_str()).collect::<Vec<_>>(), ["pl-a", "pl-b"]);
    }

    #[test]
    fn other_series_elements_on_a_track() {
        let pts: Vec<LatLon> = (0..100).map(|k| LatLon::new(45.0 + k as f64 * 0.0001, 5.0)).collect();
        let rows = parse_series(
            "id,type,groupe,serie,passages_masques\nnv-1,ouvrage,,tenue_a_l_ecart,\nnv-2,ouvrage,,tenue_a_l_ecart,\nnv-3,ouvrage,,calibration,\npl-x-s0,temoin,digue,tenue_a_l_ecart,\n",
        );
        let mut refs = BTreeMap::new();
        refs.insert("nv-1".into(), vec![LatLon::new(45.001, 5.0), LatLon::new(45.002, 5.0001)]);
        // Un point à 1 km : pas sur le parcours.
        refs.insert("nv-2".into(), vec![LatLon::new(45.001, 5.0), LatLon::new(45.001, 5.013)]);
        refs.insert("nv-3".into(), vec![LatLon::new(45.001, 5.0)]);
        let got = other_series_on(&pts, &refs, &rows, Serie::Calibration, "pl-x");
        assert_eq!(got, ["nv-1", "pl-x-s0"]);
    }

    /// README §2, point 3 : la série V1 jugée passe en calibration. Traces, itinéraires, témoins
    /// et éléments tenus à l'écart sont lus en calibration ; plus aucun masque ni extrait ; un cas
    /// absent du manifest (série V1.1) n'est jamais sélectionné.
    #[test]
    fn a_judged_v1_series_is_calibration() {
        use crate::manifest::Trace;
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
            traces: vec![
                trace("perso-01", Serie::Calibration, &[]),
                trace("perso-40", Serie::Calibration, &["nv-9"]),
                trace("perso-53", Serie::TenueALEcart, &["nv-1"]),
            ],
            v1: Some(PersoV1 {
                traces: ["perso-40", "perso-53"].iter().map(|s| (*s).to_owned()).collect(),
                planifies: ["pl-r-a", "pl-s2-b"]
                    .iter()
                    .map(|i| Planned { id: (*i).into(), ..Planned::default() })
                    .collect(),
                ..PersoV1::default()
            }),
            mvp_judged: true,
            ..Manifest::default()
        };
        let csv = "id,type,groupe,serie,passages_masques\npl-r-a,planifie,R1_R2,tenue_a_l_ecart,\n\
                   pl-s2-b,planifie,S2,par_cas,\nnv-1,ouvrage,,tenue_a_l_ecart,\nnv-9,ouvrage,,calibration,\n\
                   pl-s2-b-s0,temoin,digue,tenue_a_l_ecart,\n";
        // Avant : perso-53 et pl-r-a tenus à l'écart, nv-1 et le témoin masqués.
        let before = crate::campaign::select_series(&m, &[], &[], Serie::Calibration);
        assert_eq!(before.excluded_held_out, ["perso-53"]);
        let rows = parse_series(csv);
        assert_eq!(select_planned(&m, &rows, &[], Serie::Calibration).excluded, ["pl-r-a"]);
        let mut refs = BTreeMap::new();
        refs.insert("nv-1".into(), vec![LatLon::new(45.001, 5.0)]);
        let pts: Vec<LatLon> = (0..100).map(|k| LatLon::new(45.0 + k as f64 * 0.0001, 5.0)).collect();
        assert_eq!(
            other_series_on(&pts, &refs, &rows, Serie::Calibration, "pl-s2-b"),
            ["nv-1", "pl-s2-b-s0"]
        );
        // Après le jugement V1.
        let m = m.with_v1_judged();
        assert!(m.v1.as_ref().unwrap().judged);
        let after = crate::campaign::select_series(&m, &[], &[], Serie::Calibration);
        assert!(after.excluded_held_out.is_empty());
        assert_eq!(
            after.cases.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["perso-01", "perso-40", "perso-53"]
        );
        assert!(m.traces.iter().all(|t| t.passages_masques.is_empty()), "plus de fenêtres masquées");
        assert!(crate::campaign::select_series(&m, &[], &[], Serie::TenueALEcart).cases.is_empty());
        let mut rows = parse_series(csv);
        judged_rows(&mut rows);
        let cal = select_planned(&m, &rows, &[], Serie::Calibration);
        assert_eq!(cal.cases.iter().map(|c| c.0.id.as_str()).collect::<Vec<_>>(), ["pl-r-a", "pl-s2-b"]);
        assert!(cal.excluded.is_empty());
        assert!(
            other_series_on(&pts, &refs, &rows, Serie::Calibration, "pl-s2-b").is_empty(),
            "plus de masques"
        );
        assert_eq!(element_serie(&rows, "nv-1"), Some(Serie::Calibration));
        // Index V1 (témoins) : calibration une fois jugée.
        let v1 = m.v1.as_ref().unwrap();
        assert_eq!(index_serie(v1, Some("tenue_a_l_ecart")), Some(Serie::Calibration));
        assert_eq!(index_serie(&PersoV1::default(), Some("tenue_a_l_ecart")), Some(Serie::TenueALEcart));
        // Une trace hors du manifest (V1.1) n'est jamais un cas.
        assert!(!after.cases.iter().any(|t| t.id == "perso-130"));
    }

    #[test]
    fn q1_defect_attested_both_ways() {
        assert!(q1_attested(Some(257.0), Some(651.0)));
        assert!(q1_attested(Some(2565.0), Some(1460.0)));
        assert!(!q1_attested(Some(1000.0), Some(1100.0)));
        assert!(!q1_attested(None, Some(1.0)));
    }
}
