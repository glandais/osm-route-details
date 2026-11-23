//! Les mesures qui lisent le graphe : classes de voie et de revêtement (§3.2), degrés, recouvrement (§11.5).

use std::collections::{BTreeMap, BTreeSet};

use ord_graph::attrs::{Access, Attrs};
use ord_graph::mmap::MappedGraph;
use ord_graph::overlay::Overlay;
use ord_graph_match::agg::{coverage_threshold, union_coverage, Cov};
use ord_loops_check::TypeIndex;
use serde::Serialize;

use super::shape;
use super::walk::{Join, Trace, Walk};

/// Tout ce qu'on mesure d'une marche, réelle ou générée (§11.5).
#[derive(Clone, Debug, Default, Serialize)]
pub struct Measures {
    /// D de la requête (longueur du graphe, trous compris), km.
    pub d_km: f64,
    pub r_over_d: f64,
    pub far_pos: f64,
    /// Détour jusqu'au point lointain : `far_pos / r_over_d` (sonde du goût, §12.4).
    pub detour_far: Option<f64>,
    pub compactness: f64,
    /// La définition figée (5 % de D, ancrée au point le plus éloigné, distance au segment).
    pub vertices: u32,
    /// La même à 10 % de D : le compte proposé pour le tirage du §4.1.
    pub vertices_10: u32,
    /// La règle de `shapes.py` sur la même polyligne appariée.
    pub vertices_legacy: u32,
    pub turns_per_10km: f64,
    /// `l_poly` sur le périmètre du polygone fermé des points gardés à 10 % de D (sonde du goût, §12.4).
    pub sinuosity_10: Option<f64>,
    pub shared_share: f64,
    /// Croisements du contrôleur ; `None` pour une marche réelle non continue.
    pub crossings: Option<u32>,
    /// Parts de longueur par classe de voie : grande route, tertiaire, petite route, rue, voie cyclable ou verte, chemin.
    pub way: [f64; 6],
    /// Parts de longueur par groupe de revêtement : revêtu connu, présumé, non revêtu connu.
    pub surface: [f64; 3],
    /// Part sur un itinéraire balisé.
    pub marked: f64,
    /// Part en accès `destination`.
    pub destination: f64,
    /// Trous / D (0 pour le généré).
    pub gap_share: f64,
    /// Attracteurs OSM (lot 4, 4d, §17.11) : masques seulement, jamais une position. `None` sans fichier ni pour le MTB.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attr: Option<AttrMasks>,
    /// Lot 5, §3.1 : part de la longueur où `Attrs::surface_rd()` est `Unpaved` (la définition route-details).
    pub offroad: f64,
    /// Réel seulement : part de la longueur annoncée des `network_edges` dont `sc == "unpaved"` (`shapes.py`).
    pub offroad_sc: Option<f64>,
    /// Médiane, pondérée par la longueur, des tronçons non revêtus ; `None` sans non-revêtu.
    pub run_p50_m: Option<f64>,
    /// Part de la longueur non revêtue prise dans des tronçons d'au moins 1 km ; `None` sans non-revêtu.
    pub run1k: Option<f64>,
    /// Passages revêtu / non revêtu par 10 km.
    pub switch_10km: f64,
    /// Part de la longueur sur des arêtes que les bornes du §6.1 écarteraient, équipe non exemptée.
    pub banned: f64,
    /// La même, après l'exemption d'équipe du §6.2 (sans surcouche : égale à `banned`).
    pub banned_team: f64,
    /// Part de la longueur dont `smoothness` est renseigné.
    pub smooth_known: f64,
    /// Part de la longueur dont `mtb:scale` est renseigné.
    pub mtb_known: f64,
}

/// Un trou revêtu plus court que cela (cumulé, en m) n'interrompt pas un tronçon non revêtu (§3.1).
pub const RUN_BRIDGE_M: f64 = 50.0;
/// Seuil de la « chaîne » du plan §6.4, en m.
pub const RUN_LONG_M: f64 = 1000.0;

/// Les tronçons non revêtus d'une suite `(longueur m, non revêtu)` : suites maximales d'arêtes non revêtues, un trou
/// revêtu de moins de [`RUN_BRIDGE_M`] ne les coupe pas (sa longueur n'est pas comptée dans le tronçon). Rend
/// `(médiane pondérée par la longueur, part de la longueur en tronçons d'au moins 1 km, passages)`.
pub fn run_stats(segs: &[(f64, bool)]) -> (Option<f64>, Option<f64>, u32) {
    let mut runs: Vec<f64> = Vec::new();
    let (mut cur, mut active, mut gap) = (0.0f64, false, 0.0f64);
    for &(len, off) in segs {
        if off {
            if active {
                cur += len;
            } else {
                active = true;
                cur = len;
            }
            gap = 0.0;
        } else if active {
            gap += len;
            if gap >= RUN_BRIDGE_M {
                runs.push(cur);
                (cur, active, gap) = (0.0, false, 0.0);
            }
        }
    }
    if active {
        runs.push(cur);
    }
    let switches = segs.windows(2).filter(|w| w[0].1 != w[1].1).count() as u32;
    let total: f64 = runs.iter().sum();
    if total <= 0.0 {
        return (None, None, switches);
    }
    runs.sort_by(f64::total_cmp);
    let mut acc = 0.0;
    let mut p50 = runs[runs.len() - 1];
    for &r in &runs {
        acc += r;
        if acc * 2.0 >= total {
            p50 = r;
            break;
        }
    }
    let long: f64 = runs.iter().filter(|&&r| r >= RUN_LONG_M).sum();
    (Some(p50), Some(long / total), switches)
}

/// Ce que les bornes de roulabilité du §6.1 disent d'une arête pour un type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ban {
    Free,
    /// Écartée sauf si l'équipe l'a roulée avec un parcours du type (§6.2).
    Exemptible,
    /// Écartée même d'équipe (`smoothness=impassable`).
    Always,
}

const SMOOTH_HORRIBLE: u8 = 6;
const SMOOTH_VERY_HORRIBLE: u8 = 7;
const SMOOTH_IMPASSABLE: u8 = 8;

/// Les bornes du §6.1, écrites sur les codes du graphe : `mtb_scale()` vaut échelle + 1 (« ≥ 2 » est le code 3, « ≥ 4 »
/// le code 5), `smoothness()` la position + 1, `tracktype()` 5 pour `grade5`.
pub fn ban_of(ride: ord_loops::RideType, a: Attrs) -> Ban {
    let sm = a.smoothness();
    match ride {
        ord_loops::RideType::Road => Ban::Free,
        ord_loops::RideType::Gravel => {
            if sm == SMOOTH_IMPASSABLE {
                Ban::Always
            } else if sm == SMOOTH_HORRIBLE
                || sm == SMOOTH_VERY_HORRIBLE
                || a.mtb_scale() >= 3
                || a.tracktype() == 5
                || Some(a.surface()) == mud_code()
            {
                Ban::Exemptible
            } else {
                Ban::Free
            }
        }
        ord_loops::RideType::Mtb => {
            if sm == SMOOTH_IMPASSABLE {
                Ban::Always
            } else if a.mtb_scale() >= 5 {
                Ban::Exemptible
            } else {
                Ban::Free
            }
        }
    }
}

fn mud_code() -> Option<u8> {
    ord_graph::attrs::SURFACES.iter().position(|s| *s == "mud").map(|p| p as u8 + 1)
}

/// La surcouche d'une équipe lue pour l'exemption du §6.2.
#[derive(Clone, Copy, Debug)]
pub struct TeamRode<'a> {
    pub overlay: &'a Overlay,
    pub types: TypeIndex,
}

impl TeamRode<'_> {
    /// L'arête a-t-elle été roulée par l'équipe avec un parcours du type : GRAVEL ou MIXED (VTT : aussi MTB) ?
    pub fn exempt(&self, ride: ord_loops::RideType, edge: u32) -> bool {
        let Some(counts) = self.overlay.get(edge) else { return false };
        let ti = &self.types;
        let types: &[Option<usize>] = match ride {
            ord_loops::RideType::Road => return false,
            ord_loops::RideType::Gravel => &[ti.gravel, ti.mixed],
            ord_loops::RideType::Mtb => &[ti.mtb, ti.gravel, ti.mixed],
        };
        types.iter().flatten().any(|&t| {
            counts.get(2 * t).copied().unwrap_or(0) > 0 || counts.get(2 * t + 1).copied().unwrap_or(0) > 0
        })
    }
}

/// Les mesures de revêtement et de roulabilité d'une suite de jambes `(arête, longueur m, attributs)` ; `exempt`
/// rend vrai pour une arête roulée par l'équipe avec un parcours du type.
pub fn surface_measures(
    segs: &[(u32, f64, Attrs)],
    ride: ord_loops::RideType,
    exempt: &dyn Fn(u32) -> bool,
) -> Measures {
    let total: f64 = segs.iter().map(|s| s.1).sum();
    if total <= 0.0 {
        return Measures::default();
    }
    let flags: Vec<(f64, bool)> = segs.iter().map(|&(_, l, a)| (l, a.offroad())).collect();
    let (run_p50_m, run1k, switches) = run_stats(&flags);
    let share = |f: &dyn Fn(u32, Attrs) -> bool| -> f64 {
        segs.iter().filter(|&&(e, _, a)| f(e, a)).map(|s| s.1).sum::<f64>() / total
    };
    Measures {
        offroad: share(&|_, a| a.offroad()),
        run_p50_m,
        run1k,
        switch_10km: f64::from(switches) / (total / 10_000.0),
        banned: share(&|_, a| ban_of(ride, a) != Ban::Free),
        banned_team: share(&|e, a| match ban_of(ride, a) {
            Ban::Free => false,
            Ban::Always => true,
            Ban::Exemptible => !exempt(e),
        }),
        smooth_known: share(&|_, a| a.smoothness() != 0),
        mtb_known: share(&|_, a| a.mtb_scale() != 0),
        ..Measures::default()
    }
}

impl Measures {
    /// Ajoute les mesures du lot 5 (§3.1) d'une marche : par la longueur de chaque jambe (`Trace::leg_m`), pour le type
    /// `ride` et, si on l'a, l'équipe dont la surcouche donne l'exemption. Ne change aucune autre mesure.
    pub fn add_surface(
        &mut self,
        gf: &MappedGraph,
        walk: &Walk,
        t: &Trace,
        ride: ord_loops::RideType,
        team: Option<&TeamRode<'_>>,
    ) {
        let segs: Vec<(u32, f64, Attrs)> =
            walk.legs.iter().zip(&t.leg_m).map(|(l, &m)| (l.edge, m, gf.edge(l.edge).attrs)).collect();
        let exempt = |e: u32| team.is_some_and(|t| t.exempt(ride, e));
        let s = surface_measures(&segs, ride, &exempt);
        self.offroad = s.offroad;
        self.run_p50_m = s.run_p50_m;
        self.run1k = s.run1k;
        self.switch_10km = s.switch_10km;
        self.banned = s.banned;
        self.banned_team = s.banned_team;
        self.smooth_known = s.smooth_known;
        self.mtb_known = s.mtb_known;
    }
}

/// Les grilles d'ancrages des vues `road` et `gravel` (le VTT n'a pas d'attracteur).
#[derive(Debug, Default)]
pub struct AttrGrids {
    pub road: Option<ord_graph::attract::AnchorGrid>,
    pub gravel: Option<ord_graph::attract::AnchorGrid>,
}

impl AttrGrids {
    pub fn for_ride(&self, ride: ord_loops::RideType) -> Option<&ord_graph::attract::AnchorGrid> {
        match ride {
            ord_loops::RideType::Road => self.road.as_ref(),
            ord_loops::RideType::Gravel => self.gravel.as_ref(),
            ord_loops::RideType::Mtb => None,
        }
    }
}

/// Les catégories d'attracteurs sous le point lointain (le sommet le plus loin du départ) et sous les sommets intérieurs à
/// 10 % de D : à 300 m (`reach_m` du fichier), et à 1 km pour le point lointain. Des masques de bits, pas de coordonnées.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct AttrMasks {
    pub far: u8,
    pub far_1k: u8,
    /// Un masque par sommet à 10 % de D (départ et arrivée exclus, point lointain compris).
    pub v10: Vec<u8>,
}

/// L'indice du premier point le plus éloigné du départ (`shape::far_of` : le premier maximum strict gagne).
pub fn far_index(pts: &[shape::Pt]) -> Option<usize> {
    let o = *pts.first()?;
    let (mut best, mut at) = (0.0f64, 0usize);
    for (i, p) in pts.iter().enumerate().skip(1) {
        let d = shape::dist(*p, o);
        if d > best {
            best = d;
            at = i;
        }
    }
    (at > 0).then_some(at)
}

/// Les masques d'attracteurs d'une marche et de sa polyligne ; `None` sans grille pour ce type.
pub fn attr_masks(
    gf: &MappedGraph,
    grids: &AttrGrids,
    ride: ord_loops::RideType,
    walk: &Walk,
    t: &Trace,
) -> Option<AttrMasks> {
    let grid = grids.for_ride(ride)?;
    let e7 = |p: shape::Pt| ord_graph_match::walk::plane_to_e7(gf, walk, p);
    let far_at = far_index(&t.pts)?;
    let (lat, lon) = e7(t.pts[far_at])?;
    let kept = shape::vertex_indices(&t.pts, 0.10);
    let v10: Vec<u8> = if kept.len() >= 3 {
        kept[1..kept.len() - 1]
            .iter()
            .filter_map(|&i| e7(t.pts[i]))
            .map(|(la, lo)| grid.hits(la, lo))
            .collect()
    } else {
        Vec::new()
    };
    Some(AttrMasks { far: grid.hits(lat, lon), far_1k: grid.hits_within(lat, lon, 1000.0), v10 })
}

/// Classe de voie (§3.2) : la définition est celle du moteur, `ord_loops::taste_way`.
pub fn way_class(a: Attrs) -> usize {
    ord_loops::taste_way(a)
}

/// Groupe de revêtement : la définition est celle du moteur, `ord_loops::taste_surface`.
pub fn surface_group(a: Attrs) -> usize {
    ord_loops::taste_surface(a)
}

pub use ord_graph_match::walk::class_lengths;

/// Degré du nœud `n` : les ids d'arête distincts du CSR de sa cellule (les sens ouverts au vélo), plus `e_in` et
/// `e_out` (la marche arrive peut-être par un sens unique que le CSR ne porte pas).
pub fn degree(gf: &MappedGraph, n: u32, e_in: u32, e_out: u32) -> u8 {
    let ci = gf.node_cell(n);
    let cell = gf.cell(ci);
    let local = (n - gf.cell_info(ci).first_node) as usize;
    let mut ids: BTreeSet<u32> = cell.out(local).iter().map(|d| d.edge_id()).collect();
    ids.insert(e_in);
    ids.insert(e_out);
    ids.len().min(255) as u8
}

/// Part de longueur du graphe des arêtes prises par deux jambes ou plus (sens confondus), chaque arête comptée une
/// fois, sur `d_graph`.
pub fn shared_share(gf: &MappedGraph, walk: &Walk, d_graph: f64) -> f64 {
    // Ordonné : la somme se fait toujours dans le même ordre (sorties identiques d'un lancement à l'autre).
    let mut count: BTreeMap<u32, u32> = BTreeMap::new();
    for e in walk.edges() {
        *count.entry(e).or_default() += 1;
    }
    let shared: f64 =
        count.iter().filter(|(_, &c)| c >= 2).map(|(&e, _)| f64::from(gf.edge(e).len_dm) / 10.0).sum();
    if d_graph > 0.0 {
        shared / d_graph
    } else {
        0.0
    }
}

/// Les arêtes que la marche **prend au sens du dépôt** (`agg::push_chain_covs` puis `agg::count_track`) : sens
/// ouvert au vélo, union des portions parcourues dans un sens au moins égale à `coverage_threshold` (la moitié de
/// l'arête, 50 m au plus). C'est `E(R)` de la reconstruction (§11.8) : une arête qu'une trace réelle ne fait
/// qu'effleurer (bruit de recalage d'un bout d'appariement au carrefour, quelques mètres sur une rue transversale)
/// n'est comptée par le dépôt ni pour elle ni pour aucune autre ; comptée à sa pleine longueur, elle gonflerait le
/// dénominateur, et surtout celui de la part « qu'aucune autre trace ne prend », même dans l'échantillon.
pub fn taken_edges(gf: &MappedGraph, walk: &Walk) -> BTreeSet<u32> {
    let mut covs: Vec<Cov> = Vec::with_capacity(walk.legs.len());
    for l in &walk.legs {
        let a = gf.edge(l.edge).attrs;
        if !(if l.reverse { a.bwd_ok() } else { a.fwd_ok() }) {
            continue;
        }
        let len = gf.edge(l.edge).len_dm as f32 / 10.0;
        covs.push(Cov {
            edge: l.edge,
            dir: u8::from(l.reverse),
            lo: l.lo as f32 * len,
            hi: l.hi as f32 * len,
            elen: len,
        });
    }
    let mut out = BTreeSet::new();
    union_coverage(&mut covs, |edge, _, covered, elen| {
        if covered + 1e-6 >= coverage_threshold(elen) {
            out.insert(edge);
        }
    });
    out
}

/// Les mesures d'une marche et de sa polyligne. `crossings` : celles du contrôleur, quand on les a.
pub fn measure(gf: &MappedGraph, walk: &Walk, t: &Trace, crossings: Option<u32>) -> Measures {
    let s = shape::shape(&t.pts);
    let mut junctions: Vec<(f64, u8)> = Vec::new();
    for (j, join) in walk.joins.iter().enumerate() {
        if let Join::Node(n) = join {
            junctions.push((t.join_arc[j], degree(gf, *n, walk.legs[j].edge, walk.legs[j + 1].edge)));
        }
    }
    let turns = shape::turns(&t.pts, &t.cum, &junctions, 3, 45.0, 25.0);
    let (mut way, mut surface) = ([0.0; 6], [0.0; 3]);
    let (mut marked, mut destination) = (0.0, 0.0);
    let sum_legs: f64 = t.leg_m.iter().sum();
    for (l, &m) in walk.legs.iter().zip(&t.leg_m) {
        let a = gf.edge(l.edge).attrs;
        way[way_class(a)] += m;
        surface[surface_group(a)] += m;
        if a.routes() != 0 {
            marked += m;
        }
        if a.access() == Access::Destination {
            destination += m;
        }
    }
    let norm = |x: f64| if sum_legs > 0.0 { x / sum_legs } else { 0.0 };
    Measures {
        d_km: t.d_graph / 1000.0,
        r_over_d: s.r_over_d,
        far_pos: s.far_pos,
        detour_far: s.detour_far,
        compactness: s.compactness,
        vertices: s.vertices,
        vertices_10: s.vertices_10,
        vertices_legacy: s.vertices_legacy,
        turns_per_10km: if t.l_poly > 0.0 { f64::from(turns) / (t.l_poly / 10_000.0) } else { 0.0 },
        sinuosity_10: s.sinuosity_10,
        shared_share: shared_share(gf, walk, t.d_graph),
        crossings,
        way: way.map(norm),
        surface: surface.map(norm),
        marked: norm(marked),
        destination: norm(destination),
        gap_share: if t.d_graph > 0.0 { t.holes_m / t.d_graph } else { 0.0 },
        attr: None,
        ..Measures::default()
    }
}

#[cfg(test)]
mod tests {
    use ord_graph_match::matcher::Piece;

    use super::*;
    use crate::likeness::fixture::{edge_between, graph, id, len_m};

    fn full(gf: &MappedGraph, from: i64, to: i64) -> Piece {
        let (e, rev) = edge_between(gf, from, to);
        let l = len_m(gf, e) as f32;
        let (a, b) = if rev { (l, 0.0) } else { (0.0, l) };
        Piece { edge: e, from: a, to: b, elen: l }
    }

    fn tagged(tags: &[(&str, &str)]) -> Attrs {
        ord_graph::attrs::classify(tags).expect("way gardée").attrs
    }

    #[test]
    fn a_short_paved_gap_does_not_break_a_run_but_a_long_one_does() {
        // Trois morceaux de 600 m non revêtus séparés par 40 m revêtus : un seul tronçon de 1 800 m.
        let bridged = [(600.0, true), (40.0, false), (600.0, true), (40.0, false), (600.0, true)];
        let (p50, r1k, sw) = run_stats(&bridged);
        assert_eq!((p50, r1k, sw), (Some(1800.0), Some(1.0), 4));
        // Séparés par 60 m : trois tronçons de 600 m, aucun d'un kilomètre.
        let split = [(600.0, true), (60.0, false), (600.0, true), (60.0, false), (600.0, true)];
        let (p50, r1k, sw) = run_stats(&split);
        assert_eq!((p50, r1k, sw), (Some(600.0), Some(0.0), 4));
        // Un trou fait de deux arêtes de 30 m coupe aussi (somme cumulée de 60 m).
        let two = [(600.0, true), (30.0, false), (30.0, false), (600.0, true)];
        assert_eq!(run_stats(&two).0, Some(600.0));
        // Une borne exacte de 50 m coupe ; 49,9 m ne coupe pas.
        assert_eq!(run_stats(&[(600.0, true), (50.0, false), (600.0, true)]).0, Some(600.0));
        assert_eq!(run_stats(&[(600.0, true), (49.9, false), (600.0, true)]).0, Some(1200.0));
    }

    #[test]
    fn run_median_is_weighted_by_length_and_empty_without_offroad() {
        // Un tronçon de 3 km et deux de 200 m : la médiane pondérée est celle du long (3 000 m de 3 400).
        let segs = [(200.0, true), (100.0, false), (3000.0, true), (100.0, false), (200.0, true)];
        let (p50, r1k, _) = run_stats(&segs);
        assert_eq!(p50, Some(3000.0));
        assert!((r1k.unwrap() - 3000.0 / 3400.0).abs() < 1e-12);
        assert_eq!(run_stats(&[(500.0, false), (500.0, false)]), (None, None, 0));
        assert_eq!(run_stats(&[]), (None, None, 0));
    }

    #[test]
    fn switches_count_every_change_between_paved_and_offroad() {
        let segs = [(100.0, false), (100.0, true), (100.0, true), (100.0, false), (100.0, true)];
        assert_eq!(run_stats(&segs).2, 3);
    }

    #[test]
    fn bans_follow_the_edges_of_section_6_1() {
        use ord_loops::RideType::{Gravel, Mtb, Road};
        let track = |extra: &[(&str, &str)]| {
            let mut t = vec![("highway", "track"), ("surface", "gravel")];
            t.extend_from_slice(extra);
            tagged(&t)
        };
        let plain = track(&[]);
        assert_eq!(ban_of(Gravel, plain), Ban::Free);
        assert_eq!(ban_of(Mtb, plain), Ban::Free);
        // smoothness : bad (5) et very_bad restent permis, horrible et very_horrible écartés, impassable toujours.
        assert_eq!(ban_of(Gravel, track(&[("smoothness", "very_bad")])), Ban::Free);
        assert_eq!(ban_of(Gravel, track(&[("smoothness", "horrible")])), Ban::Exemptible);
        assert_eq!(ban_of(Gravel, track(&[("smoothness", "very_horrible")])), Ban::Exemptible);
        assert_eq!(ban_of(Gravel, track(&[("smoothness", "impassable")])), Ban::Always);
        assert_eq!(ban_of(Mtb, track(&[("smoothness", "horrible")])), Ban::Free);
        assert_eq!(ban_of(Mtb, track(&[("smoothness", "impassable")])), Ban::Always);
        // mtb:scale : 1 permis en gravel, 2 écarté ; en VTT 3 permis, 4 écarté.
        assert_eq!(ban_of(Gravel, track(&[("mtb:scale", "1")])), Ban::Free);
        assert_eq!(ban_of(Gravel, track(&[("mtb:scale", "2")])), Ban::Exemptible);
        assert_eq!(ban_of(Mtb, track(&[("mtb:scale", "3")])), Ban::Free);
        assert_eq!(ban_of(Mtb, track(&[("mtb:scale", "4")])), Ban::Exemptible);
        // tracktype : grade4 permis, grade5 écarté (gravel seulement).
        assert_eq!(ban_of(Gravel, track(&[("tracktype", "grade4")])), Ban::Free);
        assert_eq!(ban_of(Gravel, track(&[("tracktype", "grade5")])), Ban::Exemptible);
        assert_eq!(ban_of(Mtb, track(&[("tracktype", "grade5")])), Ban::Free);
        // surface=mud écarté en gravel ; sand et grass permis.
        assert_eq!(ban_of(Gravel, tagged(&[("highway", "track"), ("surface", "mud")])), Ban::Exemptible);
        assert_eq!(ban_of(Gravel, tagged(&[("highway", "track"), ("surface", "sand")])), Ban::Free);
        assert_eq!(ban_of(Gravel, tagged(&[("highway", "track"), ("surface", "grass")])), Ban::Free);
        // ROAD : aucune borne nouvelle.
        assert_eq!(ban_of(Road, track(&[("smoothness", "impassable")])), Ban::Free);
    }

    #[test]
    fn unknown_values_of_the_tags_stay_permitted() {
        use ord_loops::RideType::Gravel;
        let a = tagged(&[("highway", "track"), ("smoothness", "strange"), ("tracktype", "gradeX")]);
        assert_eq!(a.smoothness(), 15);
        assert_eq!(ban_of(Gravel, a), Ban::Free);
        assert_eq!(a.tracktype(), 7);
    }

    #[test]
    fn surface_measures_split_the_ban_with_and_without_the_team_exemption() {
        use ord_loops::RideType::Gravel;
        let road = tagged(&[("highway", "residential"), ("surface", "asphalt")]);
        let rough = tagged(&[("highway", "track"), ("surface", "gravel"), ("smoothness", "horrible")]);
        let dead = tagged(&[("highway", "track"), ("surface", "gravel"), ("smoothness", "impassable")]);
        let segs = [(1, 600.0, road), (2, 200.0, rough), (3, 100.0, dead), (4, 100.0, road)];
        let none = surface_measures(&segs, Gravel, &|_| false);
        assert!((none.banned - 0.3).abs() < 1e-12 && (none.banned_team - 0.3).abs() < 1e-12);
        // L'équipe a roulé les arêtes 2 et 3 : la 2 est exemptée, la 3 (impassable) ne l'est jamais.
        let team = surface_measures(&segs, Gravel, &|e| e == 2 || e == 3);
        assert!((team.banned - 0.3).abs() < 1e-12);
        assert!((team.banned_team - 0.1).abs() < 1e-12, "{}", team.banned_team);
        // Le non-revêtu est celui de `surface_rd` : les deux tracks de gravier, 300 m sur 1 000.
        assert!((team.offroad - 0.3).abs() < 1e-12);
        // Trois changements de revêtement : route / chemin, chemin / chemin (aucun), chemin / route.
        assert!((team.switch_10km - 2.0 / 0.1).abs() < 1e-9, "{}", team.switch_10km);
        assert!((team.smooth_known - 0.3).abs() < 1e-12);
        assert_eq!(team.mtb_known, 0.0);
        assert_eq!(team.run_p50_m, Some(300.0));
        assert_eq!(team.run1k, Some(0.0));
    }

    #[test]
    fn surface_measures_of_an_empty_suite_are_zero() {
        let m = surface_measures(&[], ord_loops::RideType::Gravel, &|_| false);
        assert_eq!((m.offroad, m.banned, m.switch_10km), (0.0, 0.0, 0.0));
        assert_eq!(m.run_p50_m, None);
    }

    #[test]
    fn degree_counts_the_open_directions_plus_the_walk_edges() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let (e1, _) = edge_between(&gf, id(1, 0), id(1, 1));
        let (e2, _) = edge_between(&gf, id(1, 1), id(1, 2));
        // Le nœud central de la grille : quatre arêtes.
        let center = (0..gf.header().nodes as u32).find(|&n| gf.osm_node_id(n) == id(1, 1) as u64).unwrap();
        assert_eq!(degree(&gf, center, e1, e2), 4);
        // Un coin : deux arêtes.
        let corner = (0..gf.header().nodes as u32).find(|&n| gf.osm_node_id(n) == id(0, 0) as u64).unwrap();
        let (c1, _) = edge_between(&gf, id(0, 0), id(0, 1));
        let (c2, _) = edge_between(&gf, id(0, 0), id(1, 0));
        assert_eq!(degree(&gf, corner, c1, c2), 2);
    }

    #[test]
    fn the_square_has_a_quarter_of_cycleway_and_four_right_angles_at_the_central_degrees() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        // (0,0) → (0,1) → (1,1) → (1,0) → (0,0) : un carré de 4 × 300 m, dont (1,0)-(1,1) est la cycleway.
        let ring = [
            full(&gf, id(0, 0), id(0, 1)),
            full(&gf, id(0, 1), id(1, 1)),
            full(&gf, id(1, 1), id(1, 0)),
            full(&gf, id(1, 0), id(0, 0)),
        ];
        let w = Walk::from_pieces(&gf, &ring);
        let t = w.trace(&gf);
        let m = measure(&gf, &w, &t, None);
        let cyc = len_m(&gf, edge_between(&gf, id(1, 0), id(1, 1)).0);
        assert!((m.way[4] - cyc / t.d_graph).abs() < 1e-9, "{:?}", m.way);
        assert!((m.way.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        // Longueurs par classe de goût : le carré fait 1 200 m, dont la cycleway ; la somme est celle des jambes.
        let cl = class_lengths(&gf, &w, &t);
        assert!((cl.iter().sum::<f64>() - t.leg_m.iter().sum::<f64>()).abs() < 1e-9);
        let cyc_class = ord_loops::taste_class(gf.edge(edge_between(&gf, id(1, 0), id(1, 1)).0).attrs);
        assert!((cl[cyc_class] - cyc).abs() < 1e-9, "{cl:?}");
        assert_eq!(ord_loops::taste_way(gf.edge(edge_between(&gf, id(1, 0), id(1, 1)).0).attrs), 4);
        assert!((m.surface.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        // Trois nœuds intérieurs de la marche ; (0,1) et (1,0) sont de degré 3, (1,1) de degré 4 : trois virages droits.
        assert!((m.turns_per_10km - 3.0 / (t.l_poly / 10_000.0)).abs() < 1e-6, "{}", m.turns_per_10km);
        assert_eq!(m.shared_share, 0.0);
        assert_eq!(m.gap_share, 0.0);
        assert!(
            (m.compactness - 4.0 * std::f64::consts::PI * 300.0 * 300.0 / (1200.0f64).powi(2)).abs() < 0.05
        );
    }

    #[test]
    fn an_edge_only_grazed_is_not_taken() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let (e1, _) = edge_between(&gf, id(0, 0), id(0, 1));
        let (e2, _) = edge_between(&gf, id(0, 1), id(1, 1));
        let (e3, _) = edge_between(&gf, id(0, 1), id(0, 2));
        let l3 = len_m(&gf, e3) as f32;
        // (0,0) → (0,1) entière, 12 m sur (0,1)-(1,1) (bruit de recalage), puis 60 m de (0,1)-(0,2) : seuil de 50 m.
        let mut p2 = full(&gf, id(0, 1), id(1, 1));
        p2.to = if p2.to > p2.from { p2.from + 12.0 } else { p2.from - 12.0 };
        let mut p3 = full(&gf, id(0, 1), id(0, 2));
        p3.to = if p3.to > p3.from { p3.from + 60.0 } else { p3.from - 60.0 };
        let w = Walk::from_pieces(&gf, &[full(&gf, id(0, 0), id(0, 1)), p2, p3]);
        assert_eq!(w.legs.len(), 3);
        let taken = taken_edges(&gf, &w);
        assert!(taken.contains(&e1) && taken.contains(&e3), "{taken:?}");
        assert!(!taken.contains(&e2), "12 m sur 300 : effleurée");
        assert!(l3 > 100.0);
    }

    #[test]
    fn an_edge_taken_twice_is_shared_once() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let out_and_back = [full(&gf, id(0, 0), id(0, 1)), full(&gf, id(0, 1), id(0, 0))];
        let w = Walk::from_pieces(&gf, &out_and_back);
        let t = w.trace(&gf);
        let m = measure(&gf, &w, &t, None);
        let l = len_m(&gf, edge_between(&gf, id(0, 0), id(0, 1)).0);
        assert!((m.shared_share - l / t.d_graph).abs() < 1e-9, "{}", m.shared_share);
        assert!((m.r_over_d - 0.5).abs() < 0.01);
        assert_eq!(m.vertices, 1);
    }
}
