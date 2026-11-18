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
use ord_graph_match::profile::format::{decode as decode_profile, Incompatible, ProfileMeta};
use ord_loops::{Engine, Taste, TeamData};
use ord_loops_check::{Config as CheckConfig, GraphIndex, TeamCheck};

use serde::{Deserialize, Serialize};

use super::crypto::{Credentials, MAGIC_PROFILE};

pub const FILE_EXT: &str = "ordo.enc";
/// Fichier du profil de goût d'une équipe (3b), à côté de sa surcouche. Deux conditions de nommage, tenues par un test,
/// le mettent hors d'atteinte de la purge d'une 1.9.x (§14.9) : le temporaire contient `.tmp-` et ne finit pas par
/// `.ordo.enc` ; le nom définitif ne contient pas `.tmp-`.
pub const PROFILE_EXT: &str = "ordp.enc";
/// Âge au-delà duquel un profil **partiel** (1 ou 2 pas) ne suffit plus : sans profil complet depuis plus de 7 jours,
/// l'état est `stale` raison `profile` et le backend redépose (décision 6 du 17 novembre 2026).
pub const PARTIAL_PROFILE_MAX_AGE: Duration = Duration::from_secs(7 * 86_400);
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
    /// Plafonds des équipes résidentes (§20.6) : en nombre et en octets, sur les seules entrées inactives.
    pub team_cache_entries: usize,
    pub team_cache_bytes: usize,
    /// Profils de goût (3b) : `None`, ils sont désactivés (ni écrits, ni lus).
    pub profile: Option<ProfileSettings>,
}

/// Ce qui fait un profil compatible : la table commune et les réglages d'apprentissage du moteur qui lit.
#[derive(Clone, Debug)]
pub struct ProfileSettings {
    pub common: ord_graph_match::profile::learn::Common,
    pub learn: ord_graph_match::profile::learn::LearnParams,
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
    /// Distance minimale d'une boucle d'apprentissage, en millièmes de km (0 : le défaut de 5 km) : les graphes
    /// synthétiques des tests n'ont pas de boucle de 5 km. Lu à la construction du générateur.
    pub profile_min_km_milli: std::sync::atomic::AtomicU64,
    /// La phase de profil attend tant que ce drapeau est levé (une phase retenue, pour tester les places).
    pub profile_hold: AtomicBool,
    /// La phase de profil panique une fois les données de la nouvelle surcouche comptées dans le budget.
    pub profile_panic: AtomicBool,
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
    /// Pas de profil de goût utilisable (absent, ou appris autrement) : à redéposer pour l'apprendre. **Jamais rendue
    /// par `Store::ready`** : un profil absent ne bloque aucune génération, la table commune sert.
    Profile,
}

/// Le profil de goût d'une équipe, tel que le statut le décrit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileState {
    Present,
    Absent,
    Incompatible,
}

#[derive(Clone, Debug)]
pub struct ProfileInfo {
    pub state: ProfileState,
    pub learned_at: Option<i64>,
    pub steps: Option<u8>,
    pub loops: Option<u32>,
}

/// Ce que la lecture du fichier de profil rend. Une valeur par équipe et par lecture : l'en-tête, qui porte les formes de
/// l'équipe depuis le 4c, n'est pas mis en boîte.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum ProfileRead {
    Present(Box<Taste>, ProfileMeta, FileId),
    Absent,
    Incompatible(Incompatible),
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
    /// Le profil de goût, quand la surcouche existe et que les profils sont actifs.
    pub profile: Option<ProfileInfo>,
    pub meta: Option<FileMeta>,
    /// Date du fichier quand ses métadonnées ne sont pas lisibles (autre graphe).
    pub file_time: Option<i64>,
}

/// Empreinte d'un fichier : un remplacement atomique change l'inode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileId {
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
    /// Une entrée est tombée : ses octets ne comptent plus et ses agrégats par fenêtre (`serial`) partent.
    fn release_team(&self, bytes: usize, serial: u64);
}

/// Ce qui rend les octets d'une équipe **à la chute de son entrée**, quelle que soit sa sortie (remplacement par un
/// nouveau dépôt, `forget`, plafonds, purge) : tant qu'une génération tient l'entrée, elle reste comptée (§20.6).
struct TeamTicket {
    bytes: usize,
    serial: u64,
    memory: Arc<dyn TeamMemory>,
}

impl Drop for TeamTicket {
    fn drop(&mut self) {
        self.memory.release_team(self.bytes, self.serial);
    }
}

/// Une entrée du cache d'équipes, pour les plafonds.
trait Resident {
    fn resident_bytes(&self) -> usize;
}

impl Resident for Entry {
    fn resident_bytes(&self) -> usize {
        self.bytes
    }
}

/// Retire, de la moins récemment utilisée à la plus récente, les entrées **inactives** tant que le nombre ou les octets
/// dépassent leur plafond ; une entrée tenue n'est jamais retirée (le dépassement se résorbe à l'insertion suivante).
/// Rend les entrées retirées : à lâcher **hors** du verrou du cache (leur ticket prend celui du gestionnaire).
fn evict_over_caps<K, V: Resident>(
    cache: &mut Vec<(K, Arc<V>)>,
    max_entries: usize,
    max_bytes: usize,
) -> Vec<Arc<V>> {
    let mut removed = Vec::new();
    loop {
        let over_count = cache.len() > max_entries;
        let over_bytes = cache.iter().map(|(_, e)| e.resident_bytes()).sum::<usize>() > max_bytes;
        if !over_count && !over_bytes {
            break;
        }
        let Some(i) = lru_idle(cache) else { break };
        removed.push(cache.remove(i).1);
        metrics::counter!(
            "ord_generator_team_evictions_total",
            "reason" => if over_count { "count" } else { "bytes" }
        )
        .increment(1);
    }
    removed
}

/// `TeamData` et `TeamCheck` d'une surcouche, construits en parallèle (le temps est celui de la plus longue : pages
/// froides, une grosse équipe passait 8 s en série). La mémoire transitoire des **deux** constructions est réservée dans
/// le budget du générateur (`reserve`), ou la demande attend, puis 503 ; elle est rendue avant le retour : l'appelant
/// compte ce qui reste (`hold_team`). Partagée par la lecture d'une équipe et la phase de profil d'un dépôt (3b).
pub(crate) fn build_team_pair<B: AsRef<[u8]> + Sync>(
    engine: &Engine<B>,
    gi: &GraphIndex,
    overlay: &Overlay,
    reserve: Reserve<'_>,
) -> Result<(TeamData, TeamCheck), ReadyError> {
    let stale = || ReadyError::State(State::Stale(StaleReason::Graph));
    let check_cfg = CheckConfig::default();
    let plan = TeamData::plan_build(engine, overlay).map_err(|_| stale())?;
    let check_plan = TeamCheck::plan(engine.base(), overlay, &check_cfg);
    let guard = reserve(plan.estimated_bytes + check_plan.estimated_bytes)?;
    let (team, check) = std::thread::scope(|sc| {
        let check =
            sc.spawn(|| TeamCheck::build_planned(engine.base(), gi, overlay, &check_cfg, &check_plan));
        let team = TeamData::build_planned(engine, overlay, &plan);
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
    Ok((team, check))
}

/// Numéro d'une construction d'équipe : la clé de ses agrégats dans les fenêtres en cache.
pub(crate) fn next_serial() -> u64 {
    NEXT_SERIAL.fetch_add(1, Ordering::Relaxed)
}

/// Une équipe prête : surcouche et données du moteur.
pub struct Entry {
    id: FileId,
    /// Le profil de goût de l'équipe (3b) ; `None` : la table commune sert. Il suit le fichier de profil : un profil
    /// remplacé recharge l'entrée.
    pub profile: Option<Taste>,
    /// Les formes de l'équipe, de l'en-tête de son profil (lot 4, 4c) : `None` sans profil ou avec un profil écrit par 1.10
    /// (le mélange général sert jusqu'au dépôt suivant, l'état ne devient pas `stale` pour cela).
    pub shapes: Option<ord_graph_match::profile::shape::ShapeProfile>,
    profile_id: Option<FileId>,
    /// Numéro unique de cette construction : la clé de ses agrégats dans les fenêtres en cache.
    pub serial: u64,
    pub meta: FileMeta,
    pub overlay: Overlay,
    pub team: TeamData,
    /// Arêtes comblées du contrôleur indépendant, calculées une fois sur la zone de l'équipe (lot 6, §8.1).
    pub check: TeamCheck,
    /// Octets tenus : surcouche décodée, données du moteur et tables du contrôleur (capacités réelles).
    pub bytes: usize,
    /// Rend ces octets au budget quand l'entrée tombe.
    _ticket: TeamTicket,
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
    pub profile_ttl: u64,
    pub profile_orphan: u64,
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

    /// Retire du cache les entrées dont la clé satisfait `pred` ; leurs octets sont rendus à leur chute (un ticket par
    /// entrée), c'est-à-dire quand la dernière génération qui les tient a fini.
    fn forget(&self, pred: impl Fn(&CacheKey) -> bool) {
        let mut gone = Vec::new();
        {
            let mut cache = lock(&self.cache);
            let mut i = 0;
            while i < cache.len() {
                if pred(&cache[i].0) {
                    gone.push(cache.remove(i).1);
                } else {
                    i += 1;
                }
            }
        }
        // Hors du verrou : le ticket prend celui du gestionnaire.
        drop(gone);
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

    fn profile_path(&self, c: &Credentials) -> PathBuf {
        self.graph_dir().join(&c.ns).join(format!("{}.{PROFILE_EXT}", c.team))
    }

    /// Les profils sont-ils actifs (`generator.profile.enabled`) ?
    pub fn profiles_enabled(&self) -> bool {
        self.cfg.profile.is_some()
    }

    /// Lit le fichier de profil : jamais d'effacement à la lecture (un format plus récent peut venir d'une autre
    /// instance pendant un retour arrière) ; un déchiffrement qui échoue est `Incompatible(Format)`.
    pub fn read_profile(&self, c: &Credentials) -> ProfileRead {
        let Some(settings) = &self.cfg.profile else { return ProfileRead::Absent };
        let path = self.profile_path(c);
        let Ok(mut f) = std::fs::File::open(&path) else { return ProfileRead::Absent };
        let Some(id) = f.metadata().ok().map(|m| file_id(&m)) else { return ProfileRead::Absent };
        let mut raw = Vec::with_capacity(usize::try_from(id.len).unwrap_or(0));
        if f.read_to_end(&mut raw).is_err() {
            return ProfileRead::Absent;
        }
        let Ok(plain) = c.open_with(MAGIC_PROFILE, &self.graph_id, &raw) else {
            return ProfileRead::Incompatible(Incompatible::Format);
        };
        match decode_profile(&plain, &settings.common, &settings.learn) {
            Ok((meta, taste)) => ProfileRead::Present(Box::new(taste), meta, id),
            Err(e) => ProfileRead::Incompatible(e),
        }
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

    /// État de l'équipe sans construire ses données (`GET`). Les raisons `graph` et `matcher` priment sur `profile`.
    pub fn status(&self, c: &Credentials) -> Status {
        self.status_at(c, secs(SystemTime::now()))
    }

    /// `status` à une date donnée (secondes depuis l'époque) : la règle des 7 jours d'un profil partiel en dépend.
    pub fn status_at(&self, c: &Credentials, now: i64) -> Status {
        match self.read_current(c) {
            Some((_, meta, _)) if meta.matcher_hash == self.matcher_hash_hex() => {
                if !self.profiles_enabled() {
                    return Status { state: State::Ready, profile: None, meta: Some(meta), file_time: None };
                }
                let (state, info) = match self.read_profile(c) {
                    ProfileRead::Present(_, pm, _) => (
                        // Un profil partiel est servi comme un autre et l'état reste `ready`, sauf si le dernier
                        // profil complet date de plus de 7 jours : la nuit suivante le backend réessaie.
                        if self.partial_profile_is_too_old(&pm, now) {
                            State::Stale(StaleReason::Profile)
                        } else {
                            State::Ready
                        },
                        ProfileInfo {
                            state: ProfileState::Present,
                            learned_at: Some(pm.learned_at),
                            steps: Some(pm.steps),
                            loops: Some(pm.loops.iter().sum()),
                        },
                    ),
                    ProfileRead::Absent => (
                        State::Stale(StaleReason::Profile),
                        ProfileInfo {
                            state: ProfileState::Absent,
                            learned_at: None,
                            steps: None,
                            loops: None,
                        },
                    ),
                    ProfileRead::Incompatible(_) => (
                        State::Stale(StaleReason::Profile),
                        ProfileInfo {
                            state: ProfileState::Incompatible,
                            learned_at: None,
                            steps: None,
                            loops: None,
                        },
                    ),
                };
                Status { state, profile: Some(info), meta: Some(meta), file_time: None }
            }
            Some((_, meta, _)) => Status {
                state: State::Stale(StaleReason::Matcher),
                profile: None,
                meta: Some(meta),
                file_time: None,
            },
            None => match self.stale_elsewhere(c) {
                Some(t) => Status {
                    state: State::Stale(StaleReason::Graph),
                    profile: None,
                    meta: None,
                    file_time: Some(t),
                },
                None => Status { state: State::Absent, profile: None, meta: None, file_time: None },
            },
        }
    }

    fn partial_profile_is_too_old(&self, pm: &ProfileMeta, now: i64) -> bool {
        let steps = self.cfg.profile.as_ref().map_or(0, |p| p.learn.steps);
        !pm.is_complete(steps)
            && now.saturating_sub(pm.complete_since())
                > i64::try_from(PARTIAL_PROFILE_MAX_AGE.as_secs()).unwrap_or(i64::MAX)
    }

    /// L'équipe prête pour une génération : depuis le cache si le fichier n'a pas changé, sinon relue puis `TeamData`
    /// construit. `Err(State)` quand elle n'est pas prête (absente ou périmée).
    pub fn ready<B: AsRef<[u8]> + Sync>(
        &self,
        c: &Credentials,
        engine: &Engine<B>,
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

    /// Empreinte du fichier de profil d'une équipe ; `None` s'il n'existe pas.
    fn profile_file_id(&self, key: &CacheKey) -> Option<FileId> {
        let p = self.graph_dir().join(&key.0).join(format!("{}.{PROFILE_EXT}", key.1));
        std::fs::metadata(p).ok().map(|m| file_id(&m))
    }

    /// L'entrée du cache si elle correspond au fichier présent ; la marque comme la plus récemment utilisée.
    fn cached(&self, key: &CacheKey, path: &Path) -> Option<Arc<Entry>> {
        let entry = lock(&self.cache).iter().find(|(k, _)| k == key).map(|(_, e)| Arc::clone(e))?;
        let md = std::fs::metadata(path).ok()?;
        if entry.id != file_id(&md) {
            return None;
        }
        // Un profil remplacé (ou apparu, ou disparu) recharge l'entrée ; sans profils actifs, on ne regarde pas.
        if self.profiles_enabled() {
            let pid = self.profile_file_id(key);
            if entry.profile_id != pid {
                return None;
            }
        }
        let mut cache = lock(&self.cache);
        if let Some(i) = cache.iter().position(|(k, _)| k == key) {
            let item = cache.remove(i);
            cache.push(item);
        }
        Some(entry)
    }

    fn ready_build<B: AsRef<[u8]> + Sync>(
        &self,
        c: &Credentials,
        engine: &Engine<B>,
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
        // Le profil de goût, s'il y en a un d'utilisable : sinon la table commune sert (jamais une erreur ici).
        let (profile, shapes, profile_id) = match self.read_profile(c) {
            ProfileRead::Present(t, m, id) => (Some(*t), m.shapes, Some(id)),
            _ => (None, None, self.profile_file_id(&key).filter(|_| self.profiles_enabled())),
        };
        self.hooks.team_builds.fetch_add(1, Ordering::Relaxed);
        let (team, check) = build_team_pair(engine, gi, &overlay, reserve)?;
        let serial = NEXT_SERIAL.fetch_add(1, Ordering::Relaxed);
        let bytes = std::mem::size_of::<Entry>() + overlay.bytes() + team.bytes() + check.bytes();
        // Comptée d'abord (le budget libère la réserve d'espaces de travail puis des fenêtres inactives, jamais une
        // autre équipe), cachée ensuite ; le ticket rend les octets quand l'entrée tombe.
        self.memory.hold_team(bytes);
        let ticket = TeamTicket { bytes, serial, memory: Arc::clone(&self.memory) };
        let entry = Arc::new(Entry {
            id,
            profile,
            shapes,
            profile_id,
            serial,
            meta,
            overlay,
            team,
            check,
            bytes,
            _ticket: ticket,
        });
        let removed = {
            let mut cache = lock(&self.cache);
            let mut removed: Vec<Arc<Entry>> = Vec::new();
            if let Some(i) = cache.iter().position(|(k, _)| *k == key) {
                removed.push(cache.remove(i).1);
            }
            cache.push((key, Arc::clone(&entry)));
            // Plafonds en nombre et en octets : la moins récemment utilisée des entrées inactives ; l'entrée neuve est
            // tenue par nous, et une entrée tenue par une génération ne part jamais.
            removed.extend(evict_over_caps(
                &mut cache,
                self.cfg.team_cache_entries,
                self.cfg.team_cache_bytes,
            ));
            removed
        };
        drop(removed);
        Ok(entry)
    }

    // --- Écriture ----------------------------------------------------------------------------------------------

    /// Écrit la surcouche : fichier temporaire dans le même dossier, `fsync`, `rename`, `fsync` du dossier. Jusqu'au
    /// `rename`, l'ancienne surcouche sert ; en cas d'échec, le temporaire est effacé et rien d'autre ne change.
    /// L'appelant tient le verrou de l'équipe.
    pub fn commit(&self, c: &Credentials, meta: &FileMeta, ordo: &[u8]) -> Result<u64, StoreError> {
        self.commit_with_profile(c, meta, ordo, None)
    }

    /// Comme [`Store::commit`], avec le profil de goût en clair (`profile`, octets de `profile::format::encode`), scellé
    /// sous `ORDPENC1`. Ordre : le profil d'abord (temporaire, `fsync`, `rename`), puis la surcouche ; si la surcouche
    /// échoue, le nouveau profil sert avec l'ancienne surcouche et l'erreur remonte. Les octets du profil comptent dans
    /// le quota de l'espace, pas dans le nombre de surcouches. Rend les octets de la surcouche scellée.
    pub fn commit_with_profile(
        &self,
        c: &Credentials,
        meta: &FileMeta,
        ordo: &[u8],
        profile: Option<&[u8]>,
    ) -> Result<u64, StoreError> {
        let meta_json = serde_json::to_vec(meta).map_err(|_| StoreError::Io)?;
        let mut plain = Vec::with_capacity(4 + meta_json.len() + ordo.len());
        plain.extend_from_slice(&u32::try_from(meta_json.len()).map_err(|_| StoreError::Io)?.to_le_bytes());
        plain.extend_from_slice(&meta_json);
        plain.extend_from_slice(ordo);
        let sealed = c.seal(&self.graph_id, &plain).map_err(|_| StoreError::Io)?;
        let sealed_profile = match profile {
            Some(p) => Some(c.seal_with(MAGIC_PROFILE, &self.graph_id, p).map_err(|_| StoreError::Io)?),
            None => None,
        };

        let ns_dir = self.graph_dir().join(&c.ns);
        let final_path = self.file_path(c);
        let profile_path = self.profile_path(c);
        let existing = std::fs::metadata(&final_path).ok();
        let existing_profile = std::fs::metadata(&profile_path).ok().map(|m| m.len());
        // Les octets remplacés : la surcouche, et le profil seulement si un nouveau est écrit.
        let replacing_bytes = existing.as_ref().map(std::fs::Metadata::len).unwrap_or(0)
            + if sealed_profile.is_some() { existing_profile.unwrap_or(0) } else { 0 };
        let new_bytes = sealed.len() as u64 + sealed_profile.as_ref().map_or(0, |p| p.len() as u64);
        self.check_quota(
            c,
            &ns_dir,
            existing.as_ref().map(|_| replacing_bytes),
            new_bytes,
            existing.is_some(),
        )?;
        std::fs::create_dir_all(&ns_dir).map_err(|_| StoreError::Io)?;
        let mut random = [0u8; 8];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut random)
            .map_err(|_| StoreError::Io)?;
        // Le profil d'abord. Son temporaire contient `.tmp-` et ne finit pas par `.ordo.enc` : une purge d'une 1.9.x le
        // retire s'il traîne (300 s au démarrage, 1 h ensuite), jamais le définitif.
        if let Some(sp) = &sealed_profile {
            let ptmp = ns_dir.join(format!("{}.ordp{TMP_MARK}{}", c.team, hex8(&random)));
            let written = (|| -> std::io::Result<()> {
                let mut f = std::fs::OpenOptions::new().create_new(true).write(true).open(&ptmp)?;
                f.write_all(sp)?;
                f.sync_all()?;
                std::fs::rename(&ptmp, &profile_path)
            })();
            if written.is_err() {
                let _ = std::fs::remove_file(&ptmp);
                return Err(StoreError::Io);
            }
        }
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
            // Le nouveau profil reste (il est écrit) ; l'ancienne surcouche sert encore. Le cache est à rafraîchir.
            self.forget(|k| k.0 == c.ns && k.1 == c.team);
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
        has_overlay: bool,
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
        if !has_overlay {
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
                .filter(|e| is_overlay_name(&e.file_name()) || is_profile_name(&e.file_name()))
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
        let overlay = std::fs::metadata(self.file_path(c)).ok().map(|m| m.len());
        // Un dépôt remplace aussi le profil : ses octets sont libérés d'avance (contrôle conservateur).
        let profile = std::fs::metadata(self.profile_path(c)).ok().map_or(0, |m| m.len());
        self.check_quota(c, &ns_dir, overlay.map(|o| o + profile), 0, overlay.is_some())
    }

    // --- Suppression -------------------------------------------------------------------------------------------

    /// Efface la surcouche d'une équipe, sous tous les graphes ; idempotent.
    pub fn delete(&self, c: &Credentials) {
        for d in self.graph_dirs() {
            let _ = std::fs::remove_file(d.join(&c.ns).join(format!("{}.{FILE_EXT}", c.team)));
            let _ = std::fs::remove_file(d.join(&c.ns).join(format!("{}.{PROFILE_EXT}", c.team)));
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
        let files = self.walk_overlays();
        let overlays = files.iter().filter(|(p, _, _)| p.file_name().is_some_and(is_overlay_name)).count();
        let profiles = files.iter().filter(|(p, _, _)| p.file_name().is_some_and(is_profile_name)).count();
        metrics::gauge!("ord_overlays_stored").set(overlays as f64);
        metrics::gauge!("ord_profiles_stored").set(profiles as f64);
    }

    /// Nombre de profils stockés (jauge `ord_profiles_stored`).
    pub fn profile_count(&self) -> usize {
        self.walk_overlays().iter().filter(|(p, _, _)| p.file_name().is_some_and(is_profile_name)).count()
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
                    // Les profils d'un autre graphe partent avec lui (c'est voulu) ; comptés à part des temporaires.
                    r.profile_ttl += files.iter().filter(|(p, _)| is_profile_path(p)).count() as u64;
                    r.tmp += files.iter().filter(|(p, _)| !is_overlay_path(p) && !is_profile_path(p)).count()
                        as u64;
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
                    if is_profile_name(&name) {
                        // Un profil part au TTL, ou orphelin (sans surcouche voisine) depuis plus d'une heure : reste
                        // d'une suppression par une 1.9.x, ou d'un premier dépôt dont la surcouche n'a pas pu s'écrire.
                        let team =
                            name.to_string_lossy().trim_end_matches(&format!(".{PROFILE_EXT}")).to_string();
                        let orphan = !ns_path.join(format!("{team}.{FILE_EXT}")).exists();
                        if age(t) > self.cfg.ttl && std::fs::remove_file(f.path()).is_ok() {
                            r.profile_ttl += 1;
                        } else if orphan && age(t) > TMP_MAX_AGE && std::fs::remove_file(f.path()).is_ok() {
                            r.profile_orphan += 1;
                        }
                    } else if is_tmp_name(&name) {
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
        for (reason, n) in [
            ("ttl", r.ttl),
            ("stale_graph", r.stale_graph),
            ("tmp", r.tmp),
            ("empty", r.empty),
            ("profile_ttl", r.profile_ttl),
            ("profile_orphan", r.profile_orphan),
        ] {
            if n > 0 {
                metrics::counter!("ord_overlay_purged_total", "reason" => reason).increment(n);
            }
        }
        if r != PurgeReport::default() {
            tracing::info!(
                profile_ttl = r.profile_ttl,
                profile_orphan = r.profile_orphan,
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

/// Un profil de goût : finit par `.ordp.enc` et ne contient pas `.tmp-`. Ne reconnaît ni une surcouche ni un temporaire.
fn is_profile_name(n: &std::ffi::OsStr) -> bool {
    let n = n.to_string_lossy();
    n.ends_with(&format!(".{PROFILE_EXT}")) && !n.contains(TMP_MARK)
}

fn is_overlay_path(p: &Path) -> bool {
    p.file_name().is_some_and(is_overlay_name)
}

fn is_profile_path(p: &Path) -> bool {
    p.file_name().is_some_and(is_profile_name)
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
                team_cache_bytes: usize::MAX,
                profile: None,
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
        fn release_team(&self, _: usize, _: u64) {}
    }

    /// Un budget qui note ce qui entre et ce qui sort : `(octets comptés, constructions rendues)`.
    #[derive(Default)]
    struct Ledger {
        state: Mutex<(usize, Vec<(usize, u64)>)>,
    }

    impl TeamMemory for Ledger {
        fn hold_team(&self, bytes: usize) {
            lock(&self.state).0 += bytes;
        }
        fn release_team(&self, bytes: usize, serial: u64) {
            let mut s = lock(&self.state);
            s.0 -= bytes;
            s.1.push((bytes, serial));
        }
    }

    impl Ledger {
        fn counted(&self) -> usize {
            lock(&self.state).0
        }
        fn released(&self) -> Vec<(usize, u64)> {
            lock(&self.state).1.clone()
        }
    }

    /// Un magasin sur le petit moteur de test : de vraies équipes (`ready` construit leurs données), plafonds réglables.
    struct Rig {
        store: Arc<Store>,
        engine: Engine<Vec<u8>>,
        gi: GraphIndex,
        ledger: Arc<Ledger>,
        _dir: tempfile::TempDir,
    }

    impl Rig {
        fn new(entries: usize, bytes: usize) -> Self {
            let ledger = Arc::new(Ledger::default());
            Self::with_memory(entries, bytes, Arc::clone(&ledger) as Arc<dyn TeamMemory>, ledger)
        }

        fn with_memory(
            entries: usize,
            bytes: usize,
            memory: Arc<dyn TeamMemory>,
            ledger: Arc<Ledger>,
        ) -> Self {
            let engine = super::super::testkit::engine();
            let dir = tempfile::tempdir().unwrap();
            let store = Store::new(
                StoreConfig {
                    dir: dir.path().to_path_buf(),
                    ttl: Duration::from_secs(14 * 86_400),
                    stale_grace: Duration::from_secs(48 * 3600),
                    max_namespaces: 8,
                    max_overlays_per_namespace: 8,
                    max_namespace_bytes: u64::MAX,
                    allowed_namespaces: Vec::new(),
                    team_cache_entries: entries,
                    team_cache_bytes: bytes,
                    profile: None,
                },
                "g1".into(),
                engine.base().graph_identity(),
                1,
                Arc::new(TestHooks::default()),
                memory,
            );
            let gi = GraphIndex::build(engine.base());
            Self { store, engine, gi, ledger, _dir: dir }
        }

        /// Dépose une surcouche de quelques arêtes pour l'équipe `n`.
        fn deposit(&self, n: u8) -> Credentials {
            let c = creds(n, "t");
            self.redeposit(&c);
            c
        }

        fn redeposit(&self, c: &Credentials) {
            let meta = OverlayMeta {
                graph: self.engine.base().graph_identity(),
                team_id: 0,
                export_xxh3: 0,
                export_rows: 0,
                params_hash: 0,
                tool_version: "t".into(),
                build_epoch: 0,
                types: vec!["ROAD".into(), "GRAVEL".into(), "MTB".into(), "MIXED".into()],
            };
            let edges: Vec<u32> = (0..40).collect();
            let counts: Vec<u16> = edges.iter().flat_map(|_| [1u16, 1, 0, 0, 0, 0, 0, 0]).collect();
            let ordo = encode(&meta, &edges, &counts).unwrap();
            self.store.commit(c, &self::meta(1), &ordo).unwrap();
        }

        fn ready(&self, c: &Credentials) -> Arc<Entry> {
            let reserve: Reserve<'_> = &|_| Ok(Box::new(()) as Box<dyn Send>);
            self.store.ready(c, &self.engine, &self.gi, reserve).expect("équipe prête")
        }
    }

    #[test]
    fn resident_teams_are_capped_in_number_on_idle_entries_only_and_their_bytes_come_back_on_drop() {
        let r = Rig::new(2, usize::MAX);
        let (a, b, c, d) = (r.deposit(1), r.deposit(2), r.deposit(3), r.deposit(4));
        let held = r.ready(&a);
        let (sa, bytes) = (held.serial, held.bytes);
        drop(r.ready(&b));
        assert_eq!(r.store.cached_bytes(), 2 * bytes, "deux équipes résidentes de même taille");
        // Une troisième : `a` est tenue, `b` (inactive) est la moins récente des inactives : elle part.
        drop(r.ready(&c));
        assert_eq!(lock(&r.store.cache).len(), 2);
        assert_eq!(r.ledger.released().len(), 1, "une entrée tombée, ses octets rendus");
        assert_ne!(r.ledger.released()[0].1, sa, "la tenue est restée");
        assert_eq!(r.ledger.counted(), 2 * bytes);
        // `a` rendue : à l'insertion suivante, elle est la plus ancienne des inactives et part, avec son numéro.
        drop(held);
        drop(r.ready(&d));
        assert_eq!(lock(&r.store.cache).len(), 2);
        assert!(r.ledger.released().iter().any(|&(n, s)| s == sa && n == bytes));
        assert_eq!(r.ledger.counted(), 2 * bytes);
        assert_eq!(r.store.cached_bytes(), r.ledger.counted(), "le compte est exact");
    }

    #[test]
    fn the_byte_cap_evicts_the_least_recent_idle_team_and_never_a_held_one() {
        let probe = Rig::new(8, usize::MAX);
        let bytes = probe.ready(&probe.deposit(1)).bytes;
        // Le plafond tient une équipe et demie : la seconde chasse la première, jamais la tenue.
        let r = Rig::new(8, bytes + bytes / 2);
        let (a, b, c) = (r.deposit(1), r.deposit(2), r.deposit(3));
        let held = r.ready(&a);
        drop(r.ready(&b));
        assert_eq!(lock(&r.store.cache).len(), 2, "la tenue reste : le plafond est dépassé un moment");
        assert!(r.ledger.released().is_empty() || r.ledger.released().iter().all(|x| x.1 != held.serial));
        drop(r.ready(&c));
        assert!(lock(&r.store.cache).iter().any(|(_, e)| Arc::ptr_eq(e, &held)), "la tenue est toujours là");
        drop(held);
        drop(r.ready(&b));
        assert_eq!(lock(&r.store.cache).len(), 1, "une seule équipe tient sous le plafond d'octets");
        assert_eq!(r.store.cached_bytes(), r.ledger.counted());
    }

    /// Le constat A, de bout en bout : la réserve d'espaces de travail est pleine, le budget n'a pas la place de la
    /// construction d'une équipe **en plus** de cette réserve ; la construction réussit (la réserve est évincée), elle
    /// ne répondait 503 pour toujours.
    #[test]
    fn a_team_build_succeeds_when_the_workspace_pool_is_full_and_the_budget_has_no_room_for_both() {
        use super::super::testkit::request;
        use super::super::windows::WindowManager;
        // Ce que la construction réserve : relevé sur une première, sans contrainte.
        let probe = Rig::new(4, usize::MAX);
        let wanted = std::cell::Cell::new(0usize);
        let reserve: Reserve<'_> = &|b| {
            wanted.set(b);
            Ok(Box::new(()) as Box<dyn Send>)
        };
        probe.store.ready(&probe.deposit(1), &probe.engine, &probe.gi, reserve).unwrap();
        let r = wanted.get();
        assert!(r > 1_000_000, "la construction réserve de la mémoire : {r}");

        let ledger = Arc::new(Ledger::default());
        // Le gestionnaire sert de budget aux équipes ; son budget est celui de la construction et d'une moitié de réserve.
        let windows = Arc::new(WindowManager::new(r + 600_000));
        let rig = Rig::with_memory(4, usize::MAX, Arc::clone(&windows) as Arc<dyn TeamMemory>, ledger);
        let (req, now) = (request(500.0, 500.0), std::time::Instant::now());
        let plan = rig.engine.plan(&req, 0.0);
        let held =
            windows.acquire(&rig.engine, &plan, None, now + Duration::from_secs(5), &mut || false).unwrap();
        let mut spaces: Vec<ord_loops::Workspace> = (0..6).map(|_| windows.take_workspace()).collect();
        for ws in &mut spaces {
            ord_loops::generate_in(&rig.engine, &held.win, None, &req, ws, &mut || false).unwrap();
        }
        windows.release(held);
        for ws in spaces {
            windows.give_workspace(ws, usize::MAX, 6);
        }
        let pool = windows.pool_bytes();
        assert!(pool > 600_000 / 2, "la réserve est pleine : {pool}");
        // La construction passe par le même gestionnaire que le service.
        let c = rig.deposit(1);
        let reserve: Reserve<'_> = &|b| {
            windows
                .reserve_build(b, now + Duration::from_secs(2), &|| false)
                .map(|g| Box::new(g) as Box<dyn Send + '_>)
                .map_err(|_| ReadyError::Overloaded)
        };
        let t = std::time::Instant::now();
        let entry = rig.store.ready(&c, &rig.engine, &rig.gi, reserve).expect("la construction réussit");
        assert!(t.elapsed() < Duration::from_secs(1), "elle n'a pas attendu l'échéance");
        assert_eq!(windows.pool_bytes(), 0, "la réserve a été évincée");
        assert!(entry.bytes > 0);
    }

    #[test]
    fn a_replaced_or_forgotten_team_gives_its_bytes_back_when_its_last_holder_lets_go() {
        let r = Rig::new(8, usize::MAX);
        let a = r.deposit(1);
        let held = r.ready(&a);
        let (bytes, serial) = (held.bytes, held.serial);
        // Un nouveau dépôt remplace la surcouche : l'entrée sort du cache mais une génération la tient encore.
        r.redeposit(&a);
        assert_eq!(lock(&r.store.cache).len(), 0);
        assert_eq!(r.ledger.counted(), bytes, "toujours comptée tant qu'elle est tenue");
        assert!(r.ledger.released().is_empty());
        drop(held);
        assert_eq!(r.ledger.released(), vec![(bytes, serial)]);
        assert_eq!(r.ledger.counted(), 0);
        // Un fichier retiré (`forget`) : même chose, sans personne qui la tienne.
        let again = r.ready(&a);
        let serial2 = again.serial;
        drop(again);
        r.store.forget(|k| k.1 == a.team);
        assert_eq!(r.ledger.released().last().map(|x| x.1), Some(serial2));
        assert_eq!(r.ledger.counted(), 0);
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

    // --- Profils de goût (3b) ---------------------------------------------------------------------------------------

    use ord_graph_match::profile::format::{encode as encode_profile, hex16, ProfileMeta, FORMAT};
    use ord_graph_match::profile::learn::{Common, LearnParams};

    fn settings() -> ProfileSettings {
        let learn = LearnParams::default();
        ProfileSettings { common: Common::engine(learn.normalize), learn }
    }

    fn store_with_profiles(dir: &Path, matcher: u64) -> Arc<Store> {
        let s = store(dir, matcher, 1 << 20);
        // Le même magasin, profils actifs : on le reconstruit avec la configuration voulue.
        drop(s);
        Store::new(
            StoreConfig {
                dir: dir.to_path_buf(),
                ttl: Duration::from_secs(14 * 86_400),
                stale_grace: Duration::from_secs(48 * 3600),
                max_namespaces: 4,
                max_overlays_per_namespace: 4,
                max_namespace_bytes: 1 << 20,
                allowed_namespaces: Vec::new(),
                team_cache_entries: 2,
                team_cache_bytes: usize::MAX,
                profile: Some(settings()),
            },
            "g1".into(),
            identity(),
            matcher,
            Arc::new(TestHooks::default()),
            Arc::new(NoMemory),
        )
    }

    fn profile_bytes(tweak: impl FnOnce(&mut ProfileMeta, &mut ord_loops::Taste)) -> Vec<u8> {
        let st = settings();
        let mut meta = ProfileMeta {
            format: FORMAT,
            learned_at: 1_700_000_100,
            overlay_deposited_at: 1_700_000_000,
            common: hex16(st.common.hash_for(st.learn.mix)),
            learn: hex16(st.learn.hash()),
            engine: "1.10.0".into(),
            steps: 3,
            loops: [4, 2, 0],
            pairs: [8, 4, 0],
            normalized: st.learn.normalize,
            complete_at: None,
            shapes: None,
        };
        let mut taste = ord_loops::Taste::neutral();
        taste.q8[0][3] = 400;
        tweak(&mut meta, &mut taste);
        encode_profile(&meta, &taste)
    }

    fn profile_file(dir: &Path, c: &Credentials) -> PathBuf {
        dir.join("graph-g1").join(&c.ns).join(format!("{}.{PROFILE_EXT}", c.team))
    }

    fn overlay_file(dir: &Path, c: &Credentials) -> PathBuf {
        dir.join("graph-g1").join(&c.ns).join(format!("{}.{FILE_EXT}", c.team))
    }

    #[test]
    fn a_committed_profile_is_read_back_and_makes_the_state_ready() {
        let dir = tempfile::tempdir().unwrap();
        let (c, s) = (creds(1, "t"), store_with_profiles(dir.path(), 1));
        s.commit_with_profile(&c, &meta(1), &ordo(), Some(&profile_bytes(|_, _| {}))).unwrap();
        match s.read_profile(&c) {
            ProfileRead::Present(t, m, _) => {
                assert_eq!((t.q8[0][3], t.q8[1][3], m.steps, m.loops), (400, 256, 3, [4, 2, 0]));
            }
            other => panic!("{other:?}"),
        }
        let st = s.status(&c);
        assert_eq!(st.state, State::Ready);
        let p = st.profile.unwrap();
        assert_eq!((p.state, p.steps, p.loops), (ProfileState::Present, Some(3), Some(6)));
    }

    #[test]
    fn a_partial_profile_is_ready_unless_the_last_complete_one_is_older_than_seven_days() {
        let dir = tempfile::tempdir().unwrap();
        let (c, s) = (creds(1, "t"), store_with_profiles(dir.path(), 1));
        let now = 1_700_000_000_i64 + 100 * 86_400;
        let day = 86_400_i64;
        let state_at = |s: &Store, now: i64| s.status_at(&c, now);
        // Complet : prêt, quel que soit son âge (rien ne dit qu'il faut le refaire avant le changement des traces).
        s.commit_with_profile(
            &c,
            &meta(1),
            &ordo(),
            Some(&profile_bytes(|m, _| m.learned_at = now - 30 * day)),
        )
        .unwrap();
        assert_eq!(state_at(&s, now).state, State::Ready);
        // Partiel, récent (le complet remonte à 1 jour) : prêt, et le statut dit les pas faits.
        let recent = profile_bytes(|m, _| {
            m.steps = 2;
            m.learned_at = now;
            m.complete_at = Some(now - day);
        });
        s.commit_with_profile(&c, &meta(1), &ordo(), Some(&recent)).unwrap();
        let st = state_at(&s, now);
        assert_eq!(st.state, State::Ready);
        assert_eq!(st.profile.unwrap().steps, Some(2));
        // Partiel dont le dernier complet a 8 jours : `stale` raison `profile` ; le profil, lui, est bien là.
        let old = profile_bytes(|m, _| {
            m.steps = 1;
            m.learned_at = now;
            m.complete_at = Some(now - 8 * day);
        });
        s.commit_with_profile(&c, &meta(1), &ordo(), Some(&old)).unwrap();
        let st = state_at(&s, now);
        assert_eq!(st.state, State::Stale(StaleReason::Profile));
        assert_eq!(st.profile.unwrap().state, ProfileState::Present);
        // Exactement 7 jours : encore prêt (« plus de 7 jours »).
        let edge = profile_bytes(|m, _| {
            m.steps = 1;
            m.learned_at = now;
            m.complete_at = Some(now - 7 * day);
        });
        s.commit_with_profile(&c, &meta(1), &ordo(), Some(&edge)).unwrap();
        assert_eq!(state_at(&s, now).state, State::Ready);
        // Écrit sans `complete_at` (date inconnue) : celle de l'apprentissage, donc récent.
        let none = profile_bytes(|m, _| {
            m.steps = 2;
            m.learned_at = now - day;
        });
        s.commit_with_profile(&c, &meta(1), &ordo(), Some(&none)).unwrap();
        assert_eq!(state_at(&s, now).state, State::Ready);
        // Une surcouche sans boucle (ensemble vide, `steps = 0`) est complète.
        let empty = profile_bytes(|m, _| {
            m.steps = 0;
            m.loops = [0; 3];
            m.pairs = [0; 3];
            m.learned_at = now - 20 * day;
        });
        s.commit_with_profile(&c, &meta(1), &ordo(), Some(&empty)).unwrap();
        assert_eq!(state_at(&s, now).state, State::Ready);
    }

    #[test]
    fn an_absent_or_incompatible_profile_is_stale_with_the_reason_profile_and_the_file_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let (c, s) = (creds(1, "t"), store_with_profiles(dir.path(), 1));
        // Surcouche seule (le dépôt d'une 1.9.x, ou le premier démarrage de 1.10) : `stale` raison `profile`.
        s.commit(&c, &meta(1), &ordo()).unwrap();
        let st = s.status(&c);
        assert_eq!(st.state, State::Stale(StaleReason::Profile));
        assert_eq!(st.profile.unwrap().state, ProfileState::Absent);
        // Un format plus récent : `incompatible`, le fichier est gardé.
        let newer = profile_bytes(|m, _| m.format = 2);
        s.commit_with_profile(&c, &meta(1), &ordo(), Some(&newer)).unwrap();
        let st = s.status(&c);
        assert_eq!(st.state, State::Stale(StaleReason::Profile));
        assert_eq!(st.profile.unwrap().state, ProfileState::Incompatible);
        assert!(profile_file(dir.path(), &c).exists(), "jamais effacé à la lecture");
        // D'autres réglages d'apprentissage : lisible, mais appris autrement.
        let other = profile_bytes(|m, _| m.learn = hex16(1));
        s.commit_with_profile(&c, &meta(1), &ordo(), Some(&other)).unwrap();
        assert!(matches!(s.read_profile(&c), ProfileRead::Incompatible(Incompatible::Settings)));
        assert!(profile_file(dir.path(), &c).exists());
        // Un fichier qui ne se déchiffre pas : `Format`, gardé.
        std::fs::write(profile_file(dir.path(), &c), b"not a profile").unwrap();
        assert!(matches!(s.read_profile(&c), ProfileRead::Incompatible(Incompatible::Format)));
        assert!(profile_file(dir.path(), &c).exists());
    }

    #[test]
    fn disabled_profiles_leave_the_state_ready_and_another_matcher_or_graph_outranks_the_profile() {
        let dir = tempfile::tempdir().unwrap();
        let c = creds(1, "t");
        let off = store(dir.path(), 1, 1 << 20);
        off.commit(&c, &meta(1), &ordo()).unwrap();
        assert_eq!(off.status(&c).state, State::Ready);
        assert!(off.status(&c).profile.is_none());
        assert!(matches!(off.read_profile(&c), ProfileRead::Absent));
        // Un autre appariement : `matcher` prime sur l'absence de profil.
        let on = store_with_profiles(dir.path(), 2);
        assert_eq!(on.status(&c).state, State::Stale(StaleReason::Matcher));
        // Une surcouche d'un autre graphe, avec son profil : `graph`.
        let other = dir.path().join("graph-old").join(&c.ns);
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join(format!("{}.{FILE_EXT}", c.team)), b"x").unwrap();
        std::fs::write(other.join(format!("{}.{PROFILE_EXT}", c.team)), b"x").unwrap();
        let fresh = tempfile::tempdir().unwrap();
        let on2 = store_with_profiles(fresh.path(), 1);
        std::fs::create_dir_all(fresh.path().join("graph-old").join(&c.ns)).unwrap();
        std::fs::write(
            fresh.path().join("graph-old").join(&c.ns).join(format!("{}.{FILE_EXT}", c.team)),
            b"x",
        )
        .unwrap();
        assert_eq!(on2.status(&c).state, State::Stale(StaleReason::Graph));
    }

    #[test]
    fn a_failed_overlay_write_keeps_the_new_profile_and_the_old_overlay() {
        let dir = tempfile::tempdir().unwrap();
        let hooks = Arc::new(TestHooks::default());
        let s = Store::new(
            StoreConfig {
                dir: dir.path().to_path_buf(),
                ttl: Duration::from_secs(14 * 86_400),
                stale_grace: Duration::from_secs(48 * 3600),
                max_namespaces: 4,
                max_overlays_per_namespace: 4,
                max_namespace_bytes: 1 << 20,
                allowed_namespaces: Vec::new(),
                team_cache_entries: 2,
                team_cache_bytes: usize::MAX,
                profile: Some(settings()),
            },
            "g1".into(),
            identity(),
            1,
            Arc::clone(&hooks),
            Arc::new(NoMemory),
        );
        let c = creds(1, "t");
        s.commit_with_profile(&c, &meta(1), &ordo(), Some(&profile_bytes(|_, _| {}))).unwrap();
        let old_overlay = std::fs::read(overlay_file(dir.path(), &c)).unwrap();
        hooks.fail_commit.store(true, Ordering::Relaxed);
        let r = s.commit_with_profile(&c, &meta(1), &ordo(), Some(&profile_bytes(|_, t| t.q8[0][3] = 300)));
        assert!(matches!(r, Err(StoreError::Io)));
        assert_eq!(std::fs::read(overlay_file(dir.path(), &c)).unwrap(), old_overlay);
        match s.read_profile(&c) {
            ProfileRead::Present(t, _, _) => {
                assert_eq!(t.q8[0][3], 300, "le profil neuf est écrit avant la surcouche")
            }
            other => panic!("{other:?}"),
        }
        let ns = dir.path().join("graph-g1").join(&c.ns);
        assert!(
            std::fs::read_dir(ns).unwrap().filter_map(Result::ok).all(|e| !is_tmp_name(&e.file_name())),
            "aucun temporaire ne reste"
        );
    }

    #[test]
    fn profile_purge_follows_the_ttl_and_removes_orphans_after_an_hour_only() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (creds(1, "a"), creds(1, "b"));
        let s = store_with_profiles(dir.path(), 1);
        for c in [&a, &b] {
            s.commit_with_profile(c, &meta(1), &ordo(), Some(&profile_bytes(|_, _| {}))).unwrap();
        }
        let age = |p: &Path, secs: u64| {
            let t = SystemTime::now() - Duration::from_secs(secs);
            std::fs::File::options().write(true).open(p).unwrap().set_modified(t).unwrap();
        };
        // Orphelin récent (30 min) : gardé ; orphelin d'une heure et plus : retiré.
        std::fs::remove_file(overlay_file(dir.path(), &a)).unwrap();
        age(&profile_file(dir.path(), &a), 1800);
        let r = s.purge(SystemTime::now(), false);
        assert_eq!((r.profile_orphan, r.profile_ttl), (0, 0));
        assert!(profile_file(dir.path(), &a).exists());
        age(&profile_file(dir.path(), &a), 7200);
        let r = s.purge(SystemTime::now(), false);
        assert_eq!(r.profile_orphan, 1);
        assert!(!profile_file(dir.path(), &a).exists());
        // Au TTL : surcouche et profil partent chacun pour sa raison.
        age(&overlay_file(dir.path(), &b), 15 * 86_400);
        age(&profile_file(dir.path(), &b), 15 * 86_400);
        let r = s.purge(SystemTime::now(), false);
        assert_eq!((r.ttl, r.profile_ttl), (1, 1));
        // Un temporaire de profil part à son âge, comme les autres.
        let ns = dir.path().join("graph-g1").join(&b.ns);
        std::fs::create_dir_all(&ns).unwrap();
        let tmp = ns.join(format!("x.ordp{TMP_MARK}ab"));
        std::fs::write(&tmp, b"x").unwrap();
        age(&tmp, 7200);
        assert_eq!(s.purge(SystemTime::now(), false).tmp, 1);
        assert!(!tmp.exists());
    }

    /// Les deux règles de purge d'une 1.9.2, recopiées telles quelles (`develop`) : aucune ne reconnaît un profil, et
    /// la seconde ne reconnaît pas son temporaire. C'est ce qui rend le déploiement roulant sûr (§14.9).
    #[test]
    fn the_192_purge_rules_never_touch_a_profile_file() {
        fn is_tmp_name_192(n: &str) -> bool {
            n.contains(".tmp-")
        }
        fn is_overlay_name_192(n: &str) -> bool {
            n.ends_with(".ordo.enc") && !n.contains(".tmp-")
        }
        let team = "a".repeat(64);
        let profile = format!("{team}.{PROFILE_EXT}");
        let profile_tmp = format!("{team}.ordp{TMP_MARK}0123456789abcdef");
        assert!(
            !is_tmp_name_192(&profile) && !is_overlay_name_192(&profile),
            "le profil définitif est hors d'atteinte"
        );
        assert!(
            is_tmp_name_192(&profile_tmp) && !is_overlay_name_192(&profile_tmp),
            "son temporaire est ramassé, jamais pris pour une surcouche"
        );
        // Les noms de 1.10 : le profil est un profil, le temporaire n'en est pas un, aucun n'est une surcouche.
        assert!(is_profile_name(std::ffi::OsStr::new(&profile)));
        assert!(!is_profile_name(std::ffi::OsStr::new(&profile_tmp)));
        assert!(!is_overlay_name(std::ffi::OsStr::new(&profile)));
    }

    #[test]
    fn delete_removes_both_files_and_the_quota_counts_the_profile() {
        let dir = tempfile::tempdir().unwrap();
        let (c, s) = (creds(1, "t"), store_with_profiles(dir.path(), 1));
        let one = ordo().len() as u64;
        s.commit_with_profile(&c, &meta(1), &ordo(), Some(&profile_bytes(|_, _| {}))).unwrap();
        assert_eq!(s.profile_count(), 1);
        assert_eq!(s.count(), 1, "le nombre de surcouches ne compte pas les profils");
        s.delete(&c);
        assert!(!profile_file(dir.path(), &c).exists() && !overlay_file(dir.path(), &c).exists());
        // Le quota d'un espace compte les octets du profil : une surcouche seule tient, avec son profil non.
        let tight = tempfile::tempdir().unwrap();
        let t = Store::new(
            StoreConfig {
                dir: tight.path().to_path_buf(),
                ttl: Duration::from_secs(14 * 86_400),
                stale_grace: Duration::from_secs(48 * 3600),
                max_namespaces: 4,
                max_overlays_per_namespace: 4,
                max_namespace_bytes: one + 300,
                allowed_namespaces: Vec::new(),
                team_cache_entries: 2,
                team_cache_bytes: usize::MAX,
                profile: Some(settings()),
            },
            "g1".into(),
            identity(),
            1,
            Arc::new(TestHooks::default()),
            Arc::new(NoMemory),
        );
        assert!(t.commit(&c, &meta(1), &ordo()).is_ok());
        let with = t.commit_with_profile(&creds(1, "u"), &meta(1), &ordo(), Some(&profile_bytes(|_, _| {})));
        assert!(matches!(with, Err(StoreError::Quota)), "{with:?}");
    }
}
