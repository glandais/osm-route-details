//! Les fenêtres du banc (lot 6) : le moteur **à plat** (une fenêtre de toutes les tuiles par vue, la référence) ou le
//! moteur **fenêtré** (cache LRU de fenêtres sous un budget, comme le service), et ce qu'on en rapporte : tailles,
//! constructions à froid, mémoire comptée et mémoire du processus.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Result};
use ord_loops::{CacheStats, Engine, Request, TeamData, Window, WindowCache, WindowTeam};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Flat,
    Window,
}

/// Ce que la génération reçoit pour une requête.
pub struct Acquired {
    pub win: Arc<Window>,
    pub wt: Arc<WindowTeam>,
    /// Fenêtre construite pour cette requête (et non servie du cache), et sa durée.
    pub built: bool,
    pub build_ms: f64,
}

pub struct Windows<'a, B> {
    engine: &'a Engine<B>,
    kind: Kind,
    cache: WindowCache,
    step_m: f64,
    flat: [Option<Arc<Window>>; 2],
    flat_teams: HashMap<(u64, usize), Arc<WindowTeam>>,
    /// Une entrée par fenêtre construite : (durée ms, chaînes, octets).
    pub built: Vec<(f64, usize, usize)>,
    pub team_builds_ms: Vec<f64>,
    pub peak_counted: usize,
    pub escapes: u64,
}

impl<'a, B: AsRef<[u8]>> Windows<'a, B> {
    pub fn new(engine: &'a Engine<B>, kind: Kind, budget_bytes: usize, step_m: f64) -> Self {
        Self {
            engine,
            kind,
            cache: WindowCache::new(budget_bytes),
            step_m,
            flat: [None, None],
            flat_teams: HashMap::new(),
            built: Vec::new(),
            team_builds_ms: Vec::new(),
            peak_counted: 0,
            escapes: 0,
        }
    }

    pub fn cache_stats(&self) -> CacheStats {
        self.cache.stats()
    }

    pub fn counted_now(&self) -> usize {
        match self.kind {
            Kind::Window => self.cache.total(),
            Kind::Flat => {
                self.flat.iter().flatten().map(|w| w.bytes()).sum::<usize>()
                    + self.flat_teams.values().map(|t| t.bytes()).sum::<usize>()
            }
        }
    }

    /// La fenêtre et les agrégats d'équipe de la requête. `team_key` identifie l'équipe dans le cache.
    pub fn acquire(&mut self, team_key: u64, team: &TeamData, req: &Request) -> Result<Acquired> {
        let vi = req.ride_type.view_index();
        let (mut built, mut build_ms) = (false, 0.0);
        let win = match self.kind {
            Kind::Flat => {
                if self.flat[vi].is_none() {
                    let t = Instant::now();
                    let w = Arc::new(self.engine.full_window(vi));
                    self.built.push((t.elapsed().as_secs_f64() * 1000.0, w.n_chains(), w.bytes()));
                    self.flat[vi] = Some(w);
                }
                Arc::clone(self.flat[vi].as_ref().expect("construite"))
            }
            Kind::Window => {
                let plan = self.engine.plan(req, self.step_m);
                match self.cache.lookup(vi, &plan.need) {
                    Some(w) => w,
                    None => {
                        let est = self.engine.estimate_window_bytes(vi, &plan.class);
                        if !self.cache.make_room(est) {
                            return Err(anyhow!("budget de mémoire épuisé : {est} octets demandés"));
                        }
                        let t = Instant::now();
                        let w = self
                            .engine
                            .build_window(vi, &plan.class, &mut || false)
                            .ok_or_else(|| anyhow!("construction coupée"))?;
                        build_ms = t.elapsed().as_secs_f64() * 1000.0;
                        built = true;
                        self.built.push((build_ms, w.n_chains(), w.bytes()));
                        self.cache.insert(w)
                    }
                }
            }
        };
        let t = Instant::now();
        let (wt, team_ms) = match self.kind {
            Kind::Flat => match self.flat_teams.get(&(team_key, vi)) {
                Some(wt) => (Arc::clone(wt), 0.0),
                None => {
                    let wt = Arc::new(WindowTeam::build(self.engine, &win, team));
                    self.flat_teams.insert((team_key, vi), Arc::clone(&wt));
                    (wt, t.elapsed().as_secs_f64() * 1000.0)
                }
            },
            Kind::Window => match self.cache.team(&win, team_key) {
                Some(wt) => (wt, 0.0),
                None => {
                    let wt = WindowTeam::build(self.engine, &win, team);
                    let ms = t.elapsed().as_secs_f64() * 1000.0;
                    // La place des agrégats d'équipe : si elle manque, on les sert sans les garder.
                    if self.cache.make_room(wt.bytes()) {
                        (self.cache.insert_team(&win, team_key, wt), ms)
                    } else {
                        (Arc::new(wt), ms)
                    }
                }
            },
        };
        if team_ms > 0.0 {
            self.team_builds_ms.push(team_ms);
        }
        self.peak_counted = self.peak_counted.max(self.counted_now());
        Ok(Acquired { win, wt, built, build_ms })
    }

    /// Oublie les agrégats d'une équipe (équipe suivante) : ils ne servent plus au banc.
    pub fn forget_team(&mut self, team_key: u64) {
        self.cache.drop_team(team_key);
        self.flat_teams.retain(|k, _| k.0 != team_key);
    }
}

/// Lignes de `/proc/self/status` utiles : `(VmHWM, VmRSS, RssAnon, RssFile)` en Mo.
pub fn memory_mb() -> (f64, f64, f64, f64) {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let kb = |key: &str| {
        s.lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0)
            / 1000.0
    };
    (kb("VmHWM:"), kb("VmRSS:"), kb("RssAnon:"), kb("RssFile:"))
}

/// Médiane, p90 et maximum d'une liste de durées.
pub fn quantiles(mut v: Vec<f64>) -> Option<(f64, f64, f64)> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let at = |q: f64| v[(((v.len() - 1) as f64) * q).round() as usize];
    Some((at(0.5), at(0.9), at(1.0)))
}
