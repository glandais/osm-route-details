//! Lectures de démarrage (T02 §3, §5.3, §9) : version publiée de Mapterhorn, qui entre dans
//! l'espace de noms disque, et index `coverage-index.pmtiles` chargé en mémoire.
//!
//! Chaque lecture en ligne est recopiée dans le cache disque (`<racine>/meta/`,
//! `<racine>/coverage/`), relue en mode hors ligne ou si la source est en panne : le service
//! démarre toujours, au pire sans version (`unversioned`) ni index (`DemKind::Unknown`).
//!
//! Écart noté : `tiles.mapterhorn.com/tilejson.json` ne publie pas de version (vérifié le
//! 2026-10-31, tension 1 de T02 §12). La version est lue dans `tilejson.json` si elle y apparaît,
//! sinon dans `download.mapterhorn.com/download_urls.json` (champ `version`, « 0.0.13 »).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header;
use serde::{Deserialize, Serialize};

use crate::coverage::CoverageIndex;

/// Âge au-delà duquel l'index de couverture est revalidé (« rechargé chaque semaine », T02 §5.3).
pub const COVERAGE_INDEX_MAX_AGE: Duration = Duration::from_secs(7 * 86_400);

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Sidecar {
    version: Option<String>,
    etag: Option<String>,
    last_modified: Option<String>,
    fetched_at_s: u64,
}

fn now_s() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn write_atomic(path: &Path, bytes: &[u8]) {
    let result = (|| -> std::io::Result<()> {
        let dir = path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir)?;
        let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
        tmp.write_all(bytes)?;
        tmp.persist(path).map_err(|e| e.error)?;
        Ok(())
    })();
    if let Err(e) = result {
        tracing::warn!(error = %e, "startup cache write failed");
    }
}

fn read_sidecar(path: &Path) -> Option<Sidecar> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn version_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join("meta").join("mapterhorn-version.json")
}

async fn get_json(client: &reqwest::Client, url: &str) -> Option<serde_json::Value> {
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    serde_json::from_slice(&response.bytes().await.ok()?).ok()
}

/// Version publiée de Mapterhorn : en ligne (`tilejson.json`, sinon `download_urls.json`), copie
/// disque sinon. `None` : aucune version connue.
pub async fn mapterhorn_version(
    client: &reqwest::Client,
    tilejson_url: &str,
    versions_url: &str,
    cache_dir: &Path,
    offline: bool,
) -> Option<String> {
    let path = version_path(cache_dir);
    let cached = read_sidecar(&path).and_then(|s| s.version);
    if offline {
        return cached;
    }
    let mut version =
        get_json(client, tilejson_url).await.and_then(|v| v["version"].as_str().map(str::to_owned));
    if version.is_none() {
        version = get_json(client, versions_url).await.and_then(|v| v["version"].as_str().map(str::to_owned));
    }
    match version {
        Some(v) => {
            if cached.as_deref() != Some(v.as_str()) {
                tracing::info!(version = %v, "Mapterhorn version read");
            }
            let sidecar = Sidecar { version: Some(v.clone()), fetched_at_s: now_s(), ..Sidecar::default() };
            write_atomic(&path, &serde_json::to_vec(&sidecar).unwrap_or_default());
            Some(v)
        }
        None => {
            tracing::warn!("Mapterhorn version unavailable online; using the cached one");
            cached
        }
    }
}

/// Date HTTP (`Mon, 14 Sep 2026 05:51:25 GMT`) → `2026-09-14`.
fn http_date(v: &str) -> Option<String> {
    const MONTHS: [&str; 12] =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let parts: Vec<&str> = v.split_whitespace().collect();
    let (day, month, year) = (parts.get(1)?, parts.get(2)?, parts.get(3)?);
    let day: u32 = day.parse().ok()?;
    let month = MONTHS.iter().position(|m| m == month)? + 1;
    let year: u32 = year.parse().ok()?;
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

/// Version lisible d'un index : date `Last-Modified` si connue, sinon l'ETag sans guillemets.
fn index_version(s: &Sidecar) -> Option<String> {
    s.last_modified
        .as_deref()
        .and_then(http_date)
        .or_else(|| s.etag.as_ref().map(|e| e.trim_matches('"').to_owned()))
}

fn parse_index(bytes: Vec<u8>, version: Option<String>) -> Option<CoverageIndex> {
    match CoverageIndex::parse(bytes, version) {
        Ok(index) => Some(index),
        Err(e) => {
            tracing::warn!(error = %e, "coverage index unreadable");
            None
        }
    }
}

/// `coverage-index.pmtiles` : fichier local s'il est configuré, sinon copie disque revalidée
/// chaque semaine (ETag) ; hors ligne, la copie disque seule.
pub async fn coverage_index(
    client: &reqwest::Client,
    url: Option<&str>,
    local: Option<&Path>,
    cache_dir: &Path,
    offline: bool,
) -> Option<CoverageIndex> {
    if let Some(path) = local {
        let bytes = std::fs::read(path)
            .map_err(|e| tracing::warn!(error = %e, "coverage index file unreadable"))
            .ok()?;
        return parse_index(bytes, None);
    }
    let dir = cache_dir.join("coverage");
    let (data_path, meta_path) = (dir.join("coverage-index.pmtiles"), dir.join("coverage-index.json"));
    let cached = std::fs::read(&data_path).ok().zip(read_sidecar(&meta_path));
    let fresh = cached
        .as_ref()
        .is_some_and(|(_, s)| now_s().saturating_sub(s.fetched_at_s) < COVERAGE_INDEX_MAX_AGE.as_secs());
    let url = match url {
        Some(u) if !offline && !fresh => u,
        _ => return cached.and_then(|(b, s)| parse_index(b, index_version(&s))),
    };
    let mut req = client.get(url);
    if let Some(etag) = cached.as_ref().and_then(|(_, s)| s.etag.clone()) {
        req = req.header(header::IF_NONE_MATCH, etag);
    }
    let response = req.send().await;
    match response {
        Ok(r) if r.status() == reqwest::StatusCode::NOT_MODIFIED => {
            let (bytes, mut sidecar) = cached?;
            sidecar.fetched_at_s = now_s();
            write_atomic(&meta_path, &serde_json::to_vec(&sidecar).unwrap_or_default());
            parse_index(bytes, index_version(&sidecar))
        }
        Ok(r) if r.status().is_success() => {
            let h = |name: header::HeaderName| {
                r.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_owned)
            };
            let sidecar = Sidecar {
                version: None,
                etag: h(header::ETAG),
                last_modified: h(header::LAST_MODIFIED),
                fetched_at_s: now_s(),
            };
            let bytes = r.bytes().await.ok()?.to_vec();
            let index = parse_index(bytes.clone(), index_version(&sidecar))?;
            write_atomic(&data_path, &bytes);
            write_atomic(&meta_path, &serde_json::to_vec(&sidecar).unwrap_or_default());
            tracing::info!(bytes = bytes.len(), "coverage index downloaded");
            Some(index)
        }
        _ => {
            tracing::warn!("coverage index unavailable online; using the cached copy if any");
            cached.and_then(|(b, s)| parse_index(b, index_version(&s)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_version_is_the_last_modified_date() {
        let s = Sidecar { last_modified: Some("Mon, 14 Sep 2026 05:51:25 GMT".into()), ..Sidecar::default() };
        assert_eq!(index_version(&s).as_deref(), Some("2026-09-14"));
        let e = Sidecar { etag: Some("\"abc\"".into()), ..Sidecar::default() };
        assert_eq!(index_version(&e).as_deref(), Some("abc"));
    }

    #[tokio::test]
    async fn offline_reads_the_cached_copies_only() {
        let root = tempfile::tempdir().unwrap();
        let client = reqwest::Client::new();
        // Rien en cache : aucune version, aucun index, sans réseau.
        assert_eq!(
            mapterhorn_version(&client, "http://127.0.0.1:1/t", "http://127.0.0.1:1/v", root.path(), true)
                .await,
            None
        );
        assert!(coverage_index(&client, Some("http://127.0.0.1:1/i"), None, root.path(), true)
            .await
            .is_none());
        let sidecar = Sidecar { version: Some("0.0.13".into()), ..Sidecar::default() };
        write_atomic(&version_path(root.path()), &serde_json::to_vec(&sidecar).unwrap());
        let v =
            mapterhorn_version(&client, "http://127.0.0.1:1/t", "http://127.0.0.1:1/v", root.path(), true)
                .await;
        assert_eq!(v.as_deref(), Some("0.0.13"));
        // En ligne mais source injoignable : la copie disque sert encore.
        let v =
            mapterhorn_version(&client, "http://127.0.0.1:1/t", "http://127.0.0.1:1/v", root.path(), false)
                .await;
        assert_eq!(v.as_deref(), Some("0.0.13"));
    }
}
