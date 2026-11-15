//! Index spatial des segments du graphe : un R-tree `rstar` de segments `f32` par cellule du graphe, dans
//! le repère local de la cellule (mètres, origine au coin nord-ouest, échelle cos φ du centre).
//!
//! Un segment est inscrit dans **chaque** cellule que sa boîte touche (arêtes à cheval sur une limite) ;
//! une requête de rayon r interroge les cellules que touche sa boîte. Construit d'un coup, en parallèle,
//! pour toute la région (voir « Écarts » de la note de conception pour le choix de ne pas construire à la demande).

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
