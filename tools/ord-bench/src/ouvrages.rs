//! Métriques d'ouvrage du banc (README §3, §4 bis : E3, E4, E5, S1, S2, S3), incréments 4 et 4 bis.
//!
//! Sources, toutes de la série de calibration :
//! - traces perso : passages des ouvrages du corpus (`data/ouvrages-geom.json`, extension perso
//!   hors git) franchis à moins de 40 m de chacun de leurs points de référence ; E3 contre le baro
//!   corrigé localement (décalage = médiane baro − service au sol, entre 1 et 3 km du milieu), E4
//!   contre la droite entre portails, E5 au croisement d'un passage dessous, S1 sur l'emprise, S3 au
//!   milieu de `stn-acces-nord` ;
//! - parcours `test1..4` de vcyclist, envoyés point par point sur les stations de la vérité : E3
//!   contre `B` (médiane MNS, cohérence seulement) sur les ponts « tablier », S1 sur les ponts OSM
//!   `missed` ; balcons de `test4` contre `S` (S3, rapportés) ;
//! - tunnels OSM d'`ouvrages.yaml` (géométrie Overpass de `data/ouvrages-osm/`), chacun entre ses
//!   propres portails, sur le parcours vcyclist qui les franchit (`test4` ou un témoin) ;
//! - témoins vcyclist : E4 et S1 sur les tunnels `N` (hors tunnels d'`ouvrages.yaml` déjà mesurés),
//!   S1 sur les ponts `D`, S2 sur les négatifs (`n*`, `tn*`, `hn*` : longueur typée pont ou tunnel
//!   hors des plages `X`/`x`), E5 sur les stations `O` des négatifs. `p06-normandie` est exclu : il
//!   franchit un ouvrage de la série tenue à l'écart.
//!
//! §4 bis (décision de l'utilisateur, 2026-10-31) :
//! - un ouvrage dont la voie est exclue du graphe vélo par la règle de tags du service
//!   (`bicycle_routable`, T03 §4.2 étape 1) sort de S1, E3, E4 et E5 et est rapporté en
//!   `hors_graphe_velo` (non apparié et `unmatched_range` attendus, erreur sans seuil). Tags lus
//!   dans les tuiles de la campagne (cache du banc), sur la voie de l'ouvrage (élément de même
//!   nature qui suit le mieux la géométrie du corpus), jamais dans le corpus Overpass ; un pont ou un
//!   tunnel du corpus sans voie dans les tuiles est `absent_des_donnees`, rapporté
//!   (`E3_non_couvert`, `E4_non_couvert`) et non compté. Pour E5, c'est la voie du passage dessous
//!   qui compte ; les témoins négatifs dont la trace est hors graphe restent dans S2 et E5 ;
//! - E4 par tunnel OSM ; altitude de portail = médiane du MNT brut des stations entre 10 et 30 m
//!   hors du portail ; bande invalide (autre ouvrage, moins de 3 stations) remplacée par celle du
//!   portail extérieur suivant du même côté, à moins de 1 km, sinon « référence indisponible »,
//!   rapportée et non comptée ; sensibilité à 5, 10, 20 et 30 m, « référence ambiguë » si le
//!   verdict bascule ;
//! - limite des données : ouvrages de `criteria.yaml` (`perimetre.limite_donnees`) comptés, avec
//!   la note `limite_donnees` ;
//! - E3 par pont = médiane, sur les passages, de l'écart au milieu et de l'écart max.
//!
//! Aucune coordonnée, aucun nom ni longueur ou emprise d'ouvrage perso dans les sorties :
//! identifiants `nv-NN` seulement (clés `_…` retirées par `public`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use ord_adapters::Shortbread;
use ord_core::classification::tables::bicycle_routable;
use ord_core::rules::Rules;
use ord_domain::budget::{Deadline, RequestId};
use ord_domain::ctx::RequestCtx;
use ord_domain::geo::{dist_units, haversine_m, project_on_segment};
use ord_domain::request::GainPreset;
use ord_domain::street::{StreetTile, StructureTag};
use ord_domain::tiles::{TileKey, TileOutcome};
use ord_domain::LatLon;
use serde_json::{json, Value};

use crate::metrics::{dplus, median};
use crate::service::{Pt, Service};

/// Nature d'un ouvrage du corpus pour les métriques.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bridge,
    Tunnel,
    /// Passage sous un pont (E5).
    Under,
    /// Ouvrage non tagué (S3).
    Untagged,
}

#[derive(Clone, Debug)]
pub struct Ouvrage {
    pub id: String,
    pub kind: Kind,
    pub length_m: f64,
    /// A, milieu, B.
    pub pts: [LatLon; 3],
    pub perso: bool,
    /// Périmètre vélo, décidé sur les tuiles de la campagne (§4 bis.1).
    pub perimeter: Perimeter,
    /// Deux voies candidates à moins de 3 m l'une de l'autre en médiane donnent des périmètres
    /// différents : rapporté (`perimetre_ambigu`).
    pub perimeter_ambiguous: bool,
    pub chain: Vec<u64>,
    /// Une way de l'ouvrage porte `tunnel=avalanche_protector` dans le corpus : le schéma
    /// Shortbread n'a pas cette valeur, la galerie y est une voie ordinaire (§4 bis.4).
    pub avalanche_protector: bool,
}

/// Périmètre vélo d'un ouvrage du corpus, lu dans les tuiles de la campagne (couche `streets` :
/// `kind`, `bicycle`, `bridge`, `tunnel`), jamais dans le corpus Overpass (§4 bis.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Perimeter {
    /// Voie de l'ouvrage (ou du passage dessous) dans le graphe vélo : mesuré.
    InGraph,
    /// Exclue du graphe vélo : `hors_graphe_velo`.
    OffGraph,
    /// Pont ou tunnel sans équivalent dans les tuiles : `absent_des_donnees`, non compté.
    Absent,
    /// Tuile absente du cache : rien n'est décidé, l'ouvrage reste dans le périmètre.
    Unknown,
}

fn kind_of(t: &str) -> Kind {
    if t.contains("passage sous") {
        Kind::Under
    } else if t.contains("non tagué") {
        Kind::Untagged
    } else if t.starts_with("tunnel") || t.starts_with("covered") {
        Kind::Tunnel
    } else {
        Kind::Bridge
    }
}

/// Stations d'un ouvrage du corpus pour la lecture de son périmètre : tous les 5 m le long de sa
/// géométrie Overpass (ways de la chaîne), sinon de la ligne A–milieu–B.
pub fn perimeter_stations(o: &Ouvrage, geoms: &BTreeMap<u64, Vec<LatLon>>) -> Vec<LatLon> {
    let ways: Vec<&Vec<LatLon>> = o.chain.iter().filter_map(|w| geoms.get(w)).collect();
    let lines: Vec<Vec<LatLon>> = if !ways.is_empty() && ways.len() == o.chain.len() {
        ways.into_iter().cloned().collect()
    } else {
        vec![o.pts.to_vec()]
    };
    let mut out = Vec::new();
    for l in &lines {
        for w in l.windows(2) {
            let len = haversine_m(w[0], w[1]);
            let n = (len / 5.0).ceil().max(1.0) as usize;
            for k in 0..n {
                let f = k as f64 / n as f64;
                out.push(LatLon::new(
                    w[0].lat + f * (w[1].lat - w[0].lat),
                    w[0].lon + f * (w[1].lon - w[0].lon),
                ));
            }
        }
        out.extend(l.last().copied());
    }
    out
}

/// Périmètre vélo des ouvrages du corpus, d'après les tuiles de la campagne (§4 bis.1).
pub fn apply_perimeter(ouvrages: &mut [Ouvrage], tiles: &Tiles, geoms: &BTreeMap<u64, Vec<LatLon>>) {
    for o in ouvrages {
        let stations = perimeter_stations(o, geoms);
        (o.perimeter, o.perimeter_ambiguous) = tiles.perimeter(o.pts[1], &stations, o.kind);
    }
}

/// Ouvrages au périmètre ambigu, rapportés (§4 bis.1).
pub fn perimeter_items(ouvrages: &[Ouvrage]) -> Vec<Value> {
    ouvrages
        .iter()
        .filter(|o| o.perimeter_ambiguous)
        .map(|o| json!({"ouvrage": o.id, "metrique": "perimetre_ambigu", "perimetre": format!("{:?}", o.perimeter)}))
        .collect()
}

/// Ouvrage sans équivalent dans les tuiles : rapporté non couvert, non compté (§4 bis.1).
fn absent_item(o: &Ouvrage, case: &str) -> Value {
    let metrique = if o.kind == Kind::Tunnel { "E4_non_couvert" } else { "E3_non_couvert" };
    json!({"ouvrage": o.id, "cas": case, "metrique": metrique, "absent_des_donnees": true,
        "raison": "absent_des_donnees : géométrie du corpus sans équivalent dans les tuiles de la campagne"})
}

/// Notes `limite_donnees` de `criteria.yaml` (`perimetre.limite_donnees.cas_connus`, §4 bis.4) :
/// `(ouvrage, note)`.
pub fn data_limits(bench: &Path) -> anyhow::Result<Vec<(String, String)>> {
    let text = std::fs::read_to_string(bench.join("criteria.yaml")).context("criteria.yaml")?;
    let root: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).context("criteria.yaml illisible")?;
    let cases = &root["perimetre"]["limite_donnees"]["cas_connus"];
    Ok(cases
        .as_sequence()
        .into_iter()
        .flatten()
        .filter_map(|c| Some((c["ouvrage"].as_str()?.to_owned(), c["note"].as_str()?.to_owned())))
        .collect())
}

/// Limites des données lues dans le corpus lui-même (§4 bis.4, règle générale ; `cas_connus` de
/// `criteria.yaml` n'est pas une liste fermée) : toute galerie `tunnel=avalanche_protector`, que le
/// schéma Shortbread rend en voie ordinaire, porte la note de la galerie de l'Arly.
pub fn corpus_data_limits(ouvrages: &[Ouvrage], known: &[(String, String)]) -> Vec<(String, String)> {
    let note = known
        .iter()
        .map(|(_, n)| n)
        .find(|n| n.contains("avalanche_protector"))
        .cloned()
        .unwrap_or_else(|| "shortbread sans avalanche_protector".to_owned());
    let mut out = known.to_vec();
    for o in ouvrages.iter().filter(|o| o.avalanche_protector) {
        if !out.iter().any(|(id, _)| id == &o.id) {
            out.push((o.id.clone(), note.clone()));
        }
    }
    out
}

/// Porte la note `limite_donnees` sur les mesures de ces ouvrages (comptées, §4 bis.4).
pub fn annotate_data_limits(items: &mut [Value], limits: &[(String, String)]) {
    for i in items.iter_mut() {
        let Some(id) = i["ouvrage"].as_str() else { continue };
        let note = limits
            .iter()
            .find(|(o, _)| id == o || id.strip_prefix(o.as_str()).is_some_and(|r| r.starts_with('#')));
        if let Some((_, note)) = note {
            i["limite_donnees"] = json!(note);
        }
    }
}

/// Ouvrages de calibration (publics, puis extension perso si présente).
pub fn calibration_ouvrages(
    paths: &crate::BenchPaths,
    held_out: &BTreeSet<String>,
) -> anyhow::Result<Vec<Ouvrage>> {
    let mut out = Vec::new();
    let mut files = vec![(paths.bench.join("data/ouvrages-geom.json"), false)];
    if paths.private_present() {
        files.push((paths.private.join("ouvrages-perso.json"), true));
    }
    for (f, perso) in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let v: Value = serde_json::from_str(&text).context("géométrie des ouvrages")?;
        for o in v.as_array().into_iter().flatten() {
            let Some(id) = o["id"].as_str() else { continue };
            if held_out.contains(id) || o["serie"].as_str() == Some("tenue_a_l_ecart") {
                continue;
            }
            let p = |k: &str| -> Option<LatLon> {
                let x = &o["pts"][k];
                Some(LatLon::new(x[0].as_f64()?, x[1].as_f64()?))
            };
            let (Some(a), Some(m), Some(b)) = (p("A"), p("milieu"), p("B")) else { continue };
            let chain: Vec<u64> =
                o["chain"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
            let avalanche_protector = chain
                .iter()
                .any(|w| o["tags"][w.to_string()]["tunnel"].as_str() == Some("avalanche_protector"));
            out.push(Ouvrage {
                id: id.to_owned(),
                kind: kind_of(o["type"].as_str().unwrap_or("bridge")),
                length_m: o["length_m"].as_f64().unwrap_or(0.0),
                pts: [a, m, b],
                perso,
                // Décidé sur les tuiles de la campagne (`apply_perimeter`).
                perimeter: Perimeter::Unknown,
                perimeter_ambiguous: false,
                chain,
                avalanche_protector,
            });
        }
    }
    Ok(out)
}

/// Géométrie des ways du corpus (réponses Overpass de `data/ouvrages-osm/`).
pub fn way_geometries(bench: &Path) -> BTreeMap<u64, Vec<LatLon>> {
    let mut out = BTreeMap::new();
    let Ok(dir) = std::fs::read_dir(bench.join("data/ouvrages-osm")) else { return out };
    let mut files: Vec<_> = dir.filter_map(Result::ok).map(|e| e.path()).collect();
    files.sort();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
        for e in v["elements"].as_array().into_iter().flatten() {
            let (Some(id), Some(g)) = (e["id"].as_u64(), e["geometry"].as_array()) else { continue };
            if e["type"] != "way" {
                continue;
            }
            let pts: Vec<LatLon> =
                g.iter().filter_map(|p| Some(LatLon::new(p["lat"].as_f64()?, p["lon"].as_f64()?))).collect();
            if pts.len() >= 2 {
                out.entry(id).or_insert(pts);
            }
        }
    }
    out
}

/// Profil d'un cas sur la grille du service, avec ce qu'en disent les réponses.
#[allow(missing_debug_implementations)]
pub struct CaseProfile<'a> {
    pub d: &'a [f64],
    pub pts: &'a [LatLon],
    pub svc: &'a [f64],
    pub raw: &'a [f64],
    /// Baro transporté, sans décalage (`NaN` = absent ou masqué).
    pub baro: &'a [f64],
    /// Station sur un pont ou un tunnel du service.
    pub on_structure: &'a [bool],
    /// Station sur terrain IGN LiDAR HD.
    pub ign: &'a [bool],
    pub matched: &'a [bool],
    /// Station couverte par un avertissement `unmatched_range`.
    pub unmatched_warn: &'a [bool],
    /// Station sur un pont ou un tunnel des tuiles de la campagne (`Tiles::structure_mask`).
    pub tile_structure: &'a [bool],
}

fn nearest(d: &[f64], pts: &[LatLon], target: LatLon, lo: f64, hi: f64) -> Option<(usize, f64)> {
    (0..d.len())
        .filter(|&i| d[i] >= lo && d[i] <= hi)
        .map(|i| (i, haversine_m(pts[i], target)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

fn r2(x: f64) -> Value {
    if x.is_finite() {
        json!((x * 100.0).round() / 100.0 + 0.0)
    } else {
        Value::Null
    }
}

fn r4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

/// Seuils E3 (`criteria.yaml`) : `|milieu| ≤ 2 m`, `max ≤ 3 m` (< 300 m) ou `≤ 5 m`.
fn e3_pass(length_m: f64, mid: f64, max: f64) -> bool {
    let max_lim = if length_m < 300.0 { 3.0 } else { 5.0 };
    mid.abs() <= 2.0 && max <= max_lim
}

/// D+ (preset `dem`) d'une tranche.
fn dplus_slice(d: &[f64], z: &[f64]) -> f64 {
    dplus(d, z, Rules::default().elevation.gain.preset(GainPreset::Dem))
}

/// Bande de portail : stations entre 10 et 30 m hors du point `p` (`dir` = −1 avant l'entrée, +1
/// après la sortie). `Some(médiane)` si elle est valide : aucune station dans un autre ouvrage
/// (`blocked` : tunnel, galerie, balcon) et au moins 3 stations à MNT lu (§4 bis.2).
fn band(d: &[f64], raw: &[f64], blocked: &[bool], p: usize, dir: f64) -> Option<f64> {
    let ks: Vec<usize> = (0..d.len())
        .filter(|&k| {
            let x = dir * (d[k] - d[p]);
            (10.0..=30.0).contains(&x)
        })
        .collect();
    if ks.iter().any(|&k| blocked[k]) {
        return None;
    }
    let z: Vec<f64> = ks.iter().map(|&k| raw[k]).filter(|z| z.is_finite()).collect();
    (z.len() >= 3).then(|| median(&z))
}

/// Tronçon à l'air libre maximal entre deux tunnels OSM pour qu'une bande de portail qui s'y trouve
/// soit invalide (balcon non tagué, README §4 bis.2, `bande_invalide.balcon_entre_tunnels_osm`).
const SHORT_BALCONY_M: f64 = 120.0;

/// Marque dans `blocked` les stations des tronçons à l'air libre de moins de 120 m entre deux
/// tunnels OSM consécutifs du parcours (`tunnels` : stations des portails, entrée et sortie) : sur
/// ces balcons non tagués, le MNT lit la paroi, pas la chaussée (README §4 bis.2).
fn block_short_balconies(d: &[f64], tunnels: &[(usize, usize)], blocked: &mut [bool]) {
    let mut spans = tunnels.to_vec();
    spans.sort_unstable();
    for w in spans.windows(2) {
        let (out, back) = (w[0].1, w[1].0);
        if back > out + 1 && d[back] - d[out] < SHORT_BALCONY_M {
            blocked[out + 1..back].fill(true);
        }
    }
}

/// Portail retenu d'un côté du tunnel dont le portail est `i` : le sien si sa bande est valide,
/// sinon le portail extérieur de l'ouvrage voisin suivant (`blocked`), et ainsi de suite tant que
/// la bande reste à moins de 1 km de `i` (§4 bis.2). Renvoie la station retenue, l'altitude de sa
/// bande et le nombre de bandes sautées ; `None` : référence indisponible.
fn retained_portal(
    d: &[f64],
    raw: &[f64],
    blocked: &[bool],
    i: usize,
    dir: f64,
) -> Option<(usize, f64, usize)> {
    let step = |k: usize| -> Option<usize> {
        if dir < 0.0 {
            k.checked_sub(1)
        } else {
            (k + 1 < d.len()).then_some(k + 1)
        }
    };
    let mut p = i;
    let mut skipped = 0;
    loop {
        if dir * (d[p] - d[i]) + 30.0 > 1_000.0 {
            return None;
        }
        if let Some(z) = band(d, raw, blocked, p, dir) {
            return Some((p, z, skipped));
        }
        skipped += 1;
        // Premier ouvrage voisin plus loin de ce côté, puis la première station après lui.
        let mut k = step(p)?;
        while !blocked[k] {
            if dir * (d[k] - d[i]) > 1_000.0 {
                return None;
            }
            k = step(k)?;
        }
        while blocked[k] {
            k = step(k)?;
        }
        p = k;
    }
}

/// Portail retenu d'un côté d'un tunnel (§4 bis.2).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Portal {
    /// Station du portail retenu (≠ portail du tunnel : bande remplacée).
    at: usize,
    /// Médiane du MNT brut de sa bande 10–30 m.
    z: f64,
    /// Lectures à 5, 10, 20 et 30 m du portail retenu (sensibilité).
    variants: [f64; 4],
    /// Bandes invalides sautées avant celle-ci.
    skipped: usize,
}

/// Altitude d'un portail : médiane du MNT brut des stations entre 10 et 30 m hors du portail
/// retenu (`dir` = −1 avant l'entrée, +1 après la sortie), sur l'approche, et lectures à 5, 10,
/// 20 et 30 m de ce portail ; `blocked` : stations dans un autre ouvrage (tunnel, galerie, balcon).
/// `None` : référence indisponible (§4 bis.2).
fn portal(d: &[f64], raw: &[f64], blocked: &[bool], i: usize, dir: f64) -> Option<Portal> {
    let (p, z, skipped) = retained_portal(d, raw, blocked, i, dir)?;
    let at = |m: f64| -> f64 {
        let target = d[p] + dir * m;
        (0..d.len())
            .filter(|&k| (dir * (d[k] - d[p])) >= 0.0)
            .min_by(|&a, &b| (d[a] - target).abs().total_cmp(&(d[b] - target).abs()))
            .filter(|&k| (d[k] - target).abs() <= 3.0)
            .map_or(f64::NAN, |k| raw[k])
    };
    Some(Portal { at: p, z, variants: [at(5.0), at(10.0), at(20.0), at(30.0)], skipped })
}

/// Pente implicite maximale entre les portails retenus de la référence E4 (`E4.plausibilite`,
/// géométrie routière) ; au-delà, référence indisponible, rapportée, non comptée.
const IMPLICIT_GRADE_MAX: f64 = 0.25;

/// E4 d'un tunnel OSM entre ses portails `i0` (entrée) et `i1` (sortie) sur un parcours ;
/// `blocked` : stations dans un autre ouvrage (bandes de portail invalides, §4 bis.2). L'erreur
/// est mesurée sur le seul tunnel, contre la droite entre les deux portails retenus.
#[allow(clippy::too_many_arguments)]
fn tunnel_item(
    id: &str,
    case: &str,
    d: &[f64],
    svc: &[f64],
    raw: &[f64],
    on: &[bool],
    blocked: &[bool],
    i0: usize,
    i1: usize,
) -> Value {
    let typed = (i0..=i1).filter(|&k| on[k]).count() as f64 / (i1 - i0 + 1) as f64 * (d[i1] - d[i0]);
    let (Some(a), Some(b)) = (portal(d, raw, blocked, i0, -1.0), portal(d, raw, blocked, i1, 1.0)) else {
        // Rapporté, non compté en E4 (pas de `pass`) ; S1 reste mesuré.
        return json!({"ouvrage": id, "cas": case, "metrique": "E4", "reference_indisponible": true,
            "raison": "aucune bande de portail valide à moins de 1 km",
            "S1.emprise_m": r2(d[i1] - d[i0]), "S1.type_m": r2(typed)});
    };
    let (pa, pb) = (a.at, b.at);
    // Plausibilité (§4 bis.2, `E4.plausibilite`) : pente implicite entre les portails retenus.
    let grade = (b.z - a.z).abs() / (d[pb] - d[pa]).abs().max(1e-9);
    if grade > IMPLICIT_GRADE_MAX {
        return json!({"ouvrage": id, "cas": case, "metrique": "E4", "reference_indisponible": true,
            "raison": "pente implicite entre portails retenus au-delà de 25 %",
            "pente_implicite_portails": r4(grade),
            "S1.emprise_m": r2(d[i1] - d[i0]), "S1.type_m": r2(typed)});
    }
    // Droite entre les deux portails retenus, chacun à sa place ; erreur sur le seul tunnel.
    let judge = |za: f64, zb: f64| -> (f64, f64, bool) {
        let line = |k: usize| za + (zb - za) * (d[k] - d[pa]) / (d[pb] - d[pa]).max(1e-9);
        let max = (i0..=i1).map(|k| (svc[k] - line(k)).abs()).fold(f64::NAN, f64::max);
        let lz: Vec<f64> = (i0..=i1).map(line).collect();
        let fict = dplus_slice(&d[i0..=i1], &svc[i0..=i1]) - dplus_slice(&d[i0..=i1], &lz);
        (max, fict, max <= 5.0 && fict <= 5.0)
    };
    let (max, fict, pass) = judge(a.z, b.z);
    let variants: Vec<bool> = (0..4).map(|k| judge(a.variants[k], b.variants[k]).2).collect();
    // Distances curvilignes et altitudes seulement (jamais de coordonnée).
    let side = |p: &Portal, i: usize| {
        json!({"remplacee": p.at != i, "distance_m": r2((d[p.at] - d[i]).abs()),
            "bandes_sautees": p.skipped, "altitude_m": r2(p.z)})
    };
    let ambiguous = variants.iter().any(|v| *v != pass);
    json!({"ouvrage": id, "cas": case, "metrique": "E4", "reference": "droite_portails_mnt_median_10_30m",
        "ecart_max_m": r2(max), "dplus_fictif_m": r2(fict), "pass": max.is_finite() && pass,
        "bande_remplacee": {"entree": side(&a, i0), "sortie": side(&b, i1)},
        "sensibilite_portail_m": {"5": variants[0], "10": variants[1], "20": variants[2], "30": variants[3]},
        "reference_ambigue": ambiguous,
        "pente_implicite_portails": r4(grade),
        "S1.emprise_m": r2(d[i1] - d[i0]), "S1.type_m": r2(typed)})
}

/// Référence d'altitude d'une portion hors graphe : nom et altitudes par station.
type AltRef<'a> = Option<(&'static str, &'a [f64])>;

/// Contrôle d'un ouvrage hors graphe vélo (README §4 bis.1, `hors_graphe_velo.controle`) : il passe
/// si la portion est non appariée avec `unmatched_range`, **ou** appariée à une voie du graphe vélo
/// distincte de celle de l'ouvrage (trace qui longe l'ouvrage sur une piste parallèle). La voie de
/// l'ouvrage étant exclue du graphe (même règle de tags que le service), toute portion appariée
/// l'est à une autre voie : appariée sur toute sa longueur suffit. Une portion en partie appariée
/// échoue. Erreur d'altitude rendue contre la référence (stations non appariées, ou toutes les
/// stations si la portion suit une voie parallèle) ; `null` avec une raison sans référence.
#[allow(clippy::too_many_arguments)]
fn hors_graphe_item(
    id: &str,
    case: &str,
    matched: &[bool],
    warn: &[bool],
    svc: &[f64],
    reference: AltRef<'_>,
    i0: usize,
    i1: usize,
) -> Value {
    let n = (i1 - i0 + 1) as f64;
    let unmatched = (i0..=i1).filter(|&k| !matched[k]).count() as f64 / n;
    let warned = (i0..=i1).any(|k| warn[k]);
    let unmatched_ok = unmatched >= 0.99 && warned;
    let parallel_ok = unmatched <= 0.01;
    let mut item = json!({"ouvrage": id, "cas": case, "metrique": "hors_graphe_velo",
        "part_non_appariee": r4(unmatched), "unmatched_range": warned,
        "controle": unmatched_ok || parallel_ok,
        "controle_par": if unmatched_ok { json!("non_apparie") } else if parallel_ok { json!("voie_parallele_du_graphe") } else { Value::Null }});
    let Some((name, z)) = reference.filter(|(_, z)| (i0..=i1).any(|k| z[k].is_finite())) else {
        item["ecart_max_m"] = Value::Null;
        item["raison_ecart"] = json!("aucune référence d'altitude sur cette portion");
        return item;
    };
    let err = (i0..=i1)
        .filter(|&k| (parallel_ok || !matched[k]) && z[k].is_finite() && svc[k].is_finite())
        .map(|k| (svc[k] - z[k]).abs())
        .fold(f64::NAN, f64::max);
    item["reference"] = json!(name);
    item["ecart_max_m"] = r2(err);
    if !err.is_finite() {
        item["raison_ecart"] = json!("aucune station comparable avec référence");
    }
    item
}

/// Approches du décalage local E3 (§4 bis.3) : de 50 à 300 m de chaque extrémité du pont.
const APPROACH_M: (f64, f64) = (50.0, 300.0);
/// Stations au moins, par côté ; sinon repli sur la règle 1–3 km.
const APPROACH_MIN_STATIONS: usize = 20;
/// Écart maximal entre les décalages des deux côtés (tolérance E3) ; au-delà, référence incertaine.
const APPROACH_SIDES_MAX_DIFF_M: f64 = 2.0;

/// Décalage local d'un passage de pont sur ses approches.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Approaches {
    /// Médianes de (baro − MNT LiDAR) avant et après le pont, et médiane des deux côtés réunis.
    Sides { before: f64, after: f64, offset: f64 },
    /// Moins de 20 stations d'un côté : repli 1–3 km (`decalage_repli_1_3km`).
    Fallback,
}

/// Décalage local E3 (§4 bis.3) : médiane de (baro − MNT LiDAR) sur les approches de 50 à 300 m
/// de chaque côté de l'emprise `[i0, i1]`, hors stations d'un autre ouvrage (service, tuiles, ou
/// emprise `others` d'un autre ouvrage du corpus franchi : accès non tagué compris)
/// et hors fenêtres masquées (baro absent). Le MNT LiDAR est le profil du service appelé sans
/// correction d'ouvrage (`raw`) : baro et MNT seulement, jamais la sortie corrigée.
fn approach_offset(p: &CaseProfile<'_>, i0: usize, i1: usize, others: &[(usize, usize)]) -> Approaches {
    let side = |from: f64, dir: f64| -> Vec<f64> {
        (0..p.d.len())
            .filter(|&i| {
                let x = dir * (p.d[i] - from);
                (APPROACH_M.0..=APPROACH_M.1).contains(&x)
                    && !p.on_structure[i]
                    && !p.tile_structure[i]
                    && !others.iter().any(|&(a, b)| i >= a && i <= b)
            })
            .map(|i| p.baro[i] - p.raw[i])
            .filter(|x| x.is_finite())
            .collect()
    };
    let (before, after) = (side(p.d[i0], -1.0), side(p.d[i1], 1.0));
    if before.len() < APPROACH_MIN_STATIONS || after.len() < APPROACH_MIN_STATIONS {
        return Approaches::Fallback;
    }
    let all: Vec<f64> = before.iter().chain(&after).copied().collect();
    Approaches::Sides { before: median(&before), after: median(&after), offset: median(&all) }
}

/// Mesures d'ouvrage sur les passages d'un cas perso.
pub fn perso_items(case: &str, p: &CaseProfile<'_>, ouvrages: &[Ouvrage], tol: f64) -> Vec<Value> {
    let mut out = Vec::new();
    let n = p.d.len();
    // Passages de chaque ouvrage : (ouvrage, station du milieu, A, B).
    let mut found: Vec<(&Ouvrage, usize, usize, usize)> = Vec::new();
    for o in ouvrages {
        // Un passage par traversée du disque de rayon `tol` autour du milieu : suite de stations
        // dans le disque, coupée dès que la trace en sort (écart de plus de deux pas). Deux
        // traversées proches (aller-retour, pont parcouru deux fois par deux voies parallèles)
        // restent deux passages : chaque ouvrage garde tous ses passages baro (README §2).
        let near_mid: Vec<usize> = (0..n).filter(|&i| haversine_m(p.pts[i], o.pts[1]) <= tol).collect();
        let mut passages: Vec<Vec<usize>> = Vec::new();
        for i in near_mid {
            match passages.last_mut() {
                Some(g) if i <= *g.last().unwrap() + 2 => g.push(i),
                _ => passages.push(vec![i]),
            }
        }
        for (k, g) in passages.iter().enumerate() {
            let Some(&im) = g.iter().min_by(|&&a, &&b| {
                haversine_m(p.pts[a], o.pts[1]).total_cmp(&haversine_m(p.pts[b], o.pts[1]))
            }) else {
                continue;
            };
            let dm = p.d[im];
            let span = o.length_m + 200.0;
            // Extrémités cherchées sur cette traversée seulement : jamais au-delà du point médian
            // qui la sépare de la traversée voisine.
            let lo = k
                .checked_sub(1)
                .map_or(f64::NEG_INFINITY, |j| 0.5 * (p.d[*passages[j].last().unwrap()] + p.d[g[0]]));
            let hi = passages
                .get(k + 1)
                .map_or(f64::INFINITY, |next| 0.5 * (p.d[*g.last().unwrap()] + p.d[next[0]]));
            let (lo, hi) = ((dm - span).max(lo), (dm + span).min(hi));
            let (Some((ia, da)), Some((ib, db))) =
                (nearest(p.d, p.pts, o.pts[0], lo, hi), nearest(p.d, p.pts, o.pts[2], lo, hi))
            else {
                continue;
            };
            if da > tol || db > tol {
                continue;
            }
            found.push((o, im, ia.min(ib), ia.max(ib)));
        }
    }
    // Stations dans un tunnel du corpus franchi (bandes de portail, §4 bis.2).
    let tunnels: Vec<(usize, usize)> =
        found.iter().filter(|f| f.0.kind == Kind::Tunnel).map(|f| (f.2, f.3)).collect();
    for &(o, im, i0, i1) in &found {
        let dm = p.d[im];
        {
            let mut item = serde_json::Map::new();
            item.insert("ouvrage".into(), json!(o.id));
            item.insert("cas".into(), json!(case));
            item.insert("d_milieu_m".into(), r2(dm));
            // Ouvrage perso : longueurs sous des clés `_`, retirées avant le rapport (vie privée).
            let pre = if o.perso { "_" } else { "" };
            item.insert(format!("{pre}longueur_m"), json!(o.length_m));
            // Baro corrigé localement. Pont (E3, §4 bis.3) : médiane de (baro − MNT LiDAR) sur les
            // approches de 50 à 300 m de chaque côté, hors autres ouvrages et fenêtres masquées ;
            // repli sur la règle 1–3 km sans 20 stations d'un côté. Autres natures : 1–3 km.
            let ring: Vec<f64> = (0..n)
                .filter(|&i| {
                    let x = (p.d[i] - dm).abs();
                    (1_000.0..=3_000.0).contains(&x) && !p.on_structure[i]
                })
                .map(|i| p.baro[i] - p.svc[i])
                .filter(|x| x.is_finite())
                .collect();
            let others: Vec<(usize, usize)> =
                found.iter().filter(|f| (f.2, f.3) != (i0, i1)).map(|f| (f.2, f.3)).collect();
            let approaches = (o.kind == Kind::Bridge).then(|| approach_offset(p, i0, i1, &others));
            let offset = match approaches {
                Some(Approaches::Sides { offset, .. }) => offset,
                _ => median(&ring),
            };
            let err = |i: usize| p.svc[i] - (p.baro[i] - offset);
            // Stations d'extrémité exclues du max : la station la plus proche d'un point de
            // référence peut tomber jusqu'à un demi-pas (2,5 m) au-delà de la culée.
            let inner = if i1 > i0 + 2 { i0 + 1..=i1 - 1 } else { i0..=i1 };
            let worst =
                inner.filter(|&i| err(i).is_finite()).max_by(|&a, &b| err(a).abs().total_cmp(&err(b).abs()));
            let max = worst.map_or(f64::NAN, |i| err(i).abs());
            if o.perimeter == Perimeter::Absent {
                out.push(absent_item(o, case));
                continue;
            }
            if o.perimeter == Perimeter::OffGraph && o.kind != Kind::Untagged {
                let baro: Vec<f64> = (0..n).map(|i| p.baro[i] - offset).collect();
                let reference = Some(("baro_corrige_local", baro.as_slice()));
                let mut h =
                    hors_graphe_item(&o.id, case, p.matched, p.unmatched_warn, p.svc, reference, i0, i1);
                h["d_milieu_m"] = r2(dm);
                out.push(h);
                continue;
            }
            if matches!(o.kind, Kind::Bridge | Kind::Tunnel) && i1 > i0 {
                let typed = (i0..=i1).filter(|&i| p.on_structure[i]).count();
                item.insert(format!("{pre}S1.emprise_m"), r2(p.d[i1] - p.d[i0]));
                item.insert(
                    format!("{pre}S1.type_m"),
                    r2(typed as f64 / (i1 - i0 + 1) as f64 * (p.d[i1] - p.d[i0])),
                );
                item.insert("S1.rappel".into(), json!(r4(typed as f64 / (i1 - i0 + 1) as f64)));
            }
            match o.kind {
                Kind::Bridge | Kind::Untagged => {
                    let mid = err(im);
                    item.insert("ecart_milieu_m".into(), r2(mid));
                    item.insert("ecart_max_m".into(), r2(max));
                    if !mid.is_finite() || !max.is_finite() {
                        // Passage sans référence : non compté dans la médiane du pont.
                        let why = if offset.is_finite() {
                            "pas de baro transporté sur l'emprise (points non appariés par le recalage ou à plus de 30 m)"
                        } else {
                            "pas de décalage local (aucune station au sol avec baro entre 1 et 3 km)"
                        };
                        item.insert("raison_ecart".into(), json!(why));
                    }
                    item.insert("ecart_max_d_m".into(), worst.map_or(Value::Null, |i| r2(p.d[i])));
                    item.insert(format!("{pre}emprise_d_m"), json!([r2(p.d[i0]), r2(p.d[i1])]));
                    item.insert("decalage_local_m".into(), r2(offset));
                    match approaches {
                        Some(Approaches::Sides { before, after, .. }) => {
                            item.insert("decalage_approches_m".into(), json!([r2(before), r2(after)]));
                            if (before - after).abs() > APPROACH_SIDES_MAX_DIFF_M {
                                // Rapportée, non comptée (§4 bis.3).
                                item.insert("reference_incertaine".into(), json!(true));
                            }
                        }
                        Some(Approaches::Fallback) => {
                            item.insert("decalage_repli_1_3km".into(), json!(true));
                        }
                        None => {}
                    }
                    if o.kind == Kind::Untagged {
                        item.insert("metrique".into(), json!("S3"));
                        item.insert("service_milieu_m".into(), r2(p.svc[im]));
                        item.insert("baro_corrige_milieu_m".into(), r2(p.baro[im] - offset));
                    } else {
                        let france = p.ign[im];
                        item.insert("metrique".into(), json!(if france { "E3" } else { "E3_hors_france" }));
                        item.insert("reference".into(), json!("baro_corrige_local"));
                    }
                }
                Kind::Tunnel => {
                    let mut blocked: Vec<bool> = (0..n)
                        .map(|k| tunnels.iter().any(|&(a, b)| (a, b) != (i0, i1) && k > a && k < b))
                        .collect();
                    block_short_balconies(p.d, &tunnels, &mut blocked);
                    // Pont ou tunnel des tuiles de la campagne (§4 bis.2), hors du tunnel mesuré.
                    for k in (0..n).filter(|&k| k <= i0 || k >= i1) {
                        blocked[k] |= p.tile_structure[k];
                    }
                    let t = tunnel_item(&o.id, case, p.d, p.svc, p.raw, p.on_structure, &blocked, i0, i1);
                    for (k, v) in t.as_object().into_iter().flatten() {
                        if !k.starts_with("S1.") {
                            item.insert(k.clone(), v.clone());
                        }
                    }
                    // Ouvrage perso : la distance au portail retenu dirait l'emprise des
                    // ouvrages voisins (vie privée) ; gardée sous une clé `_`, retirée du rapport.
                    if o.perso {
                        if let Some(Value::Object(sides)) = item.get_mut("bande_remplacee") {
                            for v in sides.values_mut() {
                                if let Some(m) = v.as_object_mut() {
                                    if let Some(x) = m.remove("distance_m") {
                                        m.insert("_distance_m".into(), x);
                                    }
                                }
                            }
                        }
                    }
                }
                Kind::Under => {
                    let e = (p.svc[im] - p.raw[im]).abs();
                    item.insert("metrique".into(), json!("E5"));
                    item.insert("ecart_croisement_m".into(), r2(e));
                    item.insert("pass".into(), json!(e <= 1.0));
                }
            }
            out.push(Value::Object(item));
        }
    }
    out
}

/// Réponse du service point par point (mode `input`, morceaux de 15 000 points).
#[derive(Clone, Debug, Default)]
pub struct Query {
    pub ele: Vec<f64>,
    /// Station sur un pont ou un tunnel du service.
    pub on: Vec<bool>,
    pub matched: Vec<bool>,
    /// Station couverte par `unmatched_range`.
    pub warn: Vec<bool>,
}

pub async fn query_points(service: &Service, pts: &[Pt], correction: bool) -> anyhow::Result<Query> {
    const CHUNK: usize = 15_000;
    let n = pts.len();
    let mut q =
        Query { ele: vec![f64::NAN; n], on: vec![false; n], matched: vec![true; n], warn: vec![false; n] };
    for (c, chunk) in pts.chunks(CHUNK).enumerate() {
        let r = service
            .route_details(
                chunk,
                json!({"sampling": {"mode": "input"}, "sections": ["elevation", "typology"], "structure_correction": correction}),
            )
            .await?;
        let segs: Vec<(f64, f64, bool, bool)> = r["segments"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| {
                let st = s["structure"].as_str().unwrap_or("");
                (
                    s["from_m"].as_f64().unwrap_or(0.0),
                    s["to_m"].as_f64().unwrap_or(0.0),
                    st == "bridge" || st == "tunnel",
                    s["matched"].as_bool().unwrap_or(true),
                )
            })
            .collect();
        let warns: Vec<(f64, f64)> = r["quality"]["warnings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|w| w["code"] == "unmatched_range")
            .filter_map(|w| Some((w["from_m"].as_f64()?, w["to_m"].as_f64()?)))
            .collect();
        for p in r["points"].as_array().context("points")? {
            let i = c * CHUNK + p["index"].as_u64().context("index")? as usize;
            q.ele[i] = p["ele"].as_f64().unwrap_or(f64::NAN);
            let d = p["d_m"].as_f64().unwrap_or(0.0);
            if let Some(s) = segs.iter().find(|s| d >= s.0 && d < s.1).or(segs.last()) {
                q.on[i] = s.2;
                q.matched[i] = s.3;
            }
            q.warn[i] = warns.iter().any(|w| d >= w.0 - 1e-6 && d <= w.1 + 1e-6);
        }
    }
    Ok(q)
}

/// Vérité d'un témoin ou d'un parcours vcyclist : points, nature par station (et altitude).
struct Stations {
    pts: Vec<Pt>,
    kind: Vec<String>,
    z: Vec<f64>,
}

fn read_stations(path: &Path, kind_col: usize, z_col: Option<usize>) -> anyhow::Result<Stations> {
    let text = std::fs::read_to_string(path).with_context(|| format!("lecture de {}", path.display()))?;
    let mut s = Stations { pts: Vec::new(), kind: Vec::new(), z: Vec::new() };
    for l in text.lines().filter(|l| !l.starts_with('#') && !l.starts_with("s,")) {
        let f: Vec<&str> = l.split(',').collect();
        if f.len() <= kind_col {
            continue;
        }
        s.pts.push(Pt { lat: f[1].parse()?, lon: f[2].parse()? });
        s.kind.push(f[kind_col].trim().to_owned());
        s.z.push(z_col.and_then(|c| f.get(c)).and_then(|x| x.parse().ok()).unwrap_or(f64::NAN));
    }
    Ok(s)
}

fn cum(pts: &[Pt]) -> Vec<f64> {
    let mut d = vec![0.0];
    for w in pts.windows(2) {
        d.push(
            d.last().copied().unwrap_or(0.0)
                + haversine_m(LatLon::new(w[0].lat, w[0].lon), LatLon::new(w[1].lat, w[1].lon)),
        );
    }
    d
}

/// Plage d'ouvrage de `structures.csv` : `(nature, i0, i1, drapeau)`.
type VcSpan = (String, usize, usize, String);

fn vc_spans(bench: &Path, route: &str) -> anyhow::Result<Vec<VcSpan>> {
    let text = std::fs::read_to_string(bench.join("data/vcyclist/analysis/truth/structures.csv"))?;
    let mut out = Vec::new();
    for l in text.lines().skip(1) {
        let f: Vec<&str> = l.split(',').collect();
        if f.len() > 3 && f[0] == route {
            out.push((f[1].to_owned(), f[2].parse()?, f[3].parse()?, f.last().unwrap_or(&"").to_string()));
        }
    }
    Ok(out)
}

/// Témoins vcyclist retenus : `(dossier de vérité, id, rôle)`, `p06-normandie` exclu.
pub fn controls(bench: &Path) -> anyhow::Result<Vec<(String, String, String)>> {
    let mut out = Vec::new();
    for dir in ["controls", "tunnels"] {
        let text =
            std::fs::read_to_string(bench.join(format!("data/vcyclist/analysis/truth/{dir}/index.csv")))?;
        for l in text.lines().skip(1) {
            let f: Vec<&str> = l.split(',').collect();
            if f.len() < 2 || f[0] == "p06-normandie" {
                continue;
            }
            out.push((dir.to_owned(), f[0].to_owned(), f[1].to_owned()));
        }
    }
    Ok(out)
}

fn control_stations(bench: &Path, dir: &str, id: &str) -> anyhow::Result<Stations> {
    read_stations(&bench.join(format!("data/vcyclist/analysis/truth/{dir}/{id}.csv")), 3, None)
}

/// Points d'un témoin (fill-cache).
pub fn control_points(bench: &Path, dir: &str, id: &str) -> anyhow::Result<Vec<Pt>> {
    Ok(control_stations(bench, dir, id)?.pts)
}

/// Tuiles VersaTiles du cache (hors ligne), pour la règle de tags des spans vcyclist.
pub struct Tiles {
    tiles: BTreeMap<TileKey, Option<Arc<StreetTile>>>,
}

impl std::fmt::Debug for Tiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tiles").field("tiles", &self.tiles.len()).finish()
    }
}

impl Tiles {
    pub async fn load(cache: &Path, pts: &[LatLon]) -> anyhow::Result<Self> {
        let config = crate::service::bench_config(cache, true, crate::service::Variant::DEFAULT);
        let chain = ord_server::wiring::typology_chain(&config)?;
        let ctx =
            RequestCtx::new(RequestId("bench".into()), Deadline::after(std::time::Duration::from_secs(600)));
        let keys: Vec<TileKey> = pts
            .iter()
            .map(|p| Shortbread::GRID.tile_of(Shortbread::GRID.to_units(*p)))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let outcomes = chain.store().get_many(&keys, &ctx).await;
        let tiles = keys
            .into_iter()
            .zip(outcomes)
            .map(|(k, o)| (k, if let TileOutcome::Data { tile, .. } = o { Some(tile) } else { None }))
            .collect();
        Ok(Self { tiles })
    }

    /// Périmètre vélo d'un ouvrage du corpus de milieu `mid` et de stations `stations` (§4 bis.1) :
    /// lu sur la voie de l'ouvrage, l'élément de la tuile (attributs identiques) de même nature
    /// (pont ; tunnel ; voie au sol pour un passage dessous) qui suit le mieux la géométrie du
    /// corpus, c'est-à-dire de plus petite médiane de distance aux stations, acceptée à ≤ 15 m ; à
    /// défaut, toute voie avec le même critère (ouvrage que la source ne décrit pas comme tel,
    /// §4 bis.4). Aucune : pont ou tunnel absent des données. Ambigu si un autre candidat du même
    /// lot est à moins de 3 m de médiane en plus et donne l'autre périmètre.
    fn perimeter(&self, mid: LatLon, stations: &[LatLon], kind: Kind) -> (Perimeter, bool) {
        let grid = Shortbread::GRID;
        if !matches!(self.tiles.get(&grid.tile_of(grid.to_units(mid))), Some(Some(_))) || stations.is_empty()
        {
            return (Perimeter::Unknown, false);
        }
        // Distance de chaque station à chaque élément candidat (∞ hors de sa tuile).
        let mut dist: BTreeMap<String, (bool, bool, Vec<f64>)> = BTreeMap::new();
        for (s, p) in stations.iter().enumerate() {
            let u = grid.to_units(*p);
            let mpu = grid.metres_per_unit(p.lat);
            let Some(Some(t)) = self.tiles.get(&grid.tile_of(u)) else { continue };
            for l in &t.lines {
                let a = &t.attrs[l.attr as usize];
                if a.rail || a.highway.is_empty() {
                    continue;
                }
                let d = l
                    .points
                    .windows(2)
                    .map(|w| dist_units(u, project_on_segment(u, w[0], w[1]).point) * mpu)
                    .fold(f64::INFINITY, f64::min);
                if d > 100.0 {
                    continue;
                }
                let same_kind = match a.structure {
                    StructureTag::Bridge => kind == Kind::Bridge,
                    StructureTag::Tunnel | StructureTag::Covered | StructureTag::BuildingPassage => {
                        kind == Kind::Tunnel
                    }
                    _ => kind == Kind::Under,
                };
                let e = dist
                    .entry(format!("{a:?}"))
                    .or_insert_with(|| (same_kind, bicycle_routable(a), vec![f64::INFINITY; stations.len()]));
                e.2[s] = e.2[s].min(d);
            }
        }
        let medians: Vec<(bool, bool, f64)> =
            dist.values().map(|(same, r, d)| (*same, *r, median(d))).filter(|c| c.2 <= 15.0).collect();
        for pool in [true, false] {
            let cands: Vec<&(bool, bool, f64)> = medians.iter().filter(|c| !pool || c.0).collect();
            let Some(best) = cands.iter().min_by(|a, b| a.2.total_cmp(&b.2)) else { continue };
            let ambiguous = cands.iter().any(|c| c.1 != best.1 && c.2 - best.2 < 3.0);
            let p = if best.1 { Perimeter::InGraph } else { Perimeter::OffGraph };
            return (p, ambiguous);
        }
        if matches!(kind, Kind::Bridge | Kind::Tunnel) {
            (Perimeter::Absent, false)
        } else {
            (Perimeter::Unknown, false)
        }
    }

    /// Stations sur un pont ou un tunnel des tuiles de la campagne (`bridge` / `tunnel`, du corpus ou
    /// non, §4 bis.2) : projection à moins de 10 m d'une ligne d'ouvrage, à l'intérieur de la ligne
    /// (une projection sur l'une de ses extrémités ne compte pas : la station 10 m au-delà d'un
    /// portail n'est pas dans ce tunnel). Tuile absente : `false`.
    pub fn structure_mask(&self, pts: &[LatLon]) -> Vec<bool> {
        let grid = Shortbread::GRID;
        pts.iter()
            .map(|p| {
                let u = grid.to_units(*p);
                let mpu = grid.metres_per_unit(p.lat);
                let Some(Some(t)) = self.tiles.get(&grid.tile_of(u)) else { return false };
                t.lines.iter().any(|l| {
                    let a = &t.attrs[l.attr as usize];
                    if !matches!(
                        a.structure,
                        StructureTag::Bridge
                            | StructureTag::Tunnel
                            | StructureTag::Covered
                            | StructureTag::BuildingPassage
                    ) {
                        return false;
                    }
                    let last = l.points.len().saturating_sub(2);
                    l.points.windows(2).enumerate().any(|(k, w)| {
                        let pr = project_on_segment(u, w[0], w[1]);
                        let at_end = (k == 0 && pr.t <= 0.0) || (k == last && pr.t >= 1.0);
                        !at_end && pr.dist2.sqrt() * mpu <= TILE_STRUCTURE_TOL_M
                    })
                })
            })
            .collect()
    }

    /// Les ouvrages (`tunnel` ou pont) à moins de 15 m de `p` sont-ils dans le graphe vélo ?
    /// `None` : aucun ouvrage trouvé (tuile absente ou ouvrage non tagué).
    fn routable(&self, p: LatLon, tunnel: bool) -> Option<bool> {
        let grid = Shortbread::GRID;
        let u = grid.to_units(p);
        let mpu = grid.metres_per_unit(p.lat);
        let t = self.tiles.get(&grid.tile_of(u))?.as_ref()?;
        let mut found: Option<bool> = None;
        for l in &t.lines {
            let a = &t.attrs[l.attr as usize];
            let is = match a.structure {
                StructureTag::Bridge => !tunnel,
                StructureTag::Tunnel | StructureTag::Covered | StructureTag::BuildingPassage => tunnel,
                _ => false,
            };
            if !is {
                continue;
            }
            let near = l
                .points
                .windows(2)
                .any(|w| dist_units(u, project_on_segment(u, w[0], w[1]).point) * mpu <= 15.0);
            if near {
                found = Some(found.unwrap_or(false) || bicycle_routable(a));
            }
        }
        found
    }
}

/// Distance latérale maximale d'une station à une ligne de pont ou de tunnel des tuiles pour
/// qu'elle soit « sur » cet ouvrage (bandes de portail, §4 bis.2).
const TILE_STRUCTURE_TOL_M: f64 = 10.0;

/// Points de tous les parcours vcyclist mesurés (tuiles de rues à charger).
pub fn vc_points(bench: &Path) -> anyhow::Result<Vec<LatLon>> {
    let mut out = Vec::new();
    for route in ["test1", "test2", "test3", "test4"] {
        let s = read_stations(&bench.join(format!("data/vcyclist/analysis/truth/{route}.csv")), 5, Some(3))?;
        out.extend(s.pts.iter().map(|p| LatLon::new(p.lat, p.lon)));
    }
    for (dir, id, _) in controls(bench)? {
        out.extend(control_stations(bench, &dir, &id)?.pts.iter().map(|p| LatLon::new(p.lat, p.lon)));
    }
    Ok(out)
}

/// Un parcours vcyclist interrogé : stations, vérité, service avec et sans correction.
struct Route {
    id: String,
    role: String,
    s: Stations,
    d: Vec<f64>,
    svc: Query,
    raw: Query,
}

/// Tunnel OSM d'`ouvrages.yaml` sur un parcours : groupes de ways tunnel contigus, en stations.
fn tunnel_groups(route: &Route, ways: &[Vec<LatLon>]) -> Option<Vec<(usize, usize)>> {
    let ll: Vec<LatLon> = route.s.pts.iter().map(|p| LatLon::new(p.lat, p.lon)).collect();
    let near = |q: LatLon| -> Option<usize> {
        (0..ll.len())
            .map(|i| (i, haversine_m(ll[i], q)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|x| x.1 <= 20.0)
            .map(|x| x.0)
    };
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for w in ways {
        let (a, b) = (near(w[0])?, near(w[w.len() - 1])?);
        spans.push((a.min(b), a.max(b)));
    }
    spans.sort_unstable();
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for s in spans {
        match groups.last_mut() {
            Some(g) if route.d[s.0] - route.d[g.1] <= 10.0 => g.1 = g.1.max(s.1),
            _ => groups.push(s),
        }
    }
    Some(groups)
}

/// E3, E4, E5, S1, S2, S3 sur les parcours vcyclist (`test1..4` et témoins) et les tunnels OSM
/// d'`ouvrages.yaml`.
pub async fn vc_items(
    service: &Service,
    bench: &Path,
    tiles: &Tiles,
    ouvrages: &[Ouvrage],
    skipped: &mut Vec<Value>,
) -> anyhow::Result<Vec<Value>> {
    let mut routes: Vec<Route> = Vec::new();
    let load = |id: String, role: String, s: Stations| (id, role, s);
    let mut todo = Vec::new();
    for route in ["test1", "test2", "test3", "test4"] {
        let s = read_stations(&bench.join(format!("data/vcyclist/analysis/truth/{route}.csv")), 5, Some(3))?;
        todo.push(load(format!("vc-{route}"), "route".into(), s));
    }
    for (dir, id, role) in controls(bench)? {
        todo.push(load(id.clone(), role, control_stations(bench, &dir, &id)?));
    }
    for (id, role, s) in todo {
        let q = async {
            let svc = query_points(service, &s.pts, true).await?;
            let raw = query_points(service, &s.pts, false).await?;
            anyhow::Ok((svc, raw))
        };
        match q.await {
            Ok((svc, raw)) => {
                let d = cum(&s.pts);
                eprintln!("{id} : mesuré");
                routes.push(Route { id, role, s, d, svc, raw });
            }
            Err(e) => skipped.push(json!({"id": id, "raison": e.to_string()})),
        }
    }
    let ll = |r: &Route, i: usize| LatLon::new(r.s.pts[i].lat, r.s.pts[i].lon);
    let mut out = Vec::new();

    // Tunnels OSM d'`ouvrages.yaml`, chacun entre ses propres portails (§4 bis.2).
    let geoms = way_geometries(bench);
    let mut covered: Vec<(String, usize, usize)> = Vec::new();
    // (ouvrage, parcours, groupes de stations entre portails)
    type Found<'o> = (&'o Ouvrage, usize, Vec<(usize, usize)>);
    let mut found: Vec<Found<'_>> = Vec::new();
    for o in ouvrages.iter().filter(|o| o.kind == Kind::Tunnel && !o.perso) {
        let ways: Vec<Vec<LatLon>> = o.chain.iter().filter_map(|w| geoms.get(w).cloned()).collect();
        if ways.is_empty() || ways.len() != o.chain.len() {
            skipped.push(json!({"id": o.id, "raison": "géométrie OSM absente"}));
            continue;
        }
        let Some((ri, groups)) =
            routes.iter().enumerate().find_map(|(ri, r)| tunnel_groups(r, &ways).map(|g| (ri, g)))
        else {
            out.push(json!({"ouvrage": o.id, "metrique": "E4_non_couvert", "raison": "aucun parcours vcyclist ne franchit ce tunnel"}));
            continue;
        };
        for &(i0, i1) in &groups {
            covered.push((routes[ri].id.clone(), i0, i1));
        }
        found.push((o, ri, groups));
    }
    // Stations dans un autre ouvrage, par parcours (bandes de portail, §4 bis.2) : tunnels
    // d'`ouvrages.yaml`, plages `N` des témoins et balcons `S`.
    // Balcons de moins de 120 m entre deux tunnels OSM : bandes invalides.
    let inside: Vec<Vec<bool>> = routes
        .iter()
        .map(|r| {
            let n = r.s.pts.len();
            // Les plages `N` de `test4` couvrent des séries entières (balcons compris) : seules
            // celles des témoins sont des tunnels OSM.
            let osm_n = |i: usize| r.role != "route" && r.s.kind[i] == "N";
            let mut blocked: Vec<bool> = (0..n)
                .map(|i| {
                    osm_n(i)
                        || r.s.kind[i] == "S"
                        || covered.iter().any(|c| c.0 == r.id && i > c.1 && i < c.2)
                })
                .collect();
            // Tunnels OSM du parcours, en stations de portail : ceux d'`ouvrages.yaml` et les
            // plages `N` des témoins (portail = première station hors du tunnel).
            let mut tunnels: Vec<(usize, usize)> =
                covered.iter().filter(|c| c.0 == r.id).map(|c| (c.1, c.2)).collect();
            let mut i = 0;
            while i < n {
                if osm_n(i) {
                    let j = (i..n).take_while(|&k| osm_n(k)).last().unwrap_or(i);
                    tunnels.push((i.saturating_sub(1), (j + 1).min(n - 1)));
                    i = j + 1;
                } else {
                    i += 1;
                }
            }
            block_short_balconies(&r.d, &tunnels, &mut blocked);
            // Pont ou tunnel des tuiles de la campagne (§4 bis.2) ; les stations d'un tunnel
            // mesuré ne sont jamais dans sa propre bande.
            let ll: Vec<LatLon> = r.s.pts.iter().map(|p| LatLon::new(p.lat, p.lon)).collect();
            for (b, t) in blocked.iter_mut().zip(tiles.structure_mask(&ll)) {
                *b |= t;
            }
            blocked
        })
        .collect();
    for (o, ri, groups) in found {
        let r = &routes[ri];
        if o.perimeter == Perimeter::Absent {
            out.push(absent_item(o, &r.id));
            continue;
        }
        for (k, &(i0, i1)) in groups.iter().enumerate() {
            let id = if groups.len() > 1 { format!("{}#{k}", o.id) } else { o.id.clone() };
            if o.perimeter == Perimeter::OffGraph {
                let reference = Some(("vcyclist_T", r.s.z.as_slice()));
                out.push(hors_graphe_item(
                    &id,
                    &r.id,
                    &r.svc.matched,
                    &r.svc.warn,
                    &r.svc.ele,
                    reference,
                    i0,
                    i1,
                ));
            } else {
                out.push(tunnel_item(
                    &id,
                    &r.id,
                    &r.d,
                    &r.svc.ele,
                    &r.raw.ele,
                    &r.svc.on,
                    &inside[ri],
                    i0,
                    i1,
                ));
            }
        }
        // Balcons non tagués entre les tunnels d'une série (S3, référence vcyclist `S`).
        for w in groups.windows(2) {
            let (a, b) = (w[0].1 + 1, w[1].0.saturating_sub(1));
            let s: Vec<usize> = (a..=b).filter(|&i| r.s.kind.get(i).is_some_and(|k| k == "S")).collect();
            if s.is_empty() || r.s.z.iter().all(|z| z.is_nan()) {
                continue;
            }
            let max = s.iter().map(|&i| (r.svc.ele[i] - r.s.z[i]).abs()).fold(f64::NAN, f64::max);
            out.push(json!({"ouvrage": format!("{}#balcon", o.id), "cas": r.id, "metrique": "S3",
                "reference": "vcyclist_S", "ecart_max_m": r2(max)}));
        }
    }
    let is_covered =
        |id: &str, i0: usize, i1: usize| covered.iter().any(|c| c.0 == id && c.1 <= i1 && i0 <= c.2);

    for (ri, r) in routes.iter().enumerate() {
        let n = r.s.pts.len();
        let span_typed = |i0: usize, i1: usize| {
            (i0..=i1).filter(|&i| r.svc.on[i]).count() as f64 / (i1 - i0 + 1) as f64 * (r.d[i1] - r.d[i0])
        };
        if r.role == "route" {
            let route = r.id.trim_start_matches("vc-");
            for (k, (kind, i0, i1, flag)) in vc_spans(bench, route)?.into_iter().enumerate() {
                if i1 >= n || i1 <= i0 {
                    continue;
                }
                let id = format!("{}#{k}", r.id);
                let mid = ll(r, (i0 + i1) / 2);
                match kind.as_str() {
                    // Balcons de `test4` : S3 contre `S`.
                    "balcony" => {
                        let s: Vec<usize> = (i0..=i1).filter(|&i| r.s.kind[i] == "S").collect();
                        if !s.is_empty() {
                            let max =
                                s.iter().map(|&i| (r.svc.ele[i] - r.s.z[i]).abs()).fold(f64::NAN, f64::max);
                            out.push(json!({"ouvrage": id, "cas": r.id, "metrique": "S3", "reference": "vcyclist_S", "ecart_max_m": r2(max)}));
                        }
                    }
                    // Tunnels : mesurés par tunnel OSM ci-dessus (série de `structures.csv` découpée).
                    "tunnel" => {}
                    "bridge" | "missed" => {
                        if tiles.routable(mid, false) == Some(false) {
                            let reference = Some(("vcyclist_T", r.s.z.as_slice()));
                            out.push(hors_graphe_item(
                                &id,
                                &r.id,
                                &r.svc.matched,
                                &r.svc.warn,
                                &r.svc.ele,
                                reference,
                                i0,
                                i1,
                            ));
                            continue;
                        }
                        let mut item = json!({"ouvrage": id, "cas": r.id, "nature": kind});
                        // S1 sur les emprises OSM (`missed`) ; les plages `bridge` sont celles du
                        // détecteur de vcyclist (accès nord non tagué compris).
                        if kind == "missed" {
                            item["S1.emprise_m"] = r2(r.d[i1] - r.d[i0]);
                            item["S1.type_m"] = r2(span_typed(i0, i1));
                        } else if flag.trim() == "tablier" {
                            // Partie taguée seulement (stations `B` typées pont).
                            let b: Vec<usize> =
                                (i0..=i1).filter(|&i| r.s.kind[i] == "B" && r.svc.on[i]).collect();
                            if !b.is_empty() {
                                let im = b[b.len() / 2];
                                let mid_e = r.svc.ele[im] - r.s.z[im];
                                let max = b
                                    .iter()
                                    .map(|&i| (r.svc.ele[i] - r.s.z[i]).abs())
                                    .fold(f64::NAN, f64::max);
                                item["metrique"] = json!("E3");
                                item["reference"] = json!("mns_mediane_25m_coherence");
                                item["longueur_m"] = r2(r.d[i1] - r.d[i0]);
                                item["ecart_milieu_m"] = r2(mid_e);
                                item["ecart_max_m"] = r2(max);
                            }
                        }
                        out.push(item);
                    }
                    _ => {}
                }
            }
            continue;
        }
        let negative = r.role.contains("negative");
        let mut spans: Vec<(String, usize, usize)> = Vec::new();
        for i in 0..n {
            match spans.last_mut() {
                Some(sp) if sp.0 == r.s.kind[i] => sp.2 = i,
                _ => spans.push((r.s.kind[i].clone(), i, i)),
            }
        }
        for (k, (kind, i0, i1)) in spans.iter().cloned().enumerate() {
            let id = format!("{}#{k}", r.id);
            let mid = ll(r, (i0 + i1) / 2);
            if kind == "N" && !is_covered(&r.id, i0, i1) {
                if tiles.routable(mid, true) == Some(false) {
                    let reference = Some(("vcyclist_T", r.s.z.as_slice()));
                    out.push(hors_graphe_item(
                        &id,
                        &r.id,
                        &r.svc.matched,
                        &r.svc.warn,
                        &r.svc.ele,
                        reference,
                        i0,
                        i1,
                    ));
                } else {
                    // Plages `N` : la station du portail est la première hors du tunnel.
                    let (a, b) = (i0.saturating_sub(1), (i1 + 1).min(n - 1));
                    out.push(tunnel_item(
                        &id,
                        &r.id,
                        &r.d,
                        &r.svc.ele,
                        &r.raw.ele,
                        &r.svc.on,
                        &inside[ri],
                        a,
                        b,
                    ));
                }
            } else if kind == "D" && !negative {
                if tiles.routable(mid, false) == Some(false) {
                    let reference = Some(("vcyclist_T", r.s.z.as_slice()));
                    out.push(hors_graphe_item(
                        &id,
                        &r.id,
                        &r.svc.matched,
                        &r.svc.warn,
                        &r.svc.ele,
                        reference,
                        i0,
                        i1,
                    ));
                } else {
                    out.push(json!({"ouvrage": id, "cas": r.id, "nature": "bridge",
                        "S1.emprise_m": r2(r.d[i1] - r.d[i0]), "S1.type_m": r2(span_typed(i0, i1))}));
                }
            } else if kind == "O" && negative {
                // La trace du négatif reste dans E5 même hors graphe (§4 bis.1).
                let e = (i0..=i1).map(|i| (r.svc.ele[i] - r.raw.ele[i]).abs()).fold(f64::NAN, f64::max);
                out.push(json!({"ouvrage": id, "cas": r.id, "metrique": "E5", "ecart_croisement_m": r2(e), "pass": e <= 1.0}));
            }
        }
        if negative {
            // S2 : longueur typée pont ou tunnel hors des plages `X`/`x` (ouvrage propre exclu).
            let (mut fp_m, mut fictifs) = (0.0, 0usize);
            let mut run: Option<(usize, usize)> = None;
            let flush = |rr: Option<(usize, usize)>, fictifs: &mut usize| {
                if let Some((a, b)) = rr {
                    let bad = (a..=b).filter(|&i| !matches!(r.s.kind[i].as_str(), "X" | "x")).count();
                    if bad * 2 > b - a + 1 {
                        *fictifs += 1;
                    }
                }
            };
            for i in 0..n {
                if r.svc.on[i] {
                    if !matches!(r.s.kind[i].as_str(), "X" | "x") && i > 0 {
                        fp_m += r.d[i] - r.d[i - 1];
                    }
                    run = Some(run.map_or((i, i), |x| (x.0, i)));
                } else {
                    flush(run.take(), &mut fictifs);
                }
            }
            flush(run, &mut fictifs);
            out.push(json!({"cas": r.id, "metrique": "S2", "longueur_m": r2(r.d[n - 1]),
                "faux_positif_m": r2(fp_m), "ouvrages_fictifs": fictifs}));
        }
    }
    Ok(out)
}

/// Verdicts E3, E4, E5, S1, S2 et S3 (rapporté), `hors_graphe_velo` (rapporté).
pub fn judge(items: &[Value]) -> (Value, Value) {
    let by = |m: &str| -> Vec<&Value> { items.iter().filter(|i| i["metrique"] == m).collect() };
    let rate = |v: &[&Value]| -> (usize, usize) {
        let judged: Vec<&&Value> = v.iter().filter(|i| i["pass"].is_boolean()).collect();
        (judged.iter().filter(|i| i["pass"] == true).count(), judged.len())
    };
    // E3 par pont : médiane, sur les passages, de l'écart au milieu et de l'écart max (§4 bis.3).
    let mut ponts: BTreeMap<String, (Vec<f64>, Vec<f64>, f64)> = BTreeMap::new();
    for i in by("E3") {
        // Référence incertaine (§4 bis.3) : rapportée, non comptée.
        if i["reference_incertaine"] == true {
            continue;
        }
        let (Some(mid), Some(max)) = (i["ecart_milieu_m"].as_f64(), i["ecart_max_m"].as_f64()) else {
            continue;
        };
        let len = i["longueur_m"].as_f64().or_else(|| i["_longueur_m"].as_f64()).unwrap_or(0.0);
        let e = ponts.entry(i["ouvrage"].as_str().unwrap_or("").to_owned()).or_insert((
            Vec::new(),
            Vec::new(),
            len,
        ));
        e.0.push(mid);
        e.1.push(max);
    }
    let e3: Vec<(String, f64, f64, bool, usize)> = ponts
        .iter()
        .map(|(k, (mid, max, len))| {
            let (m, x) = (median(mid), median(max));
            (k.clone(), m, x, e3_pass(*len, m, x), mid.len())
        })
        .collect();
    let e3_n = e3.len();
    let e3_ok = e3.iter().filter(|p| p.3).count();
    let e3_ponts: Vec<Value> = e3
        .iter()
        .map(|p| json!({"ouvrage": p.0, "ecart_milieu_mediane_m": r2(p.1), "ecart_max_mediane_m": r2(p.2), "passages": p.4, "pass": p.3}))
        .collect();
    let e3_fail: Vec<&String> = e3.iter().filter(|p| !p.3).map(|p| &p.0).collect();
    let e4 = by("E4");
    let (e4_ok, e4_n) = rate(&e4);
    let e4_fail: Vec<&Value> = e4.iter().filter(|i| i["pass"] == false).map(|i| &i["ouvrage"]).collect();
    let e4_amb: Vec<&Value> =
        e4.iter().filter(|i| i["reference_ambigue"] == true).map(|i| &i["ouvrage"]).collect();
    // Rapportés, non comptés (§4 bis.1, §4 bis.2).
    let e4_unavailable: Vec<&Value> =
        e4.iter().filter(|i| i["reference_indisponible"] == true).map(|i| &i["ouvrage"]).collect();
    let not_covered = |m: &str| -> Vec<Value> {
        let mut seen = BTreeSet::new();
        by(m)
            .into_iter()
            .filter(|i| seen.insert(i["ouvrage"].as_str().unwrap_or("").to_owned()))
            .map(|i| {
                let mut i = i.clone();
                if let Some(o) = i.as_object_mut() {
                    o.remove("cas");
                }
                i
            })
            .collect()
    };
    let limits: BTreeMap<&str, &Value> = items
        .iter()
        .filter(|i| i["limite_donnees"].is_string())
        .filter_map(|i| Some((i["ouvrage"].as_str()?, &i["limite_donnees"])))
        .collect();
    let (e5_ok, e5_n) = rate(&by("E5"));
    let e5_fail: Vec<&Value> =
        by("E5").into_iter().filter(|i| i["pass"] == false).map(|i| &i["ouvrage"]).collect();
    let sum = |k: &str| -> f64 {
        items.iter().filter_map(|i| i[k].as_f64().or_else(|| i[format!("_{k}")].as_f64())).sum()
    };
    let (s1_ref, s1_typed) = (sum("S1.emprise_m"), sum("S1.type_m"));
    let s1 = s1_typed / s1_ref.max(1e-9);
    let s2 = by("S2");
    let s2_len: f64 = s2.iter().filter_map(|i| i["longueur_m"].as_f64()).sum();
    let s2_fp: f64 = s2.iter().filter_map(|i| i["faux_positif_m"].as_f64()).sum();
    let s2_fict: u64 = s2.iter().filter_map(|i| i["ouvrages_fictifs"].as_u64()).sum();
    let s2_share = s2_fp / s2_len.max(1e-9);
    let hg = by("hors_graphe_velo");
    let hg_ok = hg.iter().filter(|i| i["controle"] == true).count();
    let verdict = |ok: bool, n: usize| {
        if n == 0 {
            "skipped"
        } else if ok {
            "pass"
        } else {
            "fail"
        }
    };
    let agg = json!({
        "E3": {"ponts": e3_n, "ponts_ok": e3_ok, "part": r4(e3_ok as f64 / e3_n.max(1) as f64), "seuil": 0.90, "par_pont": e3_ponts,
            "references_incertaines": by("E3").into_iter().filter(|i| i["reference_incertaine"] == true).map(strip_private).collect::<Vec<_>>()},
        "E3_hors_france": by("E3_hors_france").into_iter().map(strip_private).collect::<Vec<_>>(),
        "E3_non_couverts": not_covered("E3_non_couvert"),
        "E4": {"tunnels": e4_n, "tunnels_ok": e4_ok, "part": r4(e4_ok as f64 / e4_n.max(1) as f64), "seuil": 0.857, "reference_ambigue": e4_amb, "reference_indisponible": e4_unavailable},
        "E4_non_couverts": not_covered("E4_non_couvert"),
        "limite_donnees": limits,
        "E5": {"croisements": e5_n, "ok": e5_ok},
        "S1": {"emprise_m": r2(s1_ref), "type_m": r2(s1_typed), "rappel": r4(s1), "seuil": 0.95},
        "S2": {"longueur_m": r2(s2_len), "faux_positif_m": r2(s2_fp), "part": r4(s2_share), "ouvrages_fictifs": s2_fict},
        "S3": by("S3").into_iter().map(strip_private).collect::<Vec<_>>(),
        "hors_graphe_velo": {"ouvrages": hg.len(), "controle_ok": hg_ok, "detail": hg.iter().map(|i| strip_private(i)).collect::<Vec<_>>()},
    });
    let verdicts = json!({
        "E3": verdict(e3_ok as f64 >= 0.90 * e3_n as f64, e3_n),
        "E3_echecs": e3_fail,
        "E3_hors_france": if by("E3_hors_france").is_empty() { "skipped" } else { "reported" },
        "E4": verdict(e4_ok as f64 >= 0.857 * e4_n as f64, e4_n),
        "E4_echecs": e4_fail,
        "E5": verdict(e5_ok == e5_n, e5_n),
        "E5_echecs": e5_fail,
        "S1": verdict(s1 >= 0.95, usize::from(s1_ref > 0.0)),
        "S2": verdict(s2_fict == 0 && s2_share <= 0.005, s2.len()),
        "S3": if by("S3").is_empty() { "skipped" } else { "reported" },
        "hors_graphe_velo": if hg.is_empty() { "skipped" } else { "reported" },
    });
    (agg, verdicts)
}

/// Items sans leurs clés privées (`_…`), pour le rapport.
pub fn public(items: &[Value]) -> Vec<Value> {
    items.iter().map(strip_private).collect()
}

/// Valeur sans ses clés privées (`_…`), à tous les niveaux.
fn strip_private(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .filter(|(k, _)| !k.starts_with('_'))
                .map(|(k, x)| (k.clone(), strip_private(x)))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(strip_private).collect()),
        x => x.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn e3_thresholds_by_length() {
        assert!(e3_pass(200.0, 1.9, 2.9));
        assert!(!e3_pass(200.0, 1.0, 3.5));
        assert!(e3_pass(800.0, -1.5, 4.5));
        assert!(!e3_pass(800.0, 2.5, 2.5));
    }

    #[test]
    fn e3_is_the_median_of_the_passages() {
        let p = |mid: f64, max: f64| json!({"ouvrage": "a", "metrique": "E3", "longueur_m": 200.0, "ecart_milieu_m": mid, "ecart_max_m": max});
        // Un passage bruité sur trois ne fait plus échouer le pont.
        let items = vec![
            p(0.5, 1.0),
            p(2.6, 3.4),
            p(1.0, 1.5),
            json!({"metrique": "S2", "longueur_m": 1000.0, "faux_positif_m": 0.0, "ouvrages_fictifs": 0}),
        ];
        let (agg, v) = judge(&items);
        assert_eq!(agg["E3"]["ponts"], 1);
        assert_eq!(v["E3"], "pass");
        assert_eq!(v["S2"], "pass");
        assert_eq!(v["E4"], "skipped");
    }

    #[test]
    fn portal_altitude_is_the_median_of_the_10_30_m_band() {
        // Stations à 5 m ; falaise au portail (station juste avant l'entrée à 50 m), sol à 10 m.
        let d: Vec<f64> = (0..21).map(|k| k as f64 * 5.0).collect();
        let raw: Vec<f64> = (0..21).map(|k| if k == 9 { 50.0 } else { 10.0 }).collect();
        let p = portal(&d, &raw, &[false; 21], 10, -1.0).expect("bande valide");
        assert_eq!(p.z, 10.0);
        assert_eq!(p.variants, [50.0, 10.0, 10.0, 10.0]);
        assert_eq!((p.at, p.skipped), (10, 0));
    }

    #[test]
    fn a_band_over_a_neighbour_is_replaced_by_the_outer_portal() {
        // Stations à 5 m. Tunnel voisin des stations 20 à 26 (balcon de 15 m avant notre entrée à
        // 30) : la bande 10–30 m de l'entrée le chevauche ; la bande retenue est celle de son
        // portail extérieur (station 19), sur le sol à 100 m.
        let d: Vec<f64> = (0..60).map(|k| k as f64 * 5.0).collect();
        let raw: Vec<f64> = (0..60).map(|k| if k < 20 { 100.0 } else { 200.0 }).collect();
        let blocked: Vec<bool> = (0..60).map(|k| (20..=26).contains(&k)).collect();
        assert_eq!(band(&d, &raw, &blocked, 30, -1.0), None);
        let p = portal(&d, &raw, &blocked, 30, -1.0).expect("portail extérieur");
        assert_eq!((p.at, p.z, p.skipped), (19, 100.0, 1));
        // Route à 1 % : la droite passe par chaque portail retenu à sa place ; le tunnel (stations
        // 30 à 40) qui suit la pente n'a que l'écart de la bande à son portail (≤ 0,2 m), et non
        // les 0,75 m d'un portail extérieur ramené à l'entrée du tunnel.
        let slope: Vec<f64> = (0..60).map(|k| k as f64 * 0.05).collect();
        let t = tunnel_item("t", "c", &d, &slope, &slope, &[true; 60], &blocked, 30, 40);
        let entry = &t["bande_remplacee"]["entree"];
        assert_eq!(entry["remplacee"], true);
        assert_eq!(entry["distance_m"], 55.0);
        assert_eq!(entry["bandes_sautees"], 1);
        assert_eq!(t["bande_remplacee"]["sortie"]["remplacee"], false);
        assert!(t["ecart_max_m"].as_f64().unwrap() <= 0.2, "{t}");
        // Moins de 3 stations à MNT lu : invalide aussi.
        let mut sparse = raw.clone();
        (31..=37).for_each(|k| sparse[k] = f64::NAN);
        assert_eq!(band(&d, &sparse, &[false; 60], 30, 1.0), None);
    }

    #[test]
    fn no_valid_band_within_1_km_is_reported_not_counted() {
        // Entrée à la station 230 (1 150 m) ; ouvrages voisins partout sur le premier kilomètre.
        let d: Vec<f64> = (0..300).map(|k| k as f64 * 5.0).collect();
        let raw = vec![10.0; 300];
        let svc = vec![10.0; 300];
        let blocked: Vec<bool> = (0..300).map(|k| k < 229 && k % 40 != 0).collect();
        let t = tunnel_item("t", "c", &d, &svc, &raw, &[true; 300], &blocked, 230, 260);
        assert_eq!(t["reference_indisponible"], true);
        assert!(t.get("pass").is_none());
        let (agg, v) = judge(&[t]);
        assert_eq!(agg["E4"]["tunnels"], 0);
        assert_eq!(agg["E4"]["reference_indisponible"], json!(["t"]));
        assert_eq!(v["E4"], "skipped");
    }

    #[test]
    fn perimeter_is_read_in_the_campaign_tiles() {
        use ord_domain::street::{StreetLine, SurfaceTag, WayAttrs};
        let grid = Shortbread::GRID;
        let mid = LatLon::new(47.2, -1.5);
        let u = grid.to_units(mid);
        let key = grid.tile_of(u);
        let way = |kind: &str, bicycle: Option<&str>, structure: StructureTag| WayAttrs {
            highway: Arc::from(kind),
            surface: SurfaceTag::Absent,
            tracktype: None,
            bicycle: bicycle.map(Arc::from),
            service: None,
            link: false,
            rail: false,
            structure,
            layer: None,
            smoothness: None,
            osm_id: None,
            raw: None,
        };
        // Une voie est-ouest décalée de `north` unités (≈ 0,4 m l'unité).
        let tiles_with = |ways: Vec<(WayAttrs, f64)>| {
            let mut t = StreetTile::empty(key);
            for (k, (a, north)) in ways.into_iter().enumerate() {
                t.attrs.push(a);
                t.lines.push(StreetLine {
                    attr: k as u32,
                    points: vec![[u[0] - 50.0, u[1] + north], [u[0] + 50.0, u[1] + north]],
                    cut: [false, false],
                });
            }
            Tiles { tiles: BTreeMap::from([(key, Some(Arc::new(t)))]) }
        };
        // Stations du corpus le long de l'axe (y = 0).
        let stations: Vec<LatLon> = (-8..=8).map(|k| grid.to_latlon([u[0] + 5.0 * k as f64, u[1]])).collect();
        let read = |ways: Vec<(WayAttrs, f64)>, kind: Kind| tiles_with(ways).perimeter(mid, &stations, kind);
        let closed = || (way("trunk", Some("no"), StructureTag::Bridge), 0.0);
        assert_eq!(read(vec![closed()], Kind::Bridge), (Perimeter::OffGraph, false));
        // Bandes de portail (§4 bis.2) : sur la ligne de pont, oui ; au-delà de son extrémité ou à
        // plus de 10 m de côté, non.
        let on_mask = tiles_with(vec![closed()]).structure_mask(&[
            mid,
            grid.to_latlon([u[0] + 60.0, u[1]]),
            grid.to_latlon([u[0], u[1] + 40.0]),
        ]);
        assert_eq!(on_mask, vec![true, false, false]);
        // Un pont cyclable voisin (≈ 10 m) ne change rien : la voie de l'ouvrage est celle qui
        // suit le mieux sa géométrie.
        let cycle = |north: f64| (way("cycleway", None, StructureTag::Bridge), north);
        assert_eq!(read(vec![closed(), cycle(25.0)], Kind::Bridge), (Perimeter::OffGraph, false));
        // Accolé (< 3 m de médiane) avec l'autre périmètre : ambigu, rapporté.
        assert_eq!(read(vec![closed(), cycle(2.0)], Kind::Bridge), (Perimeter::OffGraph, true));
        // Le pont cyclable sur l'axe et la route fermée au sol à côté : dans le graphe.
        let ground = (way("trunk", Some("no"), StructureTag::None), 0.5);
        assert_eq!(read(vec![ground, cycle(0.0)], Kind::Bridge), (Perimeter::InGraph, false));
        // Tunnel que la source rend en voie ordinaire : dans le périmètre (limite des données).
        let plain = (way("primary", None, StructureTag::None), 0.0);
        assert_eq!(read(vec![plain], Kind::Tunnel), (Perimeter::InGraph, false));
        // Aucune voie à 15 m : absent des données ; tuile hors cache : rien n'est décidé.
        let far = (way("primary", None, StructureTag::Bridge), 200.0);
        assert_eq!(read(vec![far], Kind::Bridge), (Perimeter::Absent, false));
        let other = LatLon::new(48.0, 2.0);
        assert_eq!(
            tiles_with(vec![closed()]).perimeter(other, &stations, Kind::Bridge).0,
            Perimeter::Unknown
        );
    }

    #[test]
    fn off_graph_error_is_measured_against_a_reference_or_null() {
        // Portion non appariée (stations 1 à 3) : l'altitude rendue est le MNT brut.
        let matched = [true, false, false, false, true];
        let warn = [false, true, true, true, false];
        let svc = [10.0, 12.0, 15.0, 11.0, 10.0];
        let truth = [10.0, 10.0, 10.0, 10.0, 0.0];
        let h = hors_graphe_item("o", "c", &matched, &warn, &svc, Some(("vcyclist_T", &truth)), 0, 4);
        assert_eq!(h["ecart_max_m"], 5.0, "la station appariée n'entre pas : {h}");
        assert_eq!(h["reference"], "vcyclist_T");
        let none = [f64::NAN; 5];
        let h = hors_graphe_item("o", "c", &matched, &warn, &svc, Some(("vcyclist_T", &none)), 0, 4);
        assert!(h["ecart_max_m"].is_null() && h["raison_ecart"].is_string(), "{h}");
        assert_eq!(h["controle"], false, "part non appariée 0,6");
    }

    #[test]
    fn off_graph_control_passes_on_a_parallel_way_of_the_graph() {
        // Trace qui longe l'ouvrage sur une piste parallèle : appariée de bout en bout.
        let matched = [true; 5];
        let warn = [false; 5];
        let svc = [10.0, 11.0, 12.0, 11.0, 10.0];
        let baro = [10.0, 10.0, 10.0, 10.0, 10.0];
        let h = hors_graphe_item("o", "c", &matched, &warn, &svc, Some(("baro_corrige_local", &baro)), 0, 4);
        assert_eq!(h["controle"], true, "{h}");
        assert_eq!(h["controle_par"], "voie_parallele_du_graphe");
        assert_eq!(h["ecart_max_m"], 2.0, "erreur rapportée sur la portion : {h}");
        // Non appariée avec `unmatched_range` : l'autre branche.
        let h = hors_graphe_item("o", "c", &[false; 5], &[true; 5], &svc, None, 0, 4);
        assert_eq!((h["controle"].clone(), h["controle_par"].clone()), (json!(true), json!("non_apparie")));
        assert!(h["ecart_max_m"].is_null() && h["raison_ecart"].is_string());
    }

    #[test]
    fn a_short_balcony_between_two_tunnels_invalidates_the_band() {
        let d: Vec<f64> = (0..121).map(|k| k as f64 * 5.0).collect();
        // Tunnels des stations 20–40 et 60–80 (balcon de 100 m), puis 100–110 (balcon de 100 m).
        // MNT : 100 m partout, paroi à 140 m sur les balcons.
        let raw: Vec<f64> = (0..121)
            .map(|k| if (41..60).contains(&k) || (81..100).contains(&k) { 140.0 } else { 100.0 })
            .collect();
        let svc = vec![100.0; 121];
        let tunnels = [(20, 40), (60, 80), (100, 110)];
        let mut blocked: Vec<bool> =
            (0..121).map(|k| tunnels.iter().any(|&(a, b)| (a, b) != (20, 40) && k > a && k < b)).collect();
        block_short_balconies(&d, &tunnels, &mut blocked);
        let t = tunnel_item("t", "c", &d, &svc, &raw, &[true; 121], &blocked, 20, 40);
        assert_eq!(t["bande_remplacee"]["sortie"]["remplacee"], true, "{t}");
        // Sortie du 1er tunnel, puis entrée et sortie du 2e, entrée du 3e : bande retenue à la
        // sortie du dernier tunnel de la série.
        assert_eq!(t["bande_remplacee"]["sortie"]["bandes_sautees"], 4, "{t}");
        assert_eq!(t["bande_remplacee"]["sortie"]["distance_m"], 350.0, "{t}");
        assert_eq!(t["pass"], true, "{t}");
        // Un tronçon de 120 m ou plus n'est pas un balcon : rien n'est marqué.
        let tunnels = [(20, 40), (64, 80)];
        let mut b = vec![false; 121];
        block_short_balconies(&d, &tunnels, &mut b);
        assert!(b.iter().all(|x| !x));
    }

    #[test]
    fn avalanche_galleries_of_the_corpus_carry_the_data_limit() {
        let mut o = Ouvrage {
            id: "nv-01".into(),
            kind: Kind::Tunnel,
            length_m: 100.0,
            pts: [LatLon::new(47.0, -1.0); 3],
            perso: true,
            perimeter: Perimeter::InGraph,
            perimeter_ambiguous: false,
            chain: vec![1],
            avalanche_protector: true,
        };
        let known = vec![("arly-galerie".to_owned(), "shortbread sans avalanche_protector".to_owned())];
        let l = corpus_data_limits(std::slice::from_ref(&o), &known);
        assert_eq!(l.len(), 2);
        assert_eq!(l[1], ("nv-01".to_owned(), "shortbread sans avalanche_protector".to_owned()));
        o.avalanche_protector = false;
        assert_eq!(corpus_data_limits(&[o], &known).len(), 1);
    }

    #[test]
    fn e3_offset_is_read_on_the_approaches_and_flags_a_baro_step() {
        // Pont des stations 100 à 120 sur une grille de 5 m ; MNT plat à 0, baro décalé de +3 m.
        let n = 221;
        let d: Vec<f64> = (0..n).map(|k| k as f64 * 5.0).collect();
        let pts = vec![LatLon::new(47.0, -1.0); n];
        let raw = vec![0.0; n];
        let flags = vec![false; n];
        let profile_with = |baro: &[f64], others: &[(usize, usize)]| -> Approaches {
            let p = CaseProfile {
                d: &d,
                pts: &pts,
                svc: &raw,
                raw: &raw,
                baro,
                on_structure: &flags,
                ign: &flags,
                matched: &flags,
                unmatched_warn: &flags,
                tile_structure: &flags,
            };
            approach_offset(&p, 100, 120, others)
        };
        let profile = |baro: &[f64]| profile_with(baro, &[]);
        let baro = vec![3.0; n];
        assert_eq!(profile(&baro), Approaches::Sides { before: 3.0, after: 3.0, offset: 3.0 });
        // Saut de baro de 3 m juste avant la culée (hors des approches) : les deux côtés diffèrent.
        let step: Vec<f64> = (0..n).map(|k| if k < 95 { 6.0 } else { 3.0 }).collect();
        match profile(&step) {
            Approaches::Sides { before, after, .. } => {
                assert!((before - after).abs() > APPROACH_SIDES_MAX_DIFF_M, "{before} {after}")
            }
            other => panic!("{other:?}"),
        }
        // Baro absent sur une approche (fenêtre masquée) : repli 1–3 km.
        let gap: Vec<f64> = (0..n).map(|k| if k > 120 { f64::NAN } else { 3.0 }).collect();
        assert_eq!(profile(&gap), Approaches::Fallback);
        // Approche sur un autre ouvrage du corpus (accès non tagué) : stations exclues, repli.
        assert_eq!(profile_with(&baro, &[(40, 95)]), Approaches::Fallback);
    }

    #[test]
    fn an_implausible_implicit_grade_makes_the_reference_unavailable() {
        let d: Vec<f64> = (0..41).map(|k| k as f64 * 5.0).collect();
        // Tunnel des stations 10 à 14 (20 m) ; MNT à 100 m avant, 140 m après : 40 m sur 40 m.
        let raw: Vec<f64> = (0..41).map(|k| if k < 12 { 100.0 } else { 140.0 }).collect();
        let t = tunnel_item("t", "c", &d, &raw, &raw, &[true; 41], &[false; 41], 10, 14);
        assert_eq!(t["reference_indisponible"], true, "{t}");
        assert!(t["pass"].is_null(), "non compté");
        assert!(t["pente_implicite_portails"].as_f64().unwrap() > IMPLICIT_GRADE_MAX);
    }

    #[test]
    fn aggregates_never_carry_private_keys() {
        let items = vec![
            json!({"ouvrage": "nv-01", "cas": "c", "metrique": "E3", "ecart_milieu_m": 0.1, "ecart_max_m": 0.2,
                "reference_incertaine": true, "_emprise_d_m": [1.0, 2.0], "_S1.emprise_m": 50.0}),
            json!({"ouvrage": "nv-02", "cas": "c", "metrique": "S3", "_longueur_m": 10.0}),
        ];
        let (agg, _) = judge(&items);
        assert!(!agg.to_string().contains("\"_"), "{agg}");
    }

    #[test]
    fn private_keys_are_stripped_at_every_level() {
        let i = json!({"ouvrage": "nv-01", "_longueur_m": 1.0,
            "bande_remplacee": {"entree": {"_distance_m": 5.0, "altitude_m": 1.0}}});
        let p = public(&[i]);
        assert_eq!(p[0], json!({"ouvrage": "nv-01", "bande_remplacee": {"entree": {"altitude_m": 1.0}}}));
    }

    #[test]
    fn absent_and_data_limited_structures() {
        let o = Ouvrage {
            id: "a".into(),
            kind: Kind::Bridge,
            length_m: 100.0,
            pts: [LatLon::new(47.0, -1.0); 3],
            perso: false,
            perimeter: Perimeter::Absent,
            perimeter_ambiguous: false,
            chain: Vec::new(),
            avalanche_protector: false,
        };
        let mut items = vec![
            absent_item(&o, "c1"),
            absent_item(&o, "c2"),
            json!({"ouvrage": "g#1", "metrique": "E4", "pass": false}),
            json!({"ouvrage": "gx", "metrique": "E4", "pass": true}),
        ];
        annotate_data_limits(&mut items, &[("g".into(), "note".into())]);
        assert_eq!(items[2]["limite_donnees"], "note");
        assert!(items[3].get("limite_donnees").is_none());
        let (agg, v) = judge(&items);
        assert_eq!(agg["E3_non_couverts"].as_array().map(Vec::len), Some(1));
        assert_eq!(agg["E3"]["ponts"], 0);
        assert_eq!(agg["E4"]["tunnels"], 2, "la galerie reste comptée");
        assert_eq!(agg["limite_donnees"]["g#1"], "note");
        assert_eq!(v["E4"], "fail");
    }

    #[test]
    fn tunnel_reference_flags_an_ambiguous_portal() {
        let d: Vec<f64> = (0..41).map(|k| k as f64 * 5.0).collect();
        // Tunnel des stations 10 à 30, service plat à 10 m ; falaise à 5 m de l'entrée.
        let raw: Vec<f64> = (0..41).map(|k| if k == 9 { 40.0 } else { 10.0 }).collect();
        let svc = vec![10.0; 41];
        let t = tunnel_item("t", "c", &d, &svc, &raw, &[true; 41], &[false; 41], 10, 30);
        assert_eq!(t["pass"], true);
        assert_eq!(t["reference_ambigue"], true, "le portail à 5 m fait basculer le verdict");
    }
}
