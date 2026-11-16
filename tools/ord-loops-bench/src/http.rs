//! Mode HTTP du banc (lot 3a) : la même requête, envoyée à `POST /v1/generator/routes` d'un service qui tourne, avec
//! la clé d'environnement et la référence d'équipe que le backend enverrait. Les parcours rendus ont déjà passé le
//! contrôleur indépendant **dans le service** ; le banc n'en relit que la géométrie (empreinte) et les longueurs.
//!
//! **Privé** : le fichier des clés (`{ id d'équipe : { "key": …, "team": … } }`) est un fichier local, hors dépôt ; ni
//! clé, ni référence d'équipe, ni coordonnée n'est écrite au rapport.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context as _, Result};
use ord_loops::{Bearing8, LatLon, RideType};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::record::{Rec, RouteRec};

/// Clé d'environnement et référence d'équipe d'une équipe.
#[derive(Clone, Debug, Deserialize)]
pub struct TeamKey {
    pub key: String,
    pub team: String,
}

pub fn read_keys(path: &Path) -> Result<BTreeMap<String, TeamKey>> {
    let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
    serde_json::from_str(&text).context("fichier des clés illisible")
}

/// Empreinte d'une géométrie : xxh3 des entiers en millionièmes de degré (ceux d'une polyligne de précision 6).
pub fn geom_xxh3(points: impl IntoIterator<Item = (i64, i64)>) -> String {
    let mut bytes = Vec::new();
    for (la, lo) in points {
        bytes.extend_from_slice(&la.to_le_bytes());
        bytes.extend_from_slice(&lo.to_le_bytes());
    }
    format!("{:016x}", xxhash_rust::xxh3::xxh3_64(&bytes))
}

/// Points d'un parcours du moteur, arrondis comme la polyligne du service.
pub fn rounded(points: &[LatLon]) -> Vec<(i64, i64)> {
    points.iter().map(|p| ((p.lat * 1e6).round() as i64, (p.lon * 1e6).round() as i64)).collect()
}

/// Polyligne de précision 6 en entiers (millionièmes de degré).
fn decode_polyline6(s: &str) -> Option<Vec<(i64, i64)>> {
    let b = s.as_bytes();
    let (mut i, mut lat, mut lon) = (0usize, 0i64, 0i64);
    let mut out = Vec::new();
    let next = |i: &mut usize| -> Option<i64> {
        let (mut result, mut shift) = (0i64, 0u32);
        loop {
            let c = i64::from(*b.get(*i)?) - 63;
            *i += 1;
            if !(0..64).contains(&c) || shift > 60 {
                return None;
            }
            result |= (c & 0x1f) << shift;
            shift += 5;
            if c < 0x20 {
                break;
            }
        }
        Some(if result & 1 != 0 { !(result >> 1) } else { result >> 1 })
    };
    while i < b.len() {
        lat += next(&mut i)?;
        lon += next(&mut i)?;
        out.push((lat, lon));
    }
    Some(out)
}

fn bearing_v1(b: Bearing8) -> &'static str {
    match b {
        Bearing8::N => "N",
        Bearing8::NE => "NE",
        Bearing8::E => "E",
        Bearing8::SE => "SE",
        Bearing8::S => "S",
        Bearing8::SW => "SW",
        Bearing8::W => "W",
        Bearing8::NW => "NW",
    }
}

fn ride_v1(r: RideType) -> &'static str {
    match r {
        RideType::Road => "road",
        RideType::Gravel => "gravel",
        RideType::Mtb => "mtb",
    }
}

/// Une demande telle que le banc la rejoue.
#[derive(Debug)]
pub struct Ask<'a> {
    pub start: LatLon,
    pub end: Option<LatLon>,
    pub bearing: Option<Bearing8>,
    pub distance_m: f64,
    pub ride: RideType,
    pub cursor: f64,
    pub seed: u64,
    pub key: Option<&'a TeamKey>,
}

/// Réponse lue : statut, corps, durée murale côté client.
#[derive(Debug)]
pub struct Answer {
    pub status: u16,
    pub body: Value,
    pub ms: f64,
}

/// Client HTTP : `reqwest` asynchrone sur un runtime d'un fil (pas de caisse en plus pour le mode bloquant) ;
/// connexion gardée ouverte d'une requête à l'autre, comme le client du backend.
pub struct Client {
    rt: tokio::runtime::Runtime,
    http: reqwest::Client,
    url: String,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").finish_non_exhaustive()
    }
}

impl Client {
    pub fn new(base: &str) -> Result<Self> {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().context("runtime")?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .pool_max_idle_per_host(4)
            .build()
            .context("client HTTP")?;
        Ok(Self { rt, http, url: format!("{}/v1/generator/routes", base.trim_end_matches('/')) })
    }

    pub fn ask(&self, a: &Ask<'_>) -> Result<Answer> {
        let body = json!({
            "start": {"lat": a.start.lat, "lon": a.start.lon},
            "end": a.end.map(|e| json!({"lat": e.lat, "lon": e.lon})),
            "bearing": a.bearing.map(bearing_v1),
            "distance_m": a.distance_m,
            "ride_type": ride_v1(a.ride),
            "cursor": a.cursor,
            "seed": a.seed,
        });
        let mut req = self.http.post(&self.url).header("content-type", "application/json");
        if let Some(k) = a.key {
            req = req.header("ord-namespace-key", &k.key).header("ord-team", &k.team);
        }
        let body = body.to_string();
        let t = Instant::now();
        let (status, text) = self
            .rt
            .block_on(async move {
                let resp = req.body(body).send().await?;
                let status = resp.status().as_u16();
                Ok::<_, reqwest::Error>((status, resp.text().await?))
            })
            .map_err(|e| anyhow!("HTTP : {e}"))?;
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let body = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok(Answer { status, body, ms })
    }

    /// Un tour froid (son résultat est celui du rapport), puis `rounds` tours chauds dont on garde la médiane.
    pub fn run(&self, a: &Ask<'_>, rounds: usize) -> Result<(Answer, f64)> {
        let first = self.ask(a)?;
        let mut warm: Vec<f64> = Vec::new();
        for _ in 0..rounds {
            warm.push(self.ask(a)?.ms);
        }
        warm.sort_by(f64::total_cmp);
        let ms = if warm.is_empty() { first.ms } else { warm[warm.len() / 2] };
        Ok((first, ms))
    }
}

/// Code d'issue du banc pour une raison `generation_failed` du service.
fn failure_from_reason(reason: &str) -> String {
    match reason {
        "start_too_far" => "START_TOO_FAR".into(),
        "end_too_far" => "END_TOO_FAR".into(),
        "distance_too_short" => "DISTANCE_TOO_SHORT".into(),
        "network_too_small" => "NETWORK_TOO_SMALL".into(),
        "end_unreachable" => "NO_ROUTE:END_UNREACHABLE".into(),
        "no_way_back" => "NO_ROUTE:NO_WAY_BACK".into(),
        "tail_too_long" => "NO_ROUTE:TAIL_TOO_LONG".into(),
        "no_candidate" => "NO_ROUTE:NO_CANDIDATE".into(),
        "budget" => "NO_ROUTE:BUDGET".into(),
        "checker_rejected" => "VIOLATION".into(),
        other => format!("GENERATION_FAILED:{other}"),
    }
}

fn num(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}

/// Remplit l'enregistrement depuis la réponse du service.
pub fn fill(rec: &mut Rec, ans: &Answer, ms: f64, distance_m: f64) {
    rec.ms = ms;
    rec.cold_ms = ans.ms;
    rec.http_status = Some(ans.status);
    let b = &ans.body;
    rec.applied = b.get("mode").and_then(Value::as_str).map(|m| {
        format!("{m} λ={}", b.get("lambda").and_then(Value::as_f64).map_or("-".into(), |l| l.to_string()))
    });
    match ans.status {
        200 => {
            let routes = b.get("routes").and_then(Value::as_array).cloned().unwrap_or_default();
            let dropped = b.get("dropped").and_then(Value::as_u64).unwrap_or(0) as u32;
            rec.dropped = Some(dropped);
            rec.n_routes = routes.len() + dropped as usize;
            rec.n_ok = routes.len();
            if dropped > 0 {
                rec.violations.insert("DROPPED_IN_SERVICE".into(), dropped);
            }
            for r in &routes {
                let len = num(r, "distance_m");
                let geom =
                    r.get("polyline").and_then(Value::as_str).and_then(decode_polyline6).map(geom_xxh3);
                rec.routes.push(RouteRec {
                    length_km: len / 1000.0,
                    over_target: len / distance_m,
                    team_share: if len > 0.0 { num(r, "team_m") / len } else { 0.0 },
                    bridged_m: num(r, "bridged_m"),
                    shared_m: num(r, "tail_m"),
                    crossings: r.get("crossings").and_then(Value::as_u64).unwrap_or(0) as u32,
                    engine_team_m: f64::NAN,
                    family: match r.get("family").and_then(Value::as_str) {
                        Some("turnaround") => "Turnaround".into(),
                        Some("polygon") => "Polygon".into(),
                        other => other.unwrap_or("").into(),
                    },
                    violations: Vec::new(),
                    internal: Vec::new(),
                    checker_shared_m: num(r, "tail_m"),
                    checker_crossings: r.get("crossings").and_then(Value::as_u64).unwrap_or(0) as u32,
                    checker_team_m: num(r, "team_m"),
                    checker_bridged_m: num(r, "bridged_m"),
                    start_zone_m: f64::NAN,
                    end_zone_m: f64::NAN,
                    corridor_share: f64::NAN,
                    geom_xxh3: geom,
                });
            }
            rec.start_snap_m = routes.first().map_or(0.0, |r| num(r, "start_snap_m"));
            rec.outcome = if rec.n_ok > 0 { "OK".into() } else { "NO_ROUTES".into() };
        }
        _ => {
            let code = b.get("code").and_then(Value::as_str).unwrap_or("");
            let details = b.get("details");
            rec.outcome = match code {
                "generation_failed" => failure_from_reason(
                    details.and_then(|d| d.get("reason")).and_then(Value::as_str).unwrap_or(""),
                ),
                "overlay_required" => "NO_TEAM_DATA".into(),
                "timeout" => "STOPPED".into(),
                _ => format!("HTTP_{}:{code}", ans.status),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polyline_integers_round_trip() {
        // « _p~iF~ps|U » : (38.5, -120.2) en précision 5, soit (3.85, -12.02) en précision 6.
        let pts = decode_polyline6("_p~iF~ps|U").unwrap();
        assert_eq!(pts, vec![(3_850_000, -12_020_000)]);
        assert!(decode_polyline6("_p~iF").is_none());
        assert_eq!(geom_xxh3(pts.clone()), geom_xxh3(rounded(&[LatLon::new(3.85, -12.02)])));
    }

    #[test]
    fn failure_codes_match_the_in_process_ones() {
        assert_eq!(failure_from_reason("no_candidate"), "NO_ROUTE:NO_CANDIDATE");
        assert_eq!(failure_from_reason("start_too_far"), "START_TOO_FAR");
    }
}
