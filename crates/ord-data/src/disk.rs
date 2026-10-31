//! Cache disque autoréparé (T02 §5.2) : un fichier par tuile, en-tête et corps publiés par un seul
//! `rename`, somme de contrôle xxh3. Une entrée illisible est supprimée et traitée comme absente ;
//! une erreur d'écriture est journalisée et n'échoue jamais la lecture.
//!
//! Enveloppe : `ORDT` + version (1 octet) + longueur de l'en-tête JSON (u32 LE) + en-tête + corps.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ord_domain::tiles::{TileKey, TtlClass};
use serde::{Deserialize, Serialize};
use xxhash_rust::xxh3::xxh3_64;

use crate::port::EntryMeta;

const MAGIC: &[u8; 4] = b"ORDT";
const VERSION: u8 = 1;

/// Issue persistée : `Empty` et `Absent` sont des réponses, pas des échecs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredIssue {
    Body,
    Empty,
    Absent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Header {
    issue: StoredIssue,
    etag: Option<String>,
    last_modified: Option<String>,
    fetched_at_s: u64,
    server_max_age_s: Option<u64>,
    /// `None` : immuable.
    ttl_s: Option<u64>,
    body_len: u32,
    body_xxh3: u64,
}

/// Entrée lue sur disque.
#[derive(Clone, Debug, PartialEq)]
pub struct DiskEntry {
    pub issue: StoredIssue,
    pub meta: EntryMeta,
    pub ttl: TtlClass,
    pub body: Vec<u8>,
}

impl DiskEntry {
    pub fn is_fresh(&self, now: SystemTime) -> bool {
        match self.ttl {
            TtlClass::Immutable => true,
            TtlClass::Expires(ttl) => now.duration_since(self.meta.fetched_at).map_or(true, |age| age < ttl),
        }
    }
}

/// Un espace de noms du cache disque : `{racine}/{espace}/{z}/{x}/{y}.tile`.
#[derive(Clone, Debug)]
pub struct DiskStore {
    dir: PathBuf,
}

fn secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl DiskStore {
    pub fn new(root: &Path, namespace: &str) -> Self {
        Self { dir: root.join(namespace) }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, key: TileKey) -> PathBuf {
        self.dir.join(key.z.to_string()).join(key.x.to_string()).join(format!("{}.tile", key.y))
    }

    /// Lit l'entrée ; une entrée corrompue, tronquée ou d'une autre version est supprimée.
    pub fn read(&self, key: TileKey) -> Option<DiskEntry> {
        let path = self.path(key);
        let bytes = std::fs::read(&path).ok()?;
        match parse(&bytes) {
            Some(entry) => Some(entry),
            None => {
                tracing::warn!(namespace = %self.dir.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(), "corrupt disk cache entry removed");
                let _ = std::fs::remove_file(&path);
                None
            }
        }
    }

    /// Supprime une entrée (corps qui ne décode plus, par exemple).
    pub fn remove(&self, key: TileKey) {
        let _ = std::fs::remove_file(self.path(key));
    }

    /// Écriture atomique (temporaire unique dans le même dossier, puis `rename`). Une erreur est
    /// journalisée, jamais propagée.
    pub fn write(&self, key: TileKey, issue: StoredIssue, meta: &EntryMeta, ttl: TtlClass, body: &[u8]) {
        if let Err(e) = self.try_write(key, issue, meta, ttl, body) {
            tracing::warn!(error = %e, "disk cache write failed");
        }
    }

    fn try_write(
        &self,
        key: TileKey,
        issue: StoredIssue,
        meta: &EntryMeta,
        ttl: TtlClass,
        body: &[u8],
    ) -> std::io::Result<()> {
        let path = self.path(key);
        let parent = path.parent().unwrap_or(&self.dir);
        std::fs::create_dir_all(parent)?;
        let header = Header {
            issue,
            etag: meta.etag.clone(),
            last_modified: meta.last_modified.clone(),
            fetched_at_s: secs(meta.fetched_at),
            server_max_age_s: meta.server_max_age.map(|d| d.as_secs()),
            ttl_s: match ttl {
                TtlClass::Immutable => None,
                TtlClass::Expires(d) => Some(d.as_secs()),
            },
            body_len: u32::try_from(body.len()).map_err(std::io::Error::other)?,
            body_xxh3: xxh3_64(body),
        };
        let header = serde_json::to_vec(&header).map_err(std::io::Error::other)?;
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        tmp.write_all(MAGIC)?;
        tmp.write_all(&[VERSION])?;
        tmp.write_all(&u32::try_from(header.len()).map_err(std::io::Error::other)?.to_le_bytes())?;
        tmp.write_all(&header)?;
        tmp.write_all(body)?;
        tmp.persist(&path).map_err(|e| e.error)?;
        Ok(())
    }
}

fn parse(bytes: &[u8]) -> Option<DiskEntry> {
    let rest = bytes.strip_prefix(MAGIC)?;
    let (&version, rest) = rest.split_first()?;
    if version != VERSION || rest.len() < 4 {
        return None;
    }
    let len = u32::from_le_bytes(rest[..4].try_into().ok()?) as usize;
    let rest = &rest[4..];
    if rest.len() < len {
        return None;
    }
    let header: Header = serde_json::from_slice(&rest[..len]).ok()?;
    let body = &rest[len..];
    if body.len() != header.body_len as usize || xxh3_64(body) != header.body_xxh3 {
        return None;
    }
    Some(DiskEntry {
        issue: header.issue,
        meta: EntryMeta {
            etag: header.etag,
            last_modified: header.last_modified,
            fetched_at: UNIX_EPOCH + Duration::from_secs(header.fetched_at_s),
            server_max_age: header.server_max_age_s.map(Duration::from_secs),
        },
        ttl: header.ttl_s.map_or(TtlClass::Immutable, |s| TtlClass::Expires(Duration::from_secs(s))),
        body: body.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const K: TileKey = TileKey { z: 12, x: 2047, y: 1400 };

    #[test]
    fn round_trip_and_self_repair() {
        let root = tempfile::tempdir().unwrap();
        let store = DiskStore::new(root.path(), "ns");
        assert!(store.read(K).is_none());
        let meta = EntryMeta { etag: Some("\"e\"".into()), ..EntryMeta::now() };
        store.write(K, StoredIssue::Body, &meta, TtlClass::Immutable, b"payload");
        let e = store.read(K).expect("entrée");
        assert_eq!(e.body, b"payload");
        assert_eq!(e.meta.etag.as_deref(), Some("\"e\""));
        assert!(e.is_fresh(SystemTime::now()));

        // Corps altéré : la somme de contrôle échoue, l'entrée est supprimée.
        let path = store.path(K);
        let mut raw = std::fs::read(&path).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0xff;
        std::fs::write(&path, &raw).unwrap();
        assert!(store.read(K).is_none());
        assert!(!path.exists(), "entrée corrompue supprimée");

        // Entrée tronquée (crash pendant une écriture).
        store.write(K, StoredIssue::Absent, &meta, TtlClass::Immutable, b"");
        let raw = std::fs::read(&path).unwrap();
        std::fs::write(&path, &raw[..raw.len() / 2]).unwrap();
        assert!(store.read(K).is_none());
    }

    #[test]
    fn expiry() {
        let root = tempfile::tempdir().unwrap();
        let store = DiskStore::new(root.path(), "ns");
        let old = EntryMeta { fetched_at: UNIX_EPOCH + Duration::from_secs(1_000), ..EntryMeta::now() };
        store.write(K, StoredIssue::Empty, &old, TtlClass::Expires(Duration::from_secs(60)), b"");
        let e = store.read(K).unwrap();
        assert_eq!(e.issue, StoredIssue::Empty);
        assert!(!e.is_fresh(SystemTime::now()));
    }
}
