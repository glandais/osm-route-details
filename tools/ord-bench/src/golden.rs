//! Scénarios golden (T05 §2.6) sur des ouvrages **de calibration** du corpus public
//! (`bench/data/ouvrages.yaml`) : le parcours est la suite des points de référence `A`, `milieu`,
//! `B` de l'ouvrage. Jamais de trace personnelle ; un ouvrage de la série tenue à l'écart est
//! refusé (il ferait fuiter la série).
//!
//! La réponse est produite sur le cache de banc (hors ligne), `meta.request_id` et
//! `meta.duration_ms` effacés, puis comparée au snapshot `tests/golden/<id>.json` d'`ord-bench`
//! avec une tolérance d'une unité du dernier chiffre (1 cm, 1e-4).

use std::path::PathBuf;

use anyhow::{bail, Context};
use serde_json::{json, Value};

use crate::manifest::Manifest;
use crate::service::{Pt, Service};
use crate::BenchPaths;

/// Scénarios du MVP à l'incrément 2 : GLO-30 (`adapted[]` non vide) et hors France.
pub const SCENARIOS: [&str; 2] = ["krk", "liege-belle-liegeoise"];

/// Scénarios d'ouvrage de l'incrément 4 (T05 §2.6), sur des ouvrages de calibration :
/// - `test1-depart-sur-tablier` : `test1` de vcyclist coupé à 2,5 km, au milieu du tablier du pont
///   de Saint-Nazaire (T05 §2.6), jusqu'à 5 km ;
/// - `nantes-pirmil-deux-fois` : pont de Pirmil parcouru deux fois (aller-retour) ;
/// - `nantes-pirmil-demi-tour` : demi-tour au milieu du pont de Pirmil.
pub const STRUCTURE_SCENARIOS: [&str; 3] =
    ["test1-depart-sur-tablier", "nantes-pirmil-deux-fois", "nantes-pirmil-demi-tour"];

/// Options des scénarios d'ouvrage : profil à 10 m (le tablier se lit), toutes les sections.
pub fn structure_options() -> Value {
    json!({"sampling": {"mode": "distance", "step_m": 10.0}})
}

/// Point prolongé de `m` mètres au-delà de `a`, dans la direction `b → a`.
fn beyond(a: Pt, b: Pt, m: f64) -> Pt {
    let d = ord_domain::geo::haversine_m(
        ord_domain::LatLon::new(a.lat, a.lon),
        ord_domain::LatLon::new(b.lat, b.lon),
    );
    let t = if d > 0.0 { m / d } else { 0.0 };
    Pt { lat: a.lat + (a.lat - b.lat) * t, lon: a.lon + (a.lon - b.lon) * t }
}

/// Parcours d'un scénario d'ouvrage.
pub fn structure_scenario(paths: &BenchPaths, manifest: &Manifest, id: &str) -> anyhow::Result<Scenario> {
    let point = |o: &str, k: &str| -> anyhow::Result<Pt> {
        let text =
            std::fs::read_to_string(paths.bench.join("data/ouvrages.yaml")).context("ouvrages.yaml")?;
        let root: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).context("ouvrages.yaml illisible")?;
        let list = root.get("ouvrages").and_then(|v| v.as_sequence()).context("liste `ouvrages`")?;
        let x = list
            .iter()
            .find(|x| x.get("id").and_then(|v| v.as_str()) == Some(o))
            .and_then(|x| x.get("points")?.get(k)?.as_sequence().cloned())
            .with_context(|| format!("{o} : point {k}"))?;
        Ok(Pt { lat: x[0].as_f64().context("lat")?, lon: x[1].as_f64().context("lon")? })
    };
    if id == "test1-depart-sur-tablier" {
        let pts = crate::gpx::read(&paths.bench.join("data/vcyclist/test1.gpx"))?;
        let mut out = Vec::new();
        let mut s = 0.0;
        for (i, p) in pts.iter().enumerate() {
            if i > 0 {
                let q = pts[i - 1];
                s += ord_domain::geo::haversine_m(
                    ord_domain::LatLon::new(q.lat, q.lon),
                    ord_domain::LatLon::new(p.lat, p.lon),
                );
            }
            if (2_500.0..=5_000.0).contains(&s) {
                out.push(Pt { lat: p.lat, lon: p.lon });
            }
        }
        return Ok(Scenario { id: id.to_owned(), points: out });
    }
    let ouvrage = if id.starts_with("nantes-pirmil") { "nantes-pirmil" } else { id };
    if manifest.ouvrages_tenus.contains(ouvrage) {
        bail!("{ouvrage} : ouvrage de la série tenue à l'écart, refusé en golden (T05 §2.6)");
    }
    let points = match id {
        "nantes-pirmil-deux-fois" | "nantes-pirmil-demi-tour" => {
            let (a, m, b) = (point(ouvrage, "A")?, point(ouvrage, "milieu")?, point(ouvrage, "B")?);
            let (ea, eb) = (beyond(a, m, 60.0), beyond(b, m, 60.0));
            if id.ends_with("deux-fois") {
                vec![ea, a, m, b, eb, b, m, a, ea]
            } else {
                vec![ea, a, m, a, ea]
            }
        }
        other => bail!("{other} : scénario d'ouvrage inconnu"),
    };
    Ok(Scenario { id: id.to_owned(), points })
}

/// Parcours d'un scénario (coordonnées publiques d'un ouvrage de calibration).
#[derive(Clone)]
pub struct Scenario {
    pub id: String,
    pub points: Vec<Pt>,
}

impl std::fmt::Debug for Scenario {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scenario").field("id", &self.id).field("points", &self.points.len()).finish()
    }
}

pub fn scenario(paths: &BenchPaths, manifest: &Manifest, id: &str) -> anyhow::Result<Scenario> {
    if manifest.ouvrages_tenus.contains(id) {
        bail!("{id} : ouvrage de la série tenue à l'écart, refusé en golden (T05 §2.6)");
    }
    let text = std::fs::read_to_string(paths.bench.join("data/ouvrages.yaml")).context("ouvrages.yaml")?;
    let root: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).context("ouvrages.yaml illisible")?;
    let list = root.get("ouvrages").and_then(|v| v.as_sequence()).context("liste `ouvrages`")?;
    let o = list
        .iter()
        .find(|o| o.get("id").and_then(|v| v.as_str()) == Some(id))
        .with_context(|| format!("{id} absent d'ouvrages.yaml"))?;
    let pts = o.get("points").context("points de référence")?;
    let mut points = Vec::new();
    for k in ["A", "milieu", "B"] {
        let p = pts.get(k).and_then(|v| v.as_sequence()).with_context(|| format!("{id} : point {k}"))?;
        let (Some(lat), Some(lon)) = (p.first().and_then(|v| v.as_f64()), p.get(1).and_then(|v| v.as_f64()))
        else {
            bail!("{id} : point {k} invalide");
        };
        points.push(Pt { lat, lon });
    }
    Ok(Scenario { id: id.to_owned(), points })
}

/// Réponse du service, champs volatils effacés.
pub async fn response(service: &Service, s: &Scenario) -> anyhow::Result<Value> {
    response_with(service, s, json!({"sampling": {"mode": "input"}})).await
}

/// Réponse du service avec ces options, champs volatils effacés.
pub async fn response_with(service: &Service, s: &Scenario, options: Value) -> anyhow::Result<Value> {
    let mut r = service.route_details(&s.points, options).await?;
    r["meta"]["request_id"] = Value::Null;
    r["meta"]["duration_ms"] = Value::Null;
    Ok(r)
}

/// Fichier du snapshot.
pub fn snapshot_path(id: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(format!("{id}.json"))
}

/// Égalité de deux JSON, nombres à une unité du dernier chiffre près (1e-2 + marge).
pub fn diff(path: &str, a: &Value, b: &Value, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap_or(f64::NAN), y.as_f64().unwrap_or(f64::NAN));
            if (x - y).abs() > 0.011 {
                out.push(format!("{path} : {x} ≠ {y}"));
            }
        }
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys().filter(|k| !x.contains_key(*k))) {
                diff(
                    &format!("{path}/{k}"),
                    x.get(k).unwrap_or(&Value::Null),
                    y.get(k).unwrap_or(&Value::Null),
                    out,
                );
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                diff(&format!("{path}/{i}"), p, q, out);
            }
        }
        _ if a == b => {}
        _ => out.push(format!("{path} : {a} ≠ {b}")),
    }
}
