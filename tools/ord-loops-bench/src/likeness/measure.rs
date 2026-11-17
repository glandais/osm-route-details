//! Les mesures qui lisent le graphe : classes de voie et de revêtement (§3.2), degrés, recouvrement (§11.5).

use std::collections::{BTreeMap, BTreeSet};

use ord_graph::attrs::{Access, Attrs};
use ord_graph::mmap::MappedGraph;
use ord_graph_match::agg::{coverage_threshold, union_coverage, Cov};
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
}

/// Classe de voie (§3.2) : la définition est celle du moteur, `ord_loops::taste_way`.
pub fn way_class(a: Attrs) -> usize {
    ord_loops::taste_way(a)
}

/// Groupe de revêtement : la définition est celle du moteur, `ord_loops::taste_surface`.
pub fn surface_group(a: Attrs) -> usize {
    ord_loops::taste_surface(a)
}

/// Longueur parcourue par classe de goût (`ord_loops::taste_class`), en mètres, trous exclus : `Σ leg_m` des jambes
/// de la marche (sonde du goût, §12.2).
pub fn class_lengths(gf: &MappedGraph, walk: &Walk, t: &Trace) -> [f64; ord_loops::TASTE_CLASSES] {
    let mut out = [0.0; ord_loops::TASTE_CLASSES];
    for (l, &m) in walk.legs.iter().zip(&t.leg_m) {
        out[ord_loops::taste_class(gf.edge(l.edge).attrs)] += m;
    }
    out
}

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
