//! Métriques d'ouvrage du banc (README §3 : E3, E4, E5, S1, S2, S3), incrément 4.
//!
//! Sources, toutes de la série de calibration :
//! - traces perso : passages des ouvrages du corpus (`data/ouvrages-geom.json`, extension perso
//!   hors git) franchis à moins de 40 m de chacun de leurs points de référence ; E3 contre le baro
//!   corrigé localement (décalage = médiane baro − service au sol, entre 1 et 3 km du milieu), E4
//!   contre la droite entre les MNT bruts (service `structure_correction: false`) juste hors des
//!   portails, E5 au croisement d'un passage dessous, S1 sur l'emprise, S3 au milieu de
//!   `stn-acces-nord` ;
//! - parcours `test1..4` de vcyclist, envoyés point par point sur les stations de la vérité : E3
//!   contre `B` (médiane MNS, cohérence seulement) sur les ponts « tablier », E4 contre `N`, S1 sur
//!   les ouvrages OSM de `structures.csv` ;
//! - témoins vcyclist : E4 et S1 sur les tunnels (`N`), S1 sur les ponts (`D`), S2 sur les
//!   négatifs (`n*`, `tn*`, `hn*` : longueur typée pont ou tunnel hors des plages `X`/`x`), E5 sur
//!   les stations `O` (ouvrage au-dessus) des négatifs. `p06-normandie` est exclu : il franchit un
//!   ouvrage de la série tenue à l'écart.
//!
//! Aucune coordonnée, aucun nom ni longueur d'ouvrage perso dans les sorties : identifiants
//! `nv-NN` seulement.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Context;
use ord_core::rules::Rules;
use ord_domain::geo::haversine_m;
use ord_domain::request::GainPreset;
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
            out.push(Ouvrage {
                id: id.to_owned(),
                kind: kind_of(o["type"].as_str().unwrap_or("bridge")),
                length_m: o["length_m"].as_f64().unwrap_or(0.0),
                pts: [a, m, b],
                perso,
            });
        }
    }
    Ok(out)
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
}

fn nearest(p: &CaseProfile<'_>, target: LatLon, lo: f64, hi: f64) -> Option<(usize, f64)> {
    (0..p.d.len())
        .filter(|&i| p.d[i] >= lo && p.d[i] <= hi)
        .map(|i| (i, haversine_m(p.pts[i], target)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

fn r2(x: f64) -> Value {
    if x.is_finite() {
        json!((x * 100.0).round() / 100.0 + 0.0)
    } else {
        Value::Null
    }
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

/// Mesures d'ouvrage sur les passages d'un cas perso.
pub fn perso_items(case: &str, p: &CaseProfile<'_>, ouvrages: &[Ouvrage], tol: f64) -> Vec<Value> {
    let mut out = Vec::new();
    let n = p.d.len();
    for o in ouvrages {
        // Stations près du milieu, regroupées en passages (écart > 1 km).
        let near_mid: Vec<usize> = (0..n).filter(|&i| haversine_m(p.pts[i], o.pts[1]) <= tol).collect();
        let mut passages: Vec<Vec<usize>> = Vec::new();
        for i in near_mid {
            match passages.last_mut() {
                Some(g) if p.d[i] - p.d[*g.last().unwrap()] <= 1_000.0 => g.push(i),
                _ => passages.push(vec![i]),
            }
        }
        for g in passages {
            let Some(&im) = g.iter().min_by(|&&a, &&b| {
                haversine_m(p.pts[a], o.pts[1]).total_cmp(&haversine_m(p.pts[b], o.pts[1]))
            }) else {
                continue;
            };
            let dm = p.d[im];
            let span = o.length_m + 200.0;
            let (Some((ia, da)), Some((ib, db))) =
                (nearest(p, o.pts[0], dm - span, dm + span), nearest(p, o.pts[2], dm - span, dm + span))
            else {
                continue;
            };
            if da > tol || db > tol {
                continue;
            }
            let (i0, i1) = (ia.min(ib), ia.max(ib));
            let mut item = serde_json::Map::new();
            item.insert("ouvrage".into(), json!(o.id));
            item.insert("cas".into(), json!(case));
            item.insert("d_milieu_m".into(), r2(dm));
            if !o.perso {
                item.insert("longueur_m".into(), json!(o.length_m));
            }
            let len_ok = o.length_m;
            // S1 : part de l'emprise typée pont ou tunnel.
            // Ouvrage perso : longueurs sous des clés `_`, retirées avant le rapport (vie privée).
            let pre = if o.perso { "_" } else { "" };
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
                    // Baro corrigé localement : décalage au sol entre 1 et 3 km du milieu.
                    let ring: Vec<f64> = (0..n)
                        .filter(|&i| {
                            let x = (p.d[i] - dm).abs();
                            (1_000.0..=3_000.0).contains(&x) && !p.on_structure[i]
                        })
                        .map(|i| p.baro[i] - p.svc[i])
                        .filter(|x| x.is_finite())
                        .collect();
                    let offset = median(&ring);
                    let err = |i: usize| p.svc[i] - (p.baro[i] - offset);
                    let mid = err(im);
                    // Stations d'extrémité exclues du max : la station la plus proche d'un point de
                    // référence peut tomber jusqu'à un demi-pas (2,5 m) au-delà de la culée.
                    let inner = if i1 > i0 + 2 { i0 + 1..=i1 - 1 } else { i0..=i1 };
                    let worst = inner
                        .filter(|&i| err(i).is_finite())
                        .max_by(|&a, &b| err(a).abs().total_cmp(&err(b).abs()));
                    let max = worst.map_or(f64::NAN, |i| err(i).abs());
                    item.insert("ecart_milieu_m".into(), r2(mid));
                    item.insert("ecart_max_d_m".into(), worst.map_or(Value::Null, |i| r2(p.d[i])));
                    item.insert("emprise_d_m".into(), json!([r2(p.d[i0]), r2(p.d[i1])]));
                    item.insert("ecart_max_m".into(), r2(max));
                    item.insert("decalage_local_m".into(), r2(offset));
                    if o.kind == Kind::Untagged {
                        item.insert("metrique".into(), json!("S3"));
                        item.insert("service_milieu_m".into(), r2(p.svc[im]));
                        item.insert("baro_corrige_milieu_m".into(), r2(p.baro[im] - offset));
                    } else {
                        let france = p.ign[im];
                        item.insert("metrique".into(), json!(if france { "E3" } else { "E3_hors_france" }));
                        item.insert("reference".into(), json!("baro_corrige_local"));
                        if mid.is_finite() && max.is_finite() {
                            item.insert("pass".into(), json!(e3_pass(len_ok, mid, max)));
                        }
                    }
                }
                Kind::Tunnel => {
                    let (a, b) = (i0.saturating_sub(2), (i1 + 2).min(n - 1));
                    let line =
                        |i: usize| p.raw[a] + (p.raw[b] - p.raw[a]) * (p.d[i] - p.d[a]) / (p.d[b] - p.d[a]);
                    let max = (i0..=i1).map(|i| (p.svc[i] - line(i)).abs()).fold(f64::NAN, f64::max);
                    let lz: Vec<f64> = (a..=b).map(line).collect();
                    let fict = dplus_slice(&p.d[a..=b], &p.svc[a..=b]) - dplus_slice(&p.d[a..=b], &lz);
                    item.insert("metrique".into(), json!("E4"));
                    item.insert("reference".into(), json!("droite_mnt_portails"));
                    item.insert("ecart_max_m".into(), r2(max));
                    item.insert("dplus_fictif_m".into(), r2(fict));
                    item.insert("pass".into(), json!(max <= 5.0 && fict <= 5.0));
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

/// Réponse du service point par point (mode `input`, morceaux de 15 000 points) : altitude et
/// station sur pont ou tunnel.
pub async fn query_points(
    service: &Service,
    pts: &[Pt],
    correction: bool,
) -> anyhow::Result<(Vec<f64>, Vec<bool>)> {
    const CHUNK: usize = 15_000;
    let mut ele = vec![f64::NAN; pts.len()];
    let mut on = vec![false; pts.len()];
    for (c, chunk) in pts.chunks(CHUNK).enumerate() {
        let r = service
            .route_details(
                chunk,
                json!({"sampling": {"mode": "input"}, "sections": ["elevation", "typology"], "structure_correction": correction}),
            )
            .await?;
        let segs: Vec<(f64, f64, bool)> = r["segments"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| {
                let st = s["structure"].as_str().unwrap_or("");
                (
                    s["from_m"].as_f64().unwrap_or(0.0),
                    s["to_m"].as_f64().unwrap_or(0.0),
                    st == "bridge" || st == "tunnel",
                )
            })
            .collect();
        for p in r["points"].as_array().context("points")? {
            let i = c * CHUNK + p["index"].as_u64().context("index")? as usize;
            ele[i] = p["ele"].as_f64().unwrap_or(f64::NAN);
            let d = p["d_m"].as_f64().unwrap_or(0.0);
            on[i] = segs.iter().find(|s| d >= s.0 && d < s.1).or(segs.last()).is_some_and(|s| s.2);
        }
    }
    Ok((ele, on))
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

/// Plage d'ouvrage de `structures.csv` : `(nature, i0, i1, drapeau, nom)`.
type VcSpan = (String, usize, usize, String, String);

/// Plages d'ouvrage d'un parcours `testN` (`structures.csv`).
fn vc_spans(bench: &Path, route: &str) -> anyhow::Result<Vec<VcSpan>> {
    let text = std::fs::read_to_string(bench.join("data/vcyclist/analysis/truth/structures.csv"))?;
    let mut out = Vec::new();
    for l in text.lines().skip(1) {
        let f: Vec<&str> = l.split(',').collect();
        if f.len() > 3 && f[0] == route {
            out.push((
                f[1].to_owned(),
                f[2].parse()?,
                f[3].parse()?,
                f.last().unwrap_or(&"").to_string(),
                f.get(13).unwrap_or(&"").to_string(),
            ));
        }
    }
    Ok(out)
}

/// E3 (cohérence `B`), E4 (`N`), S1 sur un parcours `testN` de vcyclist.
pub async fn vc_route_items(service: &Service, bench: &Path, route: &str) -> anyhow::Result<Vec<Value>> {
    let s = read_stations(&bench.join(format!("data/vcyclist/analysis/truth/{route}.csv")), 5, Some(3))?;
    let (svc, on) = query_points(service, &s.pts, true).await?;
    let d = cum(&s.pts);
    let mut out = Vec::new();
    for (k, (kind, i0, i1, flag, _)) in vc_spans(bench, route)?.into_iter().enumerate() {
        if i1 >= s.pts.len() || i1 <= i0 {
            continue;
        }
        if kind == "balcony" {
            continue;
        }
        let id = format!("vc-{route}#{k}");
        let typed = (i0..=i1).filter(|&i| on[i]).count() as f64 / (i1 - i0 + 1) as f64 * (d[i1] - d[i0]);
        let mut item = json!({"ouvrage": id, "cas": format!("vc-{route}"), "nature": kind});
        // S1 sur les emprises OSM (`missed`, tunnels) ; les plages `bridge` sont celles du
        // détecteur de vcyclist (accès nord non tagué compris), pas des emprises OSM.
        if kind != "bridge" {
            item["S1.emprise_m"] = r2(d[i1] - d[i0]);
            item["S1.type_m"] = r2(typed);
        }
        if kind == "bridge" && flag.trim() == "tablier" {
            // Partie taguée seulement (stations `B` typées pont) : le reste de la plage du
            // détecteur est un ouvrage non tagué (S3).
            let b: Vec<usize> = (i0..=i1).filter(|&i| s.kind[i] == "B" && on[i]).collect();
            if !b.is_empty() {
                let im = b[b.len() / 2];
                let mid = svc[im] - s.z[im];
                let max = b.iter().map(|&i| (svc[i] - s.z[i]).abs()).fold(f64::NAN, f64::max);
                let len = d[i1] - d[i0];
                item["metrique"] = json!("E3");
                item["reference"] = json!("mns_mediane_25m_coherence");
                item["longueur_m"] = r2(len);
                item["ecart_milieu_m"] = r2(mid);
                item["ecart_max_m"] = r2(max);
                item["pass"] = json!(e3_pass(len, mid, max));
            }
        } else if kind == "tunnel" {
            let nn: Vec<usize> = (i0..=i1).filter(|&i| s.kind[i] == "N").collect();
            if !nn.is_empty() {
                let max = nn.iter().map(|&i| (svc[i] - s.z[i]).abs()).fold(f64::NAN, f64::max);
                let (a, b) = (nn[0], nn[nn.len() - 1]);
                let fict = dplus_slice(&d[a..=b], &svc[a..=b]) - dplus_slice(&d[a..=b], &s.z[a..=b]);
                item["metrique"] = json!("E4");
                item["reference"] = json!("vcyclist_N");
                item["ecart_max_m"] = r2(max);
                item["dplus_fictif_m"] = r2(fict);
                item["pass"] = json!(max <= 5.0 && fict <= 5.0);
            }
        }
        out.push(item);
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

/// E4, E5, S1, S2 sur un témoin vcyclist.
pub async fn control_items(
    service: &Service,
    bench: &Path,
    dir: &str,
    id: &str,
    role: &str,
) -> anyhow::Result<Vec<Value>> {
    let s = control_stations(bench, dir, id)?;
    let (svc, on) = query_points(service, &s.pts, true).await?;
    let (raw, _) = query_points(service, &s.pts, false).await?;
    let d = cum(&s.pts);
    let n = s.pts.len();
    let mut out = Vec::new();
    let negative = role.contains("negative");
    // Plages de même nature.
    let mut spans: Vec<(String, usize, usize)> = Vec::new();
    for i in 0..n {
        match spans.last_mut() {
            Some(sp) if sp.0 == s.kind[i] => sp.2 = i,
            _ => spans.push((s.kind[i].clone(), i, i)),
        }
    }
    for (k, (kind, i0, i1)) in spans.iter().cloned().enumerate() {
        let typed = (i0..=i1).filter(|&i| on[i]).count() as f64 / (i1 - i0 + 1) as f64 * (d[i1] - d[i0]);
        if kind == "N" && !negative {
            let (a, b) = (i0.saturating_sub(1), (i1 + 1).min(n - 1));
            let line = |i: usize| raw[a] + (raw[b] - raw[a]) * (d[i] - d[a]) / (d[b] - d[a]).max(1e-9);
            let max = (i0..=i1).map(|i| (svc[i] - line(i)).abs()).fold(f64::NAN, f64::max);
            let lz: Vec<f64> = (a..=b).map(line).collect();
            let fict = dplus_slice(&d[a..=b], &svc[a..=b]) - dplus_slice(&d[a..=b], &lz);
            out.push(json!({"ouvrage": format!("{id}#{k}"), "cas": id, "metrique": "E4",
                "reference": "droite_mnt_portails", "ecart_max_m": r2(max), "dplus_fictif_m": r2(fict),
                "pass": max <= 5.0 && fict <= 5.0, "S1.emprise_m": r2(d[i1] - d[i0]), "S1.type_m": r2(typed)}));
        } else if kind == "D" && !negative {
            out.push(json!({"ouvrage": format!("{id}#{k}"), "cas": id, "nature": "bridge",
                "S1.emprise_m": r2(d[i1] - d[i0]), "S1.type_m": r2(typed)}));
        } else if kind == "O" && negative {
            let e = (i0..=i1).map(|i| (svc[i] - raw[i]).abs()).fold(f64::NAN, f64::max);
            out.push(json!({"ouvrage": format!("{id}#{k}"), "cas": id, "metrique": "E5",
                "ecart_croisement_m": r2(e), "pass": e <= 1.0}));
        }
    }
    if negative {
        // S2 : longueur typée pont ou tunnel hors des plages `X`/`x` (ouvrage propre exclu).
        let mut fp_m = 0.0;
        let mut fictifs = 0usize;
        let mut run: Option<(usize, usize)> = None;
        let flush = |r: Option<(usize, usize)>, fictifs: &mut usize| {
            if let Some((a, b)) = r {
                let bad = (a..=b).filter(|&i| !matches!(s.kind[i].as_str(), "X" | "x")).count();
                if bad * 2 > b - a + 1 {
                    *fictifs += 1;
                }
            }
        };
        for i in 0..n {
            if on[i] {
                if !matches!(s.kind[i].as_str(), "X" | "x") && i > 0 {
                    fp_m += d[i] - d[i - 1];
                }
                run = Some(run.map_or((i, i), |r| (r.0, i)));
            } else {
                flush(run.take(), &mut fictifs);
            }
        }
        flush(run, &mut fictifs);
        out.push(json!({"cas": id, "metrique": "S2", "longueur_m": r2(d[n - 1]),
            "faux_positif_m": r2(fp_m), "ouvrages_fictifs": fictifs}));
    }
    Ok(out)
}

/// Verdicts E3, E4, E5, S1, S2 et S3 (rapporté) sur les items mesurés.
pub fn judge(items: &[Value]) -> (Value, Value) {
    let by = |m: &str| -> Vec<&Value> { items.iter().filter(|i| i["metrique"] == m).collect() };
    let rate = |v: &[&Value]| -> (usize, usize) {
        let judged: Vec<&&Value> = v.iter().filter(|i| i["pass"].is_boolean()).collect();
        (judged.iter().filter(|i| i["pass"] == true).count(), judged.len())
    };
    // E3 par pont : un pont passe si tous ses passages passent.
    let mut ponts: BTreeMap<String, bool> = BTreeMap::new();
    for i in by("E3").iter().filter(|i| i["pass"].is_boolean()) {
        let k = i["ouvrage"].as_str().unwrap_or("").to_owned();
        let ok = i["pass"] == true;
        ponts.entry(k).and_modify(|v| *v &= ok).or_insert(ok);
    }
    let e3_pass = ponts.values().filter(|v| **v).count();
    let e3_n = ponts.len();
    let e3_fail: Vec<&String> = ponts.iter().filter(|(_, v)| !**v).map(|(k, _)| k).collect();
    let (e4_ok, e4_n) = rate(&by("E4"));
    let e4_fail: Vec<&Value> =
        by("E4").into_iter().filter(|i| i["pass"] == false).map(|i| &i["ouvrage"]).collect();
    let (e5_ok, e5_n) = rate(&by("E5"));
    let e5_fail: Vec<&Value> =
        by("E5").into_iter().filter(|i| i["pass"] == false).map(|i| &i["ouvrage"]).collect();
    let sum = |k: &str| -> f64 {
        items.iter().filter_map(|i| i[k].as_f64().or_else(|| i[format!("_{k}")].as_f64())).sum()
    };
    let (s1_ref, s1_typed) = (sum("S1.emprise_m"), sum("S1.type_m"));
    let s1 = s1_typed / s1_ref.max(1e-9);
    let s2: Vec<&Value> = by("S2");
    let s2_len: f64 = s2.iter().filter_map(|i| i["longueur_m"].as_f64()).sum();
    let s2_fp: f64 = s2.iter().filter_map(|i| i["faux_positif_m"].as_f64()).sum();
    let s2_fict: u64 = s2.iter().filter_map(|i| i["ouvrages_fictifs"].as_u64()).sum();
    let s2_share = s2_fp / s2_len.max(1e-9);
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
        "E3": {"ponts": e3_n, "ponts_ok": e3_pass, "part": r4(e3_pass as f64 / e3_n.max(1) as f64), "seuil": 0.90},
        "E3_hors_france": by("E3_hors_france"),
        "E4": {"tunnels": e4_n, "tunnels_ok": e4_ok, "part": r4(e4_ok as f64 / e4_n.max(1) as f64), "seuil": 0.857},
        "E5": {"croisements": e5_n, "ok": e5_ok},
        "S1": {"emprise_m": r2(s1_ref), "type_m": r2(s1_typed), "rappel": r4(s1), "seuil": 0.95},
        "S2": {"longueur_m": r2(s2_len), "faux_positif_m": r2(s2_fp), "part": r4(s2_share), "ouvrages_fictifs": s2_fict},
        "S3": by("S3"),
    });
    let verdicts = json!({
        "E3": verdict(e3_pass as f64 >= 0.90 * e3_n as f64, e3_n),
        "E3_echecs": e3_fail,
        "E3_hors_france": if by("E3_hors_france").is_empty() { "skipped" } else { "reported" },
        "E4": verdict(e4_ok as f64 >= 0.857 * e4_n as f64, e4_n),
        "E4_echecs": e4_fail,
        "E5": verdict(e5_ok == e5_n, e5_n),
        "E5_echecs": e5_fail,
        "S1": verdict(s1 >= 0.95, usize::from(s1_ref > 0.0)),
        "S2": verdict(s2_fict == 0 && s2_share <= 0.005, s2.len()),
        "S3": if by("S3").is_empty() { "skipped" } else { "reported" },
    });
    (agg, verdicts)
}

fn r4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

/// Items sans leurs clés privées (`_…`), pour le rapport.
pub fn public(items: &[Value]) -> Vec<Value> {
    items
        .iter()
        .map(|i| match i {
            Value::Object(m) => Value::Object(
                m.iter().filter(|(k, _)| !k.starts_with('_')).map(|(k, v)| (k.clone(), v.clone())).collect(),
            ),
            v => v.clone(),
        })
        .collect()
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
    fn judge_counts_bridges_once_and_s2() {
        let items = vec![
            json!({"ouvrage": "a", "metrique": "E3", "pass": true}),
            json!({"ouvrage": "a", "metrique": "E3", "pass": false}),
            json!({"ouvrage": "b", "metrique": "E3", "pass": true}),
            json!({"metrique": "S2", "longueur_m": 1000.0, "faux_positif_m": 0.0, "ouvrages_fictifs": 0}),
            json!({"S1.emprise_m": 100.0, "S1.type_m": 99.0}),
        ];
        let (agg, v) = judge(&items);
        assert_eq!(agg["E3"]["ponts"], 2);
        assert_eq!(v["E3"], "fail", "1/2 < 90 %");
        assert_eq!(v["S2"], "pass");
        assert_eq!(v["S1"], "pass");
        assert_eq!(v["E4"], "skipped");
    }
}
