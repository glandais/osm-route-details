//! Entrées privées du banc : requêtes et résultats Java, points de départ, lettres des équipes. Rien de tout cela
//! n'est dans le dépôt ; l'outil les lit sous des chemins donnés en argument.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use ord_loops::{Bearing8, RideType};
use serde_json::Value;

/// Une route rendue par le générateur Java, ce qu'il en reste dans le journal du banc Java.
#[derive(Clone, Debug)]
pub struct JavaRoute {
    pub km: f64,
    pub shared_m: f64,
    pub crossings: u64,
}

/// Une requête du banc Java et son issue.
#[derive(Clone, Debug)]
pub struct JavaRequest {
    pub i: u32,
    pub kind: String,
    /// Indice du départ dans la liste de départs de l'équipe.
    pub start: usize,
    pub km: f64,
    pub bearing: Option<Bearing8>,
    pub familiarity: f32,
    pub ride: RideType,
    pub outcome: String,
    pub ms: f64,
    pub routes: Vec<JavaRoute>,
}

impl JavaRequest {
    pub fn ok(&self) -> bool {
        self.outcome == "OK"
    }
}

fn bearing(s: &str) -> Option<Bearing8> {
    Some(match s {
        "N" => Bearing8::N,
        "NE" => Bearing8::NE,
        "E" => Bearing8::E,
        "SE" => Bearing8::SE,
        "S" => Bearing8::S,
        "SW" => Bearing8::SW,
        "W" => Bearing8::W,
        "NW" => Bearing8::NW,
        _ => return None,
    })
}

pub fn bearing_name(b: Option<Bearing8>) -> String {
    b.map_or_else(|| "-".into(), |b| format!("{b:?}"))
}

pub fn ride_name(r: RideType) -> &'static str {
    match r {
        RideType::Road => "ROAD",
        RideType::Gravel => "GRAVEL",
        RideType::Mtb => "MTB",
    }
}

/// `requests-<équipe>.jsonl` d'un banc Java.
pub fn read_java_requests(path: &Path) -> Result<Vec<JavaRequest>> {
    let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
    let mut out = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(line).context("ligne de requête illisible")?;
        let f = |k: &str| v.get(k).ok_or_else(|| anyhow!("champ `{k}` absent"));
        let ride = match f("type")?.as_str().unwrap_or("") {
            "GRAVEL" => RideType::Gravel,
            "MTB" => RideType::Mtb,
            _ => RideType::Road,
        };
        let routes = v
            .get("routes")
            .and_then(Value::as_array)
            .map(|rs| {
                rs.iter()
                    .map(|r| JavaRoute {
                        km: r.get("km").and_then(Value::as_f64).unwrap_or(0.0),
                        shared_m: r.get("sharedM").and_then(Value::as_f64).unwrap_or(0.0),
                        crossings: r.get("crossings").and_then(Value::as_u64).unwrap_or(0),
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(JavaRequest {
            i: f("i")?.as_u64().ok_or_else(|| anyhow!("`i` illisible"))? as u32,
            kind: f("kind")?.as_str().unwrap_or("").to_string(),
            start: f("start")?.as_u64().ok_or_else(|| anyhow!("`start` illisible"))? as usize,
            km: f("km")?.as_f64().ok_or_else(|| anyhow!("`km` illisible"))?,
            bearing: f("bearing")?.as_str().and_then(bearing),
            familiarity: f("familiarityAsked")?.as_f64().unwrap_or(0.5) as f32,
            ride,
            outcome: f("outcome")?.as_str().unwrap_or("").to_string(),
            ms: v.get("ms").and_then(Value::as_f64).unwrap_or(0.0),
            routes,
        });
    }
    Ok(out)
}

/// Départs reconstruits de l'export : `{ id d'équipe : [[lat, lon], ...] }`.
pub fn read_starts(path: &Path) -> Result<BTreeMap<String, Vec<(f64, f64)>>> {
    let v: Value =
        serde_json::from_str(&std::fs::read_to_string(path).with_context(|| path.display().to_string())?)?;
    let obj = v.as_object().ok_or_else(|| anyhow!("départs : un objet est attendu"))?;
    obj.iter()
        .map(|(team, pts)| {
            let pts = pts
                .as_array()
                .ok_or_else(|| anyhow!("départs d'une équipe : une liste est attendue"))?
                .iter()
                .map(|p| {
                    let a = p.as_array().ok_or_else(|| anyhow!("point illisible"))?;
                    Ok((
                        a.first().and_then(Value::as_f64).ok_or_else(|| anyhow!("lat"))?,
                        a.get(1).and_then(Value::as_f64).ok_or_else(|| anyhow!("lon"))?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok((team.clone(), pts))
        })
        .collect()
}

/// `{ id d'équipe : lettre }`, fichier local : les ids ne sont jamais écrits dans le dépôt.
pub fn read_letters(path: &Path) -> Result<BTreeMap<String, String>> {
    let v: Value =
        serde_json::from_str(&std::fs::read_to_string(path).with_context(|| path.display().to_string())?)?;
    let obj = v.as_object().ok_or_else(|| anyhow!("lettres : un objet est attendu"))?;
    Ok(obj.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
}

/// Distance équirectangulaire entre deux points `(lat, lon)`, comme la reconstruction des départs du banc Java.
pub fn crow_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    const R: f64 = 6_371_008.8;
    let dy = (b.0 - a.0).to_radians() * R;
    let dx = (b.1 - a.1).to_radians() * R * ((a.0 + b.0) / 2.0).to_radians().cos();
    dx.hypot(dy)
}

/// Arrivée d'un aller simple : le premier autre départ à 20 à 60 km à vol d'oiseau (`RealDataGeneratorBench`).
pub fn oneway_end(starts: &[(f64, f64)], from: usize) -> Option<usize> {
    (0..starts.len()).find(|&j| {
        let d = crow_m(starts[from], starts[j]);
        j != from && (20_000.0..=60_000.0).contains(&d)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oneway_end_is_the_first_start_between_20_and_60_km() {
        let pts = [(47.0, -1.0), (47.05, -1.0), (47.3, -1.0), (47.5, -1.0)];
        // 5 km, puis 33 km, puis 55 km à vol d'oiseau du premier
        assert_eq!(oneway_end(&pts, 0), Some(2));
        assert_eq!(oneway_end(&pts, 3), Some(0));
        assert_eq!(oneway_end(&pts[..2], 0), None);
    }

    #[test]
    fn reads_a_java_request_line() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("r.jsonl");
        std::fs::write(
            &p,
            r#"{"i":3,"kind":"loop-gravel","start":2,"km":50.0,"bearing":null,"familiarityAsked":0.5,"type":"GRAVEL","outcome":"GENERATOR_NO_ROUTE:NO_WAY_BACK","ms":12.5}
{"i":4,"kind":"loop","start":0,"km":30.0,"bearing":"NE","familiarityAsked":1.0,"type":"ROAD","outcome":"OK","ms":100.0,"routes":[{"km":30.4,"sharedM":10.0,"crossings":1}]}
"#,
        )
        .unwrap();
        let r = read_java_requests(&p).unwrap();
        assert_eq!(r.len(), 2);
        assert!(!r[0].ok() && r[0].bearing.is_none() && r[0].ride == RideType::Gravel);
        assert!(r[1].ok() && r[1].bearing == Some(Bearing8::NE) && r[1].routes[0].crossings == 1);
    }
}
