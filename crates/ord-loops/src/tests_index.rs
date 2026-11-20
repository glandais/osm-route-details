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
            let got: BTreeSet<(u32, u32)> = e.idx[vi].foreign_all().iter().collect();
            assert_eq!(got.len(), e.idx[vi].foreign_all().len(), "doublons");
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

// --- Fichier d'index de service (note `ord-loops-planete.md` §5) -------------------------------------------------

/// Les fichiers d'un graphe de scénario (base, `road`, `gravel`), en octets : à ouvrir deux fois.
fn scenario_bytes(seed: u64, zoom: u8, dir: &std::path::Path) -> [Vec<u8>; 3] {
    use ord_graph::build::{build_graph, BuildOptions};
    use ord_graph::view::{build_view, ViewSpec};
    let data = build_graph(&mut scenario_graph(seed), &BuildOptions { zoom }, "test", &mut |_| {}).unwrap();
    let base = crate::tests::roundtrip(data, dir, "b.ordg");
    let view = |n: &str| {
        crate::tests::roundtrip(
            build_view(&base, &ViewSpec::parse(n).unwrap()).unwrap(),
            dir,
            &format!("b.{n}.ordv"),
        )
    };
    let (_, _) = (view("road"), view("gravel"));
    ["b.ordg", "b.road.ordv", "b.gravel.ordv"].map(|n| std::fs::read(dir.join(n)).unwrap())
}

fn open3(b: &[Vec<u8>; 3]) -> [ord_graph::format::GraphFile<Vec<u8>>; 3] {
    b.clone().map(|x| ord_graph::format::GraphFile::open(x).unwrap())
}

/// Écrit le fichier d'index de ces fichiers, au seuil `cap_dm`, par les fonctions du démarrage.
fn index_file(
    b: &[Vec<u8>; 3],
    cap_dm: u64,
    dir: &std::path::Path,
) -> ord_graph::service_index::IndexFile<Vec<u8>> {
    use ord_graph::service_index::{node_sample, write_index, IndexData, IndexFile, ViewTables};
    let [base, road, gravel] = open3(b);
    let tables = [&road, &gravel].map(|v| {
        let (short, lmax_dm) = crate::index::short_components(v, cap_dm);
        ViewTables {
            foreign: crate::index::foreign_index(v, 2),
            short: short.into_iter().map(|(c, l)| (c, l as u32)).collect(),
            lmax_dm,
        }
    });
    let (node_bbox, sample) = node_sample(&base);
    let data = IndexData {
        views: tables,
        cap_dm: cap_dm as u32,
        wide_seg: Vec::new(),
        wide_end: Vec::new(),
        node_bbox,
        node_sample: sample,
    };
    let path = dir.join("b.ordi");
    let _ = std::fs::remove_file(&path);
    write_index(&path, &base, [&road, &gravel], &data, b"{}", 0).unwrap();
    IndexFile::open(std::fs::read(&path).unwrap()).unwrap()
}

#[test]
fn an_engine_on_the_index_file_reads_the_tables_it_would_compute_and_generates_the_same_routes() {
    use crate::generate::generate_with_diagnostics_in;
    use crate::tests_reference::{scenario_params, scenario_requests};
    use crate::Workspace;
    use std::sync::Arc;
    let small_cap = Params { max_distance_m: 2_000.0, ..scenario_params() };
    for (seed, zoom, params) in [(7, 16, scenario_params()), (9, 14, small_cap)] {
        let dir = tempfile::tempdir().unwrap();
        let b = scenario_bytes(seed, zoom, dir.path());
        let cap = crate::index::cap_dm_of(&params);
        // Le fichier au seuil des réglages, puis à un seuil plus haut : mêmes réponses.
        for file_cap in [cap, cap * 3] {
            let [base, road, gravel] = open3(&b);
            let computed = Engine::new(base, road, gravel, params.clone()).unwrap();
            let [base, road, gravel] = open3(&b);
            let f = Arc::new(index_file(&b, file_cap, dir.path()));
            let filed = Engine::with_index(base, road, gravel, params.clone(), f).unwrap();
            assert!(filed.index_from_file() && !computed.index_from_file());
            assert_eq!(filed.index_bytes(), 0);
            for vi in 0..2 {
                let (c, d) = (&computed.idx[vi], &filed.idx[vi]);
                assert_eq!(
                    c.foreign_all().iter().collect::<Vec<_>>(),
                    d.foreign_all().iter().collect::<Vec<_>>()
                );
                assert_eq!(c.lmax_dm, d.lmax_dm);
                assert_eq!(c.cap_dm, d.cap_dm);
                for ch in 0..computed.views_files[vi].header().edges as u32 {
                    assert_eq!(
                        c.comp_len_dm(ch),
                        d.comp_len_dm(ch),
                        "vue {vi}, chaîne {ch}, seuil {file_cap}"
                    );
                }
            }
            for r in scenario_requests(seed, 12) {
                let run = |e: &Engine<Vec<u8>>| {
                    let plan = e.plan(&r, 0.0);
                    let win = e.build_window(r.ride_type.view_index(), &plan.need, &mut || false).unwrap();
                    let g =
                        generate_with_diagnostics_in(e, &win, None, &r, &mut Workspace::new(), &mut || false);
                    (plan.need, format!("{g:?}"))
                };
                assert_eq!(run(&computed), run(&filed), "{r:?}");
            }
        }
    }
}

#[test]
fn an_index_file_of_another_build_or_with_a_lower_cap_is_refused() {
    use std::sync::Arc;
    let params = Params::default();
    let cap = crate::index::cap_dm_of(&params);
    let dir = tempfile::tempdir().unwrap();
    let b = scenario_bytes(7, 16, dir.path());
    let low = Arc::new(index_file(&b, cap - 1, dir.path()));
    let [base, road, gravel] = open3(&b);
    let err = Engine::with_index(base, road, gravel, params.clone(), low).unwrap_err();
    assert!(matches!(err, crate::EngineError::IndexMismatch(ref s) if s.contains("seuil")), "{err}");
    // Le fichier d'un autre graphe (autre graine).
    let other = tempfile::tempdir().unwrap();
    let b2 = scenario_bytes(9, 16, other.path());
    let f2 = Arc::new(index_file(&b2, cap, other.path()));
    let [base, road, gravel] = open3(&b);
    let err = Engine::with_index(base, road, gravel, params, f2).unwrap_err();
    assert!(matches!(err, crate::EngineError::IndexMismatch(_)), "{err}");
}
