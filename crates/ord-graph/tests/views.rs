//! Vues par type de parcours : filtre, contraction des chaînes, sens uniques, boucles, arêtes parallèles,
//! élagage des impasses, et équivalence des distances avec le graphe de base filtré (lot 0 bis).

use std::path::Path;

use ord_graph::build::{build_graph, BuildOptions, GraphData, MemSource};
use ord_graph::format::{write_graph, FormatError, GraphFile, DIR_BIT};
use ord_graph::search::{SearchConfig, Searcher, StateKind};
use ord_graph::view::{
    alive_edges, build_view, check_view_structure, filtered_reference_distances, BaseEdges, Incidence,
    Snapper, ViewKind, ViewSpec, NO_WEIGHT,
};

const LAT0: f64 = 47.2;
const LON0: f64 = -1.56;

fn built(src: &mut MemSource, zoom: u8) -> GraphData {
    build_graph(src, &BuildOptions { zoom }, "test", &mut |_| {}).expect("construction")
}

fn roundtrip(mut data: GraphData, dir: &Path, name: &str) -> GraphFile<Vec<u8>> {
    let path = dir.join(name);
    write_graph(&path, &mut data, "test", 1_700_000_000).expect("écriture");
    let g = GraphFile::open(std::fs::read(path).unwrap()).expect("ouverture");
    g.verify().expect("verify");
    g
}

fn base(src: &mut MemSource, zoom: u8) -> GraphFile<Vec<u8>> {
    let dir = tempfile::tempdir().unwrap();
    roundtrip(built(src, zoom), dir.path(), "base.ordg")
}

fn view_of(g: &GraphFile<Vec<u8>>, spec: &str) -> GraphFile<Vec<u8>> {
    let spec = ViewSpec::parse(spec).expect("nom de vue");
    let data = build_view(g, &spec).expect("vue");
    let dir = tempfile::tempdir().unwrap();
    let v = roundtrip(data, dir.path(), "view.ordg");
    check_view_structure(g, &v).unwrap_or_else(|e| panic!("structure de la vue {} : {e}", spec.name()));
    v
}

fn tags<'a>(highway: &'a str, extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    let mut t = vec![("highway", highway)];
    t.extend_from_slice(extra);
    t
}

/// Nœud `id` à `(dlat, dlon)` millièmes de degré du point de référence.
fn at(s: &mut MemSource, id: i64, dlat: i64, dlon: i64) {
    s.node(id, LAT0 + 0.001 * dlat as f64, LON0 + 0.001 * dlon as f64);
}

fn osm_ids(v: &GraphFile<Vec<u8>>, e: u32) -> (u64, u64) {
    let (_, a, b) = v.identity(e);
    (a, b)
}

fn chain_edges(v: &GraphFile<Vec<u8>>, e: u32) -> Vec<(u32, bool)> {
    v.view_chain(e).unwrap().iter().map(|l| (l.get() & !DIR_BIT, l.get() & DIR_BIT != 0)).collect()
}

// --- Filtre et contraction -----------------------------------------------------------------------------------

/// Rue 1-2-3 et 3-4-5 (deux ways), un trottoir 3-6 : à nœud 3 de degré 3 en base.
fn street_with_footway() -> MemSource {
    let mut s = MemSource::default();
    for (i, id) in [1, 2, 3, 4, 5].into_iter().enumerate() {
        at(&mut s, id, i as i64, 0);
    }
    at(&mut s, 6, 2, 1);
    s.way(10, &[1, 2, 3], &tags("residential", &[])).way(11, &[3, 4, 5], &tags("residential", &[]));
    s.way(12, &[3, 6], &tags("footway", &[]));
    s
}

#[test]
fn road_view_drops_the_footway_and_merges_the_street_into_one_edge() {
    let g = base(&mut street_with_footway(), 11);
    assert_eq!((g.header().nodes, g.header().edges), (4, 3));
    let road = view_of(&g, "road");
    assert_eq!((road.header().nodes, road.header().edges), (2, 1));
    assert_eq!(osm_ids(&road, 0), (1, 5));
    // Deux arêtes de base, dans l'ordre, sans demi-tour ; longueur = somme ; géométrie = polyligne complète.
    let chain = chain_edges(&road, 0);
    assert_eq!(chain.len(), 2);
    assert!(chain.iter().all(|&(_, rev)| !rev));
    let sum: u32 = chain.iter().map(|&(e, _)| g.edge(e).len_dm).sum();
    assert_eq!(road.edge(0).len_dm, sum);
    let geom = road.geometry(0);
    assert_eq!(geom.len(), 3, "points 2, 3 et 4 : les nœuds fondus restent dans la géométrie");
    assert_eq!(
        geom[1],
        g.node_pos(chain_edges(&road, 0).first().map(|_| g.edge_ends(chain[0].0).1).unwrap())
    );
    let meta = road.view_meta().unwrap();
    assert_eq!((meta.kind, meta.filtered_edges, meta.merged_nodes), (ViewKind::Road, 2, 1));
    // Vue `all` : rien n'est écarté, le nœud 3 (degré 3) reste.
    let all = view_of(&g, "all");
    assert_eq!((all.header().nodes, all.header().edges), (4, 3));
}

#[test]
fn all_view_merges_only_identical_attribute_seams() {
    // Mêmes attributs : fondus. Surface différente : le nœud reste.
    let mut s = MemSource::default();
    for i in 0..5 {
        at(&mut s, 1 + i, i, 0);
    }
    s.way(10, &[1, 2, 3], &tags("residential", &[])).way(11, &[3, 4, 5], &tags("residential", &[]));
    let g = base(&mut s, 11);
    assert_eq!(g.header().edges, 2);
    assert_eq!(view_of(&g, "all").header().edges, 1);
    let mut s2 = MemSource::default();
    for i in 0..5 {
        at(&mut s2, 1 + i, i, 0);
    }
    s2.way(10, &[1, 2, 3], &tags("residential", &[])).way(
        11,
        &[3, 4, 5],
        &tags("residential", &[("surface", "gravel")]),
    );
    let g2 = base(&mut s2, 11);
    assert_eq!(view_of(&g2, "all").header().edges, 2);
    // Le filtre `road` écarte la rue en gravier, la vue `gravel` la garde et fond les deux.
    let road2 = view_of(&g2, "road");
    assert_eq!(road2.header().edges, 1, "seule la première reste");
    assert_eq!(view_of(&g2, "gravel").header().edges, 1);
}

#[test]
fn one_way_semantics_follow_the_chain_and_inconsistent_directions_are_not_merged() {
    // Rue 1→3→5 à sens unique de même orientation, en deux ways, avec un trottoir en 3.
    let mk = |way11: &[i64], way11_oneway: &str| {
        let mut s = MemSource::default();
        for i in 0..5 {
            at(&mut s, 1 + i, i, 0);
        }
        at(&mut s, 6, 2, 1);
        s.way(10, &[1, 2, 3], &tags("residential", &[("oneway", "yes")]));
        s.way(11, way11, &tags("residential", &[("oneway", way11_oneway)]));
        s.way(12, &[3, 6], &tags("footway", &[]));
        base(&mut s, 11)
    };
    // Même sens (3→5) : chaîne à sens unique.
    let g = mk(&[3, 4, 5], "yes");
    let v = view_of(&g, "road");
    assert_eq!(v.header().edges, 1);
    let a = v.edge(0).attrs;
    assert!(a.fwd_ok() && !a.bwd_ok(), "sens unique 1→5");
    // Way 11 écrite à l'envers (5,4,3) avec `yes` : circule 5→3, donc deux sens qui s'opposent en 3 (puits).
    let g = mk(&[5, 4, 3], "yes");
    let v = view_of(&g, "road");
    assert_eq!(v.header().edges, 2, "un puits n'est pas fondu : le nœud 3 reste atteignable");
    // Même circulation, way 11 à l'envers avec `-1` : 3→5, fondu, et l'arête de vue est à sens unique.
    let g = mk(&[5, 4, 3], "-1");
    let v = view_of(&g, "road");
    assert_eq!(v.header().edges, 1);
    let a = v.edge(0).attrs;
    assert!(a.fwd_ok() && !a.bwd_ok());
    assert_eq!(chain_edges(&v, 0).len(), 2);
    // Un double sens suivi d'un sens unique garde son nœud (sinon 3 ne serait plus atteignable depuis 1).
    let mut s = MemSource::default();
    for i in 0..5 {
        at(&mut s, 1 + i, i, 0);
    }
    at(&mut s, 6, 2, 1);
    s.way(10, &[1, 2, 3], &tags("residential", &[])).way(
        11,
        &[3, 4, 5],
        &tags("residential", &[("oneway", "yes")]),
    );
    s.way(12, &[3, 6], &tags("footway", &[]));
    let v = view_of(&base(&mut s, 11), "road");
    assert_eq!(v.header().edges, 2);
}

#[test]
fn a_chain_that_loops_back_is_cut_and_parallel_edges_survive() {
    // Anneau 1-2-3 (way 10) puis 3-4-1 (way 11), un chemin d'accès 9-1 en résidentiel, un trottoir en 3 :
    // en vue `road`, 3 est de degré 2 et la chaîne repart de 1 pour revenir en 1.
    let mut s = MemSource::default();
    at(&mut s, 1, 0, 0);
    at(&mut s, 2, 1, 1);
    at(&mut s, 3, 2, 0);
    at(&mut s, 4, 1, -1);
    at(&mut s, 9, -1, 0);
    at(&mut s, 6, 2, 1);
    s.way(10, &[1, 2, 3], &tags("residential", &[])).way(11, &[3, 4, 1], &tags("residential", &[]));
    s.way(12, &[9, 1], &tags("residential", &[])).way(13, &[3, 6], &tags("footway", &[]));
    let g = base(&mut s, 11);
    assert_eq!(g.header().edges, 4);
    let v = view_of(&g, "road");
    // Accès 9-1 et deux arêtes parallèles 1-3 (la boucle coupée par le nœud 3) : 3 arêtes, 3 nœuds.
    assert_eq!((v.header().nodes, v.header().edges), (3, 3));
    let mut pairs: Vec<(u32, u32)> = (0..3).map(|e| v.edge_ends(e)).collect();
    pairs.sort_unstable();
    assert!(pairs.iter().all(|(a, b)| a != b), "aucune boucle");
    assert_eq!(pairs.windows(2).filter(|w| w[0] == w[1]).count(), 1, "deux arêtes parallèles gardées");
    assert_eq!(v.view_meta().unwrap().loops_broken, 1);
    // `all` : le trottoir garde 3 et 1 comme nœuds, les deux arêtes 1-3 sont parallèles, rien ne se perd.
    let all = view_of(&g, "all");
    assert_eq!(all.header().edges, 4);
}

#[test]
fn a_ring_without_any_junction_is_cut_in_two_parallel_edges() {
    // Deux ways forment un anneau 1-2-3-4 ; trottoirs en 1 et 3. En `road`, tous les nœuds sont fondus.
    let mut s = MemSource::default();
    at(&mut s, 1, 0, 0);
    at(&mut s, 2, 1, 1);
    at(&mut s, 3, 2, 0);
    at(&mut s, 4, 1, -1);
    at(&mut s, 6, 0, 1);
    at(&mut s, 7, 2, 1);
    s.way(10, &[1, 2, 3], &tags("residential", &[])).way(11, &[3, 4, 1], &tags("residential", &[]));
    s.way(12, &[1, 6], &tags("footway", &[])).way(13, &[3, 7], &tags("footway", &[]));
    let g = base(&mut s, 11);
    let v = view_of(&g, "road");
    assert_eq!((v.header().nodes, v.header().edges), (2, 2));
    let (a0, b0) = v.edge_ends(0);
    assert_eq!((a0, b0), v.edge_ends(1));
    assert_ne!(a0, b0);
    let meta = v.view_meta().unwrap();
    assert_eq!((meta.rings, meta.loops_broken), (1, 1));
    // L'anneau est coupé au plus petit nœud (id de base 0 de la cellule) : les deux chaînes se partagent les 2 arêtes de base.
    let total: usize = (0..2).map(|e| chain_edges(&v, e).len()).sum();
    assert_eq!(total, 2);
}

#[test]
fn dead_end_pruning_removes_spurs_iteratively() {
    // Anneau 1-2-3 / 3-4-1 avec une queue 1-5-6 en impasse (deux ways) : sans élagage la queue reste.
    let mut s = MemSource::default();
    at(&mut s, 1, 0, 0);
    at(&mut s, 2, 1, 1);
    at(&mut s, 3, 2, 0);
    at(&mut s, 4, 1, -1);
    at(&mut s, 5, -1, 0);
    at(&mut s, 6, -2, 0);
    s.way(10, &[1, 2, 3], &tags("residential", &[])).way(11, &[3, 4, 1], &tags("residential", &[]));
    s.way(12, &[1, 5], &tags("residential", &[])).way(13, &[5, 6], &tags("residential", &[]));
    let g = base(&mut s, 11);
    let plain = view_of(&g, "road");
    // Sans élagage : 5 fondu dans la chaîne 1-6, 6 est un cul-de-sac : arêtes 1-3 ×2 (boucle coupée en 3) et 1-6.
    assert_eq!(plain.header().edges, 3);
    let pruned = view_of(&g, "road-prune");
    let meta = pruned.view_meta().unwrap();
    assert_eq!(meta.pruned_edges, 2, "les deux arêtes de la queue sont retirées de proche en proche");
    assert_eq!((pruned.header().nodes, pruned.header().edges), (2, 2));
    // Un arbre disparaît entièrement.
    let mut t = MemSource::default();
    for i in 0..4 {
        at(&mut t, 1 + i, i, 0);
    }
    t.way(10, &[1, 2, 3, 4], &tags("residential", &[]));
    let tree = base(&mut t, 11);
    assert_eq!(view_of(&tree, "road-prune").header().edges, 0);
}

#[test]
fn weights_are_the_sum_of_base_costs_and_access_no_survives_only_in_the_control_view() {
    let mut s = MemSource::default();
    for i in 0..4 {
        at(&mut s, 1 + i, i, 0);
    }
    s.way(10, &[1, 2], &tags("residential", &[])).way(11, &[2, 3], &tags("tertiary", &[]));
    s.way(12, &[3, 4], &tags("residential", &[("bicycle", "no")]));
    let g = base(&mut s, 11);
    // Accès `No` : écarté de `road`, gardé par `all` avec un poids inutilisable.
    let road = view_of(&g, "road");
    assert_eq!(road.header().edges, 1);
    let w = road.view_weights().unwrap()[0].get();
    let expected: u32 = (0..2)
        .map(|e| {
            let ev = g.edge(find(&g, e));
            ord_graph::search::edge_cost(ev.attrs, ev.len_dm).unwrap()
        })
        .sum();
    assert_eq!(w, expected);
    let all = view_of(&g, "all");
    assert_eq!(all.header().edges, 3, "trois jeux d'attributs différents : aucune couture fondue");
    assert!((0..3).any(|e| all.view_weights().unwrap()[e].get() == NO_WEIGHT));
}

/// Arête de base portant la way `10 + k`.
fn find(g: &GraphFile<Vec<u8>>, k: u64) -> u32 {
    (0..g.header().edges as u32).find(|&e| g.identity(e).0 == 10 + k).unwrap()
}

#[test]
fn filter_decisions_are_the_documented_ones() {
    use ord_graph::attrs::classify;
    let a = |extra: &[(&str, &str)], hw: &str| classify(&tags(hw, extra)).unwrap().attrs;
    // Route : classes routières et pistes cyclables, revêtu ou de surface inconnue.
    assert!(ViewKind::Road.keep(a(&[], "residential")));
    assert!(ViewKind::Road.keep(a(&[("surface", "asphalt")], "tertiary")));
    assert!(ViewKind::Road.keep(a(&[], "cycleway")));
    assert!(ViewKind::Road.keep(a(&[("surface", "wood")], "unclassified")), "valeur inconnue : gardée");
    assert!(!ViewKind::Road.keep(a(&[("surface", "gravel")], "residential")));
    for hw in ["footway", "path", "track", "steps", "bridleway", "pedestrian"] {
        assert!(!ViewKind::Road.keep(a(&[("bicycle", "yes")], hw)), "{hw}");
    }
    assert!(!ViewKind::Road.keep(a(&[("service", "driveway")], "service")));
    assert!(!ViewKind::Road.keep(a(&[("service", "parking_aisle")], "service")));
    assert!(ViewKind::Road.keep(a(&[("service", "alley")], "service")));
    assert!(!ViewKind::Road.keep(a(&[("access", "private")], "residential")));
    assert!(!ViewKind::Road.keep(a(&[("bicycle", "no")], "residential")));
    assert!(ViewKind::Road.keep(a(&[("access", "destination")], "residential")), "Destination gardé");
    // Gravel : en plus pistes, chemins, trottoirs désignés ; jamais les escaliers.
    assert!(ViewKind::Gravel.keep(a(&[("surface", "gravel")], "residential")));
    assert!(ViewKind::Gravel.keep(a(&[], "track")));
    assert!(ViewKind::Gravel.keep(a(&[], "path")));
    assert!(!ViewKind::Gravel.keep(a(&[], "footway")));
    assert!(ViewKind::Gravel.keep(a(&[("bicycle", "designated")], "footway")));
    assert!(!ViewKind::Gravel.keep(a(&[("bicycle", "yes")], "steps")));
    assert!(!ViewKind::Gravel.keep(a(&[("access", "private")], "track")));
    assert!(!ViewKind::Gravel.keep(a(&[("service", "driveway")], "service")));
    // Toute arête de `road` est dans `gravel` (qui est un sur-ensemble), et `all` garde tout.
    for hw in ["residential", "cycleway", "tertiary", "service", "track", "path", "footway", "steps"] {
        let x = a(&[], hw);
        assert!(!ViewKind::Road.keep(x) || ViewKind::Gravel.keep(x), "{hw}");
        assert!(ViewKind::All.keep(x));
    }
}

#[test]
fn a_start_inside_a_merged_chain_snaps_to_its_end() {
    let g = base(&mut street_with_footway(), 11);
    let road = view_of(&g, "road");
    let snapper = Snapper::new(&g, ViewKind::Road);
    let nodes = road.view_base_nodes().unwrap();
    let base_id = |osm: u64| (0..g.header().nodes as u32).find(|&v| g.osm_node_id(v) == osm).unwrap();
    // Le nœud 3 est fondu : il se rattache à un bout de la chaîne.
    let snapped = snapper.snap(nodes, base_id(3)).expect("rattaché");
    assert!([base_id(1), base_id(5)].contains(&nodes[snapped as usize].get()));
    // Un nœud de la vue reste lui-même.
    let s1 = snapper.snap(nodes, base_id(1)).unwrap();
    assert_eq!(nodes[s1 as usize].get(), base_id(1));
    // Le bout du trottoir (6) n'a aucune arête retenue : pas de rattachement.
    assert!(snapper.snap(nodes, base_id(6)).is_none());
}

#[test]
fn a_view_of_a_corrupted_file_is_refused() {
    let g = base(&mut street_with_footway(), 11);
    let spec = ViewSpec::parse("road").unwrap();
    let mut data = build_view(&g, &spec).unwrap();
    // Chaîne qui désigne une arête de base hors des comptes.
    let chain = data.extras.iter_mut().find(|x| x.kind == ord_graph::format::KIND_VIEW_CHAIN).unwrap();
    chain.bytes[0..4].copy_from_slice(&0x7FFF_FFF0u32.to_le_bytes());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.ordg");
    // `write_graph` relit et vérifie : l'écriture elle-même refuse.
    assert!(matches!(write_graph(&path, &mut data, "test", 0), Err(FormatError::Truncated(_))));
    assert!(!path.exists());
}

// --- Équivalence avec le graphe de base filtré, sur des graphes pseudo-aléatoires -------------------------------

fn random_source(seed: u64) -> MemSource {
    let mut x = seed;
    let mut rnd = move || {
        x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let mut s = MemSource::default();
    let n = 1500i64;
    for i in 0..n {
        let lat = 47.0 + (rnd() % 100_000) as f64 / 100_000.0 * 0.3;
        let lon = -1.9 + (rnd() % 100_000) as f64 / 100_000.0 * 0.45;
        s.node(1000 + i, lat, lon);
    }
    let classes =
        ["residential", "tertiary", "cycleway", "track", "path", "service", "footway", "unclassified"];
    for w in 0..1100i64 {
        let len = 2 + (rnd() % 8) as usize;
        let start = (rnd() % n as u64) as i64;
        let refs: Vec<i64> =
            (0..len as i64).map(|k| 1000 + (start + k * (1 + (rnd() % 3) as i64)) % n).collect();
        let class = classes[(rnd() % classes.len() as u64) as usize];
        let mut t = vec![("highway", class)];
        match rnd() % 9 {
            0 => t.push(("oneway", "yes")),
            1 => t.push(("oneway", "-1")),
            2 => t.push(("surface", "gravel")),
            3 => t.push(("bicycle", "no")),
            4 => t.push(("access", "private")),
            5 if class == "service" => t.push(("service", "driveway")),
            6 => t.push(("surface", "asphalt")),
            _ => {}
        }
        s.way(500 + w, &refs, &t);
    }
    s
}

#[test]
fn views_give_the_distances_of_the_filtered_base_graph_in_every_state() {
    let g = base(&mut random_source(77), 12);
    let be = BaseEdges::load(&g);
    let mut any_merge = false;
    for name in ["road", "gravel", "all", "road-prune", "gravel-prune", "all-prune"] {
        let spec = ViewSpec::parse(name).unwrap();
        let v = view_of(&g, name);
        let (alive, _) = alive_edges(&be, &spec);
        let alive_n = alive.iter().filter(|&&x| x).count();
        assert!(
            v.header().edges as usize <= alive_n,
            "{name} : une vue n'a jamais plus d'arêtes que le filtre"
        );
        any_merge |= (v.header().edges as usize) < alive_n;
        let inc = Incidence::new(&be, &alive);
        let nodes = v.view_base_nodes().unwrap();
        let mut compared = 0;
        let mut reached_somewhere = false;
        let step = (v.header().nodes as usize / 3).max(1);
        for start in (0..v.header().nodes as u32).step_by(step) {
            let reference = filtered_reference_distances(&be, &inc, nodes[start as usize].get());
            let mut results = Vec::new();
            for kind in StateKind::ALL {
                let cfg =
                    SearchConfig { radius_m: 1e7, forbid_uturn: false, state: kind, collect_nodes: true };
                results.push(Searcher::new(&v).run(start, &cfg).node_dist);
            }
            assert!(results.windows(2).all(|w| w[0] == w[1]), "{name} : états différents depuis {start}");
            let got: std::collections::HashMap<u32, u32> = results[0].iter().copied().collect();
            for (vn, base_node) in nodes.iter().enumerate() {
                let want = reference[base_node.get() as usize];
                match got.get(&(vn as u32)) {
                    Some(&d) => assert_eq!(d, want, "{name} : distance au nœud {vn} depuis {start}"),
                    None => {
                        assert_eq!(want, u32::MAX, "{name} : nœud {vn} atteint par la base, pas par la vue")
                    }
                }
                compared += 1;
            }
            reached_somewhere |= got.len() > 5;
        }
        assert!(compared > 0 && reached_somewhere, "{name} : rien de comparé");
    }
    assert!(any_merge, "la contraction doit agir sur ce graphe");
}

#[test]
fn view_geometry_is_the_concatenation_of_its_base_edges() {
    let g = base(&mut random_source(5), 12);
    let v = view_of(&g, "gravel");
    for e in 0..v.header().edges as u32 {
        let mut expected = Vec::new();
        let chain = chain_edges(&v, e);
        for (i, &(be, rev)) in chain.iter().enumerate() {
            let mut inner = g.geometry(be);
            if rev {
                inner.reverse();
            }
            expected.extend(inner);
            if i + 1 < chain.len() {
                let (a, b) = g.edge_ends(be);
                expected.push(g.node_pos(if rev { a } else { b }));
            }
        }
        assert_eq!(v.geometry(e), expected, "arête {e}");
    }
}

#[test]
fn view_files_are_deterministic() {
    let g = base(&mut random_source(9), 12);
    let digests = |g: &GraphFile<Vec<u8>>| g.section_digests();
    assert_eq!(digests(&view_of(&g, "road")), digests(&view_of(&g, "road")));
}

#[test]
fn road_view_keeps_greenways_and_paved_tracks_but_never_known_unpaved() {
    use ord_graph::attrs::classify;
    let a = |extra: &[(&str, &str)], hw: &str| classify(&tags(hw, extra)).unwrap().attrs;
    // Voies vertes : désignées aux vélos, surface revêtue ou inconnue.
    for hw in ["path", "footway", "pedestrian", "bridleway"] {
        assert!(ViewKind::Road.keep(a(&[("bicycle", "designated")], hw)), "{hw} désigné sans surface");
        assert!(
            ViewKind::Road.keep(a(&[("bicycle", "designated"), ("surface", "asphalt")], hw)),
            "{hw} asphalte"
        );
        assert!(
            !ViewKind::Road.keep(a(&[("bicycle", "designated"), ("surface", "gravel")], hw)),
            "{hw} gravier"
        );
        assert!(!ViewKind::Road.keep(a(&[("bicycle", "yes")], hw)), "{hw} seulement permis");
        assert!(!ViewKind::Road.keep(a(&[], hw)), "{hw} sans tag");
    }
    // `track` : grade1 ou surface connue revêtue ; sans surface ni grade, écarté.
    assert!(ViewKind::Road.keep(a(&[("tracktype", "grade1")], "track")));
    assert!(ViewKind::Road.keep(a(&[("surface", "asphalt")], "track")));
    assert!(!ViewKind::Road.keep(a(&[], "track")));
    assert!(!ViewKind::Road.keep(a(&[("tracktype", "grade3")], "track")));
    assert!(
        !ViewKind::Road.keep(a(&[("tracktype", "grade1"), ("surface", "gravel")], "track")),
        "jamais de non-revêtu connu"
    );
    // Toujours un sous-ensemble de gravel.
    for (extra, hw) in [(vec![("bicycle", "designated")], "path"), (vec![("tracktype", "grade1")], "track")] {
        let x = a(&extra, hw);
        assert!(!ViewKind::Road.keep(x) || ViewKind::Gravel.keep(x), "{hw}");
    }
}

#[test]
fn view_headings_follow_the_chain_geometry() {
    use ord_graph::view::{heading_from_u16, polyline_heading};
    // Rue qui part plein est, puis tourne plein nord.
    let mut s = MemSource::default();
    at(&mut s, 1, 0, 0);
    at(&mut s, 2, 0, 2);
    at(&mut s, 3, 2, 2);
    s.way(10, &[1, 2, 3], &tags("residential", &[]));
    let g = base(&mut s, 11);
    let road = view_of(&g, "road");
    let h = road.view_headings().expect("section VIEW_HEADING");
    assert_eq!(h.len() as u64, road.header().edges);
    let (start, end) = (heading_from_u16(h[0][0]), heading_from_u16(h[0][1]));
    let near = |x: f64, y: f64| ((x - y + 540.0).rem_euclid(360.0) - 180.0).abs() < 2.0;
    assert!(
        (near(start, 90.0) && near(end, 0.0)) || (near(start, 180.0) && near(end, 270.0)),
        "caps {start} / {end}"
    );
    // Fonction pure : ligne droite plein sud.
    let pts = [(472_000_000, -15_600_000), (471_990_000, -15_600_000), (471_980_000, -15_600_000)];
    assert!((polyline_heading(&pts, true) - 180.0).abs() < 0.5);
    assert!((polyline_heading(&pts, false) - 180.0).abs() < 0.5);
}

#[test]
fn the_allocation_free_readers_agree_with_the_bulk_ones() {
    let g = base(&mut street_with_footway(), 11);
    let v = view_of(&g, "road");
    let all = v.view_headings().expect("section de caps");
    assert_eq!(v.view_heading_count(), all.len());
    for (c, h) in all.iter().enumerate() {
        assert_eq!(v.view_heading(c as u32), Some(*h));
    }
    assert_eq!(v.view_heading(all.len() as u32), None);
    let nodes = v.view_base_nodes().unwrap();
    for (i, n) in nodes.iter().enumerate() {
        assert_eq!(v.view_node_of_base(n.get()), Some(i as u32));
    }
    // Le nœud 6 (bout du trottoir) est dans la base mais pas dans la vue `road`.
    let n6 = (0..g.header().nodes as u32).find(|&n| g.osm_node_id(n) == 6).unwrap();
    assert_eq!(v.view_node_of_base(n6), None);
    for e in 0..g.header().edges as u32 {
        assert_eq!(g.edge_way(e), g.identity(e).0);
    }
}
