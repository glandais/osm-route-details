//! Référence figée du lot 6 : sorties du moteur à plat du 16 novembre 2026 sur un graphe synthétique de 48 × 48
//! carrefours (sens uniques, classes, pistes et trous tirés d'une graine fixe), construit à z11 (une tuile) et à z16
//! (tuiles de ~400 m : une requête de quelques kilomètres a une vraie fenêtre partielle).
//!
//! La fixture `tests/fixtures/window_reference.txt` garde, par requête, l'empreinte FNV-1a de la sortie complète
//! (`Debug` du résultat et des diagnostics : étapes, positions, mesures, refus) et un résumé lisible. Elle a été
//! écrite **avant** le moteur fenêtré ; tout moteur doit la relire à l'identique. `ORD_LOOPS_BLESS=1` la réécrit.

use std::fmt::Write as _;

use ord_graph::build::MemSource;
use ord_graph::util::SplitMix64;

use crate::engine::{Engine, Params};
use crate::generate::generate_with_diagnostics;
use crate::team::TeamData;
use crate::tests::{engine_zoomed, ll, overlay_of, pt, Counts};
use crate::{Bearing8, Diagnostics, Failure, Generation, Mode, Request, RideType, Workspace};

pub(crate) const N: usize = 48;
pub(crate) const CELL: f64 = 140.0;

pub(crate) fn id(i: usize, j: usize) -> i64 {
    (1 + i * N + j) as i64
}

/// Grille de 48 × 48 carrefours dont chaque segment est une way : 7 % de segments absents (nœuds de degré 2 et 3,
/// donc des chaînes), 8 % à sens unique, classes et surfaces variées.
pub(crate) fn scenario_graph(seed: u64) -> MemSource {
    build_scenario(seed, 70)
}

/// Même grille sans segment absent ni voie hors route : des chaînes d'un segment, donc une zone d'accroche étroite
/// et des fenêtres vraiment partielles.
pub(crate) fn scenario_graph_dense(seed: u64) -> MemSource {
    build_scenario(seed, 0)
}

fn build_scenario(seed: u64, missing_per_mille: usize) -> MemSource {
    let mut rng = SplitMix64(seed);
    let mut s = MemSource::default();
    for i in 0..N {
        for j in 0..N {
            pt(&mut s, id(i, j), i as f64 * CELL, j as f64 * CELL);
        }
    }
    let mut way = 1_000_000i64;
    let mut seg = |s: &mut MemSource, a: i64, b: i64| {
        way += 1;
        let r = rng.below(1000);
        if r < missing_per_mille {
            return;
        }
        let (x, y) = if rng.below(2) == 0 { (a, b) } else { (b, a) };
        let class = if missing_per_mille == 0 { rng.below(70) } else { rng.below(100) };
        let mut t: Vec<(&str, &str)> = match class {
            0..=54 => vec![("highway", "residential")],
            55..=69 => vec![("highway", "tertiary")],
            70..=79 => vec![("highway", "cycleway")],
            80..=87 => vec![("highway", "track"), ("tracktype", "grade4")],
            88..=93 => vec![("highway", "path")],
            94..=96 => vec![("highway", "track"), ("surface", "gravel")],
            _ => vec![("highway", "secondary")],
        };
        if r > 920 {
            t.push(("oneway", "yes"));
        }
        s.way(way, &[x, y], &t);
    };
    for i in 0..N {
        for j in 0..N {
            if j + 1 < N {
                seg(&mut s, id(i, j), id(i, j + 1));
            }
            if i + 1 < N {
                seg(&mut s, id(i, j), id(i + 1, j));
            }
        }
    }
    s
}

/// Surcouche tirée : deux rectangles du réseau, 80 % de leurs arêtes roulées, types et sens variés.
pub(crate) fn scenario_rows(e: &Engine<Vec<u8>>, seed: u64) -> Vec<(u32, Counts)> {
    let mut rng = SplitMix64(seed);
    let rects: Vec<(usize, usize, usize, usize)> = (0..2)
        .map(|_| {
            let (i0, j0) = (rng.below(N / 2), rng.below(N / 2));
            (i0, j0, i0 + N / 4 + rng.below(N / 4), j0 + N / 4 + rng.below(N / 4))
        })
        .collect();
    let pos = |osm: u64| ((osm - 1) as usize / N, (osm - 1) as usize % N);
    let g = e.base();
    let mut rows = Vec::new();
    for ed in 0..g.header().edges as u32 {
        let (a, b) = g.edge_ends(ed);
        let (pa, pb) = (pos(g.osm_node_id(a)), pos(g.osm_node_id(b)));
        let inside =
            |p: (usize, usize)| rects.iter().any(|r| p.0 >= r.0 && p.0 <= r.2 && p.1 >= r.1 && p.1 <= r.3);
        if !(inside(pa) && inside(pb)) || rng.below(100) >= 80 {
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

/// Requêtes tirées : boucles et allers simples, trois modes, trois types, avec et sans cap, D de 1 à 6 km, départs
/// jusqu'à 400 m hors du réseau.
pub(crate) fn scenario_requests(seed: u64, n: usize) -> Vec<Request> {
    scenario_requests_sized(seed, n, 1000.0, 5000)
}

/// Comme [`scenario_requests`], avec D de `d_min` à `d_min + d_span` mètres.
pub(crate) fn scenario_requests_sized(seed: u64, n: usize, d_min: f64, d_span: usize) -> Vec<Request> {
    let mut rng = SplitMix64(seed);
    let extent = (N - 1) as f64 * CELL;
    let point = |rng: &mut SplitMix64| {
        let f = |rng: &mut SplitMix64| -400.0 + (rng.below(10_000) as f64 / 10_000.0) * (extent + 800.0);
        ll(f(rng), f(rng))
    };
    (0..n)
        .map(|_| {
            let start = point(&mut rng);
            let is_loop = rng.below(3) != 0;
            let end = (!is_loop).then(|| point(&mut rng));
            let bearings = [
                Bearing8::N,
                Bearing8::NE,
                Bearing8::E,
                Bearing8::SE,
                Bearing8::S,
                Bearing8::SW,
                Bearing8::W,
                Bearing8::NW,
            ];
            Request {
                start,
                end,
                bearing: (is_loop && rng.below(3) == 0).then(|| bearings[rng.below(8)]),
                distance_m: d_min + rng.below(d_span) as f64,
                ride_type: RideType::ALL[rng.below(3)],
                mode: [Mode::Strict, Mode::TeamFirst, Mode::Discovery][rng.below(3)],
                familiarity: rng.below(11) as f32 / 10.0,
                off_team_cost_q8: [256, 768, 1280][rng.below(3)],
                seed: rng.next_u64(),
            }
        })
        .collect()
}

pub(crate) fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
}

/// Une ligne de fixture : empreinte de la sortie complète et résumé.
pub(crate) fn snapshot(i: usize, res: &(Result<Generation, Failure>, Diagnostics)) -> String {
    let full = format!("{res:?}");
    let sum = match &res.0 {
        Ok(g) => {
            let steps: Vec<usize> = g.routes.iter().map(|r| r.steps.len()).collect();
            format!("ok routes={} steps={steps:?}", g.routes.len())
        }
        Err(f) => format!("err {f:?}"),
    };
    format!("{i} {:016x} {sum}", fnv(&full))
}

/// Les trois zooms de la référence : z11 (une tuile), z16 (grille de tuiles de ~400 m).
/// Paramètres de fenêtre réduits (marge 20 m) : une requête de quelques kilomètres a une vraie fenêtre partielle.
pub(crate) fn scenario_params() -> Params {
    Params { window_margin_m: 20.0, ..Params::default() }
}

pub(crate) const ZOOMS: [u8; 2] = [11, 16];
pub(crate) const REQUESTS: usize = 70;

pub(crate) type Runner<'a> = &'a dyn Fn(
    &Engine<Vec<u8>>,
    Option<&TeamData>,
    &Request,
    &mut Workspace,
) -> (Result<Generation, Failure>, Diagnostics);

/// Rejoue les requêtes de la référence avec `runner` et rend les lignes de fixture.
pub(crate) fn reference_lines(runner: Runner<'_>) -> Vec<String> {
    let mut out = Vec::new();
    for zoom in ZOOMS {
        let e = engine_zoomed(&mut scenario_graph(7), zoom, scenario_params());
        let team = TeamData::build(&e, &overlay_of(&e, &scenario_rows(&e, 11))).unwrap();
        let mut ws = Workspace::new();
        for (i, r) in scenario_requests(13, REQUESTS).iter().enumerate() {
            let res = runner(&e, Some(&team), r, &mut ws);
            out.push(format!("z{zoom} {}", snapshot(i, &res)));
        }
    }
    out
}

pub(crate) fn fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/window_reference.txt")
}

pub(crate) fn assert_matches_reference(runner: Runner<'_>) {
    let lines = reference_lines(runner);
    let path = fixture_path();
    if std::env::var_os("ORD_LOOPS_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).expect("fixture de référence absente (ORD_LOOPS_BLESS=1)");
    let want: Vec<&str> = want.lines().collect();
    assert_eq!(want.len(), lines.len(), "nombre de requêtes");
    let mut bad = String::new();
    for (w, g) in want.iter().zip(&lines) {
        if w != g {
            let _ = writeln!(bad, "attendu {w}\n   reçu {g}");
        }
    }
    assert!(bad.is_empty(), "écarts à la référence :\n{bad}");
}

#[test]
fn the_flat_engine_still_matches_the_frozen_reference() {
    assert_matches_reference(&|e, team, r, ws| generate_with_diagnostics(e, team, r, ws, &mut || false));
}

#[test]
fn the_reference_scenario_exercises_chains_and_failures() {
    let lines = reference_lines(&|e, team, r, ws| generate_with_diagnostics(e, team, r, ws, &mut || false));
    let ok = lines.iter().filter(|l| l.contains(" ok ")).count();
    assert!(ok * 3 >= lines.len(), "trop peu de succès ({ok}/{})", lines.len());
    assert!(lines.iter().any(|l| l.contains(" err ")), "aucun échec");
}
