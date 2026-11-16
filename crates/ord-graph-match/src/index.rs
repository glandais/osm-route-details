//! Index spatial des segments du graphe : un R-tree `rstar` de segments `f32` par cellule du graphe, dans
//! le repère local de la cellule (mètres, origine au coin nord-ouest, échelle cos φ du centre).
//!
//! Un segment est inscrit dans **chaque** cellule que sa boîte touche (arêtes à cheval sur une limite) ;
//! une requête de rayon r interroge les cellules que touche sa boîte.
//!
//! Deux réalisations du même contrat ([`Near`]) : [`SegIndex`], construit d'un coup en parallèle pour toute la
//! région (l'outil : 402 Mio mesurés pour la région, 0,4 s à 16 fils), et [`CellIndex`], construit **par cellule, à la
//! demande**, borné en mémoire (le service : lot 3a §4). Les deux rendent exactement les mêmes `Hit`.

use ord_graph::grid::{tile_of_e7, tile_origin_deg};
use ord_graph::mmap::MappedGraph;
use rayon::prelude::*;
use rstar::primitives::{GeomWithData, Line};
use rstar::{PointDistance, RTree};

/// Mètres par degré de latitude (même constante que `geo`).
const M_PER_DEG: f64 = crate::geo::EARTH_R * std::f64::consts::PI / 180.0;

type Seg = GeomWithData<Line<[f32; 2]>, u32>;
/// Segments d'une cellule : ceux qu'elle possède, et ceux qui débordent vers `(cellule cible, segment)`.
type CellParts = (Vec<RawSeg>, Vec<(u32, RawSeg)>);

#[derive(Clone, Copy, Debug)]
struct CellFrame {
    lat0: f64,
    lon0: f64,
    kx: f64,
}

impl CellFrame {
    fn of(x: u32, y: u32, zoom: u8) -> Self {
        let (lat0, lon0) = tile_origin_deg(x, y, zoom);
        let (lat1, _) = tile_origin_deg(x, y + 1, zoom);
        Self { lat0, lon0, kx: M_PER_DEG * ((lat0 + lat1) / 2.0).to_radians().cos() }
    }
    fn xy(&self, lat: f64, lon: f64) -> [f32; 2] {
        [((lon - self.lon0) * self.kx) as f32, ((lat - self.lat0) * M_PER_DEG) as f32]
    }
}

/// Un segment trouvé : arête de base et distance (m) du point de requête, dans le repère de la cellule.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub edge: u32,
    pub dist: f32,
}

/// Ce que l'appariement demande à un index : les segments proches d'un point.
pub trait Near: Sync {
    /// Segments à `radius_m` mètres ou moins de `(lat, lon)` : un `Hit` par segment (une arête peut
    /// apparaître plusieurs fois).
    fn near(&self, lat: f64, lon: f64, radius_m: f64, out: &mut Vec<Hit>);
}

impl Near for SegIndex {
    fn near(&self, lat: f64, lon: f64, radius_m: f64, out: &mut Vec<Hit>) {
        SegIndex::near(self, lat, lon, radius_m, out);
    }
}

#[derive(Debug)]
pub struct SegIndex {
    zoom: u8,
    /// Une entrée par cellule du graphe : repère et arbre.
    cells: Vec<(CellFrame, RTree<Seg>)>,
    xy: Vec<(u32, u32)>,
    segments: usize,
    /// Positions des cellules, triées par Morton, pour retrouver une tuile.
    graph_cells: Vec<(u64, usize)>,
}

#[derive(Clone, Copy)]
struct RawSeg {
    edge: u32,
    p: (i32, i32),
    q: (i32, i32),
}

/// Points d'une arête de base, extrémités comprises, de `a` vers `b`, en E7 `(lat, lon)`.
pub fn edge_points_e7(gf: &MappedGraph, edge: u32) -> Vec<(i32, i32)> {
    let (a, b) = gf.edge_ends(edge);
    let mut pts = vec![gf.node_pos(a)];
    pts.extend(gf.geometry(edge));
    pts.push(gf.node_pos(b));
    pts
}

impl SegIndex {
    pub fn build(gf: &MappedGraph) -> Self {
        let zoom = gf.header().zoom;
        let n = gf.n_cells();
        let lookup: Vec<(u64, usize)> =
            (0..n).map(|i| (ord_graph::grid::morton(gf.cell_xy(i).0, gf.cell_xy(i).1), i)).collect();
        let cell_of = |x: u32, y: u32| -> Option<usize> {
            let key = ord_graph::grid::morton(x, y);
            lookup.binary_search_by_key(&key, |c| c.0).ok().map(|k| lookup[k].1)
        };
        // Phase A : segments de chaque cellule (les arêtes qu'elle possède), et ceux qui débordent.
        let parts: Vec<CellParts> = (0..n)
            .into_par_iter()
            .map(|ci| {
                let info = gf.cell_info(ci);
                let (mut local, mut spill) = (Vec::new(), Vec::new());
                for k in 0..info.n_edges {
                    let edge = info.first_edge + k;
                    let pts = edge_points_e7(gf, edge);
                    for w in pts.windows(2) {
                        let raw = RawSeg { edge, p: w[0], q: w[1] };
                        let (lat_hi, lat_lo) = (w[0].0.max(w[1].0), w[0].0.min(w[1].0));
                        let (lon_lo, lon_hi) = (w[0].1.min(w[1].1), w[0].1.max(w[1].1));
                        let (x0, y0) = tile_of_e7(lat_hi, lon_lo, zoom);
                        let (x1, y1) = tile_of_e7(lat_lo, lon_hi, zoom);
                        for x in x0..=x1 {
                            for y in y0..=y1 {
                                match cell_of(x, y) {
                                    Some(c) if c == ci => local.push(raw),
                                    Some(c) => spill.push((c as u32, raw)),
                                    None => {}
                                }
                            }
                        }
                    }
                }
                (local, spill)
            })
            .collect();
        let mut buckets: Vec<Vec<RawSeg>> = Vec::with_capacity(n);
        let mut spills = Vec::new();
        for (local, spill) in parts {
            buckets.push(local);
            spills.push(spill);
        }
        for (cell, raw) in spills.into_iter().flatten() {
            buckets[cell as usize].push(raw);
        }
        // Phase B : un arbre par cellule.
        let segments = buckets.iter().map(Vec::len).sum();
        let cells: Vec<(CellFrame, RTree<Seg>)> = buckets
            .into_par_iter()
            .enumerate()
            .map(|(ci, raws)| {
                let (x, y) = gf.cell_xy(ci);
                let frame = CellFrame::of(x, y, zoom);
                let segs: Vec<Seg> = raws
                    .iter()
                    .map(|r| {
                        let p = frame.xy(f64::from(r.p.0) * 1e-7, f64::from(r.p.1) * 1e-7);
                        let q = frame.xy(f64::from(r.q.0) * 1e-7, f64::from(r.q.1) * 1e-7);
                        GeomWithData::new(Line::new(p, q), r.edge)
                    })
                    .collect();
                (frame, RTree::bulk_load(segs))
            })
            .collect();
        let xy = (0..n).map(|i| gf.cell_xy(i)).collect();
        Self { zoom, cells, xy, segments, graph_cells: lookup }
    }

    /// Nombre de segments inscrits (un segment à cheval compte une fois par cellule).
    pub fn segments(&self) -> usize {
        self.segments
    }

    fn cell_at(&self, x: u32, y: u32) -> Option<usize> {
        let key = ord_graph::grid::morton(x, y);
        self.graph_cells.binary_search_by_key(&key, |c| c.0).ok().map(|k| self.graph_cells[k].1)
    }

    /// Segments à `radius_m` mètres ou moins de `(lat, lon)` : un `Hit` par segment (une arête peut
    /// apparaître plusieurs fois).
    pub fn near(&self, lat: f64, lon: f64, radius_m: f64, out: &mut Vec<Hit>) {
        let dlat = radius_m / M_PER_DEG;
        let dlon = radius_m / (M_PER_DEG * lat.to_radians().cos().max(0.01));
        let (x0, y0) = ord_graph::grid::tile_of_deg(lat + dlat, lon - dlon, self.zoom);
        let (x1, y1) = ord_graph::grid::tile_of_deg(lat - dlat, lon + dlon, self.zoom);
        for x in x0..=x1 {
            for y in y0..=y1 {
                let Some(ci) = self.cell_at(x, y) else { continue };
                debug_assert_eq!(self.xy[ci], (x, y));
                let (frame, tree) = &self.cells[ci];
                let q = frame.xy(lat, lon);
                let r2 = (radius_m * radius_m) as f32;
                for s in tree.locate_within_distance(q, r2) {
                    out.push(Hit { edge: s.data, dist: s.distance_2(&q).sqrt() });
                }
            }
        }
    }
}

// --- Index par cellule, à la demande -------------------------------------------------------------------------------

/// Un arbre de cellule et son repère.
#[derive(Debug)]
struct CellTree {
    frame: CellFrame,
    tree: RTree<Seg>,
    bytes: usize,
}

/// Octets estimés par segment inscrit (segment, nœud d'arbre, marge) et par arbre vide : borne de l'éviction,
/// pas une mesure exacte.
const SEG_BYTES: usize = 56;
const TREE_BASE_BYTES: usize = 256;

#[derive(Debug, Default)]
struct Lru {
    map: std::collections::HashMap<(u32, u32), std::sync::Arc<CellTree>>,
    /// Du moins récemment au plus récemment utilisé.
    order: std::collections::VecDeque<(u32, u32)>,
    bytes: usize,
}

impl Lru {
    fn touch(&mut self, key: (u32, u32)) {
        if let Some(i) = self.order.iter().position(|k| *k == key) {
            self.order.remove(i);
        }
        self.order.push_back(key);
    }
}

/// Index par cellule construit à la demande, borné par `cap_bytes` (éviction de la cellule la moins récente).
///
/// Une cellule est construite depuis ses 9 voisines du graphe en gardant les segments dont la boîte la touche.
/// Correct tant qu'**aucune arête n'est plus large qu'une cellule** (le segment d'une arête possédée par `C`
/// ne touche alors que `C` et ses voisines) : [`CellIndex::fits`] vérifie cette borne.
#[derive(Debug)]
pub struct CellIndex<'g> {
    gf: &'g MappedGraph,
    zoom: u8,
    cap_bytes: usize,
    lru: std::sync::Mutex<Lru>,
    built: std::sync::atomic::AtomicU64,
}

/// Tuiles `((x0, y0), (x1, y1))` que touche la boîte du segment `p`-`q`.
fn segment_tiles(p: (i32, i32), q: (i32, i32), zoom: u8) -> ((u32, u32), (u32, u32)) {
    let (lat_hi, lat_lo) = (p.0.max(q.0), p.0.min(q.0));
    let (lon_lo, lon_hi) = (p.1.min(q.1), p.1.max(q.1));
    (tile_of_e7(lat_hi, lon_lo, zoom), tile_of_e7(lat_lo, lon_hi, zoom))
}

impl<'g> CellIndex<'g> {
    /// Vrai si **aucune arête n'est plus large qu'une cellule** : chacun de ses segments ne touche que sa cellule
    /// propriétaire et ses 8 voisines. Parcourt tout le graphe (une fois au chargement) ; sinon, l'index par cellule
    /// serait incomplet et l'appelant construit l'index entier ([`SegIndex`]).
    pub fn fits(gf: &MappedGraph) -> bool {
        let zoom = gf.header().zoom;
        for ci in 0..gf.n_cells() {
            let info = gf.cell_info(ci);
            let (cx, cy) = gf.cell_xy(ci);
            for k in 0..info.n_edges {
                let pts = edge_points_e7(gf, info.first_edge + k);
                for w in pts.windows(2) {
                    let ((x0, y0), (x1, y1)) = segment_tiles(w[0], w[1], zoom);
                    if x0 + 1 < cx || y0 + 1 < cy || x1 > cx + 1 || y1 > cy + 1 {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Index vide. L'appelant a vérifié [`CellIndex::fits`] pour ce graphe.
    pub fn new(gf: &'g MappedGraph, cap_bytes: usize) -> Self {
        Self {
            gf,
            zoom: gf.header().zoom,
            cap_bytes,
            lru: std::sync::Mutex::new(Lru::default()),
            built: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Arbres construits depuis l'ouverture (une cellule évincée puis redemandée compte deux fois).
    pub fn built(&self) -> u64 {
        self.built.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Octets estimés des arbres gardés.
    pub fn bytes(&self) -> usize {
        self.lru.lock().map_or(0, |l| l.bytes)
    }

    fn build_cell(&self, x: u32, y: u32) -> CellTree {
        let frame = CellFrame::of(x, y, self.zoom);
        let mut segs: Vec<Seg> = Vec::new();
        for nx in x.saturating_sub(1)..=x + 1 {
            for ny in y.saturating_sub(1)..=y + 1 {
                let Some(ci) = self.gf.find_cell(nx, ny) else { continue };
                let info = self.gf.cell_info(ci);
                for k in 0..info.n_edges {
                    let edge = info.first_edge + k;
                    let pts = edge_points_e7(self.gf, edge);
                    for w in pts.windows(2) {
                        let ((x0, y0), (x1, y1)) = segment_tiles(w[0], w[1], self.zoom);
                        if (x0..=x1).contains(&x) && (y0..=y1).contains(&y) {
                            let p = frame.xy(f64::from(w[0].0) * 1e-7, f64::from(w[0].1) * 1e-7);
                            let q = frame.xy(f64::from(w[1].0) * 1e-7, f64::from(w[1].1) * 1e-7);
                            segs.push(GeomWithData::new(Line::new(p, q), edge));
                        }
                    }
                }
            }
        }
        let bytes = TREE_BASE_BYTES + segs.len() * SEG_BYTES;
        CellTree { frame, tree: RTree::bulk_load(segs), bytes }
    }

    fn cell(&self, x: u32, y: u32) -> Option<std::sync::Arc<CellTree>> {
        self.gf.find_cell(x, y)?;
        {
            let mut lru = self.lru.lock().ok()?;
            if let Some(t) = lru.map.get(&(x, y)).cloned() {
                lru.touch((x, y));
                return Some(t);
            }
        }
        let tree = std::sync::Arc::new(self.build_cell(x, y));
        self.built.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut lru = self.lru.lock().ok()?;
        if let Some(t) = lru.map.get(&(x, y)).cloned() {
            return Some(t); // construite entre-temps par un autre fil
        }
        lru.bytes += tree.bytes;
        lru.map.insert((x, y), tree.clone());
        lru.order.push_back((x, y));
        while lru.bytes > self.cap_bytes && lru.order.len() > 1 {
            let Some(old) = lru.order.pop_front() else { break };
            if let Some(t) = lru.map.remove(&old) {
                lru.bytes -= t.bytes;
            }
        }
        Some(tree)
    }
}

impl Near for CellIndex<'_> {
    fn near(&self, lat: f64, lon: f64, radius_m: f64, out: &mut Vec<Hit>) {
        let dlat = radius_m / M_PER_DEG;
        let dlon = radius_m / (M_PER_DEG * lat.to_radians().cos().max(0.01));
        let (x0, y0) = ord_graph::grid::tile_of_deg(lat + dlat, lon - dlon, self.zoom);
        let (x1, y1) = ord_graph::grid::tile_of_deg(lat - dlat, lon + dlon, self.zoom);
        for x in x0..=x1 {
            for y in y0..=y1 {
                let Some(cell) = self.cell(x, y) else { continue };
                let q = cell.frame.xy(lat, lon);
                let r2 = (radius_m * radius_m) as f32;
                for s in cell.tree.locate_within_distance(q, r2) {
                    out.push(Hit { edge: s.data, dist: s.distance_2(&q).sqrt() });
                }
            }
        }
    }
}
