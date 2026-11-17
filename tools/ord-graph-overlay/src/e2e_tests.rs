//! Tests de bout en bout sur un graphe et un export **synthétiques** (aucune donnée de production) :
//! `MemSource` → `build_graph` → `write_graph`, export gzip fabriqué, `build_cmd::run`.
//!
//! Une équipe par scénario (id = numéro de scénario), chacun dans sa bande de latitude, dans un seul graphe.

use std::io::Write;
use std::path::{Path, PathBuf};

use ord_graph::build::{build_graph, BuildOptions, MemSource};
use ord_graph::format::write_graph;
use ord_graph::mmap::MappedGraph;
use ord_graph::overlay::Overlay;
use serde_json::{json, Value};

use crate::aggregate::{EdgeAcc, TeamAcc};
use crate::build_cmd::{inspect, overlay_name, run, BuildArgs, InspectArgs};
use crate::geo::{encode_polyline6, haversine, P2};

const LON: f64 = -1.560;
/// Mètres par degré de latitude (même valeur que le repère de l'outil).
const M_PER_DEG: f64 = 111_195.08;

fn north(m: f64) -> f64 {
    m / M_PER_DEG
}

/// Latitude de base du scénario `k`.
fn lat(k: i64) -> f64 {
    47.20 + 0.003 * k as f64
}

fn nid(k: i64, i: i64) -> i64 {
    k * 100 + i
}

fn glen(g: &[P2]) -> f64 {
    g.windows(2).map(|w| haversine(w[0], w[1])).sum()
}

#[derive(Clone, Debug, Default)]
struct E {
    g: Vec<P2>,
    trv: Option<f64>,
    wk: Option<&'static str>,
    ow: Option<&'static str>,
    fm: Option<f64>,
    tm: Option<f64>,
    from_node: Option<i64>,
    to_node: Option<i64>,
    raw_g: Option<String>,
}

fn e(g: &[P2]) -> E {
    E { g: g.to_vec(), ..Default::default() }
}

struct Row {
    team: u64,
    ty: &'static str,
    excluded: bool,
    edges: Vec<E>,
}

fn row(team: u64, ty: &'static str, edges: Vec<E>) -> Row {
    Row { team, ty, excluded: false, edges }
}

fn csv_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Le document `network_edges` d'une ligne de l'export.
fn doc_json(r: &Row) -> String {
    let edges: Vec<Value> = r
        .edges
        .iter()
        .map(|x| {
            let g = x.raw_g.clone().unwrap_or_else(|| encode_polyline6(&x.g));
            let mut o = json!({"g": g, "zz": {"inconnue": [1, 2]}});
            let trv = x.trv.unwrap_or_else(|| glen(&x.g));
            o["trv"] = json!(trv);
            for (k, v) in [("fm", x.fm), ("tm", x.tm)] {
                if let Some(v) = v {
                    o[k] = json!(v);
                }
            }
            for (k, v) in [("fn", x.from_node), ("tn", x.to_node)] {
                if let Some(v) = v {
                    o[k] = json!(v);
                }
            }
            if let Some(w) = x.wk {
                o["wk"] = json!(w);
            }
            if let Some(w) = x.ow {
                o["ow"] = json!(w);
            }
            o
        })
        .collect();
    json!({"v": 1, "api": "1.8", "edges": edges}).to_string()
}

fn write_export(path: &Path, rows: &[Row]) {
    let mut text = String::from(
        "team_id,enable_route_planner,track_id,route_id,surface_type,excluded_from_generator,distance,rev,network_edges\n",
    );
    for (i, r) in rows.iter().enumerate() {
        let doc = doc_json(r);
        text.push_str(&format!(
            "{},t,{},{},{},{},1.0,7,{}\n",
            r.team,
            1000 + i,
            2000 + i,
            r.ty,
            if r.excluded { "t" } else { "f" },
            csv_quote(&doc)
        ));
    }
    let mut gz =
        flate2::write::GzEncoder::new(std::fs::File::create(path).unwrap(), flate2::Compression::fast());
    gz.write_all(text.as_bytes()).unwrap();
    gz.finish().unwrap();
}

fn write_poly(path: &Path) {
    // Lon -1,60 à -1,55 ; lat 47,0 à 47,4.
    std::fs::write(
        path,
        "t\n1\n   -1.60 47.0\n   -1.55 47.0\n   -1.55 47.4\n   -1.60 47.4\n   -1.60 47.0\nEND\nEND\n",
    )
    .unwrap();
}

fn write_graph_file(path: &Path, src: &mut MemSource) {
    let mut data = build_graph(src, &BuildOptions { zoom: 11 }, "test", &mut |_| {}).unwrap();
    write_graph(path, &mut data, "test", 1_700_000_000).unwrap();
}

/// Le graphe de tous les scénarios.
fn scenario_graph() -> MemSource {
    let mut s = MemSource::default();
    let res = [("highway", "residential")];
    // 1 : route A-B-C coupée en B par une allée.
    let l = lat(1);
    s.node(nid(1, 1), l, LON).node(nid(1, 2), l, LON + 0.001).node(nid(1, 3), l, LON + 0.002);
    s.node(nid(1, 4), l + 0.0005, LON + 0.001);
    s.way(101, &[nid(1, 1), nid(1, 2), nid(1, 3)], &res).way(
        102,
        &[nid(1, 2), nid(1, 4)],
        &[("highway", "service")],
    );
    // 2 : route et piste cyclable parallèles à 4,4 m (nœuds distincts) ; deux voies à 0,3 m entre les mêmes nœuds.
    let l = lat(2);
    s.node(nid(2, 1), l, LON).node(nid(2, 2), l, LON + 0.002);
    s.node(nid(2, 3), l + north(4.4), LON).node(nid(2, 4), l + north(4.4), LON + 0.002);
    s.way(201, &[nid(2, 1), nid(2, 2)], &res).way(202, &[nid(2, 3), nid(2, 4)], &[("highway", "cycleway")]);
    s.node(nid(2, 5), l + 0.001, LON).node(nid(2, 6), l + 0.001, LON + 0.002);
    s.node(nid(2, 7), l + 0.001 + north(0.3), LON + 0.001);
    s.way(203, &[nid(2, 5), nid(2, 6)], &res).way(204, &[nid(2, 5), nid(2, 7), nid(2, 6)], &res);
    // 3 : sens unique ; sens unique avec `oneway:bicycle=no`.
    let l = lat(3);
    s.node(nid(3, 1), l, LON).node(nid(3, 2), l, LON + 0.002);
    s.way(301, &[nid(3, 1), nid(3, 2)], &[("highway", "residential"), ("oneway", "yes")]);
    s.node(nid(3, 3), l + 0.001, LON).node(nid(3, 4), l + 0.001, LON + 0.002);
    s.way(
        302,
        &[nid(3, 3), nid(3, 4)],
        &[("highway", "residential"), ("oneway", "yes"), ("oneway:bicycle", "no")],
    );
    // 4 : une arête de 200 m, du sud au nord.
    let l = lat(4);
    s.node(nid(4, 1), l, LON).node(nid(4, 2), l + north(200.0), LON);
    s.way(401, &[nid(4, 1), nid(4, 2)], &res);
    // 5 : lien absent (3,3 m entre deux nœuds) ; piéton alors que l'équipe dit route.
    let l = lat(5);
    s.node(nid(5, 1), l, LON).node(nid(5, 2), l, LON + 0.001);
    s.node(nid(5, 3), l + north(3.3), LON + 0.001).node(nid(5, 4), l, LON + 0.002);
    s.way(501, &[nid(5, 1), nid(5, 2)], &res).way(502, &[nid(5, 3), nid(5, 4)], &res);
    s.node(nid(5, 5), l + 0.001, LON).node(nid(5, 6), l + 0.001, LON + 0.002);
    s.way(503, &[nid(5, 5), nid(5, 6)], &[("highway", "footway")]);
    // 6 : continuité, chaînes contiguës adjacentes puis séparées de 3,3 m.
    let l = lat(6);
    s.node(nid(6, 1), l, LON).node(nid(6, 2), l, LON + 0.001).node(nid(6, 3), l, LON + 0.002);
    s.node(nid(6, 4), l + 0.0005, LON + 0.001);
    s.way(601, &[nid(6, 1), nid(6, 2), nid(6, 3)], &res).way(
        602,
        &[nid(6, 2), nid(6, 4)],
        &[("highway", "service")],
    );
    let l2 = l + 0.001;
    s.node(nid(6, 11), l2, LON).node(nid(6, 12), l2, LON + 0.001);
    s.node(nid(6, 13), l2 + north(3.3), LON + 0.001).node(nid(6, 14), l2, LON + 0.002);
    s.way(611, &[nid(6, 11), nid(6, 12)], &res).way(612, &[nid(6, 13), nid(6, 14)], &res);
    s
}

fn all_rows() -> Vec<Row> {
    let mut rows = Vec::new();
    // Équipe 1 : tolérances.
    let l = lat(1);
    let ac = [[l, LON], [l, LON + 0.002]];
    rows.push(row(1, "ROAD", vec![e(&ac)]));
    let shifted = [[l + north(0.4), LON], [l + north(0.4), LON + 0.002]];
    rows.push(row(1, "ROAD", vec![e(&shifted)]));
    let far = [[l + north(7.0), LON], [l + north(7.0), LON + 0.002]];
    rows.push(row(1, "ROAD", vec![e(&far)]));
    // Équipe 2 : parallèles.
    let l = lat(2);
    rows.push(row(2, "ROAD", vec![e(&[[l, LON], [l, LON + 0.002]])]));
    let mut cyc = e(&[[l + north(4.4), LON], [l + north(4.4), LON + 0.002]]);
    cyc.wk = Some("cycleway");
    rows.push(row(2, "ROAD", vec![cyc]));
    let amb = l + 0.001 + north(0.15);
    rows.push(row(2, "ROAD", vec![e(&[[amb, LON], [amb, LON + 0.002]])]));
    // Équipe 3 : sens uniques, parcourus à contresens (ow = against).
    let l = lat(3);
    let mut closed = e(&[[l, LON + 0.002], [l, LON]]);
    closed.ow = Some("against");
    rows.push(row(3, "ROAD", vec![closed]));
    let mut open = e(&[[l + 0.001, LON + 0.002], [l + 0.001, LON]]);
    open.ow = Some("against");
    rows.push(row(3, "ROAD", vec![open]));
    // Équipe 4 : arêtes partielles, demi-tour, doubles passages, types, exclu.
    let l = lat(4);
    let seg = |a: f64, b: f64| e(&[[l + north(a), LON], [l + north(b), LON]]);
    rows.push(row(4, "ROAD", vec![seg(20.0, 50.0)])); // 30 m : sous le seuil
    rows.push(row(4, "ROAD", vec![seg(10.0, 160.0)])); // 150 m : comptée
    rows.push(row(4, "GRAVEL", vec![seg(0.0, 200.0)]));
    rows.push(row(4, "ROAD", vec![seg(10.0, 160.0), seg(160.0, 10.0)])); // demi-tour
    rows.push(row(4, "ROAD", vec![seg(0.0, 200.0), seg(0.0, 200.0)])); // deux passages
    let mut excl = row(4, "ROAD", vec![seg(0.0, 200.0)]);
    excl.excluded = true;
    rows.push(excl);
    // Équipe 5 : causes.
    let l = lat(5);
    rows.push(row(5, "ROAD", vec![e(&[[l, LON], [l, LON + 0.002]])])); // chaîne non reliée
    let mut wk = e(&[[l + 0.001, LON], [l + 0.001, LON + 0.002]]);
    wk.wk = Some("major_road");
    rows.push(row(5, "ROAD", vec![wk])); // piéton pour une route
    rows.push(row(5, "ROAD", vec![e(&[[l, -1.540], [l, -1.530]])])); // hors région
    rows.push(row(5, "ROAD", vec![e(&[[l + 0.002, LON], [l + 0.002, -1.540]])])); // à cheval
    let mut empty = e(&[]);
    empty.raw_g = Some(String::new());
    empty.trv = Some(100.0);
    let mut garbage = e(&[]);
    garbage.raw_g = Some("\u{1}\u{2}".into());
    garbage.trv = Some(50.0);
    rows.push(row(5, "ROAD", vec![empty, garbage])); // illisibles
                                                     // Équipe 6 : continuité.
    let l = lat(6);
    let mut e1 = e(&[[l, LON], [l, LON + 0.001]]);
    (e1.fm, e1.tm, e1.from_node, e1.to_node) = (Some(0.0), Some(75.6), Some(0), Some(1));
    let mut e2 = e(&[[l, LON + 0.001], [l, LON + 0.002]]);
    (e2.fm, e2.tm, e2.from_node, e2.to_node) = (Some(75.6), Some(151.2), Some(1), Some(2));
    rows.push(row(6, "ROAD", vec![e1, e2]));
    let l2 = l + 0.001;
    let mut g1 = e(&[[l2, LON], [l2, LON + 0.001]]);
    (g1.fm, g1.tm) = (Some(0.0), Some(75.6));
    let mut g2 = e(&[[l2 + north(3.3), LON + 0.001], [l2, LON + 0.002]]);
    (g2.fm, g2.tm) = (Some(75.6), Some(151.2));
    rows.push(row(6, "ROAD", vec![g1, g2]));
    rows
}

struct Fx {
    dir: tempfile::TempDir,
    graph: PathBuf,
    poly: PathBuf,
}

fn fixture(rows: &[Row]) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let graph = dir.path().join("test-z11.ordg");
    write_graph_file(&graph, &mut scenario_graph());
    let poly = dir.path().join("region.poly");
    write_poly(&poly);
    write_export(&dir.path().join("export.csv.gz"), rows);
    Fx { dir, graph, poly }
}

fn args(fx: &Fx, out: &str, threads: usize) -> BuildArgs {
    BuildArgs {
        graph: fx.graph.clone(),
        export: fx.dir.path().join("export.csv.gz"),
        poly: fx.poly.clone(),
        out_dir: fx.dir.path().join(out),
        report: fx.dir.path().join(format!("{out}.json")),
        teams: vec![],
        threads: Some(threads),
        verify_sample: 50,
        seed: 20_261_115,
    }
}

fn edge_of(gf: &MappedGraph, way: u64, a: i64, b: i64) -> u32 {
    (0..gf.header().edges as u32)
        .find(|&x| gf.identity(x) == (way, a as u64, b as u64))
        .unwrap_or_else(|| panic!("arête ({way},{a},{b}) absente"))
}

fn overlay_of(fx: &Fx, out: &str, team: u64) -> Overlay {
    Overlay::read(&fx.dir.path().join(out).join(overlay_name(team, &fx.graph))).unwrap()
}

fn km(report: &Value, team: u64, path: &[&str]) -> f64 {
    let mut v = &report["teams"][team.to_string()]["km"];
    for p in path {
        v = &v[*p];
    }
    v.as_f64().unwrap_or_else(|| panic!("km {path:?} absent pour l'équipe {team}"))
}

fn cause(report: &Value, team: u64, name: &str) -> f64 {
    km(report, team, &["unmatched_by_cause", name])
}

fn near(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "{a} != {b} (±{tol})");
}

#[test]
fn every_scenario_end_to_end() {
    let rows = all_rows();
    let fx = fixture(&rows);
    let out = run(&args(&fx, "o1", 4)).unwrap();
    let rep = &out.report;
    let gf = MappedGraph::open_mmap(&fx.graph).unwrap();
    let trv = |k: usize, i: usize| glen(&rows[k].edges[i].g) / 1000.0;

    // 1. Route A-B-C coupée en B : chaîne [AB, BC] ; décalée de 0,4 m et simplifiée : appariée ; à 7 m : sans arête.
    near(km(rep, 1, &["matched"]), 2.0 * trv(0, 0), 6e-4);
    near(cause(rep, 1, "no_osm_edge"), trv(2, 0), 6e-4);
    let o = overlay_of(&fx, "o1", 1);
    let (ab, bc) = (edge_of(&gf, 101, nid(1, 1), nid(1, 2)), edge_of(&gf, 101, nid(1, 2), nid(1, 3)));
    assert_eq!(o.len(), 2);
    assert_eq!(o.get(ab), Some(&[2u16, 0, 0, 0, 0, 0, 0, 0][..]));
    assert_eq!(o.get(bc), Some(&[2u16, 0, 0, 0, 0, 0, 0, 0][..]));
    assert_eq!(km(rep, 1, &["matched_share_of_in_region"]), 0.667);

    // 2. Parallèles : route -> route, piste -> piste ; à 0,3 m des deux : ambiguë, hors surcouche.
    let o = overlay_of(&fx, "o1", 2);
    let (road, cycle) = (edge_of(&gf, 201, nid(2, 1), nid(2, 2)), edge_of(&gf, 202, nid(2, 3), nid(2, 4)));
    assert_eq!(o.len(), 2, "ni la voie ambiguë ni son jumeau n'entrent");
    assert_eq!(o.get(road).map(|c| c[0]), Some(1));
    assert_eq!(o.get(cycle).map(|c| c[0]), Some(1));
    near(cause(rep, 2, "ambiguous_parallel"), trv(5, 0), 6e-4);

    // 3. Sens unique remonté : fermé ; avec oneway:bicycle=no : apparié, sens arrière compté.
    near(cause(rep, 3, "direction_closed"), trv(6, 0), 6e-4);
    near(km(rep, 3, &["direction_closed_by_oneway", "against"]), trv(6, 0), 6e-4);
    near(km(rep, 3, &["matched"]), trv(7, 0), 6e-4);
    let o = overlay_of(&fx, "o1", 3);
    assert_eq!(o.len(), 1);
    assert_eq!(o.get(edge_of(&gf, 302, nid(3, 3), nid(3, 4))), Some(&[0u16, 1, 0, 0, 0, 0, 0, 0][..]));

    // 4. Arêtes partielles, demi-tour, doubles passages, types, exclu.
    let o = overlay_of(&fx, "o1", 4);
    assert_eq!(o.len(), 1);
    let e4 = edge_of(&gf, 401, nid(4, 1), nid(4, 2));
    // ROAD : 150 m + demi-tour (aller) + deux passages = 3 en avant ; demi-tour : 1 en arrière ; GRAVEL : 1 en avant.
    assert_eq!(o.get(e4), Some(&[3u16, 1, 1, 0, 0, 0, 0, 0][..]));
    near(km(rep, 4, &["below_threshold"]), 0.030, 0.002);
    assert_eq!(rep["teams"]["4"]["rows"]["excluded"], 1);
    assert_eq!(rep["teams"]["4"]["overlay"]["routes_per_edge"]["distribution"]["max"], 4.0);

    // 5. Causes.
    near(cause(rep, 5, "chain_not_connected"), glen(&rows[14].edges[0].g) / 1000.0, 6e-4);
    near(cause(rep, 5, "way_kind"), glen(&rows[15].edges[0].g) / 1000.0, 6e-4);
    near(km(rep, 5, &["outside_region"]), glen(&rows[16].edges[0].g) / 1000.0, 6e-4);
    near(cause(rep, 5, "border"), glen(&rows[17].edges[0].g) / 1000.0, 6e-4);
    near(cause(rep, 5, "invalid"), 0.150, 6e-4);
    let t5 = &rep["teams"]["5"]["tracks"];
    assert_eq!(
        (&t5["in_region"], &t5["out_of_region"], &t5["partly_in_region"]),
        (&json!(3), &json!(1), &json!(1))
    );
    assert_eq!(rep["teams"]["5"]["overlay"]["base_edges"], 0);

    // 6. Continuité : paire adjacente, puis paire séparée de 3,3 m, détectée.
    let c = &rep["teams"]["6"]["checks"]["continuity"];
    assert_eq!((&c["pairs"], &c["adjacent"], &c["same_edge_share"]), (&json!(2), &json!(1), &json!(0.0)));
    near(c["gaps_m"]["max"].as_f64().unwrap(), 3.3, 0.2);
    assert_eq!(c["internal_gaps"], 0);

    // Contrôle indépendant : Hausdorff / Fréchet de l'échantillon sous le seuil d'alerte.
    let v = &rep["overall"]["checks"]["verification"];
    assert!(v["sample"].as_u64().unwrap() > 0);
    assert_eq!(v["alert_p99_over_1m"], false);
    assert!(v["hausdorff_m"]["max"].as_f64().unwrap() < 1.0);
    // Aucune coordonnée, aucun id de parcours dans le rapport.
    let text = serde_json::to_string(rep).unwrap();
    assert!(
        !text.contains("track_id")
            && !text.contains("\"lat\"")
            && !text.contains("\"lon\"")
            && !text.contains("coordinates")
    );

    // Fichier du rapport écrit, surcouches en lecture seule, liées à ce graphe.
    assert!(fx.dir.path().join("o1.json").exists());
    let path = fx.dir.path().join("o1").join(overlay_name(4, &fx.graph));
    assert!(std::fs::metadata(&path).unwrap().permissions().readonly());
    let info = inspect(&InspectArgs { overlay: path, graph: fx.graph.clone() }).unwrap();
    assert_eq!(info["entries"], 1);
    // Aucune surcouche ne porte un sens fermé au vélo (relu du fichier contre les attributs du graphe).
    for team in 1..=6 {
        let path = fx.dir.path().join("o1").join(overlay_name(team, &fx.graph));
        let info = inspect(&InspectArgs { overlay: path, graph: fx.graph.clone() }).unwrap();
        assert_eq!(info["closed_direction_entries"], 0, "équipe {team}");
    }
}

#[test]
fn same_result_with_one_and_four_threads() {
    let fx = fixture(&all_rows());
    let (a, b) = (run(&args(&fx, "t1", 1)).unwrap(), run(&args(&fx, "t4", 4)).unwrap());
    assert_eq!(a.report["teams"], b.report["teams"]);
    assert_eq!(a.report["overall"], b.report["overall"]);
    for team in 1..=6 {
        let (x, y) = (overlay_of(&fx, "t1", team), overlay_of(&fx, "t4", team));
        assert_eq!(x.iter().collect::<Vec<_>>(), y.iter().collect::<Vec<_>>());
        assert_eq!(x.meta.params_hash, y.meta.params_hash);
    }
}

#[test]
fn overlay_of_another_graph_is_refused() {
    let fx = fixture(&all_rows());
    run(&args(&fx, "o", 2)).unwrap();
    let path = fx.dir.path().join("o").join(overlay_name(1, &fx.graph));
    // Même région, une voie de plus : les ids d'arête ne désignent plus les mêmes arêtes.
    let mut other = scenario_graph();
    other.node(9_001, 47.1, -1.58).node(9_002, 47.1, -1.579).way(
        9_901,
        &[9_001, 9_002],
        &[("highway", "residential")],
    );
    let other_path = fx.dir.path().join("autre-z11.ordg");
    write_graph_file(&other_path, &mut other);
    let err = inspect(&InspectArgs { overlay: path, graph: other_path }).unwrap_err();
    assert!(format!("{err:#}").contains("autre graphe"), "{err:#}");
}

#[test]
fn counts_saturate_at_65535_in_the_file() {
    let mut acc = TeamAcc::default();
    acc.counts.insert(5, EdgeAcc { counts: [70_000, 0, 3, 0, 0, 0, 0, 0], any: 70_000 });
    acc.counts.insert(2, EdgeAcc { counts: [0, 1, 0, 0, 0, 0, 0, 0], any: 1 });
    let (edges, counts) = crate::build_cmd::overlay_arrays(&acc);
    assert_eq!(edges, vec![2, 5]);
    assert_eq!(&counts[..8], &[0, 1, 0, 0, 0, 0, 0, 0]);
    assert_eq!(&counts[8..], &[65_535, 0, 3, 0, 0, 0, 0, 0]);
}

/// Le chemin du service (`Deposit`) donne la surcouche de l'outil : mêmes arêtes, mêmes comptes, avec l'index entier
/// comme avec l'index par cellule, et avec une mémoïsation minuscule (les évictions ne changent aucun résultat).
#[test]
fn the_service_deposit_gives_the_tool_overlay() {
    use ord_graph::util::Poly;
    use ord_graph_match::doc::{parse_document, type_code};
    use ord_graph_match::job::{match_job, Job};
    use ord_graph_match::{CellIndex, Deposit, DepositStats, Near, SegIndex};

    let rows = all_rows();
    let fx = fixture(&rows);
    run(&args(&fx, "o1", 2)).unwrap();
    let gf = MappedGraph::open_mmap(&fx.graph).unwrap();
    let poly = Poly::read(&fx.poly).unwrap();
    assert!(CellIndex::fits(&gf));
    let full = SegIndex::build(&gf);
    let cell = CellIndex::new(&gf, 1 << 20);
    let tiny_cell = CellIndex::new(&gf, 1);
    let mut teams: Vec<u64> = rows.iter().map(|r| r.team).collect();
    teams.sort_unstable();
    teams.dedup();
    for team in teams {
        let tool = overlay_of(&fx, "o1", team);
        let want_edges: Vec<u32> = tool.iter().map(|(e, _)| e).collect();
        let want_counts: Vec<u16> = tool.iter().flat_map(|(_, c)| c.iter().copied()).collect();
        for (name, idx, memo) in [
            ("index entier", &full as &dyn Near, 1000usize),
            ("index par cellule", &cell, 1000),
            ("index par cellule évincé", &tiny_cell, 1000),
            ("mémo minuscule", &cell, 2),
        ] {
            let mut dep = Deposit::new(&gf, idx, &poly, memo);
            for r in rows.iter().filter(|r| r.team == team) {
                dep.push_track(type_code(r.ty), r.excluded, parse_document(doc_json(r).as_bytes()));
            }
            let (edges, counts, stats) = dep.finish();
            assert_eq!(edges, want_edges, "équipe {team}, {name} : arêtes");
            assert_eq!(counts, want_counts, "équipe {team}, {name} : comptes");
            assert_eq!(stats.tracks, rows.iter().filter(|r| r.team == team).count() as u64);
        }
        // Appariements précalculés (`match_job`, pure) puis `push_matched` : mêmes arêtes et mêmes comptes, et les
        // mêmes statistiques que `push_track` (hors compteurs de mémoïsation).
        let mut reference = Deposit::new(&gf, &full, &poly, 1000);
        let mut matched = Deposit::new(&gf, &full, &poly, 1000);
        for r in rows.iter().filter(|r| r.team == team) {
            let doc = || parse_document(doc_json(r).as_bytes());
            reference.push_track(type_code(r.ty), r.excluded, doc());
            let jobs: Option<Vec<(u8, f32, Job)>> = doc().map(|edges| {
                edges.iter().map(|e| (e.wk, e.trv, match_job(&gf, &full, &poly, &e.g, e.trv))).collect()
            });
            matched.push_matched(
                type_code(r.ty),
                r.excluded,
                jobs.as_ref().map(|v| v.iter().map(|(wk, trv, job)| (*wk, *trv, job))),
            );
        }
        let (edges, counts, stats) = matched.finish();
        let (ref_edges, ref_counts, ref_stats) = reference.finish();
        assert_eq!(edges, want_edges, "équipe {team}, appariements précalculés : arêtes");
        assert_eq!(counts, want_counts, "équipe {team}, appariements précalculés : comptes");
        assert_eq!((edges, counts), (ref_edges, ref_counts));
        let zero_memo = |s: DepositStats| DepositStats { memo_hits: 0, memo_misses: 0, ..s };
        assert_eq!(zero_memo(stats), zero_memo(ref_stats), "équipe {team} : statistiques");
    }
}
