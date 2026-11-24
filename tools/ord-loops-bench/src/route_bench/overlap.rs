//! Recouvrement d'un itinéraire et de sa référence, en longueur, arête de base par arête de base : fonctions pures.
//!
//! Chaque côté est une liste de morceaux `(arête, fraction de début, fraction de fin)` ; sur une arête, les morceaux
//! d'un côté sont d'abord réunis (une arête prise deux fois ne compte qu'une fois), puis intersectés avec ceux de
//! l'autre côté. Longueur d'un morceau : `|fin − début| × longueur de l'arête`.

use rustc_hash::FxHashMap;

/// Un morceau d'arête de base parcouru.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub edge: u32,
    pub lo: f64,
    pub hi: f64,
}

impl Piece {
    pub fn new(edge: u32, a: f64, b: f64) -> Self {
        let (a, b) = (a.clamp(0.0, 1.0), b.clamp(0.0, 1.0));
        Self { edge, lo: a.min(b), hi: a.max(b) }
    }
}

/// Réunion d'intervalles `[lo, hi]`, triés et disjoints.
pub fn union(mut v: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    v.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(v.len());
    for (lo, hi) in v {
        match out.last_mut() {
            Some(last) if lo <= last.1 => last.1 = last.1.max(hi),
            _ => out.push((lo, hi)),
        }
    }
    out
}

/// Longueur commune de deux réunions triées.
pub fn common_len(a: &[(f64, f64)], b: &[(f64, f64)]) -> f64 {
    let (mut i, mut j, mut s) = (0, 0, 0.0);
    while i < a.len() && j < b.len() {
        let lo = a[i].0.max(b[j].0);
        let hi = a[i].1.min(b[j].1);
        if hi > lo {
            s += hi - lo;
        }
        if a[i].1 < b[j].1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    s
}

/// Un côté réuni par arête.
#[derive(Clone, Debug, Default)]
pub struct Side {
    by_edge: FxHashMap<u32, Vec<(f64, f64)>>,
}

impl Side {
    pub fn of(pieces: &[Piece]) -> Self {
        let mut raw: FxHashMap<u32, Vec<(f64, f64)>> = FxHashMap::default();
        for p in pieces {
            if p.hi > p.lo {
                raw.entry(p.edge).or_default().push((p.lo, p.hi));
            }
        }
        Self { by_edge: raw.into_iter().map(|(e, v)| (e, union(v))).collect() }
    }

    /// Longueur de la réunion, m.
    pub fn len_m(&self, edge_len_m: &dyn Fn(u32) -> f64) -> f64 {
        self.by_edge.iter().map(|(&e, v)| v.iter().map(|(a, b)| b - a).sum::<f64>() * edge_len_m(e)).sum()
    }
}

/// Recouvrement d'un itinéraire et de sa référence.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Overlap {
    /// Longueurs des réunions, m.
    pub route_m: f64,
    pub ref_m: f64,
    pub common_m: f64,
}

impl Overlap {
    pub fn jaccard(&self) -> f64 {
        let d = self.route_m + self.ref_m - self.common_m;
        if d > 0.0 {
            self.common_m / d
        } else {
            0.0
        }
    }

    /// Part de l'itinéraire sur la référence.
    pub fn precision(&self) -> f64 {
        if self.route_m > 0.0 {
            self.common_m / self.route_m
        } else {
            0.0
        }
    }

    /// Part de la référence retrouvée.
    pub fn recall(&self) -> f64 {
        if self.ref_m > 0.0 {
            self.common_m / self.ref_m
        } else {
            0.0
        }
    }
}

pub fn overlap(route: &Side, reference: &Side, edge_len_m: &dyn Fn(u32) -> f64) -> Overlap {
    let common_m = route
        .by_edge
        .iter()
        .filter_map(|(e, a)| reference.by_edge.get(e).map(|b| common_len(a, b) * edge_len_m(*e)))
        .sum();
    Overlap { route_m: route.len_m(edge_len_m), ref_m: reference.len_m(edge_len_m), common_m }
}

/// Kilomètres par case de voie et non revêtus d'une liste de morceaux (sans réunion : une arête prise deux fois compte
/// deux fois, comme la longueur parcourue).
pub fn km_by_way(
    pieces: &[Piece],
    edge_len_m: &dyn Fn(u32) -> f64,
    way_of: &dyn Fn(u32) -> usize,
    offroad: &dyn Fn(u32) -> bool,
) -> ([f64; 7], f64) {
    let mut km = [0.0; 7];
    let mut off = 0.0;
    for p in pieces {
        let l = (p.hi - p.lo) * edge_len_m(p.edge) / 1000.0;
        km[way_of(p.edge).min(6)] += l;
        if offroad(p.edge) {
            off += l;
        }
    }
    (km, off)
}

/// Fractions `(début, fin)` couvertes par le pas `i` d'un itinéraire de `n` pas, comme `trim_piece` du moteur :
/// `os` et `oe` sont les décalages (m, depuis le premier nœud de l'arête) du départ et de l'arrivée, `g` la longueur
/// géométrique de l'arête.
pub fn step_fractions(i: usize, n: usize, reverse: bool, os: f64, oe: f64, g: f64) -> (f64, f64) {
    if g <= 0.0 {
        return (0.0, 1.0);
    }
    let (os, oe) = (os.clamp(0.0, g), oe.clamp(0.0, g));
    let (mut lo, mut hi) = (0.0, g);
    if n == 1 {
        (lo, hi) = (os.min(oe), os.max(oe));
    } else if i == 0 {
        if reverse {
            hi = os;
        } else {
            lo = os;
        }
    } else if i == n - 1 {
        if reverse {
            lo = oe;
        } else {
            hi = oe;
        }
    }
    (lo / g, hi / g)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn len(_: u32) -> f64 {
        1000.0
    }

    #[test]
    fn identical_sides_have_jaccard_one_and_disjoint_zero() {
        let a = [Piece::new(1, 0.0, 1.0), Piece::new(2, 1.0, 0.0)];
        let o = overlap(&Side::of(&a), &Side::of(&a), &len);
        assert_eq!((o.route_m, o.ref_m, o.common_m), (2000.0, 2000.0, 2000.0));
        assert_eq!((o.jaccard(), o.precision(), o.recall()), (1.0, 1.0, 1.0));
        let b = [Piece::new(3, 0.0, 1.0)];
        assert_eq!(overlap(&Side::of(&a), &Side::of(&b), &len).jaccard(), 0.0);
    }

    #[test]
    fn partial_pieces_and_repeated_edges() {
        // Itinéraire : la moitié haute de 1 et toute l'arête 2 ; référence : tout 1, et 2 parcourue deux fois.
        let r = [Piece::new(1, 0.5, 1.0), Piece::new(2, 0.0, 1.0)];
        let f = [Piece::new(1, 0.0, 1.0), Piece::new(2, 0.0, 1.0), Piece::new(2, 1.0, 0.0)];
        let o = overlap(&Side::of(&r), &Side::of(&f), &len);
        assert!((o.route_m - 1500.0).abs() < 1e-9 && (o.ref_m - 2000.0).abs() < 1e-9);
        assert!((o.common_m - 1500.0).abs() < 1e-9);
        assert!((o.jaccard() - 0.75).abs() < 1e-9);
        assert!((o.precision() - 1.0).abs() < 1e-9 && (o.recall() - 0.75).abs() < 1e-9);
    }

    #[test]
    fn union_and_common_of_interval_lists() {
        assert_eq!(union(vec![(0.5, 0.7), (0.0, 0.2), (0.1, 0.3)]), vec![(0.0, 0.3), (0.5, 0.7)]);
        let a = union(vec![(0.0, 0.3), (0.5, 0.7)]);
        let b = union(vec![(0.2, 0.6)]);
        assert!((common_len(&a, &b) - 0.2).abs() < 1e-12);
    }

    #[test]
    fn km_by_way_counts_each_pass() {
        let p = [Piece::new(1, 0.0, 1.0), Piece::new(1, 0.0, 0.5), Piece::new(2, 0.0, 1.0)];
        let (km, off) = km_by_way(&p, &len, &|e| if e == 1 { 6 } else { 4 }, &|e| e == 2);
        assert!((km[6] - 1.5).abs() < 1e-12 && (km[4] - 1.0).abs() < 1e-12 && (off - 1.0).abs() < 1e-12);
    }

    #[test]
    fn step_fractions_follow_trim_piece() {
        // Un seul pas : entre les deux décalages.
        assert_eq!(step_fractions(0, 1, false, 200.0, 800.0, 1000.0), (0.2, 0.8));
        // Premier pas à l'endroit : du départ au bout ; à l'envers : du début au départ.
        assert_eq!(step_fractions(0, 3, false, 200.0, 0.0, 1000.0), (0.2, 1.0));
        assert_eq!(step_fractions(0, 3, true, 200.0, 0.0, 1000.0), (0.0, 0.2));
        // Dernier pas : jusqu'à l'arrivée ; au milieu : toute l'arête.
        assert_eq!(step_fractions(2, 3, false, 0.0, 300.0, 1000.0), (0.0, 0.3));
        assert_eq!(step_fractions(2, 3, true, 0.0, 300.0, 1000.0), (0.3, 1.0));
        assert_eq!(step_fractions(1, 3, true, 10.0, 20.0, 1000.0), (0.0, 1.0));
    }
}
