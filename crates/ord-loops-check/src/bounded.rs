//! Les tables du contrôleur, **bornées** (note lot 6, §8) : avant, il reconstruisait à chaque requête des tableaux
//! sur tout le graphe (degrés de tous les nœuds, incidence de toutes les arêtes admises, un booléen par arête), soit
//! 0,3 à 0,45 Go et une à deux secondes sur la France. Maintenant :
//!
//! - le **degré admis** ne se calcule que pour les nœuds du parcours, en balayant les arêtes des cellules de ces
//!   nœuds et de leurs voisines ;
//! - les **arêtes comblées** (règle des trous) se calculent une fois par équipe, sur la zone de l'équipe (tuiles de
//!   ses arêtes et voisines à ± 1 km, puis leurs voisines), et se gardent en [`TeamCheck`] ;
//! - une arête plus large qu'une cellule ([`GraphIndex`]) est ajoutée à l'incidence de ses deux bouts : une arête
//!   possédée par `C` n'a ses bouts que dans `C` et ses voisines tant qu'elle n'est pas plus large qu'une cellule.
//!
//! Écrit depuis la même règle que le reste du contrôleur, sans rien du moteur ni de ses fenêtres.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use ord_graph::format::GraphFile;
use ord_graph::grid::{tile_key, tile_of_e7, tile_of_key, tile_origin_deg};
use ord_graph::overlay::Overlay;
use ord_loops::RideType;

use crate::check::Config;
use crate::rules::{admitted, team_edge, Admission, TypeIndex};

const M_PER_DEG: f64 = 6_371_008.8 * std::f64::consts::PI / 180.0;
/// Demi-côté (m) de la boîte autour de chaque point d'une arête de la surcouche.
const ZONE_BOX_M: f64 = 1_000.0;

/// Plus petite dimension (m) d'une tuile du graphe.
fn min_tile_m<B: AsRef<[u8]>>(g: &GraphFile<B>) -> f64 {
    let zoom = g.header().zoom;
    let mut min = f64::INFINITY;
    for ci in 0..g.n_cells() {
        let (x, y) = g.cell_xy(ci);
        let (lat_hi, lon_lo) = tile_origin_deg(x, y, zoom);
        let (lat_lo, lon_hi) = tile_origin_deg(x + 1, y + 1, zoom);
        let width = (lon_hi - lon_lo) * M_PER_DEG * lat_hi.abs().max(lat_lo.abs()).to_radians().cos();
        min = min.min(((lat_hi - lat_lo) * M_PER_DEG).min(width));
    }
    min
}

/// Ce qui est vrai de tout le graphe : les arêtes plus larges qu'une cellule.
#[derive(Debug, Default)]
pub struct GraphIndex {
    /// Arêtes dont un bout n'est ni dans leur cellule ni dans une voisine, croissantes.
    wide: Vec<u32>,
}

impl GraphIndex {
    /// Un balayage des enregistrements d'arêtes ; seules les arêtes de plus de la moitié d'une tuile sont examinées.
    pub fn build<B: AsRef<[u8]>>(g: &GraphFile<B>) -> Self {
        let thr_dm = (min_tile_m(g) * 0.5 * 10.0).min(f64::from(u32::MAX)) as u32;
        let mut wide = Vec::new();
        for ci in 0..g.n_cells() {
            let info = *g.cell_info(ci);
            let cv = g.cell(ci);
            for k in 0..info.n_edges {
                if cv.owned(k as usize).len_dm <= thr_dm {
                    continue;
                }
                let e = info.first_edge + k;
                let (a, b) = g.edge_ends(e);
                let far = |n: u32| {
                    let (x, y) = g.cell_xy(g.node_cell(n));
                    x.abs_diff(info.x) > 1 || y.abs_diff(info.y) > 1
                };
                if far(a) || far(b) {
                    wide.push(e);
                }
            }
        }
        Self { wide }
    }

    pub fn wide_edges(&self) -> &[u32] {
        &self.wide
    }
}

/// Cellules (indices) des tuiles `keys` et de leurs 8 voisines.
fn cells_around<B: AsRef<[u8]>>(g: &GraphFile<B>, keys: impl IntoIterator<Item = u32>) -> Vec<usize> {
    let mut tiles: Vec<u32> = Vec::new();
    for k in keys {
        let (x, y) = tile_of_key(k);
        for nx in x.saturating_sub(1)..=x + 1 {
            for ny in y.saturating_sub(1)..=y + 1 {
                tiles.push(tile_key(nx, ny));
            }
        }
    }
    tiles.sort_unstable();
    tiles.dedup();
    tiles
        .into_iter()
        .filter_map(|k| {
            let (x, y) = tile_of_key(k);
            g.find_cell(x, y)
        })
        .collect()
}

/// Arêtes admises incidentes à chaque nœud, des cellules `cells` : `(nœud, arête)` triés, sens ignorés (une boucle
/// compte une fois), plus les arêtes plus larges qu'une cellule que ces cellules ne possèdent pas.
fn incidence_of<B: AsRef<[u8]>>(
    g: &GraphFile<B>,
    gi: &GraphIndex,
    adm: Admission,
    cells: &[usize],
) -> Vec<(u32, u32)> {
    let mut list: Vec<(u32, u32)> = Vec::new();
    let mut scanned = HashSet::new();
    for &ci in cells {
        scanned.insert(ci);
        let info = *g.cell_info(ci);
        let cv = g.cell(ci);
        for k in 0..info.n_edges {
            if admitted(adm, cv.owned(k as usize).attrs) {
                let e = info.first_edge + k;
                let (a, b) = g.edge_ends(e);
                list.push((a, e));
                if a != b {
                    list.push((b, e));
                }
            }
        }
    }
    for &e in &gi.wide {
        if scanned.contains(&g.edge_cell(e)) {
            continue;
        }
        if admitted(adm, g.edge(e).attrs) {
            let (a, b) = g.edge_ends(e);
            list.push((a, e));
            if a != b {
                list.push((b, e));
            }
        }
    }
    list.sort_unstable();
    list.dedup();
    list
}

fn edges_at(inc: &[(u32, u32)], n: u32) -> impl Iterator<Item = u32> + '_ {
    let lo = inc.partition_point(|x| x.0 < n);
    let hi = inc.partition_point(|x| x.0 <= n);
    inc[lo..hi].iter().map(|x| x.1)
}

/// Degré admis des nœuds `nodes` : arêtes admises incidentes, sens ignorés, une boucle comptant deux fois.
pub(crate) fn degrees_of<B: AsRef<[u8]>>(
    g: &GraphFile<B>,
    gi: &GraphIndex,
    adm: Admission,
    nodes: &[u32],
) -> HashMap<u32, u8> {
    let want: HashSet<u32> = nodes.iter().copied().collect();
    let keys: HashSet<u32> = nodes
        .iter()
        .map(|&n| {
            let (x, y) = g.cell_xy(g.node_cell(n));
            tile_key(x, y)
        })
        .collect();
    let cells = cells_around(g, keys);
    let inc = incidence_of(g, gi, adm, &cells);
    let mut deg: HashMap<u32, u8> = want.iter().map(|&n| (n, 0)).collect();
    // Une boucle (a == b) compte deux fois.
    for &n in &want {
        for e in edges_at(&inc, n) {
            let (a, b) = g.edge_ends(e);
            let d = deg.get_mut(&n).expect("nœud voulu");
            *d = d.saturating_add(1 + u8::from(a == b));
        }
    }
    deg
}

/// Octets transitoires par arête des cellules de la zone : la liste d'incidence `(nœud, arête)` (deux entrées de 8 o
/// par arête admise, capacité de croissance du vecteur comprise), un seul des trois types à la fois. Réglé sur les
/// équipes A, C et D de la France, tables seules : pic anonyme mesuré 63, 99 et 139 Mo contre 82, 120 et 175 Mo estimés.
const BUILD_BYTES_PER_CELL_EDGE: usize = 16;
/// Octets transitoires par arête de la surcouche : ensemble des arêtes d'équipe, degrés des nœuds, parcours bornés.
const BUILD_BYTES_PER_OVERLAY_EDGE: usize = 24;

/// Ce qu'il faut construire pour les tables du contrôleur d'une équipe : les cellules de sa zone, et la mémoire
/// transitoire de la construction.
#[derive(Clone, Debug)]
pub struct CheckPlan {
    cells: Vec<usize>,
    /// Octets transitoires estimés de la construction.
    pub estimated_bytes: usize,
}

/// Les arêtes comblées par la règle des trous, par type : une fois par équipe, sur la zone de l'équipe.
#[derive(Debug, Default)]
pub struct TeamCheck {
    bridged: [HashSet<u32>; 3],
}

fn slot(r: RideType) -> usize {
    match r {
        RideType::Road => 0,
        RideType::Gravel => 1,
        RideType::Mtb => 2,
    }
}

impl TeamCheck {
    /// Calcule les arêtes comblées des trois types sur la zone de l'équipe.
    pub fn build<B: AsRef<[u8]>>(g: &GraphFile<B>, gi: &GraphIndex, overlay: &Overlay, cfg: &Config) -> Self {
        Self::build_on(g, gi, overlay, cfg, false)
    }

    /// Comme [`TeamCheck::build`] ; `whole` : sur toutes les cellules du graphe (la référence des tests).
    pub fn build_on<B: AsRef<[u8]>>(
        g: &GraphFile<B>,
        gi: &GraphIndex,
        overlay: &Overlay,
        cfg: &Config,
        whole: bool,
    ) -> Self {
        let plan = Self::plan_on(g, overlay, whole);
        Self::build_planned(g, gi, overlay, cfg, &plan)
    }

    /// Les cellules de la zone de l'équipe et la mémoire transitoire de la construction : l'appelant la réserve avant
    /// de construire (budget unique du service, lot 6 §10.1), comme pour `TeamData::plan_build`.
    pub fn plan<B: AsRef<[u8]>>(g: &GraphFile<B>, overlay: &Overlay) -> CheckPlan {
        Self::plan_on(g, overlay, false)
    }

    fn plan_on<B: AsRef<[u8]>>(g: &GraphFile<B>, overlay: &Overlay, whole: bool) -> CheckPlan {
        let zoom = g.header().zoom;
        let cells: Vec<usize> = if whole {
            (0..g.n_cells()).collect()
        } else {
            // Tuiles qui contiennent un point d'une arête de la surcouche, ou que touche la boîte de ± 1 km.
            let ky = M_PER_DEG * 1e-7;
            let dlat = (ZONE_BOX_M / ky) as i32 + 2;
            let mut tiles: Vec<u32> = Vec::new();
            for (e, c) in overlay.iter() {
                if c.iter().all(|&x| x == 0) {
                    continue;
                }
                let (a, b) = g.edge_ends(e);
                let mut pts = vec![g.node_pos(a)];
                pts.extend(g.geometry(e));
                pts.push(g.node_pos(b));
                for p in pts {
                    let dlon =
                        (ZONE_BOX_M / (ky * (f64::from(p.0) * 1e-7).to_radians().cos().max(0.05))) as i32 + 2;
                    let (x0, y0) = tile_of_e7(p.0.saturating_add(dlat), p.1.saturating_sub(dlon), zoom);
                    let (x1, y1) = tile_of_e7(p.0.saturating_sub(dlat), p.1.saturating_add(dlon), zoom);
                    for y in y0..=y1 {
                        for x in x0..=x1 {
                            tiles.push(tile_key(x, y));
                        }
                    }
                }
                if tiles.len() > 1 << 16 {
                    tiles.sort_unstable();
                    tiles.dedup();
                }
            }
            tiles.sort_unstable();
            tiles.dedup();
            cells_around(g, tiles)
        };
        let edges: usize = cells.iter().map(|&ci| g.cell_info(ci).n_edges as usize).sum();
        let estimated_bytes =
            edges * BUILD_BYTES_PER_CELL_EDGE + overlay.len() * BUILD_BYTES_PER_OVERLAY_EDGE;
        CheckPlan { cells, estimated_bytes }
    }

    /// Construit sur les cellules d'un [`CheckPlan`].
    pub fn build_planned<B: AsRef<[u8]>>(
        g: &GraphFile<B>,
        gi: &GraphIndex,
        overlay: &Overlay,
        cfg: &Config,
        plan: &CheckPlan,
    ) -> Self {
        let types = TypeIndex::of(overlay);
        let mut out = Self::default();
        for ride in [RideType::Road, RideType::Gravel, RideType::Mtb] {
            out.bridged[slot(ride)] = bridged_edges(g, gi, overlay, &types, cfg, ride, &plan.cells);
        }
        out
    }

    pub fn bridged(&self, ride: RideType) -> &HashSet<u32> {
        &self.bridged[slot(ride)]
    }

    /// Forme canonique en octets (finition 2, §20.4) : les trois ensembles d'arêtes comblées, triés, petit-boutiste.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for set in &self.bridged {
            let mut v: Vec<u32> = set.iter().copied().collect();
            v.sort_unstable();
            out.extend((v.len() as u64).to_le_bytes());
            for e in v {
                out.extend(e.to_le_bytes());
            }
        }
        out
    }

    /// Octets tenus : une table de hachage de `u32` coûte 5 octets par alvéole (valeur et octet de contrôle) et ses
    /// alvéoles sont les 8/7 de sa capacité.
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.bridged.iter().map(|s| s.capacity() * 8 / 7 * 5 + 16).sum::<usize>()
    }
}

/// Les arêtes comblées en strict, recalculées d'après la surcouche et la règle d'admission (note lot 2 §2.3) :
/// (1) le plus court chemin non roulé de `hole_m` au plus, d'un cul-de-sac fictif (un seul bout d'arête d'équipe, au
/// moins deux arêtes admises) au plus proche nœud d'équipe ; (2) tout chemin non roulé de `connector_m` au plus entre
/// deux nœuds d'équipe. Un parcours ne passe que par ces arêtes hors équipe.
fn bridged_edges<B: AsRef<[u8]>>(
    g: &GraphFile<B>,
    gi: &GraphIndex,
    o: &Overlay,
    types: &TypeIndex,
    cfg: &Config,
    ride: RideType,
    cells: &[usize],
) -> HashSet<u32> {
    let mut out = HashSet::new();
    let adm = Admission::of(ride);
    let inc = incidence_of(g, gi, adm, cells);
    let mut team: HashSet<u32> = HashSet::new();
    let mut tdeg: HashMap<u32, u8> = HashMap::new();
    for (e, counts) in o.iter() {
        if team_edge(ride, g.edge(e).attrs, Some(counts), types) {
            team.insert(e);
            let (a, b) = g.edge_ends(e);
            for n in [a, b] {
                let d = tdeg.entry(n).or_insert(0);
                *d = d.saturating_add(1);
            }
        }
    }
    let len_dm = |e: u32| g.edge(e).len_dm;
    // Degré admis d'un nœud, une boucle comptant deux fois.
    let adm_deg = |n: u32| -> u32 {
        edges_at(&inc, n)
            .map(|e| {
                let (a, b) = g.edge_ends(e);
                1 + u32::from(a == b)
            })
            .sum()
    };
    // Dijkstra borné sur les arêtes non roulées ; `all` : tous les nœuds d'équipe atteints, sinon le premier.
    let walk = |s: u32, limit_dm: u32, all: bool, out: &mut HashSet<u32>| {
        let mut dist: HashMap<u32, (u32, u32)> = HashMap::new();
        let mut heap = BinaryHeap::new();
        dist.insert(s, (0, u32::MAX));
        heap.push(Reverse((0u32, s)));
        while let Some(Reverse((d, x))) = heap.pop() {
            if dist[&x].0 < d {
                continue;
            }
            if x != s && tdeg.get(&x).is_some_and(|&t| t >= 1) {
                let mut cur = x;
                while cur != s {
                    let (_, pe) = dist[&cur];
                    out.insert(pe);
                    let (a, b) = g.edge_ends(pe);
                    cur = if a == cur { b } else { a };
                }
                if all {
                    continue;
                }
                break;
            }
            for be in edges_at(&inc, x) {
                if team.contains(&be) {
                    continue;
                }
                let nd = d + len_dm(be);
                if nd > limit_dm {
                    continue;
                }
                let (a, b) = g.edge_ends(be);
                let y = if a == x { b } else { a };
                if dist.get(&y).is_none_or(|&(od, _)| nd < od) {
                    dist.insert(y, (nd, be));
                    heap.push(Reverse((nd, y)));
                }
            }
        }
    };
    let hole_dm = (cfg.hole_m * 10.0) as u32;
    let conn_dm = (cfg.connector_m * 10.0) as u32;
    let mut nodes: Vec<u32> = tdeg.keys().copied().collect();
    nodes.sort_unstable();
    for &n in &nodes {
        if tdeg[&n] == 1 && adm_deg(n) >= 2 {
            walk(n, hole_dm, false, &mut out);
        }
    }
    for &n in &nodes {
        if edges_at(&inc, n).any(|e| !team.contains(&e) && len_dm(e) <= conn_dm) {
            walk(n, conn_dm, true, &mut out);
        }
    }
    out
}
