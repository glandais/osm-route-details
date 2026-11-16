//! Stockage des surcouches d'équipe (note lot 3a §3) : fichiers chiffrés sous
//! `<overlay_dir>/graph-<graph_id>/<ns>/<team>.ordo.enc`, remplacement atomique, cache mémoire, quotas, purge.
//!
//! Toutes les fonctions sont **bloquantes** (disque, chiffrement, construction de `TeamData`) : les appelants passent
//! par `spawn_blocking`. Les noms de fichiers sont des hexadécimaux calculés par le service (`crypto`), jamais une
//! chaîne du client. Rien d'ici ne journalise un nom de fichier, une clé ou une référence d'équipe.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ord_graph::format::GraphIdentity;
use ord_graph::overlay::Overlay;
use ord_loops::{Engine, TeamData};
use ord_loops_check::{Config as CheckConfig, GraphIndex, TeamCheck};

use super::windows::TeamEvictor;
use serde::{Deserialize, Serialize};

use super::crypto::Credentials;

pub const FILE_EXT: &str = "ordo.enc";
const TMP_MARK: &str = ".tmp-";
/// Un `.tmp` plus ancien que cela est un reste d'écriture interrompue.
const TMP_MAX_AGE: Duration = Duration::from_secs(3600);
/// Au démarrage, seuls les temporaires plus vieux que cela partent : pendant un déploiement « start-first », l'ancienne
/// instance peut finir une écriture (un `write` et un `fsync` de moins d'un Mio, bien en deçà).
const TMP_STARTUP_AGE: Duration = Duration::from_secs(300);

/// Le moteur sur des fichiers projetés en mémoire.
pub type GenEngine = Engine<memmap2::Mmap>;

/// Réglages du stockage.
#[derive(Clone, Debug)]
pub struct StoreConfig {
    pub dir: PathBuf,
    pub ttl: Duration,
    pub stale_grace: Duration,
    pub max_namespaces: usize,
    pub max_overlays_per_namespace: usize,
    /// Octets au plus par espace : un environnement ne peut pas user la place des autres.
    pub max_namespace_bytes: u64,
    /// Préfixes `ns8` autorisés à créer un espace ; vide : tout le monde (jusqu'à `max_namespaces`).
    pub allowed_namespaces: Vec<String>,
    pub team_cache_entries: usize,
}

/// Crochets des tests : jamais activés en production.
#[doc(hidden)]
#[derive(Debug, Default)]
pub struct TestHooks {
    /// L'écriture d'une surcouche échoue après le fichier temporaire (disque plein simulé).
    pub fail_commit: AtomicBool,
    /// Chaque parcours est altéré avant le contrôleur (une arête de moins).
    pub corrupt_routes: AtomicBool,
    /// Le `fsync` du dossier échoue après le `rename` (le dépôt est quand même réussi).
    pub fail_dir_sync: AtomicBool,
    /// Constructions de `TeamData` faites (une par équipe et par fichier, jamais deux en parallèle).
    pub team_builds: std::sync::atomic::AtomicU64,
}

/// Métadonnées d'une surcouche, dans la partie chiffrée du fichier.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileMeta {
    /// Secondes depuis l'époque Unix.
    pub deposited_at: i64,
    pub source_digest: Option<String>,
    /// `params_hash` de l'appariement (hexadécimal).
    pub matcher_hash: String,
    pub edges: u64,
    pub team_km: f64,
    pub tracks: u64,
    pub tracks_excluded: u64,
    pub tracks_unreadable: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaleReason {
    /// Déposée pour un autre graphe.
    Graph,
    /// Déposée par un autre appariement.
    Matcher,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Absent,
    Ready,
    Stale(StaleReason),
}

/// État d'une équipe, sans construire ses données.
#[derive(Clone, Debug)]
pub struct Status {
    pub state: State,
    pub meta: Option<FileMeta>,
    /// Date du fichier quand ses métadonnées ne sont pas lisibles (autre graphe).
    pub file_time: Option<i64>,
}

/// Empreinte d'un fichier : un remplacement atomique change l'inode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileId {
    ino: u64,
    mtime_ns: u128,
    len: u64,
}

fn file_id(md: &std::fs::Metadata) -> FileId {
    #[cfg(unix)]
    let ino = std::os::unix::fs::MetadataExt::ino(md);
    #[cfg(not(unix))]
    let ino = 0;
    let mtime_ns =
        md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
    FileId { ino, mtime_ns, len: md.len() }
}

/// Pourquoi une équipe n'est pas rendue : pas prête, plus de place dans le budget de mémoire pour la construire
/// (503), ou la demande est coupée avant (client parti, échéance : 504).
#[derive(Debug)]
pub enum ReadyError {
    State(State),
    Overloaded,
    Stopped,
    /// La construction a quitté le voisinage de l'équipe, même élargi (jamais un résultat faux : 500).
    Failed,
}

/// Réserve la mémoire transitoire d'une construction d'équipe (`Overloaded` : pas de place à l'échéance ; `Stopped` :
/// coupé par l'appelant) ; la garde rendue libère la réservation à sa chute.
pub type Reserve<'a> = &'a dyn Fn(usize) -> Result<Box<dyn Send + 'a>, ReadyError>;

/// Le budget de mémoire qui compte les équipes résidentes (lot 6, finition) : le stockage lui dit ce qui entre et ce
/// qui sort ; c'est lui qui, par [`TeamEvictor`], demande d'évincer quand la place manque. Les appels ne se font
/// **jamais** sous le verrou du cache d'équipes.
pub trait TeamMemory: Send + Sync {
    /// Une équipe est devenue résidente : ses octets comptent, sans pouvoir être refusés.
    fn hold_team(&self, bytes: usize);
    /// Des octets d'équipes ne sont plus résidents.
    fn release_team(&self, bytes: usize);
}

/// Une équipe prête : surcouche et données du moteur.
pub struct Entry {
    id: FileId,
    /// Numéro unique de cette construction : la clé de ses agrégats dans les fenêtres en cache.
    pub serial: u64,
    pub meta: FileMeta,
    pub overlay: Overlay,
    pub team: TeamData,
    /// Arêtes comblées du contrôleur indépendant, calculées une fois sur la zone de l'équipe (lot 6, §8.1).
    pub check: TeamCheck,
    /// Octets tenus : surcouche décodée, données du moteur et tables du contrôleur (capacités réelles).
    pub bytes: usize,
}

static NEXT_SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entry").field("edges", &self.overlay.len()).finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("storage quota reached")]
    Quota,
    #[error("storage failure")]
    Io,
}

/// Ce qu'une purge a retiré.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PurgeReport {
    pub ttl: u64,
    pub stale_graph: u64,
    pub tmp: u64,
    pub empty: u64,
}

type CacheKey = (String, String);

pub struct Store {
    cfg: StoreConfig,
    graph_id: String,
    identity: GraphIdentity,
    matcher_hash: u64,
    hooks: Arc<TestHooks>,
    locks: Mutex<HashSet<CacheKey>>,
    /// Du moins récemment au plus récemment utilisé.
    cache: Mutex<Vec<(CacheKey, Arc<Entry>)>>,
    /// Une construction de `TeamData` à la fois par équipe : les autres attendent et réutilisent l'entrée.
    builds: Mutex<std::collections::HashMap<CacheKey, Arc<Mutex<()>>>>,
    /// Le budget qui compte les entrées du cache.
    memory: Arc<dyn TeamMemory>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").field("graph_id", &self.graph_id).finish_non_exhaustive()
    }
}

impl TeamEvictor for Store {
    fn evict_idle(&self) -> Option<(usize, u64)> {
        let mut cache = lock(&self.cache);
        let i = lru_idle(&cache)?;
        let e = cache.remove(i).1;
        Some((e.bytes, e.serial))
    }
}

/// La moins récemment utilisée des entrées (la liste va de la plus ancienne à la plus récente) que personne ne tient.
fn lru_idle<K, V>(cache: &[(K, Arc<V>)]) -> Option<usize> {
    cache.iter().position(|(_, e)| Arc::strong_count(e) == 1)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn secs(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Verrou d'une équipe (un dépôt à la fois) ; relâché à la chute.
#[derive(Debug)]
pub struct TeamGuard {
    store: Arc<Store>,
    key: CacheKey,
}

impl Drop for TeamGuard {
    fn drop(&mut self) {
        lock(&self.store.locks).remove(&self.key);
    }
}

impl Store {
    pub fn new(
        cfg: StoreConfig,
        graph_id: String,
        identity: GraphIdentity,
        matcher_hash: u64,
        hooks: Arc<TestHooks>,
        memory: Arc<dyn TeamMemory>,
    ) -> Arc<Self> {
        Arc::new(Self {
            cfg,
            graph_id,
            identity,
            matcher_hash,
            hooks,
            locks: Mutex::new(HashSet::new()),
            cache: Mutex::new(Vec::new()),
            builds: Mutex::new(std::collections::HashMap::new()),
            memory,
        })
    }

    /// Retire du cache les entrées dont la clé satisfait `pred` et rend leurs octets au budget.
    fn forget(&self, pred: impl Fn(&CacheKey) -> bool) {
        let mut freed = 0;
        lock(&self.cache).retain(|(k, e)| {
            let gone = pred(k);
            if gone {
                freed += e.bytes;
            }
            !gone
        });
        if freed > 0 {
            self.memory.release_team(freed);
        }
    }

    /// Octets des équipes en cache (tests et jauge).
    pub fn cached_bytes(&self) -> usize {
        lock(&self.cache).iter().map(|(_, e)| e.bytes).sum()
    }

    pub fn graph_id(&self) -> &str {
        &self.graph_id
    }

    pub fn matcher_hash_hex(&self) -> String {
        format!("{:016x}", self.matcher_hash)
    }

    pub fn ttl(&self) -> Duration {
        self.cfg.ttl
    }

    fn graph_dir(&self) -> PathBuf {
        self.cfg.dir.join(format!("graph-{}", self.graph_id))
    }

    fn file_path(&self, c: &Credentials) -> PathBuf {
        self.graph_dir().join(&c.ns).join(format!("{}.{FILE_EXT}", c.team))
    }

    /// Les dossiers `graph-*` du volume.
    fn graph_dirs(&self) -> Vec<PathBuf> {
        let Ok(rd) = std::fs::read_dir(&self.cfg.dir) else { return Vec::new() };
        rd.filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with("graph-"))
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect()
    }

    // --- Verrous -----------------------------------------------------------------------------------------------

    /// Prend le verrou de l'équipe ; `None` si un dépôt (ou une suppression) la tient déjà.
    pub fn try_lock(self: &Arc<Self>, c: &Credentials) -> Option<TeamGuard> {
        let key = (c.ns.clone(), c.team.clone());
        lock(&self.locks).insert(key.clone()).then(|| TeamGuard { store: Arc::clone(self), key })
    }

    /// Combien d'équipes de cet espace sont verrouillées (dépôts ou suppressions en cours) ?
    pub fn namespace_locks(&self, c: &Credentials) -> usize {
        lock(&self.locks).iter().filter(|(ns, _)| *ns == c.ns).count()
    }

    /// Un dépôt est-il en cours dans cet espace ?
    pub fn namespace_busy(&self, c: &Credentials) -> bool {
        lock(&self.locks).iter().any(|(ns, _)| *ns == c.ns)
    }

    // --- Lecture -----------------------------------------------------------------------------------------------

    /// Lit, déchiffre et décode le fichier courant. Un fichier qui ne se déchiffre pas ou ne se décode pas est
    /// effacé, compté, et traité comme absent.
    fn read_current(&self, c: &Credentials) -> Option<(FileId, FileMeta, Overlay)> {
        let path = self.file_path(c);
        let mut f = std::fs::File::open(&path).ok()?;
        let id = file_id(&f.metadata().ok()?);
        let mut raw = Vec::with_capacity(usize::try_from(id.len).unwrap_or(0));
        f.read_to_end(&mut raw).ok()?;
        match self.decode(c, &raw) {
            Some((meta, overlay)) => Some((id, meta, overlay)),
            None => {
                let _ = std::fs::remove_file(&path);
                self.forget(|k| k.0 == c.ns && k.1 == c.team);
                metrics::counter!("ord_overlay_corrupt_total").increment(1);
                tracing::warn!(ns8 = c.ns8(), "overlay file unreadable: removed");
                None
            }
        }
    }

    fn decode(&self, c: &Credentials, raw: &[u8]) -> Option<(FileMeta, Overlay)> {
        let plain = c.open(&self.graph_id, raw).ok()?;
        let len = u32::from_le_bytes(plain.get(..4)?.try_into().ok()?) as usize;
        let meta: FileMeta = serde_json::from_slice(plain.get(4..4 + len)?).ok()?;
        let overlay = Overlay::decode_for(plain.get(4 + len..)?, &self.identity).ok()?;
        Some((meta, overlay))
    }

    fn stale_elsewhere(&self, c: &Credentials) -> Option<i64> {
        let current = self.graph_dir();
        self.graph_dirs()
            .into_iter()
            .filter(|d| *d != current)
            .filter_map(|d| std::fs::metadata(d.join(&c.ns).join(format!("{}.{FILE_EXT}", c.team))).ok())
            .filter_map(|md| md.modified().ok())
            .map(secs)
            .max()
    }

    /// État de l'équipe sans construire ses données (`GET`).
    pub fn status(&self, c: &Credentials) -> Status {
        match self.read_current(c) {
            Some((_, meta, _)) if meta.matcher_hash == self.matcher_hash_hex() => {
                Status { state: State::Ready, meta: Some(meta), file_time: None }
            }
            Some((_, meta, _)) => {
                Status { state: State::Stale(StaleReason::Matcher), meta: Some(meta), file_time: None }
            }
            None => match self.stale_elsewhere(c) {
                Some(t) => Status { state: State::Stale(StaleReason::Graph), meta: None, file_time: Some(t) },
                None => Status { state: State::Absent, meta: None, file_time: None },
            },
        }
    }

    /// L'équipe prête pour une génération : depuis le cache si le fichier n'a pas changé, sinon relue puis `TeamData`
    /// construit. `Err(State)` quand elle n'est pas prête (absente ou périmée).
    pub fn ready(
        &self,
        c: &Credentials,
        engine: &GenEngine,
        gi: &GraphIndex,
        reserve: Reserve<'_>,
    ) -> Result<Arc<Entry>, ReadyError> {
        let key = (c.ns.clone(), c.team.clone());
        if let Some(entry) = self.cached(&key, &self.file_path(c)) {
            metrics::counter!("ord_overlay_cache_hits_total").increment(1);
            return Ok(entry);
        }
        // Une seule construction par équipe : une seconde demande attend la première et relit le cache.
        let build_lock = Arc::clone(lock(&self.builds).entry(key.clone()).or_default());
        let _building = lock(&build_lock);
        let result = self.ready_build(c, engine, gi, reserve, &key);
        drop(_building);
        let mut builds = lock(&self.builds);
        // Plus personne n'attend (le verrou n'est tenu que par nous et `builds`) : l'entrée de la table s'efface.
        if Arc::strong_count(&build_lock) <= 2 {
            builds.remove(&key);
        }
        result
    }

    /// L'entrée du cache si elle correspond au fichier présent ; la marque comme la plus récemment utilisée.
    fn cached(&self, key: &CacheKey, path: &Path) -> Option<Arc<Entry>> {
        let entry = lock(&self.cache).iter().find(|(k, _)| k == key).map(|(_, e)| Arc::clone(e))?;
        let md = std::fs::metadata(path).ok()?;
        if entry.id != file_id(&md) {
            return None;
        }
        let mut cache = lock(&self.cache);
        if let Some(i) = cache.iter().position(|(k, _)| k == key) {
            let item = cache.remove(i);
            cache.push(item);
        }
        Some(entry)
    }

    fn ready_build(
        &self,
        c: &Credentials,
        engine: &GenEngine,
        gi: &GraphIndex,
        reserve: Reserve<'_>,
        key: &CacheKey,
    ) -> Result<Arc<Entry>, ReadyError> {
        let key = key.clone();
        if let Some(entry) = self.cached(&key, &self.file_path(c)) {
            // Construite pendant que nous attendions.
            metrics::counter!("ord_overlay_cache_hits_total").increment(1);
            return Ok(entry);
        }
        metrics::counter!("ord_overlay_cache_misses_total").increment(1);
        let Some((id, meta, overlay)) = self.read_current(c) else {
            self.forget(|k| *k == key);
            return Err(ReadyError::State(match self.stale_elsewhere(c) {
                Some(_) => State::Stale(StaleReason::Graph),
                None => State::Absent,
            }));
        };
        if meta.matcher_hash != self.matcher_hash_hex() {
            return Err(ReadyError::State(State::Stale(StaleReason::Matcher)));
        }
        self.hooks.team_builds.fetch_add(1, Ordering::Relaxed);
        let stale = || ReadyError::State(State::Stale(StaleReason::Graph));
        let check_cfg = CheckConfig::default();
        let plan = TeamData::plan_build(engine, &overlay).map_err(|_| stale())?;
        let check_plan = TeamCheck::plan(engine.base(), &overlay, &check_cfg);
        // La construction tient de la mémoire transitoire (fenêtres de la zone de l'équipe, incidence du contrôleur) :
        // celle des **deux** constructions, qui tournent en parallèle, est réservée dans le budget du générateur, ou
        // la demande attend, puis 503.
        let guard = reserve(plan.estimated_bytes + check_plan.estimated_bytes)?;
        // Les deux constructions ne dépendent que de la surcouche : en parallèle, le temps est celui de la plus longue
        // (pages froides, une grosse équipe passait 8 s en série).
        let (team, check) = std::thread::scope(|sc| {
            let check =
                sc.spawn(|| TeamCheck::build_planned(engine.base(), gi, &overlay, &check_cfg, &check_plan));
            let team = TeamData::build_planned(engine, &overlay, &plan);
            (team, check.join())
        });
        let check = check.unwrap_or_else(|e| std::panic::resume_unwind(e));
        let (team, check) = match (team, check) {
            (Ok(t), Ok(c)) => (t, c),
            (Err(ord_loops::EngineError::TeamZoneEscape), _) | (_, Err(_)) => {
                metrics::counter!("ord_generator_team_build_escapes_total").increment(1);
                tracing::warn!("team build left the neighbourhood of the team network even widened");
                return Err(ReadyError::Failed);
            }
            (Err(_), _) => return Err(stale()),
        };
        if team.zone_retried() || check.zone_retried() {
            metrics::counter!("ord_generator_team_build_escapes_total").increment(1);
            tracing::warn!("team build retried with wider hot cells");
        }
        // La mémoire transitoire est rendue avant de compter ce qui reste : sinon l'entrée neuve serait comptée avec sa
        // propre construction et chasserait des équipes pour rien.
        drop(guard);
        let serial = NEXT_SERIAL.fetch_add(1, Ordering::Relaxed);
        let bytes = std::mem::size_of::<Entry>() + overlay.bytes() + team.bytes() + check.bytes();
        let entry = Arc::new(Entry { id, serial, meta, overlay, team, check, bytes });
        // Comptée d'abord (le budget évince ce que personne ne tient pour lui faire de la place), cachée ensuite : elle
        // ne peut pas s'évincer elle-même.
        self.memory.hold_team(bytes);
        let mut freed = 0;
        {
            let mut cache = lock(&self.cache);
            if let Some(i) = cache.iter().position(|(k, _)| *k == key) {
                freed += cache.remove(i).1.bytes;
            }
            cache.push((key, Arc::clone(&entry)));
            // Plafond en nombre : la plus ancienne part, tenue ou non (sa mémoire revient quand la génération qui la
            // tient finit).
            while cache.len() > self.cfg.team_cache_entries {
                freed += cache.remove(0).1.bytes;
                metrics::counter!("ord_generator_team_evictions_total", "reason" => "count").increment(1);
            }
        }
        if freed > 0 {
            self.memory.release_team(freed);
        }
        Ok(entry)
    }

    // --- Écriture ----------------------------------------------------------------------------------------------

    /// Écrit la surcouche : fichier temporaire dans le même dossier, `fsync`, `rename`, `fsync` du dossier. Jusqu'au
    /// `rename`, l'ancienne surcouche sert ; en cas d'échec, le temporaire est effacé et rien d'autre ne change.
    /// L'appelant tient le verrou de l'équipe.
    pub fn commit(&self, c: &Credentials, meta: &FileMeta, ordo: &[u8]) -> Result<u64, StoreError> {
        let meta_json = serde_json::to_vec(meta).map_err(|_| StoreError::Io)?;
        let mut plain = Vec::with_capacity(4 + meta_json.len() + ordo.len());
        plain.extend_from_slice(&u32::try_from(meta_json.len()).map_err(|_| StoreError::Io)?.to_le_bytes());
        plain.extend_from_slice(&meta_json);
        plain.extend_from_slice(ordo);
        let sealed = c.seal(&self.graph_id, &plain).map_err(|_| StoreError::Io)?;

        let ns_dir = self.graph_dir().join(&c.ns);
        let final_path = self.file_path(c);
        let existing = std::fs::metadata(&final_path).ok();
        self.check_quota(c, &ns_dir, existing.as_ref().map(std::fs::Metadata::len), sealed.len() as u64)?;
        std::fs::create_dir_all(&ns_dir).map_err(|_| StoreError::Io)?;
        let mut random = [0u8; 8];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut random)
            .map_err(|_| StoreError::Io)?;
        let tmp = ns_dir.join(format!("{}{TMP_MARK}{}", c.team, hex8(&random)));
        let written = (|| -> std::io::Result<()> {
            let mut f = std::fs::OpenOptions::new().create_new(true).write(true).open(&tmp)?;
            f.write_all(&sealed)?;
            f.sync_all()?;
            if self.hooks.fail_commit.load(Ordering::Relaxed) {
                return Err(std::io::Error::other("injected failure"));
            }
            std::fs::rename(&tmp, &final_path)
        })();
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
            return Err(StoreError::Io);
        }
        // Le `rename` est le point de validation : la nouvelle surcouche sert déjà. Un échec du `fsync` du dossier
        // (qui protège d'une coupure) ne rend pas le dépôt « échoué » : il est journalisé, le dépôt reste réussi.
        if self.hooks.fail_dir_sync.load(Ordering::Relaxed)
            || std::fs::File::open(&ns_dir).and_then(|d| d.sync_all()).is_err()
        {
            tracing::warn!(ns8 = c.ns8(), "overlay directory fsync failed after rename");
        }
        self.forget(|k| k.0 == c.ns && k.1 == c.team);
        self.update_gauge();
        Ok(sealed.len() as u64)
    }

    fn check_quota(
        &self,
        c: &Credentials,
        ns_dir: &Path,
        replacing: Option<u64>,
        new_len: u64,
    ) -> Result<(), StoreError> {
        let graph_dir = self.graph_dir();
        if !ns_dir.exists() {
            // Une liste blanche (non secrète, tenue par l'opérateur) : un inconnu ne prend jamais une place.
            if !self.cfg.allowed_namespaces.is_empty()
                && !self.cfg.allowed_namespaces.iter().any(|p| c.ns8() == p.to_ascii_lowercase())
            {
                return Err(StoreError::Quota);
            }
            let n = std::fs::read_dir(&graph_dir).map_or(0, |rd| rd.filter_map(Result::ok).count());
            if n >= self.cfg.max_namespaces {
                return Err(StoreError::Quota);
            }
        }
        if replacing.is_none() {
            let n = std::fs::read_dir(ns_dir).map_or(0, |rd| {
                rd.filter_map(Result::ok).filter(|e| is_overlay_name(&e.file_name())).count()
            });
            if n >= self.cfg.max_overlays_per_namespace {
                return Err(StoreError::Quota);
            }
        }
        // Octets de **cet** espace seulement : la décision ne dépend jamais de ce que les autres stockent.
        let used: u64 = std::fs::read_dir(ns_dir).map_or(0, |rd| {
            rd.filter_map(Result::ok)
                .filter(|e| is_overlay_name(&e.file_name()))
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum()
        });
        let after = used.saturating_sub(replacing.unwrap_or(0)) + new_len;
        if after > self.cfg.max_namespace_bytes {
            return Err(StoreError::Quota);
        }
        Ok(())
    }

    /// Un nouvel espace ou une nouvelle équipe tiendrait-il ? Contrôle préalable, avant de lire un corps de 700 Mo.
    pub fn precheck_quota(&self, c: &Credentials) -> Result<(), StoreError> {
        let ns_dir = self.graph_dir().join(&c.ns);
        let replacing = std::fs::metadata(self.file_path(c)).ok().map(|m| m.len());
        self.check_quota(c, &ns_dir, replacing, 0)
    }

    // --- Suppression -------------------------------------------------------------------------------------------

    /// Efface la surcouche d'une équipe, sous tous les graphes ; idempotent.
    pub fn delete(&self, c: &Credentials) {
        for d in self.graph_dirs() {
            let _ = std::fs::remove_file(d.join(&c.ns).join(format!("{}.{FILE_EXT}", c.team)));
        }
        self.forget(|k| k.0 == c.ns && k.1 == c.team);
        self.update_gauge();
    }

    /// Efface tout l'espace de l'environnement, sous tous les graphes ; idempotent.
    pub fn delete_namespace(&self, c: &Credentials) {
        for d in self.graph_dirs() {
            let _ = std::fs::remove_dir_all(d.join(&c.ns));
        }
        self.forget(|k| k.0 == c.ns);
        self.update_gauge();
    }

    // --- Purge -------------------------------------------------------------------------------------------------

    /// Tous les fichiers de surcouche et temporaires : `(chemin, taille, date)`.
    fn walk_overlays(&self) -> Vec<(PathBuf, u64, SystemTime)> {
        let mut out = Vec::new();
        for g in self.graph_dirs() {
            for ns in std::fs::read_dir(&g).into_iter().flatten().filter_map(Result::ok) {
                if !ns.path().is_dir() {
                    continue;
                }
                for f in std::fs::read_dir(ns.path()).into_iter().flatten().filter_map(Result::ok) {
                    if let Ok(md) = f.metadata() {
                        if md.is_file() {
                            out.push((f.path(), md.len(), md.modified().unwrap_or(UNIX_EPOCH)));
                        }
                    }
                }
            }
        }
        out
    }

    /// Nombre de surcouches stockées (jauge `ord_overlays_stored`).
    pub fn count(&self) -> usize {
        self.walk_overlays().iter().filter(|(p, _, _)| p.file_name().is_some_and(is_overlay_name)).count()
    }

    pub fn update_gauge(&self) {
        metrics::gauge!("ord_overlays_stored").set(self.count() as f64);
    }

    /// Purge à l'instant `now` ; `startup` : les temporaires de plus de quelques minutes partent (jamais l'écriture en cours de l'autre instance).
    pub fn purge(&self, now: SystemTime, startup: bool) -> PurgeReport {
        let mut r = PurgeReport::default();
        let age = |t: SystemTime| now.duration_since(t).unwrap_or(Duration::ZERO);
        let current = self.graph_dir();
        for g in self.graph_dirs() {
            if g != current {
                // Un autre graphe : tout le dossier part une fois son dernier fichier plus vieux que la grâce.
                let files = files_under(&g);
                let newest = files.iter().map(|(_, t)| *t).max();
                let old = match newest {
                    Some(t) => age(t) > self.cfg.stale_grace,
                    None => std::fs::metadata(&g)
                        .and_then(|m| m.modified())
                        .is_ok_and(|t| age(t) > self.cfg.stale_grace),
                };
                if old && std::fs::remove_dir_all(&g).is_ok() {
                    r.stale_graph += files.iter().filter(|(p, _)| is_overlay_path(p)).count() as u64;
                    r.tmp += files.iter().filter(|(p, _)| !is_overlay_path(p)).count() as u64;
                }
                continue;
            }
            for ns in std::fs::read_dir(&g).into_iter().flatten().filter_map(Result::ok) {
                let ns_path = ns.path();
                if !ns_path.is_dir() {
                    continue;
                }
                for f in std::fs::read_dir(&ns_path).into_iter().flatten().filter_map(Result::ok) {
                    let name = f.file_name();
                    let Some(t) = f.metadata().ok().and_then(|m| m.modified().ok()) else { continue };
                    if is_tmp_name(&name) {
                        if age(t) > if startup { TMP_STARTUP_AGE } else { TMP_MAX_AGE }
                            && std::fs::remove_file(f.path()).is_ok()
                        {
                            r.tmp += 1;
                        }
                    } else if is_overlay_name(&name)
                        && age(t) > self.cfg.ttl
                        && std::fs::remove_file(f.path()).is_ok()
                    {
                        r.ttl += 1;
                    }
                }
                // `remove_dir` ne retire qu'un dossier vide.
                if std::fs::remove_dir(&ns_path).is_ok() {
                    r.empty += 1;
                }
            }
        }
        for (reason, n) in
            [("ttl", r.ttl), ("stale_graph", r.stale_graph), ("tmp", r.tmp), ("empty", r.empty)]
        {
            if n > 0 {
                metrics::counter!("ord_overlay_purged_total", "reason" => reason).increment(n);
            }
        }
        if r != PurgeReport::default() {
            tracing::info!(
                ttl = r.ttl,
                stale_graph = r.stale_graph,
                tmp = r.tmp,
                empty = r.empty,
                "overlays purged"
            );
        }
        self.update_gauge();
        r
    }
}

fn hex8(b: &[u8; 8]) -> String {
    use std::fmt::Write;
    b.iter().fold(String::new(), |mut s, x| {
        let _ = write!(s, "{x:02x}");
        s
    })
}

fn is_tmp_name(n: &std::ffi::OsStr) -> bool {
    n.to_string_lossy().contains(TMP_MARK)
}

fn is_overlay_name(n: &std::ffi::OsStr) -> bool {
    let n = n.to_string_lossy();
    n.ends_with(&format!(".{FILE_EXT}")) && !n.contains(TMP_MARK)
}

fn is_overlay_path(p: &Path) -> bool {
    p.file_name().is_some_and(is_overlay_name)
}

/// Fichiers sous un dossier `graph-*` (deux niveaux) avec leur date.
fn files_under(g: &Path) -> Vec<(PathBuf, SystemTime)> {
    let mut out = Vec::new();
    for ns in std::fs::read_dir(g).into_iter().flatten().filter_map(Result::ok) {
        for f in std::fs::read_dir(ns.path()).into_iter().flatten().filter_map(Result::ok) {
            if let Some(t) = f.metadata().ok().and_then(|m| m.modified().ok()) {
                out.push((f.path(), t));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use ord_graph::overlay::{encode, OverlayMeta};

    use super::super::crypto::{Credentials, KEY_LEN};
    use super::*;

    fn identity() -> GraphIdentity {
        GraphIdentity {
            format_major: 1,
            attr_layout: 1,
            zoom: 11,
            nodes: 10,
            edges: 20,
            directed: 40,
            id_edges_xxh3: 7,
            id_nodes_xxh3: 9,
            pbf_xxh3: 1,
            replication_timestamp: 1_700_000_000,
        }
    }

    fn store(dir: &Path, matcher: u64, max_total: u64) -> Arc<Store> {
        Store::new(
            StoreConfig {
                dir: dir.to_path_buf(),
                ttl: Duration::from_secs(14 * 86_400),
                stale_grace: Duration::from_secs(48 * 3600),
                max_namespaces: 4,
                max_overlays_per_namespace: 4,
                max_namespace_bytes: max_total,
                allowed_namespaces: Vec::new(),
                team_cache_entries: 2,
            },
            "g1".into(),
            identity(),
            matcher,
            Arc::new(TestHooks::default()),
            Arc::new(NoMemory),
        )
    }

    struct NoMemory;

    impl TeamMemory for NoMemory {
        fn hold_team(&self, _: usize) {}
        fn release_team(&self, _: usize) {}
    }

    #[test]
    fn the_least_recent_entry_nobody_holds_is_the_one_to_evict() {
        let held = Arc::new(2u8);
        // De la plus ancienne à la plus récente : `a` est tenue par une génération (deux références).
        let cache = vec![("a", Arc::clone(&held)), ("b", Arc::new(3u8)), ("c", Arc::new(4u8))];
        assert_eq!(lru_idle(&cache), Some(1), "la plus ancienne est tenue : la suivante part");
        drop(held);
        assert_eq!(lru_idle(&cache), Some(0), "libérée, elle redevient la première à partir");
        let cache = vec![("a", Arc::new(1u8)), ("b", Arc::new(3u8))];
        let all_held: Vec<_> = cache.iter().map(|(_, v)| Arc::clone(v)).collect();
        assert_eq!(lru_idle(&cache), None, "tout est tenu : rien à évincer");
        drop(all_held);
    }

    fn ordo() -> Vec<u8> {
        let meta = OverlayMeta {
            graph: identity(),
            team_id: 0,
            export_xxh3: 0,
            export_rows: 0,
            params_hash: 0,
            tool_version: "t".into(),
            build_epoch: 0,
            types: vec!["ROAD".into()],
        };
        encode(&meta, &[], &[]).unwrap()
    }

    fn meta(matcher: u64) -> FileMeta {
        FileMeta {
            deposited_at: 1_700_000_000,
            source_digest: Some("d".into()),
            matcher_hash: format!("{matcher:016x}"),
            edges: 0,
            team_km: 0.0,
            tracks: 0,
            tracks_excluded: 0,
            tracks_unreadable: 0,
        }
    }

    fn creds(n: u8, team: &str) -> Credentials {
        Credentials::derive(&[n; KEY_LEN], team)
    }

    #[test]
    fn a_namespace_quota_does_not_depend_on_what_other_namespaces_store() {
        let dir = tempfile::tempdir().unwrap();
        let one = ordo().len() as u64;
        // Un espace tient au plus deux surcouches : 2 × (taille scellée) ; les octets sont ceux du fichier chiffré.
        let probe = store(dir.path(), 1, u64::MAX);
        let sealed = probe.commit(&creds(9, "p"), &meta(1), &ordo()).unwrap();
        probe.delete_namespace(&creds(9, "p"));
        assert!(sealed > one);
        let s = store(dir.path(), 1, sealed * 2);
        // L'espace 1 se remplit jusqu'à son quota ; l'espace 2 reste libre de déposer.
        assert!(s.commit(&creds(1, "a"), &meta(1), &ordo()).is_ok());
        assert!(s.commit(&creds(1, "b"), &meta(1), &ordo()).is_ok());
        assert!(matches!(s.commit(&creds(1, "c"), &meta(1), &ordo()), Err(StoreError::Quota)));
        assert!(s.commit(&creds(2, "a"), &meta(1), &ordo()).is_ok(), "l'espace voisin n'est pas touché");
        assert!(s.commit(&creds(2, "b"), &meta(1), &ordo()).is_ok());
    }

    #[test]
    fn only_listed_namespaces_may_create_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(dir.path(), 1, u64::MAX);
        let allowed = creds(1, "a").ns8().to_ascii_uppercase();
        Arc::get_mut(&mut s).unwrap().cfg.allowed_namespaces = vec![allowed];
        assert!(s.commit(&creds(1, "a"), &meta(1), &ordo()).is_ok());
        assert!(matches!(s.commit(&creds(2, "a"), &meta(1), &ordo()), Err(StoreError::Quota)));
        assert!(matches!(s.precheck_quota(&creds(2, "a")), Err(StoreError::Quota)));
    }

    #[test]
    fn startup_purge_spares_a_fresh_temporary_file_of_a_running_write() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path(), 1, u64::MAX);
        let c = creds(1, "a");
        s.commit(&c, &meta(1), &ordo()).unwrap();
        let ns_dir = s.graph_dir().join(&c.ns);
        let tmp = ns_dir.join(format!("{}{TMP_MARK}0011223344556677", c.team));
        std::fs::write(&tmp, b"en cours").unwrap();
        s.purge(SystemTime::now(), true);
        assert!(tmp.exists(), "l'écriture de l'autre instance continue");
        s.purge(SystemTime::now() + Duration::from_secs(600), true);
        assert!(!tmp.exists(), "un reste de plus de quelques minutes part au démarrage");
    }

    #[test]
    fn another_matcher_makes_the_overlay_stale_and_the_same_one_keeps_it_ready() {
        let dir = tempfile::tempdir().unwrap();
        let c = creds(1, "t");
        let a = store(dir.path(), 0xAAAA, 1 << 20);
        a.commit(&c, &meta(0xAAAA), &ordo()).unwrap();
        assert_eq!(a.status(&c).state, State::Ready);
        // Redémarrage avec le même appariement : prête ; avec un autre : périmée, raison `matcher`.
        assert_eq!(store(dir.path(), 0xAAAA, 1 << 20).status(&c).state, State::Ready);
        let b = store(dir.path(), 0xBBBB, 1 << 20);
        assert_eq!(b.status(&c).state, State::Stale(StaleReason::Matcher));
        assert_eq!(b.status(&creds(2, "t")).state, State::Absent, "autre clé : absente");
    }

    #[test]
    fn the_total_quota_counts_the_replaced_file_once_and_failures_leave_no_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let c = creds(1, "t");
        let probe = store(dir.path(), 1, 1 << 20);
        let size = probe.commit(&c, &meta(1), &ordo()).unwrap();
        let tight = store(dir.path(), 1, size + size / 2);
        // Remplacer le même fichier tient ; une seconde équipe ne tient plus.
        tight.commit(&c, &meta(1), &ordo()).unwrap();
        assert!(matches!(tight.commit(&creds(1, "u"), &meta(1), &ordo()), Err(StoreError::Quota)));
        let names: Vec<String> =
            tight.walk_overlays().into_iter().map(|(p, _, _)| p.display().to_string()).collect();
        assert_eq!(names.len(), 1, "{names:?}");
        assert!(names.iter().all(|n| !n.contains(TMP_MARK)));
    }

    #[test]
    fn startup_purge_removes_the_temporaries_older_than_minutes_and_a_later_purge_only_the_old_ones() {
        let dir = tempfile::tempdir().unwrap();
        let c = creds(1, "t");
        let s = store(dir.path(), 1, 1 << 20);
        s.commit(&c, &meta(1), &ordo()).unwrap();
        let ns = dir.path().join("graph-g1").join(&c.ns);
        let fresh = ns.join(format!("x{TMP_MARK}1"));
        let old = ns.join(format!("y{TMP_MARK}2"));
        std::fs::write(&fresh, b"x").unwrap();
        std::fs::write(&old, b"y").unwrap();
        let two_hours_ago = SystemTime::now() - Duration::from_secs(7200);
        std::fs::File::options().write(true).open(&old).unwrap().set_modified(two_hours_ago).unwrap();
        let r = s.purge(SystemTime::now(), false);
        assert_eq!((r.tmp, fresh.exists(), old.exists()), (1, true, false));
        let r = s.purge(SystemTime::now() + Duration::from_secs(600), true);
        assert_eq!((r.tmp, fresh.exists()), (1, false));
        assert_eq!(s.status(&c).state, State::Ready, "la surcouche, elle, reste");
    }
}
