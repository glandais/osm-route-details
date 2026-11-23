//! Mise à l'écart et surcouches reconstruites (§11.8 de la note).
//!
//! Les surcouches se refont par **le code du dépôt** (`Deposit::push_matched`), sans réapparier chaque trace : la
//! surcouche complète d'une équipe doit égaler, arête par arête et compte par compte, le fichier du lot 6 ; la
//! variante « écart » saute les boucles mises à l'écart.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{anyhow, Result};
use ord_graph::mmap::MappedGraph;
use ord_graph::overlay::{encode, Overlay, OverlayMeta};
use ord_graph::util::Poly;
use ord_graph_match::agg::params_hash;
use ord_graph_match::doc::TYPES;
use ord_graph_match::index::SegIndex;
use ord_graph_match::job::Job;
use ord_graph_match::profile::collect::{LearnSet, LoopCollector};
use ord_graph_match::Deposit;
use serde::Serialize;

use super::export::{Ingest, Loop};

/// Mise à l'écart, stable : pour chaque équipe d'au moins `min_loops` boucles retenues, ses boucles triées par
/// empreinte d'id de trace croissante, les `ceil(share × n)` premières sont mises à l'écart. Ne dépend que de
/// l'ensemble des boucles retenues de l'équipe : ni de l'ordre de lecture, ni des autres équipes, ni des fils.
/// Rend les indices (dans `loops`) des boucles mises à l'écart.
pub fn select(loops: &[Loop], share: f64, min_loops: usize) -> HashSet<usize> {
    let mut by_team: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for (i, l) in loops.iter().enumerate() {
        by_team.entry(l.team).or_default().push(i);
    }
    let mut out = HashSet::new();
    for (_, mut v) in by_team {
        if v.len() < min_loops {
            continue;
        }
        // Les empreintes sont distinctes ; à égalité (id en double), le rang dans l'équipe départage.
        v.sort_by_key(|&i| (loops[i].id_hash, loops[i].i));
        // `0,1 × 30` vaut 3,000 000 000 000 000 4 en flottant : sans la marge, `ceil` rendrait 4.
        let n = (share * v.len() as f64 - 1e-9).ceil().max(0.0) as usize;
        out.extend(v.into_iter().take(n));
    }
    out
}

/// Ce que le dépôt a compté pour une équipe.
#[derive(Clone, Debug, Serialize)]
pub struct OverlayCheck {
    pub team: String,
    /// Arêtes de la surcouche reconstruite.
    pub edges: usize,
    /// Kilomètres comptés par le dépôt.
    pub km_counted: f64,
    /// Arêtes de la variante « écart » (`None` : pas de mise à l'écart).
    pub holdout_edges: Option<usize>,
    /// Égalité au fichier du lot 6 : `None` si le fichier est absent ; sinon le nombre d'arêtes différentes (0 :
    /// identique).
    pub differing_edges: Option<usize>,
}

/// Surcouche d'une équipe par le dépôt, en sautant les traces de `skip` (indices dans `Ingest::tracks`), avec un
/// collecteur facultatif de boucles d'apprentissage (3b) : le dépôt les retient au passage, sur les traces non sautées. Les
/// tableaux de la surcouche sont les mêmes avec ou sans collecteur.
#[allow(clippy::too_many_arguments)]
pub fn build_overlay_learn(
    gf: &MappedGraph,
    seg: &SegIndex,
    poly: &Poly,
    ing: &Ingest,
    jobs: &[Job],
    team: usize,
    skip: &HashSet<usize>,
    collector: Option<LoopCollector>,
) -> Result<(Overlay, f64, Option<LearnSet>)> {
    let mut dep = match collector {
        Some(c) => Deposit::with_collector(gf, seg, poly, 1, c),
        None => Deposit::new(gf, seg, poly, 1),
    };
    for (idx, t) in ing.tracks.iter().enumerate() {
        if t.team != team || skip.contains(&idx) {
            continue;
        }
        let edges = t.readable.then(|| t.edges.iter().map(|e| (e.wk, e.trv, &jobs[e.key as usize])));
        dep.push_matched(t.ty, t.excluded, edges);
    }
    let (edges, counts, stats, learn) = dep.finish_learn();
    let meta = OverlayMeta {
        graph: gf.graph_identity(),
        team_id: 0,
        export_xxh3: ing.export_xxh3,
        export_rows: stats.tracks,
        params_hash: params_hash(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        build_epoch: 0,
        types: TYPES.iter().map(|t| (*t).to_string()).collect(),
    };
    let bytes = encode(&meta, &edges, &counts).map_err(|e| anyhow!("encodage de la surcouche : {e}"))?;
    let o = Overlay::decode(&bytes).map_err(|e| anyhow!("relecture de la surcouche : {e}"))?;
    Ok((o, stats.km_counted, learn))
}

/// Nombre d'arêtes dont les comptes diffèrent entre deux surcouches (absente d'un côté comptée comme différente).
pub fn differing_edges(a: &Overlay, b: &Overlay) -> usize {
    let (mut x, mut y) = (a.iter().peekable(), b.iter().peekable());
    let mut diff = 0;
    loop {
        match (x.peek(), y.peek()) {
            (None, None) => break,
            (Some(_), None) => {
                diff += 1;
                x.next();
            }
            (None, Some(_)) => {
                diff += 1;
                y.next();
            }
            (Some(p), Some(q)) => match p.0.cmp(&q.0) {
                std::cmp::Ordering::Less => {
                    diff += 1;
                    x.next();
                }
                std::cmp::Ordering::Greater => {
                    diff += 1;
                    y.next();
                }
                std::cmp::Ordering::Equal => {
                    diff += usize::from(p.1 != q.1);
                    x.next();
                    y.next();
                }
            },
        }
    }
    diff
}

/// Le fichier de surcouche d'une équipe dans `dir` : `<id>-….ordo`.
pub fn overlay_file(dir: &Path, team_id: u64) -> Option<std::path::PathBuf> {
    let prefix = format!("{team_id}-");
    std::fs::read_dir(dir).ok()?.filter_map(Result::ok).map(|e| e.path()).find(|p| {
        p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".ordo"))
    })
}

/// **Jumelles** d'une boucle mise à l'écart : la sortie de groupe enregistrée par plusieurs membres, ou le même
/// parcours roulé deux fois, reste dans la surcouche « écart » par l'autre trace, et la reconstruction retrouve alors
/// ce que l'équipe a déjà roulé, pas ce que le goût proposerait. Pour chaque boucle mise à l'écart : la plus grande
/// part de ses mètres `E(R)` (arêtes comptées par le dépôt, chacune à sa longueur) que prend une autre boucle
/// retenue de la même équipe **non** mise à l'écart ; `None` pour les autres boucles. Les traces non retenues (pas
/// une boucle, part appariée trop faible) sont aussi dans la surcouche et n'y sont pas regardées : c'est un minorant.
pub fn twin_shares(
    gf: &MappedGraph,
    loops: &[Loop],
    held: &HashSet<usize>,
    edges: &[std::collections::BTreeSet<u32>],
) -> Vec<Option<f64>> {
    use rayon::prelude::*;
    let len = |e: &u32| f64::from(gf.edge(*e).len_dm) / 10.0;
    (0..loops.len())
        .into_par_iter()
        .map(|h| {
            if !held.contains(&h) {
                return None;
            }
            let total: f64 = edges[h].iter().map(len).sum();
            if total <= 0.0 {
                return Some(0.0);
            }
            let best = (0..loops.len())
                .filter(|&k| k != h && loops[k].team == loops[h].team && !held.contains(&k))
                .map(|k| edges[h].intersection(&edges[k]).map(len).sum::<f64>())
                .fold(0.0, f64::max);
            Some(best / total)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::likeness::export::Loop;
    use crate::likeness::walk::{Trace, Walk};

    fn lp(team: usize, i: u32, id_hash: u64) -> Loop {
        Loop {
            team,
            i,
            track: 0,
            id_hash,
            ty: 0,
            walk: Walk { legs: Vec::new(), joins: Vec::new() },
            trace: Trace {
                pts: Vec::new(),
                cum: Vec::new(),
                join_arc: Vec::new(),
                leg_m: Vec::new(),
                d_graph: 0.0,
                holes_m: 0.0,
                max_hole_m: 0.0,
                l_poly: 0.0,
            },
            matched_share: 1.0,
            sc_share: None,
        }
    }

    /// Les empreintes mises à l'écart d'une équipe, triées.
    fn held(loops: &[Loop], sel: &HashSet<usize>, team: usize) -> Vec<u64> {
        let mut v: Vec<u64> =
            sel.iter().map(|&i| &loops[i]).filter(|l| l.team == team).map(|l| l.id_hash).collect();
        v.sort_unstable();
        v
    }

    #[test]
    fn the_holdout_is_the_ceil_of_the_share_by_hash_rank_and_is_stable() {
        let hashes: Vec<u64> = (0..23).map(|k| (k * 7_919 + 13) % 1_000).collect();
        let loops: Vec<Loop> = hashes.iter().enumerate().map(|(i, &h)| lp(0, i as u32, h)).collect();
        let sel = select(&loops, 0.10, 10);
        assert_eq!(sel.len(), 3, "ceil(0,10 × 23) = 3");
        let mut sorted = hashes.clone();
        sorted.sort_unstable();
        assert_eq!(held(&loops, &sel, 0), sorted[..3].to_vec(), "les plus petites empreintes");
        // Les mêmes boucles dans un autre ordre : les mêmes empreintes à l'écart.
        let mut shuffled: Vec<Loop> =
            hashes.iter().rev().enumerate().map(|(i, &h)| lp(0, i as u32, h)).collect();
        shuffled.rotate_left(5);
        let sel2 = select(&shuffled, 0.10, 10);
        assert_eq!(held(&shuffled, &sel2, 0), held(&loops, &sel, 0));
        // Une boucle ajoutée dans une autre équipe ne change rien.
        let mut more: Vec<Loop> = hashes.iter().enumerate().map(|(i, &h)| lp(0, i as u32, h)).collect();
        more.extend((0..12).map(|i| lp(1, i, 5_000 + u64::from(i))));
        let sel3 = select(&more, 0.10, 10);
        assert_eq!(held(&more, &sel3, 0), held(&loops, &sel, 0));
        assert_eq!(held(&more, &sel3, 1).len(), 2, "ceil(0,10 × 12) = 2");
        // Un multiple exact de 10 : 0,1 × 30 et 0,1 × 70 dépassent l'entier d'un ulp en flottant.
        for (n, want) in [(30, 3), (70, 7), (31, 4)] {
            let loops: Vec<Loop> = (0..n).map(|i| lp(0, i, u64::from(i))).collect();
            assert_eq!(select(&loops, 0.10, 10).len(), want, "ceil(0,10 × {n})");
        }
    }

    #[test]
    fn a_team_under_the_threshold_holds_nothing_out() {
        let loops: Vec<Loop> = (0..9).map(|i| lp(0, i, u64::from(i))).collect();
        assert!(select(&loops, 0.10, 10).is_empty());
        let loops: Vec<Loop> = (0..10).map(|i| lp(0, i, u64::from(i))).collect();
        assert_eq!(select(&loops, 0.10, 10).len(), 1);
    }

    #[test]
    fn a_twin_is_another_kept_loop_of_the_team_that_takes_the_same_metres() {
        use crate::likeness::fixture::{edge_between, graph, id};
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let e = |a: (i64, i64), b: (i64, i64)| edge_between(&gf, id(a.0, a.1), id(b.0, b.1)).0;
        let (e1, e2, e3, e4) = (e((0, 0), (0, 1)), e((0, 1), (1, 1)), e((1, 1), (1, 0)), e((1, 0), (0, 0)));
        let sets: Vec<std::collections::BTreeSet<u32>> = vec![
            [e1, e2, e3, e4].into(), // 0 : à l'écart
            [e1, e2, e3, e4].into(), // 1 : sa jumelle, dans la surcouche
            [e1, e2].into(), // 2 : à l'écart, couverte à moitié par 0 et 1, mais 0 est à l'écart aussi
            [e1, e2, e3, e4].into(), // 3 : même figure, autre équipe
        ];
        let loops = vec![lp(0, 0, 1), lp(0, 1, 2), lp(0, 2, 3), lp(1, 0, 4)];
        let held: HashSet<usize> = [0, 2].into();
        let tw = twin_shares(&gf, &loops, &held, &sets);
        assert_eq!(tw[1], None);
        assert_eq!(tw[3], None);
        assert!((tw[0].unwrap() - 1.0).abs() < 1e-12);
        assert!((tw[2].unwrap() - 1.0).abs() < 1e-12, "1 la prend entière");
        // Sans la jumelle 1 : 2 n'est couverte que par une boucle à l'écart, qui ne compte pas.
        let loops = vec![lp(0, 0, 1), lp(0, 2, 3)];
        let sets = vec![sets[0].clone(), sets[2].clone()];
        let tw = twin_shares(&gf, &loops, &[0, 1].into(), &sets);
        assert_eq!(tw, vec![Some(0.0), Some(0.0)]);
    }
}
