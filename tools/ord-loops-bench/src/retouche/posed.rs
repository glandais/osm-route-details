//! Plages continues d'une marche et coutures valides (note : `ord-retouche-lot0.md` §2.3 et §3).
//!
//! Une **couture** est une jonction `Node(n)` où une greffe pourrait partir ou arriver exactement. Elle est valide si :
//! 1. `n` est un nœud de la vue du type (seul un nœud de vue est un bout de chaîne) ;
//! 2. les arêtes des deux jambes sont dans cette vue (sans quoi le virage ne se jugerait pas sur des chaînes que le
//!    moteur connaît) ;
//! 3. ce n'est pas un demi-tour : deux arêtes distinctes, et un angle d'au plus 150° entre la sortie de la première et
//!    l'entrée de la seconde (le `max_turn_deg` du contrôleur) ;
//! 4. `n` est dans une plage continue à `margin_m` ou plus de chaque bout (`Touch` coupe la plage, sauf
//!    `touch_continues`). Pour `move_end`, seule la marge **avant** compte, la suite étant abandonnée.

use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};

use ord_graph::mmap::MappedGraph;
use ord_graph_match::geo::decode_polyline6;
use ord_graph_match::walk::{Join, Leg, Walk};
use rayon::prelude::*;

use super::{continuity, project};
use crate::likeness::export;
use crate::likeness::walk::Trace;

/// Bit de sens d'un lien de chaîne de vue.
const DIR_BIT: u32 = 1 << 31;
/// Demi-tour : l'angle de virage maximal du contrôleur, degrés.
pub const MAX_TURN_DEG: f64 = 150.0;

/// Ce que la vue du type sait dire d'une arête de base et d'un nœud de base.
pub trait ViewLookup: Sync {
    fn has_node(&self, base_node: u32) -> bool;
    fn has_edge(&self, base_edge: u32) -> bool;
}

/// Une vue sur disque et le masque de bits des arêtes de base qu'elle porte.
pub struct ViewInfo<'a> {
    view: &'a MappedGraph,
    bits: Vec<u64>,
}

impl<'a> ViewInfo<'a> {
    /// Parcourt toutes les chaînes de la vue (`view_chain(c)` pour `c` dans `0..header().edges`).
    pub fn build(view: &'a MappedGraph, n_base_edges: usize) -> Self {
        let atoms: Vec<AtomicU64> = (0..n_base_edges.div_ceil(64)).map(|_| AtomicU64::new(0)).collect();
        (0..view.header().edges as u32).into_par_iter().for_each(|c| {
            for l in view.view_chain(c).expect("chaînes d'une vue valide") {
                let e = (l.get() & !DIR_BIT) as usize;
                atoms[e / 64].fetch_or(1 << (e % 64), Ordering::Relaxed);
            }
        });
        Self { view, bits: atoms.into_iter().map(AtomicU64::into_inner).collect() }
    }
}

impl ViewLookup for ViewInfo<'_> {
    fn has_node(&self, base_node: u32) -> bool {
        self.view.view_node_of_base(base_node).is_some()
    }

    fn has_edge(&self, e: u32) -> bool {
        self.bits.get(e as usize / 64).is_some_and(|w| w >> (e % 64) & 1 == 1)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SeamOpts {
    pub margin_m: f64,
    /// `Touch` ne coupe pas une plage (variante, pour voir si le choix pèse).
    pub touch_continues: bool,
}

/// Une couture candidate (passe les règles 1 à 3) et ses marges continues.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seam {
    /// La jonction est entre `legs[join]` et `legs[join + 1]`.
    pub join: usize,
    pub node: u32,
    /// Abscisse sur la polyligne de la marche (`Trace::join_arc`).
    pub a_m: f64,
    /// Longueur graphe continue avant et après la couture, dans sa plage.
    pub before_m: f64,
    pub after_m: f64,
}

impl Seam {
    /// Valide pour `resize` : marge des deux côtés.
    pub fn valid(&self, margin_m: f64) -> bool {
        self.before_m >= margin_m && self.after_m >= margin_m
    }

    /// Valide pour `move_end` : seule la marge avant compte.
    pub fn valid_before(&self, margin_m: f64) -> bool {
        self.before_m >= margin_m
    }
}

/// Plages continues : suites maximales de jambes jointes par `Node` (et par `Touch` si `touch_continues`).
pub fn runs(walk: &Walk, touch_continues: bool) -> Vec<Range<usize>> {
    let n = walk.legs.len();
    let mut out = Vec::new();
    let mut start = 0;
    for (j, join) in walk.joins.iter().enumerate() {
        let joined = matches!(join, Join::Node(_)) || (touch_continues && matches!(join, Join::Touch));
        if !joined {
            out.push(start..j + 1);
            start = j + 1;
        }
    }
    if n > 0 {
        out.push(start..n);
    }
    out
}

/// Angle de virage (degrés, 0 = tout droit) entre la sortie de `x` et l'entrée de `y` à leur nœud commun, mesuré sur
/// les géométries d'arête dans le sens de la marche. `None` si une géométrie est dégénérée.
pub fn turn_deg(gf: &MappedGraph, x: &Leg, y: &Leg) -> Option<f64> {
    let out_x = bearing_of(&walking_pts(gf, x), true)?;
    let in_y = bearing_of(&walking_pts(gf, y), false)?;
    let mut d = (in_y - out_x).abs() % 360.0;
    if d > 180.0 {
        d = 360.0 - d;
    }
    Some(d)
}

/// Points `(lat, lon)` d'une arête dans le sens de la marche de la jambe.
fn walking_pts(gf: &MappedGraph, leg: &Leg) -> Vec<(f64, f64)> {
    let e7 = |p: (i32, i32)| (f64::from(p.0) * 1e-7, f64::from(p.1) * 1e-7);
    let (a, b) = gf.edge_ends(leg.edge);
    let mut pts = vec![e7(gf.node_pos(a))];
    pts.extend(gf.geometry(leg.edge).into_iter().map(e7));
    pts.push(e7(gf.node_pos(b)));
    if leg.reverse {
        pts.reverse();
    }
    pts
}

fn bearing(a: (f64, f64), b: (f64, f64)) -> Option<f64> {
    let dy = b.0 - a.0;
    let dx = (b.1 - a.1) * a.0.to_radians().cos();
    (dx != 0.0 || dy != 0.0).then(|| (dx.atan2(dy).to_degrees() + 360.0) % 360.0)
}

/// Cap (degrés, 0 = nord, sens horaire) de la dernière (`last`) ou de la première portion non nulle.
fn bearing_of(pts: &[(f64, f64)], last: bool) -> Option<f64> {
    if last {
        pts.windows(2).rev().find_map(|w| bearing(w[0], w[1]))
    } else {
        pts.windows(2).find_map(|w| bearing(w[0], w[1]))
    }
}

/// Les coutures candidates d'une marche (règles 1 à 3 et marges des plages), dans l'ordre de la marche.
pub fn candidates(
    gf: &MappedGraph,
    walk: &Walk,
    trace: &Trace,
    view: &dyn ViewLookup,
    touch_continues: bool,
) -> Vec<Seam> {
    let mut out = Vec::new();
    for run in runs(walk, touch_continues) {
        let total: f64 = trace.leg_m[run.clone()].iter().sum();
        let mut before = 0.0;
        for j in run.clone() {
            before += trace.leg_m[j];
            if j + 1 >= run.end {
                break;
            }
            let Join::Node(n) = walk.joins[j] else { continue };
            let (x, y) = (&walk.legs[j], &walk.legs[j + 1]);
            if x.edge == y.edge || !view.has_node(n) || !view.has_edge(x.edge) || !view.has_edge(y.edge) {
                continue;
            }
            // Un angle indéterminé (géométrie dégénérée) n'est pas un demi-tour avéré : la couture est refusée par
            // prudence, ce qui ne peut que sous-estimer K1.
            match turn_deg(gf, x, y) {
                Some(t) if t <= MAX_TURN_DEG => {}
                _ => continue,
            }
            out.push(Seam {
                join: j,
                node: n,
                a_m: trace.join_arc[j],
                before_m: before,
                after_m: total - before,
            });
        }
    }
    out
}

/// Une couture candidate posée sur la trace d'origine.
#[derive(Clone, Copy, Debug)]
pub struct PSeam {
    pub seam: Seam,
    /// Abscisse sur la polyligne du document (§2.4), m.
    pub s_m: f64,
}

/// Une trace posée : tout ce que les prototypes lisent (note §3). **Privé** : porte l'identifiant de hachage de la trace,
/// jamais écrit.
pub struct Posed {
    pub team: usize,
    pub letter: String,
    pub rank: u32,
    pub id_hash: u64,
    pub ty: u8,
    pub ride: ord_loops::RideType,
    pub shape: continuity::Shape,
    pub walk: Walk,
    pub line: project::DocLine,
    /// Longueur de la polyligne du document, m.
    pub l_m: f64,
    pub seams: Vec<PSeam>,
    /// Longueur graphe cumulée à la fin de chaque jambe, m.
    pub cum_end: Vec<f64>,
    pub edge_set: rustc_hash::FxHashSet<u32>,
}

impl Posed {
    /// Indices des coutures valides pour `resize` (marge des deux côtés).
    pub fn both(&self, margin_m: f64) -> Vec<usize> {
        (0..self.seams.len()).filter(|&k| self.seams[k].seam.valid(margin_m)).collect()
    }

    /// Indices des coutures valides pour `move_end` (marge avant seulement).
    pub fn before(&self, margin_m: f64) -> Vec<usize> {
        (0..self.seams.len()).filter(|&k| self.seams[k].seam.valid_before(margin_m)).collect()
    }

    /// Début de la jambe `j`, abscisse de graphe cumulée.
    pub fn leg_start(&self, j: usize) -> f64 {
        if j == 0 {
            0.0
        } else {
            self.cum_end[j - 1]
        }
    }
}

/// Longueur graphe d'une jambe, m : `(hi − lo) × len_dm / 10`.
pub fn leg_len_m(gf: &MappedGraph, l: &Leg) -> f64 {
    (l.hi - l.lo).abs() * f64::from(gf.edge(l.edge).len_dm) / 10.0
}

/// Pose une trace de l'export : marche, polyligne du document, coutures (règles 1 à 3, marges des plages) et leurs
/// abscisses de trace. `None` pour une trace illisible, vide ou de type au-delà de MTB.
pub fn pose(ctx: &continuity::Ctx<'_>, idx: usize, rank: u32, letter: &str) -> Option<Posed> {
    let t = &ctx.ing.tracks[idx];
    if !t.readable || t.ty > 2 {
        return None;
    }
    let m = export::track_match(ctx.jobs, t);
    if m.pieces.is_empty() {
        return None;
    }
    let walk = Walk::from_pieces(ctx.gf, &m.pieces);
    if walk.legs.is_empty() {
        return None;
    }
    let trace = walk.trace(ctx.gf);
    let gap = {
        let (a, b) = (trace.pts[0], trace.pts[trace.pts.len() - 1]);
        (a[0] - b[0]).hypot(a[1] - b[1])
    };
    let shape = if gap < ctx.retain.loop_gap * trace.l_poly {
        continuity::Shape::Loop
    } else {
        continuity::Shape::OneWay
    };
    let line = project::DocLine::from_parts(
        t.edges.iter().map(|e| decode_polyline6(&ctx.ing.keys[e.key as usize].0).unwrap_or_default()),
    );
    let l_m = line.len_m();
    let view = ctx.views[usize::from(t.ty == 1 || t.ty == 2)];
    let cands = candidates(ctx.gf, &walk, &trace, view, ctx.opts.touch_continues);
    let mut prev_join = 0usize;
    let points: Vec<project::SeamPoint> = cands
        .iter()
        .map(|c| {
            let p = ctx.gf.node_pos(c.node);
            let chords: f64 = walk.joins[prev_join..c.join]
                .iter()
                .filter_map(|j| if let Join::Hole(d) = j { Some(*d) } else { None })
                .sum();
            prev_join = c.join;
            project::SeamPoint {
                latlon: (f64::from(p.0) * 1e-7, f64::from(p.1) * 1e-7),
                a_m: c.a_m,
                slack_m: 3.0 * chords,
            }
        })
        .collect();
    let proj = project::project(&line, &points, ctx.proj_tol_m);
    let seams = cands.iter().zip(&proj.s).map(|(c, &s)| PSeam { seam: *c, s_m: s }).collect();
    let mut acc = 0.0;
    let cum_end = walk
        .legs
        .iter()
        .map(|l| {
            acc += leg_len_m(ctx.gf, l);
            acc
        })
        .collect();
    let edge_set = walk.legs.iter().map(|l| l.edge).collect();
    Some(Posed {
        team: t.team,
        letter: letter.to_string(),
        rank,
        id_hash: t.id_hash,
        ty: t.ty,
        ride: ord_graph_match::profile::collect::ride_of(t.ty),
        shape,
        walk,
        line,
        l_m,
        seams,
        cum_end,
        edge_set,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::likeness::fixture::{edge_between, graph, id};

    struct Fake {
        nodes_off: Vec<u32>,
        edges_off: Vec<u32>,
    }

    impl ViewLookup for Fake {
        fn has_node(&self, n: u32) -> bool {
            !self.nodes_off.contains(&n)
        }

        fn has_edge(&self, e: u32) -> bool {
            !self.edges_off.contains(&e)
        }
    }

    fn all() -> Fake {
        Fake { nodes_off: vec![], edges_off: vec![] }
    }

    fn leg(gf: &MappedGraph, from: i64, to: i64) -> Leg {
        let (e, reverse) = edge_between(gf, from, to);
        Leg { edge: e, reverse, lo: 0.0, hi: 1.0 }
    }

    fn walk_of(gf: &MappedGraph, path: &[i64]) -> Walk {
        let legs: Vec<Leg> = path.windows(2).map(|w| leg(gf, w[0], w[1])).collect();
        let joins = legs
            .windows(2)
            .map(|w| {
                let (a, b) = gf.edge_ends(w[0].edge);
                Join::Node(if w[0].reverse { a } else { b })
            })
            .collect();
        Walk { legs, joins }
    }

    #[test]
    fn straight_walk_has_valid_seams_with_their_margins() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let w = walk_of(&gf, &[id(0, 0), id(0, 1), id(0, 2), id(1, 2), id(2, 2)]);
        let t = w.trace(&gf);
        let seams = candidates(&gf, &w, &t, &all(), false);
        assert_eq!(seams.len(), 3);
        // Arêtes de 300 m : la première couture a 300 m derrière et 900 devant.
        assert!(
            (seams[0].before_m - 300.0).abs() < 1.0 && (seams[0].after_m - 900.0).abs() < 1.0,
            "{seams:?}"
        );
        assert!(seams.iter().all(|s| s.valid(200.0)));
        // Une marge de 400 m écarte la première (300 m derrière) et la dernière (300 m devant).
        let kept: Vec<usize> = seams.iter().filter(|s| s.valid(400.0)).map(|s| s.join).collect();
        assert_eq!(kept, vec![1]);
        // Pour move_end, seule la marge avant compte : la dernière couture reste valide à 400 m.
        assert_eq!(seams.iter().filter(|s| s.valid_before(400.0)).count(), 2);
    }

    #[test]
    fn short_margin_node_outside_view_and_edge_outside_view_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let mut w = walk_of(&gf, &[id(0, 0), id(0, 1), id(0, 2), id(1, 2)]);
        // Première jambe réduite à 100 m : la marge avant de la première couture est courte.
        w.legs[0].lo = 2.0 / 3.0;
        let t = w.trace(&gf);
        let s = candidates(&gf, &w, &t, &all(), false);
        assert!(!s[0].valid(200.0) && s[1].valid(100.0));
        // Nœud hors vue.
        let n = match w.joins[0] {
            Join::Node(n) => n,
            _ => unreachable!(),
        };
        let off = Fake { nodes_off: vec![n], edges_off: vec![] };
        assert_eq!(candidates(&gf, &w, &t, &off, false).len(), 1);
        // Arête hors vue : les deux coutures qui la touchent tombent.
        let off = Fake { nodes_off: vec![], edges_off: vec![w.legs[1].edge] };
        assert!(candidates(&gf, &w, &t, &off, false).is_empty());
    }

    #[test]
    fn a_u_turn_is_not_a_seam_but_a_right_angle_is() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        // Aller-retour sur la même arête : même arête, refusé.
        let back = walk_of(&gf, &[id(0, 0), id(0, 1), id(0, 0)]);
        let t = back.trace(&gf);
        assert!(candidates(&gf, &back, &t, &all(), false).is_empty());
        // Demi-tour sur deux arêtes distinctes (l'angle dépasse 150°) : impossible sur la grille ; on vérifie
        // l'angle : un virage à 90° passe, 0° passe.
        let turn = walk_of(&gf, &[id(0, 0), id(0, 1), id(1, 1)]);
        let (x, y) = (&turn.legs[0], &turn.legs[1]);
        let a = turn_deg(&gf, x, y).unwrap();
        assert!((a - 90.0).abs() < 1.0, "{a}");
        let straight = walk_of(&gf, &[id(0, 0), id(0, 1), id(0, 2)]);
        assert!(turn_deg(&gf, &straight.legs[0], &straight.legs[1]).unwrap() < 1.0);
        // Un demi-tour à deux arêtes distinctes : on revient par l'autre arête voisine et l'angle est de 180°.
        let mut fake = straight.clone();
        fake.legs[1] = Leg { reverse: !straight.legs[0].reverse, ..straight.legs[0] };
        assert!(turn_deg(&gf, &fake.legs[0], &fake.legs[1]).unwrap() > 179.0);
    }

    #[test]
    fn touch_cuts_a_run_unless_asked_otherwise_and_a_hole_always_does() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let mut w = walk_of(&gf, &[id(0, 0), id(0, 1), id(0, 2), id(1, 2), id(2, 2)]);
        w.joins[1] = Join::Touch;
        let t = w.trace(&gf);
        assert_eq!(runs(&w, false), vec![0..2, 2..4]);
        assert_eq!(runs(&w, true), vec![0..4]);
        // Coupée : les coutures 0 et 2 sont à 300 m d'un bout (valides à 200), celle du milieu n'existe plus.
        let s = candidates(&gf, &w, &t, &all(), false);
        assert_eq!(s.iter().map(|s| s.join).collect::<Vec<_>>(), vec![0, 2]);
        assert!((s[0].after_m - 300.0).abs() < 1.0);
        // Continue : même les deux coutures voisines du Touch gardent leurs marges de la plage entière.
        let s = candidates(&gf, &w, &t, &all(), true);
        assert_eq!(s.iter().map(|s| s.join).collect::<Vec<_>>(), vec![0, 2]);
        assert!((s[0].after_m - 900.0).abs() < 1.0);
        w.joins[1] = Join::Hole(500.0);
        assert_eq!(runs(&w, true), vec![0..2, 2..4]);
    }
}
