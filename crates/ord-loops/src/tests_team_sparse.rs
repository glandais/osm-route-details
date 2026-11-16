//! Construction creuse des données d'équipe (note lot 6, §20.3 et §20.4) : sur 200 surcouches tirées d'une graine
//! fixe, la forme canonique (octets) de [`TeamData`] est celle que la construction d'avant la finition 2 rendait.
//!
//! La fixture `tests/fixtures/team_reference.txt` a été écrite **avant** tout changement de la construction
//! (`ORD_LOOPS_BLESS=1` la réécrit) ; elle garde l'empreinte FNV-1a de la forme canonique, par surcouche.

use ord_graph::util::SplitMix64;

use crate::engine::{Engine, Params};
use crate::team::TeamData;
use crate::tests::{engine_zoomed, overlay_of, Counts};
use crate::tests_reference::{fnv, scenario_graph, scenario_graph_dense, scenario_params, N, ZOOMS};

/// Trous de 300 m et jonctions de 150 m : sur des rues de 140 m, la règle des trous comble vraiment.
pub(crate) fn team_params() -> Params {
    Params { hole_m: 300.0, connector_m: 150.0, ..scenario_params() }
}

pub(crate) const OVERLAYS: usize = 200;

/// Les quatre moteurs de la référence : deux zooms (z11 une tuile, z16 des tuiles de ~400 m), grille à chaînes et
/// grille dense.
pub(crate) fn engines() -> Vec<(String, Engine<Vec<u8>>)> {
    let mut out = Vec::new();
    for zoom in ZOOMS {
        out.push((format!("z{zoom}"), engine_zoomed(&mut scenario_graph(7), zoom, team_params())));
        out.push((format!("z{zoom}d"), engine_zoomed(&mut scenario_graph_dense(5), zoom, team_params())));
    }
    out
}

/// Surcouche tirée : de un à trois rectangles de 2 à 8 carrefours de côté, une part tirée de leurs arêtes roulée,
/// types, sens et fréquences variés.
pub(crate) fn sparse_rows(e: &Engine<Vec<u8>>, seed: u64) -> Vec<(u32, Counts)> {
    let mut rng = SplitMix64(seed.wrapping_mul(0x9e37_79b9) ^ 0x5bd1);
    let rects: Vec<(usize, usize, usize, usize)> = (0..1 + rng.below(3))
        .map(|_| {
            let (i0, j0) = (rng.below(N - 2), rng.below(N - 2));
            (i0, j0, (i0 + 1 + rng.below(7)).min(N - 1), (j0 + 1 + rng.below(7)).min(N - 1))
        })
        .collect();
    let keep = 45 + rng.below(56);
    let pos = |osm: u64| ((osm - 1) as usize / N, (osm - 1) as usize % N);
    let g = e.base();
    let mut rows = Vec::new();
    for ed in 0..g.header().edges as u32 {
        let (a, b) = g.edge_ends(ed);
        let (pa, pb) = (pos(g.osm_node_id(a)), pos(g.osm_node_id(b)));
        let inside =
            |p: (usize, usize)| rects.iter().any(|r| p.0 >= r.0 && p.0 <= r.2 && p.1 >= r.1 && p.1 <= r.3);
        if !(inside(pa) && inside(pb)) || rng.below(100) >= keep {
            continue;
        }
        let m = |rng: &mut SplitMix64| rng.below(4) as u16;
        let mut c: Counts = [0; 8];
        match rng.below(5) {
            0 => (c[0], c[1]) = (m(&mut rng) + 1, m(&mut rng)),
            1 => (c[2], c[3]) = (m(&mut rng), m(&mut rng) + 1),
            2 => (c[4], c[5]) = (m(&mut rng) + 1, m(&mut rng) + 1),
            3 => (c[6], c[7]) = (m(&mut rng) + 1, m(&mut rng) + 1),
            _ => {
                c[0] = 1;
                c[1] = 1;
                c[2] = m(&mut rng);
            }
        }
        rows.push((ed, c));
    }
    rows
}

pub(crate) fn fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/team_reference.txt")
}

fn hex(b: &[u8]) -> String {
    format!("{:016x}", fnv(&b.iter().map(|x| format!("{x:02x}")).collect::<String>()))
}

/// Une ligne par surcouche : indice, moteur, nombre de lignes, empreinte et taille de la forme canonique.
pub(crate) fn fixture_lines(
    build: &dyn Fn(&Engine<Vec<u8>>, &ord_graph::overlay::Overlay) -> TeamData,
) -> Vec<String> {
    let engines = engines();
    (0..OVERLAYS)
        .map(|i| {
            let (name, e) = &engines[i % engines.len()];
            let rows = sparse_rows(e, i as u64);
            let bytes = build(e, &overlay_of(e, &rows)).canonical_bytes();
            format!("{i} {name} rows={} {} {}", rows.len(), hex(&bytes), bytes.len())
        })
        .collect()
}

pub(crate) fn assert_matches_fixture(lines: &[String]) {
    let path = fixture_path();
    if std::env::var_os("ORD_LOOPS_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).expect("fixture de référence absente (ORD_LOOPS_BLESS=1)");
    let want: Vec<&str> = want.lines().collect();
    assert_eq!(want.len(), lines.len(), "nombre de surcouches");
    let bad: Vec<String> = want
        .iter()
        .zip(lines)
        .filter(|(w, g)| w != g)
        .map(|(w, g)| format!("attendu {w}\n   reçu {g}"))
        .collect();
    assert!(bad.is_empty(), "{} écarts à la référence :\n{}", bad.len(), bad[..bad.len().min(8)].join("\n"));
}

#[test]
fn the_team_data_still_matches_the_frozen_reference() {
    assert_matches_fixture(&fixture_lines(&|e, ov| TeamData::build(e, ov).unwrap()));
}

#[test]
fn the_reference_overlays_exercise_bridging_and_strict_components() {
    let engines = engines();
    let (mut bridged, mut strict, mut empty) = (0u64, 0usize, 0usize);
    for i in 0..OVERLAYS {
        let (_, e) = &engines[i % engines.len()];
        let rows = sparse_rows(e, i as u64);
        empty += usize::from(rows.is_empty());
        let t = TeamData::build(e, &overlay_of(e, &rows)).unwrap();
        for r in crate::RideType::ALL {
            bridged += t.of(r).edges.iter().filter(|x| x.1 & crate::team::BRIDGED != 0).count() as u64;
            strict += t.of(r).strict_len.len();
        }
    }
    assert!(bridged > 100 && strict > 100 && empty < 20, "bridged {bridged} strict {strict} vides {empty}");
}
