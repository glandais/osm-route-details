//! Les fenêtres du moteur (note lot 6, §10) : cache sous **un budget d'octets unique**, une construction par clé,
//! attente de la place quand elle manque, 503 à l'échéance.
//!
//! Le calcul d'une génération reste du CPU pur sous le sémaphore `max_concurrent` ; la construction d'une fenêtre se
//! fait dans la même tâche bloquante, dans le budget de temps de la requête. Le cache ne change jamais un résultat :
//! une fenêtre qui contient les tuiles nécessaires rend le même parcours, quelle qu'elle soit.
//!
//! **Aucune tuile, aucun centre, aucun rayon de classe n'est journalisé ni étiqueté** (une tuile z11 dit à 14 km
//! près d'où part une équipe) : les métriques ne portent que la vue, des tailles et des octets.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError, Weak};
use std::time::{Duration, Instant};

use ord_loops::{CacheStats, Engine, TeamData, Window, WindowCache, WindowPlan, WindowTeam, Workspace};

/// Octets d'espace de travail par chaîne de fenêtre : la réservation d'une génération. La note du lot 6 comptait 118 o
/// (§9) ; la sonde `window_probe` relève 142 à 146 o (les listes `settled` et `reached` grossissent), d'où 150.
pub const WORKSPACE_BYTES_PER_CHAIN: usize = 150;

/// Pourquoi une fenêtre n'est pas rendue.
#[derive(Debug, PartialEq, Eq)]
pub enum AcquireError {
    /// Coupé par l'appelant (client parti, budget de temps).
    Stopped,
    /// Le budget de mémoire n'a pas libéré de place avant l'échéance : 503 `overloaded`.
    Overloaded,
}

/// Les données d'équipe résidentes, que le gestionnaire évince quand la place manque (le stockage les tient, le
/// gestionnaire les compte dans le budget unique). Appelé **sous le verrou du gestionnaire** : il ne doit jamais
/// rappeler celui-ci.
pub trait TeamEvictor: Send + Sync {
    /// Retire la moins récemment utilisée des équipes que personne ne tient ; rend `(octets libérés, numéro de sa
    /// construction)`, `None` s'il n'y en a pas.
    fn evict_idle(&self) -> Option<(usize, u64)>;
}

struct Inner {
    cache: WindowCache,
    /// Constructions en cours : (vue, tuiles de la classe).
    building: Vec<(usize, Vec<u32>)>,
    published: CacheStats,
    /// Réserve d'espaces de travail (§9) : leurs octets comptent dans le budget (réservés) et s'évincent **en
    /// premier**, avant toute fenêtre (finition 2, §20.5).
    pool: Vec<Workspace>,
}

impl Inner {
    fn pool_bytes(&self) -> usize {
        self.pool.iter().map(Workspace::bytes).sum()
    }
}

/// Ce qu'une génération tient : la fenêtre, les agrégats de l'équipe, et la réservation de son espace de travail.
pub struct Held {
    pub win: Arc<Window>,
    pub wt: Option<Arc<WindowTeam>>,
    reserved: usize,
}

/// Place réservée à une construction d'équipe ; rendue à la chute.
pub struct BuildGuard<'a> {
    mgr: &'a WindowManager,
    bytes: usize,
}

impl Drop for BuildGuard<'_> {
    fn drop(&mut self) {
        metrics::gauge!("ord_generator_memory_bytes", "kind" => "team_build").decrement(self.bytes as f64);
        lock(&self.mgr.inner).cache.release(self.bytes);
        self.mgr.cv.notify_all();
    }
}

impl super::store::TeamMemory for WindowManager {
    fn hold_team(&self, bytes: usize) {
        WindowManager::hold_team(self, bytes);
    }

    fn release_team(&self, bytes: usize) {
        WindowManager::release_team(self, bytes);
    }
}

pub struct WindowManager {
    inner: Mutex<Inner>,
    cv: Condvar,
    /// Fenêtres construites depuis le démarrage (une par clé, même sous concurrence).
    built: std::sync::atomic::AtomicU64,
    evictor: OnceLock<Weak<dyn TeamEvictor>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn view_name(vi: usize) -> &'static str {
    if vi == 0 {
        "road"
    } else {
        "gravel"
    }
}

impl WindowManager {
    pub fn new(budget_bytes: usize) -> Self {
        metrics::gauge!("ord_generator_memory_budget_bytes").set(budget_bytes as f64);
        Self {
            inner: Mutex::new(Inner {
                cache: WindowCache::new(budget_bytes),
                building: Vec::new(),
                published: CacheStats::default(),
                pool: Vec::new(),
            }),
            cv: Condvar::new(),
            built: std::sync::atomic::AtomicU64::new(0),
            evictor: OnceLock::new(),
        }
    }

    /// Branche l'éviction des équipes résidentes (une seule fois, au chargement).
    pub fn set_evictor(&self, evictor: Weak<dyn TeamEvictor>) {
        let _ = self.evictor.set(evictor);
    }

    /// Évince une équipe que personne ne tient ; vrai si de la place est revenue.
    fn evict_team(&self, g: &mut Inner) -> bool {
        let Some((bytes, serial)) = self.evictor.get().and_then(Weak::upgrade).and_then(|e| e.evict_idle())
        else {
            return false;
        };
        g.cache.release_external(bytes);
        // Ses agrégats par fenêtre ne servent plus à personne : la prochaine construction aura un autre numéro.
        g.cache.drop_team(serial);
        metrics::counter!("ord_generator_team_evictions_total", "reason" => "budget").increment(1);
        true
    }

    /// Libère la réserve d'espaces de travail, du plus gros au plus petit : se réallouer coûte quelques ms.
    fn evict_pool(g: &mut Inner) {
        g.pool.sort_by_key(|w| std::cmp::Reverse(w.bytes()));
        let n = g.pool.len() as u64;
        let bytes = g.pool_bytes();
        g.pool.clear();
        g.cache.release(bytes);
        metrics::counter!("ord_generator_window_evictions_total", "kind" => "workspace").increment(n);
    }

    /// Réserve `bytes`, en libérant dans cet ordre : **1. la réserve d'espaces de travail** (en entier), **2. les
    /// fenêtres que personne ne tient** (la moins récemment servie d'abord) ; rien d'autre n'est évincé (§20.5).
    fn reserve_in(&self, g: &mut Inner, bytes: usize) -> bool {
        loop {
            if !g.cache.fits(bytes) && !g.pool.is_empty() {
                Self::evict_pool(g);
            }
            if g.cache.reserve(bytes) {
                return true;
            }
            if !self.evict_team(g) {
                return false;
            }
        }
    }

    /// Ramène le total sous le budget autant que possible : octets déjà alloués, comptés après coup.
    fn settle(&self, g: &mut Inner) {
        while !g.cache.make_room(0) {
            if !self.evict_team(g) {
                return;
            }
        }
    }

    #[cfg(test)]
    pub fn budget(&self) -> usize {
        lock(&self.inner).cache.budget()
    }

    /// Fenêtres construites depuis le démarrage.
    #[cfg(test)]
    pub fn built(&self) -> u64 {
        self.built.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Octets comptés (fenêtres, agrégats, réservations).
    #[cfg(test)]
    pub fn counted(&self) -> usize {
        lock(&self.inner).cache.total()
    }

    fn publish(&self, g: &mut Inner) {
        let (w, t) = g.cache.split_bytes();
        metrics::gauge!("ord_generator_memory_bytes", "kind" => "team_cache").set(g.cache.external() as f64);
        metrics::gauge!("ord_generator_memory_bytes", "kind" => "window").set(w as f64);
        metrics::gauge!("ord_generator_memory_bytes", "kind" => "window_team").set(t as f64);
        metrics::gauge!("ord_generator_memory_bytes", "kind" => "workspace").set(g.pool_bytes() as f64);
        let s = g.cache.stats();
        let d = |a: u64, b: u64| a.saturating_sub(b);
        metrics::counter!("ord_generator_window_evictions_total", "kind" => "window")
            .increment(d(s.evicted_windows, g.published.evicted_windows));
        metrics::counter!("ord_generator_window_evictions_total", "kind" => "window_team")
            .increment(d(s.evicted_teams, g.published.evicted_teams));
        g.published = s;
    }

    /// Attend un signal (une construction finie, une place rendue) au plus jusqu'à `deadline` ; `Err` si l'échéance
    /// passe (`Overloaded`) ou si l'appelant coupe (`Stopped`).
    fn wait<'a>(
        &self,
        g: MutexGuard<'a, Inner>,
        deadline: Instant,
        stop: &mut dyn FnMut() -> bool,
        waited_for_memory: bool,
    ) -> Result<MutexGuard<'a, Inner>, AcquireError> {
        let now = Instant::now();
        if now >= deadline {
            return Err(if waited_for_memory { AcquireError::Overloaded } else { AcquireError::Stopped });
        }
        if stop() {
            return Err(AcquireError::Stopped);
        }
        let slice = (deadline - now).min(Duration::from_millis(50));
        let (g, _) = self.cv.wait_timeout(g, slice).unwrap_or_else(PoisonError::into_inner);
        Ok(g)
    }

    /// La fenêtre de la requête (du cache, attendue, ou construite), les agrégats de l'équipe dans cette fenêtre, et
    /// la place de l'espace de travail. Bloquant.
    pub fn acquire<B: AsRef<[u8]>>(
        &self,
        engine: &Engine<B>,
        plan: &WindowPlan,
        team: Option<(u64, &TeamData)>,
        deadline: Instant,
        stop: &mut dyn FnMut() -> bool,
    ) -> Result<Held, AcquireError> {
        let started = Instant::now();
        let vi = plan.view;
        let view = view_name(vi);
        let mut outcome = "hit";
        let mut g = lock(&self.inner);
        let win = loop {
            if let Some(w) = g.cache.lookup(vi, &plan.need) {
                break w;
            }
            let key = (vi, plan.class.clone());
            if g.building.contains(&key) {
                outcome = "wait";
                g = self.wait(g, deadline, stop, false)?;
                continue;
            }
            let est = engine.estimate_window_bytes(vi, &plan.class);
            if !self.reserve_in(&mut g, est) {
                outcome = "wait";
                g = self.wait(g, deadline, stop, true)?;
                continue;
            }
            g.building.push(key.clone());
            drop(g);
            let t = Instant::now();
            let built = engine.build_window(vi, &plan.class, stop);
            let mut g2 = lock(&self.inner);
            g2.cache.release(est);
            g2.building.retain(|k| *k != key);
            self.cv.notify_all();
            let Some(w) = built else { return Err(AcquireError::Stopped) };
            metrics::histogram!("ord_generator_window_build_seconds", "view" => view)
                .record(t.elapsed().as_secs_f64());
            metrics::histogram!("ord_generator_window_chains", "view" => view).record(w.n_chains() as f64);
            outcome = "built";
            self.built.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let w = g2.cache.insert(w);
            g = g2;
            break w;
        };
        self.publish(&mut g);
        drop(g);
        metrics::counter!("ord_generator_window_requests_total", "view" => view, "outcome" => outcome)
            .increment(1);

        // Agrégats de l'équipe dans cette fenêtre : du cache, sinon calculés hors du verrou puis gardés si la place
        // le permet (sinon servis sans être gardés).
        let wt = match team {
            None => None,
            Some((serial, data)) => {
                let cached = lock(&self.inner).cache.team(&win, serial);
                Some(match cached {
                    Some(wt) => wt,
                    None => {
                        // Réservé avant d'être construit : un agrégat servi sans être gardé reste compté.
                        let est = WindowTeam::estimate_bytes(&win);
                        let mut g = lock(&self.inner);
                        while !self.reserve_in(&mut g, est) {
                            g = self.wait(g, deadline, stop, true)?;
                        }
                        drop(g);
                        let wt = WindowTeam::build(engine, &win, data);
                        let mut g = lock(&self.inner);
                        g.cache.release(est);
                        let r = if g.cache.make_room(wt.bytes()) {
                            g.cache.insert_team(&win, serial, wt)
                        } else {
                            Arc::new(wt)
                        };
                        self.publish(&mut g);
                        drop(g);
                        self.cv.notify_all();
                        r
                    }
                })
            }
        };

        // L'espace de travail de la génération : sa place est réservée, ou on attend qu'une autre génération rende la
        // sienne.
        let reserved = win.n_chains() * WORKSPACE_BYTES_PER_CHAIN;
        let mut g = lock(&self.inner);
        let mut waited = false;
        while !self.reserve_in(&mut g, reserved) {
            waited = true;
            g = self.wait(g, deadline, stop, true)?;
        }
        if waited {
            metrics::histogram!("ord_generator_budget_wait_seconds").record(started.elapsed().as_secs_f64());
        }
        Ok(Held { win, wt, reserved })
    }

    /// Réserve `bytes` pour une construction transitoire (les données d'une équipe : fenêtres de sa zone, incidence,
    /// composantes) : attend la place jusqu'à `deadline`, `Overloaded` sinon. Rendue à la chute de la garde.
    pub fn reserve_build(
        &self,
        bytes: usize,
        deadline: Instant,
        stop: &dyn Fn() -> bool,
    ) -> Result<BuildGuard<'_>, AcquireError> {
        let started = Instant::now();
        let mut g = lock(&self.inner);
        if bytes > g.cache.budget() {
            return Err(AcquireError::Overloaded);
        }
        let mut waited = false;
        while !self.reserve_in(&mut g, bytes) {
            waited = true;
            g = self.wait(g, deadline, &mut || stop(), true)?;
        }
        if waited {
            metrics::histogram!("ord_generator_budget_wait_seconds").record(started.elapsed().as_secs_f64());
        }
        metrics::gauge!("ord_generator_memory_bytes", "kind" => "team_build").increment(bytes as f64);
        Ok(BuildGuard { mgr: self, bytes })
    }

    /// Compte une équipe devenue résidente (`bytes` : surcouche, données, tables du contrôleur) : elle existe déjà, le
    /// budget ne peut pas la refuser ; il rend de la place en évinçant ce que personne ne tient.
    pub fn hold_team(&self, bytes: usize) {
        let mut g = lock(&self.inner);
        g.cache.hold_external(bytes);
        self.settle(&mut g);
        self.publish(&mut g);
    }

    /// Rend les octets d'une équipe qui n'est plus résidente (remplacée, supprimée, plafond de nombre).
    pub fn release_team(&self, bytes: usize) {
        let mut g = lock(&self.inner);
        g.cache.release_external(bytes);
        self.publish(&mut g);
        drop(g);
        self.cv.notify_all();
    }

    /// Rend la réservation d'une génération (les fenêtres et agrégats restent au cache).
    pub fn release(&self, held: Held) {
        let mut g = lock(&self.inner);
        g.cache.release(held.reserved);
        drop(held.win);
        drop(held.wt);
        self.publish(&mut g);
        drop(g);
        self.cv.notify_all();
    }

    /// Un espace de travail de la réserve (un neuf si elle est vide) ; ses octets sortent du compte.
    pub fn take_workspace(&self) -> Workspace {
        let mut g = lock(&self.inner);
        let Some(ws) = g.pool.pop() else { return Workspace::default() };
        g.cache.release(ws.bytes());
        self.publish(&mut g);
        drop(g);
        self.cv.notify_all();
        ws
    }

    /// Rend un espace de travail à la réserve ; un trop gros (plus de `keep_bytes`) est libéré (§9). Il n'est gardé
    /// que si le budget a la place **sans rien évincer**, et au plus `max_pool` à la fois : la réserve ne chasse
    /// jamais une fenêtre (§20.5).
    pub fn give_workspace(&self, mut ws: Workspace, keep_bytes: usize, max_pool: usize) {
        ws.shrink_to(keep_bytes);
        let bytes = ws.bytes();
        let mut g = lock(&self.inner);
        if bytes == 0 {
            // Rien à réutiliser (libéré par `shrink_to`, ou jamais servi) : pas de place prise dans la réserve.
        } else if g.pool.len() < max_pool && g.cache.fits(bytes) {
            g.cache.force_reserve(bytes);
            g.pool.push(ws);
        } else {
            metrics::counter!("ord_generator_window_evictions_total", "kind" => "workspace").increment(1);
        }
        self.publish(&mut g);
    }

    /// Octets de la réserve d'espaces de travail.
    #[cfg(test)]
    pub fn pool_bytes(&self) -> usize {
        lock(&self.inner).pool_bytes()
    }

    /// Fenêtres en cache.
    #[cfg(test)]
    pub fn cached_windows(&self) -> usize {
        lock(&self.inner).cache.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ord_graph::build::{build_graph, BuildOptions, MemSource};
    use ord_graph::format::{write_graph, GraphFile};
    use ord_graph::view::{build_view, ViewSpec};
    use ord_loops::{Mode, Params, Request, RideType};

    /// Grille de 30 × 30 carrefours de 140 m à z16 (tuiles de ~400 m) : plusieurs fenêtres disjointes.
    fn engine() -> Engine<Vec<u8>> {
        let (n, cell) = (30usize, 140.0f64);
        let (lat0, lon0, m_lat) = (47.2f64, -1.56f64, 111_194.926_6f64);
        let m_lon = m_lat * lat0.to_radians().cos();
        let mut s = MemSource::default();
        let id = |i: usize, j: usize| (1 + i * n + j) as i64;
        for i in 0..n {
            for j in 0..n {
                s.node(id(i, j), lat0 + i as f64 * cell / m_lat, lon0 + j as f64 * cell / m_lon);
            }
        }
        let mut way = 0i64;
        for i in 0..n {
            for j in 0..n {
                for (di, dj) in [(0, 1), (1, 0)] {
                    if i + di < n && j + dj < n {
                        way += 1;
                        s.way(way, &[id(i, j), id(i + di, j + dj)], &[("highway", "residential")]);
                    }
                }
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let file = |mut data: ord_graph::build::GraphData, name: &str| {
            let path = dir.path().join(name);
            write_graph(&path, &mut data, "test", 1_700_000_000).unwrap();
            GraphFile::open(std::fs::read(path).unwrap()).unwrap()
        };
        let base =
            file(build_graph(&mut s, &BuildOptions { zoom: 16 }, "test", &mut |_| {}).unwrap(), "b.ordg");
        let view = |name: &str| file(build_view(&base, &ViewSpec::parse(name).unwrap()).unwrap(), name);
        let (road, gravel) = (view("road"), view("gravel"));
        Engine::new(base, road, gravel, Params { window_margin_m: 20.0, ..Params::default() }).unwrap()
    }

    fn request(north: f64, east: f64) -> Request {
        let (lat0, lon0, m_lat) = (47.2f64, -1.56f64, 111_194.926_6f64);
        Request {
            start: ord_loops::LatLon::new(
                lat0 + north / m_lat,
                lon0 + east / (m_lat * lat0.to_radians().cos()),
            ),
            end: None,
            bearing: None,
            distance_m: 1500.0,
            ride_type: RideType::Road,
            mode: Mode::Discovery,
            familiarity: 0.5,
            off_team_cost_q8: 768,
            seed: 1,
        }
    }

    fn soon(ms: u64) -> Instant {
        Instant::now() + Duration::from_millis(ms)
    }

    #[test]
    fn a_window_is_built_once_per_key_even_under_concurrency() {
        let e = engine();
        let m = WindowManager::new(usize::MAX / 4);
        let plan = e.plan(&request(1000.0, 1000.0), 500.0);
        let held: Vec<Held> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..8)
                .map(|_| s.spawn(|| m.acquire(&e, &plan, None, soon(5_000), &mut || false).unwrap()))
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert_eq!(m.built(), 1, "une seule construction pour huit demandes");
        assert!(held.windows(2).all(|w| Arc::ptr_eq(&w[0].win, &w[1].win)));
        for h in held {
            m.release(h);
        }
        // Une requête voisine, que la même classe couvre, est servie sans construction.
        let near = e.plan(&request(1010.0, 1005.0), 500.0);
        let h = m.acquire(&e, &near, None, soon(5_000), &mut || false).unwrap();
        assert_eq!(m.built(), 1);
        m.release(h);
    }

    #[test]
    fn it_evicts_only_what_nobody_holds_waits_for_room_and_answers_503_at_the_deadline() {
        let e = engine();
        let (a, b) = (e.plan(&request(500.0, 500.0), 0.0), e.plan(&request(3500.0, 3500.0), 0.0));
        // Un budget de la taille d'une fenêtre et de son espace de travail, pas du double.
        let probe = WindowManager::new(usize::MAX / 4);
        let h = probe.acquire(&e, &a, None, soon(5_000), &mut || false).unwrap();
        let one = probe.counted();
        probe.release(h);
        let m = WindowManager::new(one + one / 4);
        let first = m.acquire(&e, &a, None, soon(5_000), &mut || false).unwrap();
        // `first` est tenue : rien à évincer, l'attente va à l'échéance, puis 503.
        let t = Instant::now();
        let r = m.acquire(&e, &b, None, soon(300), &mut || false);
        assert!(matches!(r, Err(AcquireError::Overloaded)), "{:?}", r.err());
        assert!(t.elapsed() >= Duration::from_millis(250));
        assert_eq!(m.built(), 1, "la seconde n'a pas été construite");
        // Une fois rendue, la première est évincée à la demande et la seconde passe.
        m.release(first);
        let second = m.acquire(&e, &b, None, soon(5_000), &mut || false).unwrap();
        assert_eq!(m.built(), 2);
        assert!(m.counted() <= m.budget() + m.budget() / 2, "{} / {}", m.counted(), m.budget());
        m.release(second);
        // Une demande qui attend une place se réveille quand une autre génération rend la sienne.
        let first = m.acquire(&e, &b, None, soon(5_000), &mut || false).unwrap();
        let other = std::thread::scope(|s| {
            let h = s.spawn(|| m.acquire(&e, &a, None, soon(5_000), &mut || false));
            std::thread::sleep(Duration::from_millis(150));
            m.release(first);
            h.join().unwrap()
        });
        assert!(other.is_ok());
    }

    #[test]
    fn a_stopped_request_leaves_nothing_in_the_cache() {
        let e = engine();
        let m = WindowManager::new(usize::MAX / 4);
        let plan = e.plan(&request(1000.0, 1000.0), 500.0);
        let r = m.acquire(&e, &plan, None, soon(5_000), &mut || true);
        assert!(matches!(r, Err(AcquireError::Stopped)));
        assert_eq!(m.built(), 0);
        assert_eq!(m.counted(), 0, "rien n'est resté réservé");
        assert!(m.acquire(&e, &plan, None, soon(5_000), &mut || false).is_ok());
    }

    #[test]
    fn a_team_build_reserves_its_transient_memory_in_the_same_budget() {
        let m = WindowManager::new(1_000);
        // Plus que tout le budget : jamais, 503 tout de suite.
        let t = Instant::now();
        assert!(matches!(m.reserve_build(2_000, soon(5_000), &|| false), Err(AcquireError::Overloaded)));
        assert!(t.elapsed() < Duration::from_millis(200));
        let g = m.reserve_build(800, soon(1_000), &|| false).unwrap();
        assert_eq!(m.counted(), 800);
        // Pas la place pour une seconde : attente jusqu'à l'échéance, puis 503.
        assert!(matches!(m.reserve_build(400, soon(150), &|| false), Err(AcquireError::Overloaded)));
        drop(g);
        assert_eq!(m.counted(), 0);
        let again = m.reserve_build(400, soon(1_000), &|| false).unwrap();
        assert_eq!(m.counted(), 400);
        drop(again);
        // Le client part : arrêt, pas 503.
        let g = m.reserve_build(900, soon(1_000), &|| false).unwrap();
        assert!(matches!(m.reserve_build(400, soon(5_000), &|| true), Err(AcquireError::Stopped)));
        drop(g);
    }

    /// Des générations qui laissent `n` espaces de travail non vides en réserve et leur fenêtre en cache.
    fn warm(m: &WindowManager, e: &Engine<Vec<u8>>, plan: &WindowPlan, r: &Request, n: usize) {
        let held = m.acquire(e, plan, None, soon(5_000), &mut || false).unwrap();
        let mut spaces: Vec<Workspace> = (0..n).map(|_| m.take_workspace()).collect();
        for ws in &mut spaces {
            ord_loops::generate_in(e, &held.win, None, r, ws, &mut || false).unwrap();
        }
        m.release(held);
        for ws in spaces {
            m.give_workspace(ws, usize::MAX, n);
        }
    }

    /// Le constat A, en synthétique : la réserve d'espaces de travail est comptée mais ne se rendait jamais sous
    /// pression ; une construction de `R` octets avec `R + P > B >= R + (fenêtres)` attendait pour toujours.
    #[test]
    fn a_full_workspace_pool_is_evicted_first_so_a_big_team_build_is_granted_at_once() {
        let e = engine();
        let (a, ra) = (e.plan(&request(500.0, 500.0), 0.0), request(500.0, 500.0));
        let probe = WindowManager::new(usize::MAX / 4);
        warm(&probe, &e, &a, &ra, 6);
        let (pool, windows) = (probe.pool_bytes(), probe.counted() - probe.pool_bytes());
        assert!(pool > 0 && windows > 0, "{pool} {windows}");

        let slack = 1_000;
        let budget = windows + pool + slack;
        // R + P > B (la réserve gêne), R + fenêtres <= B (les fenêtres ne gênent pas).
        let r = slack + pool / 2;
        assert!(r + pool > budget && r + windows <= budget);
        let m = WindowManager::new(budget);
        warm(&m, &e, &a, &ra, 6);
        assert_eq!(m.pool_bytes(), pool);
        let t = Instant::now();
        let g = m.reserve_build(r, soon(5_000), &|| false).expect("accordée tout de suite");
        assert!(t.elapsed() < Duration::from_millis(100), "elle a attendu {:?}", t.elapsed());
        assert_eq!(m.pool_bytes(), 0, "la réserve est vidée");
        assert_eq!(m.cached_windows(), 1, "la fenêtre est restée : la réserve part avant les fenêtres");
        assert_eq!(m.counted(), windows + r);
        drop(g);

        // Plus gros : la réserve puis la fenêtre inactive partent, rien d'autre ne gêne.
        let m = WindowManager::new(budget);
        warm(&m, &e, &a, &ra, 6);
        let g = m.reserve_build(budget - 10, soon(5_000), &|| false).expect("accordée tout de suite");
        assert_eq!((m.pool_bytes(), m.cached_windows()), (0, 0));
        drop(g);
    }

    #[test]
    fn the_pool_keeps_a_workspace_only_when_the_budget_has_room_without_evicting_anything() {
        let e = engine();
        let (a, ra) = (e.plan(&request(500.0, 500.0), 0.0), request(500.0, 500.0));
        // Le budget exact d'une génération en cours : fenêtre et réservation de son espace de travail.
        let probe = WindowManager::new(usize::MAX / 4);
        let h = probe.acquire(&e, &a, None, soon(5_000), &mut || false).unwrap();
        let one = probe.counted();
        probe.release(h);
        let m = WindowManager::new(one);
        let held = m.acquire(&e, &a, None, soon(5_000), &mut || false).unwrap();
        let mut ws = m.take_workspace();
        ord_loops::generate_in(&e, &held.win, None, &ra, &mut ws, &mut || false).unwrap();
        m.give_workspace(ws, usize::MAX, 4);
        assert_eq!(m.pool_bytes(), 0, "sans place, l'espace est libéré");
        m.release(held);
        assert_eq!(m.cached_windows(), 1, "et la fenêtre n'a pas été évincée pour lui");
        // Trop gros pour la réserve (§9) : libéré aussi.
        let m = WindowManager::new(usize::MAX / 4);
        let held = m.acquire(&e, &a, None, soon(5_000), &mut || false).unwrap();
        let mut ws = m.take_workspace();
        ord_loops::generate_in(&e, &held.win, None, &ra, &mut ws, &mut || false).unwrap();
        m.give_workspace(ws, 1, 4);
        assert_eq!(m.pool_bytes(), 0);
        // Au plus `max_pool` en réserve.
        let (mut ws1, mut ws2) = (Workspace::default(), Workspace::default());
        ord_loops::generate_in(&e, &held.win, None, &ra, &mut ws1, &mut || false).unwrap();
        ord_loops::generate_in(&e, &held.win, None, &ra, &mut ws2, &mut || false).unwrap();
        m.release(held);
        let sizes = (ws1.bytes(), ws2.bytes());
        m.give_workspace(ws1, usize::MAX, 1);
        let one = m.pool_bytes();
        assert!(one > 0, "{sizes:?}");
        m.give_workspace(ws2, usize::MAX, 1);
        assert_eq!(m.pool_bytes(), one, "une seule en réserve");
    }

    /// Des équipes résidentes : `(octets, numéro)`, de la plus ancienne à la plus récente ; une seule tenue n'est pas
    /// évincée.
    struct Teams {
        list: Mutex<Vec<(usize, u64)>>,
        held: Option<u64>,
    }

    impl TeamEvictor for Teams {
        fn evict_idle(&self) -> Option<(usize, u64)> {
            let mut l = lock(&self.list);
            let i = l.iter().position(|(_, n)| Some(*n) != self.held)?;
            Some(l.remove(i))
        }
    }

    fn manager_with(budget: usize, teams: &[(usize, u64)], held: Option<u64>) -> (WindowManager, Arc<Teams>) {
        let m = WindowManager::new(budget);
        let t = Arc::new(Teams { list: Mutex::new(teams.to_vec()), held });
        let weak: Weak<dyn TeamEvictor> = Arc::downgrade(&t) as Weak<dyn TeamEvictor>;
        m.set_evictor(weak);
        for (b, _) in teams {
            m.hold_team(*b);
        }
        (m, t)
    }

    #[test]
    fn resident_teams_count_in_the_budget_and_the_least_recent_leave_when_room_is_missing() {
        let (m, t) = manager_with(1_000, &[(300, 1), (300, 2), (300, 3)], None);
        assert_eq!(m.counted(), 900);
        // 900 + 400 > 1 000 : la plus ancienne équipe part, pas les autres.
        let g = m.reserve_build(400, soon(1_000), &|| false).unwrap();
        assert_eq!(m.counted(), 1_000);
        assert_eq!(*lock(&t.list), vec![(300, 2), (300, 3)]);
        // Une réservation de plus : la suivante, dans l'ordre d'ancienneté.
        let g2 = m.reserve_build(300, soon(1_000), &|| false).unwrap();
        assert_eq!(*lock(&t.list), vec![(300, 3)]);
        assert_eq!(m.counted(), 1_000);
        drop((g, g2));
        assert_eq!(m.counted(), 300, "les réservations rendues, il ne reste que l'équipe résidente");
        // Une équipe rendue par le stockage (supprimée, remplacée) libère son compte.
        m.release_team(300);
        assert_eq!(m.counted(), 0);
    }

    #[test]
    fn a_team_nobody_can_evict_makes_the_reservation_wait_then_answer_503() {
        // La seule équipe est tenue par une génération : rien à évincer.
        let (m, t) = manager_with(1_000, &[(700, 1)], Some(1));
        let r = m.reserve_build(500, soon(200), &|| false);
        assert!(matches!(r, Err(AcquireError::Overloaded)));
        assert_eq!(lock(&t.list).len(), 1, "l'équipe tenue est restée");
        assert_eq!(m.counted(), 700);
        // Libérée, elle s'évince et la réservation passe.
        let (m, t) = manager_with(1_000, &[(700, 1)], None);
        assert!(m.reserve_build(500, soon(200), &|| false).is_ok());
        assert!(lock(&t.list).is_empty());
    }

    #[test]
    fn a_team_that_becomes_resident_evicts_older_ones_to_fit_and_is_never_refused() {
        let (m, t) = manager_with(1_000, &[(400, 1), (400, 2)], None);
        // Une équipe de 500 o de plus est déjà en mémoire : elle est comptée, la plus ancienne part pour revenir
        // sous le budget.
        lock(&t.list).push((500, 3));
        m.hold_team(500);
        assert_eq!(*lock(&t.list), vec![(400, 2), (500, 3)]);
        assert_eq!(m.counted(), 900);
        // Même seule au-dessus du budget, elle est comptée (elle existe) sans rien évincer d'autre qu'elle ne peut.
        let (m, _t) = manager_with(1_000, &[], None);
        m.hold_team(1_500);
        assert_eq!(m.counted(), 1_500);
    }
}
