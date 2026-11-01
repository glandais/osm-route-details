//! Tags OSM de la voie des témoins négatifs (README §4 ter.6, précisé en eb4c4cc) : ways portées par
//! les stations des fichiers de vérité (`analysis/truth/{controls,tunnels}/<id>.csv`, colonne `way`,
//! plages X et O comprises), tags lus dans la **base OSM du témoin** (`osm_base` de l'en-tête),
//! jamais dans la copie vcyclist, qui ne les porte pas.
//!
//! `fill` (en ligne, `fill-cache`) : une requête Overpass datée par témoin, réponse écrite dans le
//! cache de banc (`temoins-tags/<id>.json`). `load` (hors ligne, campagne) : lecture du cache. Seuls
//! des identifiants de ways publiques sont envoyés (témoins vcyclist), aucune donnée personnelle.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde_json::{json, Value};

/// Point d'accès Overpass (requête datée `[date:…]`, base du témoin).
const OVERPASS: &str = "https://overpass-api.de/api/interpreter";

/// Tags par way d'un témoin, à sa base OSM.
pub type WayTags = BTreeMap<u64, BTreeMap<String, String>>;

/// Base OSM et ways (dans l'ordre des stations) d'un fichier de stations de témoin.
pub fn station_ways(bench: &Path, id: &str) -> anyhow::Result<(String, Vec<u64>)> {
    for dir in ["controls", "tunnels"] {
        let p = bench.join(format!("data/vcyclist/analysis/truth/{dir}/{id}.csv"));
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        let base = text
            .lines()
            .find(|l| l.starts_with("# control="))
            .and_then(|l| l.split_whitespace().find_map(|w| w.strip_prefix("osm_base=")))
            .with_context(|| format!("{id} : osm_base absent de l'en-tête"))?
            .to_owned();
        let ways = text
            .lines()
            .filter(|l| !l.starts_with('#') && !l.starts_with("s,"))
            .map(|l| l.split(',').nth(4).unwrap_or("0").trim().parse::<u64>().unwrap_or(0))
            .collect();
        return Ok((base, ways));
    }
    anyhow::bail!("{id} : fichier de stations introuvable")
}

fn cache_file(cache: &Path, id: &str) -> PathBuf {
    cache.join("temoins-tags").join(format!("{id}.json"))
}

/// Tags du cache (`None` si le témoin n'a pas été rempli).
pub fn load(cache: &Path, id: &str) -> anyhow::Result<Option<WayTags>> {
    let p = cache_file(cache, id);
    let Ok(text) = std::fs::read_to_string(&p) else { return Ok(None) };
    let v: Value = serde_json::from_str(&text).with_context(|| format!("{} illisible", p.display()))?;
    Ok(Some(parse(&v)))
}

/// Réponse Overpass (`out tags`) → tags par way.
pub fn parse(v: &Value) -> WayTags {
    let mut out = WayTags::new();
    for e in v["elements"].as_array().into_iter().flatten() {
        let Some(id) = e["id"].as_u64() else { continue };
        let tags = e["tags"]
            .as_object()
            .map(|t| t.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned()))).collect())
            .unwrap_or_default();
        out.insert(id, tags);
    }
    out
}

/// Remplit le cache des tags des témoins négatifs absents du cache (en ligne).
pub async fn fill(bench: &Path, cache: &Path, ids: &[String]) -> anyhow::Result<Vec<String>> {
    let client = reqwest::Client::builder()
        .user_agent("osm-route-details-bench")
        .timeout(std::time::Duration::from_secs(180))
        .build()?;
    let mut filled = Vec::new();
    for id in ids {
        let p = cache_file(cache, id);
        if p.exists() {
            continue;
        }
        let (base, ways) = station_ways(bench, id)?;
        let mut uniq: Vec<u64> = ways.into_iter().filter(|w| *w != 0).collect();
        uniq.sort_unstable();
        uniq.dedup();
        let list = uniq.iter().map(u64::to_string).collect::<Vec<_>>().join(",");
        let q = format!("[out:json][timeout:120][date:\"{base}\"];way(id:{list});out tags;");
        let mut last = None;
        let mut body = None;
        for attempt in 0..4u64 {
            match client.post(OVERPASS).form(&[("data", q.as_str())]).send().await {
                Ok(r) if r.status().is_success() => {
                    body = Some(r.text().await?);
                    break;
                }
                Ok(r) => last = Some(format!("HTTP {}", r.status())),
                Err(e) => last = Some(e.to_string()),
            }
            tokio::time::sleep(std::time::Duration::from_secs(10 * (attempt + 1))).await;
        }
        let body = body.with_context(|| format!("{id} : Overpass en échec ({last:?})"))?;
        let v: Value =
            serde_json::from_str(&body).with_context(|| format!("{id} : réponse Overpass illisible"))?;
        let got = parse(&v);
        if uniq.iter().any(|w| !got.contains_key(w)) {
            anyhow::bail!("{id} : ways absentes de la réponse Overpass à {base}");
        }
        std::fs::create_dir_all(p.parent().unwrap_or(cache))?;
        let out = json!({"osm_base": base, "elements": v["elements"]});
        std::fs::write(&p, serde_json::to_string_pretty(&out)? + "\n")?;
        eprintln!("{id} : tags de {} ways à {base}", uniq.len());
        filled.push(id.clone());
    }
    Ok(filled)
}

/// Nature d'ouvrage d'une way selon ses tags (README §4 ter.6) : `bridge`, `gallery`
/// (`tunnel=avalanche_protector`), `tunnel`, `covered` (`covered=yes`), sinon `None`.
pub fn nature(tags: &BTreeMap<String, String>) -> Option<&'static str> {
    let on = |k: &str| tags.get(k).is_some_and(|v| v != "no");
    if on("bridge") {
        Some("bridge")
    } else if tags.get("tunnel").is_some_and(|v| v == "avalanche_protector") {
        Some("gallery")
    } else if on("tunnel") {
        Some("tunnel")
    } else if tags.get("covered").is_some_and(|v| v == "yes") {
        Some("covered")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natures_and_parsing() {
        let v = json!({"elements": [
            {"type": "way", "id": 1, "tags": {"highway": "primary", "tunnel": "avalanche_protector"}},
            {"type": "way", "id": 2, "tags": {"highway": "primary", "covered": "yes"}},
            {"type": "way", "id": 3, "tags": {"highway": "primary", "bridge": "viaduct", "covered": "yes"}},
            {"type": "way", "id": 4, "tags": {"highway": "primary", "tunnel": "no", "bridge": "no"}},
            {"type": "way", "id": 5, "tags": {"highway": "primary", "tunnel": "building_passage"}}
        ]});
        let t = parse(&v);
        let n: Vec<Option<&str>> = (1..=5).map(|k| nature(&t[&k])).collect();
        assert_eq!(n, [Some("gallery"), Some("covered"), Some("bridge"), None, Some("tunnel")]);
    }

    #[test]
    fn station_ways_read_the_base_and_the_way_column() {
        let bench = crate::BenchPaths::repo_bench();
        let (base, ways) = station_ways(&bench, "tn12-arly-galleries").unwrap();
        assert!(base.starts_with("2026-10-"), "{base}");
        assert!(ways.len() > 100 && ways.iter().all(|w| *w > 0));
    }
}
