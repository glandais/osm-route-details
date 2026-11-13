//! Ouvrages (T03 §6.2–§6.3) : graphe des ouvrages, extrémités de l'ouvrage (et non de la trace),
//! extension aux tuiles voisines (`Step::Need`), éléments, chaînage, passages et doutes.
//!
//! Le graphe d'appariement ne contient que le couloir : un pont y est coupé au bord des cellules.
//! Le `StructureGraph` réunit toutes les lignes de pont et de tunnel des tuiles lues, sans filtre de
//! couloir. Chaque suite d'ouvrage appariée y est prolongée, à chaque nœud par la continuation
//! d'ouvrage de plus faible changement de cap, jusqu'à une culée (fin reliée au sol), une impasse,
//! ou une tuile non lue (le calcul rend alors les tuiles voisines à lire).
//!
//! Écarts à T03, choisis pour rester simples : les nœuds d'ouvrage sont reliés par coordonnée
//! quantifiée (un pont et un tunnel contigus se relient, inv. 3) ; `approach` commence à la culée
//! elle-même (la fusion y marche de `abutment_probe_m`) ; une continuation qui tourne de plus de
//! `max_turn_deg` n'est pas suivie (bretelle d'échangeur).

use std::sync::Arc;

use ord_domain::geo::{dist_units, haversine_m, lerp, project_on_segment, Grid};
use ord_domain::street::{StreetTile, StructureTag, WayAttrs};
use ord_domain::tiles::TileKey;
use ord_domain::LatLon;
use rstar::primitives::{GeomWithData, Line};
use rstar::{PointDistance, RTree};
use rustc_hash::{FxHashMap, FxHashSet};

use super::graph::{StreetGraph, MERGE_UNITS};
use super::matcher::{MatchedPath, ObsState, Step};
use super::{RunMatch, TileState, TypedRun};
use crate::rules::StructureRules;

/// Identité stable d'un ouvrage dans une réponse : plus petit segment de son axe.
pub type StructureId = u32;
pub type PassageId = u32;

/// Nature d'un élément d'ouvrage ou d'une détection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StructureKind {
    Bridge,
    Tunnel,
}

impl StructureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bridge => "bridge",
            Self::Tunnel => "tunnel",
        }
    }
}

/// Un way pont ou tunnel (une ligne de tuile, recollée aux bords) projeté sur l'axe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StructureElement {
    pub id: u32,
    pub kind: StructureKind,
    pub chain_from: f64,
    pub chain_to: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnresolvedEnd {
    TileAbsent,
    TileFailed,
    ExtentLimit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndKind {
    /// Fin reliée au sol.
    Abutment,
    /// Fin de ligne sans sol ni continuation.
    DeadEnd,
    Unresolved(UnresolvedEnd),
}

/// Extrémité de l'ouvrage, sur l'ouvrage.
#[derive(Clone, PartialEq)]
pub struct StructureEnd {
    pub at: LatLon,
    pub kind: EndKind,
    /// Voie au sol qui continue au-delà de la culée, sur `approach_m` au plus ; `approach[0]` est la
    /// culée elle-même. Vide hors culée.
    pub approach: Vec<LatLon>,
}

impl std::fmt::Debug for StructureEnd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StructureEnd")
            .field("kind", &self.kind)
            .field("approach", &self.approach.len())
            .finish_non_exhaustive()
    }
}

/// Un ouvrage : suite d'éléments contigus, axe orienté de la culée A à la culée B.
#[derive(Clone, PartialEq)]
pub struct Structure {
    pub id: StructureId,
    pub axis: Vec<LatLon>,
    /// Chaînage de chaque sommet de l'axe, en mètres depuis A.
    pub chain: Vec<f64>,
    pub length_m: f64,
    pub elements: Vec<StructureElement>,
    pub ends: [StructureEnd; 2],
}

impl std::fmt::Debug for Structure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Structure")
            .field("id", &self.id)
            .field("length_m", &self.length_m)
            .field("elements", &self.elements)
            .field("ends", &self.ends)
            .finish_non_exhaustive()
    }
}

impl Structure {
    /// Point de l'axe au chaînage `c` (borné à `[0, length_m]`).
    pub fn point_at(&self, c: f64) -> LatLon {
        let c = c.clamp(0.0, self.length_m);
        let i = self.chain.partition_point(|&x| x <= c).clamp(1, self.chain.len().max(2) - 1);
        let (c0, c1) = (self.chain[i - 1], self.chain[i]);
        let t = if c1 > c0 { (c - c0) / (c1 - c0) } else { 0.0 };
        let (a, b) = (self.axis[i - 1], self.axis[i]);
        LatLon::new(a.lat + (b.lat - a.lat) * t, a.lon + (b.lon - a.lon) * t)
    }

    /// Nature de l'élément au chaînage `c`.
    pub fn kind_at(&self, c: f64) -> StructureKind {
        self.elements
            .iter()
            .find(|e| c >= e.chain_from - 1e-9 && c <= e.chain_to + 1e-9)
            .or(self.elements.first())
            .map_or(StructureKind::Bridge, |e| e.kind)
    }
}

/// Ce que la typologie constate ; la fusion décide (inv. 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PassageDoubt {
    EndUnresolved,
    UnmatchedAdjacent,
    JumpInside,
    FarFromAxis,
    ChainBreak,
    /// Le parcours avance sans avancer le long de l'axe : il croise l'ouvrage (passage dessous
    /// apparié par un saut sur un pont qui l'enjambe) au lieu de le suivre.
    AcrossAxis,
}

/// Un passage du parcours sur un ouvrage.
#[derive(Clone, Debug, PartialEq)]
pub struct StructurePassage {
    pub id: PassageId,
    pub structure: StructureId,
    pub from_m: f64,
    pub to_m: f64,
    /// `(d_m, chaînage)`, croissant en `d_m`.
    pub chainage: Vec<(f64, f64)>,
    pub doubts: Vec<PassageDoubt>,
}

impl StructurePassage {
    /// Chaînage à la distance `d` : interpolé entre deux échantillons, prolongé au-delà avec une
    /// pente de ±1 (un mètre de parcours = un mètre d'axe), dans le sens du passage.
    pub fn chainage_at(&self, d: f64) -> f64 {
        let s = &self.chainage;
        match s.len() {
            0 => 0.0,
            1 => s[0].1,
            n => {
                let i = s.partition_point(|x| x.0 <= d);
                if i == 0 {
                    let sign = sign_of(s[n.min(s.len()) - 1].1 - s[0].1);
                    s[0].1 - sign * (s[0].0 - d)
                } else if i >= n {
                    let sign = sign_of(s[n - 1].1 - s[0].1);
                    s[n - 1].1 + sign * (d - s[n - 1].0)
                } else {
                    let (a, b) = (s[i - 1], s[i]);
                    if b.0 > a.0 {
                        a.1 + (b.1 - a.1) * (d - a.0) / (b.0 - a.0)
                    } else {
                        b.1
                    }
                }
            }
        }
    }

    /// Plus petit et plus grand chaînage atteints.
    pub fn chain_extent(&self) -> (f64, f64) {
        self.chainage
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &(_, c)| (lo.min(c), hi.max(c)))
    }
}

fn sign_of(x: f64) -> f64 {
    if x < 0.0 {
        -1.0
    } else {
        1.0
    }
}

/// Résultat de l'étape « ouvrages » : ouvrages et passages, ou tuiles voisines à lire.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StructuresOut {
    pub structures: Vec<Structure>,
    pub passages: Vec<StructurePassage>,
}

// ------------------------------------------------------------------------------------------------
// Graphe des ouvrages
// ------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct SSeg {
    a: u32,
    b: u32,
    element: u32,
    kind: StructureKind,
    len_m: f64,
}

/// Lignes de pont et de tunnel de toutes les tuiles lues, reliées par coordonnée.
struct StructureGraph {
    grid: Grid,
    nodes: Vec<[f64; 2]>,
    segs: Vec<SSeg>,
    adj: Vec<Vec<(u32, u32)>>,
    /// Tuile où continue une fin coupée restée pendante.
    cut_next: FxHashMap<u32, TileKey>,
    index: RTree<GeomWithData<Line<[f64; 2]>, u32>>,
}

/// Voies au sol proches des ouvrages : culées et voie d'approche.
struct Ground {
    lines: Vec<(Vec<[f64; 2]>, [bool; 2])>,
    /// Sommets par cellule de `MERGE_UNITS`.
    cells: FxHashMap<(i64, i64), Vec<(u32, u32)>>,
}

fn cell(p: [f64; 2]) -> (i64, i64) {
    ((p[0] / MERGE_UNITS).floor() as i64, (p[1] / MERGE_UNITS).floor() as i64)
}

/// Nature d'ouvrage d'une voie (table `level_of` de la classification, sans `layer`).
fn structure_kind(a: &WayAttrs) -> Option<StructureKind> {
    if a.rail {
        return None;
    }
    match a.structure {
        StructureTag::Bridge => Some(StructureKind::Bridge),
        StructureTag::Tunnel | StructureTag::Covered | StructureTag::BuildingPassage => {
            Some(StructureKind::Tunnel)
        }
        StructureTag::Ford | StructureTag::None => None,
    }
}

/// Tuile dans laquelle continue une fin coupée `end` (venant de `prev`).
fn next_tile(grid: Grid, prev: [f64; 2], end: [f64; 2]) -> TileKey {
    let (dx, dy) = (end[0] - prev[0], end[1] - prev[1]);
    let len = (dx * dx + dy * dy).sqrt().max(1e-9);
    grid.tile_of([end[0] + 0.5 * dx / len, end[1] + 0.5 * dy / len])
}

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

    fn union(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra.max(rb) as usize] = ra.min(rb);
        }
    }
}

impl StructureGraph {
    fn build(grid: Grid, tiles: &[Arc<StreetTile>]) -> Self {
        let mut key_of: FxHashMap<(i64, i64), u32> = FxHashMap::default();
        let mut nodes: Vec<[f64; 2]> = Vec::new();
        // (a, b, ligne, nature) ; fins coupées : (nœud, ligne, tuile suivante).
        let mut raw: Vec<(u32, u32, u32, StructureKind)> = Vec::new();
        let mut cut_ends: Vec<(u32, u32, TileKey)> = Vec::new();
        let mut line_attrs: Vec<u32> = Vec::new();
        let mut attrs_id: FxHashMap<&WayAttrs, u32> = FxHashMap::default();
        for tile in tiles {
            for line in &tile.lines {
                let Some(attrs) = tile.attrs.get(line.attr as usize) else { continue };
                let Some(kind) = structure_kind(attrs) else { continue };
                if line.points.len() < 2 {
                    continue;
                }
                let next_attr = attrs_id.len() as u32;
                let aid = *attrs_id.entry(attrs).or_insert(next_attr);
                let lid = line_attrs.len() as u32;
                line_attrs.push(aid);
                let mut node_of = |p: [f64; 2]| -> u32 {
                    let key = ((p[0] * 16.0).round() as i64, (p[1] * 16.0).round() as i64);
                    *key_of.entry(key).or_insert_with(|| {
                        nodes.push(p);
                        (nodes.len() - 1) as u32
                    })
                };
                let ids: Vec<u32> = line.points.iter().map(|&p| node_of(p)).collect();
                for w in ids.windows(2) {
                    if w[0] != w[1] {
                        raw.push((w[0], w[1], lid, kind));
                    }
                }
                let n = line.points.len();
                if line.cut[0] {
                    cut_ends.push((ids[0], lid, next_tile(grid, line.points[1], line.points[0])));
                }
                if line.cut[1] {
                    cut_ends.push((ids[n - 1], lid, next_tile(grid, line.points[n - 2], line.points[n - 1])));
                }
            }
        }
        // Recollage des fins coupées au nœud d'ouvrage le plus proche (bords de tuile).
        let mut degree = vec![0u32; nodes.len()];
        for &(a, b, _, _) in &raw {
            degree[a as usize] += 1;
            degree[b as usize] += 1;
        }
        let mut buckets: FxHashMap<(i64, i64), Vec<u32>> = FxHashMap::default();
        for (i, p) in nodes.iter().enumerate() {
            buckets.entry(cell(*p)).or_default().push(i as u32);
        }
        let mut uf = UnionFind((0..nodes.len() as u32).collect());
        let mut lines = UnionFind((0..line_attrs.len() as u32).collect());
        let line_of_cut: FxHashMap<u32, Vec<u32>> = cut_ends.iter().fold(FxHashMap::default(), |mut m, c| {
            m.entry(c.0).or_insert_with(Vec::new).push(c.1);
            m
        });
        for &(n, lid, _) in &cut_ends {
            if degree[n as usize] != 1 {
                continue;
            }
            let p = nodes[n as usize];
            let (cx, cy) = cell(p);
            let mut best: Option<(f64, u32)> = None;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for &j in buckets.get(&(cx + dx, cy + dy)).map_or(&[][..], Vec::as_slice) {
                        if j == n {
                            continue;
                        }
                        let d = dist_units(p, nodes[j as usize]);
                        if d <= MERGE_UNITS && best.is_none_or(|(bd, bj)| d < bd || (d == bd && j < bj)) {
                            best = Some((d, j));
                        }
                    }
                }
            }
            if let Some((_, j)) = best {
                uf.union(n, j);
                for &other in line_of_cut.get(&j).map_or(&[][..], Vec::as_slice) {
                    if line_attrs[other as usize] == line_attrs[lid as usize] {
                        lines.union(lid, other);
                    }
                }
            }
        }
        // Une fin coupée à un nœud partagé par la même ligne voisine (clé identique) : même élément.
        for (n, ls) in &line_of_cut {
            let _ = n;
            for (k, &l1) in ls.iter().enumerate() {
                for &l2 in &ls[k + 1..] {
                    if line_attrs[l1 as usize] == line_attrs[l2 as usize] {
                        lines.union(l1, l2);
                    }
                }
            }
        }
        let mut segs: Vec<SSeg> = Vec::with_capacity(raw.len());
        let mut seen: FxHashSet<(u32, u32)> = FxHashSet::default();
        for (a, b, lid, kind) in raw {
            let (a, b) = (uf.find(a), uf.find(b));
            if a == b || !seen.insert((a.min(b), a.max(b))) {
                continue;
            }
            let len_m = haversine_m(grid.to_latlon(nodes[a as usize]), grid.to_latlon(nodes[b as usize]));
            segs.push(SSeg { a, b, element: lines.find(lid), kind, len_m });
        }
        let mut adj = vec![Vec::new(); nodes.len()];
        for (i, s) in segs.iter().enumerate() {
            adj[s.a as usize].push((i as u32, s.b));
            adj[s.b as usize].push((i as u32, s.a));
        }
        let mut cut_next = FxHashMap::default();
        for (n, _, t) in cut_ends {
            let r = uf.find(n);
            if adj[r as usize].len() == 1 {
                cut_next.insert(r, t);
            }
        }
        let index = RTree::bulk_load(
            segs.iter()
                .enumerate()
                .map(|(i, s)| {
                    GeomWithData::new(Line::new(nodes[s.a as usize], nodes[s.b as usize]), i as u32)
                })
                .collect(),
        );
        Self { grid, nodes, segs, adj, cut_next, index }
    }

    /// Segment d'ouvrage le plus proche d'un point (à `tol` unités au plus).
    fn locate(&self, p: [f64; 2], tol: f64) -> Option<u32> {
        let g = self.index.nearest_neighbor(p)?;
        (g.geom().distance_2(&p) <= tol * tol).then_some(g.data)
    }

    fn other(&self, seg: u32, node: u32) -> u32 {
        let s = self.segs[seg as usize];
        if s.a == node {
            s.b
        } else {
            s.a
        }
    }
}

impl Ground {
    /// Voies au sol des tuiles lues dont un sommet est à moins de `radius` unités d'un nœud
    /// d'ouvrage.
    fn build(tiles: &[Arc<StreetTile>], sg: &StructureGraph, radius: f64) -> Self {
        const NEAR: f64 = 256.0;
        let near: FxHashSet<(i64, i64)> = {
            let r = (radius / NEAR).ceil() as i64;
            let mut s = FxHashSet::default();
            for p in &sg.nodes {
                let (cx, cy) = ((p[0] / NEAR).floor() as i64, (p[1] / NEAR).floor() as i64);
                for dx in -r..=r {
                    for dy in -r..=r {
                        s.insert((cx + dx, cy + dy));
                    }
                }
            }
            s
        };
        let mut lines = Vec::new();
        let mut cells: FxHashMap<(i64, i64), Vec<(u32, u32)>> = FxHashMap::default();
        if near.is_empty() {
            return Self { lines, cells };
        }
        for tile in tiles {
            for line in &tile.lines {
                let Some(attrs) = tile.attrs.get(line.attr as usize) else { continue };
                if attrs.rail || structure_kind(attrs).is_some() || line.points.len() < 2 {
                    continue;
                }
                let touches = line
                    .points
                    .iter()
                    .any(|p| near.contains(&((p[0] / NEAR).floor() as i64, (p[1] / NEAR).floor() as i64)));
                if !touches {
                    continue;
                }
                let lid = lines.len() as u32;
                for (k, p) in line.points.iter().enumerate() {
                    cells.entry(cell(*p)).or_default().push((lid, k as u32));
                }
                lines.push((line.points.clone(), line.cut));
            }
        }
        Self { lines, cells }
    }

    /// Sommets au sol à moins de `MERGE_UNITS` de `p`.
    fn near(&self, p: [f64; 2]) -> Vec<(u32, u32)> {
        let (cx, cy) = cell(p);
        let mut out = Vec::new();
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &(l, k) in self.cells.get(&(cx + dx, cy + dy)).map_or(&[][..], Vec::as_slice) {
                    if dist_units(p, self.lines[l as usize].0[k as usize]) <= MERGE_UNITS {
                        out.push((l, k));
                    }
                }
            }
        }
        out.sort_unstable();
        out
    }
}

/// Angle entre deux directions, en degrés (0 = tout droit).
fn turn_deg(d0: [f64; 2], d1: [f64; 2]) -> f64 {
    let n0 = (d0[0] * d0[0] + d0[1] * d0[1]).sqrt();
    let n1 = (d1[0] * d1[0] + d1[1] * d1[1]).sqrt();
    if n0 <= 0.0 || n1 <= 0.0 {
        return 0.0;
    }
    ((d0[0] * d1[0] + d0[1] * d1[1]) / (n0 * n1)).clamp(-1.0, 1.0).acos().to_degrees()
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

/// Issue d'une marche le long d'un ouvrage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stop {
    Abutment,
    DeadEnd,
    Need(TileKey),
    Unresolved(UnresolvedEnd),
}

/// Contexte d'une résolution : graphe, sol, tuiles lues, règles.
struct Ctx<'a> {
    sg: &'a StructureGraph,
    ground: &'a Ground,
    states: &'a FxHashMap<TileKey, &'a TileState>,
    rules: &'a StructureRules,
}

impl Ctx<'_> {
    /// Fin d'un ouvrage au nœud `node`, sans continuation.
    fn classify_end(&self, node: u32) -> Stop {
        let p = self.sg.nodes[node as usize];
        if !self.ground.near(p).is_empty() {
            return Stop::Abutment;
        }
        match self.sg.cut_next.get(&node) {
            Some(t) => self.tile_stop(*t),
            None => Stop::DeadEnd,
        }
    }

    fn tile_stop(&self, t: TileKey) -> Stop {
        match self.states.get(&t) {
            None => Stop::Need(t),
            Some(TileState::Absent) => Stop::Unresolved(UnresolvedEnd::TileAbsent),
            Some(TileState::Failed) => Stop::Unresolved(UnresolvedEnd::TileFailed),
            Some(TileState::Data(_) | TileState::Empty) => Stop::DeadEnd,
        }
    }

    /// Marche depuis `node` en quittant `from_seg` : segments parcourus et issue.
    fn walk(
        &self,
        mut node: u32,
        mut from_seg: u32,
        seeds: &FxHashSet<u32>,
        used: &mut FxHashSet<u32>,
        mut length: f64,
    ) -> (Vec<u32>, u32, Stop) {
        let mut out = Vec::new();
        loop {
            let prev = self.sg.other(from_seg, node);
            let dir = sub(self.sg.nodes[node as usize], self.sg.nodes[prev as usize]);
            let mut best: Option<(bool, f64, u32, u32)> = None;
            for &(seg, next) in &self.sg.adj[node as usize] {
                if used.contains(&seg) {
                    continue;
                }
                let turn = turn_deg(dir, sub(self.sg.nodes[next as usize], self.sg.nodes[node as usize]));
                let seed = seeds.contains(&seg);
                if !seed && turn > self.rules.max_turn_deg {
                    continue;
                }
                // Une graine d'abord, puis le plus faible changement de cap, puis le plus petit id.
                let key = (!seed, turn, seg, next);
                if best.is_none_or(|b| (key.0, key.1, key.2) < (b.0, b.1, b.2)) {
                    best = Some(key);
                }
            }
            let Some((_, _, seg, next)) = best else {
                return (out, node, self.classify_end(node));
            };
            used.insert(seg);
            out.push(seg);
            length += self.sg.segs[seg as usize].len_m;
            node = next;
            from_seg = seg;
            if length > self.rules.max_structure_extent_m {
                return (out, node, Stop::Unresolved(UnresolvedEnd::ExtentLimit));
            }
        }
    }

    /// Voie au sol au-delà de la culée `p`, en s'éloignant dans la direction `dir`, sur
    /// `approach_m` ; tuile à lire si la voie sort des tuiles lues.
    fn approach(&self, p: [f64; 2], dir: [f64; 2]) -> (Vec<[f64; 2]>, Option<TileKey>) {
        let mut out = vec![p];
        let mut length = 0.0;
        let mut at = p;
        let mut heading = dir;
        let mut visited: FxHashSet<(u32, u32)> = FxHashSet::default();
        let max = self.rules.approach_m;
        let mpu = |q: [f64; 2]| self.sg.grid.metres_per_unit(self.sg.grid.to_latlon(q).lat);
        while length < max {
            // Prochain sommet : sur une voie qui touche `at`, dans le sens du plus faible virage.
            let mut best: Option<(f64, u32, u32, i64)> = None;
            for (l, k) in self.ground.near(at) {
                let pts = &self.ground.lines[l as usize].0;
                for step in [-1i64, 1] {
                    let j = k as i64 + step;
                    if j < 0 || j >= pts.len() as i64 || visited.contains(&(l, j as u32)) {
                        continue;
                    }
                    let turn = turn_deg(heading, sub(pts[j as usize], at));
                    if turn > self.rules.max_turn_deg && out.len() > 1 {
                        continue;
                    }
                    if best.is_none_or(|b| (turn, l, j as u32) < (b.0, b.1, b.2)) {
                        best = Some((turn, l, j as u32, step));
                    }
                }
            }
            let Some((_, l, j, step)) = best else {
                // Fin d'une voie coupée au bord d'une tuile non lue : la lire.
                let need = self.ground.near(at).into_iter().find_map(|(l, k)| {
                    let (pts, cut) = &self.ground.lines[l as usize];
                    let last = pts.len() as u32 - 1;
                    let (is_cut, prev) = if k == 0 && cut[0] {
                        (true, pts[1])
                    } else if k == last && cut[1] {
                        (true, pts[last as usize - 1])
                    } else {
                        (false, at)
                    };
                    (is_cut && !self.states.contains_key(&next_tile(self.sg.grid, prev, at)))
                        .then(|| next_tile(self.sg.grid, prev, at))
                });
                return (out, need);
            };
            let pts = &self.ground.lines[l as usize].0;
            visited.insert((l, j));
            visited.insert((l, (j as i64 - step) as u32));
            let q = pts[j as usize];
            let seg = dist_units(at, q) * mpu(at);
            if length + seg >= max && seg > 0.0 {
                let t = (max - length) / seg;
                out.push(lerp(at, q, t));
                return (out, None);
            }
            length += seg;
            heading = sub(q, at);
            at = q;
            out.push(q);
        }
        (out, None)
    }
}

/// Échantillon du chemin apparié sur un ouvrage : distance de parcours et position.
#[derive(Clone, Copy, Debug)]
struct OnStructure {
    d: f64,
    pos: [f64; 2],
}

/// Échantillons « sur ouvrage » du chemin apparié : extrémités de chaque morceau de segment
/// d'ouvrage, à leur distance au prorata (comme `runs`).
fn structure_samples(g: &StreetGraph, path: &MatchedPath) -> Vec<OnStructure> {
    let mut out = Vec::new();
    let obs = &path.obs;
    for t in 0..obs.len().saturating_sub(1) {
        let (d0, d1) = (obs[t].d_m, obs[t + 1].d_m);
        let Some(Step::Route(pieces)) = &path.steps[t] else { continue };
        let w: Vec<f64> = pieces.iter().map(|p| (p.t1 - p.t0).abs() * g.segs[p.seg as usize].len_m).collect();
        let total: f64 = w.iter().sum();
        let mut at = d0;
        for (k, (p, wk)) in pieces.iter().zip(&w).enumerate() {
            let to = if total <= 1e-9 || k + 1 == pieces.len() { d1 } else { at + (d1 - d0) * wk / total };
            let s = g.segs[p.seg as usize];
            if s.level.is_structure() {
                let (a, b) = g.seg_points(p.seg);
                out.push(OnStructure { d: at, pos: lerp(a, b, p.t0) });
                out.push(OnStructure { d: to, pos: lerp(a, b, p.t1) });
            }
            at = to;
        }
    }
    out
}

/// Plages contiguës de tronçons appariés sur un ouvrage.
fn passage_ranges(runs: &[TypedRun]) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = Vec::new();
    for r in runs {
        let on = matches!(r.matched, RunMatch::Matched { element: Some(_), .. });
        if !on {
            continue;
        }
        match out.last_mut() {
            Some(last) if (last.1 - r.from_m).abs() < 1e-9 => last.1 = r.to_m,
            _ => out.push((r.from_m, r.to_m)),
        }
    }
    out
}

/// Axe d'un ouvrage en construction : segments ordonnés et nœuds.
struct Chain {
    segs: Vec<u32>,
    nodes: Vec<u32>,
    stops: [Stop; 2],
}

fn build_chain(ctx: &Ctx<'_>, seeds: &FxHashSet<u32>, start: u32) -> Chain {
    let s0 = ctx.sg.segs[start as usize];
    let mut used: FxHashSet<u32> = FxHashSet::default();
    used.insert(start);
    let (back, back_end, back_stop) = ctx.walk(s0.a, start, seeds, &mut used, s0.len_m);
    let (fwd, _, fwd_stop) = ctx.walk(s0.b, start, seeds, &mut used, s0.len_m);
    let mut segs: Vec<u32> = back.iter().rev().copied().collect();
    segs.push(start);
    segs.extend(fwd);
    let mut nodes = vec![back_end];
    let mut at = back_end;
    for &s in &segs {
        at = ctx.sg.other(s, at);
        nodes.push(at);
    }
    Chain { segs, nodes, stops: [back_stop, fwd_stop] }
}

/// Ouvrage construit, avec sa chaîne dans l'ordre canonique (A → B) et la tuile qui manque à
/// la voie d'approche de chaque extrémité.
struct Built {
    structure: Structure,
    segs: Vec<u32>,
    nodes: Vec<u32>,
    approach_need: [Option<TileKey>; 2],
}

fn make_structure(ctx: &Ctx<'_>, chain: Chain) -> Built {
    let sg = ctx.sg;
    let n = chain.nodes.len();
    // A = extrémité de plus petite coordonnée canonique.
    let (p0, p1) = (sg.nodes[chain.nodes[0] as usize], sg.nodes[chain.nodes[n - 1] as usize]);
    let reversed = (p1[0], p1[1]) < (p0[0], p0[1]);
    let (nodes, segs, stops) = if reversed {
        (
            chain.nodes.iter().rev().copied().collect::<Vec<_>>(),
            chain.segs.iter().rev().copied().collect::<Vec<_>>(),
            [chain.stops[1], chain.stops[0]],
        )
    } else {
        (chain.nodes, chain.segs, chain.stops)
    };
    let pts: Vec<[f64; 2]> = nodes.iter().map(|&k| sg.nodes[k as usize]).collect();
    let axis: Vec<LatLon> = pts.iter().map(|&p| sg.grid.to_latlon(p)).collect();
    let mut chain_m = vec![0.0];
    for &s in &segs {
        chain_m.push(chain_m.last().copied().unwrap_or(0.0) + sg.segs[s as usize].len_m);
    }
    let length_m = chain_m.last().copied().unwrap_or(0.0);
    let mut elements: Vec<StructureElement> = Vec::new();
    for (i, &s) in segs.iter().enumerate() {
        let seg = sg.segs[s as usize];
        match elements.last_mut() {
            Some(e) if e.id == seg.element && e.kind == seg.kind => e.chain_to = chain_m[i + 1],
            _ => elements.push(StructureElement {
                id: seg.element,
                kind: seg.kind,
                chain_from: chain_m[i],
                chain_to: chain_m[i + 1],
            }),
        }
    }
    let mut approach_need = [None, None];
    let mut end = |k: usize| -> StructureEnd {
        let (i, j) = if k == 0 { (0, 1) } else { (n - 1, n - 2) };
        let (p, q) = (pts[i], pts[j]);
        let kind = match stops[k] {
            Stop::Abutment => EndKind::Abutment,
            Stop::DeadEnd => EndKind::DeadEnd,
            // Tuile non lue à la dernière résolution : limite d'extension atteinte.
            Stop::Need(_) => EndKind::Unresolved(UnresolvedEnd::ExtentLimit),
            Stop::Unresolved(u) => EndKind::Unresolved(u),
        };
        let approach = if kind == EndKind::Abutment {
            let (line, need) = ctx.approach(p, sub(p, q));
            approach_need[k] = need;
            line.into_iter().map(|u| sg.grid.to_latlon(u)).collect()
        } else {
            Vec::new()
        };
        StructureEnd { at: sg.grid.to_latlon(p), kind, approach }
    };
    let ends = [end(0), end(1)];
    let id = segs.iter().copied().min().unwrap_or(0);
    Built {
        structure: Structure { id, axis, chain: chain_m, length_m, elements, ends },
        segs,
        nodes,
        approach_need,
    }
}

/// Calcul des ouvrages et passages sur les tuiles lues. `Err(tuiles)` : une extrémité ou une voie
/// d'approche sort des tuiles lues (seulement si `allow_need`) ; sans `allow_need`, l'extrémité
/// reste non résolue (`ExtentLimit`).
pub fn resolve(
    g: &StreetGraph,
    path: &MatchedPath,
    runs: &[TypedRun],
    tiles: &[(TileKey, TileState)],
    rules: &StructureRules,
    allow_need: bool,
) -> Result<StructuresOut, Vec<TileKey>> {
    let ranges = passage_ranges(runs);
    if ranges.is_empty() {
        return Ok(StructuresOut::default());
    }
    let data: Vec<Arc<StreetTile>> = tiles
        .iter()
        .filter_map(|(_, s)| match s {
            TileState::Data(t) => Some(Arc::clone(t)),
            _ => None,
        })
        .collect();
    let states: FxHashMap<TileKey, &TileState> = tiles.iter().map(|(k, s)| (*k, s)).collect();
    let sg = StructureGraph::build(g.grid, &data);
    let lat = sg.nodes.first().map_or(45.0, |p| g.grid.to_latlon(*p).lat);
    let radius = rules.approach_m / g.grid.metres_per_unit(lat).max(1e-6) + 64.0;
    let ground = Ground::build(&data, &sg, radius);
    let ctx = Ctx { sg: &sg, ground: &ground, states: &states, rules };
    let samples = structure_samples(g, path);
    let tol = 2.0 * MERGE_UNITS;

    let mut need: Vec<TileKey> = Vec::new();
    let mut built: Vec<Built> = Vec::new();
    // Segments de chaque ouvrage déjà construit (dédoublonnage, inv. 8).
    let mut owned: FxHashMap<u32, usize> = FxHashMap::default();
    let mut passages: Vec<StructurePassage> = Vec::new();

    for (pid, &(from, to)) in ranges.iter().enumerate() {
        let located: Vec<(f64, [f64; 2], Option<u32>)> = samples
            .iter()
            .filter(|s| s.d >= from - 1e-6 && s.d <= to + 1e-6)
            .map(|s| (s.d, s.pos, sg.locate(s.pos, tol)))
            .collect();
        let seeds: FxHashSet<u32> = located.iter().filter_map(|x| x.2).collect();
        let mut doubts: Vec<PassageDoubt> = Vec::new();
        if located.iter().any(|x| x.2.is_none()) {
            doubts.push(PassageDoubt::ChainBreak);
        }
        let Some(first) = located.iter().find_map(|x| x.2) else {
            // Aucun segment d'ouvrage retrouvé : passage douteux, sans ouvrage.
            doubts.push(PassageDoubt::ChainBreak);
            passages.push(StructurePassage {
                id: pid as PassageId,
                structure: NO_STRUCTURE,
                from_m: from,
                to_m: to,
                chainage: Vec::new(),
                doubts,
            });
            continue;
        };
        let k = match owned.get(&first) {
            Some(&k) => k,
            None => {
                let chain = build_chain(&ctx, &seeds, first);
                for stop in chain.stops {
                    if let Stop::Need(t) = stop {
                        need.push(t);
                    }
                }
                let b = make_structure(&ctx, chain);
                let k = built.len();
                for &s in &b.segs {
                    owned.entry(s).or_insert(k);
                }
                built.push(b);
                k
            }
        };
        let b = &built[k];
        let seg_pos: FxHashMap<u32, usize> = b.segs.iter().enumerate().map(|(i, &s)| (s, i)).collect();
        let mut chainage: Vec<(f64, f64)> = Vec::new();
        for &(d, pos, seg) in &located {
            // Un échantillon localisé sur un autre ouvrage (portail partagé, tube voisin) est
            // projeté sur l'axe s'il en est assez proche ; sinon, coupure de chaîne.
            let i = match seg.and_then(|seg| seg_pos.get(&seg).copied()) {
                Some(i) => Some(i),
                None => (0..b.segs.len())
                    .map(|i| {
                        let (a, c) = (sg.nodes[b.nodes[i] as usize], sg.nodes[b.nodes[i + 1] as usize]);
                        (i, project_on_segment(pos, a, c).dist2)
                    })
                    .min_by(|x, y| x.1.total_cmp(&y.1))
                    .filter(|x| x.1 <= tol * tol)
                    .map(|x| x.0),
            };
            let Some(i) = i else {
                if !doubts.contains(&PassageDoubt::ChainBreak) {
                    doubts.push(PassageDoubt::ChainBreak);
                }
                continue;
            };
            let (a, c) = (sg.nodes[b.nodes[i] as usize], sg.nodes[b.nodes[i + 1] as usize]);
            let pr = project_on_segment(pos, a, c);
            chainage
                .push((d, b.structure.chain[i] + pr.t * (b.structure.chain[i + 1] - b.structure.chain[i])));
        }
        chainage.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        chainage.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-9);
        let progress: f64 = chainage.windows(2).map(|w| (w[1].1 - w[0].1).abs()).sum();
        if chainage.len() >= 2 && progress < rules.min_axis_progress * (to - from) {
            doubts.push(PassageDoubt::AcrossAxis);
        }
        if b.structure.ends.iter().any(|e| matches!(e.kind, EndKind::Unresolved(_))) {
            doubts.push(PassageDoubt::EndUnresolved);
        }
        passages.push(StructurePassage {
            id: pid as PassageId,
            structure: b.structure.id,
            from_m: from,
            to_m: to,
            chainage,
            doubts,
        });
    }

    // Voie d'approche des extrémités qu'aucun passage n'atteint : ses tuiles sont lues au besoin.
    for b in &built {
        let s = &b.structure;
        for (side, need_side) in b.approach_need.iter().enumerate() {
            let Some(t) = *need_side else { continue };
            let end_c = if side == 0 { 0.0 } else { s.length_m };
            let reached = passages.iter().filter(|p| p.structure == s.id).any(|p| {
                let (lo, hi) = p.chain_extent();
                (end_c - lo).abs() <= rules.end_reached_m || (end_c - hi).abs() <= rules.end_reached_m
            });
            if !reached {
                need.push(t);
            }
        }
    }
    if allow_need && !need.is_empty() {
        need.sort_unstable();
        need.dedup();
        return Err(need);
    }

    // Doutes du chemin apparié autour de chaque passage.
    for p in &mut passages {
        let length = built.iter().find(|b| b.structure.id == p.structure).map(|b| b.structure.length_m);
        add_path_doubts(p, length, path, runs, rules);
    }
    Ok(StructuresOut { structures: built.into_iter().map(|b| b.structure).collect(), passages })
}

/// Passage sans ouvrage retrouvé dans le graphe des ouvrages.
pub const NO_STRUCTURE: StructureId = u32::MAX;

/// Doutes du chemin apparié sur un passage ; `length` : longueur de l'ouvrage (culées en chaînage
/// 0 et `length`).
fn add_path_doubts(
    p: &mut StructurePassage,
    length: Option<f64>,
    path: &MatchedPath,
    runs: &[TypedRun],
    rules: &StructureRules,
) {
    let guard = rules.doubt_guard_m;
    let unmatched = runs.iter().any(|r| {
        matches!(r.matched, RunMatch::Unmatched | RunMatch::OffGraph { .. } | RunMatch::NoData { .. })
            && r.to_m > p.from_m - guard
            && r.from_m < p.to_m + guard
    });
    if unmatched {
        p.doubts.push(PassageDoubt::UnmatchedAdjacent);
    }
    let (mut dist_sum, mut dist_n) = (0.0, 0usize);
    let mut chains: FxHashSet<u32> = FxHashSet::default();
    for t in 0..path.obs.len() {
        let d = path.obs[t].d_m;
        if d < p.from_m || d > p.to_m {
            continue;
        }
        if let ObsState::Matched { cand, chain } = path.state[t] {
            dist_sum += cand.dist_m;
            dist_n += 1;
            chains.insert(chain);
        }
    }
    if dist_n > 0 && dist_sum / dist_n as f64 > rules.far_from_axis_m {
        p.doubts.push(PassageDoubt::FarFromAxis);
    }
    // Coupure de chaîne de l'appariement dans le passage.
    if chains.len() > 1 {
        p.doubts.push(PassageDoubt::ChainBreak);
    }
    for t in 0..path.obs.len().saturating_sub(1) {
        if !matches!(path.steps[t], Some(Step::Jump)) {
            continue;
        }
        let (d0, d1) = (path.obs[t].d_m, path.obs[t + 1].d_m);
        let inside = d1 > p.from_m + 1e-6 && d0 < p.to_m - 1e-6;
        let near = d1 >= p.from_m - guard && d0 <= p.to_m + guard;
        let long = haversine_m(path.obs[t].at, path.obs[t + 1].at) > rules.max_end_jump_m;
        // Saut court qui franchit une culée (T03 §6.3 : seul un saut de plus de `max_end_jump_m`
        // y est un doute) : il chevauche une borne du passage et son extrémité sur l'ouvrage est à
        // une culée (chaînage 0 ou `length` à `end_reached_m` près), pas au milieu d'un ouvrage
        // longé par-dessous. C'est un trou du graphe entre la voie au sol et l'ouvrage (ways sans
        // nœud commun), pas un saut dans l'ouvrage.
        let at_abutment = |d: f64| {
            let c = p.chainage_at(d);
            length.is_some_and(|l| c.abs() <= rules.end_reached_m || (l - c).abs() <= rules.end_reached_m)
        };
        let enters = d0 <= p.from_m + 1e-6 && d1 > p.from_m + 1e-6;
        let leaves = d0 < p.to_m - 1e-6 && d1 >= p.to_m - 1e-6;
        let abutment_jump =
            !long && ((enters && !leaves && at_abutment(d1)) || (leaves && !enters && at_abutment(d0)));
        if (inside && !abutment_jump) || (near && long) {
            p.doubts.push(PassageDoubt::JumpInside);
            break;
        }
    }
    p.doubts.sort_unstable();
    p.doubts.dedup();
}

#[cfg(test)]
mod doubt_tests {
    use super::*;
    use crate::typology::matcher::Obs;

    /// Parcours rectiligne vers l'est, une observation tous les `step` mètres.
    fn path(ds: &[f64], jump_at: usize) -> MatchedPath {
        let m_per_deg = haversine_m(LatLon::new(45.0, 0.0), LatLon::new(45.0, 1.0));
        let obs: Vec<Obs> = ds
            .iter()
            .map(|&d| Obs { p: [0.0, 0.0], at: LatLon::new(45.0, d / m_per_deg), d_m: d, no_data: None })
            .collect();
        let mut steps = vec![None; ds.len()];
        steps[jump_at] = Some(Step::Jump);
        MatchedPath { state: vec![ObsState::Unmatched; ds.len()], obs, steps }
    }

    /// Passage de 1 000 à 1 900 m sur un ouvrage de 900 m (chaînage = d − 1 000).
    fn passage() -> StructurePassage {
        StructurePassage {
            id: 0,
            structure: 0,
            from_m: 1_000.0,
            to_m: 1_900.0,
            chainage: (0..=18).map(|k| (1_000.0 + 50.0 * k as f64, 50.0 * k as f64)).collect(),
            doubts: Vec::new(),
        }
    }

    fn doubts(ds: &[f64], jump_at: usize, length: Option<f64>) -> Vec<PassageDoubt> {
        let mut p = passage();
        add_path_doubts(&mut p, length, &path(ds, jump_at), &[], &StructureRules::default());
        p.doubts
    }

    #[test]
    fn a_short_jump_onto_an_abutment_is_not_a_jump_inside() {
        // Saut de 3 m qui franchit la culée d'entrée (trou du graphe entre la voie au sol et le
        // pont) : pas de doute.
        assert!(doubts(&[990.0, 998.5, 1_001.5, 1_010.0], 1, Some(900.0)).is_empty());
        // Même saut à la culée de sortie.
        assert!(doubts(&[1_890.0, 1_898.5, 1_901.5, 1_910.0], 1, Some(900.0)).is_empty());
        // Saut dans l'ouvrage, au milieu : doute.
        assert_eq!(
            doubts(&[1_400.0, 1_405.0, 1_408.0, 1_410.0], 1, Some(900.0)),
            vec![PassageDoubt::JumpInside]
        );
        // Saut long à la culée : doute.
        assert_eq!(doubts(&[960.0, 970.0, 1_001.5, 1_010.0], 1, Some(900.0)), vec![PassageDoubt::JumpInside]);
        // Arrivée au milieu d'un ouvrage longé (pas une culée) : doute, même court.
        let mut p = passage();
        p.chainage = (0..=18).map(|k| (1_000.0 + 50.0 * k as f64, 300.0 + 50.0 * k as f64)).collect();
        add_path_doubts(
            &mut p,
            Some(2_000.0),
            &path(&[990.0, 998.5, 1_001.5, 1_010.0], 1),
            &[],
            &StructureRules::default(),
        );
        assert_eq!(p.doubts, vec![PassageDoubt::JumpInside]);
    }
}
