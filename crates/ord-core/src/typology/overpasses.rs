//! Ponts au-dessus du parcours (T03 §6.4, règles 2026.11.6) : croisements de la trace avec les
//! lignes `bridge` de la couche des rues, toutes natures (route, voie fermée au vélo, rail).
//!
//! Le graphe par niveaux (§4.2) ne relie un pont au sol que par ses culées : un pont qui croise la
//! trace par un sommet intérieur l'enjambe. Ici on ne lit que la géométrie, sur toutes les tuiles
//! lues (couloir et extensions), sans filtre de classe : un pont rail ou autoroutier fait la même
//! bosse dans le MNT qu'un pont routier. La décision (trace au sol, angle, culée, fenêtre) revient à
//! la fusion (T04 §3.4), qui lit les tronçons et les passages.
//!
//! Choix simples, notés :
//! - la trace est la polyligne des observations (tous les points d'entrée y sont) : c'est sur elle
//!   que l'altitude est lue ;
//! - une ligne coupée au bord de tuile n'a pas d'extrémité réelle de ce côté (`cut`), et une
//!   extrémité posée sur une autre ligne de pont (≤ `MERGE_UNITS`, way découpé au-dessus de la
//!   voie) est une jonction du même tablier : la distance à la culée ne lit que les autres ;
//! - un même croisement lu dans deux tuiles (géométrie tamponnée au bord) est compté une fois
//!   (moins de 1 m d'écart en distance de parcours).

use std::sync::Arc;

use ord_domain::geo::{dist_units, project_on_segment, Grid};
use ord_domain::street::{StreetTile, StructureTag};
use rstar::primitives::{GeomWithData, Line};
use rstar::{RTree, AABB};

use super::graph::MERGE_UNITS;
use super::matcher::Obs;
use super::TileState;

/// Segment de pont indexé : (tuile, ligne, segment).
type BridgeSeg = GeomWithData<Line<[f64; 2]>, (usize, usize, usize)>;

/// Un pont des tuiles qui croise la trace.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Overpass {
    /// Distance de parcours du croisement.
    pub d_m: f64,
    /// Angle entre la trace et le pont, dans `[0°, 90°]`.
    pub angle_deg: f64,
    /// Distance du croisement à l'extrémité réelle (non coupée) la plus proche du morceau de ligne
    /// croisé ; `f64::INFINITY` si les deux extrémités sont coupées au bord de tuile.
    pub end_m: f64,
}

/// Une extrémité de ligne de pont posée sur l'intérieur d'une autre ligne de pont (≤
/// `MERGE_UNITS`) : jonction de deux éléments du même ouvrage, pas une culée.
fn segs_touch(data: &[&Arc<StreetTile>], e: [f64; 2], own: (usize, usize)) -> bool {
    data.iter().enumerate().any(|(ti, t)| {
        t.lines.iter().enumerate().any(|(li, l)| {
            (ti, li) != own
                && t.attrs.get(l.attr as usize).is_some_and(|a| a.structure == StructureTag::Bridge)
                && l.points
                    .windows(2)
                    .any(|w| dist_units(e, project_on_segment(e, w[0], w[1]).point) <= MERGE_UNITS)
        })
    })
}

/// Intersection propre de `[p0, p1]` et `[q0, q1]` : paramètres `(t, u)` dans `[0, 1]`.
fn intersect(p0: [f64; 2], p1: [f64; 2], q0: [f64; 2], q1: [f64; 2]) -> Option<(f64, f64)> {
    let r = [p1[0] - p0[0], p1[1] - p0[1]];
    let s = [q1[0] - q0[0], q1[1] - q0[1]];
    let den = r[0] * s[1] - r[1] * s[0];
    if den.abs() < 1e-12 {
        return None;
    }
    let w = [q0[0] - p0[0], q0[1] - p0[1]];
    let t = (w[0] * s[1] - w[1] * s[0]) / den;
    let u = (w[0] * r[1] - w[1] * r[0]) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((t, u))
}

/// Angle non orienté entre deux directions, ramené dans `[0°, 90°]`.
fn angle_deg(r: [f64; 2], s: [f64; 2]) -> f64 {
    let (nr, ns) = (r[0].hypot(r[1]), s[0].hypot(s[1]));
    if nr == 0.0 || ns == 0.0 {
        return 0.0;
    }
    let c = ((r[0] * s[0] + r[1] * s[1]) / (nr * ns)).abs().min(1.0);
    c.acos().to_degrees()
}

/// Croisements de la trace (`obs`, polyligne en unités globales) avec les ponts des tuiles lues,
/// triés par distance de parcours.
pub fn find(grid: Grid, obs: &[Obs], tiles: &[(ord_domain::tiles::TileKey, TileState)]) -> Vec<Overpass> {
    // Segments de pont : (tuile, ligne, segment).
    let data: Vec<&Arc<StreetTile>> = tiles
        .iter()
        .filter_map(|(_, s)| match s {
            TileState::Data(t) => Some(t),
            _ => None,
        })
        .collect();
    let mut segs: Vec<BridgeSeg> = Vec::new();
    for (ti, t) in data.iter().enumerate() {
        for (li, l) in t.lines.iter().enumerate() {
            let bridge = t.attrs.get(l.attr as usize).is_some_and(|a| a.structure == StructureTag::Bridge);
            if !bridge {
                continue;
            }
            for k in 0..l.points.len().saturating_sub(1) {
                segs.push(GeomWithData::new(Line::new(l.points[k], l.points[k + 1]), (ti, li, k)));
            }
        }
    }
    if segs.is_empty() || obs.len() < 2 {
        return Vec::new();
    }
    // Extrémités de lignes de pont (non coupées), pour reconnaître une jonction pont – pont : une
    // ligne découpée au-dessus de la voie (deux ways de même tablier) n'y a pas de culée.
    let bridge_ends: Vec<([f64; 2], (usize, usize))> = segs
        .iter()
        .filter_map(|s| {
            let (ti, li, k) = s.data;
            let l = &data[ti].lines[li];
            let last = l.points.len() - 2;
            let mut v = Vec::new();
            if k == 0 && !l.cut[0] {
                v.push((l.points[0], (ti, li)));
            }
            if k == last && !l.cut[1] {
                v.push((l.points[last + 1], (ti, li)));
            }
            (!v.is_empty()).then_some(v)
        })
        .flatten()
        .collect();
    let joined = |e: [f64; 2], own: (usize, usize)| {
        bridge_ends.iter().any(|(q, id)| *id != own && dist_units(e, *q) <= MERGE_UNITS)
            || segs_touch(&data, e, own)
    };
    let index = RTree::bulk_load(segs);
    let mut out: Vec<Overpass> = Vec::new();
    for w in obs.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if b.d_m <= a.d_m {
            continue;
        }
        let env = AABB::from_corners(
            [a.p[0].min(b.p[0]), a.p[1].min(b.p[1])],
            [a.p[0].max(b.p[0]), a.p[1].max(b.p[1])],
        );
        for seg in index.locate_in_envelope_intersecting(env) {
            let (q0, q1) = (seg.geom().from, seg.geom().to);
            let Some((t, _)) = intersect(a.p, b.p, q0, q1) else { continue };
            let (ti, li, _) = seg.data;
            let line = &data[ti].lines[li];
            let x = [a.p[0] + t * (b.p[0] - a.p[0]), a.p[1] + t * (b.p[1] - a.p[1])];
            let mpu = grid.metres_per_unit(a.at.lat + t * (b.at.lat - a.at.lat));
            let ends = [(line.points[0], line.cut[0]), (line.points[line.points.len() - 1], line.cut[1])];
            let end_m = ends
                .iter()
                .filter(|(e, cut)| !cut && !joined(*e, (ti, li)))
                .map(|(e, _)| dist_units(x, *e) * mpu)
                .fold(f64::INFINITY, f64::min);
            let r = [b.p[0] - a.p[0], b.p[1] - a.p[1]];
            let s = [q1[0] - q0[0], q1[1] - q0[1]];
            out.push(Overpass { d_m: a.d_m + t * (b.d_m - a.d_m), angle_deg: angle_deg(r, s), end_m });
        }
    }
    out.sort_by(|x, y| x.d_m.total_cmp(&y.d_m));
    // Même croisement lu deux fois (tuiles voisines, sommet partagé par deux segments).
    out.dedup_by(|x, y| (x.d_m - y.d_m).abs() < 1.0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proper_intersection_and_angle() {
        let (t, u) = intersect([0.0, 0.0], [10.0, 0.0], [5.0, -5.0], [5.0, 5.0]).unwrap();
        assert!((t - 0.5).abs() < 1e-12 && (u - 0.5).abs() < 1e-12);
        assert!(intersect([0.0, 0.0], [10.0, 0.0], [11.0, -5.0], [11.0, 5.0]).is_none());
        assert!(intersect([0.0, 0.0], [10.0, 0.0], [0.0, 1.0], [10.0, 1.0]).is_none());
        assert!((angle_deg([1.0, 0.0], [0.0, 1.0]) - 90.0).abs() < 1e-9);
        assert!((angle_deg([1.0, 0.0], [-1.0, 0.0])).abs() < 1e-6);
        assert!((angle_deg([1.0, 0.0], [-1.0, 1.0]) - 45.0).abs() < 1e-9);
    }
}
