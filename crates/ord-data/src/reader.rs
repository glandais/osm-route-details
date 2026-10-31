//! `TileReader<C>` (T02 §2.1, §5) : mémoire → négatif → disque → transport → validation →
//! écriture disque → mémoire. Un chargement est partagé entre requêtes concurrentes et tourne dans
//! une tâche détachée : un demandeur qui abandonne n'annule pas le téléchargement.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use bytes::Bytes;
use futures_util::future::join_all;
use ord_domain::catalog::DataVersion;
use ord_domain::ctx::{FailureKind, RequestCtx, TileFailure};
use ord_domain::tiles::{SourceId, TileCodec, TileKey, TileOutcome, TileStore, TtlClass};

use crate::disk::{DiskEntry, DiskStore, StoredIssue};
use crate::port::{EntryMeta, FetchError, Fetched, TileSource};

/// Réglages d'un lecteur (T07 `cache`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderSettings {
    /// Disque seul ; une tuile absente du disque est `Failed(Offline)` (T02 §4.2).
    pub offline: bool,
    /// Plafond du LRU mémoire, en octets décodés.
    pub memory_bytes: u64,
    /// Durée du cache négatif (60 s, T02 §5.1).
    pub negative_ttl: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoadOrigin {
    Disk,
    Fetched(u64),
    NotModified,
    Stale,
}

/// Valeur du LRU mémoire.
enum Cached<T> {
    Data { tile: Arc<T>, fetched_at: SystemTime, origin: LoadOrigin, weight: u32 },
    Empty { tile: Arc<T>, weight: u32 },
    Absent,
}

impl<T> Clone for Cached<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Data { tile, fetched_at, origin, weight } => Self::Data {
                tile: Arc::clone(tile),
                fetched_at: *fetched_at,
                origin: *origin,
                weight: *weight,
            },
            Self::Empty { tile, weight } => Self::Empty { tile: Arc::clone(tile), weight: *weight },
            Self::Absent => Self::Absent,
        }
    }
}

impl<T> Cached<T> {
    fn weight(&self) -> u32 {
        match self {
            Self::Data { weight, .. } | Self::Empty { weight, .. } => *weight,
            Self::Absent => 16,
        }
    }

    fn with_origin(self, origin: LoadOrigin) -> Self {
        match self {
            Self::Data { tile, fetched_at, weight, .. } => Self::Data { tile, fetched_at, origin, weight },
            other => other,
        }
    }
}

struct Inner<C: TileCodec> {
    source: Arc<dyn TileSource>,
    codec: C,
    disk: Option<DiskStore>,
    memory: moka::future::Cache<TileKey, Cached<C::Tile>>,
    negative: moka::sync::Cache<TileKey, TileFailure>,
    offline: bool,
}

/// Lecteur de tuiles d'une source, vu par le pipeline comme `Arc<dyn TileStore<T>>`.
pub struct TileReader<C: TileCodec> {
    inner: Arc<Inner<C>>,
}

impl<C: TileCodec> std::fmt::Debug for TileReader<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TileReader")
            .field("source", self.inner.source.descriptor())
            .field("offline", &self.inner.offline)
            .finish_non_exhaustive()
    }
}

fn failure(
    source: SourceId,
    tile: TileKey,
    kind: FailureKind,
    status: Option<u16>,
    message: String,
) -> TileFailure {
    TileFailure { source, tile, kind, status, retry_after: None, message }
}

fn fetch_failure(source: SourceId, tile: TileKey, e: &FetchError) -> TileFailure {
    let (kind, status, retry_after) = match e {
        FetchError::Transport(_) | FetchError::Io(_) | FetchError::Timeout => {
            (FailureKind::Network, None, None)
        }
        FetchError::Http { status } => (FailureKind::Server, Some(*status), None),
        FetchError::RateLimited { retry_after } => (FailureKind::RateLimited, Some(429), *retry_after),
        FetchError::Offline => (FailureKind::Offline, None, None),
    };
    TileFailure { source, tile, kind, status, retry_after, message: e.to_string() }
}

impl<C: TileCodec> TileReader<C> {
    pub fn new(
        source: Arc<dyn TileSource>,
        codec: C,
        disk: Option<DiskStore>,
        settings: &ReaderSettings,
    ) -> Self {
        let memory = moka::future::Cache::builder()
            .max_capacity(settings.memory_bytes)
            .weigher(|_k: &TileKey, v: &Cached<C::Tile>| v.weight())
            .build();
        let negative = moka::sync::Cache::builder()
            .time_to_live(settings.negative_ttl.max(Duration::from_millis(1)))
            .build();
        Self { inner: Arc::new(Inner { source, codec, disk, memory, negative, offline: settings.offline }) }
    }

    pub fn source_id(&self) -> SourceId {
        self.inner.source.descriptor().id
    }

    /// Lit une tuile ; les échecs et les compteurs vont au journal de la requête.
    pub async fn get(&self, key: TileKey, ctx: &RequestCtx) -> TileOutcome<C::Tile> {
        let src = self.source_id();
        if let Some(f) = self.inner.negative.get(&key) {
            // Réinscrite au journal de chaque requête qui la rencontre (T02 §2.3).
            ctx.log().record_failure(f.clone());
            return TileOutcome::Failed(f);
        }
        let inner = Arc::clone(&self.inner);
        let deadline = ctx.deadline.at;
        let loaded = self
            .inner
            .memory
            .entry(key)
            .or_try_insert_with(async move {
                let task = tokio::spawn(async move { inner.load(key, deadline).await });
                task.await.unwrap_or_else(|_| {
                    Err(failure(src, key, FailureKind::Network, None, "tile load task failed".into()))
                })
            })
            .await;
        match loaded {
            Ok(entry) => {
                let inserted_here = entry.is_fresh();
                let cached = entry.into_value();
                {
                    let mut log = ctx.log();
                    match &cached {
                        Cached::Data { fetched_at, origin, .. } => {
                            log.oldest_read
                                .entry(src)
                                .and_modify(|t| *t = (*t).min(*fetched_at))
                                .or_insert(*fetched_at);
                            if !inserted_here {
                                log.counters.memory_hits += 1;
                            } else {
                                match origin {
                                    LoadOrigin::Disk => log.counters.disk_hits += 1,
                                    LoadOrigin::Fetched(n) => {
                                        log.counters.fetched += 1;
                                        log.counters.fetched_bytes += n;
                                    }
                                    LoadOrigin::NotModified => log.counters.not_modified += 1,
                                    LoadOrigin::Stale => log.stale_served.push((src, key)),
                                }
                            }
                        }
                        Cached::Empty { .. } | Cached::Absent => {
                            if !inserted_here {
                                log.counters.memory_hits += 1;
                            }
                        }
                    }
                }
                match cached {
                    Cached::Data { tile, origin, .. } => {
                        let stale = origin == LoadOrigin::Stale;
                        if stale {
                            // Servie en secours : la prochaine requête retentera la source.
                            self.inner.memory.invalidate(&key).await;
                        }
                        TileOutcome::Data { tile, stale }
                    }
                    Cached::Empty { tile, .. } => TileOutcome::Empty(tile),
                    Cached::Absent => TileOutcome::Absent,
                }
            }
            Err(f) => {
                let f = (*f).clone();
                self.inner.negative.insert(key, f.clone());
                ctx.log().record_failure(f.clone());
                TileOutcome::Failed(f)
            }
        }
    }
}

impl<C: TileCodec> Inner<C> {
    fn id(&self) -> SourceId {
        self.source.descriptor().id
    }

    async fn decode(self: &Arc<Self>, key: TileKey, bytes: Bytes) -> Result<C::Tile, String> {
        let this = Arc::clone(self);
        tokio::task::spawn_blocking(move || this.codec.decode(key, &bytes).map_err(|e| e.to_string()))
            .await
            .unwrap_or_else(|_| Err("decode task failed".into()))
    }

    fn data(&self, tile: C::Tile, fetched_at: SystemTime, origin: LoadOrigin) -> Cached<C::Tile> {
        let weight = self.codec.weight(&tile);
        Cached::Data { tile: Arc::new(tile), fetched_at, origin, weight }
    }

    fn empty(&self, key: TileKey) -> Cached<C::Tile> {
        let tile = self.codec.empty(key);
        let weight = self.codec.weight(&tile);
        Cached::Empty { tile: Arc::new(tile), weight }
    }

    async fn from_entry(self: &Arc<Self>, key: TileKey, entry: &DiskEntry) -> Option<Cached<C::Tile>> {
        match entry.issue {
            StoredIssue::Body => {
                let tile = self.decode(key, Bytes::from(entry.body.clone())).await.ok()?;
                Some(self.data(tile, entry.meta.fetched_at, LoadOrigin::Disk))
            }
            StoredIssue::Empty => Some(self.empty(key)),
            StoredIssue::Absent => Some(Cached::Absent),
        }
    }

    async fn disk_op<R: Send + 'static>(
        &self,
        op: impl FnOnce(&DiskStore) -> R + Send + 'static,
    ) -> Option<R> {
        let disk = self.disk.clone()?;
        tokio::task::spawn_blocking(move || op(&disk)).await.ok()
    }

    async fn load(self: Arc<Self>, key: TileKey, deadline: Instant) -> Result<Cached<C::Tile>, TileFailure> {
        let src = self.id();
        // 1–2. Disque : entrée fraîche servie ; entrée qui ne décode plus supprimée.
        let mut stale: Option<(DiskEntry, Cached<C::Tile>)> = None;
        if let Some(entry) = self.disk_op(move |d| d.read(key)).await.flatten() {
            match self.from_entry(key, &entry).await {
                Some(cached) if self.offline || entry.is_fresh(SystemTime::now()) => return Ok(cached),
                Some(cached) => stale = Some((entry, cached)),
                None => {
                    self.disk_op(move |d| d.remove(key)).await;
                }
            }
        }
        if self.offline {
            return Err(fetch_failure(src, key, &FetchError::Offline));
        }
        // 3. Source, avec revalidation si une entrée périmée existe.
        let revalidate = stale.as_ref().map(|(e, _)| e.meta.clone());
        let fetched = self.fetch_with_retries(key, revalidate.as_ref(), deadline).await;
        let serve_stale = |stale: Option<(DiskEntry, Cached<C::Tile>)>, f: TileFailure| match stale {
            Some((_, cached)) => Ok(cached.with_origin(LoadOrigin::Stale)),
            None => Err(f),
        };
        match fetched {
            Ok(Fetched::Body { bytes, meta }) => {
                // 4. Décoder d'abord : un corps invalide n'atteint jamais le disque.
                match self.decode(key, bytes.clone()).await {
                    Ok(tile) => {
                        let ttl = self.codec.ttl_class(&tile);
                        let fetched_at = meta.fetched_at;
                        let len = bytes.len() as u64;
                        self.disk_op(move |d| d.write(key, StoredIssue::Body, &meta, ttl, &bytes)).await;
                        Ok(self.data(tile, fetched_at, LoadOrigin::Fetched(len)))
                    }
                    Err(msg) => {
                        tracing::warn!(source = ?src, "invalid tile body rejected");
                        serve_stale(
                            stale,
                            failure(
                                src,
                                key,
                                FailureKind::Invalid,
                                Some(200),
                                format!("invalid tile: {msg}"),
                            ),
                        )
                    }
                }
            }
            Ok(Fetched::Empty) => {
                let cached = self.empty(key);
                let ttl = match &cached {
                    Cached::Empty { tile, .. } => self.codec.ttl_class(tile),
                    _ => TtlClass::Immutable,
                };
                self.disk_op(move |d| d.write(key, StoredIssue::Empty, &EntryMeta::now(), ttl, b"")).await;
                Ok(cached)
            }
            Ok(Fetched::Absent) => {
                self.disk_op(move |d| {
                    d.write(key, StoredIssue::Absent, &EntryMeta::now(), TtlClass::Immutable, b"")
                })
                .await;
                Ok(Cached::Absent)
            }
            Ok(Fetched::NotModified(meta)) => match stale {
                Some((entry, cached)) => {
                    let meta = EntryMeta {
                        etag: meta.etag.or(entry.meta.etag.clone()),
                        last_modified: meta.last_modified.or(entry.meta.last_modified.clone()),
                        ..meta
                    };
                    let fetched_at = meta.fetched_at;
                    self.disk_op(move |d| d.write(key, entry.issue, &meta, entry.ttl, &entry.body)).await;
                    Ok(match cached {
                        Cached::Data { tile, weight, .. } => {
                            Cached::Data { tile, fetched_at, origin: LoadOrigin::NotModified, weight }
                        }
                        other => other,
                    })
                }
                None => Err(failure(
                    src,
                    key,
                    FailureKind::Invalid,
                    Some(304),
                    "304 without a cached entry".into(),
                )),
            },
            // 5. Échec : entrée périmée servie si elle décode (stale-if-error).
            Err(f) => serve_stale(stale, f),
        }
    }

    async fn fetch_with_retries(
        &self,
        key: TileKey,
        revalidate: Option<&EntryMeta>,
        deadline: Instant,
    ) -> Result<Fetched, TileFailure> {
        let d = self.source.descriptor();
        let mut attempt: u32 = 1;
        loop {
            match self.source.fetch(key, revalidate).await {
                Err(e) if e.is_retryable() && attempt < d.max_attempts.max(1) => {
                    let backoff = d.retry_base.saturating_mul(1 << (attempt - 1).min(16));
                    let wait = match &e {
                        FetchError::RateLimited { retry_after: Some(ra) } => *ra,
                        _ => backoff,
                    };
                    // Aucune relance au-delà de l'échéance de la requête (T02 §4.2).
                    if Instant::now() + wait >= deadline {
                        let mut f = fetch_failure(d.id, key, &e);
                        f.kind = FailureKind::Deadline;
                        return Err(f);
                    }
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                }
                other => return other.map_err(|e| fetch_failure(d.id, key, &e)),
            }
        }
    }
}

#[async_trait::async_trait]
impl<C: TileCodec> TileStore<C::Tile> for TileReader<C> {
    async fn get_many(&self, keys: &[TileKey], ctx: &RequestCtx) -> Vec<TileOutcome<C::Tile>> {
        join_all(keys.iter().map(|&k| self.get(k, ctx))).await
    }

    fn data_version(&self) -> DataVersion {
        let d = self.inner.source.descriptor();
        DataVersion {
            source: "tiles",
            id: Some(d.namespace.clone()),
            date: None,
            zoom: Some(d.zoom),
            products: Vec::new(),
            role: ord_domain::catalog::SourceRole::Primary,
            indicative: true,
        }
    }
}
