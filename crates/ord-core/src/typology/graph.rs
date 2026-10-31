//! Graphe local par niveaux (T03 §4.2) : lignes des tuiles du couloir coupées en segments entre
//! nœuds, adjacence CSR, index spatial.
//!
//! Repris d'osm-surface (`src/network.rs`, Apache-2.0, © Gabriel Landais), avec trois écarts
//! (T03 §12.3) : la clé de nœud inclut le niveau (sol, pont, tunnel) ; le recollage aux bords de
//! tuile reste au même niveau ; un pont ou un tunnel ne se relie à un autre niveau que par une
//! vraie fin de ligne (la culée), jamais par un sommet intérieur.
//!
//! Les routes motorisées fermées au vélo (autoroute, trunk, `bicycle=no`…) n'entrent pas au
//! graphe ; leurs segments du couloir sont gardés à part (`closed`), sans topologie : le HMM y voit un état « hors
//! graphe » (T03 §5.3).

use std::sync::Arc;

use ord_domain::geo::{dist_units, haversine_m, project_on_segment, Grid};
use ord_domain::street::{SchemaCapabilities, StreetTile, WayAttrs};
use rstar::primitives::{GeomWithData, Line};
use rstar::RTree;
use rustc_hash::FxHashMap;

use super::corridor::Corridor;
use crate::classification::tables::{bicycle_closed, bicycle_routable};
use crate::classification::{classify, Classified, Level};

pub type NodeId = u32;
pub type SegId = u32;
pub type ClassId = u32;
/// Morceaux d'une même ligne de tuile, recollés aux bords de tuile : un élément d'ouvrage.
pub type LineId = u32;

/// Deux extrémités plus proches (unités globales, ≈ 0,8 m à 47° N) sont un même nœud.
pub const MERGE_UNITS: f64 = 2.0;
/// Un sommet aussi proche de l'intérieur d'un segment y est inséré (≈ 0,6 m).
pub const SPLICE_UNITS: f64 = 1.5;

/// Classe d'arêtes : attributs neutres et leur classification, calculée une fois.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeClass {
    pub attrs: WayAttrs,
    pub classified: Classified,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphSeg {
    pub a: NodeId,
    pub b: NodeId,
    pub len_m: f64,
    pub class: ClassId,
    pub level: Level,
    pub line: LineId,
}

pub type SegIndex = RTree<GeomWithData<Line<[f64; 2]>, SegId>>;
/// Segments des routes fermées au vélo, sans topologie.
pub type ClosedIndex = RTree<Line<[f64; 2]>>;

pub struct StreetGraph {
    pub grid: Grid,
    pub nodes: Vec<[f64; 2]>,
    pub segs: Vec<GraphSeg>,
    pub classes: Vec<EdgeClass>,
    adj_start: Vec<u32>,
    adj: Vec<(SegId, NodeId)>,
    pub index: SegIndex,
    /// Routes motorisées fermées au vélo du couloir (T03 §5.3).
    pub closed: ClosedIndex,
}

impl std::fmt::Debug for StreetGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreetGraph")
            .field("nodes", &self.nodes.len())
            .field("segs", &self.segs.len())
            .field("classes", &self.classes.len())
            .field("closed", &self.closed.size())
            .finish_non_exhaustive()
    }
}

/// Segment brut avant réparation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct RawSeg {
    a: NodeId,
    b: NodeId,
    class: ClassId,
    line: LineId,
}

/// Union-find à compression de chemin.
struct UnionFind(Vec<u32>);

impl UnionFind {
    fn find(&mut self, mut x: u32) -> u32 {
        while self.0[x as usize] != x {
            let p = self.0[self.0[x as usize] as usize];
            self.0[x as usize] = p;
            x = p;
        }
        x
    }
}

impl StreetGraph {
    /// Graphe des rues de `tiles` (triées) ouvertes au vélo, restreint au couloir.
    pub fn build(
        grid: Grid,
        tiles: &[Arc<StreetTile>],
        corridor: &Corridor,
        caps: &SchemaCapabilities,
    ) -> Self {
        let mut classes: Vec<EdgeClass> = Vec::new();
        let mut class_ids: FxHashMap<WayAttrs, Option<ClassId>> = FxHashMap::default();
        let mut levels: Vec<Level> = Vec::new();
        let mut class_level: Vec<u8> = Vec::new();
        let mut node_ids: FxHashMap<(i64, i64, u8), NodeId> = FxHashMap::default();
        let mut nodes: Vec<[f64; 2]> = Vec::new();
        let mut node_level: Vec<u8> = Vec::new();
        // Extrémités de ligne : (nœud, coupée, ligne, ouvrage).
        let mut ends: Vec<(NodeId, bool, LineId, bool)> = Vec::new();
        let mut cut_ends: Vec<(NodeId, LineId)> = Vec::new();
        let mut raw: Vec<RawSeg> = Vec::new();
        let mut line_count: LineId = 0;
        let mut closed: Vec<Line<[f64; 2]>> = Vec::new();

        for tile in tiles {
            let tile_classes: Vec<Option<ClassId>> = tile
                .attrs
                .iter()
                .map(|a| {
                    *class_ids.entry(a.clone()).or_insert_with(|| {
                        if !bicycle_routable(a) {
                            return None;
                        }
                        let classified = classify(a, caps);
                        let code = match levels.iter().position(|l| *l == classified.level) {
                            Some(i) => i,
                            None => {
                                levels.push(classified.level);
                                levels.len() - 1
                            }
                        };
                        // Au plus 32 niveaux distincts (masques de bits) ; au-delà, le sol.
                        class_level.push(if code < 32 { code as u8 } else { 0 });
                        classes.push(EdgeClass { attrs: a.clone(), classified });
                        Some((classes.len() - 1) as ClassId)
                    })
                })
                .collect();
            for line in &tile.lines {
                let Some(class) = tile_classes.get(line.attr as usize).copied().flatten() else {
                    if tile.attrs.get(line.attr as usize).is_some_and(bicycle_closed) {
                        closed.extend(
                            line.points
                                .windows(2)
                                .filter(|w| w[0] != w[1] && corridor.touches(w[0], w[1]))
                                .map(|w| Line::new(w[0], w[1])),
                        );
                    }
                    continue;
                };
                let line_id = line_count;
                line_count += 1;
                let code = class_level[class as usize];
                let is_structure = levels[code as usize].is_structure();
                let last = line.points.len().saturating_sub(2);
                let mut node_of = |p: [f64; 2]| -> NodeId {
                    let key = ((p[0] * 16.0).round() as i64, (p[1] * 16.0).round() as i64, code);
                    *node_ids.entry(key).or_insert_with(|| {
                        nodes.push(p);
                        node_level.push(code);
                        (nodes.len() - 1) as NodeId
                    })
                };
                for (k, w) in line.points.windows(2).enumerate() {
                    if !corridor.touches(w[0], w[1]) {
                        continue;
                    }
                    let a = node_of(w[0]);
                    let b = node_of(w[1]);
                    for (is_end, cut, n) in [(k == 0, line.cut[0], a), (k == last, line.cut[1], b)] {
                        if !is_end {
                            continue;
                        }
                        ends.push((n, cut, line_id, is_structure));
                    }
                    if a != b {
                        raw.push(RawSeg { a, b, class, line: line_id });
                    }
                }
            }
        }

        // Vraie fin de ligne (non coupée) ; et fin de ligne de pont ou de tunnel (culée).
        let mut line_end = vec![false; nodes.len()];
        let mut structure_end = vec![false; nodes.len()];
        for (n, cut, line, is_structure) in ends {
            if cut {
                cut_ends.push((n, line));
            } else {
                line_end[n as usize] = true;
                structure_end[n as usize] |= is_structure;
            }
        }
        let segs =
            repair(&nodes, &node_level, &line_end, &structure_end, &cut_ends, raw, &class_level, &levels);
        let segs: Vec<GraphSeg> = segs
            .into_iter()
            .map(|s| GraphSeg {
                a: s.a,
                b: s.b,
                len_m: haversine_m(grid.to_latlon(nodes[s.a as usize]), grid.to_latlon(nodes[s.b as usize])),
                class: s.class,
                level: levels[class_level[s.class as usize] as usize],
                line: s.line,
            })
            .collect();

        let mut adj_start = vec![0u32; nodes.len() + 1];
        for s in &segs {
            adj_start[s.a as usize + 1] += 1;
            adj_start[s.b as usize + 1] += 1;
        }
        for n in 0..nodes.len() {
            adj_start[n + 1] += adj_start[n];
        }
        let mut fill = adj_start.clone();
        let mut adj = vec![(0u32, 0u32); adj_start[nodes.len()] as usize];
        for (i, s) in segs.iter().enumerate() {
            adj[fill[s.a as usize] as usize] = (i as SegId, s.b);
            fill[s.a as usize] += 1;
            adj[fill[s.b as usize] as usize] = (i as SegId, s.a);
            fill[s.b as usize] += 1;
        }
        let index = RTree::bulk_load(
            segs.iter()
                .enumerate()
                .map(|(i, s)| {
                    GeomWithData::new(Line::new(nodes[s.a as usize], nodes[s.b as usize]), i as SegId)
                })
                .collect(),
        );
        Self { grid, nodes, segs, classes, adj_start, adj, index, closed: RTree::bulk_load(closed) }
    }

    pub fn neighbours(&self, node: NodeId) -> &[(SegId, NodeId)] {
        &self.adj[self.adj_start[node as usize] as usize..self.adj_start[node as usize + 1] as usize]
    }

    pub fn seg_points(&self, seg: SegId) -> ([f64; 2], [f64; 2]) {
        let s = &self.segs[seg as usize];
        (self.nodes[s.a as usize], self.nodes[s.b as usize])
    }

    pub fn class_of(&self, seg: SegId) -> &EdgeClass {
        &self.classes[self.segs[seg as usize].class as usize]
    }

    pub fn is_empty(&self) -> bool {
        self.segs.is_empty()
    }
}

/// Recollage aux bords de tuile (même niveau), jonctions inter-niveaux par les culées, jonctions
/// perdues à la simplification. Renvoie les segments, renumérotés sur les représentants.
#[allow(clippy::too_many_arguments)]
fn repair(
    nodes: &[[f64; 2]],
    node_level: &[u8],
    line_end: &[bool],
    structure_end: &[bool],
    cut_ends: &[(NodeId, LineId)],
    raw: Vec<RawSeg>,
    class_level: &[u8],
    levels: &[Level],
) -> Vec<RawSeg> {
    let n = nodes.len();
    let mut degree = vec![0u32; n];
    for s in &raw {
        degree[s.a as usize] += 1;
        degree[s.b as usize] += 1;
    }
    let cell = |p: [f64; 2]| ((p[0] / MERGE_UNITS).floor() as i64, (p[1] / MERGE_UNITS).floor() as i64);
    let mut bucket: FxHashMap<(i64, i64), Vec<NodeId>> = FxHashMap::default();
    for (i, p) in nodes.iter().enumerate() {
        if degree[i] > 0 {
            bucket.entry(cell(*p)).or_default().push(i as NodeId);
        }
    }
    let nearest = |i: usize, accept: &dyn Fn(usize) -> bool| -> Option<NodeId> {
        let p = nodes[i];
        let (cx, cy) = cell(p);
        let mut best: Option<(f64, NodeId)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &j in bucket.get(&(cx + dx, cy + dy)).map_or(&[][..], Vec::as_slice) {
                    if j as usize == i || !accept(j as usize) {
                        continue;
                    }
                    let d = dist_units(p, nodes[j as usize]);
                    if d <= MERGE_UNITS && best.is_none_or(|(bd, bj)| d < bd || (d == bd && j < bj)) {
                        best = Some((d, j));
                    }
                }
            }
        }
        best.map(|(_, j)| j)
    };
    let mut uf = UnionFind((0..n as u32).collect());
    let union = |uf: &mut UnionFind, i: NodeId, j: NodeId| {
        let (ri, rj) = (uf.find(i), uf.find(j));
        if ri != rj {
            // Le nœud de plus haut degré (un vrai sommet) représente l'ensemble.
            if (degree[ri as usize], std::cmp::Reverse(ri)) >= (degree[rj as usize], std::cmp::Reverse(rj)) {
                uf.0[rj as usize] = ri;
            } else {
                uf.0[ri as usize] = rj;
            }
        }
    };
    for i in 0..n {
        // 3. Extrémité pendante : nœud le plus proche du même niveau.
        if degree[i] == 1 {
            if let Some(j) = nearest(i, &|j| node_level[j] == node_level[i]) {
                union(&mut uf, i as NodeId, j);
            }
        }
        // 4. Culée : fin de ligne de pont ou de tunnel, nœud le plus proche d'un autre niveau.
        if structure_end[i] {
            if let Some(j) = nearest(i, &|j| node_level[j] != node_level[i]) {
                union(&mut uf, i as NodeId, j);
            }
        }
    }
    // Drapeaux et masques de niveaux portés par les représentants.
    let mut mask = vec![0u32; n];
    let mut is_line_end = vec![false; n];
    let mut is_structure_end = vec![false; n];
    for i in 0..n {
        let r = uf.find(i as NodeId) as usize;
        is_line_end[r] |= line_end[i];
        is_structure_end[r] |= structure_end[i];
    }
    // Éléments : les morceaux d'une même ligne recollés à une extrémité coupée (même classe).
    let line_count = raw.iter().map(|s| s.line + 1).max().unwrap_or(0);
    let mut line_class = vec![u32::MAX; line_count as usize];
    for s in &raw {
        line_class[s.line as usize] = s.class;
    }
    let mut lines = UnionFind((0..line_count).collect());
    let mut at_root: FxHashMap<NodeId, Vec<LineId>> = FxHashMap::default();
    for &(node, line) in cut_ends {
        if (line as usize) < line_class.len() {
            at_root.entry(uf.find(node)).or_default().push(line);
        }
    }
    let mut roots: Vec<(&NodeId, &Vec<LineId>)> = at_root.iter().collect();
    roots.sort_unstable();
    for (_, ls) in roots {
        for (k, &l1) in ls.iter().enumerate() {
            for &l2 in &ls[k + 1..] {
                if line_class[l1 as usize] == line_class[l2 as usize] {
                    let (r1, r2) = (lines.find(l1), lines.find(l2));
                    if r1 != r2 {
                        lines.0[r1.max(r2) as usize] = r1.min(r2);
                    }
                }
            }
        }
    }
    let mut segs: Vec<RawSeg> = raw
        .into_iter()
        .filter_map(|s| {
            let (a, b) = (uf.find(s.a), uf.find(s.b));
            (a != b).then(|| RawSeg { a: a.min(b), b: a.max(b), line: lines.find(s.line), ..s })
        })
        .collect();
    segs.sort_unstable();
    segs.dedup_by(|x, y| (x.a, x.b, x.class) == (y.a, y.b, y.class));
    for s in &segs {
        let bit = 1u32 << class_level[s.class as usize];
        mask[s.a as usize] |= bit;
        mask[s.b as usize] |= bit;
    }

    // 5. Insertion des sommets à l'intérieur d'un segment proche.
    let tree: RTree<GeomWithData<Line<[f64; 2]>, u32>> = RTree::bulk_load(
        segs.iter()
            .enumerate()
            .map(|(i, s)| GeomWithData::new(Line::new(nodes[s.a as usize], nodes[s.b as usize]), i as u32))
            .collect(),
    );
    let mut splits: Vec<Vec<(f64, NodeId)>> = vec![Vec::new(); segs.len()];
    for (node, &m) in mask.iter().enumerate() {
        if m == 0 {
            continue;
        }
        let p = nodes[node];
        let mut best: Option<(f64, u32, f64)> = None;
        for g in tree.locate_within_distance(p, SPLICE_UNITS * SPLICE_UNITS) {
            let s = segs[g.data as usize];
            if s.a as usize == node || s.b as usize == node {
                continue;
            }
            let code = class_level[s.class as usize];
            let structure = levels[code as usize].is_structure();
            let allowed = if m & (1 << code) != 0 {
                // Même niveau : tout sommet au sol ; sur un ouvrage, les fins de ligne seulement.
                !structure || is_line_end[node]
            } else {
                // Autre niveau : seulement une culée.
                is_structure_end[node]
            };
            if !allowed {
                continue;
            }
            let pr = project_on_segment(p, nodes[s.a as usize], nodes[s.b as usize]);
            let d = pr.dist2.sqrt();
            if pr.t > 0.0
                && pr.t < 1.0
                && d <= SPLICE_UNITS
                && best.is_none_or(|(bd, bs, _)| d < bd || (d == bd && g.data < bs))
            {
                best = Some((d, g.data, pr.t));
            }
        }
        if let Some((_, si, t)) = best {
            splits[si as usize].push((t, node as NodeId));
        }
    }
    let mut out = Vec::with_capacity(segs.len() + 8);
    for (s, list) in segs.iter().zip(splits.iter_mut()) {
        if list.is_empty() {
            out.push(*s);
            continue;
        }
        list.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        let mut prev = s.a;
        for &(_, n) in list.iter() {
            if n != prev {
                out.push(RawSeg { a: prev, b: n, ..*s });
                prev = n;
            }
        }
        if prev != s.b {
            out.push(RawSeg { a: prev, b: s.b, ..*s });
        }
    }
    out
}
