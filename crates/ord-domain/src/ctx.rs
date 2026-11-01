//! Contexte de requête et journal des lectures de données (T02 §2.3).

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use crate::budget::{Deadline, RequestId};
use crate::tiles::{SourceId, TileKey};

#[derive(Debug)]
pub struct RequestCtx {
    pub request_id: RequestId,
    pub deadline: Deadline,
    log: Mutex<DataLog>,
}

impl RequestCtx {
    pub fn new(request_id: RequestId, deadline: Deadline) -> Self {
        Self { request_id, deadline, log: Mutex::new(DataLog::default()) }
    }

    /// Accès au journal ; un verrou empoisonné reste lisible (journal en ajout seul).
    pub fn log(&self) -> MutexGuard<'_, DataLog> {
        self.log.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[derive(Clone, Debug, Default)]
pub struct DataLog {
    /// Dédoublonné par (source, tuile), premier échec gardé.
    pub failures: Vec<TileFailure>,
    pub stale_served: Vec<(SourceId, TileKey)>,
    /// Plus ancienne lecture par source : `read_at` de `data_versions` (T01 §9).
    pub oldest_read: BTreeMap<SourceId, SystemTime>,
    pub counters: ReadCounters,
}

impl DataLog {
    pub fn record_failure(&mut self, failure: TileFailure) {
        let seen = self.failures.iter().any(|f| f.source == failure.source && f.tile == failure.tile);
        if !seen {
            self.failures.push(failure);
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadCounters {
    pub memory_hits: u64,
    pub disk_hits: u64,
    pub fetched: u64,
    pub fetched_bytes: u64,
    pub not_modified: u64,
}

/// Échec de lecture d'une tuile : jamais d'URL ni de coordonnée dans `message`.
#[derive(Clone, Debug, PartialEq)]
pub struct TileFailure {
    pub source: SourceId,
    pub tile: TileKey,
    pub kind: FailureKind,
    pub status: Option<u16>,
    /// `Retry-After` amont (T01 §4.4).
    pub retry_after: Option<Duration>,
    /// Fin du cache négatif de la tuile (T02 §5.1), posée par le lecteur qui l'y inscrit : seconde
    /// borne du `Retry-After` d'un 503 (T01 §4.4).
    pub negative_until: Option<Instant>,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    Network,
    RateLimited,
    Server,
    Invalid,
    Deadline,
    Offline,
}
