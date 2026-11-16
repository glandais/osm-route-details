//! Index globaux d'une vue (lot 6, §4.1 et §6), comparés à la force brute et aux tableaux à plat.

use std::collections::BTreeSet;

use ord_graph::grid::{tile_key, tile_of_e7};

use ord_graph::view::BaseEdges;

use crate::engine::{chain_strong_len, strong_components, strong_lengths, Engine, Params};
use crate::index::tiles_of_polyline;
use crate::tests::engine_zoomed;
use crate::tests_reference::scenario_graph;

fn engines() -> Vec<Engine<Vec<u8>>> {
    // Un cap de composante bas, pour que le tableau creux ait des chaînes des deux côtés du seuil.
    let small_cap = Params { max_distance_m: 2_000.0, ..Params::default() };
    vec![
        engine_zoomed(&mut scenario_graph(7), 16, Params::default()),
        engine_zoomed(&mut scenario_graph(7), 16, small_cap.clone()),
        engine_zoomed(&mut scenario_graph(9), 14, small_cap),
        engine_zoomed(&mut scenario_graph(7), 11, Params::default()),
    ]
}

#[test]
fn foreign_chains_are_exactly_the_chains_that_touch_a_tile_without_owning_it() {
    for e in engines() {
        for vi in 0..2 {
            let view = &e.views_files[vi];
            let zoom = view.header().zoom;
            let mut want: BTreeSet<(u32, u32)> = BTreeSet::new();
            for c in 0..view.header().edges as u32 {
                let (a, b) = view.edge_ends(c);
                let mut pts = vec![view.node_pos(a)];
                pts.extend(view.geometry(c));
                pts.push(view.node_pos(b));
                let own = view.edge_cell(c);
                let own = tile_key(view.cell_xy(own).0, view.cell_xy(own).1);
                tiles_of_polyline(&pts, zoom, |k| {
                    if k != own {
                        want.insert((k, c));
                    }
                });
            }
            let got: BTreeSet<(u32, u32)> = e.idx[vi].foreign.iter().copied().collect();
            assert_eq!(got.len(), e.idx[vi].foreign.len(), "doublons");
            assert_eq!(got, want, "vue {vi}, z{zoom}");
            if zoom == 16 {
                assert!(got.len() > 100, "le scénario z16 doit avoir des chaînes étrangères");
            }
        }
    }
}

#[test]
fn every_node_sits_in_the_tile_of_its_cell() {
    for e in engines() {
        for view in &e.views_files {
            let zoom = view.header().zoom;
            for n in 0..view.header().nodes as u32 {
                let p = view.node_pos(n);
                let (x, y) = view.cell_xy(view.node_cell(n));
                assert_eq!(tile_of_e7(p.0, p.1, zoom), (x, y));
            }
        }
    }
}

/// Longueur de la composante fortement connexe de chaque chaîne, calculée **à plat** depuis les extrémités et les sens
/// ouverts (sans lire le CSR des cellules) : la référence du moteur d'avant le lot 6.
fn flat_comp_lengths(view: &ord_graph::format::GraphFile<Vec<u8>>) -> Vec<u64> {
    let be = BaseEdges::load(view);
    let n = view.header().nodes as usize;
    let mut out: Vec<Vec<u32>> = vec![Vec::new(); n];
    for (c, &(a, b)) in be.ends.iter().enumerate() {
        if be.attrs[c].fwd_ok() {
            out[a as usize].push(b);
        }
        if be.attrs[c].bwd_ok() {
            out[b as usize].push(a);
        }
    }
    let comp = strong_components(n, &|x, s| s.extend(out[x as usize].iter().copied()));
    let lens = strong_lengths(&comp, &be.ends, &be.len_dm, &|_| true);
    be.ends.iter().map(|&e| chain_strong_len(&comp, &lens, e)).collect()
}

#[test]
fn the_sparse_component_lengths_agree_with_the_flat_ones_below_the_cap() {
    let mut below = 0;
    let mut above = 0;
    for e in engines() {
        for vi in 0..2 {
            let idx = &e.idx[vi];
            let flat = flat_comp_lengths(&e.views_files[vi]);
            for (c, &l) in flat.iter().enumerate() {
                if l < idx.cap_dm {
                    below += 1;
                    assert_eq!(idx.comp_len_dm(c as u32), l, "chaîne {c}");
                } else {
                    above += 1;
                    assert_eq!(idx.comp_len_dm(c as u32), idx.cap_dm, "chaîne {c}");
                }
            }
            let lmax = BaseEdges::load(&e.views_files[vi]).len_dm.into_iter().max().unwrap();
            assert_eq!(idx.lmax_dm, lmax);
        }
    }
    assert!(below > 0 && above > 0, "le cap doit couper la distribution ({below}/{above})");
}

#[test]
fn the_self_check_passes_on_the_synthetic_graphs() {
    let mut seen = crate::SelfCheck::default();
    for e in engines() {
        let c = e.self_check().expect("auto-contrôle");
        seen.foreign[0] += c.foreign[0];
        seen.components[0] += c.components[0];
        seen.degrees[1] += c.degrees[1];
        seen.base_edges += c.base_edges;
    }
    assert!(seen.foreign[0] > 0 && seen.components[0] > 0 && seen.degrees[1] > 0 && seen.base_edges > 1000);
}
