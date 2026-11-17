//! Le chemin du service au banc (lot 4, 3b, §14.12) : le joueur du banc (`BenchPlayer`), l'apprentissage par équipe avec le
//! **code du dépôt** (`ord_graph_match::profile::learn`) et ce que `svc` écrit sur lui-même. **Lettres et agrégats
//! seulement.**

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use memmap2::Mmap;
use ord_graph::mmap::MappedGraph;
use ord_graph_match::profile::collect::Cap;
use ord_graph_match::profile::learn::{Common, LearnParams, Learned};
use ord_graph_match::profile::{first_kept_class_meters, Player, Stopped};
use ord_loops::{
    generate_with_diagnostics_in, Failure, Request, Taste, TeamCtx, TeamData, Workspace, TASTE_CLASSES,
};
use ord_loops_check::Checker;
use serde::Serialize;

use super::play::{Eng, SharedWindows};
use super::variants::{Svc, SvcCap};

/// Première clé d'équipe des pas d'apprentissage : au-delà des `2t` et `2t + 1` des jeux (une clé neuve par pas).
const FIRST_LEARN_KEY: u64 = 1 << 32;

/// Joue par `SharedWindows`, comme les jeux du banc : même moteur, mêmes fenêtres, même contrôleur ; la clé d'équipe est
/// neuve à chaque pas (le goût change les agrégats de l'équipe) et `drop_team` la lâche ensuite.
pub struct BenchPlayer<'a> {
    pub engine: &'a Eng,
    pub gf: &'a MappedGraph,
    pub wins: &'a SharedWindows<'a>,
    pub team: &'a TeamData,
    pub checker: &'a Checker<'a, Mmap>,
    pub threads: usize,
    pub next_key: &'a AtomicU64,
    /// Nanosecondes cumulées des jeux (temps de calcul des pas).
    pub cpu_ns: &'a AtomicU64,
}

impl std::fmt::Debug for BenchPlayer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BenchPlayer").field("threads", &self.threads).finish_non_exhaustive()
    }
}

impl BenchPlayer<'_> {
    pub fn fresh_keys() -> AtomicU64 {
        AtomicU64::new(FIRST_LEARN_KEY)
    }

    fn play_one(
        &self,
        ws: &mut Workspace,
        key: u64,
        req: &Request,
        taste: &Taste,
    ) -> Option<[f64; TASTE_CLASSES]> {
        let call = |ws: &mut Workspace, win: &ord_loops::Window, wt: &ord_loops::WindowTeam| {
            catch_unwind(AssertUnwindSafe(|| {
                let tc = TeamCtx { data: self.team, win: wt };
                generate_with_diagnostics_in(self.engine, win, Some(tc), req, ws, &mut || false).0
            }))
        };
        let (win, wt) = self.wins.acquire(key, self.team, req, Some(taste));
        let mut res = call(ws, &win, &wt);
        drop((win, wt));
        if matches!(res, Ok(Err(Failure::WindowEscape))) {
            let (win, wt) = self.wins.acquire_wide(self.team, req, Some(taste));
            res = call(ws, &win, &wt);
        }
        match res {
            Ok(Ok(g)) => first_kept_class_meters(self.gf, self.checker, req, &g),
            _ => None,
        }
    }
}

impl Player for BenchPlayer<'_> {
    fn play_all(
        &self,
        reqs: &[Request],
        taste: &Taste,
    ) -> Result<Vec<Option<[f64; TASTE_CLASSES]>>, Stopped> {
        let key = self.next_key.fetch_add(1, Ordering::Relaxed);
        let next = AtomicUsize::new(0);
        let mut out: Vec<(usize, Option<[f64; TASTE_CLASSES]>)> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..self.threads.max(1))
                .map(|_| {
                    let next = &next;
                    s.spawn(move || {
                        let mut ws = Workspace::new();
                        let mut mine = Vec::new();
                        loop {
                            let i = next.fetch_add(1, Ordering::Relaxed);
                            let Some(req) = reqs.get(i) else { break };
                            let t = Instant::now();
                            let r = self.play_one(&mut ws, key, req, taste);
                            self.cpu_ns.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
                            mine.push((i, r));
                        }
                        mine
                    })
                })
                .collect();
            hs.into_iter().flat_map(|h| h.join().expect("fil d'apprentissage")).collect()
        });
        out.sort_by_key(|(i, _)| *i);
        self.wins.drop_team(key);
        Ok(out.into_iter().map(|(_, r)| r).collect())
    }
}

/// Le plafond d'une variante, vu par l'apprentissage.
pub fn cap_of(svc: &Svc) -> Cap {
    match svc.cap {
        None => LearnParams::default().cap,
        Some(SvcCap::Team(0)) => Cap::None,
        Some(SvcCap::Team(n)) => Cap::Team(n),
        Some(SvcCap::PerType(n)) => Cap::PerType(n),
    }
}

/// Un pas d'une équipe : les agrégats du diagnostic.
#[derive(Clone, Debug, Serialize)]
pub struct SvcStep {
    pub step: u8,
    pub ty: String,
    pub weight: f64,
    pub l1_gap: f64,
    pub mean_mult: f64,
}

/// Ce que l'apprentissage d'une équipe a coûté et rendu.
#[derive(Clone, Debug, Serialize)]
pub struct SvcTeam {
    pub team: String,
    /// Boucles gardées pour l'apprentissage, par type (ROAD, GRAVEL, MTB).
    pub loops: [u32; 3],
    pub steps_done: u8,
    pub plays: u64,
    pub learn_wall_s: f64,
    /// Somme des durées des jeux, s.
    pub learn_cpu_s: f64,
    /// `learn_cpu_s / 4` : l'apprentissage sur 4 fils [calculé].
    pub est_4_threads_s: f64,
    pub steps: Vec<SvcStep>,
}

/// Ce que `common` et `svc` écrivent sur eux-mêmes.
#[derive(Clone, Debug, Serialize)]
pub struct SvcMeta {
    /// `Common::hash` et `LearnParams::hash` (16 hexadécimaux).
    pub common: String,
    pub learn: String,
    pub cap: String,
    pub normalize: bool,
    pub teams: Vec<SvcTeam>,
}

/// Les réglages d'apprentissage d'une variante.
pub fn learn_params(
    svc: &Svc,
    retain: ord_graph_match::profile::collect::RetainParams,
    cursors: [u32; 2],
    seed: u64,
) -> LearnParams {
    LearnParams { normalize: svc.norm, cap: cap_of(svc), cursors, seed, retain, ..LearnParams::default() }
}

/// Résume un apprentissage.
pub fn team_meta(letter: &str, set_loops: [u32; 3], l: &Learned, wall_s: f64, cpu_s: f64) -> SvcTeam {
    SvcTeam {
        team: letter.to_string(),
        loops: set_loops,
        steps_done: l.steps_done,
        plays: l.plays,
        learn_wall_s: wall_s,
        learn_cpu_s: cpu_s,
        est_4_threads_s: cpu_s / 4.0,
        steps: l
            .diag
            .iter()
            .map(|d| SvcStep {
                step: d.step,
                ty: super::out::TYPE_NAMES[usize::from(d.ty).min(2)].to_string(),
                weight: d.weight,
                l1_gap: d.l1_gap,
                mean_mult: d.mean_mult,
            })
            .collect(),
    }
}

/// `Common` du moteur, brute ou normalisée.
pub fn common_of(norm: bool) -> Common {
    Common::engine(norm)
}

#[cfg(test)]
mod tests {
    use ord_graph_match::profile::collect::{LearnLoop, LearnSet};
    use ord_loops::Mode;

    use super::super::chain::{step72, ParamState};
    use super::super::taste::{ratio_table, TasteParams};
    use super::super::variants::SvcKind;
    use super::*;

    /// Un joueur jouet : `gen[c] = avail[c] · mult^-0,8`, à 1 000 m par requête.
    struct Toy;

    impl Player for Toy {
        fn play_all(
            &self,
            reqs: &[Request],
            taste: &Taste,
        ) -> Result<Vec<Option<[f64; TASTE_CLASSES]>>, Stopped> {
            Ok(reqs
                .iter()
                .map(|r| {
                    let t = r.ride_type.index();
                    let mut out = [0.0; TASTE_CLASSES];
                    for (c, o) in out.iter_mut().enumerate().take(8) {
                        *o = (1.0 + c as f64 * 0.4) * (f64::from(taste.q8[t][c]) / 256.0).powf(-0.8);
                    }
                    let s: f64 = out.iter().sum();
                    out.iter_mut().for_each(|o| *o *= 1_000.0 / s);
                    Some(out)
                })
                .collect())
        }
    }

    /// Test 10c : table commune neutre, `n0 = 0`, un pas : les `q8` de `step72(ratio_table(min_km = 0))` du banc.
    #[test]
    fn one_step_on_a_neutral_common_with_no_fallback_is_the_benchs_step72() {
        let loops: Vec<LearnLoop> = (0..12)
            .map(|i| {
                let mut real_m = [0.0; TASTE_CLASSES];
                for (c, r) in real_m.iter_mut().enumerate().take(8) {
                    *r = 50.0 + ((i * 7 + c * 13) % 90) as f64;
                }
                LearnLoop {
                    key: i as u64,
                    ty: 0,
                    start: (47.0, -1.5),
                    bearing: None,
                    distance_m: 20_000.0,
                    real_m,
                }
            })
            .collect();
        let set = LearnSet { loops, counts: Default::default() };
        let common = Common { q8: [[256; TASTE_CLASSES]; 3], gen_dm_per_loop: [[0; TASTE_CLASSES]; 3] };
        let cursors = [(Mode::TeamFirst, 0.5f32, 640u32), (Mode::TeamFirst, 0.5, 256)];
        let p = LearnParams { n0: 0.0, steps: 1, normalize: false, ..LearnParams::default() };
        let out = ord_graph_match::profile::learn::learn(&set, &cursors, &Toy, &common, &p);
        // Le calcul du banc, sur les mêmes mètres.
        let (mut real, mut gen) = (vec![0.0; TASTE_CLASSES], vec![0.0; TASTE_CLASSES]);
        let neutral = Taste::neutral();
        for l in &set.loops {
            for &(mode, fam, q) in &cursors {
                let req = ord_graph_match::profile::collect::loop_request(
                    l.start,
                    l.bearing,
                    l.distance_m,
                    l.ty,
                    mode,
                    fam,
                    q,
                    1,
                );
                let g = Toy.play_all(&[req], &neutral).unwrap()[0].unwrap();
                for c in 0..TASTE_CLASSES {
                    real[c] += l.real_m[c];
                    gen[c] += g[c];
                }
            }
        }
        let tp = TasteParams { min_km: 0.0, ..TasteParams::default() };
        let (_, g) = ratio_table(&real, &gen, 1e9, 0.5, &tp);
        let mut ps = ParamState::new(super::super::variants::Dims::D72, 0.5);
        step72(&mut ps, &g, 4.0);
        assert_eq!(out.taste.q8[0].to_vec(), ps.q8());
        assert_eq!(out.steps_done, 1);
    }

    #[test]
    fn cap_of_follows_the_variant_and_the_default_is_the_services() {
        let sv = |cap| Svc { kind: SvcKind::Svc, norm: false, cap };
        assert_eq!(cap_of(&sv(None)), Cap::Team(500));
        assert_eq!(cap_of(&sv(Some(SvcCap::Team(0)))), Cap::None);
        assert_eq!(cap_of(&sv(Some(SvcCap::Team(300)))), Cap::Team(300));
        assert_eq!(cap_of(&sv(Some(SvcCap::PerType(500)))), Cap::PerType(500));
    }
}
