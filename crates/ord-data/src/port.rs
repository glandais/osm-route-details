//! Port unique d'accès aux tuiles (T02 §2) : octets bruts d'une tuile z/x/y, issues distinctes.

use std::fmt;
use std::time::{Duration, SystemTime};

use bytes::Bytes;
use ord_domain::tiles::{SourceId, TileKey};

pub use crate::reader::{ReaderSettings, TileReader};

/// Métadonnées d'une réponse, persistées avec l'entrée du cache disque.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryMeta {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub fetched_at: SystemTime,
    pub server_max_age: Option<Duration>,
}

impl EntryMeta {
    pub fn now() -> Self {
        Self { etag: None, last_modified: None, fetched_at: SystemTime::now(), server_max_age: None }
    }
}

/// Réponse d'une source.
#[derive(Clone, Debug)]
pub enum Fetched {
    Body {
        bytes: Bytes,
        meta: EntryMeta,
    },
    /// « Rien ici » (VersaTiles 204), mis en cache.
    Empty,
    /// Pas de donnée par construction (404 Mapterhorn au-delà du zoom local), mis en cache.
    Absent,
    NotModified(EntryMeta),
}

/// Échec de transport. Les messages ne portent jamais d'URL ni de coordonnée.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    #[error("transport error: {0}")]
    Transport(String),
    #[error("upstream status {status}")]
    Http { status: u16 },
    #[error("rate limited")]
    RateLimited { retry_after: Option<Duration> },
    #[error("upstream timeout")]
    Timeout,
    #[error("offline: tile not in the disk cache")]
    Offline,
    #[error("i/o error: {0}")]
    Io(String),
}

impl FetchError {
    /// Relance permise (T02 §4.2) : 429/503 et autres 5xx, erreurs de transport et délais.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::RateLimited { .. } | Self::Transport(_) | Self::Timeout => true,
            Self::Http { status } => *status >= 500,
            Self::Offline | Self::Io(_) => false,
        }
    }
}

/// Sémantique d'un statut HTTP propre à une source (T02 §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusMeaning {
    Empty,
    Absent,
    Failed,
}

/// Table des statuts d'une source : seuls 204 et 404 varient d'une source à l'autre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusPolicy {
    pub no_content: StatusMeaning,
    pub not_found: StatusMeaning,
}

impl StatusPolicy {
    /// Mapterhorn : 204 en échec, 404 = au-delà du zoom local (`Absent`).
    pub const MAPTERHORN: Self = Self { no_content: StatusMeaning::Failed, not_found: StatusMeaning::Absent };
    /// VersaTiles : 204 = tuile vide, 404 = URL fausse (jamais « pas de rue »).
    pub const VERSATILES: Self = Self { no_content: StatusMeaning::Empty, not_found: StatusMeaning::Failed };
}

/// Description d'une source : identité, zoom, gabarit, espace de noms disque, relances.
#[derive(Clone, PartialEq)]
pub struct SourceDescriptor {
    pub id: SourceId,
    pub zoom: u8,
    /// Gabarit `{z}/{x}/{y}` ; jamais journalisé (une URL de tuile est une coordonnée).
    pub template: String,
    /// Espace de noms du cache disque (source + produit + version + hachage du gabarit).
    pub namespace: String,
    pub status_policy: StatusPolicy,
    /// Requêtes en vol vers l'hôte.
    pub concurrency: usize,
    /// Essais au total (T02 §4.2 : 6).
    pub max_attempts: u32,
    /// Attente avant la deuxième tentative sans `Retry-After` ; doublée ensuite (1, 2, 4… s).
    pub retry_base: Duration,
}

impl fmt::Debug for SourceDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceDescriptor")
            .field("id", &self.id)
            .field("zoom", &self.zoom)
            .field("namespace", &self.namespace)
            .field("concurrency", &self.concurrency)
            .finish_non_exhaustive()
    }
}

impl SourceDescriptor {
    pub fn url(&self, key: TileKey) -> String {
        self.template
            .replace("{z}", &key.z.to_string())
            .replace("{x}", &key.x.to_string())
            .replace("{y}", &key.y.to_string())
    }
}

/// Espace de noms disque lisible et stable entre compilations : `<slug>-<FNV-1a du gabarit>`.
pub fn namespace(slug: &str, template: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in template.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let slug: String = slug
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '-' })
        .collect();
    format!("{slug}-{hash:016x}")
}

/// Transport des octets bruts d'une tuile (HTTP XYZ à l'incrément 1).
#[async_trait::async_trait]
pub trait TileSource: Send + Sync {
    fn descriptor(&self) -> &SourceDescriptor;
    async fn fetch(&self, id: TileKey, revalidate: Option<&EntryMeta>) -> Result<Fetched, FetchError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_is_stable_and_depends_on_template() {
        let a = namespace("mapterhorn-unversioned", "https://tiles.mapterhorn.com/{z}/{x}/{y}.webp");
        assert_eq!(a, namespace("mapterhorn-unversioned", "https://tiles.mapterhorn.com/{z}/{x}/{y}.webp"));
        assert_ne!(a, namespace("mapterhorn-unversioned", "http://127.0.0.1:1/{z}/{x}/{y}.webp"));
        assert!(a.starts_with("mapterhorn-unversioned-"));
        assert_eq!(namespace("a b/c", "t").split('-').next(), Some("a"));
    }

    #[test]
    fn retryable_errors() {
        assert!(FetchError::Http { status: 503 }.is_retryable());
        assert!(!FetchError::Http { status: 404 }.is_retryable());
        assert!(FetchError::RateLimited { retry_after: None }.is_retryable());
        assert!(!FetchError::Offline.is_retryable());
    }
}
