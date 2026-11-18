//! Requêtes, jeu en parallèle, choix des parcours (§11.6 de la note).
//!
//! Une requête par boucle réelle retenue et par valeur du curseur ; le moteur est joué comme le service (une
//! reprise après un échappement de fenêtre), les parcours que le contrôleur indépendant refuse sont écartés, et on
//! garde le premier parcours rendu et deux « meilleurs des trois ».

use std::collections::{BTreeMap, BTreeSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{anyhow, Result};
use memmap2::Mmap;
use ord_graph::mmap::MappedGraph;
use ord_graph::overlay::Overlay;
use ord_loops::{
    generate_with_figures_in, Engine, Failure, FigureDiagnostics, Request, RideType, Route, Taste, TeamCtx,
    TeamData, Window, WindowCache, WindowTeam, Workspace,
};
use ord_loops_check::{team_edge, Checker, TypeIndex};
use serde::Serialize;
use xxhash_rust::xxh3::xxh3_64;

use super::export::Loop;
use super::measure::{class_lengths, measure, Measures};
use super::variants::TeamWindowCounters;
use super::walk::{start_latlon, Walk};
use crate::record::failure_code;
use ord_graph_match::profile::collect::{bearing_of, loop_request, request_distance_m};

pub type Eng = Engine<Mmap>;

/// Le moteur, ses vues et le cache de fenêtres partagé par tous les fils.
#[derive(Debug)]
pub struct SharedWindows<'e> {
    engine: &'e Eng,
    cache: Mutex<WindowCache>,
    step_m: f64,
    built: AtomicU64,
    hits: AtomicU64,
    /// Fenêtres insérées alors que le budget ne laissait pas la place (le budget du banc est souple).
    overflow: AtomicU64,
    escapes: AtomicU64,
    /// Constructions de `WindowTeam` depuis le dernier `take_team_counters` : nombre, nanosecondes, plus gros `bytes()`.
    team_builds: AtomicU64,
    team_ns: AtomicU64,
    team_max_bytes: AtomicU64,
}

/// Compteurs rapportés.
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct WindowCounters {
    pub built: u64,
    pub hits: u64,
    pub overflow: u64,
    pub escapes: u64,
}

impl<'e> SharedWindows<'e> {
    pub fn new(engine: &'e Eng, budget_bytes: usize, step_m: f64) -> Self {
        Self {
            engine,
            cache: Mutex::new(WindowCache::new(budget_bytes)),
            step_m,
            built: AtomicU64::new(0),
            hits: AtomicU64::new(0),
            overflow: AtomicU64::new(0),
            escapes: AtomicU64::new(0),
            team_builds: AtomicU64::new(0),
            team_ns: AtomicU64::new(0),
            team_max_bytes: AtomicU64::new(0),
        }
    }

    /// Les constructions de `WindowTeam` depuis le dernier appel (les compteurs repartent de zéro).
    pub fn take_team_counters(&self) -> TeamWindowCounters {
        TeamWindowCounters {
            builds: self.team_builds.swap(0, Ordering::Relaxed),
            ms: self.team_ns.swap(0, Ordering::Relaxed) as f64 / 1e6,
            max_bytes: self.team_max_bytes.swap(0, Ordering::Relaxed),
        }
    }

    /// `WindowTeam::build_with_taste`, chronométré et compté.
    fn build_team(&self, win: &Window, team: &TeamData, taste: Option<&Taste>) -> WindowTeam {
        let t = Instant::now();
        let wt = WindowTeam::build_with_taste(self.engine, win, team, taste);
        self.team_ns.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
        self.team_builds.fetch_add(1, Ordering::Relaxed);
        self.team_max_bytes.fetch_max(wt.bytes() as u64, Ordering::Relaxed);
        wt
    }

    pub fn counters(&self) -> WindowCounters {
        WindowCounters {
            built: self.built.load(Ordering::Relaxed),
            hits: self.hits.load(Ordering::Relaxed),
            overflow: self.overflow.load(Ordering::Relaxed),
            escapes: self.escapes.load(Ordering::Relaxed),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, WindowCache> {
        self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// La fenêtre de la requête et les agrégats de l'équipe `team_key` : du cache, sinon construits (hors verrou).
    /// Deux fils peuvent bâtir la même fenêtre : sans effet sur les résultats.
    pub fn acquire(
        &self,
        team_key: u64,
        team: &TeamData,
        req: &Request,
        taste: Option<&Taste>,
    ) -> (Arc<Window>, Arc<WindowTeam>) {
        let plan = self.engine.plan(req, self.step_m);
        let vi = plan.view;
        let hit = self.lock().lookup(vi, &plan.need);
        let win = match hit {
            Some(w) => {
                self.hits.fetch_add(1, Ordering::Relaxed);
                w
            }
            None => {
                let est = self.engine.estimate_window_bytes(vi, &plan.class);
                if !self.lock().make_room(est) {
                    self.overflow.fetch_add(1, Ordering::Relaxed);
                }
                let w = self.engine.build_window(vi, &plan.class, &mut || false).expect("jamais coupée");
                self.built.fetch_add(1, Ordering::Relaxed);
                self.lock().insert(w)
            }
        };
        let cached = self.lock().team(&win, team_key);
        let wt = match cached {
            Some(wt) => wt,
            None => {
                let wt = self.build_team(&win, team, taste);
                let mut c = self.lock();
                if c.make_room(wt.bytes()) {
                    c.insert_team(&win, team_key, wt)
                } else {
                    Arc::new(wt)
                }
            }
        };
        (win, wt)
    }

    /// La reprise du service après un échappement : fenêtre élargie de la plus longue chaîne et de 10 km, construite
    /// hors cache.
    pub fn acquire_wide(
        &self,
        team: &TeamData,
        req: &Request,
        taste: Option<&Taste>,
    ) -> (Arc<Window>, Arc<WindowTeam>) {
        self.escapes.fetch_add(1, Ordering::Relaxed);
        let vi = req.ride_type.view_index();
        let extra = self.engine.longest_chain_m(vi) + 10_000.0;
        let plan = self.engine.plan_with(req, self.step_m, extra);
        let win = Arc::new(self.engine.build_window(vi, &plan.class, &mut || false).expect("jamais coupée"));
        let wt = Arc::new(self.build_team(&win, team, taste));
        (win, wt)
    }

    /// Oublie les agrégats d'une équipe (équipe suivante).
    pub fn drop_team(&self, key: u64) {
        self.lock().drop_team(key);
    }
}

// --- Enregistrements ---------------------------------------------------------------------------------------------

/// Ce qu'on garde d'une comparaison d'un parcours généré à la boucle réelle, sur une surcouche (§11.8).
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Recovered {
    /// Mètres de la boucle réelle (arêtes de base, chacune une fois) et mètres retrouvés dans le parcours.
    pub real_m: f64,
    pub found_m: f64,
    /// Les mêmes, sur les arêtes « d'équipe pour ce type » de la surcouche.
    pub team_real_m: f64,
    pub team_found_m: f64,
    /// Les mêmes, sur les arêtes qu'aucune autre trace ne prend (absentes de la surcouche ou à zéro).
    pub fresh_real_m: f64,
    pub fresh_found_m: f64,
}

impl Recovered {
    fn ratio(num: f64, den: f64) -> Option<f64> {
        (den > 0.0).then(|| num / den)
    }

    pub fn total(&self) -> Option<f64> {
        Self::ratio(self.found_m, self.real_m)
    }

    pub fn team(&self) -> Option<f64> {
        Self::ratio(self.team_found_m, self.team_real_m)
    }

    pub fn fresh(&self) -> Option<f64> {
        Self::ratio(self.fresh_found_m, self.fresh_real_m)
    }

    /// Part de nouveau dans la trace elle-même.
    pub fn fresh_real_share(&self) -> Option<f64> {
        Self::ratio(self.fresh_real_m, self.real_m)
    }
}

/// `E(G)` d'un parcours généré : ses arêtes de base, chacune une fois (sens ignorés). `E(R)` d'une boucle réelle est
/// `measure::taken_edges` (les arêtes que le dépôt compte pour elle).
fn edge_set(w: &Walk) -> BTreeSet<u32> {
    w.edges().collect()
}

/// La part retrouvée du parcours `generated` dans la boucle `real`, contre la surcouche `o` (§11.8).
pub fn recovered(
    gf: &MappedGraph,
    real: &BTreeSet<u32>,
    generated: &BTreeSet<u32>,
    o: &Overlay,
    ti: &TypeIndex,
    ride: RideType,
) -> Recovered {
    let mut r = Recovered::default();
    for &e in real {
        let len = f64::from(gf.edge(e).len_dm) / 10.0;
        let found = generated.contains(&e);
        let counts = o.get(e);
        r.real_m += len;
        r.found_m += if found { len } else { 0.0 };
        if team_edge(ride, gf.edge(e).attrs, counts, ti) {
            r.team_real_m += len;
            r.team_found_m += if found { len } else { 0.0 };
        }
        if counts.is_none_or(|c| c.iter().all(|&x| x == 0)) {
            r.fresh_real_m += len;
            r.fresh_found_m += if found { len } else { 0.0 };
        }
    }
    r
}

/// Un parcours généré gardé.
#[derive(Clone, Debug, Serialize)]
pub struct GenRoute {
    pub m: Measures,
    pub length_over_target: f64,
    pub team_share: f64,
    pub family: String,
    pub recovered: Recovered,
    /// Recouvrement mesuré par le banc moins `Route::shared_m`, m.
    pub shared_vs_route_m: f64,
    /// Rapport entre la polyligne et la longueur du graphe : `|l_poly − d_graph| / d_graph`.
    pub poly_vs_graph: f64,
    pub route_xxh3: String,
}

/// Ce que la génération organique a fait pour un jeu (lot 4, 4c, §15.11.3) : les compteurs de `FigureDiagnostics`, rien
/// de privé.
#[derive(Clone, Debug, Default, Serialize)]
pub struct FigAgg {
    pub applied: bool,
    pub tri_first: bool,
    pub rho_target: f64,
    pub candidates: u32,
    pub pairs: u32,
    pub pairs_admissible: u32,
    pub searched: u32,
    pub retried: u32,
    pub leg2_found: u32,
    pub leg2_capped: u32,
    pub leg3_t2: u32,
    pub leg3_astar: u32,
    pub leg3_failed: u32,
    pub complete: u32,
    pub within_tol: u32,
    pub spikes: u32,
    pub spikes_repaired: u32,
    pub spikes_dropped: u32,
    /// Triangles refusés par le contrôle interne du moteur, par code.
    pub tri_rejected: BTreeMap<String, u32>,
    pub tri_scored: u32,
    pub p2p_settled: u64,
    pub leg3_settled: u64,
    pub beta_final: f64,
    /// Épis dans les triangles **gardés** (relevés sur la polyligne, §15.5) : le contrôle de l'angle sur les parcours rendus.
    pub spikes_kept: u32,
}

impl FigAgg {
    pub fn of(d: &FigureDiagnostics) -> Self {
        Self {
            applied: d.applied,
            tri_first: d.tri_first,
            rho_target: d.rho_target,
            candidates: d.candidates,
            pairs: d.pairs,
            pairs_admissible: d.pairs_admissible,
            searched: d.searched,
            retried: d.retried,
            leg2_found: d.leg2_found,
            leg2_capped: d.leg2_capped,
            leg3_t2: d.leg3_t2,
            leg3_astar: d.leg3_astar,
            leg3_failed: d.leg3_failed,
            complete: d.complete,
            within_tol: d.within_tol,
            spikes: d.spikes,
            spikes_repaired: d.spikes_repaired,
            spikes_dropped: d.spikes_dropped,
            tri_rejected: d.tri_rejected.iter().map(|(v, n)| (v.code().to_string(), *n)).collect(),
            tri_scored: d.tri_scored,
            p2p_settled: d.p2p_settled,
            leg3_settled: d.leg3_settled,
            beta_final: d.beta_final,
            spikes_kept: 0,
        }
    }
}

/// Épis d'une polyligne plane ouverte au départ (§15.5, lue sur le parcours rendu) : à chaque sommet gardé à 10 % de D
/// (hors départ et arrivée), l'angle entre le cap des `spike_m` mètres d'arrivée et celui des `spike_m` mètres de départ
/// atteint `spike_deg`.
pub fn spikes_of(pts: &[ord_graph_match::plane::Pt], spike_m: f64, spike_deg: f64) -> u32 {
    use ord_graph_match::plane::{cumulative, heading_deg};
    let kept = ord_graph_match::plane::vertex_indices(pts, 0.10);
    if kept.len() < 3 {
        return 0;
    }
    let cum = cumulative(pts);
    let mut n = 0;
    for &i in &kept[1..kept.len() - 1] {
        let before = (0..i).rev().find(|&j| cum[i] - cum[j] >= spike_m).unwrap_or(0);
        let after = (i + 1..pts.len()).find(|&j| cum[j] - cum[i] >= spike_m).unwrap_or(pts.len() - 1);
        let (h_in, h_out) = (heading_deg(pts[before], pts[i]), heading_deg(pts[i], pts[after]));
        let d = (h_out - h_in + 540.0).rem_euclid(360.0) - 180.0;
        if d.abs() >= spike_deg {
            n += 1;
        }
    }
    n
}

/// Un jeu : une boucle, une valeur du curseur, une surcouche.
#[derive(Clone, Debug, Serialize)]
pub struct PlayRec {
    pub cursor: f64,
    /// `full` ou `holdout`.
    pub overlay: String,
    /// `OK`, `VIOLATION`, `NO_ROUTES`, un code d'échec, `PANIC` ou `WINDOW_ESCAPE`.
    pub outcome: String,
    pub n_routes: usize,
    pub n_kept: usize,
    /// Parcours refusés par le contrôleur, par code de violation.
    pub dropped: BTreeMap<String, u32>,
    pub ms: f64,
    /// Les gardés, dans l'ordre du moteur.
    pub routes: Vec<GenRoute>,
    pub best_shape: Option<usize>,
    pub best_recovered: Option<usize>,
    /// Mètres par classe de goût du **premier** parcours gardé (sonde du goût, §12.2) ; en mémoire seulement.
    #[serde(skip)]
    pub first_class_m: Option<Vec<f64>>,
    /// Génération organique (4c) : absent pour `Classic`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fig: Option<FigAgg>,
}

/// Une tâche : jouer la requête de la boucle `loop_idx` à la valeur `cursor` sur la surcouche de l'environnement.
#[derive(Clone, Debug)]
pub struct Task {
    pub loop_idx: usize,
    pub cursor: f64,
    pub req: Request,
    pub ride: RideType,
}

/// Ce que les jeux lisent pour une équipe et une surcouche.
#[derive(Debug)]
pub struct TeamEnv<'a> {
    pub key: u64,
    pub overlay: &'a Overlay,
    pub types: TypeIndex,
    pub team: &'a TeamData,
    pub checker: &'a Checker<'a, Mmap>,
    /// Goût de l'équipe pour cette surcouche (sonde du goût) ; `None` : le moteur d'aujourd'hui.
    pub taste: Option<&'a Taste>,
}

/// Ce qui est commun à tous les jeux.
#[derive(Debug)]
pub struct PlayCtx<'a> {
    pub engine: &'a Eng,
    pub wins: &'a SharedWindows<'a>,
    /// IQR des trois mesures de forme du réel, par type (`Road`, `Gravel`, `Mtb`) ; 1 si nul.
    pub iqr: [[f64; 3]; 3],
    /// Mesures du réel de chaque boucle (index de `Task::loop_idx`) pour `best_shape`.
    pub real_shape: &'a [[f64; 3]],
    /// `E(R)` de chaque boucle réelle : les arêtes que le dépôt compte pour elle (`measure::taken_edges`).
    pub real_edges: &'a [BTreeSet<u32>],
}

pub use ord_graph_match::profile::collect::ride_of;

/// La requête de la boucle réelle `lp` à la valeur `cursor` du curseur (§11.6).
pub fn request_of(gf: &MappedGraph, lp: &Loop, cursor: f64, seed: u64) -> Result<Request> {
    let (lat, lon) = start_latlon(gf, &lp.walk).ok_or_else(|| anyhow!("boucle sans jambe"))?;
    let applied = ord_server::generator::cursor::apply(
        ord_server::generator::cursor::hundredths(cursor)
            .ok_or_else(|| anyhow!("curseur hors de [0, 1] : {cursor}"))?,
    );
    Ok(loop_request(
        (lat, lon),
        bearing_of(&lp.trace.pts, lp.trace.l_poly),
        request_distance_m(lp.trace.d_graph),
        lp.ty,
        applied.mode,
        applied.familiarity,
        applied.off_team_cost_q8,
        lp.id_hash ^ seed,
        ord_loops::Figures::Classic,
    ))
}

/// Empreinte d'un parcours : `(arête, sens)` de chaque étape puis les deux décalages au décimètre.
pub fn route_hash(r: &Route) -> u64 {
    let mut b: Vec<u8> = Vec::with_capacity(r.steps.len() * 5 + 16);
    for s in &r.steps {
        b.extend_from_slice(&s.base_edge.to_le_bytes());
        b.push(u8::from(s.reverse));
    }
    for off in [r.start.offset_m, r.end.offset_m] {
        b.extend_from_slice(&((off * 10.0).round() as i64).to_le_bytes());
    }
    xxh3_64(&b)
}

fn shape_triple(m: &Measures) -> [f64; 3] {
    [m.r_over_d, m.far_pos, m.compactness]
}

/// Un jeu : la requête, la génération (une reprise après un échappement), le contrôle, les mesures.
pub fn play_one(
    ctx: &PlayCtx<'_>,
    env: &TeamEnv<'_>,
    ws: &mut Workspace,
    task: &Task,
    overlay: &str,
) -> PlayRec {
    let gf = ctx.engine.base();
    let req = &task.req;
    let t0 = Instant::now();
    let mut rec = PlayRec {
        cursor: task.cursor,
        overlay: overlay.to_string(),
        outcome: String::new(),
        n_routes: 0,
        n_kept: 0,
        dropped: BTreeMap::new(),
        ms: 0.0,
        routes: Vec::new(),
        best_shape: None,
        best_recovered: None,
        first_class_m: None,
        fig: None,
    };
    // Les compteurs de la génération organique, pris dans l'appel qui a rendu le résultat.
    let fig_cell = std::cell::RefCell::new(FigureDiagnostics::default());
    let call = |ws: &mut Workspace, win: &Window, wt: &WindowTeam| {
        catch_unwind(AssertUnwindSafe(|| {
            let tc = TeamCtx { data: env.team, win: wt };
            let (res, _, fig) = generate_with_figures_in(ctx.engine, win, Some(tc), req, ws, &mut || false);
            *fig_cell.borrow_mut() = fig;
            res
        }))
    };
    let (mut win, mut wt) = ctx.wins.acquire(env.key, env.team, req, env.taste);
    let mut res = call(ws, &win, &wt);
    if matches!(res, Ok(Err(Failure::WindowEscape))) {
        (win, wt) = ctx.wins.acquire_wide(env.team, req, env.taste);
        res = call(ws, &win, &wt);
    }
    rec.ms = t0.elapsed().as_secs_f64() * 1000.0;
    // La sonde point à point (4c), hors du temps mesuré, sur la fenêtre du jeu.
    if super::p2p::wanted(req, overlay) {
        let (outcome, diag) = match &res {
            Err(_) => ("PANIC".to_string(), None),
            Ok(Err(f)) => (failure_code(f), None),
            Ok(Ok(g)) => (format!("ROUTES_{}", g.routes.len()), Some(&g.diagnostics)),
        };
        super::p2p::probe(
            ctx.engine,
            &win,
            env.team,
            &wt,
            req,
            ws,
            env.key,
            task.cursor,
            rec.ms,
            &outcome,
            diag,
        );
    }
    drop((win, wt));
    if fig_cell.borrow().applied {
        rec.fig = Some(FigAgg::of(&fig_cell.borrow()));
    }
    let generation = match res {
        Err(_) => {
            rec.outcome = "PANIC".into();
            return rec;
        }
        Ok(Err(f)) => {
            rec.outcome = failure_code(&f);
            return rec;
        }
        Ok(Ok(g)) => g,
    };
    rec.n_routes = generation.routes.len();
    let real = &ctx.real_edges[task.loop_idx];
    for route in &generation.routes {
        let rep = env.checker.check(req, route);
        if !rep.ok() {
            for v in &rep.violations {
                *rec.dropped.entry(v.code().to_string()).or_default() += 1;
            }
            continue;
        }
        let walk = Walk::from_route(gf, route);
        let trace = walk.trace(gf);
        let m = measure(gf, &walk, &trace, Some(rep.crossings));
        if route.family == ord_loops::Family::Triangle {
            if let Some(f) = rec.fig.as_mut() {
                let t = ord_loops::OrganicTune::default();
                f.spikes_kept += spikes_of(&trace.pts, f64::from(t.spike_m), f64::from(t.spike_deg));
            }
        }
        let rc = recovered(gf, real, &edge_set(&walk), env.overlay, &env.types, task.ride);
        if rec.routes.is_empty() {
            rec.first_class_m = Some(class_lengths(gf, &walk, &trace).to_vec());
        }
        rec.routes.push(GenRoute {
            length_over_target: rep.length_m / req.distance_m,
            team_share: if rep.length_m > 0.0 { rep.team_m / rep.length_m } else { 0.0 },
            family: format!("{:?}", route.family),
            recovered: rc,
            shared_vs_route_m: m.shared_share * trace.d_graph - route.shared_m,
            poly_vs_graph: if trace.d_graph > 0.0 {
                (trace.l_poly - trace.d_graph).abs() / trace.d_graph
            } else {
                0.0
            },
            route_xxh3: format!("{:016x}", route_hash(route)),
            m,
        });
    }
    rec.n_kept = rec.routes.len();
    rec.outcome = if rec.n_routes == 0 {
        "NO_ROUTES".into()
    } else if rec.n_kept == 0 {
        "VIOLATION".into()
    } else {
        "OK".into()
    };
    // Meilleur des trois, pour les distributions : le plus proche du réel en forme ; égalité : le plus petit rang.
    let real_shape = ctx.real_shape[task.loop_idx];
    let iqr = ctx.iqr[task.ride.index()];
    let score = |g: &GenRoute| -> f64 {
        shape_triple(&g.m).iter().zip(real_shape).zip(iqr).map(|((a, b), q)| (a - b).abs() / q).sum()
    };
    let mut best: Option<(usize, f64)> = None;
    for (k, g) in rec.routes.iter().enumerate() {
        let s = score(g);
        if best.is_none_or(|(_, b)| s < b) {
            best = Some((k, s));
        }
    }
    rec.best_shape = best.map(|b| b.0);
    // Meilleur des trois, pour la reconstruction : la plus grande part retrouvée totale.
    let mut best: Option<(usize, f64)> = None;
    for (k, g) in rec.routes.iter().enumerate() {
        let s = g.recovered.total().unwrap_or(0.0);
        if best.is_none_or(|(_, b)| s > b) {
            best = Some((k, s));
        }
    }
    rec.best_recovered = best.map(|b| b.0);
    rec
}

/// Exécute les tâches sur `threads` fils (un `Workspace` par fil, créé une fois et réutilisé d'une équipe à l'autre),
/// l'ordre d'exécution n'a aucun effet sur les résultats. Rend `(indice de tâche, jeu)` trié par indice.
pub fn run_tasks(
    ctx: &PlayCtx<'_>,
    env: &TeamEnv<'_>,
    tasks: &[Task],
    overlay: &str,
    workspaces: &mut [Workspace],
) -> Vec<(usize, PlayRec)> {
    let next = AtomicUsize::new(0);
    let mut out: Vec<(usize, PlayRec)> = std::thread::scope(|s| {
        let handles: Vec<_> = workspaces
            .iter_mut()
            .map(|ws| {
                let next = &next;
                s.spawn(move || {
                    let mut mine: Vec<(usize, PlayRec)> = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(task) = tasks.get(i) else { break };
                        mine.push((i, play_one(ctx, env, ws, task, overlay)));
                    }
                    mine
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().expect("fil de génération")).collect()
    });
    out.sort_by_key(|(i, _)| *i);
    out
}

/// Case de 0,05° du départ d'une tâche, pour la localité du cache de fenêtres.
pub fn locality_key(t: &Task) -> (usize, i64, i64) {
    (t.ride.view_index(), (t.req.start.lat / 0.05).floor() as i64, (t.req.start.lon / 0.05).floor() as i64)
}

#[cfg(test)]
mod spike_tests {
    use super::*;

    /// Polyligne densifiée : un point tous les 10 m au plus.
    fn dense(corners: &[[f64; 2]]) -> Vec<[f64; 2]> {
        let mut out = vec![corners[0]];
        for w in corners.windows(2) {
            let n = (ord_graph_match::plane::dist(w[0], w[1]) / 10.0).ceil().max(1.0) as usize;
            for k in 1..=n {
                let t = k as f64 / n as f64;
                out.push([w[0][0] + t * (w[1][0] - w[0][0]), w[0][1] + t * (w[1][1] - w[0][1])]);
            }
        }
        out
    }

    #[test]
    fn a_triangle_has_no_spike_and_a_comb_apex_has_one() {
        let tri = dense(&[[0.0, 0.0], [4000.0, 1500.0], [4000.0, -1500.0], [0.0, 0.0]]);
        assert_eq!(spikes_of(&tri, 500.0, 150.0), 0);
        // Une branche de 4 km sur laquelle on revient presque par le même chemin, à 30 m près : le sommet est un épi.
        let comb = dense(&[
            [0.0, 0.0],
            [4000.0, 0.0],
            [4000.0, 1500.0],
            [4030.0, 1500.0],
            [4030.0, 0.0],
            [0.0, 30.0],
            [0.0, 0.0],
        ]);
        assert!(spikes_of(&comb, 500.0, 150.0) >= 1, "{}", spikes_of(&comb, 500.0, 150.0));
        // Un aller-retour est un épi par nature (son sommet repart par où il est venu) : on ne compte que les triangles.
        assert_eq!(spikes_of(&dense(&[[0.0, 0.0], [5000.0, 0.0], [0.0, 0.0]]), 500.0, 150.0), 1);
    }
}
