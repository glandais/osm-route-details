//! Contrôles indépendants de l'appariement (note de conception §9).
//!
//! 1. **Continuité** : deux chaînes de deux arêtes d'équipe contiguës doivent se toucher dans le graphe.
//! 2. **Re-vérification d'un échantillon** : Hausdorff et Fréchet discrets **en haversine** entre la polyligne
//!    d'équipe et la géométrie de la chaîne relue par `GraphFile::geometry`. Rien n'est partagé avec le
//!    matcher : ni index, ni couloir, ni repère local, ni routine de découpe.

use ord_graph::mmap::MappedGraph;

use crate::geo::{haversine, P2};
use crate::matcher::{Chain, Piece};

/// Tolérance de « même position » le long d'une arête, en mètres.
pub const ADJ_TOL_M: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Adjacency {
    SameEdge,
    SharedNode,
    /// Distance haversine entre les deux positions, en mètres.
    Gap(f64),
}

fn edge_deg(gf: &MappedGraph, edge: u32) -> Vec<P2> {
    let (a, b) = gf.edge_ends(edge);
    let e7 = |p: (i32, i32)| [f64::from(p.0) * 1e-7, f64::from(p.1) * 1e-7];
    let mut v = vec![e7(gf.node_pos(a))];
    v.extend(gf.geometry(edge).into_iter().map(e7));
    v.push(e7(gf.node_pos(b)));
    v
}

fn cumulative(pts: &[P2]) -> Vec<f64> {
    let mut c = vec![0.0];
    for w in pts.windows(2) {
        c.push(c.last().copied().unwrap_or(0.0) + haversine(w[0], w[1]));
    }
    c
}

/// Position `(lat, lon)` à la fraction `t` (0..=1) de la longueur d'une polyligne.
fn at_fraction(pts: &[P2], cum: &[f64], t: f64) -> P2 {
    let total = *cum.last().expect("au moins un point");
    let want = t.clamp(0.0, 1.0) * total;
    let i = cum.partition_point(|&c| c < want).clamp(1, pts.len() - 1);
    let (c0, c1) = (cum[i - 1], cum[i]);
    let u = if c1 > c0 { (want - c0) / (c1 - c0) } else { 0.0 };
    [pts[i - 1][0] + u * (pts[i][0] - pts[i - 1][0]), pts[i - 1][1] + u * (pts[i][1] - pts[i - 1][1])]
}

/// Nœuds d'extrémité (ids globaux) à 2 m ou moins de la position `off` sur `edge` (longueur `elen`).
fn nodes_at(gf: &MappedGraph, edge: u32, off: f32, elen: f32) -> Vec<u32> {
    let (a, b) = gf.edge_ends(edge);
    let mut v = Vec::new();
    if f64::from(off) <= ADJ_TOL_M {
        v.push(a);
    }
    if f64::from(elen - off) <= ADJ_TOL_M {
        v.push(b);
    }
    v
}

/// La fin du morceau `prev` et le début du morceau `next` sont-ils au même endroit du graphe ?
pub fn adjacency(gf: &MappedGraph, prev: &Piece, next: &Piece) -> Adjacency {
    if prev.edge == next.edge && f64::from((prev.to - next.from).abs()) <= ADJ_TOL_M {
        return Adjacency::SameEdge;
    }
    let end = nodes_at(gf, prev.edge, prev.to, prev.elen);
    let start = nodes_at(gf, next.edge, next.from, next.elen);
    if end.iter().any(|n| start.contains(n)) {
        return Adjacency::SharedNode;
    }
    let pos = |p: &Piece, off: f32| {
        let pts = edge_deg(gf, p.edge);
        let cum = cumulative(&pts);
        at_fraction(&pts, &cum, f64::from(off) / f64::from(p.elen.max(1e-6)))
    };
    Adjacency::Gap(haversine(pos(prev, prev.to), pos(next, next.from)))
}

/// Raccords non adjacents à l'intérieur d'une chaîne (devrait être 0 : un écart est un bogue).
pub fn internal_gaps(gf: &MappedGraph, chain: &Chain) -> usize {
    chain.pieces.windows(2).filter(|w| matches!(adjacency(gf, &w[0], &w[1]), Adjacency::Gap(_))).count()
}

// --- Re-vérification ------------------------------------------------------------------------------

/// Points maximum par ligne rééchantillonnée (borne le coût quadratique de Fréchet).
const MAX_POINTS: usize = 200;
const RESAMPLE_M: f64 = 1.0;

fn resample(line: &[P2]) -> Vec<P2> {
    let cum = cumulative(line);
    let total = *cum.last().expect("au moins un point");
    let n = ((total / RESAMPLE_M).ceil() as usize).clamp(1, MAX_POINTS - 1);
    (0..=n).map(|k| at_fraction(line, &cum, k as f64 / n as f64)).collect()
}

/// Géométrie de la chaîne, dans le sens de parcours, relue depuis le graphe.
fn chain_line(gf: &MappedGraph, chain: &Chain) -> Vec<P2> {
    let mut out: Vec<P2> = Vec::new();
    for p in &chain.pieces {
        let pts = edge_deg(gf, p.edge);
        let cum = cumulative(&pts);
        let elen = f64::from(p.elen.max(1e-6));
        let (t0, t1) = (f64::from(p.from) / elen, f64::from(p.to) / elen);
        let total = *cum.last().expect("au moins un point");
        let (lo, hi) = (t0.min(t1).clamp(0.0, 1.0), t0.max(t1).clamp(0.0, 1.0));
        let mut v = vec![at_fraction(&pts, &cum, lo)];
        v.extend(pts.iter().zip(&cum).filter(|(_, &c)| c > lo * total && c < hi * total).map(|(p, _)| *p));
        v.push(at_fraction(&pts, &cum, hi));
        if t0 > t1 {
            v.reverse();
        }
        for q in v {
            if out.last().is_none_or(|l| haversine(*l, q) > 1e-3) {
                out.push(q);
            }
        }
    }
    out
}

fn path_len(line: &[P2]) -> f64 {
    line.windows(2).map(|w| haversine(w[0], w[1])).sum()
}

fn directed_hausdorff(a: &[P2], b: &[P2]) -> f64 {
    a.iter().map(|p| b.iter().map(|q| haversine(*p, *q)).fold(f64::INFINITY, f64::min)).fold(0.0, f64::max)
}

/// Hausdorff discret symétrique entre deux listes de points.
pub fn discrete_hausdorff(a: &[P2], b: &[P2]) -> f64 {
    directed_hausdorff(a, b).max(directed_hausdorff(b, a))
}

/// Fréchet discret (programmation dynamique, ligne roulante).
pub fn discrete_frechet(a: &[P2], b: &[P2]) -> f64 {
    let mut prev = vec![0.0f64; b.len()];
    for (i, p) in a.iter().enumerate() {
        let mut cur = vec![0.0f64; b.len()];
        for (j, q) in b.iter().enumerate() {
            let d = haversine(*p, *q);
            cur[j] = match (i, j) {
                (0, 0) => d,
                (0, _) => cur[j - 1].max(d),
                (_, 0) => prev[0].max(d),
                _ => prev[j].min(prev[j - 1]).min(cur[j - 1]).max(d),
            };
        }
        prev = cur;
    }
    prev[b.len() - 1]
}

#[derive(Clone, Copy, Debug)]
pub struct Verification {
    pub hausdorff_m: f64,
    pub frechet_m: f64,
    /// Longueur haversine de la chaîne sur celle de `g`.
    pub len_ratio: f64,
}

/// Compare la polyligne d'équipe (`g`, `(lat, lon)` degrés) à la chaîne appariée.
pub fn verify_sample(gf: &MappedGraph, g: &[P2], chain: &Chain) -> Option<Verification> {
    let line = chain_line(gf, chain);
    if line.len() < 2 || g.len() < 2 {
        return None;
    }
    let (ga, gb) = (resample(g), resample(&line));
    let (lg, lc) = (path_len(g), path_len(&line));
    Some(Verification {
        hausdorff_m: discrete_hausdorff(&ga, &gb),
        frechet_m: discrete_frechet(&ga, &gb),
        len_ratio: if lg > 0.0 { lc / lg } else { f64::NAN },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(offset_m: f64) -> Vec<P2> {
        // Segment est-ouest de ~100 m près de Nantes, décalé vers le nord de `offset_m`.
        let dlat = offset_m / 111_195.0;
        vec![[47.2 + dlat, -1.55], [47.2 + dlat, -1.55 + 0.0013]]
    }

    #[test]
    fn hausdorff_and_frechet_on_parallel_lines() {
        let (a, b) = (resample(&line(0.0)), resample(&line(2.0)));
        assert!((discrete_hausdorff(&a, &b) - 2.0).abs() < 0.05);
        assert!((discrete_frechet(&a, &b) - 2.0).abs() < 0.05);
        assert!(discrete_hausdorff(&a, &a) < 1e-9);
    }

    #[test]
    fn frechet_sees_the_order_hausdorff_ignores() {
        let a = resample(&line(0.0));
        let mut b = a.clone();
        b.reverse();
        // Même ensemble de points : Hausdorff nul ; parcourus à l'envers : Fréchet ≈ longueur.
        assert!(discrete_hausdorff(&a, &b) < 1e-9);
        assert!(discrete_frechet(&a, &b) > 90.0);
    }

    #[test]
    fn resample_keeps_endpoints_and_bounds_points() {
        let r = resample(&line(0.0));
        assert!(r.len() <= MAX_POINTS);
        assert!(haversine(r[0], line(0.0)[0]) < 1e-6);
        assert!(haversine(*r.last().unwrap(), line(0.0)[1]) < 1e-6);
        let long = vec![[47.0, -1.0], [47.0, -0.5]];
        assert_eq!(resample(&long).len(), MAX_POINTS);
    }
}
