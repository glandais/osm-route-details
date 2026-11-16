//! Construction de référence des tables du contrôleur (finition 2 du lot 6, §20.4) : celle d'avant la construction
//! creuse, **telle quelle** : l'incidence complète des cellules de la zone, un type à la fois. Compilée sous
//! `cfg(test)` et sous la fonctionnalité `reference` ; les tests et le banc (`--teams-reference`) exigent les mêmes
//! octets canoniques que la construction creuse.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use ord_graph::format::GraphFile;
use ord_graph::overlay::Overlay;
use ord_loops::RideType;

use crate::bounded::{edges_at, incidence_of, GraphIndex, TeamCheck};
use crate::check::Config;
use crate::rules::{team_edge, Admission, TypeIndex};

impl TeamCheck {
    /// Construit à l'ancienne, sur les cellules de la zone de l'équipe.
    pub fn build_reference<B: AsRef<[u8]>>(
        g: &GraphFile<B>,
        gi: &GraphIndex,
        overlay: &Overlay,
        cfg: &Config,
    ) -> Self {
        let cells = crate::bounded::zone_cells(g, overlay).0;
        let types = TypeIndex::of(overlay);
        let mut out = Self::default();
        for ride in [RideType::Road, RideType::Gravel, RideType::Mtb] {
            out.set_bridged(ride, bridged_edges(g, gi, overlay, &types, cfg, ride, &cells));
        }
        out
    }
}

/// Les arêtes comblées en strict, recalculées d'après la surcouche et la règle d'admission (note lot 2 §2.3) :
/// (1) le plus court chemin non roulé de `hole_m` au plus, d'un cul-de-sac fictif (un seul bout d'arête d'équipe, au
/// moins deux arêtes admises) au plus proche nœud d'équipe ; (2) tout chemin non roulé de `connector_m` au plus entre
/// deux nœuds d'équipe. Un parcours ne passe que par ces arêtes hors équipe.
fn bridged_edges<B: AsRef<[u8]>>(
    g: &GraphFile<B>,
    gi: &GraphIndex,
    o: &Overlay,
    types: &TypeIndex,
    cfg: &Config,
    ride: RideType,
    cells: &[usize],
) -> HashSet<u32> {
    let mut out = HashSet::new();
    let adm = Admission::of(ride);
    let inc = incidence_of(g, gi, adm, cells);
    let mut team: HashSet<u32> = HashSet::new();
    let mut tdeg: HashMap<u32, u8> = HashMap::new();
    for (e, counts) in o.iter() {
        if team_edge(ride, g.edge(e).attrs, Some(counts), types) {
            team.insert(e);
            let (a, b) = g.edge_ends(e);
            for n in [a, b] {
                let d = tdeg.entry(n).or_insert(0);
                *d = d.saturating_add(1);
            }
        }
    }
    let len_dm = |e: u32| g.edge(e).len_dm;
    // Degré admis d'un nœud, une boucle comptant deux fois.
    let adm_deg = |n: u32| -> u32 {
        edges_at(&inc, n)
            .map(|e| {
                let (a, b) = g.edge_ends(e);
                1 + u32::from(a == b)
            })
            .sum()
    };
    // Dijkstra borné sur les arêtes non roulées ; `all` : tous les nœuds d'équipe atteints, sinon le premier.
    let walk = |s: u32, limit_dm: u32, all: bool, out: &mut HashSet<u32>| {
        let mut dist: HashMap<u32, (u32, u32)> = HashMap::new();
        let mut heap = BinaryHeap::new();
        dist.insert(s, (0, u32::MAX));
        heap.push(Reverse((0u32, s)));
        while let Some(Reverse((d, x))) = heap.pop() {
            if dist[&x].0 < d {
                continue;
            }
            if x != s && tdeg.get(&x).is_some_and(|&t| t >= 1) {
                let mut cur = x;
                while cur != s {
                    let (_, pe) = dist[&cur];
                    out.insert(pe);
                    let (a, b) = g.edge_ends(pe);
                    cur = if a == cur { b } else { a };
                }
                if all {
                    continue;
                }
                break;
            }
            for be in edges_at(&inc, x) {
                if team.contains(&be) {
                    continue;
                }
                let nd = d + len_dm(be);
                if nd > limit_dm {
                    continue;
                }
                let (a, b) = g.edge_ends(be);
                let y = if a == x { b } else { a };
                if dist.get(&y).is_none_or(|&(od, _)| nd < od) {
                    dist.insert(y, (nd, be));
                    heap.push(Reverse((nd, y)));
                }
            }
        }
    };
    let hole_dm = (cfg.hole_m * 10.0) as u32;
    let conn_dm = (cfg.connector_m * 10.0) as u32;
    let mut nodes: Vec<u32> = tdeg.keys().copied().collect();
    nodes.sort_unstable();
    for &n in &nodes {
        if tdeg[&n] == 1 && adm_deg(n) >= 2 {
            walk(n, hole_dm, false, &mut out);
        }
    }
    for &n in &nodes {
        if edges_at(&inc, n).any(|e| !team.contains(&e) && len_dm(e) <= conn_dm) {
            walk(n, conn_dm, true, &mut out);
        }
    }
    out
}
