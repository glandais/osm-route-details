//! Tables creuses du contrôleur (note lot 6, §20.3 point 7 et §20.4) : sur 200 surcouches tirées d'une graine fixe,
//! la forme canonique des arêtes comblées est celle que la construction d'avant la finition 2 rendait.
//!
//! La fixture `tests/fixtures/team_check_reference.txt` a été écrite **avant** tout changement de la construction
//! (`ORD_LOOPS_BLESS=1` la réécrit).

use ord_graph::format::GraphFile;
use ord_graph::overlay::{encode, Overlay, OverlayMeta};
use ord_graph::util::SplitMix64;

use crate::tests_bounded::{graph, N};
use crate::{Config, GraphIndex, TeamCheck, TeamCheckEscape};

pub(crate) const OVERLAYS: usize = 200;

pub(crate) fn cfg() -> Config {
    Config { hole_m: 300.0, connector_m: 150.0, ..Config::default() }
}

/// Surcouche tirée : de un à trois rectangles de 2 à 8 carrefours de côté, une part tirée de leurs arêtes roulée,
/// une part des arêtes en gravel seulement (types variés).
pub(crate) fn overlay_var(g: &GraphFile<Vec<u8>>, seed: u64) -> Overlay {
    let mut rng = SplitMix64(seed.wrapping_mul(0x9e37_79b9) ^ 0x5bd1);
    let rects: Vec<(usize, usize, usize, usize)> = (0..1 + rng.below(3))
        .map(|_| {
            let (i0, j0) = (rng.below(N - 2), rng.below(N - 2));
            (i0, j0, (i0 + 1 + rng.below(7)).min(N - 1), (j0 + 1 + rng.below(7)).min(N - 1))
        })
        .collect();
    let keep = 45 + rng.below(56);
    let pos = |osm: u64| ((osm - 1) as usize / N, (osm - 1) as usize % N);
    let (mut edges, mut counts) = (Vec::new(), Vec::new());
    for e in 0..g.header().edges as u32 {
        let (a, b) = g.edge_ends(e);
        let (pa, pb) = (pos(g.osm_node_id(a)), pos(g.osm_node_id(b)));
        let inside =
            |p: (usize, usize)| rects.iter().any(|r| p.0 >= r.0 && p.0 <= r.2 && p.1 >= r.1 && p.1 <= r.3);
        if inside(pa) && inside(pb) && rng.below(100) < keep {
            edges.push(e);
            let c: [u16; 8] =
                if rng.below(4) == 0 { [0, 0, 1, 1, 0, 0, 0, 0] } else { [1, 1, 0, 0, 0, 0, 0, 0] };
            counts.extend_from_slice(&c);
        }
    }
    let meta = OverlayMeta {
        graph: g.graph_identity(),
        team_id: 1,
        export_xxh3: 0,
        export_rows: 0,
        params_hash: 0,
        tool_version: "test".into(),
        build_epoch: 0,
        types: vec!["ROAD".into(), "GRAVEL".into(), "MTB".into(), "MIXED".into()],
    };
    Overlay::decode(&encode(&meta, &edges, &counts).unwrap()).unwrap()
}

fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &x| (h ^ u64::from(x)).wrapping_mul(0x0000_0100_0000_01b3))
}

pub(crate) fn fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/team_check_reference.txt")
}

pub(crate) type Build<'a> = &'a dyn Fn(&GraphFile<Vec<u8>>, &GraphIndex, &Overlay) -> TeamCheck;

pub(crate) fn fixture_lines(build: Build<'_>) -> Vec<String> {
    let (g, _d) = graph();
    let gi = GraphIndex::build(&g);
    (0..OVERLAYS as u64)
        .map(|i| {
            let ov = overlay_var(&g, i);
            let bytes = build(&g, &gi, &ov).canonical_bytes();
            format!("{i} rows={} {:016x} {}", ov.len(), fnv(&bytes), bytes.len())
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
fn the_team_check_still_matches_the_frozen_reference() {
    assert_matches_fixture(&fixture_lines(&|g, gi, ov| TeamCheck::build(g, gi, ov, &cfg())));
}

#[test]
fn the_reference_overlays_bridge_edges() {
    let total: usize = fixture_lines(&|g, gi, ov| TeamCheck::build(g, gi, ov, &cfg()))
        .iter()
        .filter_map(|l| l.rsplit(' ').next()?.parse::<usize>().ok())
        .map(|n| n.saturating_sub(24) / 4)
        .sum();
    assert!(total > 200, "le scénario doit combler des trous : {total}");
}

#[test]
fn the_kept_reference_construction_matches_the_frozen_reference() {
    assert_matches_fixture(&fixture_lines(&|g, gi, ov| TeamCheck::build_reference(g, gi, ov, &cfg())));
}

#[test]
fn the_sparse_tables_are_the_reference_and_the_whole_graph_byte_for_byte() {
    let (g, _d) = graph();
    let gi = GraphIndex::build(&g);
    for i in 0..OVERLAYS as u64 {
        let ov = overlay_var(&g, i);
        let sparse = TeamCheck::build(&g, &gi, &ov, &cfg());
        assert!(!sparse.zone_retried(), "{i} : reprise inattendue");
        let want = sparse.canonical_bytes();
        assert!(
            TeamCheck::build_reference(&g, &gi, &ov, &cfg()).canonical_bytes() == want,
            "{i} : creuse ≠ référence"
        );
        assert!(
            TeamCheck::build_on(&g, &gi, &ov, &cfg(), true).canonical_bytes() == want,
            "{i} : creuse ≠ graphe entier"
        );
    }
}

#[test]
fn a_forced_escape_is_retried_at_one_kilometre_and_gives_the_same_bytes_or_raises() {
    let (g, _d) = graph();
    let gi = GraphIndex::build(&g);
    let (mut escaped, mut quiet) = (0, 0);
    for i in 0..OVERLAYS as u64 {
        let ov = overlay_var(&g, i);
        let want = TeamCheck::build_reference(&g, &gi, &ov, &cfg()).canonical_bytes();
        match TeamCheck::build_forced(&g, &gi, &ov, &cfg(), 1.0, false) {
            Err(TeamCheckEscape) => {
                escaped += 1;
                let again = TeamCheck::build_forced(&g, &gi, &ov, &cfg(), 1.0, true).unwrap();
                assert!(again.zone_retried(), "{i}");
                assert!(again.canonical_bytes() == want, "{i} : la reprise change le résultat");
            }
            Ok(t) => {
                quiet += 1;
                assert!(t.canonical_bytes() == want, "{i} : résultat creux faux sans échappement");
            }
        }
    }
    assert!(escaped > 20, "le garde-fou n'a presque jamais été levé : {escaped} ({quiet} sans)");
}
