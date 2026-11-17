//! Dépôt d'une équipe : trace après trace, apparie, agrège dans l'ordre d'arrivée et lâche le texte. Mémoire bornée :
//! le document d'une trace n'est gardé que le temps de son traitement ; restent la mémoïsation par géométrie (bornée,
//! deux générations) et le compte par arête de base.
//!
//! Le même jugement et le même comptage que l'outil (`agg`) : l'agrégation est séquentielle, donc déterministe.

use std::collections::HashMap;

use ord_graph::mmap::MappedGraph;
use ord_graph::util::Poly;
use rustc_hash::FxHashMap;
use xxhash_rust::xxh3::xxh3_64;

use crate::agg::{count_track, push_chain_covs, verdict, Cov, EdgeAcc, Verdict, CAUSES};
use crate::doc::ParsedEdge;
use crate::index::Near;
use crate::job::{match_job, Job};
use crate::matcher::Piece;
use crate::profile::collect::{LearnSet, LoopCollector};

/// Clé de mémoïsation : empreinte et longueur de la polyligne, `trv` annoncé (bits). Une même géométrie annoncée avec
/// deux `trv` différents s'apparie deux fois.
type MemoKey = (u64, u32, u32);

/// Deux générations : la courante se remplit jusqu'à `cap`, puis devient l'ancienne et l'ancienne est lâchée.
/// Une éviction ne coûte que du calcul, jamais un résultat différent.
#[derive(Debug)]
struct Memo {
    cap: usize,
    cur: HashMap<MemoKey, Job>,
    old: HashMap<MemoKey, Job>,
}

impl Memo {
    fn new(entries: usize) -> Self {
        Self { cap: (entries / 2).max(1), cur: HashMap::new(), old: HashMap::new() }
    }

    fn len(&self) -> usize {
        self.cur.len() + self.old.len()
    }

    fn get(&mut self, k: &MemoKey) -> Option<Job> {
        if let Some(j) = self.cur.get(k) {
            return Some(j.clone());
        }
        let j = self.old.remove(k)?;
        self.put(*k, j.clone());
        Some(j)
    }

    fn put(&mut self, k: MemoKey, j: Job) {
        if self.cur.len() >= self.cap {
            self.old = std::mem::take(&mut self.cur);
        }
        self.cur.insert(k, j);
    }
}

/// Ce qu'un dépôt a vu, sans identifiant ni coordonnée.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DepositStats {
    /// Lignes lues (traces).
    pub tracks: u64,
    /// Traces exclues du générateur (`excluded`).
    pub excluded: u64,
    /// Traces dont le document est absent ou illisible.
    pub unreadable: u64,
    pub team_edges: u64,
    pub km_total: f64,
    /// Hors de la région.
    pub km_outside: f64,
    /// Appariés, retenus.
    pub km_matched: f64,
    /// Comptés dans la surcouche (une fois par parcours, par arête et par sens, au seuil de couverture).
    pub km_counted: f64,
    pub km_below_threshold: f64,
    /// Kilomètres non appariés, par cause ([`CAUSES`]).
    pub km_unmatched: [f64; CAUSES.len()],
    /// Appariements calculés / retrouvés dans la mémoïsation.
    pub memo_misses: u64,
    pub memo_hits: u64,
}

pub struct Deposit<'a> {
    gf: &'a MappedGraph,
    idx: &'a dyn Near,
    poly: &'a Poly,
    memo: Memo,
    counts: FxHashMap<u32, EdgeAcc>,
    covs: Vec<Cov>,
    stats: DepositStats,
    /// Retenue des boucles d'apprentissage d'un profil (lot 4, 3b) ; `None` : le dépôt d'avant, octet pour octet.
    learn: Option<Learning>,
}

/// Ce que le dépôt accumule d'une trace pour le collecteur : exactement `export::track_match` du banc.
struct Learning {
    collector: LoopCollector,
    total_m: f64,
    matched_m: f64,
    pieces: Vec<Piece>,
}

impl std::fmt::Debug for Deposit<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Deposit")
            .field("memo", &self.memo.len())
            .field("edges", &self.counts.len())
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

impl<'a> Deposit<'a> {
    pub fn new(gf: &'a MappedGraph, idx: &'a dyn Near, poly: &'a Poly, memo_entries: usize) -> Self {
        Self {
            gf,
            idx,
            poly,
            memo: Memo::new(memo_entries),
            counts: FxHashMap::default(),
            covs: Vec::new(),
            stats: DepositStats::default(),
            learn: None,
        }
    }

    /// Comme [`Deposit::new`], avec un collecteur : chaque trace appariée lui est proposée à sa fin (§14.4.2). Les
    /// tableaux de la surcouche sont les mêmes, octet pour octet, qu'avec [`Deposit::new`].
    pub fn with_collector(
        gf: &'a MappedGraph,
        idx: &'a dyn Near,
        poly: &'a Poly,
        memo_entries: usize,
        collector: LoopCollector,
    ) -> Self {
        let mut d = Self::new(gf, idx, poly, memo_entries);
        d.learn = Some(Learning { collector, total_m: 0.0, matched_m: 0.0, pieces: Vec::new() });
        d
    }

    /// Une trace sans arêtes utilisables (exclue, illisible) : le collecteur en tient le décompte.
    fn offer_refused(&mut self, ty: u8, excluded: bool, readable: bool) {
        if let Some(l) = &mut self.learn {
            l.collector.offer(self.gf, ty, excluded, readable, 0.0, 0.0, &[]);
        }
    }

    /// Entrées gardées dans la mémoïsation (jamais plus de `memo_entries` + une génération).
    pub fn memo_len(&self) -> usize {
        self.memo.len()
    }

    /// Une trace : son type (`doc::type_code`), son drapeau « exclue du générateur » et ses arêtes (`None` : document
    /// illisible).
    pub fn push_track(&mut self, ty: u8, excluded: bool, edges: Option<Vec<ParsedEdge>>) {
        self.stats.tracks += 1;
        if excluded {
            self.stats.excluded += 1;
            self.offer_refused(ty, true, edges.is_some());
            return;
        }
        let Some(edges) = edges else {
            self.stats.unreadable += 1;
            self.offer_refused(ty, false, false);
            return;
        };
        self.begin_track();
        for e in edges {
            let key = (xxh3_64(e.g.as_bytes()), e.g.len() as u32, e.trv.to_bits());
            let job = match self.memo.get(&key) {
                Some(j) => {
                    self.stats.memo_hits += 1;
                    j
                }
                None => {
                    self.stats.memo_misses += 1;
                    let j = match_job(self.gf, self.idx, self.poly, &e.g, e.trv);
                    self.memo.put(key, j.clone());
                    j
                }
            };
            self.add_edge(e.wk, e.trv, &job);
        }
        self.end_track(ty);
    }

    /// Comme [`Deposit::push_track`], pour une trace dont chaque géométrie a déjà été appariée par [`match_job`]
    /// (pure : même résultat que la mémoïsation, qui n'est donc pas utilisée). `edges` : `(wk, trv annoncé,
    /// appariement)` dans l'ordre de la trace ; `None` : document illisible. Les compteurs `memo_hits` et
    /// `memo_misses` ne bougent pas.
    pub fn push_matched<'j, I>(&mut self, ty: u8, excluded: bool, edges: Option<I>)
    where
        I: IntoIterator<Item = (u8, f32, &'j Job)>,
    {
        self.stats.tracks += 1;
        if excluded {
            self.stats.excluded += 1;
            self.offer_refused(ty, true, edges.is_some());
            return;
        }
        let Some(edges) = edges else {
            self.stats.unreadable += 1;
            self.offer_refused(ty, false, false);
            return;
        };
        self.begin_track();
        for (wk, trv, job) in edges {
            self.add_edge(wk, trv, job);
        }
        self.end_track(ty);
    }

    fn begin_track(&mut self) {
        self.covs.clear();
        if let Some(l) = &mut self.learn {
            (l.total_m, l.matched_m) = (0.0, 0.0);
            l.pieces.clear();
        }
    }

    /// Une arête d'équipe de famille `wk`, de `trv` mètres annoncés (`NaN` : repli sur la longueur de la géométrie),
    /// dont la géométrie a donné `job`.
    fn add_edge(&mut self, wk: u8, trv: f32, job: &Job) {
        let trv = if trv.is_nan() { job.glen } else { trv };
        let km = f64::from(trv) / 1000.0;
        self.stats.team_edges += 1;
        self.stats.km_total += km;
        if let Some(l) = &mut self.learn {
            l.total_m += f64::from(trv);
        }
        match verdict(wk, job) {
            Verdict::Outside => self.stats.km_outside += km,
            Verdict::Fail { cause, .. } => self.stats.km_unmatched[cause] += km,
            Verdict::Counted(chain) => {
                self.stats.km_matched += km;
                push_chain_covs(self.gf, chain, &mut self.covs);
                if let Some(l) = &mut self.learn {
                    l.matched_m += f64::from(trv);
                    l.pieces.extend_from_slice(&chain.pieces);
                }
            }
        }
    }

    /// Fin d'une trace de type `ty` : compte ses recouvrements, une fois par arête et par sens.
    fn end_track(&mut self, ty: u8) {
        let (counted, below) = count_track(&mut self.covs, usize::from(ty), &mut self.counts);
        self.stats.km_counted += counted;
        self.stats.km_below_threshold += below;
        if let Some(l) = &mut self.learn {
            l.collector.offer(self.gf, ty, false, true, l.total_m, l.matched_m, &l.pieces);
            l.pieces.clear();
        }
    }

    /// Arêtes triées, comptes à plat (voir `agg::overlay_arrays`) et statistiques.
    pub fn finish(self) -> (Vec<u32>, Vec<u16>, DepositStats) {
        let (edges, counts, stats, _) = self.finish_learn();
        (edges, counts, stats)
    }

    /// Comme [`Deposit::finish`], et les boucles retenues si le dépôt a un collecteur.
    pub fn finish_learn(self) -> (Vec<u32>, Vec<u16>, DepositStats, Option<LearnSet>) {
        let (edges, counts) = crate::agg::overlay_arrays(&self.counts);
        (edges, counts, self.stats, self.learn.map(|l| l.collector.finish()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u64) -> MemoKey {
        (n, 1, 0)
    }

    fn job() -> Job {
        Job { region: crate::job::Region::Inside, glen: 1.0, outcome: crate::job::Outcome::Invalid }
    }

    #[test]
    fn memo_stays_within_two_generations_and_evicts_the_oldest() {
        let mut m = Memo::new(10); // 5 par génération
        for n in 0..100 {
            m.put(key(n), job());
            assert!(m.len() <= 10, "{}", m.len());
        }
        assert!(m.get(&key(99)).is_some(), "le plus récent reste");
        assert!(m.get(&key(0)).is_none(), "le plus ancien est évincé");
        // Une entrée de l'ancienne génération relue repasse dans la courante.
        let old = m.old.keys().next().copied().expect("une ancienne génération");
        assert!(m.get(&old).is_some());
        assert!(m.cur.contains_key(&old));
    }
}

#[cfg(test)]
mod learn_tests {
    use ord_graph::util::Poly;

    use super::*;
    use crate::fixture::graph;
    use crate::geo::encode_polyline6;
    use crate::index::SegIndex;
    use crate::job::{Outcome, Region};
    use crate::matcher::Chain;
    use crate::profile::collect::tests::{all_laps, lap, square};
    use crate::profile::collect::{Cap, RetainParams};

    fn collector() -> LoopCollector {
        LoopCollector::new(RetainParams { min_km: 1.0, ..RetainParams::default() }, Cap::None)
    }

    fn job_of(pieces: Vec<Piece>) -> Job {
        Job {
            region: Region::Inside,
            glen: 1_200.0,
            outcome: Outcome::Matched(Chain {
                pieces,
                hausdorff: 0.0,
                road_m: 1_200.0,
                nonroad_m: 0.0,
                closed_m: 0.0,
            }),
        }
    }

    #[test]
    fn a_collector_changes_nothing_in_the_overlay_arrays_and_the_two_entry_points_agree() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let idx = SegIndex::build(&gf);
        let poly =
            Poly::parse("france\n1\n -4.0 45.0\n 2.0 45.0\n 2.0 49.0\n -4.0 49.0\nEND\nEND\n").unwrap();
        let laps = all_laps(&gf);
        let jobs: Vec<Job> = laps.iter().take(6).cloned().map(job_of).collect();
        let run = |with: bool| {
            let mut d = if with {
                Deposit::with_collector(&gf, &idx, &poly, 16, collector())
            } else {
                Deposit::new(&gf, &idx, &poly, 16)
            };
            for (i, j) in jobs.iter().enumerate() {
                d.push_matched(0, false, Some([(0u8, 1_200.0f32, j)]));
                if i == 2 {
                    d.push_matched::<std::iter::Empty<(u8, f32, &Job)>>(0, true, None);
                    d.push_matched::<std::iter::Empty<(u8, f32, &Job)>>(0, false, None);
                }
            }
            d.finish_learn()
        };
        let (e0, c0, s0, none) = run(false);
        let (e1, c1, s1, set) = run(true);
        assert!(none.is_none());
        assert_eq!((e0, c0, s0), (e1, c1, s1));
        let set = set.unwrap();
        assert_eq!(set.loops.len(), 6);
        assert_eq!(set.counts.offered, 8);
        assert_eq!(set.counts.refused[0], 1, "exclue");
        assert_eq!(set.counts.refused[1], 1, "illisible");
    }

    #[test]
    fn push_track_and_push_matched_give_the_same_learn_set() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let idx = SegIndex::build(&gf);
        let poly =
            Poly::parse("france\n1\n -4.0 45.0\n 2.0 45.0\n 2.0 49.0\n -4.0 49.0\nEND\nEND\n").unwrap();
        // La géométrie d'un tour de carré, arête par arête, en polyligne de précision 6.
        let geom = |nodes: [i64; 4]| -> String {
            let mut pts: Vec<[f64; 2]> = Vec::new();
            for k in 0..=4 {
                let (_, n) = crate::fixture::edge_between(&gf, nodes[k % 4], nodes[(k + 1) % 4]);
                let _ = n;
                let e = crate::fixture::edge_between(&gf, nodes[k % 4], nodes[(k + 1) % 4]).0;
                let (a, b) = gf.edge_ends(e);
                let want = nodes[k % 4];
                let node = if gf.identity(e).1 as i64 == want { a } else { b };
                let p = gf.node_pos(node);
                pts.push([f64::from(p.0) * 1e-7, f64::from(p.1) * 1e-7]);
            }
            encode_polyline6(&pts)
        };
        let g = geom(square(0, 0));
        let tracks: Vec<String> = vec![g.clone(), g];
        let mut a = Deposit::with_collector(&gf, &idx, &poly, 16, collector());
        let mut b = Deposit::with_collector(&gf, &idx, &poly, 16, collector());
        for t in &tracks {
            a.push_track(
                0,
                false,
                Some(vec![crate::doc::ParsedEdge {
                    g: t.clone(),
                    trv: 1_200.0,
                    len: 1_200.0,
                    fm: f32::NAN,
                    tm: f32::NAN,
                    from_off: f32::NAN,
                    to_off: f32::NAN,
                    from_node: -1,
                    to_node: -1,
                    wk: 0,
                    ow: 0,
                }]),
            );
            let job = match_job(&gf, &idx, &poly, t, 1_200.0);
            b.push_matched(0, false, Some([(0u8, 1_200.0f32, &job)]));
        }
        let (sa, sb) = (a.finish_learn().3.unwrap(), b.finish_learn().3.unwrap());
        assert_eq!(sa, sb);
        let _ = lap;
        assert_eq!(sa.counts.offered, 2);
    }
}
