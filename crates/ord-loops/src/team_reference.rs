//! Construction de référence des données d'équipe (finition 2 du lot 6, §20.4) : celle d'avant la construction creuse,
//! **telle quelle**. Les fenêtres de toute la zone (`build_window_opts` sans poids), l'incidence de toutes leurs
//! arêtes vivantes (`ZoneView`) et les composantes fortement connexes de toute la fenêtre. Elle garde la preuve
//! d'identité du §20.3 testable : les tests et le banc (`--teams-reference`) construisent chaque équipe des deux
//! façons et exigent les mêmes octets. Compilée sous `cfg(test)` et sous la fonctionnalité `reference` ; le service
//! ne l'embarque pas.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use ord_graph::attrs::HighwayClass;
use ord_graph::format::DIR_BIT;
use ord_graph::overlay::Overlay;
use rustc_hash::FxHashMap;

use crate::cost::{is_greenway, RideType};
use crate::engine::{chain_strong_len, strong_components, strong_lengths, Engine, EngineError, ViewData};
use crate::team::{rough_surface, type_index, TeamData, TypeTeam, BRIDGED, TEAM};
use crate::window::Window;

impl TeamData {
    /// Construit à l'ancienne, sur la zone `zone` (ou sur toutes les tuiles : `None`, le moteur à plat).
    pub(crate) fn build_reference_zone<B: AsRef<[u8]>>(
        e: &Engine<B>,
        overlay: &Overlay,
        zone: Option<&[u32]>,
    ) -> Result<Self, EngineError> {
        build_zone(e, overlay, zone)
    }
}

fn build_zone<B: AsRef<[u8]>>(
    e: &Engine<B>,
    overlay: &Overlay,
    zone: Option<&[u32]>,
) -> Result<TeamData, EngineError> {
    let mut views: Vec<(Window, ZoneView)> = Vec::new();
    for vi in 0..2 {
        let win = match zone {
            Some(z) => e.build_window_opts(vi, z, false, &mut || false).expect("jamais coupée"),
            None => e.full_window(vi),
        };
        let zv = ZoneView::new(e, &win.v);
        views.push((win, zv));
    }
    let per = RideType::ALL.map(|r| {
        let (win, zv) = &views[r.view_index()];
        build_type(e, overlay, r, &win.v, zv)
    });
    Ok(TeamData { per, retried: false })
}

/// Incidence d'une arête de base vivante en un nœud de base.
#[derive(Clone, Copy)]
struct Inc {
    node: u32,
    edge: u32,
    other: u32,
    len: u32,
}

/// Ce qu'il faut de la vue d'une zone pour la règle des trous : les arêtes de base vivantes (liens des chaînes) et
/// leur incidence par nœud de base, dans l'ordre croissant des arêtes (§7.1).
struct ZoneView {
    alive: Vec<u32>,
    inc: Vec<Inc>,
}

impl ZoneView {
    fn new<B: AsRef<[u8]>>(e: &Engine<B>, v: &ViewData) -> Self {
        let mut alive: Vec<u32> = v.chain_links.iter().map(|l| l & !DIR_BIT).collect();
        alive.sort_unstable();
        alive.dedup();
        let mut inc = Vec::with_capacity(2 * alive.len());
        for &ed in &alive {
            let (a, b) = e.ends(ed);
            let len = e.len_dm(ed);
            inc.push(Inc { node: a, edge: ed, other: b, len });
            inc.push(Inc { node: b, edge: ed, other: a, len });
        }
        inc.sort_unstable_by_key(|i| (i.node, i.edge));
        Self { alive, inc }
    }

    fn alive(&self, e: u32) -> bool {
        self.alive.binary_search(&e).is_ok()
    }

    fn inc_of(&self, x: u32) -> &[Inc] {
        let lo = self.inc.partition_point(|i| i.node < x);
        let hi = self.inc.partition_point(|i| i.node <= x);
        &self.inc[lo..hi]
    }
}

#[derive(Clone, Copy)]
struct EdgeInfo {
    fl: u8,
    n: u32,
    z: u8,
}

fn build_type<B: AsRef<[u8]>>(
    e: &Engine<B>,
    ov: &Overlay,
    ride: RideType,
    v: &ViewData,
    zv: &ZoneView,
) -> TypeTeam {
    let (i_road, i_gravel, i_mtb, i_mixed) =
        (type_index(ov, "ROAD"), type_index(ov, "GRAVEL"), type_index(ov, "MTB"), type_index(ov, "MIXED"));
    let cnt = |c: &[u16], idx: Option<usize>, dir: usize| idx.map_or(0u32, |i| u32::from(c[2 * i + dir]));
    let mut info: FxHashMap<u32, EdgeInfo> = FxHashMap::default();
    let mut max_n = 1u32;
    let mut outside_dm = (0u64, 0u64);
    for (edge, c) in ov.iter() {
        let any = c.iter().any(|&x| x > 0);
        if !any {
            continue;
        }
        if !zv.alive(edge) {
            let typed = match ride {
                RideType::Road => {
                    cnt(c, i_road, 0).max(cnt(c, i_road, 1)).max(cnt(c, i_mixed, 0)).max(cnt(c, i_mixed, 1))
                }
                RideType::Gravel => cnt(c, i_gravel, 0)
                    .max(cnt(c, i_gravel, 1))
                    .max(cnt(c, i_mixed, 0))
                    .max(cnt(c, i_mixed, 1)),
                RideType::Mtb => {
                    cnt(c, i_mtb, 0).max(cnt(c, i_mtb, 1)).max(cnt(c, i_mixed, 0)).max(cnt(c, i_mixed, 1))
                }
            };
            let l = u64::from(e.len_dm(edge));
            outside_dm.1 += l;
            if typed > 0 {
                outside_dm.0 += l;
            }
            continue;
        }
        let dirs = |f: &dyn Fn(usize) -> u32| f(0).max(f(1));
        let mut typed = match ride {
            RideType::Road => dirs(&|d| cnt(c, i_road, d) + cnt(c, i_mixed, d)),
            RideType::Gravel => dirs(&|d| cnt(c, i_gravel, d) + cnt(c, i_mixed, d)),
            RideType::Mtb => dirs(&|d| cnt(c, i_mtb, d) + cnt(c, i_mixed, d)),
        };
        if ride == RideType::Mtb && typed == 0 {
            typed = dirs(&|d| cnt(c, i_gravel, d));
        }
        let a = e.attrs(edge);
        // Exceptions Java (plan §5.5) : une surface présumée n'est acceptée que d'un parcours du bon type.
        let exception = match ride {
            RideType::Road => (is_greenway(a) || a.class() == Some(HighwayClass::Track)) && a.surface() == 0,
            RideType::Gravel | RideType::Mtb => {
                matches!(a.class(), Some(HighwayClass::Path | HighwayClass::Footway)) || rough_surface(a)
            }
        };
        let team = if exception { typed > 0 } else { true };
        if !team {
            continue;
        }
        let n = if typed > 0 {
            typed
        } else {
            dirs(&|d| (0..ov.meta.types.len()).map(|i| u32::from(c[2 * i + d])).sum())
        };
        info.insert(edge, EdgeInfo { fl: TEAM, n, z: 0 });
        max_n = max_n.max(n);
    }
    let lm = f64::from(max_n).ln_1p();
    for inf in info.values_mut() {
        inf.z = (255.0 * f64::from(inf.n).ln_1p() / lm).round().clamp(0.0, 255.0) as u8;
    }

    // Trous : culs-de-sac fictifs et plus court chemin non roulé de 100 m au plus vers un autre nœud d'équipe.
    let mut tdeg: FxHashMap<u32, u8> = FxHashMap::default();
    for &edge in info.keys() {
        let (a, b) = e.ends(edge);
        for x in [a, b] {
            let d = tdeg.entry(x).or_insert(0);
            *d = d.saturating_add(1);
        }
    }
    let is_team = |be: u32| info.contains_key(&be);
    let hole_dm = (e.params.hole_m * 10.0) as u32;
    let mut starts: Vec<u32> =
        tdeg.iter().filter(|&(&x, &d)| d == 1 && zv.inc_of(x).len() >= 2).map(|(&x, _)| x).collect();
    starts.sort_unstable();
    starts.dedup();
    let mut to_bridge: Vec<u32> = Vec::new();
    let walk_back = |dist: &FxHashMap<u32, (u32, u32)>, from: u32, s: u32, out: &mut Vec<u32>| {
        let mut cur = from;
        while cur != s {
            let (_, pe) = dist[&cur];
            out.push(pe);
            let (a, b) = e.ends(pe);
            cur = if a == cur { b } else { a };
        }
    };
    for &s in &starts {
        let mut dist: FxHashMap<u32, (u32, u32)> = FxHashMap::default();
        let mut heap: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
        dist.insert(s, (0, u32::MAX));
        heap.push(Reverse((0, s)));
        while let Some(Reverse((d, x))) = heap.pop() {
            if dist[&x].0 < d {
                continue;
            }
            if x != s && tdeg.contains_key(&x) {
                walk_back(&dist, x, s, &mut to_bridge);
                break;
            }
            for i in zv.inc_of(x) {
                if is_team(i.edge) {
                    continue;
                }
                let nd = d + i.len;
                if nd > hole_dm {
                    continue;
                }
                let y = i.other;
                if dist.get(&y).is_none_or(|&(od, _)| nd < od) {
                    dist.insert(y, (nd, i.edge));
                    heap.push(Reverse((nd, y)));
                }
            }
        }
    }
    // Jonctions courtes : un chemin non roulé de `connector_m` au plus entre deux nœuds d'équipe (seconde chaussée
    // d'une 2 × 2, raccord d'un carrefour que l'appariement a manqué) ; sans elle une chaussée à sens unique
    // roulée dans un sens ne se rejoint pas dans l'autre.
    let conn_dm = (e.params.connector_m * 10.0) as u32;
    let mut team_nodes: Vec<u32> = tdeg.keys().copied().collect();
    team_nodes.sort_unstable();
    for &s in &team_nodes {
        if !zv.inc_of(s).iter().any(|i| !is_team(i.edge) && i.len <= conn_dm) {
            continue;
        }
        let mut dist: FxHashMap<u32, (u32, u32)> = FxHashMap::default();
        let mut heap: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
        dist.insert(s, (0, u32::MAX));
        heap.push(Reverse((0, s)));
        while let Some(Reverse((d, x))) = heap.pop() {
            if dist[&x].0 < d {
                continue;
            }
            if x != s && tdeg.contains_key(&x) {
                walk_back(&dist, x, s, &mut to_bridge);
                continue;
            }
            for i in zv.inc_of(x) {
                if is_team(i.edge) {
                    continue;
                }
                let nd = d + i.len;
                if nd > conn_dm {
                    continue;
                }
                let y = i.other;
                if dist.get(&y).is_none_or(|&(od, _)| nd < od) {
                    dist.insert(y, (nd, i.edge));
                    heap.push(Reverse((nd, y)));
                }
            }
        }
    }
    for be in to_bridge {
        info.entry(be).and_modify(|i| i.fl |= BRIDGED).or_insert(EdgeInfo { fl: BRIDGED, n: 0, z: 0 });
    }

    let mut edges: Vec<(u32, u8, u8)> =
        info.iter().filter(|(_, i)| i.fl != 0).map(|(&ed, i)| (ed, i.fl, i.z)).collect();
    edges.sort_unstable_by_key(|x| x.0);
    let (mut team_dm, mut bridged_dm, mut team_edges) = (0u64, 0u64, 0u64);
    for &(ed, fl, _) in &edges {
        if fl & TEAM != 0 {
            team_dm += u64::from(e.len_dm(ed));
            team_edges += 1;
        } else {
            bridged_dm += u64::from(e.len_dm(ed));
        }
    }

    // Composantes fortement connexes du réseau strict (sens respectés, virages ignorés) : un réseau strict est fait
    // d'arêtes d'équipe ou comblées, toutes dans la zone ; le calcul sur la fenêtre de la zone rend les composantes
    // du graphe entier.
    let flagged = |ed: u32| match edges.binary_search_by_key(&ed, |x| x.0) {
        Ok(i) => edges[i].1 & (TEAM | BRIDGED) != 0,
        Err(_) => false,
    };
    let strict: Vec<bool> =
        (0..v.n_chains as u32).map(|c| v.links(c).iter().all(|l| flagged(l & !DIR_BIT))).collect();
    let comp = strong_components(v.n_nodes, &|x, s: &mut Vec<u32>| {
        s.extend(v.out(x).iter().filter(|&&f| strict[(f >> 1) as usize]).map(|&f| v.tgt(f)));
    });
    let lens = strong_lengths(&comp, &v.ends, &v.len_dm, &|c| strict[c]);
    let mut strict_len: Vec<(u32, u64)> = Vec::new();
    for c in 0..v.n_chains {
        if strict[c] {
            let l = chain_strong_len(&comp, &lens, v.ends[c]);
            if l > 0 {
                strict_len.push((v.gid[c], l));
            }
        }
    }
    TypeTeam {
        edges,
        strict_len,
        team_edges,
        team_m: team_dm as f64 / 10.0,
        bridged_m: bridged_dm as f64 / 10.0,
        outside_m: (outside_dm.0 as f64 / 10.0, outside_dm.1 as f64 / 10.0),
    }
}
