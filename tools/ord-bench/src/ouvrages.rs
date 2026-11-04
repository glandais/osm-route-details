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
//! - limite des données (§4 bis.4, décision de l'utilisateur du 2026-11-01) : ouvrage que le
//!   schéma de la source ne type pas (galerie `tunnel=avalanche_protector` du corpus, et ouvrages
//!   de `criteria.yaml` `perimetre.limite_donnees`) : note `limite_donnees`, hors d'E4 (E3 pour un
//!   pont) et de S1, rapporté à part (`limite_donnees`), erreur mesurée sans seuil ;
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
    /// MNH au milieu du corpus (médiane MNS 25 m − MNT LiDAR au milieu), pour le test de niveau
    /// d'un passage de pont (README §2.1, `passage_pont.niveau`) ; `None` sans valeur LiDAR.
    pub mnh_milieu: Option<f64>,
    /// Voie de l'ouvrage dans les tuiles de la campagne (§4 bis.1) : clé de l'élément (attributs
    /// identiques) qui suit le mieux sa géométrie ; sert à l'attribution des passages (§4 ter.2).
    pub voie: Option<String>,
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
        let (perimeter, ambiguous, voie) = tiles.perimeter_and_way(o.pts[1], &stations, o.kind);
        (o.perimeter, o.perimeter_ambiguous, o.voie) = (perimeter, ambiguous, voie);
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
/// `(ouvrage, note)`. Une entrée nomme un ouvrage (`ouvrage`) ou une liste (`ouvrages`).
pub fn data_limits(bench: &Path) -> anyhow::Result<Vec<(String, String)>> {
    let text = std::fs::read_to_string(bench.join("criteria.yaml")).context("criteria.yaml")?;
    let root: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).context("criteria.yaml illisible")?;
    let cases = &root["perimetre"]["limite_donnees"]["cas_connus"];
    Ok(cases
        .as_sequence()
        .into_iter()
        .flatten()
        .flat_map(|c| {
            let note = c["note"].as_str().map(str::to_owned);
            let ids: Vec<String> = match (c["ouvrage"].as_str(), c["ouvrages"].as_sequence()) {
                (Some(id), _) => vec![id.to_owned()],
                (None, Some(list)) => list.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect(),
                _ => Vec::new(),
            };
            ids.into_iter().filter_map(move |id| Some((id, note.clone()?)))
        })
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

/// Porte la note `limite_donnees` sur les mesures de ces ouvrages (hors décompte d'E4/E3 et S1,
/// §4 bis.4 ; voir `judge`).
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
    manifest: &crate::manifest::Manifest,
) -> anyhow::Result<Vec<Ouvrage>> {
    series_ouvrages(paths, manifest, crate::manifest::Serie::Calibration)
}

/// Ouvrages d'une série : ceux de `ouvrages_tenus` (ou marqués `tenue_a_l_ecart`) pour la série
/// tenue à l'écart, les autres pour la calibration. Les autres ne sont jamais lus. Après le
/// jugement du MVP, ses ouvrages tenus à l'écart sont de la calibration (README §2, point 3) ;
/// ouvrages et croisements V1 et V1.1 (`ouvrages-v1.json`, `ouvrages-v11.json`, empreinte
/// vérifiée) selon leur champ `serie`, de la calibration une fois leur série jugée.
/// Un croisement V1 (`passage sous pont`, point `croisement` seul) a A = milieu = B.
pub fn series_ouvrages(
    paths: &crate::BenchPaths,
    manifest: &crate::manifest::Manifest,
    target: crate::manifest::Serie,
) -> anyhow::Result<Vec<Ouvrage>> {
    let held_out = &manifest.ouvrages_tenus;
    let mut out = Vec::new();
    // MNH au milieu des ouvrages publics : `reference.valeurs` d'`ouvrages.yaml`.
    let public_mnh: BTreeMap<String, f64> = std::fs::read_to_string(paths.bench.join("data/ouvrages.yaml"))
        .ok()
        .and_then(|t| serde_yaml_ng::from_str::<Value>(&t).ok())
        .map(|y| {
            y["ouvrages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|o| Some((o["id"].as_str()?.to_owned(), corpus_mnh(o)?)))
                .collect()
        })
        .unwrap_or_default();
    // (fichier, perso, section V1 ou V1.1 : (empreinte, jugée))
    type Section = Option<(String, bool)>;
    let mut files: Vec<(std::path::PathBuf, bool, Section)> =
        vec![(paths.bench.join("data/ouvrages-geom.json"), false, None)];
    if paths.private_present() {
        files.push((paths.private.join("ouvrages-perso.json"), true, None));
        for sec in manifest.sections() {
            files.push((paths.resolve(&sec.ouvrages.0), true, Some((sec.ouvrages.1.clone(), sec.judged))));
        }
    }
    for (f, perso, section) in files {
        let Ok(bytes) = std::fs::read(&f) else { continue };
        if section.as_ref().is_some_and(|(sum, _)| crate::hash::sha256_bytes(&bytes) != *sum) {
            let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            anyhow::bail!("{name} : empreinte différente de celle du manifest");
        }
        let v: Value = serde_json::from_slice(&bytes).context("géométrie des ouvrages")?;
        for o in v.as_array().into_iter().flatten() {
            let Some(id) = o["id"].as_str() else { continue };
            // Série V1 (ou V1.1) jugée (README §2, point 3) : ses ouvrages tenus à l'écart sont de
            // la calibration.
            let judged = section.as_ref().is_some_and(|(_, j)| *j);
            let marked = o["serie"].as_str() == Some("tenue_a_l_ecart") && !judged;
            let is_held_out = if section.is_some() {
                marked
            } else {
                !manifest.mvp_judged && (held_out.contains(id) || marked)
            };
            if is_held_out != (target == crate::manifest::Serie::TenueALEcart) {
                continue;
            }
            let p = |k: &str| -> Option<LatLon> {
                let x = &o["pts"][k];
                Some(LatLon::new(x[0].as_f64()?, x[1].as_f64()?))
            };
            let (a, m, b) = match (p("A"), p("milieu"), p("B"), p("croisement")) {
                (Some(a), Some(m), Some(b), _) => (a, m, b),
                (_, _, _, Some(c)) => (c, c, c),
                _ => continue,
            };
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
                mnh_milieu: corpus_mnh(o)
                    .or_else(|| o["mnh_milieu_m"].as_f64())
                    .or_else(|| public_mnh.get(id).copied()),
                voie: None,
            });
        }
    }
    Ok(out)
}

/// MNH au milieu d'un ouvrage du corpus : médiane MNS 25 m (sinon MNS ponctuel) − MNT LiDAR au
/// milieu ; lu dans `valeurs` (extension perso) ou `reference.valeurs` (géométrie publique).
fn corpus_mnh(o: &Value) -> Option<f64> {
    let v = if o["valeurs"].is_object() { &o["valeurs"] } else { &o["reference"]["valeurs"] };
    let mid = &v["milieu"];
    let mnt = mid["lidar_mnt"].as_f64()?;
    let med = &o["milieu_mns_mediane_25m"];
    let med = if med.is_null() { &v["milieu_mns_mediane_25m"] } else { med };
    let mns = med["valeur"].as_f64().or_else(|| med.as_f64()).or_else(|| mid["lidar_mns"].as_f64())?;
    Some(mns - mnt)
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
    /// Sortie testée du service (mode de la passe), ramenée sur la grille.
    pub svc: &'a [f64],
    /// Références « MNT » (approches E3, test de niveau, E5, bandes de portail E4) : sortie
    /// `structure_correction: false` du mode `distance`, quel que soit le mode testé. Le passage
    /// compté et sa référence sont des propriétés de la trace et des données, pas du mode.
    pub raw: &'a [f64],
    /// Sortie corrigée du mode `distance` : repli 1–3 km du décalage local (baro − service au
    /// sol), égale à `svc` en mode `distance`.
    pub ref_svc: &'a [f64],
    /// Baro transporté, corrigé de son retard (README §4 ter.9), sans décalage (`NaN` = absent
    /// ou masqué).
    pub baro: &'a [f64],
    /// Abscisses (grille `d`) des points bruts de baro gardés, corrigées du retard (s_p − δ,
    /// 085d26e) : hors points insérés écartés et fenêtres masquées (points de baro d'E5, README
    /// §4 ter.7, 6bc77a5).
    pub baro_points: &'a [f64],
    /// Station sur un pont ou un tunnel du service.
    pub on_structure: &'a [bool],
    /// Station sur terrain IGN LiDAR HD.
    pub ign: &'a [bool],
    pub matched: &'a [bool],
    /// Station couverte par un avertissement `unmatched_range`.
    pub unmatched_warn: &'a [bool],
    /// Station sur un pont ou un tunnel des tuiles de la campagne (`Tiles::structure_mask`).
    pub tile_structure: &'a [bool],
    /// Extrait tenu à l'écart : repli 1–3 km lu sur la trace, `(abscisse, baro − MNT)`.
    pub ring: Option<&'a [(f64, f64)]>,
    /// Segments structure (`bridge` / `tunnel`) de la sortie testée, en abscisse de `d` (S1,
    /// README §4 ter.1).
    pub structure_segs: &'a [(f64, f64)],
    /// Points bruts (GPS d'origine) entre deux abscisses de `d` (attribution d'un passage, README
    /// §4 ter.2) ; `None` : pas de trace brute (le passage reste attribué).
    pub raw_gps: Option<&'a dyn Fn(f64, f64) -> Vec<LatLon>>,
    /// Tuiles de la campagne autour des ouvrages (attribution d'un passage).
    pub tiles: Option<&'a Tiles>,
    /// Trace perso fiable : le baro corrigé localement est la référence d'E5 (README §4 ter.7).
    pub baro_fiable: bool,
    /// Bruit baro de la trace (`noise_m`) : référence baro d'E5 incertaine au-delà de 1,0 m
    /// (README §4 quater.2).
    pub baro_noise_m: Option<f64>,
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

/// Tolérance de borne de S1 (README §4 ter.1, `S1.longueur_typee.tolerance_borne_m`) : un demi-pas.
pub const S1_BORDER_TOL_M: f64 = 2.5;

/// Longueur typée d'une emprise `[s_a, s_b]` (README §4 ter.1) : recouvrement, mesuré en continu
/// sur leurs abscisses, de l'emprise par les segments structure du service (`bridge` / `tunnel`,
/// `segs`, plages disjointes) ; un segment qui s'arrête à `S1_BORDER_TOL_M` au plus d'une borne,
/// à l'intérieur de l'emprise, est prolongé jusqu'à elle.
pub fn typed_overlap(segs: &[(f64, f64)], s_a: f64, s_b: f64) -> f64 {
    let (a, b) = (s_a.min(s_b), s_a.max(s_b));
    let mut spans: Vec<(f64, f64)> = segs
        .iter()
        .map(|&(from, to)| {
            let from = if from > a && from - a <= S1_BORDER_TOL_M { a } else { from };
            let to = if to < b && b - to <= S1_BORDER_TOL_M { b } else { to };
            (from.max(a), to.min(b))
        })
        .filter(|(f, t)| t > f)
        .collect();
    spans.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut total = 0.0;
    let mut cur: Option<(f64, f64)> = None;
    for (f, t) in spans {
        match cur {
            Some((cf, ct)) if f <= ct => cur = Some((cf, ct.max(t))),
            Some((cf, ct)) => {
                total += ct - cf;
                cur = Some((f, t));
            }
            None => cur = Some((f, t)),
        }
    }
    if let Some((cf, ct)) = cur {
        total += ct - cf;
    }
    total
}

/// Abscisse continue de `target` sur la polyligne des stations (`d`, `pts`), projetée sur les
/// deux segments qui encadrent la station `i` (la plus proche de `target`).
fn abscissa_near(d: &[f64], pts: &[LatLon], target: LatLon, i: usize) -> f64 {
    let grid = Shortbread::GRID;
    let u = grid.to_units(target);
    let lo = i.saturating_sub(1);
    let hi = (i + 1).min(d.len().saturating_sub(1));
    let mut best = (f64::INFINITY, d[i]);
    for k in lo..hi {
        let (pa, pb) = (grid.to_units(pts[k]), grid.to_units(pts[k + 1]));
        let pr = project_on_segment(u, pa, pb);
        if pr.dist2 < best.0 {
            best = (pr.dist2, d[k] + pr.t.clamp(0.0, 1.0) * (d[k + 1] - d[k]));
        }
    }
    best.1
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
    segs: &[(f64, f64)],
    blocked: &[bool],
    i0: usize,
    i1: usize,
) -> Value {
    // S1 (README §4 ter.1) : recouvrement continu de l'emprise entre les portails.
    let typed = typed_overlap(segs, d[i0], d[i1]);
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
    /// Moins de 20 stations d'un côté : repli 1–3 km (`decalage_repli_1_3km`), avec le nombre de
    /// stations de chaque côté.
    Fallback { before: usize, after: usize },
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
        return Approaches::Fallback { before: before.len(), after: after.len() };
    }
    let all: Vec<f64> = before.iter().chain(&after).copied().collect();
    Approaches::Sides { before: median(&before), after: median(&after), offset: median(&all) }
}

/// Test de niveau d'un passage de pont (README §2.1) : MNH minimal au milieu, demi-fenêtre autour
/// du milieu, stations baro minimales.
const LEVEL_MNH_MIN_M: f64 = 10.0;
const LEVEL_HALF_M: f64 = 12.5;
const LEVEL_MIN_STATIONS: usize = 3;

/// Passage d'un ouvrage sur un cas perso : ouvrage, station du milieu, stations de A et B,
/// emprise `[s_A, s_B]`.
type Passage<'o> = (&'o Ouvrage, usize, usize, usize, (f64, f64));

/// Demi-fenêtre autour d'un croisement V1 où la trace est comparée à la voie du dessous.
const CROSSING_HALF_M: f64 = 25.0;

/// Croisement V1 : un seul point de référence (A = milieu = B).
fn is_crossing(o: &Ouvrage) -> bool {
    o.pts[0] == o.pts[1] && o.pts[1] == o.pts[2]
}

/// Abords d'ouvrage exclus de la longueur comptée d'un témoin V1 (README §4 ter.4, nature d'un
/// témoin planifié : « sa longueur comptée exclut les abords d'ouvrage (100 m) »).
pub const TEMOIN_ABORDS_M: f64 = 100.0;

/// Fenêtre de recherche du début d'un témoin avant sa fin, en longueurs déclarées.
const TEMOIN_DEBUT_FENETRE: f64 = 1.5;

/// Stations du début et de la fin d'un témoin sur un cas, chacune la plus proche à `tol` au plus.
/// La fin est cherchée sur tout le cas, puis, si la longueur déclarée `declared` est connue, le
/// début dans `[d_fin − 1,5 × declared, d_fin]` (à défaut, témoin parcouru à rebours, dans
/// `[d_fin, d_fin + 1,5 × declared]`) : une trace qui repasse par le début ailleurs ne rallonge
/// pas le témoin (décision de l'utilisateur, 2026-11-04 ; `perso-196-s0`, début repassé trois
/// fois). Sans longueur déclarée, début cherché sur tout le cas.
fn temoin_ends(
    d: &[f64],
    pts: &[LatLon],
    debut: LatLon,
    fin: LatLon,
    declared: Option<f64>,
    tol: f64,
) -> Option<(usize, usize)> {
    let within = |x: Option<(usize, f64)>| x.filter(|(_, dist)| *dist <= tol);
    let (ib, _) = within(nearest(d, pts, fin, f64::NEG_INFINITY, f64::INFINITY))?;
    let (ia, _) = match declared {
        Some(l) => {
            let w = TEMOIN_DEBUT_FENETRE * l;
            within(nearest(d, pts, debut, d[ib] - w, d[ib]))
                .or_else(|| within(nearest(d, pts, debut, d[ib], d[ib] + w)))?
        }
        None => within(nearest(d, pts, debut, f64::NEG_INFINITY, f64::INFINITY))?,
    };
    (ia != ib).then_some((ia, ib))
}

/// S2 d'un témoin négatif V1 (README §4 ter.4–6) sur le profil d'un cas : tronçon entre les
/// stations les plus proches de ses extrémités (à 40 m au plus), sur une sortie comme sur un
/// itinéraire planifié, ou cas entier (témoin planifié sans extrémités ; affc041). Sont retirés de la longueur comptée, des ouvrages fictifs et de la longueur typée :
/// les plages hors graphe vélo (§4 ter.5, `closed`, abscisses de `d`) et les abords (100 m) de
/// tout pont ou tunnel des tuiles de la campagne (`tile_structure`) : un ouvrage réel tagué n'est
/// jamais fictif (§4 ter.6). Ouvrage fictif = suite de stations typées pont ou tunnel par le
/// service hors de ces plages. `None` : extrémités non retrouvées sur ce cas. Lecture du banc pour
/// V1 (rapportée dans les notes) : les tags absents du schéma de la source (`covered`,
/// `avalanche_protector`) ne sont pas lus sur ces témoins.
#[allow(clippy::too_many_arguments)]
pub fn temoin_v1_item(
    t: &crate::v1::TemoinV1,
    d: &[f64],
    pts: &[LatLon],
    on_structure: &[bool],
    tile_structure: &[bool],
    closed: &[std::ops::Range<f64>],
    window: Option<&std::ops::Range<f64>>,
    tol: f64,
) -> Option<Vec<Value>> {
    let n = d.len();
    if n < 2 {
        return None;
    }
    let (i0, i1) = match t.ends {
        // Témoin à extrémités, de sortie comme d'itinéraire planifié (README §4 ter.6, affc041) :
        // entre ses extrémités ; seul un témoin sans extrémités couvre tout l'itinéraire.
        Some([a, b]) => {
            let (ia, ib) = temoin_ends(d, pts, a, b, t.declared_m, tol)?;
            (ia.min(ib), ia.max(ib))
        }
        _ => (0, n - 1),
    };
    if window.is_some_and(|w| !(w.contains(&d[i0]) && w.contains(&d[i1]))) {
        return None;
    }
    let real: Vec<f64> = (0..n).filter(|&i| tile_structure[i]).map(|i| d[i]).collect();
    let near_real = |x: f64| {
        let k = real.partition_point(|v| *v < x - TEMOIN_ABORDS_M);
        real.get(k).is_some_and(|v| *v <= x + TEMOIN_ABORDS_M)
    };
    let in_closed = |x: f64| closed.iter().any(|c| c.contains(&x));
    // Ouvrages réels sur la voie du témoin (README §4 ter.6, `temoins-v1-tags.json`) : extrémités
    // projetées sur le tronçon du témoin (station la plus proche, à `tol` au plus) ; plage A–B
    // ± 35 m retirée de S2 et rapportée ; galerie et `covered` (absents du schéma) en limite des
    // données ; témoin sorti si une plage couvre plus de la moitié de sa longueur.
    let mut out = Vec::new();
    let mut real: Vec<(f64, f64)> = Vec::new();
    let mut sorti: Option<String> = None;
    for (k, r) in t.real.iter().enumerate() {
        let near = |p: LatLon| nearest(d, pts, p, d[i0] - tol, d[i1] + tol).filter(|x| x.1 <= tol);
        let limit = match r.tag.as_str() {
            "avalanche_protector" => Some(GALLERY_NOTE),
            "covered" => Some(COVERED_NOTE),
            _ => None,
        };
        let share = t.length_m.filter(|l| *l > 0.0).map(|l| r.length_m / l);
        let mut item = json!({"ouvrage": format!("{}#reel{k}", t.id), "cas": t.id, "temoin": t.id,
            "metrique": "S2_ouvrage_reel", "nature": r.tag, "longueur_m": r.length_m, "marge_m": REAL_STRUCTURE_MARGIN_M,
            "part_du_temoin": share.map_or(Value::Null, |s| json!(r4(s))),
            "limite_donnees": limit.map_or(Value::Null, |n| json!(n))});
        match (near(r.ends[0]), near(r.ends[1])) {
            (Some((a, _)), Some((b, _))) => real.push((d[a].min(d[b]), d[a].max(d[b]))),
            _ => item["non_retrouvee"] = json!(true),
        }
        if share.is_some_and(|s| s > 0.5) && sorti.is_none() {
            sorti =
                Some(format!("ouvrage réel ({}) sur {:.0} % du témoin", r.tag, 100.0 * share.unwrap_or(0.0)));
        }
        out.push(item);
    }
    if let Some(raison) = sorti {
        for it in &mut out {
            it["temoin_sorti"] = json!(true);
        }
        out.push(json!({"cas": t.id, "temoin": t.id, "metrique": "S2_temoin_sorti", "raison": raison}));
        return Some(out);
    }
    let in_real = |x: f64| {
        real.iter().any(|r| x >= r.0 - REAL_STRUCTURE_MARGIN_M && x <= r.1 + REAL_STRUCTURE_MARGIN_M)
    };
    let (mut total, mut fp, mut fictifs, mut closed_m, mut abords_m) = (0.0, 0.0, 0usize, 0.0, 0.0);
    let mut real_m = 0.0;
    let mut in_run = false;
    for i in i0..=i1 {
        let step = if i > i0 { d[i] - d[i - 1] } else { 0.0 };
        let off_graph = in_closed(d[i]);
        let masked_real = in_real(d[i]);
        let abords = near_real(d[i]);
        if off_graph {
            closed_m += step;
        } else if masked_real {
            real_m += step;
        } else if abords {
            abords_m += step;
        }
        if off_graph || masked_real || abords {
            in_run = false;
            continue;
        }
        total += step;
        if on_structure[i] {
            fp += step;
            if !in_run {
                fictifs += 1;
            }
            in_run = true;
        } else {
            in_run = false;
        }
    }
    out.push(json!({"cas": t.id, "temoin": t.id, "trace": t.trace, "metrique": "S2", "nature": t.nature,
        "longueur_m": r2(total), "faux_positif_m": r2(fp), "ouvrages_fictifs": fictifs,
        "hors_graphe_velo_m": r2(closed_m), "ouvrage_reel_masque_m": r2(real_m), "abords_ouvrages_m": r2(abords_m)}));
    Some(out)
}

/// Mesures d'ouvrage sur les passages d'un cas perso.
pub fn perso_items(case: &str, p: &CaseProfile<'_>, ouvrages: &[Ouvrage], tol: f64) -> Vec<Value> {
    let mut out = Vec::new();
    let n = p.d.len();
    // Passages de chaque ouvrage : (ouvrage, station du milieu, A, B, emprise [s_A, s_B]).
    let mut found: Vec<Passage<'_>> = Vec::new();
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
            // Abscisses de A et de B sur la trace (S1, README §4 ter.1) : projection continue.
            let (sa, sb) = (abscissa_near(p.d, p.pts, o.pts[0], ia), abscissa_near(p.d, p.pts, o.pts[2], ib));
            found.push((o, im, ia.min(ib), ia.max(ib), (sa.min(sb), sa.max(sb))));
        }
    }
    // Stations dans un tunnel du corpus franchi (bandes de portail, §4 bis.2).
    let tunnels: Vec<(usize, usize)> =
        found.iter().filter(|f| f.0.kind == Kind::Tunnel).map(|f| (f.2, f.3)).collect();
    for &(o, im, i0, i1, (sa, sb)) in &found {
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
                .map(|i| p.baro[i] - p.ref_svc[i])
                .filter(|x| x.is_finite())
                .collect();
            let others: Vec<(usize, usize)> =
                found.iter().filter(|f| (f.2, f.3) != (i0, i1)).map(|f| (f.2, f.3)).collect();
            let approaches = (o.kind == Kind::Bridge).then(|| approach_offset(p, i0, i1, &others));
            // Extrait tenu à l'écart : le repli lit la trace jusqu'à 3 km (baro − MNT), hors service.
            let ring: Vec<f64> = match p.ring {
                Some(r) => r
                    .iter()
                    .filter(|(s, _)| (1_000.0..=3_000.0).contains(&(s - dm).abs()))
                    .map(|&(_, v)| v)
                    .collect(),
                None => ring,
            };
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
            // Croisement V1 (un seul point, README §4 ter.4 ; data-init : voie du dessous lue dans
            // les tuiles) : le passage n'est un passage dessous que si la trace, à ± 25 m du
            // croisement, suit la voie du dessous au moins aussi bien que tout pont ou tunnel des
            // tuiles (même lecture que l'attribution, §4 ter.2 ; tuiles et trace seulement).
            // Sinon la trace franchit le pont par-dessus : rapporté, hors d'E5.
            if o.kind == Kind::Under && is_crossing(o) {
                let near: Vec<LatLon> =
                    (0..n).filter(|&i| (p.d[i] - dm).abs() <= CROSSING_HALF_M).map(|i| p.pts[i]).collect();
                match (p.tiles, o.voie.as_deref()) {
                    (Some(tiles), Some(voie)) => match tiles.attribution(&near, voie, None) {
                        Some(a) if !a.attributed() => {
                            out.push(json!({"ouvrage": o.id, "cas": case, "metrique": "E5_passage_dessus",
                                "mediane_voie_dessous_m": r2(a.own_m), "mediane_ouvrage_m": a.other_m.map_or(Value::Null, r2)}));
                            continue;
                        }
                        Some(_) => {}
                        None => {
                            item.insert("attribution_indeterminee".into(), json!(true));
                        }
                    },
                    _ => {
                        item.insert("attribution_indeterminee".into(), json!(true));
                    }
                }
            }
            // Attribution par la trace brute (README §4 ter.2) : le passage n'est compté sur ce pont
            // ou ce tunnel que si le GPS d'origine suit sa voie mieux que tout autre pont ou tunnel
            // des tuiles ; sinon rapporté (`passage_sur_ouvrage_parallele`), hors d'E3, E4, S1 et
            // `hors_graphe_velo`. Trace brute et tuiles seulement, jamais le service.
            let mut attribution: Option<Attribution> = None;
            if matches!(o.kind, Kind::Bridge | Kind::Tunnel) {
                if let (Some(raw_gps), Some(tiles)) = (p.raw_gps, p.tiles) {
                    let bridge = (o.kind == Kind::Bridge).then_some(&o.pts);
                    attribution =
                        o.voie.as_deref().and_then(|v| tiles.attribution(&raw_gps(sa, sb), v, bridge));
                    if let Some(a) = attribution.filter(|a| !a.attributed()) {
                        out.push(json!({"ouvrage": o.id, "cas": case, "metrique": "passage_sur_ouvrage_parallele",
                            "mediane_voie_ouvrage_m": r2(a.own_m), "mediane_autre_ouvrage_m": a.other_m.map_or(Value::Null, r2),
                            "points_bruts": a.points}));
                        continue;
                    }
                    if attribution.is_none() {
                        item.insert("attribution_indeterminee".into(), json!(true));
                    }
                }
            }
            if let Some(a) = attribution {
                item.insert(
                    "attribution_mediane_m".into(),
                    json!({"voie_ouvrage": r2(a.own_m), "autre_ouvrage": a.other_m.map_or(Value::Null, r2), "points_bruts": a.points,
                        "voies_du_meme_pont": a.same_bridge_ways}),
                );
            }
            // Passage en hauteur sur un pont (README §2.1, `passage_pont.niveau`) : baro et MNT
            // seulement, jamais le service.
            let mut level_note: Option<&str> = None;
            if o.kind == Kind::Bridge {
                let at_mid: Vec<f64> = (0..n)
                    .filter(|&i| (p.d[i] - dm).abs() <= LEVEL_HALF_M)
                    .map(|i| p.baro[i] - offset - p.raw[i])
                    .filter(|x| x.is_finite())
                    .collect();
                match o.mnh_milieu {
                    Some(mnh) if mnh >= LEVEL_MNH_MIN_M => {
                        if at_mid.len() < LEVEL_MIN_STATIONS {
                            level_note = Some("niveau_sans_baro");
                        } else if median(&at_mid) < 0.5 * mnh {
                            // Trace sous le tablier : le passage n'est pas compté sur ce pont.
                            out.push(
                                json!({"ouvrage": o.id, "cas": case, "metrique": "passage_retire_niveau",
                                "baro_moins_mnt_milieu_m": r2(median(&at_mid)), "mnh_milieu_m": r2(mnh)}),
                            );
                            continue;
                        }
                    }
                    _ => level_note = Some("niveau_non_discriminant"),
                }
            }
            if o.perimeter == Perimeter::OffGraph && o.kind != Kind::Untagged {
                let baro: Vec<f64> = (0..n).map(|i| p.baro[i] - offset).collect();
                let reference = Some(("baro_corrige_local", baro.as_slice()));
                let mut h =
                    hors_graphe_item(&o.id, case, p.matched, p.unmatched_warn, p.svc, reference, i0, i1);
                if let Some(note) = level_note {
                    h[note] = json!(true);
                }
                if item.contains_key("attribution_indeterminee") {
                    h["attribution_indeterminee"] = json!(true);
                }
                h["d_milieu_m"] = r2(dm);
                out.push(h);
                continue;
            }
            if matches!(o.kind, Kind::Bridge | Kind::Tunnel) && i1 > i0 && sb > sa {
                // S1 (README §4 ter.1) : recouvrement continu de [s_A, s_B] par les segments
                // structure du service, tolérance d'un demi-pas à chaque borne.
                let typed = typed_overlap(p.structure_segs, sa, sb);
                item.insert(format!("{pre}S1.emprise_m"), r2(sb - sa));
                item.insert(format!("{pre}S1.type_m"), r2(typed));
                item.insert("S1.rappel".into(), json!(r4(typed / (sb - sa))));
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
                    if let Some(note) = level_note {
                        item.insert(note.into(), json!(true));
                    }
                    match approaches {
                        Some(Approaches::Sides { before, after, .. }) => {
                            item.insert("decalage_approches_m".into(), json!([r2(before), r2(after)]));
                            if (before - after).abs() > APPROACH_SIDES_MAX_DIFF_M {
                                // Rapportée, non comptée (§4 bis.3).
                                item.insert("reference_incertaine".into(), json!(true));
                            }
                        }
                        Some(Approaches::Fallback { before, after }) => {
                            item.insert("decalage_repli_1_3km".into(), json!(true));
                            item.insert("decalage_repli_stations_par_cote".into(), json!([before, after]));
                        }
                        None => {}
                    }
                    if o.kind == Kind::Untagged {
                        item.insert("metrique".into(), json!("S3"));
                        // V1 (README §3, S3) : détecté oui/non = typé pont ou tunnel par le service
                        // au milieu ; part de l'emprise typée, rapportée.
                        item.insert("detecte".into(), json!(p.on_structure[im]));
                        let typed =
                            (i0..=i1).filter(|&i| p.on_structure[i]).count() as f64 / (i1 - i0 + 1) as f64;
                        item.insert("part_typee_ouvrage".into(), json!(r4(typed)));
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
                    let t = tunnel_item(&o.id, case, p.d, p.svc, p.raw, p.structure_segs, &blocked, i0, i1);
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
                    // Référence au croisement (README §4 ter.7) : trace et données, jamais le service.
                    let input = E5Input {
                        d: p.d,
                        pts: p.pts,
                        raw: p.raw,
                        ign: Some(p.ign),
                        baro: p.baro_fiable.then_some(p.baro),
                        baro_points: p.baro_points,
                        tiles: p.tiles,
                        at: o.pts[1],
                        im,
                        ends: [im, im],
                        baro_noise_m: p.baro_noise_m,
                    };
                    let bump = e5_bump(p.d, p.raw, im, [im, im]);
                    let svc = p.svc[im];
                    let ecart = |z: f64| (svc - z).abs();
                    let ecart_mnt = Some((svc - p.raw[im]).abs());
                    match e5_item(&o.id, case, e5_reference(&input), &ecart, ecart_mnt, bump) {
                        Ok(fields) => item.extend(fields),
                        Err(mut v) => {
                            // Abscisse du croisement : sites d'E5 (README §4 quater.4).
                            v["d_milieu_m"] = r2(dm);
                            out.push(v);
                            continue;
                        }
                    }
                }
            }
            out.push(Value::Object(item));
        }
    }
    out
}

/// Champs d'un passage E5 compté (`Ok`), ou item rapporté non compté (`Err`) : retiré
/// (`e5_trace_sur_ouvrage`), référence indisponible (`e5_reference_indisponible`) ou incertaine
/// (`e5_reference_incertaine`, écart indicatif). `ecart` : écart du service à une altitude de
/// référence ; `ecart_mnt` : écart au MNT, rapporté.
fn e5_item(
    id: &str,
    case: &str,
    reference: E5Reference,
    ecart: &dyn Fn(f64) -> f64,
    ecart_mnt: Option<f64>,
    bump: Option<f64>,
) -> Result<serde_json::Map<String, Value>, Value> {
    let bump_v = bump.map_or(Value::Null, r2);
    let sides_json = |m: &mut serde_json::Map<String, Value>, sides: &BaroSides| {
        m.insert("decalages_cotes_m".into(), json!(sides.offset_m.map(|o| o.map_or(Value::Null, r2))));
        m.insert("points_baro_cotes".into(), json!(sides.points));
    };
    let (z, name, offset, sides, tol) = match reference {
        E5Reference::Retire { distance_m, angle_deg } => {
            return Err(json!({"ouvrage": id, "cas": case, "metrique": "e5_trace_sur_ouvrage",
                "distance_ouvrage_m": r2(distance_m), "angle_deg": r2(angle_deg)}));
        }
        E5Reference::Indisponible { raison } => {
            return Err(json!({"ouvrage": id, "cas": case, "metrique": "e5_reference_indisponible",
                "raison": raison, "bosse_mnt_m": bump_v}));
        }
        E5Reference::Incertaine { reference, raison, z, offset_m, sides } => {
            let tol = if reference == "mnt" { E5_TOL_MNT_M } else { E5_TOL_BARO_M };
            let e = z.is_finite().then(|| ecart(z));
            let mut m = serde_json::Map::new();
            m.insert("ouvrage".into(), json!(id));
            m.insert("cas".into(), json!(case));
            m.insert("metrique".into(), json!("e5_reference_incertaine"));
            m.insert("reference".into(), json!(reference));
            m.insert("raison".into(), json!(raison));
            m.insert("bosse_mnt_m".into(), bump_v);
            if let Some(o) = offset_m {
                m.insert("decalage_local_m".into(), r2(o));
            }
            if let Some(s) = &sides {
                sides_json(&mut m, s);
            }
            m.insert("ecart_croisement_m".into(), e.map_or(Value::Null, r2));
            m.insert("ecart_mnt_m".into(), ecart_mnt.map_or(Value::Null, r2));
            m.insert("seuil_m".into(), json!(tol));
            m.insert("pass_indicatif".into(), e.map_or(Value::Null, |e| json!(e <= tol)));
            return Err(Value::Object(m));
        }
        E5Reference::Baro { z, offset_m, sides } => {
            (z, "baro_corrige_local", Some(offset_m), Some(sides), E5_TOL_BARO_M)
        }
        E5Reference::Mnt { z } => (z, "mnt", None, None, E5_TOL_MNT_M),
    };
    let e = ecart(z);
    let mut m = serde_json::Map::new();
    m.insert("metrique".into(), json!("E5"));
    m.insert("reference".into(), json!(name));
    m.insert("ecart_croisement_m".into(), r2(e));
    m.insert("ecart_mnt_m".into(), ecart_mnt.map_or(Value::Null, r2));
    m.insert("bosse_mnt_m".into(), bump_v);
    if let Some(o) = offset {
        m.insert("decalage_local_m".into(), r2(o));
    }
    if let Some(s) = &sides {
        sides_json(&mut m, s);
    }
    m.insert("seuil_m".into(), json!(tol));
    m.insert("pass".into(), json!(e <= tol));
    Ok(m)
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
    /// Segments structure (`bridge` / `tunnel`) en abscisse cumulée des points (S1, §4 ter.1).
    pub segs: Vec<(f64, f64)>,
}

pub async fn query_points(service: &Service, pts: &[Pt], correction: bool) -> anyhow::Result<Query> {
    const CHUNK: usize = 15_000;
    let n = pts.len();
    let mut q = Query {
        ele: vec![f64::NAN; n],
        on: vec![false; n],
        matched: vec![true; n],
        warn: vec![false; n],
        segs: Vec::new(),
    };
    let d_all = cum(pts);
    for (c, chunk) in pts.chunks(CHUNK).enumerate() {
        let offset = d_all[c * CHUNK];
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
        q.segs.extend(segs.iter().filter(|s| s.2).map(|s| (s.0 + offset, s.1 + offset)));
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

/// Note `limite_donnees` d'une galerie `tunnel=avalanche_protector` (§4 bis.4) : la même que celle
/// des galeries du corpus.
const GALLERY_NOTE: &str = "shortbread sans avalanche_protector";

/// Note `limite_donnees` d'une voie `covered=yes` (§4 bis.4, README §4 ter.6).
const COVERED_NOTE: &str = "shortbread sans covered";

/// Marge retirée de part et d'autre d'un ouvrage réel d'un témoin (README §4 ter.6, marge `x`).
const REAL_STRUCTURE_MARGIN_M: f64 = 35.0;

/// Ouvrages réels tagués sur la voie même d'un témoin négatif (README §4 ter.6, précisé en
/// eb4c4cc) : suites de stations consécutives dont la way (colonne `way` des stations, plages X et
/// O comprises) porte, à la base OSM du témoin, `bridge`, `tunnel` (dont `avalanche_protector` :
/// `gallery`) ou `covered=yes`, de même nature ; `(nature, i0, i1)`. `None` si les tags ne sont pas
/// dans le cache de banc (`fill-cache`).
fn own_structures(
    bench: &Path,
    cache: &Path,
    id: &str,
) -> anyhow::Result<Option<Vec<(String, usize, usize)>>> {
    let Some(tags) = crate::temoin_tags::load(cache, id)? else { return Ok(None) };
    let (_, ways) = crate::temoin_tags::station_ways(bench, id)?;
    let mut out: Vec<(String, usize, usize)> = Vec::new();
    for (i, w) in ways.iter().enumerate() {
        let Some(nature) = tags.get(w).and_then(crate::temoin_tags::nature) else { continue };
        match out.last_mut() {
            Some(last) if last.0 == nature && last.2 + 1 == i => last.2 = i,
            _ => out.push((nature.to_owned(), i, i)),
        }
    }
    Ok(Some(out))
}

/// Catégorie d'un témoin (`index.csv`, troisième colonne).
fn control_category(bench: &Path, id: &str) -> anyhow::Result<String> {
    for dir in ["controls", "tunnels"] {
        let text =
            std::fs::read_to_string(bench.join(format!("data/vcyclist/analysis/truth/{dir}/index.csv")))?;
        if let Some(f) = text.lines().skip(1).map(|l| l.split(',').collect::<Vec<_>>()).find(|f| f[0] == id) {
            return Ok(f.get(2).copied().unwrap_or("").to_owned());
        }
    }
    Ok(String::new())
}

/// L'ouvrage réel de cette nature est l'objet même du témoin (README §4 ter.6, précisé en
/// eb4c4cc) : la catégorie vcyclist nomme la nature d'un ouvrage que le témoin **emprunte**
/// (galerie, tunnel, pont, viaduc) ; un ouvrage longé, passé dessous ou dessus (`under`, `above`,
/// `beside`, `over` avant le nom) n'en fait pas l'objet. `covered` n'a pas de nom de catégorie.
fn names_object(category: &str, nature: &str) -> bool {
    let words: Vec<&str> = category.split('-').collect();
    let names = |w: &str| match nature {
        "gallery" => w.starts_with("galler"),
        "tunnel" => w.starts_with("tunnel"),
        "bridge" => w.starts_with("bridge") || w.starts_with("viaduc"),
        _ => false,
    };
    words.iter().enumerate().any(|(k, w)| {
        names(w) && !words[..k].iter().any(|p| matches!(*p, "under" | "above" | "beside" | "over"))
    })
}

fn control_stations(bench: &Path, dir: &str, id: &str) -> anyhow::Result<Stations> {
    read_stations(&bench.join(format!("data/vcyclist/analysis/truth/{dir}/{id}.csv")), 3, None)
}

/// Points d'un témoin (fill-cache).
pub fn control_points(bench: &Path, dir: &str, id: &str) -> anyhow::Result<Vec<Pt>> {
    Ok(control_stations(bench, dir, id)?.pts)
}

/// Attribution d'un passage par la trace brute (README §4 ter.2) : médianes des distances des
/// points bruts de l'emprise à la voie de l'ouvrage et à la plus proche des autres voies `bridge`
/// / `tunnel` des tuiles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Attribution {
    pub own_m: f64,
    pub other_m: Option<f64>,
    pub points: usize,
    /// Voies du même pont réunies à la voie de l'ouvrage (« un tablier, un ouvrage »).
    pub same_bridge_ways: usize,
}

impl Attribution {
    /// La trace brute suit la voie de l'ouvrage au moins aussi bien que toute autre.
    pub fn attributed(&self) -> bool {
        self.other_m.is_none_or(|o| self.own_m <= o)
    }
}

/// Tuiles VersaTiles du cache (hors ligne), pour la règle de tags des spans vcyclist.
pub struct Tiles {
    tiles: BTreeMap<TileKey, Option<Arc<StreetTile>>>,
    /// Polygones de la couche `bridges` (`man_made=bridge`) des mêmes tuiles, lus dans le cache
    /// brut (README §4 ter.2, « un tablier, un ouvrage ») : une entrée par entité, ses anneaux.
    bridges: BTreeMap<TileKey, Vec<Vec<Vec<[f64; 2]>>>>,
}

impl std::fmt::Debug for Tiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tiles").field("tiles", &self.tiles.len()).finish()
    }
}

impl Tiles {
    pub async fn load(cache: &Path, pts: &[LatLon]) -> anyhow::Result<Self> {
        Self::load_mode(cache, pts, true).await
    }

    /// Comme `load`, en ligne (fill-cache) : tuiles absentes lues et mises en cache.
    pub async fn load_online(cache: &Path, pts: &[LatLon]) -> anyhow::Result<Self> {
        Self::load_mode(cache, pts, false).await
    }

    async fn load_mode(cache: &Path, pts: &[LatLon], offline: bool) -> anyhow::Result<Self> {
        let config = crate::service::bench_config(cache, offline, crate::service::Variant::DEFAULT);
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
        // En ligne (fill-cache), campagne de tuiles verrouillée : tuile absente du cache = refus.
        if !offline {
            crate::service::ensure_streets_cached(&outcomes)?;
        }
        // Couche `bridges` lue dans le corps brut des mêmes tuiles du cache (le service ne la décode
        // pas) ; tuile illisible ou absente : aucun polygone.
        let disk = ord_data::disk::DiskStore::new(cache, &ord_server::wiring::streets_namespace(&config));
        let bridges = keys
            .iter()
            .map(|&k| {
                let polys = disk
                    .read(k)
                    .and_then(|e| {
                        ord_adapters::streets::mvt::decode_polygon_layer(
                            &e.body,
                            "bridges",
                            k,
                            Shortbread::GRID,
                        )
                        .ok()
                    })
                    .unwrap_or_default();
                (k, polys)
            })
            .collect();
        let tiles = keys
            .into_iter()
            .zip(outcomes)
            .map(|(k, o)| (k, if let TileOutcome::Data { tile, .. } = o { Some(tile) } else { None }))
            .collect();
        Ok(Self { tiles, bridges })
    }

    /// Périmètre vélo d'un ouvrage du corpus de milieu `mid` et de stations `stations` (§4 bis.1) :
    /// lu sur la voie de l'ouvrage, l'élément de la tuile (attributs identiques) de même nature
    /// (pont ; tunnel ; voie au sol pour un passage dessous) qui suit le mieux la géométrie du
    /// corpus, c'est-à-dire de plus petite médiane de distance aux stations, acceptée à ≤ 15 m ; à
    /// défaut, toute voie avec le même critère (ouvrage que la source ne décrit pas comme tel,
    /// §4 bis.4). Aucune : pont ou tunnel absent des données. Ambigu si un autre candidat du même
    /// lot est à moins de 3 m de médiane en plus et donne l'autre périmètre.
    #[cfg(test)]
    fn perimeter(&self, mid: LatLon, stations: &[LatLon], kind: Kind) -> (Perimeter, bool) {
        let (p, ambiguous, _) = self.perimeter_and_way(mid, stations, kind);
        (p, ambiguous)
    }

    /// Comme `perimeter`, avec la clé de la voie de l'ouvrage retenue (attributs de l'élément),
    /// relue par l'attribution des passages (§4 ter.2).
    pub fn perimeter_and_way(
        &self,
        mid: LatLon,
        stations: &[LatLon],
        kind: Kind,
    ) -> (Perimeter, bool, Option<String>) {
        let grid = Shortbread::GRID;
        if !matches!(self.tiles.get(&grid.tile_of(grid.to_units(mid))), Some(Some(_))) || stations.is_empty()
        {
            return (Perimeter::Unknown, false, None);
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
        let medians: Vec<(bool, bool, f64, &String)> =
            dist.iter().map(|(k, (same, r, d))| (*same, *r, median(d), k)).filter(|c| c.2 <= 15.0).collect();
        for pool in [true, false] {
            let cands: Vec<&(bool, bool, f64, &String)> = medians.iter().filter(|c| !pool || c.0).collect();
            let Some(best) = cands.iter().min_by(|a, b| a.2.total_cmp(&b.2)) else { continue };
            let ambiguous = cands.iter().any(|c| c.1 != best.1 && c.2 - best.2 < 3.0);
            let p = if best.1 { Perimeter::InGraph } else { Perimeter::OffGraph };
            return (p, ambiguous, Some(best.3.clone()));
        }
        if matches!(kind, Kind::Bridge | Kind::Tunnel) {
            (Perimeter::Absent, false, None)
        } else {
            (Perimeter::Unknown, false, None)
        }
    }

    /// Attribution d'un passage (README §4 ter.2) : médiane des distances des points bruts
    /// `raw` (GPS d'origine sur l'emprise) à la voie de l'ouvrage (`voie`, clé de
    /// `perimeter_and_way`), et plus petite médiane des autres voies `bridge` / `tunnel` des
    /// tuiles de la campagne. Même lecture que le périmètre : tuile de chaque point, distance à
    /// l'élément (lignes d'attributs identiques). `None` : aucun point brut, ou voie de l'ouvrage
    /// absente des tuiles de ces points.
    pub fn attribution(
        &self,
        raw: &[LatLon],
        voie: &str,
        bridge: Option<&[LatLon; 3]>,
    ) -> Option<Attribution> {
        if raw.is_empty() {
            return None;
        }
        let grid = Shortbread::GRID;
        let mut dist: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        for (s, p) in raw.iter().enumerate() {
            let u = grid.to_units(*p);
            let mpu = grid.metres_per_unit(p.lat);
            let Some(Some(t)) = self.tiles.get(&grid.tile_of(u)) else { continue };
            for l in &t.lines {
                let a = &t.attrs[l.attr as usize];
                let key = format!("{a:?}");
                let structure = matches!(
                    a.structure,
                    StructureTag::Bridge
                        | StructureTag::Tunnel
                        | StructureTag::Covered
                        | StructureTag::BuildingPassage
                );
                if !structure && key != voie {
                    continue;
                }
                let d = l
                    .points
                    .windows(2)
                    .map(|w| dist_units(u, project_on_segment(u, w[0], w[1]).point) * mpu)
                    .fold(f64::INFINITY, f64::min);
                let e = dist.entry(key).or_insert_with(|| vec![f64::INFINITY; raw.len()]);
                e[s] = e[s].min(d);
            }
        }
        dist.get(voie)?;
        // Un tablier, un ouvrage (README §4 ter.2, ponts seulement) : les voies du même pont que la
        // voie de l'ouvrage en font partie, la médiane de l'ouvrage est la plus petite des leurs.
        let same: BTreeSet<String> = match bridge {
            Some(pts) => dist
                .keys()
                .filter(|k| k.as_str() != voie && self.same_bridge(voie, k, pts))
                .cloned()
                .collect(),
            None => BTreeSet::new(),
        };
        let own = dist
            .iter()
            .filter(|(k, _)| k.as_str() == voie || same.contains(*k))
            .map(|(_, d)| median(d))
            .fold(f64::INFINITY, f64::min);
        if !own.is_finite() {
            return None;
        }
        let other = dist
            .iter()
            .filter(|(k, _)| k.as_str() != voie && !same.contains(*k))
            .map(|(_, d)| median(d))
            .filter(|m| m.is_finite())
            .fold(None, |acc: Option<f64>, m| Some(acc.map_or(m, |a| a.min(m))));
        Some(Attribution { own_m: own, other_m: other, points: raw.len(), same_bridge_ways: same.len() })
    }

    /// Lignes d'un élément (clé d'attributs) dans les tuiles du voisinage de `u`.
    fn element_lines(&self, key: &str, u: [f64; 2]) -> Vec<&[[f64; 2]]> {
        let grid = Shortbread::GRID;
        let k = grid.tile_of(u);
        let mut out = Vec::new();
        for dx in -1i64..=1 {
            for dy in -1i64..=1 {
                let tk = TileKey { z: k.z, x: (k.x as i64 + dx) as u32, y: (k.y as i64 + dy) as u32 };
                let Some(Some(t)) = self.tiles.get(&tk) else { continue };
                out.extend(
                    t.lines
                        .iter()
                        .filter(|l| format!("{:?}", t.attrs[l.attr as usize]) == key)
                        .map(|l| &l.points[..]),
                );
            }
        }
        out
    }

    /// Point le plus proche de `u` sur les lignes d'un élément, et sa distance (unités).
    fn nearest_on(&self, key: &str, u: [f64; 2]) -> Option<([f64; 2], f64)> {
        self.element_lines(key, u)
            .into_iter()
            .flat_map(|l| l.windows(2).map(|w| project_on_segment(u, w[0], w[1]).point).collect::<Vec<_>>())
            .map(|q| (q, dist_units(u, q)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Polygone `bridges` (groupe de morceaux réunis) qui contient `u`.
    fn bridge_polygon(&self, u: [f64; 2]) -> Option<usize> {
        let grid = Shortbread::GRID;
        let k = grid.tile_of(u);
        // Morceaux du voisinage ; deux morceaux qui se recouvrent (tampon de tuile) sont un même
        // polygone coupé aux bords de tuile.
        let mut pieces: Vec<&Vec<Vec<[f64; 2]>>> = Vec::new();
        for dx in -1i64..=1 {
            for dy in -1i64..=1 {
                let tk = TileKey { z: k.z, x: (k.x as i64 + dx) as u32, y: (k.y as i64 + dy) as u32 };
                pieces.extend(self.bridges.get(&tk).into_iter().flatten());
            }
        }
        let inside = |rings: &Vec<Vec<[f64; 2]>>, p: [f64; 2]| {
            rings.iter().filter(|r| point_in_ring(r, p)).count() % 2 == 1
        };
        let mut group: Vec<usize> = (0..pieces.len()).collect();
        fn find(g: &mut [usize], mut x: usize) -> usize {
            while g[x] != x {
                g[x] = g[g[x]];
                x = g[x];
            }
            x
        }
        for i in 0..pieces.len() {
            for j in i + 1..pieces.len() {
                let touch = pieces[i].iter().flatten().any(|&p| inside(pieces[j], p))
                    || pieces[j].iter().flatten().any(|&p| inside(pieces[i], p));
                if touch {
                    let (a, b) = (find(&mut group, i), find(&mut group, j));
                    group[a.max(b)] = a.min(b);
                }
            }
        }
        let hit = (0..pieces.len()).find(|&i| inside(pieces[i], u))?;
        Some(find(&mut group, hit))
    }

    /// Deux voies `bridge` d'un même pont (README §4 ter.2) : (a) leurs points les plus proches du
    /// milieu de l'ouvrage dans un même polygone `bridges` ; deux polygones différents, deux ponts ;
    /// (b) sinon, si l'une au moins n'est dans aucun polygone, la concurrente à 10 m au plus de la
    /// voie de l'ouvrage sur 21 points de A à B, extrémités comprises.
    fn same_bridge(&self, voie: &str, other: &str, pts: &[LatLon; 3]) -> bool {
        let grid = Shortbread::GRID;
        let mid = grid.to_units(pts[1]);
        let mpu = grid.metres_per_unit(pts[1].lat);
        let (Some((qa, _)), Some((qb, _))) = (self.nearest_on(voie, mid), self.nearest_on(other, mid)) else {
            return false;
        };
        if let (Some(a), Some(b)) = (self.bridge_polygon(qa), self.bridge_polygon(qb)) {
            return a == b;
        }
        // Emprise : polyligne A – milieu – B du corpus, 21 points à pas égal.
        let poly: Vec<[f64; 2]> = pts.iter().map(|p| grid.to_units(*p)).collect();
        let lens = [dist_units(poly[0], poly[1]), dist_units(poly[1], poly[2])];
        let total = lens[0] + lens[1];
        (0..=20).all(|k| {
            let x = total * k as f64 / 20.0;
            let (p0, p1, t) = if x <= lens[0] {
                (poly[0], poly[1], if lens[0] > 0.0 { x / lens[0] } else { 0.0 })
            } else {
                (poly[1], poly[2], if lens[1] > 0.0 { (x - lens[0]) / lens[1] } else { 0.0 })
            };
            let q = [p0[0] + t * (p1[0] - p0[0]), p0[1] + t * (p1[1] - p0[1])];
            let Some((on_voie, _)) = self.nearest_on(voie, q) else { return false };
            self.nearest_on(other, on_voie).is_some_and(|(_, d)| d * mpu <= 10.0)
        })
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

    /// Trace elle-même sur un ouvrage au croisement d'E5 (README §4 ter.7) : voie `bridge` ou
    /// `tunnel` de la couche `streets` (tuile du point et ses 8 voisines) à `max_m` au plus de
    /// `at`, faisant moins de `max_deg` avec la direction `dir` de la trace (unités globales).
    /// Renvoie la distance (m) et l'angle (°) du segment le plus proche qui convient.
    pub fn structure_along(&self, at: LatLon, dir: [f64; 2], max_m: f64, max_deg: f64) -> Option<(f64, f64)> {
        self.structure_along_except(at, dir, max_m, max_deg, &BTreeSet::new())
    }

    /// Comme `structure_along`, sans les lignes `except` (tuile, rang de la ligne dans la tuile) :
    /// approche d'E5 sur un autre ouvrage, hors voies de l'ouvrage du dessus (f38b745).
    fn structure_along_except(
        &self,
        at: LatLon,
        dir: [f64; 2],
        max_m: f64,
        max_deg: f64,
        except: &BTreeSet<(TileKey, usize)>,
    ) -> Option<(f64, f64)> {
        let grid = Shortbread::GRID;
        let u = grid.to_units(at);
        let mpu = grid.metres_per_unit(at.lat);
        let k = grid.tile_of(u);
        let mut best: Option<(f64, f64)> = None;
        for dx in -1i64..=1 {
            for dy in -1i64..=1 {
                let tk = TileKey { z: k.z, x: (k.x as i64 + dx) as u32, y: (k.y as i64 + dy) as u32 };
                let Some(Some(t)) = self.tiles.get(&tk) else { continue };
                for (li, l) in t.lines.iter().enumerate() {
                    let a = &t.attrs[l.attr as usize];
                    if !matches!(a.structure, StructureTag::Bridge | StructureTag::Tunnel)
                        || except.contains(&(tk, li))
                    {
                        continue;
                    }
                    for w in l.points.windows(2) {
                        let d = dist_units(u, project_on_segment(u, w[0], w[1]).point) * mpu;
                        if d > max_m {
                            continue;
                        }
                        let angle = axis_angle_deg(dir, [w[1][0] - w[0][0], w[1][1] - w[0][1]]);
                        if angle < max_deg && best.is_none_or(|b| d < b.0) {
                            best = Some((d, angle));
                        }
                    }
                }
            }
        }
        best
    }

    /// Voies de l'ouvrage du dessus d'un croisement d'E5 (README §4 ter.7, f38b745) : lignes
    /// `bridge` ou `tunnel` de la couche `streets` dont un segment passe à `max_m` au plus de l'une
    /// des stations `span` (point, direction de la trace) en faisant au moins `min_deg` avec la
    /// trace (le pont du dessus la croise à 20° au moins), tuile de la station et ses 8 voisines ;
    /// puis, de proche en proche, les morceaux du même élément (attributs identiques) qui les
    /// prolongent dans les tuiles voisines (un sommet à 1 m au plus d'un morceau retenu).
    fn crossing_lines(
        &self,
        span: &[(LatLon, [f64; 2])],
        max_m: f64,
        min_deg: f64,
    ) -> BTreeSet<(TileKey, usize)> {
        let grid = Shortbread::GRID;
        let mut out = BTreeSet::new();
        let mut around = BTreeSet::new();
        let is_structure = |a: &ord_domain::street::WayAttrs| {
            matches!(a.structure, StructureTag::Bridge | StructureTag::Tunnel)
        };
        for &(p, dir) in span {
            let u = grid.to_units(p);
            let mpu = grid.metres_per_unit(p.lat);
            let k = grid.tile_of(u);
            for dx in -2i64..=2 {
                for dy in -2i64..=2 {
                    let tk = TileKey { z: k.z, x: (k.x as i64 + dx) as u32, y: (k.y as i64 + dy) as u32 };
                    around.insert(tk);
                    if dx.abs() > 1 || dy.abs() > 1 {
                        continue;
                    }
                    let Some(Some(t)) = self.tiles.get(&tk) else { continue };
                    for (li, l) in t.lines.iter().enumerate() {
                        if !is_structure(&t.attrs[l.attr as usize]) {
                            continue;
                        }
                        let crosses = l.points.windows(2).any(|w| {
                            dist_units(u, project_on_segment(u, w[0], w[1]).point) * mpu <= max_m
                                && axis_angle_deg(dir, [w[1][0] - w[0][0], w[1][1] - w[0][1]]) >= min_deg
                        });
                        if crosses {
                            out.insert((tk, li));
                        }
                    }
                }
            }
        }
        let Some(&(p0, _)) = span.first() else { return out };
        let mpu = grid.metres_per_unit(p0.lat);
        let line = |&(tk, li): &(TileKey, usize)| {
            let t = self.tiles.get(&tk).and_then(Option::as_ref).expect("ligne retenue dans une tuile lue");
            (format!("{:?}", t.attrs[t.lines[li].attr as usize]), &t.lines[li].points[..])
        };
        loop {
            let kept: Vec<(String, &[[f64; 2]])> = out.iter().map(line).collect();
            let mut added = Vec::new();
            for &tk in &around {
                let Some(Some(t)) = self.tiles.get(&tk) else { continue };
                for (li, l) in t.lines.iter().enumerate() {
                    let a = &t.attrs[l.attr as usize];
                    if out.contains(&(tk, li)) || !is_structure(a) {
                        continue;
                    }
                    let key = format!("{a:?}");
                    let joins = kept.iter().filter(|(k, _)| *k == key).any(|(_, pts)| {
                        l.points.iter().any(|&q| {
                            pts.windows(2)
                                .any(|w| dist_units(q, project_on_segment(q, w[0], w[1]).point) * mpu <= 1.0)
                        })
                    });
                    if joins {
                        added.push((tk, li));
                    }
                }
            }
            if added.is_empty() {
                return out;
            }
            out.extend(added);
        }
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

/// Angle non orienté entre deux directions, dans `[0°, 90°]` ; 0 si l'une est nulle.
fn axis_angle_deg(r: [f64; 2], q: [f64; 2]) -> f64 {
    let (nr, nq) = (r[0].hypot(r[1]), q[0].hypot(q[1]));
    if nr == 0.0 || nq == 0.0 {
        return 0.0;
    }
    ((r[0] * q[0] + r[1] * q[1]) / (nr * nq)).abs().min(1.0).acos().to_degrees()
}

/// Référence d'E5 au croisement (README §4 ter.7, criteria `E5.reference`) : bornes du README.
const E5_ON_STRUCTURE_M: f64 = 10.0;
const E5_ON_STRUCTURE_DEG: f64 = 20.0;
/// Décalage du baro corrigé localement : médiane baro − MNT de 200 à 1 500 m de part et d'autre.
const E5_BARO_RING_M: (f64, f64) = (200.0, 1_500.0);
/// MNT sans donnée à 10 m au plus du croisement : référence indisponible.
const E5_NODATA_M: f64 = 10.0;
/// « Le MNT lit le tablier » : MNT au croisement − moyenne des médianes des approches 20–50 m.
const E5_APPROACH_M: (f64, f64) = (20.0, 50.0);
const E5_DECK_BUMP_M: f64 = 2.0;
/// Bosse MNT de 1,0 à 2,0 m : référence MNT incertaine (README §4 ter.7, e477dd4).
const E5_UNCERTAIN_BUMP_M: f64 = 1.0;
/// Tolérances d'E5 par référence (criteria `E5.seuils`, f1bfda3) : MNT 1 m, baro corrigé 2 m.
pub const E5_TOL_MNT_M: f64 = 1.0;
pub const E5_TOL_BARO_M: f64 = 2.0;
/// Baro incertain (f1bfda3, e9430ac) : décalages des deux côtés (200–1 500 m chacun) à plus de
/// 2 m l'un de l'autre, ou un côté à moins de 3 points de baro.
const E5_SIDE_OFFSETS_MAX_M: f64 = 2.0;
/// Bruit baro de la trace au-delà duquel (exclu) la référence baro est incertaine (criteria
/// `E5.reference.baro_incertaine_si_bruit`, comme E1 plaine).
const E5_BARO_NOISE_MAX_M: f64 = 1.0;
const E5_SIDE_POINTS_MIN: usize = 3;
/// Demi-fenêtre de la direction de la trace au croisement (test « trace sur un ouvrage »).
const E5_DIRECTION_HALF_M: f64 = 10.0;

/// Décalages baro − MNT de chaque côté du croisement (200–1 500 m, stations de la grille), avant
/// puis après, et nombre de points bruts de baro de chaque côté (6bc77a5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BaroSides {
    pub offset_m: [Option<f64>; 2],
    pub points: [usize; 2],
}

/// Ce que dit la règle de référence d'E5 pour un croisement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum E5Reference {
    /// Trace sur un ouvrage des tuiles au croisement (`e5_trace_sur_ouvrage`) : distance, angle.
    Retire { distance_m: f64, angle_deg: f64 },
    /// Baro corrigé localement au croisement : altitude, décalage (médiane des deux côtés réunis)
    /// et décalages par côté.
    Baro { z: f64, offset_m: f64, sides: BaroSides },
    /// MNT LiDAR au croisement.
    Mnt { z: f64 },
    /// `e5_reference_indisponible`, avec sa raison.
    Indisponible { raison: &'static str },
    /// `e5_reference_incertaine` (rapportée, non comptée) : référence en cause (`baro_corrige_local`
    /// ou `mnt`), raison, altitude qu'elle donnerait (indicative, `NaN` sans décalage) et, pour le
    /// baro, décalage et côtés.
    Incertaine {
        reference: &'static str,
        raison: &'static str,
        z: f64,
        offset_m: Option<f64>,
        sides: Option<BaroSides>,
    },
}

/// Profil lu par la règle de référence d'E5, sur la grille du cas.
#[allow(missing_debug_implementations)]
#[derive(Clone, Copy)]
pub struct E5Input<'a> {
    pub d: &'a [f64],
    pub pts: &'a [LatLon],
    /// MNT (sortie sans correction du mode `distance`, README §2.2.3).
    pub raw: &'a [f64],
    /// Station sur terrain IGN LiDAR HD (`None` : non lu, seul le MNT absent compte).
    pub ign: Option<&'a [bool]>,
    /// Baro transporté d'une trace perso fiable (`None` sinon).
    pub baro: Option<&'a [f64]>,
    /// Abscisses corrigées du retard (s_p − δ, 085d26e) des points bruts de baro gardés (hors
    /// points insérés écartés et fenêtres masquées), comptés par côté (minimum 3 entre 200 et
    /// 1 500 m ; 6bc77a5).
    pub baro_points: &'a [f64],
    pub tiles: Option<&'a Tiles>,
    /// Point de croisement et station la plus proche.
    pub at: LatLon,
    pub im: usize,
    /// Stations d'où partent les approches 20–50 m de la bosse : `[im, im]` au point de croisement,
    /// bouts de la plage pour les plages `O` des témoins vcyclist (README §4 ter.7, e9430ac).
    pub ends: [usize; 2],
    /// Bruit baro de la trace (`qualify_baro.py`, `noise_m` de la trace entière, segments `.sN`
    /// compris) : référence baro incertaine au-delà de 1,0 m (README §4 quater.2).
    pub baro_noise_m: Option<f64>,
}

/// Bosse du MNT au croisement : MNT(croisement) − moyenne des médianes du MNT entre 20 et 50 m
/// avant `ends[0]` et après `ends[1]`, le long de la trace (qui suit la voie du dessous). `None` :
/// MNT absent au croisement ou côté sans station.
pub fn e5_bump(d: &[f64], raw: &[f64], im: usize, ends: [usize; 2]) -> Option<f64> {
    let side = |from: usize, dir: f64| -> Option<f64> {
        let v: Vec<f64> = (0..d.len())
            .filter(|&k| (E5_APPROACH_M.0..=E5_APPROACH_M.1).contains(&(dir * (d[k] - d[from]))))
            .map(|k| raw[k])
            .filter(|z| z.is_finite())
            .collect();
        (!v.is_empty()).then(|| median(&v))
    };
    let (a, b) = (side(ends[0], -1.0)?, side(ends[1], 1.0)?);
    raw[im].is_finite().then(|| raw[im] - 0.5 * (a + b))
}

/// Direction de la trace à la station `k` (unités globales) : de la première à la dernière station
/// à ± 10 m de `k` (`d` croissante). `None` si une seule station.
fn trace_dir(d: &[f64], pts: &[LatLon], k: usize) -> Option<[f64; 2]> {
    let (mut a, mut b) = (k, k);
    while a > 0 && d[k] - d[a - 1] <= E5_DIRECTION_HALF_M {
        a -= 1;
    }
    while b + 1 < d.len() && d[b + 1] - d[k] <= E5_DIRECTION_HALF_M {
        b += 1;
    }
    if a >= b {
        return None;
    }
    let grid = Shortbread::GRID;
    let (ua, ub) = (grid.to_units(pts[a]), grid.to_units(pts[b]));
    Some([ub[0] - ua[0], ub[1] - ua[1]])
}

/// Stations des approches 20–50 m de la bosse : avant `ends[0]` et après `ends[1]`, le long de la
/// trace (bouts de la plage pour une plage `O`, e9430ac).
fn approach_stations(d: &[f64], ends: [usize; 2]) -> Vec<usize> {
    let within = |x: f64| (E5_APPROACH_M.0..=E5_APPROACH_M.1).contains(&x);
    let before = (0..=ends[0]).rev().take_while(|&k| d[ends[0]] - d[k] <= E5_APPROACH_M.1);
    let after = (ends[1]..d.len()).take_while(|&k| d[k] - d[ends[1]] <= E5_APPROACH_M.1);
    before
        .filter(|&k| within(d[ends[0]] - d[k]))
        .chain(after.filter(|&k| within(d[k] - d[ends[1]])))
        .collect()
}

/// Une approche 20–50 m chevauche un autre ouvrage (README §4 ter.7, f38b745, validé en 2e511e5) :
/// l'une de ses stations est à 10 m au plus d'une ligne `bridge` ou `tunnel` de la couche
/// `streets` (tuile de la station et ses 8 voisines, toutes classes) qui fait moins de 20° avec la
/// trace, le test de la trace sur un ouvrage au croisement ; les voies de l'ouvrage du dessus
/// (`Tiles::crossing_lines`, lues sur les stations de `ends[0]` à `ends[1]` et au croisement) ne
/// comptent pas.
fn approach_on_structure(x: &E5Input<'_>, tiles: &Tiles) -> bool {
    let (d, pts) = (x.d, x.pts);
    let mut span: Vec<(LatLon, [f64; 2])> =
        (x.ends[0]..=x.ends[1]).filter_map(|k| Some((pts[k], trace_dir(d, pts, k)?))).collect();
    span.extend(trace_dir(d, pts, x.im).map(|v| (x.at, v)));
    let above = tiles.crossing_lines(&span, E5_ON_STRUCTURE_M, E5_ON_STRUCTURE_DEG);
    approach_stations(d, x.ends).into_iter().any(|k| {
        trace_dir(d, pts, k).is_some_and(|v| {
            tiles.structure_along_except(pts[k], v, E5_ON_STRUCTURE_M, E5_ON_STRUCTURE_DEG, &above).is_some()
        })
    })
}

/// Règle de référence d'E5 (README §4 ter.7), dans l'ordre : retiré, baro, MNT (sans donnée,
/// approche sur un autre ouvrage, bosse). Jamais le service.
pub fn e5_reference(x: &E5Input<'_>) -> E5Reference {
    let (d, im) = (x.d, x.im);
    if let (Some(tiles), Some(dir)) = (x.tiles, trace_dir(d, x.pts, im)) {
        if let Some((distance_m, angle_deg)) =
            tiles.structure_along(x.at, dir, E5_ON_STRUCTURE_M, E5_ON_STRUCTURE_DEG)
        {
            return E5Reference::Retire { distance_m, angle_deg };
        }
    }
    if let Some(baro) = x.baro.filter(|b| b[im].is_finite()) {
        // Décalage par côté (200–1 500 m chacun, stations de la grille) ; décalage retenu =
        // médiane des deux réunis. Points d'un côté : points bruts de baro (6bc77a5), pas les
        // stations interpolées entre eux.
        let in_ring = |dir: f64, s: f64| (E5_BARO_RING_M.0..=E5_BARO_RING_M.1).contains(&(dir * (s - d[im])));
        let side = |dir: f64| -> Vec<f64> {
            (0..d.len())
                .filter(|&k| in_ring(dir, d[k]))
                .map(|k| baro[k] - x.raw[k])
                .filter(|v| v.is_finite())
                .collect()
        };
        let points = |dir: f64| x.baro_points.iter().filter(|&&s| in_ring(dir, s)).count();
        let (before, after) = (side(-1.0), side(1.0));
        let med = |v: &[f64]| (!v.is_empty()).then(|| median(v));
        let sides = BaroSides { offset_m: [med(&before), med(&after)], points: [points(-1.0), points(1.0)] };
        let ring: Vec<f64> = before.iter().chain(&after).copied().collect();
        let offset_m = med(&ring);
        let z = offset_m.map_or(f64::NAN, |o| baro[im] - o);
        let uncertain = |raison| E5Reference::Incertaine {
            reference: "baro_corrige_local",
            raison,
            z,
            offset_m,
            sides: Some(sides),
        };
        // Baro bruité (README §4 quater.2, f8f81a6) : après le retrait, avant les côtés ; tous
        // groupes, sans repli sur le MNT.
        if x.baro_noise_m.is_some_and(|b| b > E5_BARO_NOISE_MAX_M) {
            return uncertain("baro_bruite");
        }
        if sides.points.iter().any(|&n| n < E5_SIDE_POINTS_MIN) {
            return uncertain("points_baro_insuffisants");
        }
        // Côté sans décalage (aucune station avec baro et MNT) : incertaine aussi, jamais de
        // repli sur le MNT pour une trace fiable au baro présent au croisement (1477b5f).
        let ([Some(a), Some(b)], Some(offset_m)) = (sides.offset_m, offset_m) else {
            return uncertain("decalage_d_un_cote_absent");
        };
        if (a - b).abs() > E5_SIDE_OFFSETS_MAX_M {
            return uncertain("decalages_des_cotes");
        }
        return E5Reference::Baro { z, offset_m, sides };
    }
    let nodata = (0..d.len())
        .filter(|&k| (d[k] - d[im]).abs() <= E5_NODATA_M)
        .any(|k| !x.raw[k].is_finite() || x.ign.is_some_and(|g| !g[k]));
    if nodata {
        return E5Reference::Indisponible { raison: "mnt_sans_donnee" };
    }
    // Approche sur un autre ouvrage (f38b745, 2e511e5) : testée avant la bosse.
    if x.tiles.is_some_and(|t| approach_on_structure(x, t)) {
        return E5Reference::Incertaine {
            reference: "mnt",
            raison: "approche_sur_ouvrage",
            z: x.raw[im],
            offset_m: None,
            sides: None,
        };
    }
    match e5_bump(d, x.raw, im, x.ends) {
        Some(b) if b > E5_DECK_BUMP_M => E5Reference::Indisponible { raison: "mnt_lit_le_tablier" },
        Some(b) if b >= E5_UNCERTAIN_BUMP_M => E5Reference::Incertaine {
            reference: "mnt",
            raison: "bosse_mnt",
            z: x.raw[im],
            offset_m: None,
            sides: None,
        },
        Some(_) => E5Reference::Mnt { z: x.raw[im] },
        None => E5Reference::Indisponible { raison: "approche_sans_mnt" },
    }
}

/// Point dans un anneau (règle pair-impair).
fn point_in_ring(ring: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut inside = false;
    let n = ring.len();
    let mut j = n.saturating_sub(1);
    for i in 0..n {
        let (a, b) = (ring[i], ring[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
        j = i;
    }
    inside
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
    cache: &Path,
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
                    &r.svc.segs,
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
            let typed = s.iter().filter(|&&i| r.svc.on[i]).count() as f64 / s.len() as f64;
            out.push(json!({"ouvrage": format!("{}#balcon", o.id), "cas": r.id, "metrique": "S3",
                "reference": "vcyclist_S", "ecart_max_m": r2(max), "part_typee_ouvrage": r4(typed)}));
        }
    }
    let is_covered =
        |id: &str, i0: usize, i1: usize| covered.iter().any(|c| c.0 == id && c.1 <= i1 && i0 <= c.2);

    for (ri, r) in routes.iter().enumerate() {
        let n = r.s.pts.len();
        // S1 (README §4 ter.1) : recouvrement continu de l'emprise par les segments structure.
        let span_typed = |i0: usize, i1: usize| typed_overlap(&r.svc.segs, r.d[i0], r.d[i1]);
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
                            let typed = s.iter().filter(|&&i| r.svc.on[i]).count() as f64 / s.len() as f64;
                            out.push(json!({"ouvrage": id, "cas": r.id, "metrique": "S3", "reference": "vcyclist_S",
                                "ecart_max_m": r2(max), "part_typee_ouvrage": r4(typed)}));
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
                        &r.svc.segs,
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
                // La trace du négatif reste dans E5 même hors graphe (§4 bis.1). Référence au
                // croisement (README §4 ter.7) lue à la station du milieu de la plage `O`, approches
                // de la bosse depuis les bouts de la plage (e9430ac) ; témoin sans baro : MNT, ou
                // retiré / indisponible / incertain. Écart : max sur la plage.
                let im = (i0 + i1) / 2;
                let pts: Vec<LatLon> = (0..r.s.pts.len()).map(|i| ll(r, i)).collect();
                let input = E5Input {
                    d: &r.d,
                    pts: &pts,
                    raw: &r.raw.ele,
                    ign: None,
                    baro: None,
                    baro_points: &[],
                    tiles: Some(tiles),
                    at: pts[im],
                    im,
                    ends: [i0, i1],
                    baro_noise_m: None,
                };
                let bump = e5_bump(&r.d, &r.raw.ele, im, [i0, i1]);
                // Référence MNT : écart max sur la plage, contre le MNT de chaque station.
                let range_max =
                    (i0..=i1).map(|i| (r.svc.ele[i] - r.raw.ele[i]).abs()).fold(f64::NAN, f64::max);
                let ecart = |_z: f64| range_max;
                match e5_item(&id, &r.id, e5_reference(&input), &ecart, Some(range_max), bump) {
                    Ok(fields) => {
                        let mut item = serde_json::Map::new();
                        item.insert("ouvrage".into(), json!(id));
                        item.insert("cas".into(), json!(r.id));
                        item.extend(fields);
                        out.push(Value::Object(item));
                    }
                    Err(v) => out.push(v),
                }
            }
        }
        if negative {
            // Ouvrages réels sur la voie même du témoin (README §4 ter.6) : jamais fictifs, plage
            // A–B ± 35 m retirée de S2 (fictifs, longueur typée, longueur totale) et rapportée ;
            // galerie : limite des données (§4 bis.4), mesurée comme un tunnel entre ses portails.
            // Le témoin sort de S2 si l'ouvrage en est l'objet ou en couvre plus de la moitié.
            let Some(real) = own_structures(bench, cache, &r.id)? else {
                skipped.push(json!({"id": r.id, "raison": "S2 : tags de la base OSM du témoin absents du cache (fill-cache)"}));
                continue;
            };
            let category = control_category(bench, &r.id)?;
            let length = r.d[n - 1];
            let object = real.iter().find(|(nature, i0, i1)| {
                names_object(&category, nature) || r.d[*i1] - r.d[*i0] > 0.5 * length
            });
            for (k, (nature, i0, i1)) in real.iter().enumerate() {
                let (i0, i1) = (*i0, (*i1).min(n - 1));
                let id = format!("{}#reel{k}", r.id);
                let gallery = nature == "gallery";
                // Absents du schéma Shortbread : galerie (`avalanche_protector`), `covered`.
                let limit = match nature.as_str() {
                    "gallery" => Some(GALLERY_NOTE),
                    "covered" => Some(COVERED_NOTE),
                    _ => None,
                };
                if gallery {
                    let (a, b) = (i0.saturating_sub(1), (i1 + 1).min(n - 1));
                    let mut item =
                        tunnel_item(&id, &r.id, &r.d, &r.svc.ele, &r.raw.ele, &r.svc.segs, &inside[ri], a, b);
                    item["limite_donnees"] = json!(GALLERY_NOTE);
                    out.push(item);
                }
                let mut real_item = json!({"ouvrage": id, "cas": r.id, "metrique": "S2_ouvrage_reel", "nature": nature,
                    "de_m": r2(r.d[i0]), "a_m": r2(r.d[i1]), "marge_m": REAL_STRUCTURE_MARGIN_M,
                    "limite_donnees": limit.map_or(Value::Null, |n| json!(n)),
                    "temoin_sorti": object.is_some()});
                // Voie couverte (`covered=yes`, au niveau du sol) : erreur d'altitude mesurée contre
                // le MNT aux stations de la plage, rapportée sans seuil (README §4 ter.6, cc59b18).
                if nature == "covered" {
                    let e = (i0..=i1)
                        .map(|i| (r.svc.ele[i] - r.raw.ele[i]).abs())
                        .filter(|x| x.is_finite())
                        .fold(f64::NAN, f64::max);
                    real_item["reference"] = json!("mnt_aux_stations");
                    real_item["ecart_max_m"] = r2(e);
                    real_item["stations"] = json!(i1 + 1 - i0);
                }
                out.push(real_item);
            }
            if let Some((nature, i0, i1)) = object {
                let raison = if names_object(&category, nature) {
                    format!("ouvrage réel ({nature}) objet du témoin ({category})")
                } else {
                    format!(
                        "ouvrage réel ({nature}) sur {:.0} % du témoin",
                        100.0 * (r.d[*i1] - r.d[*i0]) / length
                    )
                };
                out.push(json!({"cas": r.id, "metrique": "S2_temoin_sorti", "raison": raison}));
                continue;
            }
            let masked = |i: usize| {
                real.iter().any(|(_, i0, i1)| {
                    r.d[i] >= r.d[*i0] - REAL_STRUCTURE_MARGIN_M
                        && r.d[i] <= r.d[(*i1).min(n - 1)] + REAL_STRUCTURE_MARGIN_M
                })
            };
            // S2 hors graphe vélo (README §4 ter.5) : plages où la trace suit une route motorisée
            // fermée au vélo (règle de M1 resserrée, tuiles seulement) exclues de S2.
            let st: Vec<(f64, LatLon)> = (0..n).map(|i| (r.d[i], ll(r, i))).collect();
            let step = if n > 1 { r.d[n - 1] / (n - 1) as f64 } else { 5.0 };
            let closed = match crate::off_network::closed_on(&st, cache, step, false).await? {
                Ok((ranges, _)) => ranges,
                Err(reason) => {
                    skipped.push(json!({"id": r.id, "raison": format!("S2 : {reason}")}));
                    continue;
                }
            };
            let off = |i: usize| closed.iter().any(|c| c.contains(&r.d[i])) || masked(i);
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
            let (mut total, mut real_m) = (0.0, 0.0);
            for i in 0..n {
                if i > 0 && masked(i) {
                    real_m += r.d[i] - r.d[i - 1];
                }
                if i > 0 && !off(i) {
                    total += r.d[i] - r.d[i - 1];
                }
                if r.svc.on[i] && !off(i) {
                    if !matches!(r.s.kind[i].as_str(), "X" | "x") && i > 0 {
                        fp_m += r.d[i] - r.d[i - 1];
                    }
                    run = Some(run.map_or((i, i), |x| (x.0, i)));
                } else {
                    flush(run.take(), &mut fictifs);
                }
            }
            flush(run, &mut fictifs);
            out.push(json!({"cas": r.id, "metrique": "S2", "longueur_m": r2(total),
                "faux_positif_m": r2(fp_m), "ouvrages_fictifs": fictifs,
                "hors_graphe_velo_m": r2(crate::off_network::length(&closed)),
                "ouvrage_reel_masque_m": r2(real_m)}));
        }
    }
    Ok(out)
}

/// Passages d'E5, comptés (`E5`) ou rapportés non comptés : réunis en sites (README §4 quater.4).
const E5_FAMILY: [&str; 4] = ["E5", "e5_reference_incertaine", "e5_reference_indisponible", "e5_trace_sur_ouvrage"];
/// Deux croisements d'un même passage à 40 m au plus l'un de l'autre (abscisse du croisement sur la
/// trace ; tolérance d'un passage, README §2.1) sont du même site.
pub const E5_SITE_TOL_M: f64 = 40.0;

/// Site physique d'E5 (README §4 quater.4, 8559a21 ; criteria `E5.site`) : ids réunis s'ils sont
/// le même ouvrage ou si un même passage les croise à 40 m au plus l'un de l'autre, de proche en
/// proche, toutes traces confondues.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct E5Site {
    pub ouvrages: BTreeSet<String>,
    pub cas: BTreeSet<String>,
    pub passages: usize,
    /// Passages comptés : référence baro ou MNT, ni retiré, ni incertain, ni indisponible.
    pub comptes: usize,
    pub echecs: usize,
    /// Écart max des passages comptés (m).
    pub ecart_max_m: f64,
    /// Marques des passages non comptés (`metrique`, raison).
    pub marques: BTreeSet<String>,
}

impl E5Site {
    /// Verdict du site : `None` sans passage compté (non compté), échec dès un passage compté en
    /// échec (lecture stricte, comme `passe_si: tous`).
    pub fn pass(&self) -> Option<bool> {
        (self.comptes > 0).then_some(self.echecs == 0)
    }

    pub fn label(&self) -> String {
        self.ouvrages.iter().cloned().collect::<Vec<_>>().join("+")
    }

    pub fn to_json(&self) -> Value {
        let mut v = json!({"ouvrages": self.ouvrages, "cas": self.cas, "passages": self.passages,
            "passages_comptes": self.comptes, "pass": self.pass()});
        if self.comptes > 0 {
            v["ecart_max_m"] = r2(self.ecart_max_m);
        } else {
            v["marques"] = json!(self.marques);
        }
        v
    }
}

/// Sites d'E5 des passages `items` (toutes traces de la passe), triés par leur premier id.
pub fn e5_sites(items: &[Value]) -> Vec<E5Site> {
    let family: Vec<&Value> =
        items.iter().filter(|i| i["metrique"].as_str().is_some_and(|m| E5_FAMILY.contains(&m))).collect();
    let ids: BTreeSet<&str> = family.iter().filter_map(|i| i["ouvrage"].as_str()).collect();
    let index: BTreeMap<&str, usize> = ids.iter().enumerate().map(|(k, id)| (*id, k)).collect();
    let mut parent: Vec<usize> = (0..ids.len()).collect();
    fn root(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    // Croisements de chaque passage (même cas), dans l'ordre de l'abscisse : voisins à 40 m reliés.
    let mut by_case: BTreeMap<&str, Vec<(f64, usize)>> = BTreeMap::new();
    for i in &family {
        if let (Some(c), Some(d), Some(id)) = (i["cas"].as_str(), i["d_milieu_m"].as_f64(), i["ouvrage"].as_str()) {
            by_case.entry(c).or_default().push((d, index[id]));
        }
    }
    for list in by_case.values_mut() {
        list.sort_by(|a, b| a.0.total_cmp(&b.0));
        for w in list.windows(2) {
            if w[1].0 - w[0].0 <= E5_SITE_TOL_M + 1e-9 {
                let (a, b) = (root(&mut parent, w[0].1), root(&mut parent, w[1].1));
                parent[a] = b;
            }
        }
    }
    let mut sites: BTreeMap<usize, E5Site> = BTreeMap::new();
    for i in &family {
        let Some(id) = i["ouvrage"].as_str() else { continue };
        let r = root(&mut parent, index[id]);
        let s = sites.entry(r).or_insert_with(|| E5Site { ecart_max_m: f64::NAN, ..E5Site::default() });
        s.ouvrages.insert(id.to_owned());
        if let Some(c) = i["cas"].as_str() {
            s.cas.insert(c.to_owned());
        }
        s.passages += 1;
        match i["pass"].as_bool().filter(|_| i["metrique"] == "E5") {
            Some(pass) => {
                s.comptes += 1;
                s.echecs += usize::from(!pass);
                let e = i["ecart_croisement_m"].as_f64().unwrap_or(f64::NAN);
                s.ecart_max_m = s.ecart_max_m.max(e);
            }
            None => {
                let m = i["metrique"].as_str().unwrap_or("");
                s.marques.insert(match i["raison"].as_str() {
                    Some(r) => format!("{m}:{r}"),
                    None => m.to_owned(),
                });
            }
        }
    }
    let mut out: Vec<E5Site> = sites.into_values().collect();
    out.sort_by(|a, b| a.ouvrages.first().cmp(&b.ouvrages.first()));
    out
}

/// Verdicts E3, E4, E5, S1, S2 et S3 (rapporté), `hors_graphe_velo` (rapporté).
pub fn judge(items: &[Value]) -> (Value, Value) {
    let by = |m: &str| -> Vec<&Value> { items.iter().filter(|i| i["metrique"] == m).collect() };
    let rate = |v: &[&Value]| -> (usize, usize) {
        let judged: Vec<&&Value> = v.iter().filter(|i| i["pass"].is_boolean()).collect();
        (judged.iter().filter(|i| i["pass"] == true).count(), judged.len())
    };
    // E3 par pont : médiane, sur les passages, de l'écart au milieu et de l'écart max (§4 bis.3).
    // Limite des données (§4 bis.4) : hors d'E4 (E3 pour un pont) et de S1, rapportée à part.
    let limited = |i: &Value| i["limite_donnees"].is_string();
    let mut ponts: BTreeMap<String, (Vec<f64>, Vec<f64>, f64)> = BTreeMap::new();
    for i in by("E3").into_iter().filter(|i| !limited(i)) {
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
    let e4: Vec<&Value> = by("E4").into_iter().filter(|i| !limited(i)).collect();
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
    let limits: Vec<Value> = items
        .iter()
        .filter(|i| {
            limited(i)
                && (matches!(i["metrique"].as_str(), Some("E3" | "E4"))
                    || (i["metrique"] == "S2_ouvrage_reel" && i["ecart_max_m"].is_number()))
        })
        .map(|i| {
            // Sans seuil : l'erreur est rapportée, jamais jugée.
            let mut i = strip_private(i);
            if let Some(m) = i.as_object_mut() {
                m.remove("pass");
            }
            i
        })
        .collect();
    // E5 par site (README §4 quater.4) : un site physique, un cas ; lecture stricte.
    let e5_sites = e5_sites(items);
    let e5_counted: Vec<&E5Site> = e5_sites.iter().filter(|s| s.pass().is_some()).collect();
    let (e5_ok, e5_n) = (e5_counted.iter().filter(|s| s.pass() == Some(true)).count(), e5_counted.len());
    let e5_fail: Vec<String> = e5_counted.iter().filter(|s| s.pass() == Some(false)).map(|s| s.label()).collect();
    let sum = |k: &str| -> f64 {
        items
            .iter()
            .filter(|i| !limited(i))
            .filter_map(|i| i[k].as_f64().or_else(|| i[format!("_{k}")].as_f64()))
            .sum()
    };
    let (s1_ref, s1_typed) = (sum("S1.emprise_m"), sum("S1.type_m"));
    let s1 = s1_typed / s1_ref.max(1e-9);
    let s2 = by("S2");
    let s2_len: f64 = s2.iter().filter_map(|i| i["longueur_m"].as_f64()).sum();
    let s2_fp: f64 = s2.iter().filter_map(|i| i["faux_positif_m"].as_f64()).sum();
    let s2_fict: u64 = s2.iter().filter_map(|i| i["ouvrages_fictifs"].as_u64()).sum();
    let s2_share = s2_fp / s2_len.max(1e-9);
    let s2_ok = s2_fict == 0 && s2_share <= 0.005;
    let s2_fail: Vec<&Value> = s2
        .iter()
        .filter(|i| {
            i["ouvrages_fictifs"].as_u64().is_some_and(|n| n > 0)
                || i["faux_positif_m"].as_f64().is_some_and(|x| x > 0.0)
        })
        .map(|i| if i["temoin"].is_string() { &i["temoin"] } else { &i["cas"] })
        .collect();
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
        "E5": {"croisements": by("E5").len(), "sites": e5_n, "sites_ok": e5_ok,
            "par_site": e5_sites.iter().map(E5Site::to_json).collect::<Vec<_>>(),
            "par_reference": {
                "baro_corrige_local": by("E5").into_iter().filter(|i| i["reference"] == "baro_corrige_local").count(),
                "mnt": by("E5").into_iter().filter(|i| i["reference"] == "mnt").count()},
            "passages_dessus": by("E5_passage_dessus").into_iter().map(strip_private).collect::<Vec<_>>(),
            // README §4 ter.7 : rapportés, non comptés.
            "trace_sur_ouvrage": by("e5_trace_sur_ouvrage").into_iter().map(strip_private).collect::<Vec<_>>(),
            "references_indisponibles": by("e5_reference_indisponible").into_iter().map(strip_private).collect::<Vec<_>>(),
            // README §4 ter.7 (f1bfda3, e477dd4, e9430ac) : rapportées, non comptées, écart indicatif.
            "references_incertaines": by("e5_reference_incertaine").into_iter().map(strip_private).collect::<Vec<_>>(),
            "seuils_m": {"mnt": E5_TOL_MNT_M, "baro_corrige_local": E5_TOL_BARO_M}},
        "S1": {"emprise_m": r2(s1_ref), "type_m": r2(s1_typed), "rappel": r4(s1), "seuil": 0.95},
        "S2": {"longueur_m": r2(s2_len), "faux_positif_m": r2(s2_fp), "part": r4(s2_share), "ouvrages_fictifs": s2_fict},
        "S3": by("S3").into_iter().map(strip_private).collect::<Vec<_>>(),
        "passages_retires_niveau": by("passage_retire_niveau").into_iter().map(strip_private).collect::<Vec<_>>(),
        // README §4 ter.2 : rapportés, comptés ni dans E3, E4, S1, ni dans hors_graphe_velo.
        "passages_sur_ouvrage_parallele": by("passage_sur_ouvrage_parallele").into_iter().map(strip_private).collect::<Vec<_>>(),
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
        "S2": verdict(s2_ok, s2.len()),
        // Témoins qui portent le faux positif d'un S2 en échec (ouvrage fictif ou longueur typée).
        "S2_echecs": if s2_ok { Vec::new() } else { s2_fail },
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

    /// Segments structure couvrant tout le parcours.
    const ALL: [(f64, f64); 1] = [(f64::NEG_INFINITY, f64::INFINITY)];

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

    /// Sites d'E5 (README §4 quater.4) : tabliers voisins d'un même passage (≤ 40 m) réunis, même
    /// pont repassé par une autre trace réuni de proche en proche ; un passage compté en échec met
    /// le site en échec ; un site sans passage compté porte ses marques et n'est pas compté.
    #[test]
    fn e5_is_judged_and_covered_by_site() {
        let e5 = |o: &str, cas: &str, d: f64, pass: bool| {
            json!({"ouvrage": o, "cas": cas, "metrique": "E5", "d_milieu_m": d, "pass": pass,
                "ecart_croisement_m": if pass { 0.5 } else { 2.5 }, "reference": "mnt"})
        };
        let items = vec![
            // Trois tabliers au même point d'une trace, dont un en échec.
            e5("nv-664", "perso-131", 1_000.0, true),
            e5("nv-795", "perso-131", 1_020.0, false),
            json!({"ouvrage": "nv-836", "cas": "perso-131", "metrique": "e5_reference_incertaine",
                "raison": "baro_bruite", "d_milieu_m": 1_055.0}),
            // nv-795 repassé sur une autre trace, réussi : même site.
            e5("nv-795", "perso-153", 300.0, true),
            // Pont à 45 m d'un autre sur la même trace : site distinct.
            e5("nv-900", "perso-131", 1_100.0, true),
            // Site sans passage compté.
            json!({"ouvrage": "nv-901", "cas": "perso-140", "metrique": "e5_reference_indisponible",
                "raison": "mnt_sans_donnee", "d_milieu_m": 10.0}),
            json!({"ouvrage": "nv-902", "cas": "perso-140", "metrique": "E5_passage_dessus", "d_milieu_m": 20.0}),
            json!({"temoin": "pl-x-s0", "cas": "pl-x-s0", "metrique": "S2", "longueur_m": 1000.0,
                "faux_positif_m": 30.0, "ouvrages_fictifs": 1}),
            json!({"temoin": "pl-y-s0", "cas": "pl-y-s0", "metrique": "S2", "longueur_m": 1000.0,
                "faux_positif_m": 0.0, "ouvrages_fictifs": 0}),
        ];
        let sites = e5_sites(&items);
        assert_eq!(sites.len(), 3, "{sites:?}");
        let first = &sites[0];
        assert_eq!(first.label(), "nv-664+nv-795+nv-836");
        assert_eq!((first.passages, first.comptes, first.pass()), (4, 3, Some(false)));
        assert_eq!(first.cas.len(), 2);
        assert!((first.ecart_max_m - 2.5).abs() < 1e-9);
        assert_eq!(sites[1].pass(), Some(true));
        assert_eq!(sites[2].pass(), None);
        assert_eq!(sites[2].to_json()["marques"], json!(["e5_reference_indisponible:mnt_sans_donnee"]));
        let (agg, v) = judge(&items);
        assert_eq!((agg["E5"]["croisements"].as_u64(), agg["E5"]["sites"].as_u64()), (Some(4), Some(2)));
        assert_eq!(v["E5"], "fail");
        assert_eq!(v["E5_echecs"], json!(["nv-664+nv-795+nv-836"]));
        assert_eq!(v["S2"], "fail");
        assert_eq!(v["S2_echecs"], json!(["pl-x-s0"]));
        let cov = crate::coverage::measure(&[], &[], &items, &BTreeMap::new(), 0, 5.0);
        assert_eq!(cov["E5"]["croisements"], 2, "couverture en sites comptés");
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
        let t = tunnel_item("t", "c", &d, &slope, &slope, &ALL, &blocked, 30, 40);
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
        let t = tunnel_item("t", "c", &d, &svc, &raw, &ALL, &blocked, 230, 260);
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
            Tiles { tiles: BTreeMap::from([(key, Some(Arc::new(t)))]), bridges: BTreeMap::new() }
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
    fn s1_is_the_continuous_overlap_with_a_half_step_tolerance() {
        // Emprise [100, 160] ; segment arrêté à 2 m de chaque borne : prolongé.
        assert_eq!(typed_overlap(&[(102.0, 158.0)], 100.0, 160.0), 60.0);
        // À 3 m d'une borne : non prolongé, recouvrement mesuré en continu.
        assert_eq!(typed_overlap(&[(103.0, 160.0)], 100.0, 160.0), 57.0);
        // Segment qui déborde : coupé aux bornes ; deux segments contigus (pont puis tunnel)
        // comptés une fois, même prolongés.
        assert_eq!(typed_overlap(&[(90.0, 130.0), (130.0, 158.5)], 100.0, 160.0), 60.0);
        // Un trou intérieur n'est jamais comblé.
        assert_eq!(typed_overlap(&[(100.0, 120.0), (122.0, 160.0)], 100.0, 160.0), 58.0);
        assert_eq!(typed_overlap(&[], 100.0, 160.0), 0.0);
        // Un segment entièrement hors de l'emprise ne compte pas, même à moins de 2,5 m.
        assert_eq!(typed_overlap(&[(161.0, 200.0)], 100.0, 160.0), 0.0);
    }

    pub(super) fn tiles_of(lines: Vec<(WayAttrsT, f64)>, mid: LatLon) -> (Tiles, [f64; 2]) {
        let grid = Shortbread::GRID;
        let u = grid.to_units(mid);
        let key = grid.tile_of(u);
        let mut t = StreetTile::empty(key);
        for (k, (a, north)) in lines.into_iter().enumerate() {
            t.attrs.push(a);
            t.lines.push(ord_domain::street::StreetLine {
                attr: k as u32,
                points: vec![[u[0] - 50.0, u[1] + north], [u[0] + 50.0, u[1] + north]],
                cut: [false, false],
            });
        }
        (Tiles { tiles: BTreeMap::from([(key, Some(Arc::new(t)))]), bridges: BTreeMap::new() }, u)
    }

    pub(super) type WayAttrsT = ord_domain::street::WayAttrs;

    pub(super) fn way_attrs(kind: &str, structure: StructureTag) -> WayAttrsT {
        WayAttrsT {
            highway: Arc::from(kind),
            surface: ord_domain::street::SurfaceTag::Absent,
            tracktype: None,
            bicycle: None,
            service: None,
            link: false,
            rail: false,
            structure,
            layer: None,
            smoothness: None,
            osm_id: None,
            raw: None,
        }
    }

    /// README §2, point 3 : les ouvrages V1 tenus à l'écart sont de la calibration une fois la
    /// série V1 jugée (`with_v1_judged`).
    #[test]
    fn judged_v1_structures_are_calibration() {
        let dir = tempfile::tempdir().unwrap();
        let private = dir.path().join("private");
        std::fs::create_dir_all(private.join("traces")).unwrap();
        std::fs::create_dir_all(private.join("v1")).unwrap();
        let json = r#"[{"id":"nv-1","type":"bridge","serie":"tenue_a_l_ecart","pts":{"croisement":[45.0,5.0]}},
            {"id":"nv-2","type":"bridge","serie":"calibration","pts":{"croisement":[45.1,5.0]}}]"#;
        std::fs::write(private.join("v1/ouvrages-v1.json"), json).unwrap();
        let paths = crate::BenchPaths::new(dir.path(), Some(dir.path()), Some(&private));
        let m = crate::manifest::Manifest {
            v1: Some(crate::manifest::PersoV1 {
                ouvrages: ("private/v1/ouvrages-v1.json".into(), crate::hash::sha256_bytes(json.as_bytes())),
                ..Default::default()
            }),
            ..Default::default()
        };
        let ids = |m: &crate::manifest::Manifest, s| -> Vec<String> {
            series_ouvrages(&paths, m, s).unwrap().into_iter().map(|o| o.id).collect()
        };
        assert_eq!(ids(&m, crate::manifest::Serie::Calibration), ["nv-2"]);
        assert_eq!(ids(&m, crate::manifest::Serie::TenueALEcart), ["nv-1"]);
        let m = m.with_v1_judged();
        assert_eq!(ids(&m, crate::manifest::Serie::Calibration), ["nv-1", "nv-2"]);
        assert!(ids(&m, crate::manifest::Serie::TenueALEcart).is_empty());
    }

    #[test]
    fn a_passage_is_attributed_by_the_raw_trace() {
        let mid = LatLon::new(47.2, -1.5);
        // Pont routier sur l'axe, passerelle à 20 unités (≈ 8 m) au nord.
        let (tiles, u) = tiles_of(
            vec![
                (way_attrs("primary", StructureTag::Bridge), 0.0),
                (way_attrs("footway", StructureTag::Bridge), 20.0),
            ],
            mid,
        );
        let grid = Shortbread::GRID;
        let stations: Vec<LatLon> = (-8..=8).map(|k| grid.to_latlon([u[0] + 5.0 * k as f64, u[1]])).collect();
        let (p, _, voie) = tiles.perimeter_and_way(mid, &stations, Kind::Bridge);
        assert_eq!(p, Perimeter::InGraph);
        let voie = voie.expect("voie de l'ouvrage");
        // GPS brut sur la passerelle : passage sur un ouvrage parallèle.
        let on_footbridge: Vec<LatLon> =
            (-6..=6).map(|k| grid.to_latlon([u[0] + 5.0 * k as f64, u[1] + 19.0])).collect();
        let a = tiles.attribution(&on_footbridge, &voie, None).unwrap();
        assert!(!a.attributed(), "{a:?}");
        // GPS brut sur le pont routier : attribué.
        let on_road: Vec<LatLon> =
            (-6..=6).map(|k| grid.to_latlon([u[0] + 5.0 * k as f64, u[1] + 2.0])).collect();
        assert!(tiles.attribution(&on_road, &voie, None).unwrap().attributed());
        // Aucun point brut : non tranché.
        assert!(tiles.attribution(&[], &voie, None).is_none());
    }

    #[test]
    fn ways_of_one_deck_are_one_structure() {
        // Pont routier sur l'axe, trottoir du même tablier à 20 unités (≈ 8 m) : sans polygone, la
        // voie reste à moins de 10 m sur toute l'emprise (b) : même pont, le passage est attribué.
        let grid = Shortbread::GRID;
        let mid = LatLon::new(47.2, -1.5);
        let lines = || {
            vec![
                (way_attrs("primary", StructureTag::Bridge), 0.0),
                (way_attrs("footway", StructureTag::Bridge), 20.0),
            ]
        };
        let (mut tiles, u) = tiles_of(lines(), mid);
        let pts =
            [grid.to_latlon([u[0] - 40.0, u[1]]), grid.to_latlon(u), grid.to_latlon([u[0] + 40.0, u[1]])];
        let voie = format!("{:?}", way_attrs("primary", StructureTag::Bridge));
        let on_sidewalk: Vec<LatLon> =
            (-6..=6).map(|k| grid.to_latlon([u[0] + 5.0 * k as f64, u[1] + 19.0])).collect();
        let a = tiles.attribution(&on_sidewalk, &voie, Some(&pts)).unwrap();
        assert!(a.attributed() && a.same_bridge_ways == 1, "{a:?}");
        // Deux polygones `bridges` distincts (a) : deux ponts, passage sur l'ouvrage parallèle.
        let square = |y0: f64, y1: f64| {
            vec![vec![
                [u[0] - 60.0, u[1] + y0],
                [u[0] + 60.0, u[1] + y0],
                [u[0] + 60.0, u[1] + y1],
                [u[0] - 60.0, u[1] + y1],
                [u[0] - 60.0, u[1] + y0],
            ]]
        };
        let key = grid.tile_of(u);
        tiles.bridges.insert(key, vec![square(-5.0, 5.0), square(15.0, 25.0)]);
        let a = tiles.attribution(&on_sidewalk, &voie, Some(&pts)).unwrap();
        assert!(!a.attributed(), "{a:?}");
        // Un même polygone pour les deux voies (a) : un seul pont.
        tiles.bridges.insert(key, vec![square(-5.0, 25.0)]);
        assert!(tiles.attribution(&on_sidewalk, &voie, Some(&pts)).unwrap().attributed());
    }

    #[test]
    fn a_parallel_structure_passage_is_reported_not_counted() {
        let grid = Shortbread::GRID;
        let mid = LatLon::new(47.2, -1.5);
        let (tiles, u) = tiles_of(
            vec![
                (way_attrs("primary", StructureTag::Bridge), 0.0),
                (way_attrs("footway", StructureTag::Bridge), 40.0),
            ],
            mid,
        );
        let n = 41;
        let pts: Vec<LatLon> =
            (0..n).map(|k| grid.to_latlon([u[0] - 100.0 + 5.0 * k as f64, u[1]])).collect();
        let d: Vec<f64> = (0..n)
            .scan(0.0, |acc, k| {
                if k > 0 {
                    *acc += haversine_m(pts[k - 1], pts[k]);
                }
                Some(*acc)
            })
            .collect();
        let stations: Vec<LatLon> = (-8..=8).map(|k| grid.to_latlon([u[0] + 5.0 * k as f64, u[1]])).collect();
        let (perimeter, _, voie) = tiles.perimeter_and_way(mid, &stations, Kind::Bridge);
        let o = Ouvrage {
            id: "pont".into(),
            kind: Kind::Bridge,
            length_m: d[28] - d[12],
            pts: [pts[12], pts[20], pts[28]],
            perso: false,
            perimeter,
            perimeter_ambiguous: false,
            chain: Vec::new(),
            avalanche_protector: false,
            mnh_milieu: None,
            voie,
        };
        let flat = vec![0.0; n];
        let flags = vec![false; n];
        let footbridge = |_: f64, _: f64| -> Vec<LatLon> {
            (-6..=6).map(|k| grid.to_latlon([u[0] + 5.0 * k as f64, u[1] + 39.0])).collect()
        };
        let segs = [(d[12], d[28])];
        let p = CaseProfile {
            d: &d,
            pts: &pts,
            svc: &flat,
            raw: &flat,
            ref_svc: &flat,
            baro: &flat,
            baro_points: &d,
            on_structure: &flags,
            ign: &flags,
            matched: &flags,
            unmatched_warn: &flags,
            tile_structure: &flags,
            ring: None,
            structure_segs: &segs,
            raw_gps: Some(&footbridge),
            tiles: Some(&tiles),
            baro_fiable: true,
            baro_noise_m: None,
        };
        let items = perso_items("c", &p, std::slice::from_ref(&o), 40.0);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["metrique"], "passage_sur_ouvrage_parallele", "{items:?}");
        let (agg, v) = judge(&items);
        assert_eq!(agg["S1"]["emprise_m"], 0.0);
        assert_eq!(v["E3"], "skipped");
        assert_eq!(agg["passages_sur_ouvrage_parallele"].as_array().map(Vec::len), Some(1));
        // GPS brut sur le pont : compté, S1 en recouvrement continu de [s_A, s_B].
        let road = |_: f64, _: f64| -> Vec<LatLon> {
            (-6..=6).map(|k| grid.to_latlon([u[0] + 5.0 * k as f64, u[1]])).collect()
        };
        let p = CaseProfile { raw_gps: Some(&road), ..p };
        let items = perso_items("c", &p, &[o], 40.0);
        assert_eq!(items[0]["metrique"], "E3_hors_france", "{items:?}");
        assert_eq!(items[0]["S1.rappel"], 1.0, "{items:?}");
    }

    /// Croisement V1 (un point) : compté en E5 si la trace suit la voie du dessous, rapporté en
    /// `E5_passage_dessus` si elle suit le pont (voie du dessous lue dans les tuiles).
    #[test]
    fn a_crossing_counts_only_from_below() {
        let grid = Shortbread::GRID;
        let mid = LatLon::new(47.2, -1.5);
        let u = grid.to_units(mid);
        let key = grid.tile_of(u);
        let mut t = StreetTile::empty(key);
        // Pont est-ouest, voie au sol nord-sud sous le pont.
        t.attrs.push(way_attrs("primary", StructureTag::Bridge));
        t.attrs.push(way_attrs("residential", StructureTag::None));
        let seg = |a: [f64; 2], b: [f64; 2], attr: u32| ord_domain::street::StreetLine {
            attr,
            points: vec![a, b],
            cut: [false, false],
        };
        t.lines.push(seg([u[0] - 300.0, u[1]], [u[0] + 300.0, u[1]], 0));
        t.lines.push(seg([u[0], u[1] - 300.0], [u[0], u[1] + 300.0], 1));
        let tiles = Tiles { tiles: BTreeMap::from([(key, Some(Arc::new(t)))]), bridges: BTreeMap::new() };
        let (perimeter, _, voie) = tiles.perimeter_and_way(mid, &[mid, mid, mid], Kind::Under);
        assert_eq!(perimeter, Perimeter::InGraph);
        let o = Ouvrage {
            id: "nv-x".into(),
            kind: Kind::Under,
            length_m: 0.0,
            pts: [mid, mid, mid],
            perso: true,
            perimeter,
            perimeter_ambiguous: false,
            chain: Vec::new(),
            avalanche_protector: false,
            mnh_milieu: None,
            voie,
        };
        let run = |dx: f64, dy: f64| {
            let n = 41;
            let pts: Vec<LatLon> = (0..n)
                .map(|k| {
                    let f = k as f64 - 20.0;
                    grid.to_latlon([u[0] + dx * f, u[1] + dy * f])
                })
                .collect();
            let d: Vec<f64> = (0..n)
                .scan(0.0, |acc, k| {
                    if k > 0 {
                        *acc += haversine_m(pts[k - 1], pts[k]);
                    }
                    Some(*acc)
                })
                .collect();
            let (z, flags, yes) = (vec![0.0; n], vec![false; n], vec![true; n]);
            let p = CaseProfile {
                d: &d,
                pts: &pts,
                svc: &z,
                raw: &z,
                ref_svc: &z,
                baro: &z,
                baro_points: &d,
                on_structure: &flags,
                ign: &yes,
                matched: &flags,
                unmatched_warn: &flags,
                tile_structure: &flags,
                ring: None,
                structure_segs: &[],
                raw_gps: None,
                tiles: Some(&tiles),
                // Sans baro fiable : référence MNT (le baro n'a pas ici de stations à 200–1 500 m).
                baro_fiable: false,
                baro_noise_m: None,
            };
            perso_items("c", &p, std::slice::from_ref(&o), 40.0)
        };
        let below = run(0.0, 12.0);
        assert_eq!(below.len(), 1);
        assert_eq!(below[0]["metrique"], "E5", "{below:?}");
        let above = run(12.0, 0.0);
        assert_eq!(above[0]["metrique"], "E5_passage_dessus", "{above:?}");
    }

    #[test]
    fn a_v1_control_excludes_real_structures_and_closed_ranges() {
        let n = 201;
        let pts: Vec<LatLon> = (0..n).map(|k| LatLon::new(45.0 + k as f64 * 0.000045, 5.0)).collect();
        let d: Vec<f64> = (0..n).map(|k| k as f64 * 5.0).collect();
        let mut on = vec![false; n];
        let mut tile = vec![false; n];
        // Pont réel des tuiles aux stations 20–24, typé par le service : abords exclus.
        on[20..=24].fill(true);
        tile[20..=24].fill(true);
        // Ouvrage fictif aux stations 100–103 ; un autre dans une plage hors graphe (150–160).
        on[100..=103].fill(true);
        on[155] = true;
        let t = crate::v1::TemoinV1 {
            id: "pl-x".into(),
            trace: "pl-x".into(),
            planifie: true,
            nature: "digue".into(),
            serie: crate::manifest::Serie::Calibration,
            ends: None,
            real: Vec::new(),
            length_m: None,
            declared_m: None,
        };
        let closed = [750.0..800.0];
        let it = temoin_v1_item(&t, &d, &pts, &on, &tile, &closed, None, 40.0).unwrap().pop().unwrap();
        assert_eq!(it["ouvrages_fictifs"], 1, "{it}");
        assert_eq!(it["faux_positif_m"], 20.0, "{it}");
        assert_eq!(it["hors_graphe_velo_m"], 50.0, "{it}");
        assert!(it["abords_ouvrages_m"].as_f64().unwrap() > 200.0, "{it}");
        assert_eq!(it["nature"], "digue");
        // Témoin de sortie : tronçon entre ses extrémités ; extrémités introuvables : None.
        let s = crate::v1::TemoinV1 { planifie: false, ends: Some([pts[90], pts[110]]), ..t.clone() };
        let it = temoin_v1_item(&s, &d, &pts, &on, &tile, &[], None, 40.0).unwrap().pop().unwrap();
        assert_eq!(it["longueur_m"], 100.0, "{it}");
        // Témoin à extrémités sur un itinéraire planifié (`pl-…-sN`, affc041) : entre ses
        // extrémités, pas sur tout l'itinéraire.
        let ps = crate::v1::TemoinV1 { id: "pl-x-s0".into(), planifie: true, ..s.clone() };
        let it = temoin_v1_item(&ps, &d, &pts, &on, &tile, &[], None, 40.0).unwrap().pop().unwrap();
        assert_eq!(it["longueur_m"], 100.0, "{it}");
        // Ouvrage réel sur la voie (README §4 ter.6) : l'ouvrage fictif des stations 100–103 est
        // un pont tagué, plage ± 35 m masquée et rapportée.
        let real = crate::v1::RealRange { tag: "bridge".into(), ends: [pts[100], pts[103]], length_m: 15.0 };
        let r = crate::v1::TemoinV1 { real: vec![real.clone()], length_m: Some(1_000.0), ..t.clone() };
        let items = temoin_v1_item(&r, &d, &pts, &on, &tile, &closed, None, 40.0).unwrap();
        assert_eq!(items[0]["metrique"], "S2_ouvrage_reel", "{items:?}");
        let it = items.last().unwrap();
        assert_eq!(it["ouvrages_fictifs"], 0, "{it}");
        assert_eq!(it["ouvrage_reel_masque_m"], 90.0, "{it}");
        // Plus de la moitié du témoin : témoin sorti de S2.
        let big = crate::v1::TemoinV1 { real: vec![real], length_m: Some(20.0), ..t.clone() };
        let items = temoin_v1_item(&big, &d, &pts, &on, &tile, &closed, None, 40.0).unwrap();
        assert_eq!(items.last().unwrap()["metrique"], "S2_temoin_sorti", "{items:?}");
        assert!(items.iter().all(|i| i["metrique"] != "S2"));
        let far = crate::v1::TemoinV1 { ends: Some([LatLon::new(46.0, 5.0), pts[110]]), ..s };
        assert!(temoin_v1_item(&far, &d, &pts, &on, &tile, &[], None, 40.0).is_none());
    }

    #[test]
    fn a_v1_control_start_is_searched_before_its_end() {
        // Trace au pas de 5 m : vers le nord (stations 0–60), retour vers le sud jusqu'au point de
        // la station 20 (station 100), puis vers l'est jusqu'à la fin du témoin (station 200). Le
        // début du témoin est passé deux fois (stations 20 et 100) ; sa longueur déclarée est de
        // 500 m : le début est cherché dans les 750 m avant la fin, pas sur toute la trace
        // (décision de l'utilisateur, 2026-11-04).
        let n = 201;
        let step_lat = 0.000045;
        let step_lon = step_lat / 45f64.to_radians().cos();
        let pts: Vec<LatLon> = (0..n)
            .map(|k| match k {
                0..=60 => LatLon::new(45.0 + k as f64 * step_lat, 5.0),
                61..=100 => LatLon::new(45.0 + (120 - k) as f64 * step_lat, 5.0),
                _ => LatLon::new(45.0 + 20.0 * step_lat, 5.0 + (k - 100) as f64 * step_lon),
            })
            .collect();
        let d: Vec<f64> = (0..n).map(|k| k as f64 * 5.0).collect();
        let none = vec![false; n];
        let t = crate::v1::TemoinV1 {
            id: "perso-x-s0".into(),
            trace: "perso-x".into(),
            planifie: false,
            nature: "remblai_deblai".into(),
            serie: crate::manifest::Serie::Calibration,
            ends: Some([pts[20], pts[200]]),
            real: Vec::new(),
            length_m: None,
            declared_m: Some(500.0),
        };
        let it = temoin_v1_item(&t, &d, &pts, &none, &none, &[], None, 40.0).unwrap().pop().unwrap();
        assert_eq!(it["longueur_m"], 500.0, "{it}");
        // Sans longueur déclarée : premier passage du début, sur toute la trace.
        let sans = crate::v1::TemoinV1 { declared_m: None, ..t.clone() };
        let it = temoin_v1_item(&sans, &d, &pts, &none, &none, &[], None, 40.0).unwrap().pop().unwrap();
        assert_eq!(it["longueur_m"], 900.0, "{it}");
        // Témoin parcouru à rebours (début après la fin) : retrouvé dans la fenêtre d'après.
        let rebours = crate::v1::TemoinV1 { ends: Some([pts[200], pts[120]]), ..t.clone() };
        let it = temoin_v1_item(&rebours, &d, &pts, &none, &none, &[], None, 40.0).unwrap().pop().unwrap();
        assert_eq!(it["longueur_m"], 400.0, "{it}");
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
        let t = tunnel_item("t", "c", &d, &svc, &raw, &ALL, &blocked, 20, 40);
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
    fn objects_of_a_negative() {
        // README §4 ter.6 (eb4c4cc) : la galerie de tn12 est l'objet du témoin ; un barrage
        // (tn10), un viaduc passé dessous ou un tunnel passé dessus ne le sont pas.
        let bench = crate::BenchPaths::repo_bench();
        let cat = control_category(&bench, "tn12-arly-galleries").unwrap();
        assert!(names_object(&cat, "gallery") && !names_object(&cat, "bridge"));
        let cat = control_category(&bench, "tn10-roselend").unwrap();
        assert!(!names_object(&cat, "bridge"));
        for (c, n) in [
            ("under-viaduct-approach", "bridge"),
            ("quay-under-bridges", "bridge"),
            ("road-over-col-above-tunnel", "tunnel"),
            ("urban-street-covered-carriageway", "covered"),
        ] {
            assert!(!names_object(c, n), "{c}");
        }
        assert!(names_object("x-tunnel-y", "tunnel") && names_object("long-viaduct", "bridge"));
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
            mnh_milieu: None,
            voie: None,
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
                ref_svc: &raw,
                baro,
                baro_points: &d,
                on_structure: &flags,
                ign: &flags,
                matched: &flags,
                unmatched_warn: &flags,
                tile_structure: &flags,
                ring: None,
                structure_segs: &[],
                raw_gps: None,
                tiles: None,
                baro_fiable: true,
                baro_noise_m: None,
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
        assert!(matches!(profile(&gap), Approaches::Fallback { after: 0, .. }));
        // Approche sur un autre ouvrage du corpus (accès non tagué) : stations exclues, repli.
        assert!(matches!(profile_with(&baro, &[(40, 95)]), Approaches::Fallback { before: 0, .. }));
    }

    #[test]
    fn an_implausible_implicit_grade_makes_the_reference_unavailable() {
        let d: Vec<f64> = (0..41).map(|k| k as f64 * 5.0).collect();
        // Tunnel des stations 10 à 14 (20 m) ; MNT à 100 m avant, 140 m après : 40 m sur 40 m.
        let raw: Vec<f64> = (0..41).map(|k| if k < 12 { 100.0 } else { 140.0 }).collect();
        let t = tunnel_item("t", "c", &d, &raw, &raw, &ALL, &[false; 41], 10, 14);
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
    fn a_high_bridge_passage_needs_the_baro_on_the_deck() {
        // Pont de 200 m (stations 180 à 220, pas de 5 m), MNH 50 m ; MNT plat à 0.
        let n = 401;
        let m_per_deg = haversine_m(LatLon::new(47.0, 0.0), LatLon::new(47.0, 1.0));
        let d: Vec<f64> = (0..n).map(|k| k as f64 * 5.0).collect();
        let pts: Vec<LatLon> = d.iter().map(|x| LatLon::new(47.0, x / m_per_deg)).collect();
        let raw = vec![0.0; n];
        let flags = vec![false; n];
        let ign = vec![true; n];
        let o = |mnh: Option<f64>| Ouvrage {
            id: "pont".into(),
            kind: Kind::Bridge,
            length_m: 200.0,
            pts: [pts[180], pts[200], pts[220]],
            perso: false,
            perimeter: Perimeter::InGraph,
            perimeter_ambiguous: false,
            chain: Vec::new(),
            avalanche_protector: false,
            mnh_milieu: mnh,
            voie: None,
        };
        let items = |baro: &[f64], mnh: Option<f64>| {
            let p = CaseProfile {
                d: &d,
                pts: &pts,
                svc: &raw,
                raw: &raw,
                ref_svc: &raw,
                baro,
                baro_points: &d,
                on_structure: &flags,
                ign: &ign,
                matched: &flags,
                unmatched_warn: &flags,
                tile_structure: &flags,
                ring: None,
                structure_segs: &[],
                raw_gps: None,
                tiles: None,
                baro_fiable: true,
                baro_noise_m: None,
            };
            perso_items("c", &p, &[o(mnh)], 40.0)
        };
        // Trace au sol sous le tablier : retirée.
        let ground = vec![0.0; n];
        let i = items(&ground, Some(50.0));
        assert_eq!(i.len(), 1);
        assert_eq!(i[0]["metrique"], "passage_retire_niveau", "{i:?}");
        // Baro sur le tablier : compté.
        let deck: Vec<f64> = (0..n).map(|k| if (180..=220).contains(&k) { 50.0 } else { 0.0 }).collect();
        let i = items(&deck, Some(50.0));
        assert_eq!(i[0]["metrique"], "E3", "{i:?}");
        assert!(i[0].get("niveau_non_discriminant").is_none());
        // MNH sous 10 m : critère horizontal seul, mentionné.
        let i = items(&ground, Some(6.0));
        assert_eq!(
            (i[0]["metrique"].clone(), i[0]["niveau_non_discriminant"].clone()),
            (json!("E3"), json!(true))
        );
        // Pas de baro au milieu : mentionné, compté.
        let gap: Vec<f64> = (0..n).map(|k| if (190..=210).contains(&k) { f64::NAN } else { 0.0 }).collect();
        let i = items(&gap, Some(50.0));
        assert_eq!(i[0]["niveau_sans_baro"], true, "{i:?}");
    }

    #[test]
    fn references_do_not_depend_on_the_tested_output() {
        // Pont de 200 m (stations 180 à 220) et tunnel (stations 300 à 340) sur un MNT en pente
        // douce ; même référence (`raw`, `ref_svc`), deux sorties testées différentes : mêmes
        // passages, décalages, mentions de niveau et bandes de portail ; seuls les écarts changent.
        let n = 401;
        let m_per_deg = haversine_m(LatLon::new(47.0, 0.0), LatLon::new(47.0, 1.0));
        let d: Vec<f64> = (0..n).map(|k| k as f64 * 5.0).collect();
        let pts: Vec<LatLon> = d.iter().map(|x| LatLon::new(47.0, x / m_per_deg)).collect();
        let raw: Vec<f64> = d.iter().map(|x| 100.0 + 0.01 * x).collect();
        let flags = vec![false; n];
        let ign = vec![true; n];
        let on: Vec<bool> = (0..n).map(|k| (180..=220).contains(&k) || (300..=340).contains(&k)).collect();
        let ouvrage = |id: &str, kind: Kind, a: usize, b: usize, mnh: Option<f64>| Ouvrage {
            id: id.into(),
            kind,
            length_m: d[b] - d[a],
            pts: [pts[a], pts[(a + b) / 2], pts[b]],
            perso: false,
            perimeter: Perimeter::InGraph,
            perimeter_ambiguous: false,
            chain: Vec::new(),
            avalanche_protector: false,
            mnh_milieu: mnh,
            voie: None,
        };
        let os = [
            ouvrage("pont", Kind::Bridge, 180, 220, Some(12.0)),
            ouvrage("tunnel", Kind::Tunnel, 300, 340, None),
        ];
        let baro: Vec<f64> =
            (0..n).map(|k| raw[k] + 3.0 + if (180..=220).contains(&k) { 12.0 } else { 0.0 }).collect();
        let items = |svc: &[f64]| {
            let p = CaseProfile {
                d: &d,
                pts: &pts,
                svc,
                raw: &raw,
                ref_svc: &raw,
                baro: &baro,
                baro_points: &d,
                on_structure: &on,
                ign: &ign,
                matched: &flags,
                unmatched_warn: &flags,
                tile_structure: &flags,
                ring: None,
                structure_segs: &[],
                raw_gps: None,
                tiles: None,
                baro_fiable: true,
                baro_noise_m: None,
            };
            perso_items("c", &p, &os, 40.0)
        };
        let a = items(&raw);
        // Sortie testée interpolée grossièrement (points tous les 50 m) et bruitée sur les ouvrages.
        let coarse: Vec<f64> =
            (0..n).map(|k| raw[k - k % 10] + if on[k] { 12.0 + (k % 7) as f64 * 0.3 } else { 0.0 }).collect();
        let b = items(&coarse);
        assert_eq!(a.len(), b.len());
        let metrics: Vec<&str> = a.iter().filter_map(|i| i["metrique"].as_str()).collect();
        assert!(metrics.contains(&"E3") && metrics.contains(&"E4"), "{metrics:?}");
        let reference_keys = |v: &Value| -> Value {
            let keep = [
                "ouvrage",
                "metrique",
                "decalage_approches_m",
                "decalage_local_m",
                "bande_remplacee",
                "niveau_non_discriminant",
                "niveau_sans_baro",
                "reference_incertaine",
                "d_milieu_m",
            ];
            Value::Object(
                v.as_object()
                    .unwrap()
                    .iter()
                    .filter(|(k, _)| keep.contains(&k.as_str()))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            )
        };
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(reference_keys(x), reference_keys(y));
        }
        assert_ne!(a, b, "les écarts du service changent");
    }

    #[test]
    fn corpus_mnh_reads_the_median_dsm_minus_the_dtm() {
        let o = json!({"valeurs": {"milieu": {"lidar_mnt": 1.5, "lidar_mns": 12.0}},
            "milieu_mns_mediane_25m": {"valeur": 11.5}});
        assert_eq!(corpus_mnh(&o), Some(10.0));
        let o = json!({"reference": {"valeurs": {"milieu": {"lidar_mnt": 2.0, "lidar_mns": 9.0}}}});
        assert_eq!(corpus_mnh(&o), Some(7.0));
        assert_eq!(corpus_mnh(&json!({})), None);
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
            mnh_milieu: None,
            voie: None,
        };
        let mut items = vec![
            absent_item(&o, "c1"),
            absent_item(&o, "c2"),
            json!({"ouvrage": "g#1", "metrique": "E4", "pass": false, "ecart_max_m": 12.0,
                "S1.emprise_m": 100.0, "S1.type_m": 0.0, "_longueur_m": 100.0}),
            json!({"ouvrage": "gx", "metrique": "E4", "pass": true, "S1.emprise_m": 50.0, "S1.type_m": 50.0}),
            json!({"ouvrage": "p", "metrique": "E3", "ecart_milieu_m": 9.0, "ecart_max_m": 9.0,
                "longueur_m": 50.0, "limite_donnees": "note"}),
        ];
        annotate_data_limits(&mut items, &[("g".into(), "note".into())]);
        assert_eq!(items[2]["limite_donnees"], "note");
        assert!(items[3].get("limite_donnees").is_none());
        let (agg, v) = judge(&items);
        assert_eq!(agg["E3_non_couverts"].as_array().map(Vec::len), Some(1));
        assert_eq!(agg["E3"]["ponts"], 0, "un pont limité sort d'E3");
        assert_eq!(agg["S1"]["emprise_m"], 50.0, "ni la galerie ni le pont dans S1");
        assert_eq!(v["S1"], "pass");
        assert_eq!(agg["limite_donnees"].as_array().map(Vec::len), Some(2));
        assert!(agg["limite_donnees"][0].get("_longueur_m").is_none());
        assert_eq!(agg["limite_donnees"][0]["ecart_max_m"], 12.0, "erreur mesurée, rapportée");
        assert!(agg["limite_donnees"][0].get("pass").is_none(), "sans seuil");
        assert_eq!(agg["E4"]["tunnels"], 1, "la galerie sort du décompte d'E4");
        assert_eq!(agg["limite_donnees"][0]["ouvrage"], "g#1");
        assert_eq!(agg["limite_donnees"][0]["limite_donnees"], "note");
        assert_eq!(v["E4"], "pass", "l'échec de la galerie ne compte pas");
    }

    #[test]
    fn tunnel_reference_flags_an_ambiguous_portal() {
        let d: Vec<f64> = (0..41).map(|k| k as f64 * 5.0).collect();
        // Tunnel des stations 10 à 30, service plat à 10 m ; falaise à 5 m de l'entrée.
        let raw: Vec<f64> = (0..41).map(|k| if k == 9 { 40.0 } else { 10.0 }).collect();
        let svc = vec![10.0; 41];
        let t = tunnel_item("t", "c", &d, &svc, &raw, &ALL, &[false; 41], 10, 30);
        assert_eq!(t["pass"], true);
        assert_eq!(t["reference_ambigue"], true, "le portail à 5 m fait basculer le verdict");
    }
}

#[cfg(test)]
mod e5_tests {
    use super::*;

    /// Trace de 400 m au pas de 5 m, croisement au milieu (station 40), le long de l'est (`east`)
    /// ou du nord.
    fn line(mid: LatLon, east: bool) -> (Vec<f64>, Vec<LatLon>) {
        let m_lat = 111_320.0;
        let m_lon = m_lat * mid.lat.to_radians().cos();
        let d: Vec<f64> = (0..=80).map(|k| k as f64 * 5.0).collect();
        let pts = d
            .iter()
            .map(|&x| {
                let s = x - 200.0;
                if east {
                    LatLon::new(mid.lat, mid.lon + s / m_lon)
                } else {
                    LatLon::new(mid.lat + s / m_lat, mid.lon)
                }
            })
            .collect();
        (d, pts)
    }

    fn input<'a>(
        d: &'a [f64],
        pts: &'a [LatLon],
        raw: &'a [f64],
        baro: Option<&'a [f64]>,
        tiles: Option<&'a Tiles>,
    ) -> E5Input<'a> {
        E5Input { d, pts, raw, ign: None, baro, baro_points: d, tiles, at: pts[40], im: 40, ends: [40, 40], baro_noise_m: None }
    }

    /// MNT plat à 100 m avec une bosse (`h` > 0) ou une cuvette (`h` < 0) de ± 10 m au croisement.
    fn dtm(d: &[f64], h: f64) -> Vec<f64> {
        d.iter().map(|&x| if (x - 200.0).abs() <= 10.0 { 100.0 + h } else { 100.0 }).collect()
    }

    #[test]
    fn the_dtm_reads_the_deck_beyond_two_metres_only() {
        let mid = LatLon::new(47.2, -1.5);
        let (d, pts) = line(mid, false);
        let bump = dtm(&d, 4.0);
        assert!((e5_bump(&d, &bump, 40, [40, 40]).unwrap() - 4.0).abs() < 1e-9);
        assert_eq!(
            e5_reference(&input(&d, &pts, &bump, None, None)),
            E5Reference::Indisponible { raison: "mnt_lit_le_tablier" }
        );
        // Bosse de 0,9 m : référence MNT. Cuvette : référence MNT (les passages dessous creusent).
        for h in [0.9, -3.0] {
            let z = dtm(&d, h);
            assert_eq!(e5_reference(&input(&d, &pts, &z, None, None)), E5Reference::Mnt { z: 100.0 + h });
        }
        // Bosse de 1,0 à 2,0 m : référence MNT incertaine (e477dd4).
        for h in [1.0, 1.9, 2.0] {
            let z = dtm(&d, h);
            assert_eq!(
                e5_reference(&input(&d, &pts, &z, None, None)),
                E5Reference::Incertaine {
                    reference: "mnt",
                    raison: "bosse_mnt",
                    z: 100.0 + h,
                    offset_m: None,
                    sides: None
                }
            );
        }
        // MNT sans donnée à ≤ 10 m.
        let mut hole = dtm(&d, 0.0);
        hole[42] = f64::NAN;
        assert_eq!(
            e5_reference(&input(&d, &pts, &hole, None, None)),
            E5Reference::Indisponible { raison: "mnt_sans_donnee" }
        );
    }

    #[test]
    fn a_reliable_baro_is_the_reference_with_its_local_offset() {
        let mid = LatLon::new(47.2, -1.5);
        let m_lat = 111_320.0;
        // Trace de 4 km pour lire le décalage de 200 à 1 500 m.
        let d: Vec<f64> = (0..=800).map(|k| k as f64 * 5.0).collect();
        let pts: Vec<LatLon> =
            d.iter().map(|&x| LatLon::new(mid.lat + (x - 2_000.0) / m_lat, mid.lon)).collect();
        let raw: Vec<f64> =
            d.iter().map(|&x| if (x - 2_000.0).abs() <= 10.0 { 106.0 } else { 100.0 }).collect();
        let baro: Vec<f64> = d.iter().map(|_| 103.0).collect();
        let x = E5Input {
            d: &d,
            pts: &pts,
            raw: &raw,
            ign: None,
            baro: Some(&baro),
            baro_points: &d,
            tiles: None,
            at: pts[400],
            im: 400,
            ends: [400, 400],
            baro_noise_m: None,
        };
        // 261 stations de 200 à 1 500 m de chaque côté.
        let sides = BaroSides { offset_m: [Some(3.0), Some(3.0)], points: [261, 261] };
        assert_eq!(e5_reference(&x), E5Reference::Baro { z: 100.0, offset_m: 3.0, sides });
        // Baro bruité (README §4 quater.2) : bruit de 1,0 m gardé, au-delà incertain, sans repli
        // sur le MNT, avant les côtés (ici trop peu de points d'un côté).
        assert!(matches!(e5_reference(&E5Input { baro_noise_m: Some(1.0), ..x }), E5Reference::Baro { .. }));
        let noisy = E5Input { baro_noise_m: Some(1.02), baro_points: &d[..300], ..x };
        assert_eq!(
            e5_reference(&noisy),
            E5Reference::Incertaine {
                reference: "baro_corrige_local",
                raison: "baro_bruite",
                z: 100.0,
                offset_m: Some(3.0),
                sides: Some(BaroSides { offset_m: [Some(3.0), Some(3.0)], points: [200, 0] }),
            }
        );
        // Baro absent au croisement (pas brut de plus de 100 m) : MNT, ici indisponible.
        let mut gap = baro.clone();
        gap[400] = f64::NAN;
        let x = E5Input { baro: Some(&gap), ..x };
        assert_eq!(e5_reference(&x), E5Reference::Indisponible { raison: "mnt_lit_le_tablier" });
    }

    #[test]
    fn a_trace_on_a_structure_of_the_tiles_is_withdrawn() {
        let mid = LatLon::new(47.2, -1.5);
        // Pont est-ouest sur le croisement.
        let (tiles, _) = super::tests::tiles_of(
            vec![(super::tests::way_attrs("primary", StructureTag::Bridge), 0.0)],
            mid,
        );
        let (d, along) = line(mid, true);
        let z = dtm(&d, 0.0);
        assert!(matches!(
            e5_reference(&input(&d, &along, &z, None, Some(&tiles))),
            E5Reference::Retire { angle_deg, .. } if angle_deg < 1.0
        ));
        // Trace nord-sud, sous le pont : croisement à 90°, compté.
        let (d, across) = line(mid, false);
        assert_eq!(e5_reference(&input(&d, &across, &z, None, Some(&tiles))), E5Reference::Mnt { z: 100.0 });
    }

    /// Trace de 4 km, croisement au milieu (station 400), MNT plat à 100 m, baro donné.
    fn long_line() -> (Vec<f64>, Vec<LatLon>, Vec<f64>) {
        let mid = LatLon::new(47.2, -1.5);
        let m_lat = 111_320.0;
        let d: Vec<f64> = (0..=800).map(|k| k as f64 * 5.0).collect();
        let pts = d.iter().map(|&x| LatLon::new(mid.lat + (x - 2_000.0) / m_lat, mid.lon)).collect();
        let raw = vec![100.0; d.len()];
        (d, pts, raw)
    }

    /// f1bfda3 : décalages des deux côtés à plus de 2 m l'un de l'autre, référence incertaine ;
    /// à 2 m au plus, référence baro, tolérance 2 m.
    #[test]
    fn a_baro_whose_sides_disagree_is_uncertain() {
        let (d, pts, raw) = long_line();
        let run = |after: f64| {
            let baro: Vec<f64> = d.iter().map(|&x| if x > 2_000.0 { 100.0 + after } else { 103.0 }).collect();
            let x = E5Input {
                d: &d,
                pts: &pts,
                raw: &raw,
                ign: None,
                baro: Some(&baro),
                baro_points: &d,
                tiles: None,
                at: pts[400],
                im: 400,
                ends: [400, 400],
                baro_noise_m: None,
            };
            e5_reference(&x)
        };
        assert!(
            matches!(run(5.5), E5Reference::Incertaine { reference: "baro_corrige_local", raison: "decalages_des_cotes", sides: Some(s), .. }
                if s.offset_m == [Some(3.0), Some(5.5)]),
            "{:?}",
            run(5.5)
        );
        assert!(matches!(run(4.9), E5Reference::Baro { .. }), "{:?}", run(4.9));
        // Écart compté contre 2 m avec cette référence.
        let ecart = |z: f64| (102.5 - z).abs();
        let Ok(m) = e5_item(
            "o",
            "c",
            E5Reference::Baro {
                z: 100.6,
                offset_m: 3.0,
                sides: BaroSides { offset_m: [Some(3.0); 2], points: [9; 2] },
            },
            &ecart,
            None,
            None,
        ) else {
            panic!("compté")
        };
        assert_eq!(m["pass"], true);
        assert_eq!(m["seuil_m"], 2.0);
        let Err(v) = e5_item("o", "c", run(5.5), &ecart, None, None) else { panic!("non compté") };
        assert_eq!(v["metrique"], "e5_reference_incertaine", "{v}");
    }

    /// e9430ac : un côté à moins de 3 points de baro entre 200 et 1 500 m, référence incertaine.
    #[test]
    fn a_baro_on_one_side_only_is_uncertain() {
        let (d, pts, raw) = long_line();
        // Baro de 50 m avant le croisement jusqu'au bout ; avant, 2 points seulement à 205–210 m.
        let baro: Vec<f64> = d
            .iter()
            .map(|&x| if x >= 1_950.0 || x == 1_790.0 || x == 1_795.0 { 103.0 } else { f64::NAN })
            .collect();
        let points: Vec<f64> = d.iter().zip(&baro).filter(|(_, z)| z.is_finite()).map(|(s, _)| *s).collect();
        let x = E5Input {
            d: &d,
            pts: &pts,
            raw: &raw,
            ign: None,
            baro: Some(&baro),
            baro_points: &points,
            tiles: None,
            at: pts[400],
            im: 400,
            ends: [400, 400],
            baro_noise_m: None,
        };
        let r = e5_reference(&x);
        assert!(
            matches!(r, E5Reference::Incertaine { raison: "points_baro_insuffisants", sides: Some(s), .. } if s.points[0] == 2),
            "{r:?}"
        );
    }

    /// 6bc77a5 : les points d'un côté sont les points bruts de baro, pas les stations de la grille
    /// interpolées entre eux ; 1477b5f : côté sans baro, incertaine sans repli sur le MNT, même
    /// quand le MNT serait une référence valable.
    #[test]
    fn baro_points_are_raw_points_and_a_side_without_baro_never_falls_back_to_the_dtm() {
        let (d, pts, raw) = long_line();
        // Baro interpolé sur toute la grille, mais 2 points bruts seulement entre 200 et 1 500 m
        // avant le croisement (pas brut ≤ 100 m ailleurs).
        let baro = vec![103.0; d.len()];
        let mut points: Vec<f64> = (0..=40).map(|k| 2_000.0 + k as f64 * 50.0).collect();
        points.extend([500.0, 1_700.0, 1_850.0, 1_950.0]);
        points.sort_by(f64::total_cmp);
        let x = E5Input {
            d: &d,
            pts: &pts,
            raw: &raw,
            ign: None,
            baro: Some(&baro),
            baro_points: &points,
            tiles: None,
            at: pts[400],
            im: 400,
            ends: [400, 400],
            baro_noise_m: None,
        };
        let r = e5_reference(&x);
        assert!(
            matches!(r, E5Reference::Incertaine { reference: "baro_corrige_local", raison: "points_baro_insuffisants", sides: Some(s), .. }
                if s.points == [2, 27] && s.offset_m == [Some(3.0), Some(3.0)]),
            "{r:?}"
        );
        // Trois points bruts : référence baro.
        let mut three = points.clone();
        three.push(1_000.0);
        three.sort_by(f64::total_cmp);
        assert!(matches!(e5_reference(&E5Input { baro_points: &three, ..x }), E5Reference::Baro { .. }));
        // Trace non fiable (baro absent de l'entrée) : MNT plat, référence MNT.
        assert_eq!(e5_reference(&E5Input { baro: None, ..x }), E5Reference::Mnt { z: 100.0 });
    }

    /// 085d26e : les points bruts d'un côté se comptent à leur abscisse corrigée du retard,
    /// s_p − δ ; trois points juste après les 200 m du côté aval en sortent avec δ = 60 m (cas de
    /// `nv-343` : 3 points à l'abscisse de `baro.csv`, 1 à l'abscisse corrigée). Les fenêtres
    /// masquées se lisent à la même abscisse.
    #[test]
    fn baro_points_are_counted_at_their_lag_corrected_abscissa() {
        let (d, pts, raw) = long_line();
        let baro = vec![103.0; d.len()];
        // Côté amont : un point tous les 50 m. Côté aval (croisement à 2 000 m) : 2 210, 2 230,
        // 2 250 m puis rien avant 3 000 m.
        let mut raw_points: Vec<f64> = (0..40).map(|k| k as f64 * 50.0).collect();
        raw_points.extend([2_210.0, 2_230.0, 2_250.0, 3_000.0, 3_050.0]);
        let count = |bd_corr: &[f64], masks: &[std::ops::Range<f64>]| {
            let points = crate::campaign::e5_baro_points(bd_corr, masks, None);
            let x = E5Input {
                d: &d,
                pts: &pts,
                raw: &raw,
                ign: None,
                baro: Some(&baro),
                baro_points: &points,
                tiles: None,
                at: pts[400],
                im: 400,
                ends: [400, 400],
                baro_noise_m: None,
            };
            e5_reference(&x)
        };
        // δ = 0 : 5 points en aval, référence baro.
        assert!(matches!(count(&raw_points, &[]), E5Reference::Baro { sides, .. } if sides.points[1] == 5));
        // δ = 60 m : 2 150, 2 170, 2 190, 2 940, 2 990 ; 2 en aval, incertaine.
        let corrected: Vec<f64> = raw_points.iter().map(|s| s - 60.0).collect();
        assert!(
            matches!(count(&corrected, &[]), E5Reference::Incertaine { raison: "points_baro_insuffisants", sides: Some(s), .. }
                if s.points[1] == 2),
            "{:?}",
            count(&corrected, &[])
        );
        // Fenêtre masquée [2 960, 3 020) : lue à l'abscisse corrigée, elle retire 2 990 (brut
        // 3 050), pas 2 940 (brut 3 000, qui serait dedans à l'abscisse de `baro.csv`).
        let kept = crate::campaign::e5_baro_points(&corrected, &[2_960.0..3_020.0], None);
        assert!(kept.contains(&2_940.0) && !kept.contains(&2_990.0), "{kept:?}");
    }

    /// Tuile de lignes données en mètres (est, nord) autour de `mid`.
    /// Ligne en mètres : nature d'ouvrage, `highway`, sommets (est, nord).
    type LineM<'a> = (StructureTag, &'a str, Vec<(f64, f64)>);

    fn tiles_m(mid: LatLon, lines: Vec<LineM<'_>>) -> Tiles {
        let grid = Shortbread::GRID;
        let m_lat = 111_320.0;
        let m_lon = m_lat * mid.lat.to_radians().cos();
        let key = grid.tile_of(grid.to_units(mid));
        let mut t = StreetTile::empty(key);
        for (k, (structure, kind, pts)) in lines.into_iter().enumerate() {
            t.attrs.push(super::tests::way_attrs(kind, structure));
            t.lines.push(ord_domain::street::StreetLine {
                attr: k as u32,
                points: pts
                    .iter()
                    .map(|&(e, n)| grid.to_units(LatLon::new(mid.lat + n / m_lat, mid.lon + e / m_lon)))
                    .collect(),
                cut: [false, false],
            });
        }
        Tiles { tiles: BTreeMap::from([(key, Some(Arc::new(t)))]), bridges: BTreeMap::new() }
    }

    fn approche_sur_ouvrage(z: f64) -> E5Reference {
        E5Reference::Incertaine {
            reference: "mnt",
            raison: "approche_sur_ouvrage",
            z,
            offset_m: None,
            sides: None,
        }
    }

    /// f38b745, 2e511e5 : trace nord-sud sous un pont est-ouest ; une approche (20–50 m) sur un
    /// pont emprunté (ligne nord-sud à ≤ 10 m, < 20°) rend la référence MNT incertaine, avant la
    /// bosse ; un ouvrage à plus de 10 m, ou qui croise l'approche, ne compte pas.
    #[test]
    fn an_approach_on_another_structure_makes_the_dtm_uncertain() {
        let mid = LatLon::new(47.2, -1.5);
        let (d, pts) = line(mid, false);
        let above = (StructureTag::Bridge, "primary", vec![(-60.0, 0.0), (60.0, 0.0)]);
        let flat = dtm(&d, 0.0);
        let deck = dtm(&d, 4.0);
        // Pont emprunté de 25 à 60 m au nord du croisement, à 3 m de l'axe de la trace.
        for (structure, kind) in [(StructureTag::Bridge, "cycleway"), (StructureTag::Tunnel, "rail")] {
            let tiles = tiles_m(mid, vec![above.clone(), (structure, kind, vec![(3.0, 25.0), (3.0, 60.0)])]);
            assert_eq!(
                e5_reference(&input(&d, &pts, &flat, None, Some(&tiles))),
                approche_sur_ouvrage(100.0)
            );
            // Testée avant la bosse : une bosse de 4 m ne la rend pas indisponible.
            assert_eq!(
                e5_reference(&input(&d, &pts, &deck, None, Some(&tiles))),
                approche_sur_ouvrage(104.0)
            );
        }
        // Pont parallèle à 12 m de l'axe, ou pont qui croise l'approche à 30 m : compté au MNT.
        for other in [vec![(12.0, 25.0), (12.0, 60.0)], vec![(-60.0, 30.0), (60.0, 30.0)]] {
            let tiles = tiles_m(mid, vec![above.clone(), (StructureTag::Bridge, "cycleway", other)]);
            assert_eq!(
                e5_reference(&input(&d, &pts, &flat, None, Some(&tiles))),
                E5Reference::Mnt { z: 100.0 }
            );
        }
        // Voie ordinaire le long de l'approche : compté au MNT.
        let tiles =
            tiles_m(mid, vec![above, (StructureTag::None, "cycleway", vec![(3.0, 25.0), (3.0, 60.0)])]);
        assert_eq!(e5_reference(&input(&d, &pts, &flat, None, Some(&tiles))), E5Reference::Mnt { z: 100.0 });
    }

    /// f38b745 : les voies de l'ouvrage du dessus ne comptent pas, même quand la bretelle du pont
    /// revient le long de l'approche ; le même tracé, porté par un autre ouvrage, compte.
    #[test]
    fn the_ways_of_the_structure_above_do_not_count() {
        let mid = LatLon::new(47.2, -1.5);
        let (d, pts) = line(mid, false);
        let flat = dtm(&d, 0.0);
        // Pont est-ouest qui croise la trace puis revient vers le nord à 6 m de son axe.
        let ramp = vec![(-60.0, 0.0), (25.0, 0.0), (6.0, 25.0), (6.0, 60.0)];
        let tiles = tiles_m(mid, vec![(StructureTag::Bridge, "primary", ramp)]);
        assert_eq!(e5_reference(&input(&d, &pts, &flat, None, Some(&tiles))), E5Reference::Mnt { z: 100.0 });
        // Le pont du dessus coupé en deux morceaux (bord de tuile) qui se prolongent : toujours exclu.
        let tiles = tiles_m(
            mid,
            vec![
                (StructureTag::Bridge, "primary", vec![(-60.0, 0.0), (25.0, 0.0), (6.0, 25.0)]),
                (StructureTag::Bridge, "primary", vec![(6.0, 25.0), (6.0, 60.0)]),
            ],
        );
        assert_eq!(e5_reference(&input(&d, &pts, &flat, None, Some(&tiles))), E5Reference::Mnt { z: 100.0 });
        // Un autre pont (attributs différents) le long de l'approche : compté comme ouvrage.
        let tiles = tiles_m(
            mid,
            vec![
                (StructureTag::Bridge, "primary", vec![(-60.0, 0.0), (25.0, 0.0), (6.0, 25.0)]),
                (StructureTag::Bridge, "cycleway", vec![(6.0, 25.0), (6.0, 60.0)]),
            ],
        );
        assert_eq!(e5_reference(&input(&d, &pts, &flat, None, Some(&tiles))), approche_sur_ouvrage(100.0));
    }

    /// f38b745 : plage `O`, approches mesurées depuis les bouts de la plage, comme la bosse.
    #[test]
    fn the_approaches_on_a_structure_of_an_o_range_start_from_its_ends() {
        let mid = LatLon::new(47.2, -1.5);
        let (d, pts) = line(mid, false);
        let flat = dtm(&d, 0.0);
        // Pont emprunté de 25 à 35 m au nord du milieu : sous la plage `O` de ± 40 m.
        let tiles = tiles_m(mid, vec![(StructureTag::Bridge, "cycleway", vec![(3.0, 25.0), (3.0, 35.0)])]);
        let x = E5Input { ends: [32, 48], ..input(&d, &pts, &flat, None, Some(&tiles)) };
        assert_eq!(e5_reference(&x), E5Reference::Mnt { z: 100.0 });
        // Depuis le milieu, il serait sur l'approche.
        assert_eq!(e5_reference(&input(&d, &pts, &flat, None, Some(&tiles))), approche_sur_ouvrage(100.0));
        // De 65 à 80 m au nord du milieu : sur l'approche depuis le bout de la plage.
        let tiles = tiles_m(mid, vec![(StructureTag::Bridge, "cycleway", vec![(3.0, 65.0), (3.0, 80.0)])]);
        let x = E5Input { ends: [32, 48], ..input(&d, &pts, &flat, None, Some(&tiles)) };
        assert_eq!(e5_reference(&x), approche_sur_ouvrage(100.0));
    }

    /// La référence au baro n'est pas concernée : l'approche sur un ouvrage ne vise que le MNT.
    #[test]
    fn an_approach_on_a_structure_leaves_the_baro_reference() {
        let (d, pts, raw) = long_line();
        let mid = pts[400];
        let baro = vec![103.0; d.len()];
        let tiles = tiles_m(mid, vec![(StructureTag::Bridge, "cycleway", vec![(3.0, 25.0), (3.0, 60.0)])]);
        let x = E5Input {
            d: &d,
            pts: &pts,
            raw: &raw,
            ign: None,
            baro: Some(&baro),
            baro_points: &d,
            tiles: Some(&tiles),
            at: mid,
            im: 400,
            ends: [400, 400],
            baro_noise_m: None,
        };
        assert!(matches!(e5_reference(&x), E5Reference::Baro { .. }), "{:?}", e5_reference(&x));
        assert_eq!(e5_reference(&E5Input { baro: None, ..x }), approche_sur_ouvrage(100.0));
    }

    /// e9430ac : plage `O` de 80 m, approches de la bosse depuis ses bouts ; mesurées depuis le
    /// milieu, elles tomberaient sous le tablier lu par le MNT.
    #[test]
    fn the_approaches_of_an_o_range_start_from_its_ends() {
        let mid = LatLon::new(47.2, -1.5);
        let (d, pts) = line(mid, false);
        // Tablier lu par le MNT sur toute la plage (± 40 m), + 6 m.
        let z: Vec<f64> = d.iter().map(|&x| if (x - 200.0).abs() <= 40.0 { 106.0 } else { 100.0 }).collect();
        let (i0, i1) = (32, 48);
        assert!((e5_bump(&d, &z, 40, [i0, i1]).unwrap() - 6.0).abs() < 1e-9);
        let x = E5Input { ends: [i0, i1], ..input(&d, &pts, &z, None, None) };
        assert_eq!(e5_reference(&x), E5Reference::Indisponible { raison: "mnt_lit_le_tablier" });
        // Depuis le milieu, la médiane des approches lit en partie le tablier.
        assert!(e5_bump(&d, &z, 40, [40, 40]).unwrap() < 6.0);
    }
}
