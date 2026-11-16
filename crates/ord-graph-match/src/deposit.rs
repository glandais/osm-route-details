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
            return;
        }
        let Some(edges) = edges else {
            self.stats.unreadable += 1;
            return;
        };
        self.covs.clear();
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
            let trv = if e.trv.is_nan() { job.glen } else { e.trv };
            let km = f64::from(trv) / 1000.0;
            self.stats.team_edges += 1;
            self.stats.km_total += km;
            match verdict(e.wk, &job) {
                Verdict::Outside => self.stats.km_outside += km,
                Verdict::Fail { cause, .. } => self.stats.km_unmatched[cause] += km,
                Verdict::Counted(chain) => {
                    self.stats.km_matched += km;
                    push_chain_covs(self.gf, chain, &mut self.covs);
                }
            }
        }
        let (counted, below) = count_track(&mut self.covs, usize::from(ty), &mut self.counts);
        self.stats.km_counted += counted;
        self.stats.km_below_threshold += below;
    }

    /// Arêtes triées, comptes à plat (voir `agg::overlay_arrays`) et statistiques.
    pub fn finish(self) -> (Vec<u32>, Vec<u16>, DepositStats) {
        let (edges, counts) = crate::agg::overlay_arrays(&self.counts);
        (edges, counts, self.stats)
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
