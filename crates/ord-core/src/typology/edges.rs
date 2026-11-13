//! Arêtes du graphe des rues parcourues par la trace appariée (api 1.7, `edges[]`).
//!
//! Une arête est une suite maximale de segments du graphe entre deux jonctions : nœud de degré
//! différent de 2 (carrefour, impasse) parmi les rues ouvertes au vélo, ou bout ouvert (la rue
//! continue hors du couloir, ou au bord d'une tuile non lue : bout inconnu). Les coupures de tuile
//! recollées, les changements d'attributs et les coupures de lignes fusionnées sont de degré 2 et
//! ne coupent pas d'arête.
//!
//! Le chemin apparié (morceaux de segments du HMM) est parcouru dans l'ordre de la trace ; un saut,
//! une observation non appariée ou hors graphe, un changement de chaîne interrompent la suite. Un
//! demi-tour (au milieu d'un segment, ou à un nœud de degré 2) ferme l'arête en cours, partielle,
//! et en ouvre une dans l'autre sens. Les distances le long de la trace suivent le même prorata que
//! les tronçons typés (`runs`), donc les mêmes abscisses que `segments[]`.

use ord_domain::street::OnewayTag;
use ord_domain::LatLon;

use super::graph::{ClassId, NodeId, SegId, StreetGraph};
use super::matcher::{MatchedPath, ObsState, Step};

/// Sens unique de la partie parcourue, rapporté au sens de parcours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeOneway {
    /// Double sens partout.
    Both,
    /// Un sens unique au moins, toujours parcouru dans le sens autorisé.
    Forward,
    /// Parcourue à contresens d'un sens unique (contresens cyclable probable : les tuiles ne
    /// portent pas `oneway:bicycle`).
    Against,
}

/// Bout connu d'une arête : coordonnée du nœud sur la grille des tuiles (unités entières, stable
/// tant que le nœud OSM ne bouge pas), position, degré.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeNode {
    pub key: [i64; 2],
    pub at: LatLon,
    pub degree: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatchedEdge {
    /// Distance le long de la trace.
    pub from_m: f64,
    pub to_m: f64,
    /// Bouts de l'arête complète dans le sens de parcours ; `None` : hors du graphe lu.
    pub from_node: Option<EdgeNode>,
    pub to_node: Option<EdgeNode>,
    /// La trace ne parcourt pas l'arête d'un bout à l'autre.
    pub partial: bool,
    /// Longueur réseau de l'arête complète ; `None` si un bout est inconnu.
    pub length_m: Option<f64>,
    /// Longueur réseau parcourue.
    pub traversed_m: f64,
    /// Position d'entrée et de sortie le long de l'arête complète, depuis `from_node` ; `None` si
    /// ce bout est inconnu.
    pub from_offset_m: Option<f64>,
    pub to_offset_m: Option<f64>,
    pub oneway: EdgeOneway,
    /// Géométrie parcourue, recalée sur la rue, dans le sens de parcours.
    pub geometry: Vec<LatLon>,
    /// Classes des segments parcourus, distinctes, dans l'ordre.
    pub classes: Vec<ClassId>,
}

/// Morceau de segment parcouru, avec ses distances le long de la trace.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Move {
    seg: SegId,
    t0: f64,
    t1: f64,
    m0: f64,
    m1: f64,
}

const EPS_T: f64 = 1e-9;

impl Move {
    /// Sens du segment, de `a` vers `b`.
    fn ab(&self) -> bool {
        self.t1 > self.t0
    }
}

/// Aller-retour plus court sur un même segment : bruit de l'appariement (projection qui déborde de
/// quelques centimètres sur une rue voisine au carrefour), pas un usage de la rue ; aussi le seuil
/// d'une arête partielle en bout de suite.
pub const SPUR_M: f64 = 5.0;

/// Ajoute `m` à la suite : fusionné avec le morceau précédent s'il le prolonge ; un aller-retour
/// dont une branche fait moins de `SPUR_M` est annulé (l'excursion disparaît, ses distances le
/// long de la trace restent au morceau précédent). `carry` : début de plage d'un éperon annulé en
/// tête de suite.
fn push_move(g: &StreetGraph, walk: &mut Vec<Move>, mut m: Move, carry: &mut Option<f64>) {
    if let Some(c) = carry.take() {
        m.m0 = c;
    }
    let len = |x: &Move| (x.t1 - x.t0).abs() * g.segs[x.seg as usize].len_m;
    loop {
        let Some(last) = walk.last().copied() else {
            walk.push(m);
            return;
        };
        if last.seg != m.seg || (last.t1 - m.t0).abs() >= EPS_T {
            walk.push(m);
            return;
        }
        if last.ab() == m.ab() {
            let l = walk.last_mut().expect("non vide");
            l.t1 = m.t1;
            l.m1 = m.m1;
            return;
        }
        if len(&last).min(len(&m)) >= SPUR_M {
            walk.push(m);
            return;
        }
        walk.pop();
        let net = Move { seg: m.seg, t0: last.t0, t1: m.t1, m0: last.m0, m1: m.m1 };
        if len(&net) < 1e-6 {
            match walk.last_mut() {
                Some(p) => p.m1 = m.m1,
                None => *carry = Some(last.m0),
            }
            return;
        }
        m = net;
    }
}

/// Suites continues de morceaux parcourus, distances au prorata des longueurs réseau (`runs`).
fn walks(g: &StreetGraph, path: &MatchedPath) -> Vec<Vec<Move>> {
    let mut out: Vec<Vec<Move>> = Vec::new();
    let mut cur: Vec<Move> = Vec::new();
    let mut carry: Option<f64> = None;
    let obs = &path.obs;
    for t in 0..obs.len().saturating_sub(1) {
        let (d0, d1) = (obs[t].d_m, obs[t + 1].d_m);
        match (&path.state[t], &path.state[t + 1], &path.steps[t]) {
            (ObsState::Matched { .. }, ObsState::Matched { .. }, Some(Step::Route(pieces))) => {
                let w: Vec<f64> =
                    pieces.iter().map(|p| (p.t1 - p.t0).abs() * g.segs[p.seg as usize].len_m).collect();
                let total: f64 = w.iter().sum();
                if total <= 1e-9 {
                    // Sur place : la plage reste au morceau précédent.
                    if let Some(last) = cur.last_mut() {
                        last.m1 = d1;
                    } else {
                        carry.get_or_insert(d0);
                    }
                    continue;
                }
                let mut at = d0;
                for (k, (p, wk)) in pieces.iter().zip(&w).enumerate() {
                    let to = if k + 1 == pieces.len() { d1 } else { at + (d1 - d0) * wk / total };
                    push_move(
                        g,
                        &mut cur,
                        Move { seg: p.seg, t0: p.t0, t1: p.t1, m0: at, m1: to },
                        &mut carry,
                    );
                    at = to;
                }
            }
            _ => {
                carry = None;
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn node_at(g: &StreetGraph, seg: SegId, t: f64) -> Option<NodeId> {
    let s = &g.segs[seg as usize];
    if t.abs() < EPS_T {
        Some(s.a)
    } else if (t - 1.0).abs() < EPS_T {
        Some(s.b)
    } else {
        None
    }
}

fn other_end(g: &StreetGraph, seg: SegId, n: NodeId) -> NodeId {
    let s = &g.segs[seg as usize];
    if s.a == n {
        s.b
    } else {
        s.a
    }
}

fn is_boundary(g: &StreetGraph, n: NodeId) -> bool {
    g.open[n as usize] || g.junction_degree(n) != 2
}

/// Depuis `at` (atteint depuis `from`), à travers les nœuds de degré 2 jusqu'à une jonction :
/// `(bout, longueur, segments franchis)` ; bout `None` sur un anneau sans jonction ou un bout ouvert.
fn chain(g: &StreetGraph, mut at: NodeId, mut from: NodeId) -> (Option<NodeId>, f64, usize) {
    let (mut len, mut steps) = (0.0, 0usize);
    let start = at;
    loop {
        if is_boundary(g, at) {
            return ((!g.open[at as usize]).then_some(at), len, steps);
        }
        // Degré 2 : l'unique autre voisin ; parmi des segments parallèles, le plus petit.
        let next =
            g.neighbours(at).iter().filter(|&&(_, o)| o != from && o != at).min_by_key(|&&(s, _)| s).copied();
        let Some((seg, o)) = next else {
            return (None, len, steps);
        };
        len += g.segs[seg as usize].len_m;
        steps += 1;
        from = at;
        at = o;
        if at == start || steps > g.segs.len() {
            return (None, len, steps);
        }
    }
}

fn pos(g: &StreetGraph, seg: SegId, t: f64) -> [f64; 2] {
    let (a, b) = g.seg_points(seg);
    [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]
}

fn edge_node(g: &StreetGraph, n: NodeId) -> EdgeNode {
    let p = g.nodes[n as usize];
    EdgeNode {
        key: [p[0].round() as i64, p[1].round() as i64],
        at: g.grid.to_latlon(p),
        degree: g.junction_degree(n) as u32,
    }
}

fn close(g: &StreetGraph, acc: &[Move]) -> MatchedEdge {
    let (first, last) = (acc[0], acc[acc.len() - 1]);
    let fs = &g.segs[first.seg as usize];
    let ls = &g.segs[last.seg as usize];
    // Entrée : nœud de départ du premier segment dans le sens de parcours.
    let (start_node, start_other) = if first.ab() { (fs.a, fs.b) } else { (fs.b, fs.a) };
    let entry_off = if first.ab() { first.t0 } else { 1.0 - first.t0 } * fs.len_m;
    let (end_node, end_other) = if last.ab() { (ls.b, ls.a) } else { (ls.a, ls.b) };
    let exit_rest = if last.ab() { 1.0 - last.t1 } else { last.t1 } * ls.len_m;
    let (from, back_len, back_steps) = chain(g, start_node, start_other);
    let (to, fwd_len, fwd_steps) = chain(g, end_node, end_other);
    let traversed: f64 = acc.iter().map(|m| (m.t1 - m.t0).abs() * g.segs[m.seg as usize].len_m).sum();
    let from_offset = from.map(|_| back_len + entry_off);
    let to_offset = from_offset.map(|o| o + traversed);
    let length = match (to_offset, to) {
        (Some(o), Some(_)) => Some(o + exit_rest + fwd_len),
        _ => None,
    };
    let partial = from.is_none()
        || to.is_none()
        || back_steps > 0
        || fwd_steps > 0
        || entry_off > 1e-6
        || exit_rest > 1e-6;

    let mut restricted = false;
    let mut against = false;
    let mut classes: Vec<ClassId> = Vec::new();
    let mut pts: Vec<[f64; 2]> = vec![pos(g, first.seg, first.t0)];
    for m in acc {
        let s = &g.segs[m.seg as usize];
        let along_line = m.ab() == s.fwd;
        match g.classes[s.class as usize].attrs.oneway {
            OnewayTag::No => {}
            OnewayTag::Forward => {
                restricted = true;
                against |= !along_line;
            }
            OnewayTag::Backward => {
                restricted = true;
                against |= along_line;
            }
        }
        if !classes.contains(&s.class) {
            classes.push(s.class);
        }
        let p = pos(g, m.seg, m.t1);
        if pts.last() != Some(&p) {
            pts.push(p);
        }
    }
    MatchedEdge {
        from_m: first.m0,
        to_m: last.m1,
        from_node: from.map(|n| edge_node(g, n)),
        to_node: to.map(|n| edge_node(g, n)),
        partial,
        length_m: length,
        traversed_m: traversed,
        from_offset_m: from_offset,
        to_offset_m: to_offset,
        oneway: if against {
            EdgeOneway::Against
        } else if restricted {
            EdgeOneway::Forward
        } else {
            EdgeOneway::Both
        },
        geometry: pts.into_iter().map(|p| g.grid.to_latlon(p)).collect(),
        classes,
    }
}

/// Arêtes parcourues, dans l'ordre de la trace.
pub fn extract(g: &StreetGraph, path: &MatchedPath) -> Vec<MatchedEdge> {
    let mut out = Vec::new();
    for walk in walks(g, path) {
        let start = out.len();
        let mut acc: Vec<Move> = Vec::new();
        for m in walk {
            if let Some(prev) = acc.last() {
                let cut = match (node_at(g, prev.seg, prev.t1), node_at(g, m.seg, m.t0)) {
                    (Some(n1), Some(n2)) if n1 == n2 => {
                        // Demi-tour au nœud : on repart vers le nœud d'où l'on venait.
                        let came_from = other_end(g, prev.seg, n1);
                        let going_to = other_end(g, m.seg, n1);
                        came_from == going_to || is_boundary(g, n1)
                    }
                    // Même segment, au même point intérieur : demi-tour (même sens : fusionné).
                    (None, None) => {
                        !(m.seg == prev.seg && (prev.t1 - m.t0).abs() < EPS_T && m.ab() == prev.ab())
                    }
                    _ => true,
                };
                if cut {
                    out.push(close(g, &acc));
                    acc.clear();
                }
            }
            acc.push(m);
        }
        if !acc.is_empty() {
            out.push(close(g, &acc));
        }
        // Bout d'arête de moins de `SPUR_M` en tête ou en queue de suite (trace qui commence ou
        // finit juste avant un carrefour) : rattaché à l'arête voisine.
        if out.len() - start >= 2 && out[start].partial && out[start].traversed_m < SPUR_M {
            let first = out.remove(start);
            out[start].from_m = first.from_m;
        }
        let n = out.len();
        if n - start >= 2 && out[n - 1].partial && out[n - 1].traversed_m < SPUR_M {
            let last = out.pop().expect("non vide");
            out[n - 2].to_m = last.to_m;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classification::tests::caps;
    use crate::typology::tests::{at, with_surface, Net, GRID};
    use crate::typology::{Corridor, TileState};
    use ord_domain::street::PavedGroup;

    /// Route est-ouest de 0 à 200 m, rue nord à 100 m.
    fn tee() -> StreetGraph {
        let paved = || with_surface("residential", PavedGroup::Paved);
        let net = Net::new()
            .add(paved(), &[(0.0, 0.0), (100.0, 0.0), (200.0, 0.0)])
            .add(paved(), &[(100.0, 0.0), (100.0, 80.0)]);
        let route = [at(0.0, 0.0), at(200.0, 0.0)];
        let tiles: Vec<_> = net
            .tiles(&route, None)
            .into_iter()
            .filter_map(|(_, s)| match s {
                TileState::Data(t) => Some(t),
                _ => None,
            })
            .collect();
        let pts: Vec<([f64; 2], f64)> = route.iter().map(|p| (GRID.to_units(*p), p.lat)).collect();
        StreetGraph::build(GRID, &tiles, &Corridor::new(GRID, &pts, 150.0), &caps())
    }

    /// Segment de `g` entre deux nœuds aux positions données (mètres est, nord).
    fn seg(g: &StreetGraph, a: (f64, f64), b: (f64, f64)) -> (SegId, bool) {
        let (pa, pb) = (GRID.to_units(at(a.0, a.1)), GRID.to_units(at(b.0, b.1)));
        let near = |n: NodeId, p: [f64; 2]| ord_domain::geo::dist_units(g.nodes[n as usize], p) < 2.0;
        g.segs
            .iter()
            .enumerate()
            .find_map(|(i, s)| {
                if near(s.a, pa) && near(s.b, pb) {
                    Some((i as SegId, true))
                } else if near(s.a, pb) && near(s.b, pa) {
                    Some((i as SegId, false))
                } else {
                    None
                }
            })
            .expect("segment")
    }

    fn mv(s: (SegId, bool), from: f64, to: f64, m0: f64, m1: f64) -> Move {
        let t = |x: f64| if s.1 { x } else { 1.0 - x };
        Move { seg: s.0, t0: t(from), t1: t(to), m0, m1 }
    }

    #[test]
    fn a_few_centimetres_into_a_side_street_is_no_edge_use() {
        let g = tee();
        let west = seg(&g, (0.0, 0.0), (100.0, 0.0));
        let north = seg(&g, (100.0, 0.0), (100.0, 80.0));
        let east = seg(&g, (100.0, 0.0), (200.0, 0.0));
        let mut walk = Vec::new();
        let mut carry = None;
        // Débord de 4 cm dans la rue nord au carrefour, puis retour.
        for m in [
            mv(west, 0.0, 1.0, 0.0, 100.0),
            mv(north, 0.0, 0.0005, 100.0, 100.02),
            mv(north, 0.0005, 0.0, 100.02, 100.04),
            mv(east, 0.0, 1.0, 100.04, 200.0),
        ] {
            push_move(&g, &mut walk, m, &mut carry);
        }
        assert_eq!(walk.len(), 2, "{walk:?}");
        assert_eq!((walk[0].m1, walk[1].m0), (100.04, 100.04), "plage gardée, contiguë");
    }

    #[test]
    fn a_real_u_turn_is_kept_and_a_short_backstep_is_absorbed() {
        let g = tee();
        let west = seg(&g, (0.0, 0.0), (100.0, 0.0));
        let mut walk = Vec::new();
        let mut carry = None;
        for m in [
            mv(west, 0.0, 0.5, 0.0, 50.0),
            // Recul de 1 m (bruit), puis on repart : un seul morceau.
            mv(west, 0.5, 0.49, 50.0, 51.0),
            mv(west, 0.49, 0.8, 51.0, 82.0),
            // Vrai demi-tour : 30 m de retour.
            mv(west, 0.8, 0.5, 82.0, 112.0),
        ] {
            push_move(&g, &mut walk, m, &mut carry);
        }
        assert_eq!(walk.len(), 2, "{walk:?}");
        assert!(walk[0].ab() != walk[1].ab());
        assert_eq!((walk[0].m0, walk[0].m1, walk[1].m1), (0.0, 82.0, 112.0));
    }
}
