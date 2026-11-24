//! Support des tests du générateur : graphe, vues et polygone **synthétiques** écrits dans un dossier temporaire,
//! documents `network_edges` fabriqués. Aucune donnée de production.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::{Body, Bytes};
use axum::http::{header, Method, Request};
use ord_graph::build::{build_graph, BuildOptions, MemSource};
use ord_graph::format::{write_graph, GraphFile};
use ord_graph::view::{build_view, ViewSpec};
use ord_graph_match::geo::{encode_polyline6, haversine};
use ord_server::config::Config;
use ord_server::generator::crypto::encode_key;
use serde_json::{json, Value};

pub const LAT0: f64 = 47.2;
pub const LON0: f64 = -1.56;
pub const M_LAT: f64 = 111_194.926_6;
pub const STEP: f64 = 300.0;

fn m_lon() -> f64 {
    M_LAT * LAT0.to_radians().cos()
}

/// Position du carrefour `(r, c)` de la grille, en `(lat, lon)`.
pub fn node(r: usize, c: usize) -> [f64; 2] {
    [LAT0 + r as f64 * STEP / M_LAT, LON0 + c as f64 * STEP / m_lon()]
}

/// Grille de `n × n` carrefours, rues résidentielles de 300 m.
pub fn grid(n: usize) -> MemSource {
    let mut s = MemSource::default();
    let id = |r: usize, c: usize| (r * 100 + c + 1) as i64;
    for r in 0..n {
        for c in 0..n {
            let p = node(r, c);
            s.node(id(r, c), p[0], p[1]);
        }
    }
    let mut way = 1;
    for r in 0..n {
        for c in 0..n {
            for (dr, dc) in [(0, 1), (1, 0)] {
                if r + dr < n && c + dc < n {
                    s.way(way, &[id(r, c), id(r + dr, c + dc)], &[("highway", "residential")]);
                    way += 1;
                }
            }
        }
    }
    s
}

fn write(data: &mut ord_graph::build::GraphData, path: &Path) {
    write_graph(path, data, "test", 1_700_000_000).expect("écriture");
}

/// Les quatre fichiers d'un graphe synthétique.
pub struct Files {
    pub graph: PathBuf,
    pub road: PathBuf,
    pub gravel: PathBuf,
    pub poly: PathBuf,
}

/// Écrit le graphe (`n × n`), ses vues `road` et `gravel`, et un polygone qui l'entoure à 1 km.
pub fn write_files(dir: &Path, name: &str, n: usize) -> Files {
    let mut data =
        build_graph(&mut grid(n), &BuildOptions { zoom: 11 }, "test", &mut |_| {}).expect("graphe");
    let graph = dir.join(format!("{name}.ordg"));
    write(&mut data, &graph);
    let base = GraphFile::open(std::fs::read(&graph).unwrap()).unwrap();
    let mut files = Vec::new();
    for view in ["road", "gravel"] {
        let mut v = build_view(&base, &ViewSpec::parse(view).unwrap()).expect("vue");
        let p = dir.join(format!("{name}.{view}.ordv"));
        write(&mut v, &p);
        files.push(p);
    }
    let (sw, ne) = (node(0, 0), node(n - 1, n - 1));
    let (dlat, dlon) = (1000.0 / M_LAT, 1000.0 / m_lon());
    let poly = dir.join(format!("{name}.poly"));
    std::fs::write(
        &poly,
        format!(
            "t\n1\n {w} {s}\n {e} {s}\n {e} {nn}\n {w} {nn}\n {w} {s}\nEND\nEND\n",
            w = sw[1] - dlon,
            e = ne[1] + dlon,
            s = sw[0] - dlat,
            nn = ne[0] + dlat
        ),
    )
    .unwrap();
    Files { graph, road: files.remove(0), gravel: files.remove(0), poly }
}

/// Configuration de test avec le générateur branché sur ces fichiers ; stockage dans `dir/overlays`.
pub fn config_with(base: Config, dir: &Path, files: &Files) -> Config {
    let mut c = base;
    let g = &mut c.generator;
    g.graph = Some(files.graph.clone());
    g.view_road = Some(files.road.clone());
    g.view_gravel = Some(files.gravel.clone());
    g.coverage_poly = Some(files.poly.clone());
    g.overlay_dir = dir.join("overlays");
    g.distance_m_min = 1_000.0;
    g.deposit.idle_timeout_s = 2;
    c
}

/// La clé d'environnement numéro `n` (déterministe), en base64url.
pub fn key(n: u8) -> String {
    let mut k = [0u8; 32];
    for (i, b) in k.iter_mut().enumerate() {
        *b = n.wrapping_mul(37).wrapping_add(i as u8).wrapping_add(1);
    }
    encode_key(&k)
}

/// Un document `network_edges` : les lignes `rows` et les colonnes `cols` de la grille, en morceaux de 3 arêtes.
pub fn document(n: usize, rows: &[usize], cols: &[usize]) -> Value {
    let mut edges = Vec::new();
    let mut push = |pts: Vec<[f64; 2]>| {
        let trv: f64 = pts.windows(2).map(|w| haversine(w[0], w[1])).sum();
        edges
            .push(json!({"g": encode_polyline6(&pts), "trv": trv, "wk": "minor_road", "inconnue": {"a": 1}}));
    };
    for &r in rows {
        for c0 in (0..n - 1).step_by(3) {
            let c1 = (c0 + 3).min(n - 1);
            push((c0..=c1).map(|c| node(r, c)).collect());
        }
    }
    for &c in cols {
        for r0 in (0..n - 1).step_by(3) {
            let r1 = (r0 + 3).min(n - 1);
            push((r0..=r1).map(|r| node(r, c)).collect());
        }
    }
    json!({"v": 1, "api": "1.8", "edges": edges})
}

/// Une ligne de trace du dépôt.
pub fn track_line(ty: &str, excluded: bool, doc: &Value) -> String {
    json!({"type": ty, "excluded": excluded, "network_edges": doc}).to_string()
}

/// Le corps NDJSON d'un dépôt : des lignes, puis la ligne de fin si `end` est donné.
pub fn ndjson(lines: &[String], end: Option<u64>) -> String {
    let mut out = String::new();
    for l in lines {
        out.push_str(l);
        out.push('\n');
    }
    if let Some(n) = end {
        out.push_str(&json!({"end": {"tracks": n}}).to_string());
        out.push('\n');
    }
    out
}

/// Le dépôt d'une équipe qui roule les rues et les colonnes 6, 8, 10, 12 et 14 d'une grille de 21.
pub fn team_deposit() -> String {
    let doc = document(21, &[6, 8, 10, 12, 14], &[6, 8, 10, 12, 14]);
    ndjson(&[track_line("ROAD", false, &doc), track_line("ROAD", false, &doc)], Some(2))
}

pub fn put(key: &str, team: &str, body: impl Into<Body>) -> Request<Body> {
    Request::builder()
        .method(Method::PUT)
        .uri("/v1/generator/overlay")
        .header("ord-namespace-key", key)
        .header("ord-team", team)
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .body(body.into())
        .unwrap()
}

pub fn put_digest(key: &str, team: &str, digest: &str, body: impl Into<Body>) -> Request<Body> {
    let mut r = put(key, team, body);
    r.headers_mut().insert("ord-source-digest", digest.parse().unwrap());
    r
}

fn with_keys(method: Method, uri: &str, key: &str, team: Option<&str>) -> Request<Body> {
    let mut b = Request::builder().method(method).uri(uri).header("ord-namespace-key", key);
    if let Some(t) = team {
        b = b.header("ord-team", t);
    }
    b.body(Body::empty()).unwrap()
}

pub fn status(key: &str, team: &str) -> Request<Body> {
    with_keys(Method::GET, "/v1/generator/overlay", key, Some(team))
}

pub fn delete(key: &str, team: &str) -> Request<Body> {
    with_keys(Method::DELETE, "/v1/generator/overlay", key, Some(team))
}

pub fn delete_all(key: &str) -> Request<Body> {
    with_keys(Method::DELETE, "/v1/generator/overlays", key, None)
}

/// Demande de génération au centre de la grille (21 × 21), D = 3 km.
pub fn generate_body(cursor: f64) -> Value {
    let p = node(10, 10);
    json!({"start": {"lat": p[0], "lon": p[1]}, "distance_m": 3600.0, "ride_type": "road", "cursor": cursor, "seed": 1})
}

pub fn generate(key: Option<&str>, team: Option<&str>, body: &Value) -> Request<Body> {
    let mut b = Request::builder()
        .method(Method::POST)
        .uri("/v1/generator/routes")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(k) = key {
        b = b.header("ord-namespace-key", k);
    }
    if let Some(t) = team {
        b = b.header("ord-team", t);
    }
    b.body(Body::from(body.to_string())).unwrap()
}

/// Corps fait de trames fixes, puis d'une erreur de flux ou d'un silence qui ne finit pas.
pub struct Frames {
    frames: VecDeque<Result<Bytes, std::io::Error>>,
    stall: bool,
    polls: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
}

impl Frames {
    pub fn new(chunks: Vec<Vec<u8>>) -> Self {
        Self { frames: chunks.into_iter().map(|c| Ok(Bytes::from(c))).collect(), stall: false, polls: None }
    }

    /// Après les trames, le flux casse.
    pub fn then_error(mut self) -> Self {
        self.frames.push_back(Err(std::io::Error::other("flux coupé")));
        self
    }

    /// Après les trames, plus rien n'arrive et le flux ne se ferme pas.
    pub fn then_stall(mut self) -> Self {
        self.stall = true;
        self
    }

    /// Compte les lectures du corps par le serveur (une lecture, c'est ce qui lui fait répondre « 100 Continue »).
    pub fn counting(mut self, polls: std::sync::Arc<std::sync::atomic::AtomicUsize>) -> Self {
        self.polls = Some(polls);
        self
    }

    pub fn body(self) -> Body {
        Body::new(self)
    }
}

impl http_body::Body for Frames {
    type Data = Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Bytes>, Self::Error>>> {
        if let Some(p) = &self.polls {
            p.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        match self.frames.pop_front() {
            Some(r) => Poll::Ready(Some(r.map(http_body::Frame::data))),
            None if self.stall => Poll::Pending,
            None => Poll::Ready(None),
        }
    }
}

/// Un JSON de réponse sans son `instance` (différent à chaque requête).
pub fn without_instance(text: &str) -> Value {
    let mut v: Value = serde_json::from_str(text).unwrap();
    v.as_object_mut().unwrap().remove("instance");
    v
}

/// Tous les fichiers sous un dossier (chemins relatifs).
pub fn tree(dir: &Path) -> Vec<String> {
    fn walk(d: &Path, root: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, root, out);
            } else {
                out.push(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// Écrit le fichier d'index de service (`<graphe>.ordi`) de ces fichiers, par les fonctions du démarrage du moteur, au
/// seuil de composantes des réglages par défaut.
pub fn write_service_index(files: &Files, out: &Path) {
    use ord_graph::service_index::{node_sample, write_index, IndexData, ViewTables};
    let open = |p: &Path| ord_graph::mmap::MappedGraph::open_mmap(p).unwrap();
    let (base, road, gravel) = (open(&files.graph), open(&files.road), open(&files.gravel));
    let cap_dm = ord_loops::cap_dm_of(&ord_loops::Params::default());
    let tables = [&road, &gravel].map(|v| {
        let (short, lmax_dm) = ord_loops::short_components(v, cap_dm);
        ViewTables {
            foreign: ord_loops::foreign_index(v, 2),
            short: short.into_iter().map(|(c, l)| (c, l as u32)).collect(),
            lmax_dm,
        }
    });
    let (node_bbox, node_sample) = node_sample(&base);
    let data = IndexData {
        views: tables,
        cap_dm: cap_dm as u32,
        wide_seg: ord_graph_match::CellIndex::exceptions(&base),
        wide_end: ord_loops_check::GraphIndex::build(&base).wide_edges().to_vec(),
        node_bbox,
        node_sample,
    };
    write_index(out, &base, [&road, &gravel], &data, b"{}", 0).expect("index de service");
}

/// Les positions d'un chemin de carrefours `(r, c)` de la grille.
pub fn path_points(path: &[(usize, usize)]) -> Vec<[f64; 2]> {
    path.iter().map(|&(r, c)| node(r, c)).collect()
}

/// Le document `network_edges` d'un chemin de carrefours : une arête par suite droite de 3 rues au plus, avec `fm` et `tm`
/// cumulés le long du chemin et `len` (la longueur de l'axe), comme le garde Pédalons.
pub fn path_document(path: &[(usize, usize)]) -> Value {
    let pts = path_points(path);
    let dir = |i: usize| (path[i + 1].0 as i64 - path[i].0 as i64, path[i + 1].1 as i64 - path[i].1 as i64);
    // Les suites droites de 3 rues au plus, en indices de carrefours.
    let mut runs: Vec<Vec<usize>> = Vec::new();
    for i in 0..path.len() - 1 {
        match runs.last_mut() {
            Some(r) if r.len() < 4 && dir(r[r.len() - 2]) == dir(i) => r.push(i + 1),
            _ => runs.push(vec![i, i + 1]),
        }
    }
    let mut edges = Vec::new();
    let mut at = 0.0f64;
    for run in &runs {
        let line: Vec<[f64; 2]> = run.iter().map(|&i| pts[i]).collect();
        let trv: f64 = line.windows(2).map(|w| haversine(w[0], w[1])).sum();
        edges.push(json!({"g": encode_polyline6(&line), "trv": trv, "fm": at, "tm": at + trv,
            "wk": "minor_road", "inconnue": {"a": 1}}));
        at += trv;
    }
    json!({"v": 1, "api": "1.8", "len": at, "edges": edges})
}

/// Le corps d'une retouche du chemin `path` : sa polyligne, son document, `operation` (`{"resize": …}` ou
/// `{"move_end": …}`), le curseur s'il est donné.
pub fn edit_body(path: &[(usize, usize)], operation: Value, cursor: Option<f64>) -> Value {
    let mut b = json!({"polyline": encode_polyline6(&path_points(path)), "network_edges": path_document(path),
        "ride_type": "road", "operation": operation});
    if let Some(c) = cursor {
        b["cursor"] = json!(c);
    }
    b
}

/// La requête de retouche.
pub fn edit(key: Option<&str>, team: Option<&str>, body: &Value) -> Request<Body> {
    let mut b = Request::builder()
        .method(Method::POST)
        .uri("/v1/generator/routes/edit")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(k) = key {
        b = b.header("ord-namespace-key", k);
    }
    if let Some(t) = team {
        b = b.header("ord-team", t);
    }
    b.body(Body::from(body.to_string())).unwrap()
}

/// Le tour d'un carré de `n` rues de côté dont le coin nord-ouest est `(r0, c0)` : `4 n` rues, `1 200 n` m.
pub fn square(r0: usize, c0: usize, n: usize) -> Vec<(usize, usize)> {
    let mut p: Vec<(usize, usize)> = (c0..=c0 + n).map(|c| (r0, c)).collect();
    p.extend((r0 + 1..=r0 + n).map(|r| (r, c0 + n)));
    p.extend((c0..c0 + n).rev().map(|c| (r0 + n, c)));
    p.extend((r0..r0 + n).rev().map(|r| (r, c0)));
    p
}

/// Une ligne de la grille, de la colonne `c0` à la colonne `c1`.
pub fn row(r: usize, c0: usize, c1: usize) -> Vec<(usize, usize)> {
    (c0..=c1).map(|c| (r, c)).collect()
}
