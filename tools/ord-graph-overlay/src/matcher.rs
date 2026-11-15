//! Appariement d'une arête d'équipe (polyligne `g`, déjà recalée sur la rue) à une chaîne d'arêtes du graphe.
//!
//! Méthode (note de conception §2) : bouts recalés, recherche de chaîne **dans un couloir** de 5 m autour de
//! `g`, sélection **par la géométrie** (Hausdorff puis écart de longueur), validation, détection d'ambiguïté.
//! Tout se passe dans un repère local en mètres centré sur le premier point de `g`.

use std::collections::BinaryHeap;

use ord_graph::attrs::HighwayClass;
use ord_graph::mmap::MappedGraph;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::geo::{densify, dist, dist_to_polyline, hausdorff, polyline_len, seg_project, Frame, P2};
use crate::index::{edge_points_e7, Hit, SegIndex};

/// Largeur du couloir : arêtes entièrement à cette distance ou moins de `g`.
pub const CORRIDOR_M: f64 = 5.0;
/// Nœud OSM considéré comme le bout de `g`.
pub const NODE_SNAP_M: f64 = 1.5;
/// Rayon de recalage d'un bout sur l'intérieur d'une arête.
pub const EDGE_SNAP_M: f64 = 8.0;
/// Cap toléré entre `g` et l'arête au bout (cosinus de 45°).
const HEADING_COS: f64 = std::f64::consts::FRAC_1_SQRT_2;
/// Positions de bout projetées gardées.
const MAX_EDGE_STATES: usize = 3;
/// Variante « sinon, et en plus » : ajouter les positions projetées même quand un nœud est à 1,5 m.
const EDGE_STATES_WITH_NODES: bool = false;
pub const HAUSDORFF_MAX_M: f64 = 3.0;
pub const LEN_TOL_ABS_M: f64 = 5.0;
pub const LEN_TOL_REL: f64 = 0.05;
/// Deux chaînes valides à moins de cet écart de Hausdorff : ambiguës.
pub const AMBIGUITY_M: f64 = 0.5;
/// Un morceau plus court n'est pas une arête empruntée (bruit de recalage aux carrefours).
pub const MIN_PIECE_M: f64 = 1.0;
/// Pas d'échantillonnage du Hausdorff.
const HAUSDORFF_STEP_M: f64 = 0.25;
/// Pas des requêtes d'index le long de `g`, et du contrôle « chaque point a une arête à 5 m ».
const QUERY_STEP_M: f64 = 5.0;
const COVERAGE_STEP_M: f64 = 10.0;

/// Un morceau de chaîne : l'arête de base, de `from` à `to` mètres depuis son extrémité `a`
/// (`from < to` : sens a→b ; `from > to` : sens b→a), et sa longueur dans le repère de l'appariement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub edge: u32,
    pub from: f32,
    pub to: f32,
    pub elen: f32,
}

impl Piece {
    pub fn forward(&self) -> bool {
        self.to >= self.from
    }
    pub fn len(&self) -> f64 {
        f64::from((self.to - self.from).abs())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fail {
    NoOsmEdge,
    ChainNotConnected,
    Geometry,
    Ambiguous,
}

/// Famille d'une classe OSM, pour le contrôle de type de voie.
pub fn is_road_class(c: HighwayClass) -> bool {
    use HighwayClass::*;
    matches!(
        c,
        Motorway
            | Trunk
            | Primary
            | Secondary
            | Tertiary
            | Unclassified
            | Residential
            | LivingStreet
            | Service
            | Road
            | Busway
    )
}

/// Chaîne retenue, avec ce que l'agrégation doit savoir sans rouvrir le graphe.
#[derive(Clone, Debug)]
pub struct Chain {
    pub pieces: Vec<Piece>,
    pub hausdorff: f32,
    /// Mètres sur des arêtes de classe « route » / « hors route ».
    pub road_m: f32,
    pub nonroad_m: f32,
    /// Mètres (morceaux d'au moins 1 m) dont le sens n'est pas ouvert au vélo.
    pub closed_m: f32,
}

struct EdgeGeo {
    id: u32,
    a: u32,
    b: u32,
    pts: Vec<P2>,
    cum: Vec<f64>,
    len: f64,
    inside: bool,
    dev: f64,
}

impl EdgeGeo {
    fn load(gf: &MappedGraph, frame: &Frame, id: u32, g: &[P2]) -> Self {
        let pts: Vec<P2> = edge_points_e7(gf, id).iter().map(|&(la, lo)| frame.xy_e7(la, lo)).collect();
        let mut cum = Vec::with_capacity(pts.len());
        let mut acc = 0.0;
        for (i, p) in pts.iter().enumerate() {
            if i > 0 {
                acc += dist(pts[i - 1], *p);
            }
            cum.push(acc);
        }
        let (a, b) = gf.edge_ends(id);
        let (mut worst, mut sum, mut n) = (0.0f64, 0.0f64, 0u32);
        densify(&pts, QUERY_STEP_M, |p| {
            let d = dist_to_polyline(p, g);
            worst = worst.max(d);
            sum += d;
            n += 1;
        });
        Self { id, a, b, len: acc, inside: worst <= CORRIDOR_M, dev: sum / f64::from(n.max(1)), pts, cum }
    }

    fn point_at(&self, off: f64) -> P2 {
        let off = off.clamp(0.0, self.len);
        let i = self.cum.partition_point(|&c| c < off).clamp(1, self.pts.len().saturating_sub(1).max(1));
        if self.pts.len() < 2 {
            return self.pts[0];
        }
        let (c0, c1) = (self.cum[i - 1], self.cum[i]);
        let t = if c1 > c0 { (off - c0) / (c1 - c0) } else { 0.0 };
        let (p, q) = (self.pts[i - 1], self.pts[i]);
        [p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])]
    }

    /// Points de `from` à `to` (dans cet ordre), extrémités comprises.
    fn cut(&self, from: f64, to: f64, out: &mut Vec<P2>) {
        let (lo, hi) = (from.min(to), from.max(to));
        let mut v = vec![self.point_at(lo)];
        for (i, &c) in self.cum.iter().enumerate() {
            if c > lo && c < hi {
                v.push(self.pts[i]);
            }
        }
        v.push(self.point_at(hi));
        if from > to {
            v.reverse();
        }
        out.extend(v);
    }

    /// `(distance, abscisse, tangente unitaire a→b)` du point de l'arête le plus proche de `p`.
    fn project(&self, p: P2) -> (f64, f64, P2) {
        let mut best = (f64::INFINITY, 0.0, [1.0, 0.0]);
        for (i, w) in self.pts.windows(2).enumerate() {
            let (d, s) = seg_project(p, w[0], w[1]);
            if d < best.0 {
                let l = dist(w[0], w[1]).max(1e-9);
                best = (d, self.cum[i] + s, [(w[1][0] - w[0][0]) / l, (w[1][1] - w[0][1]) / l]);
            }
        }
        best
    }
}

#[derive(Clone, Copy, Debug)]
enum End {
    Node(u32),
    /// Position sur l'arête `cands[idx]`, à `off` mètres de `a`, parcourue a→b si `fwd`.
    Edge {
        idx: usize,
        off: f64,
        fwd: bool,
    },
}

fn sample_every(line: &[P2], step: f64) -> Vec<P2> {
    let mut out = vec![line[0]];
    let mut next = step;
    let mut acc = 0.0;
    for w in line.windows(2) {
        let l = dist(w[0], w[1]);
        while acc + l >= next && l > 0.0 {
            let t = (next - acc) / l;
            out.push([w[0][0] + t * (w[1][0] - w[0][0]), w[0][1] + t * (w[1][1] - w[0][1])]);
            next += step;
        }
        acc += l;
    }
    let last = *line.last().expect("au moins un point");
    if dist(*out.last().expect("au moins un point"), last) > 0.5 {
        out.push(last);
    }
    out
}

/// Cap unitaire de `g` sur ses 10 premiers (`start`) ou derniers mètres.
fn heading(g: &[P2], start: bool) -> Option<P2> {
    let pts: Vec<P2> = if start { g.to_vec() } else { g.iter().rev().copied().collect() };
    let o = pts[0];
    let far = pts.iter().find(|p| dist(o, **p) >= 10.0).or(pts.last())?;
    let d = dist(o, *far);
    if d < 0.5 {
        return None;
    }
    let (dx, dy) = ((far[0] - o[0]) / d, (far[1] - o[1]) / d);
    Some(if start { [dx, dy] } else { [-dx, -dy] })
}

/// États possibles d'un bout : nœuds à 1,5 m, sinon positions projetées sur des arêtes.
fn end_states(cands: &[EdgeGeo], p: P2, head: Option<P2>) -> Vec<End> {
    let mut nodes: Vec<u32> = Vec::new();
    for e in cands {
        if dist(e.pts[0], p) <= NODE_SNAP_M && !nodes.contains(&e.a) {
            nodes.push(e.a);
        }
        if dist(*e.pts.last().expect("arête non vide"), p) <= NODE_SNAP_M && !nodes.contains(&e.b) {
            nodes.push(e.b);
        }
    }
    nodes.sort_unstable();
    let mut states: Vec<End> = nodes.into_iter().map(End::Node).collect();
    if !states.is_empty() && !EDGE_STATES_WITH_NODES {
        return states;
    }
    let mut on_edge: Vec<(f64, End)> = Vec::new();
    for (idx, e) in cands.iter().enumerate() {
        let (d, off, t) = e.project(p);
        if d > EDGE_SNAP_M {
            continue;
        }
        match head {
            Some(h) => {
                let cos = t[0] * h[0] + t[1] * h[1];
                if cos.abs() >= HEADING_COS {
                    on_edge.push((d, End::Edge { idx, off, fwd: cos > 0.0 }));
                }
            }
            None => {
                on_edge.push((d, End::Edge { idx, off, fwd: true }));
                on_edge.push((d, End::Edge { idx, off, fwd: false }));
            }
        }
    }
    on_edge.sort_by(|x, y| x.0.total_cmp(&y.0).then_with(|| edge_key(&x.1).cmp(&edge_key(&y.1))));
    on_edge.truncate(MAX_EDGE_STATES);
    states.extend(on_edge.into_iter().map(|(_, e)| e));
    states
}

fn edge_key(e: &End) -> (usize, bool) {
    match e {
        End::Node(n) => (*n as usize, false),
        End::Edge { idx, fwd, .. } => (*idx, *fwd),
    }
}

/// Graphe du couloir : arêtes entièrement dedans, indexées dans `cands`.
struct Corridor {
    adj: FxHashMap<u32, Vec<(u32, usize)>>,
    /// Coût d'une arête : longueur, majorée de l'écart moyen au carré (départage deux parallèles dans le
    /// couloir ; la sélection finale reste géométrique).
    cost: Vec<f64>,
}

impl Corridor {
    fn new(cands: &[EdgeGeo]) -> Self {
        let mut adj: FxHashMap<u32, Vec<(u32, usize)>> = FxHashMap::default();
        let mut cost = Vec::with_capacity(cands.len());
        for (i, e) in cands.iter().enumerate() {
            cost.push(e.len * (1.0 + e.dev * e.dev / 4.0));
            if e.inside && e.a != e.b {
                adj.entry(e.a).or_default().push((e.b, i));
                adj.entry(e.b).or_default().push((e.a, i));
            }
        }
        Self { adj, cost }
    }

    /// Dijkstra depuis `source` ; `banned` : arête interdite. Rend coûts et prédécesseurs `(nœud, arête)`.
    fn search(
        &self,
        source: u32,
        init: f64,
        banned: Option<usize>,
        limit: f64,
    ) -> (FxHashMap<u32, f64>, FxHashMap<u32, (u32, usize)>) {
        let mut best: FxHashMap<u32, f64> = FxHashMap::default();
        let mut pred: FxHashMap<u32, (u32, usize)> = FxHashMap::default();
        let mut heap = BinaryHeap::new();
        best.insert(source, init);
        heap.push(std::cmp::Reverse((init.to_bits(), source)));
        while let Some(std::cmp::Reverse((bits, u))) = heap.pop() {
            let d = f64::from_bits(bits);
            if best.get(&u).is_some_and(|&b| d > b) {
                continue;
            }
            for &(v, ei) in self.adj.get(&u).map_or(&[][..], Vec::as_slice) {
                if Some(ei) == banned {
                    continue;
                }
                let nd = d + self.cost[ei];
                if nd <= limit && best.get(&v).is_none_or(|&b| nd < b) {
                    best.insert(v, nd);
                    pred.insert(v, (u, ei));
                    heap.push(std::cmp::Reverse((nd.to_bits(), v)));
                }
            }
        }
        (best, pred)
    }

    /// Arêtes du chemin de la source à `target`, dans l'ordre, avec le nœud de départ de chacune.
    fn path(pred: &FxHashMap<u32, (u32, usize)>, target: u32) -> Vec<(u32, usize)> {
        let mut out = Vec::new();
        let mut v = target;
        while let Some(&(u, ei)) = pred.get(&v) {
            out.push((u, ei));
            v = u;
        }
        out.reverse();
        out
    }
}

fn full_piece(e: &EdgeGeo, from_node: u32) -> Piece {
    let fwd = from_node == e.a;
    let (from, to) = if fwd { (0.0, e.len) } else { (e.len, 0.0) };
    Piece { edge: e.id, from: from as f32, to: to as f32, elen: e.len as f32 }
}

struct Eval {
    pieces: Vec<Piece>,
    h: f64,
    len: f64,
    valid: bool,
}

impl Eval {
    fn rank(&self, trv: f64) -> (i64, i64) {
        ((self.h * 10.0).round() as i64, ((self.len - trv).abs() * 1000.0).round() as i64)
    }
}

fn edge_set(pieces: &[Piece]) -> Vec<u32> {
    let mut v: Vec<u32> = pieces.iter().filter(|p| p.len() >= MIN_PIECE_M).map(|p| p.edge).collect();
    v.sort_unstable();
    v.dedup();
    v
}

struct Ctx<'a> {
    cands: &'a [EdgeGeo],
    by_id: FxHashMap<u32, usize>,
    g: &'a [P2],
    trv: f64,
    step: f64,
}

impl Ctx<'_> {
    fn evaluate(&self, pieces: Vec<Piece>) -> Eval {
        let mut line: Vec<P2> = Vec::new();
        for p in &pieces {
            let e = &self.cands[self.by_id[&p.edge]];
            let mut cut = Vec::new();
            e.cut(f64::from(p.from), f64::from(p.to), &mut cut);
            for q in cut {
                if line.last().is_none_or(|l| dist(*l, q) > 1e-9) {
                    line.push(q);
                }
            }
        }
        let len: f64 = pieces.iter().map(Piece::len).sum();
        let h =
            if line.len() < 2 { f64::INFINITY } else { hausdorff(self.g, &line, self.step, HAUSDORFF_MAX_M) };
        let tol = LEN_TOL_ABS_M.max(LEN_TOL_REL * self.trv);
        let valid = h <= HAUSDORFF_MAX_M && (len - self.trv).abs() <= tol;
        Eval { pieces, h, len, valid }
    }
}

/// Apparie `g` (`(lat, lon)` en degrés, sens de parcours) à une chaîne du graphe. `trv` : mètres parcourus
/// annoncés par l'export (`NaN` : on prend la longueur de `g`).
pub fn match_geometry(gf: &MappedGraph, idx: &SegIndex, g_deg: &[P2], trv: f64) -> Result<Chain, Fail> {
    let frame = Frame::new(g_deg[0][0], g_deg[0][1]);
    let g: Vec<P2> = g_deg.iter().map(|p| frame.xy(p[0], p[1])).collect();
    let glen = polyline_len(&g);
    let trv = if trv.is_finite() && trv > 0.0 { trv } else { glen };

    // Candidats : arêtes à 8 m ou moins d'un point de `g` tous les 5 m ; chaque point tous les 10 m doit en
    // avoir une à 5 m.
    let mut hits: Vec<Hit> = Vec::new();
    let mut cand_ids: FxHashSet<u32> = FxHashSet::default();
    let samples = sample_every(&g, QUERY_STEP_M);
    let per_cov = (COVERAGE_STEP_M / QUERY_STEP_M) as usize;
    for (k, s) in samples.iter().enumerate() {
        hits.clear();
        let (lat, lon) = frame.latlon(*s);
        idx.near(lat, lon, EDGE_SNAP_M, &mut hits);
        let is_cov = k % per_cov == 0 || k + 1 == samples.len();
        if is_cov && !hits.iter().any(|h| f64::from(h.dist) <= CORRIDOR_M + 0.05) {
            return Err(Fail::NoOsmEdge);
        }
        cand_ids.extend(hits.iter().map(|h| h.edge));
    }
    let mut ids: Vec<u32> = cand_ids.into_iter().collect();
    ids.sort_unstable();
    let cands: Vec<EdgeGeo> = ids.iter().map(|&e| EdgeGeo::load(gf, &frame, e, &g)).collect();
    let by_id: FxHashMap<u32, usize> = cands.iter().enumerate().map(|(i, e)| (e.id, i)).collect();
    let step = HAUSDORFF_STEP_M.max(glen / 6000.0);
    let ctx = Ctx { cands: &cands, by_id, g: &g, trv, step };

    let starts = end_states(&cands, g[0], heading(&g, true));
    let ends = end_states(&cands, *g.last().expect("au moins un point"), heading(&g, false));
    if starts.is_empty() || ends.is_empty() {
        return Err(Fail::NoOsmEdge);
    }
    let corridor = Corridor::new(&cands);
    let limit = 3.0 * trv + 50.0;

    // Une chaîne par couple (départ, arrivée).
    let mut evals: Vec<Eval> = Vec::new();
    let mut seen: Vec<Vec<(u32, i32, i32)>> = Vec::new();
    let mut push = |pieces: Vec<Piece>, evals: &mut Vec<Eval>| {
        let pieces: Vec<Piece> = pieces.into_iter().filter(|p| p.len() >= 0.05).collect();
        if pieces.is_empty() {
            return;
        }
        let sig: Vec<(u32, i32, i32)> = pieces
            .iter()
            .map(|p| (p.edge, (p.from * 10.0).round() as i32, (p.to * 10.0).round() as i32))
            .collect();
        if !seen.contains(&sig) {
            seen.push(sig);
            evals.push(ctx.evaluate(pieces));
        }
    };
    let mut connected = false;
    for s in &starts {
        let (src, init, first): (u32, f64, Option<Piece>) = match *s {
            End::Node(n) => (n, 0.0, None),
            End::Edge { idx, off, fwd } => {
                let e = &cands[idx];
                let (node, cost, to) = if fwd { (e.b, e.len - off, e.len) } else { (e.a, off, 0.0) };
                (node, cost, Some(Piece { edge: e.id, from: off as f32, to: to as f32, elen: e.len as f32 }))
            }
        };
        let (best, pred) = corridor.search(src, init, None, limit);
        for t in &ends {
            // Même arête aux deux bouts, dans le bon ordre : un seul morceau.
            if let (End::Edge { idx: i1, off: o1, fwd: f1 }, End::Edge { idx: i2, off: o2, fwd: f2 }) = (s, t)
            {
                if i1 == i2 && f1 == f2 && (if *f1 { o2 > o1 } else { o2 < o1 }) {
                    let e = &cands[*i1];
                    connected = true;
                    push(
                        vec![Piece { edge: e.id, from: *o1 as f32, to: *o2 as f32, elen: e.len as f32 }],
                        &mut evals,
                    );
                }
            }
            let (entry, last): (u32, Option<Piece>) = match *t {
                End::Node(m) => (m, None),
                End::Edge { idx, off, fwd } => {
                    let e = &cands[idx];
                    let (node, from) = if fwd { (e.a, 0.0) } else { (e.b, e.len) };
                    (node, Some(Piece { edge: e.id, from: from as f32, to: off as f32, elen: e.len as f32 }))
                }
            };
            let Some(&c) = best.get(&entry) else { continue };
            if c > limit {
                continue;
            }
            connected = true;
            let mut pieces: Vec<Piece> = first.into_iter().collect();
            for (u, ei) in Corridor::path(&pred, entry) {
                pieces.push(full_piece(&cands[ei], u));
            }
            pieces.extend(last);
            push(pieces, &mut evals);
        }
    }
    if evals.is_empty() {
        return Err(if connected { Fail::Geometry } else { Fail::ChainNotConnected });
    }
    let valid: Vec<&Eval> = evals.iter().filter(|e| e.valid).collect();
    let Some(best) = valid.iter().min_by_key(|e| e.rank(trv)).copied() else {
        return Err(Fail::Geometry);
    };
    // Ambiguïté 1 : une autre chaîne valide, d'un autre ensemble d'arêtes, à moins de 0,5 m.
    let best_set = edge_set(&best.pieces);
    if valid.iter().any(|e| e.h <= best.h + AMBIGUITY_M && edge_set(&e.pieces) != best_set) {
        return Err(Fail::Ambiguous);
    }
    // Ambiguïté 2 : une arête pleine de la chaîne remplaçable par un autre chemin du couloir.
    for (i, p) in best.pieces.iter().enumerate() {
        let e = &cands[ctx.by_id[&p.edge]];
        let full = p.len() >= e.len - 0.05 && e.inside;
        if !full {
            continue;
        }
        let (u, v) = if p.forward() { (e.a, e.b) } else { (e.b, e.a) };
        let (reach, pred) = corridor.search(u, 0.0, Some(ctx.by_id[&p.edge]), limit);
        if !reach.contains_key(&v) {
            continue;
        }
        let mut alt: Vec<Piece> = best.pieces[..i].to_vec();
        for (n, ei) in Corridor::path(&pred, v) {
            alt.push(full_piece(&cands[ei], n));
        }
        alt.extend_from_slice(&best.pieces[i + 1..]);
        let ev = ctx.evaluate(alt);
        if ev.valid && ev.h <= best.h + AMBIGUITY_M && edge_set(&ev.pieces) != best_set {
            return Err(Fail::Ambiguous);
        }
    }

    // Ce que l'agrégation doit savoir de la chaîne.
    let (mut road_m, mut nonroad_m, mut closed_m) = (0.0f64, 0.0f64, 0.0f64);
    for p in &best.pieces {
        let attrs = gf.edge(p.edge).attrs;
        let l = p.len();
        match attrs.class() {
            Some(c) if is_road_class(c) => road_m += l,
            Some(_) => nonroad_m += l,
            None => {}
        }
        let open = if p.forward() { attrs.fwd_ok() } else { attrs.bwd_ok() };
        if !open && l >= MIN_PIECE_M {
            closed_m += l;
        }
    }
    Ok(Chain {
        pieces: best.pieces.clone(),
        hausdorff: best.h as f32,
        road_m: road_m as f32,
        nonroad_m: nonroad_m as f32,
        closed_m: closed_m as f32,
    })
}
