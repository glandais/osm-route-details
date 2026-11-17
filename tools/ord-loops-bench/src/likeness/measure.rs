//! Les mesures qui lisent le graphe : classes de voie et de revêtement (§3.2), degrés, recouvrement (§11.5).

use std::collections::{BTreeSet, HashMap};

use ord_graph::attrs::{Access, Attrs, HighwayClass};
use ord_graph::mmap::MappedGraph;
use ord_loops::is_greenway;
use serde::Serialize;

use super::shape;
use super::walk::{Join, Trace, Walk};

/// Les classes de voie du §3.2 : grande route, tertiaire, petite route, rue, voie cyclable ou verte, chemin.
pub const WAY_NAMES: [&str; 6] = ["major", "tertiary", "minor", "street", "cycle", "path"];
/// Les groupes de revêtement : revêtu connu, présumé, non revêtu connu.
pub const SURFACE_NAMES: [&str; 3] = ["paved", "presumed", "unpaved"];

/// Tout ce qu'on mesure d'une marche, réelle ou générée (§11.5).
#[derive(Clone, Debug, Default, Serialize)]
pub struct Measures {
    /// D de la requête (longueur du graphe, trous compris), km.
    pub d_km: f64,
    pub r_over_d: f64,
    pub far_pos: f64,
    pub compactness: f64,
    /// La définition figée (5 % de D, ancrée au point le plus éloigné, distance au segment).
    pub vertices: u32,
    /// La même à 10 % de D : le compte proposé pour le tirage du §4.1.
    pub vertices_10: u32,
    /// La règle de `shapes.py` sur la même polyligne appariée.
    pub vertices_legacy: u32,
    pub turns_per_10km: f64,
    pub shared_share: f64,
    /// Croisements du contrôleur ; `None` pour une marche réelle non continue.
    pub crossings: Option<u32>,
    /// Parts de longueur par classe de voie ([`WAY_NAMES`]).
    pub way: [f64; 6],
    /// Parts de longueur par groupe de revêtement ([`SURFACE_NAMES`]).
    pub surface: [f64; 3],
    /// Part sur un itinéraire balisé.
    pub marked: f64,
    /// Part en accès `destination`.
    pub destination: f64,
    /// Trous / D (0 pour le généré).
    pub gap_share: f64,
}

/// Classe de voie (§3.2), dans l'ordre : voie cyclable ou verte (`Cycleway` ou `is_greenway`, la définition du
/// moteur) → 4 ; `Primary | Secondary | Trunk | Motorway` → 0 (grande route ; `Motorway`, fermée au vélo, n'y arrive
/// que par une trace réelle) ; `Tertiary` → 1 ; `Unclassified | Road` → 2 (petite route) ; `Residential |
/// LivingStreet | Service` → 3 (rue) ; tout le reste et classe absente → 5 (chemin). Les `_link` suivent leur classe.
pub fn way_class(a: Attrs) -> usize {
    use HighwayClass::*;
    if a.class() == Some(Cycleway) || is_greenway(a) {
        return 4;
    }
    match a.class() {
        Some(Primary | Secondary | Trunk | Motorway) => 0,
        Some(Tertiary) => 1,
        Some(Unclassified | Road) => 2,
        Some(Residential | LivingStreet | Service) => 3,
        _ => 5,
    }
}

/// Groupe de revêtement : `surface() == 0` → 1 (présumé) ; sinon non revêtu → 2 ; sinon 0.
pub fn surface_group(a: Attrs) -> usize {
    if a.surface() == 0 {
        1
    } else if a.is_unpaved() {
        2
    } else {
        0
    }
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
    let mut count: HashMap<u32, u32> = HashMap::new();
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
        compactness: s.compactness,
        vertices: s.vertices,
        vertices_10: s.vertices_10,
        vertices_legacy: s.vertices_legacy,
        turns_per_10km: if t.l_poly > 0.0 { f64::from(turns) / (t.l_poly / 10_000.0) } else { 0.0 },
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
