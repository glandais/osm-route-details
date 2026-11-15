//! Dijkstra par arêtes orientées, borné à un disque : **pour la mesure seulement** (§5 de la note).
//!
//! État = arête orientée (distance, prédécesseur). Le poids est une fonction fixe des attributs
//! (classe, revêtement, accès), décodée à chaque relâchement pour entrer dans le temps mesuré : ce n'est pas
//! une préférence de produit.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use rustc_hash::FxHashMap;

use crate::attrs::{Access, Attrs, HighwayClass};
use crate::format::{CellView, DirEdge, GraphFile, DIR_BIT};
use crate::grid;

const R_EARTH_M: f64 = 6_371_008.8;
const UNSEEN: u32 = u32::MAX;

/// Coût d'une arête (dm pondérés) ; `None` si l'accès est `No`.
pub fn edge_cost(a: Attrs, len_dm: u32) -> Option<u32> {
    if a.access() == Access::No {
        return None;
    }
    // Facteur en seizièmes.
    let mut f: u32 = match a.class() {
        Some(HighwayClass::Cycleway) => 12,
        Some(HighwayClass::LivingStreet) => 14,
        Some(HighwayClass::Unclassified) => 15,
        Some(HighwayClass::Residential) => 16,
        Some(HighwayClass::Tertiary) => 17,
        Some(HighwayClass::Road) => 18,
        Some(HighwayClass::Path | HighwayClass::Service) => 20,
        Some(HighwayClass::Secondary | HighwayClass::Track) => 22,
        Some(HighwayClass::Busway) => 30,
        Some(HighwayClass::Primary) => 32,
        Some(HighwayClass::Footway | HighwayClass::Pedestrian | HighwayClass::Bridleway) => 40,
        Some(HighwayClass::Trunk) => 48,
        Some(HighwayClass::Motorway) => 64,
        Some(HighwayClass::Steps) => 96,
        None => 32,
    };
    if a.is_unpaved() {
        f = f * 3 / 2;
    }
    match a.access() {
        Access::Destination => f = f * 5 / 4,
        Access::Private => f *= 3,
        _ => {}
    }
    Some(((u64::from(len_dm) * u64::from(f)).div_ceil(16)).min(u64::from(u32::MAX - 1)) as u32)
}

#[derive(Clone, Copy, Debug)]
pub struct SearchConfig {
    pub radius_m: f64,
    /// Interdit le demi-tour sur la même arête, sauf en cul-de-sac.
    pub forbid_uturn: bool,
    /// État en tableau à estampilles (12 o par orientée) plutôt qu'en table de hachage.
    pub dense: bool,
    /// Garde la meilleure distance à chaque nœud atteint (tests).
    pub collect_nodes: bool,
}

#[derive(Clone, Debug, Default)]
pub struct SearchStats {
    pub settled: u64,
    pub pushed: u64,
    pub heap_max: usize,
    pub state_bytes: u64,
    pub cells_touched: u32,
    /// Dichotomies faites pour retrouver la cellule d'un nœud ou d'une arête (hors cellule courante).
    pub bsearch_lookups: u64,
}

#[derive(Clone, Debug, Default)]
pub struct SearchResult {
    pub stats: SearchStats,
    /// `(nœud global, distance)` triés par nœud, si `collect_nodes`.
    pub node_dist: Vec<(u32, u32)>,
}

trait StateStore {
    fn best(&self, key: u32) -> u32;
    fn set(&mut self, key: u32, dist: u32, pred: u32);
    fn bytes(&self) -> u64;
}

impl StateStore for FxHashMap<u32, (u32, u32)> {
    fn best(&self, key: u32) -> u32 {
        self.get(&key).map_or(UNSEEN, |v| v.0)
    }
    fn set(&mut self, key: u32, dist: u32, pred: u32) {
        self.insert(key, (dist, pred));
    }
    fn bytes(&self) -> u64 {
        self.capacity() as u64 * (4 + 8 + 1)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Slot {
    stamp: u32,
    dist: u32,
    /// Prédécesseur (arête orientée) : écrit pour que son coût entre dans la mesure, lu par la reconstruction
    /// de chemin du lot suivant.
    #[allow(dead_code)]
    pred: u32,
}

struct Dense<'a> {
    slots: &'a mut [Slot],
    stamp: u32,
}

impl StateStore for Dense<'_> {
    fn best(&self, key: u32) -> u32 {
        let s = &self.slots[key as usize];
        if s.stamp == self.stamp {
            s.dist
        } else {
            UNSEEN
        }
    }
    fn set(&mut self, key: u32, dist: u32, pred: u32) {
        self.slots[key as usize] = Slot { stamp: self.stamp, dist, pred };
    }
    fn bytes(&self) -> u64 {
        self.slots.len() as u64 * 12
    }
}

/// Clé dense d'une arête orientée : `edge_id * 2 + sens`.
fn key_of(field: u32) -> u32 {
    ((field & !DIR_BIT) << 1) | (field >> 31)
}

struct Disc {
    lat0: f64,
    lon0: f64,
    cos0: f64,
    r2: f64,
}

impl Disc {
    fn new(lat_e7: i32, lon_e7: i32, radius_m: f64) -> Self {
        let lat0 = f64::from(lat_e7) * 1e-7;
        Self { lat0, lon0: f64::from(lon_e7) * 1e-7, cos0: lat0.to_radians().cos(), r2: radius_m * radius_m }
    }
    /// Équirectangulaire : suffisant à 75 km.
    fn contains(&self, lat_e7: i32, lon_e7: i32) -> bool {
        let dy = (f64::from(lat_e7) * 1e-7 - self.lat0).to_radians() * R_EARTH_M;
        let dx = (f64::from(lon_e7) * 1e-7 - self.lon0).to_radians() * self.cos0 * R_EARTH_M;
        dx * dx + dy * dy <= self.r2
    }
}

/// Cellules ouvertes paresseusement ; compte celles qui ont été touchées et les dichotomies faites.
///
/// Un nœud ou une arête de la cellule courante se reconnaît en une comparaison (intervalle de la cellule) :
/// la dichotomie sur `premier_nœud` / `première_arête` n'est faite que pour les arêtes de bord (§4 de la note).
struct Cells<'g, B> {
    g: &'g GraphFile<B>,
    views: Vec<Option<CellView<'g>>>,
    touched: u32,
    bsearch: u64,
}

impl<'g, B: AsRef<[u8]>> Cells<'g, B> {
    fn new(g: &'g GraphFile<B>) -> Self {
        Self { g, views: vec![None; g.n_cells()], touched: 0, bsearch: 0 }
    }
    fn get(&mut self, ci: usize) -> CellView<'g> {
        if let Some(v) = self.views[ci] {
            return v;
        }
        self.touched += 1;
        let v = self.g.cell(ci);
        self.views[ci] = Some(v);
        v
    }
    /// Cellule d'un nœud global, `hint` étant la cellule courante.
    fn node_cell(&mut self, gid: u32, hint: usize) -> usize {
        let i = self.g.cell_info(hint);
        if gid.wrapping_sub(i.first_node) < i.n_nodes {
            return hint;
        }
        self.bsearch += 1;
        self.g.node_cell(gid)
    }
    /// Cellule propriétaire d'une arête globale, `hint` étant la cellule courante.
    fn edge_cell(&mut self, e: u32, hint: usize) -> usize {
        let i = self.g.cell_info(hint);
        if e.wrapping_sub(i.first_edge) < i.n_edges {
            return hint;
        }
        self.bsearch += 1;
        self.g.edge_cell(e)
    }
}

/// État dense réutilisable (tableau à estampilles) : alloué une fois, hors de la zone chronométrée, puis
/// prêté à un `Searcher` par graphe ouvert.
#[derive(Debug, Default)]
pub struct DenseState {
    slots: Vec<Slot>,
    stamp: u32,
}

impl DenseState {
    /// Alloue et remplit le tableau pour `edges` arêtes (2 orientées chacune).
    pub fn new(edges: u64) -> Self {
        Self { slots: vec![Slot::default(); edges as usize * 2], stamp: 0 }
    }
}

/// Recherche réutilisable : le tableau dense est alloué une fois par graphe.
#[derive(Debug)]
pub struct Searcher<'g, B> {
    g: &'g GraphFile<B>,
    dense: DenseState,
}

impl<'g, B: AsRef<[u8]>> Searcher<'g, B> {
    pub fn new(g: &'g GraphFile<B>) -> Self {
        Self { g, dense: DenseState::default() }
    }

    /// Reprend un état dense déjà alloué (froid : une vue neuve par requête, un seul tableau).
    pub fn with_state(g: &'g GraphFile<B>, dense: DenseState) -> Self {
        Self { g, dense }
    }

    pub fn into_state(self) -> DenseState {
        self.dense
    }

    pub fn run(&mut self, start: u32, cfg: &SearchConfig) -> SearchResult {
        if cfg.dense {
            if self.dense.slots.is_empty() {
                self.dense = DenseState::new(self.g.header().edges);
            }
            self.dense.stamp += 1;
            let mut store = Dense { slots: &mut self.dense.slots, stamp: self.dense.stamp };
            run_with(self.g, &mut store, start, cfg)
        } else {
            let mut store: FxHashMap<u32, (u32, u32)> = FxHashMap::default();
            run_with(self.g, &mut store, start, cfg)
        }
    }
}

/// Entrée du tas : distance, arête orientée (avec sens), nœud cible, cellule du nœud cible.
type HeapEntry = Reverse<(u32, u32, u32, u32)>;

struct Run<'a, 'g, B, S> {
    cells: Cells<'g, B>,
    store: &'a mut S,
    heap: BinaryHeap<HeapEntry>,
    disc: Disc,
    stats: SearchStats,
}

impl<B: AsRef<[u8]>, S: StateStore> Run<'_, '_, B, S> {
    /// Coût de l'arête orientée `t` partant de la cellule `cur`, et cellule de sa cible ; `None` si l'accès est
    /// `No` ou si la cible est hors du disque (l'arête ne sera pas poussée).
    fn usable(&mut self, t: &DirEdge, cur: usize) -> Option<(u32, u32)> {
        let e = t.edge_id();
        let ci = self.cells.edge_cell(e, cur);
        let cv = self.cells.get(ci);
        let o = cv.owned((e - cv.info().first_edge) as usize);
        let w = edge_cost(o.attrs, o.len_dm)?;
        let tc = self.cells.node_cell(t.target(), cur);
        let tv = self.cells.get(tc);
        let (lat, lon) = tv.node((t.target() - tv.info().first_node) as usize);
        self.disc.contains(lat, lon).then_some((w, tc as u32))
    }

    fn push(&mut self, t: &DirEdge, nd: u32, pred: u32, tcell: u32) {
        let key = key_of(t.edge_field());
        if nd < self.store.best(key) {
            self.store.set(key, nd, pred);
            self.heap.push(Reverse((nd, t.edge_field(), t.target(), tcell)));
            self.stats.pushed += 1;
            self.stats.heap_max = self.stats.heap_max.max(self.heap.len());
        }
    }
}

fn run_with<B: AsRef<[u8]>, S: StateStore>(
    g: &GraphFile<B>,
    store: &mut S,
    start: u32,
    cfg: &SearchConfig,
) -> SearchResult {
    let mut cells = Cells::new(g);
    let start_cell = g.node_cell(start);
    let sv = cells.get(start_cell);
    let (slat, slon) = sv.node((start - sv.info().first_node) as usize);
    let mut run = Run {
        cells,
        store,
        heap: BinaryHeap::new(),
        disc: Disc::new(slat, slon, cfg.radius_m),
        stats: SearchStats::default(),
    };
    let mut node_best: FxHashMap<u32, u32> = FxHashMap::default();
    if cfg.collect_nodes {
        node_best.insert(start, 0);
    }
    for t in sv.out((start - sv.info().first_node) as usize) {
        if let Some((w, tc)) = run.usable(t, start_cell) {
            run.push(t, w, UNSEEN, tc);
        }
    }
    while let Some(Reverse((d, field, target, cell))) = run.heap.pop() {
        if d > run.store.best(key_of(field)) {
            continue;
        }
        run.stats.settled += 1;
        if cfg.collect_nodes {
            let e = node_best.entry(target).or_insert(d);
            *e = (*e).min(d);
        }
        let cell = cell as usize;
        let cv = run.cells.get(cell);
        let rev = field ^ DIR_BIT;
        // Le demi-tour n'est permis qu'en cul-de-sac : « autre sortie » = sortie utilisable (accès non `No`,
        // cible dans le disque), pas seulement présente.
        let mut back = None;
        let mut has_other = false;
        for t in cv.out((target - cv.info().first_node) as usize) {
            let Some((w, tc)) = run.usable(t, cell) else { continue };
            if cfg.forbid_uturn && t.edge_field() == rev {
                back = Some((t, w, tc));
                continue;
            }
            has_other = true;
            run.push(t, d.saturating_add(w), field, tc);
        }
        if let Some((t, w, tc)) = back {
            if !has_other {
                run.push(t, d.saturating_add(w), field, tc);
            }
        }
    }
    run.stats.state_bytes = run.store.bytes();
    run.stats.cells_touched = run.cells.touched;
    run.stats.bsearch_lookups = run.cells.bsearch;
    let mut node_dist: Vec<(u32, u32)> = node_best.into_iter().collect();
    node_dist.sort_unstable();
    SearchResult { stats: run.stats, node_dist }
}

/// Graphe à plat, en tableaux globaux, **pour la référence de mesure** : mêmes algorithme, mêmes états
/// (tableau dense), même poids décodé à chaque relâchement ; seule la lecture diffère (aucune cellule, aucune
/// dichotomie). L'écart avec `Searcher` en mode dense est le coût du lecteur par cellules.
#[derive(Debug)]
pub struct FlatGraph {
    pos: Vec<(i32, i32)>,
    csr: Vec<u32>,
    directed: Vec<DirEdge>,
    attrs: Vec<Attrs>,
    len_dm: Vec<u32>,
}

impl FlatGraph {
    /// Copie le graphe en tableaux globaux (ids de nœuds et d'arêtes déjà contigus par cellule).
    pub fn from_graph<B: AsRef<[u8]>>(g: &GraphFile<B>) -> Self {
        let h = g.header();
        let mut f = Self {
            pos: Vec::with_capacity(h.nodes as usize),
            csr: Vec::with_capacity(h.nodes as usize + 1),
            directed: Vec::with_capacity(h.directed as usize),
            attrs: Vec::with_capacity(h.edges as usize),
            len_dm: Vec::with_capacity(h.edges as usize),
        };
        for ci in 0..g.n_cells() {
            let cv = g.cell(ci);
            for local in 0..cv.info().n_nodes as usize {
                f.pos.push(cv.node(local));
                f.csr.push(f.directed.len() as u32);
                f.directed.extend_from_slice(cv.out(local));
            }
            for i in 0..cv.info().n_edges as usize {
                let o = cv.owned(i);
                f.attrs.push(o.attrs);
                f.len_dm.push(o.len_dm);
            }
        }
        f.csr.push(f.directed.len() as u32);
        f
    }

    fn out(&self, v: u32) -> &[DirEdge] {
        &self.directed[self.csr[v as usize] as usize..self.csr[v as usize + 1] as usize]
    }

    fn usable(&self, t: &DirEdge, disc: &Disc) -> Option<u32> {
        let e = t.edge_id() as usize;
        let w = edge_cost(self.attrs[e], self.len_dm[e])?;
        let (lat, lon) = self.pos[t.target() as usize];
        disc.contains(lat, lon).then_some(w)
    }
}

/// Recherche de référence sur `FlatGraph`, état dense.
#[derive(Debug)]
pub struct FlatSearcher<'g> {
    g: &'g FlatGraph,
    slots: Vec<Slot>,
    stamp: u32,
}

impl<'g> FlatSearcher<'g> {
    pub fn new(g: &'g FlatGraph) -> Self {
        Self { g, slots: vec![Slot::default(); g.attrs.len() * 2], stamp: 0 }
    }

    pub fn run(&mut self, start: u32, cfg: &SearchConfig) -> SearchResult {
        self.stamp += 1;
        let mut store = Dense { slots: &mut self.slots, stamp: self.stamp };
        let g = self.g;
        let (slat, slon) = g.pos[start as usize];
        let disc = Disc::new(slat, slon, cfg.radius_m);
        let mut stats = SearchStats::default();
        let mut heap: BinaryHeap<Reverse<(u32, u32, u32)>> = BinaryHeap::new();
        let mut push = |store: &mut Dense<'_>, heap: &mut BinaryHeap<_>, t: &DirEdge, nd: u32, pred: u32| {
            let key = key_of(t.edge_field());
            if nd < store.best(key) {
                store.set(key, nd, pred);
                heap.push(Reverse((nd, t.edge_field(), t.target())));
                stats.pushed += 1;
                stats.heap_max = stats.heap_max.max(heap.len());
            }
        };
        let mut node_best: FxHashMap<u32, u32> = FxHashMap::default();
        if cfg.collect_nodes {
            node_best.insert(start, 0);
        }
        for t in g.out(start) {
            if let Some(w) = g.usable(t, &disc) {
                push(&mut store, &mut heap, t, w, UNSEEN);
            }
        }
        let mut settled = 0u64;
        while let Some(Reverse((d, field, target))) = heap.pop() {
            if d > store.best(key_of(field)) {
                continue;
            }
            settled += 1;
            if cfg.collect_nodes {
                let e = node_best.entry(target).or_insert(d);
                *e = (*e).min(d);
            }
            let rev = field ^ DIR_BIT;
            let mut back = None;
            let mut has_other = false;
            for t in g.out(target) {
                let Some(w) = g.usable(t, &disc) else { continue };
                if cfg.forbid_uturn && t.edge_field() == rev {
                    back = Some((t, w));
                    continue;
                }
                has_other = true;
                push(&mut store, &mut heap, t, d.saturating_add(w), field);
            }
            if let Some((t, w)) = back {
                if !has_other {
                    push(&mut store, &mut heap, t, d.saturating_add(w), field);
                }
            }
        }
        stats.settled = settled;
        stats.state_bytes = store.bytes();
        let mut node_dist: Vec<(u32, u32)> = node_best.into_iter().collect();
        node_dist.sort_unstable();
        SearchResult { stats, node_dist }
    }
}

/// Contenu statique d'un disque : arêtes dont les deux extrémités sont dedans, arêtes orientées dont la
/// source et la cible le sont, longueur en km des premières. Parcourt les cellules qui touchent le disque.
#[derive(Clone, Copy, Debug, Default)]
pub struct DiscContent {
    pub edges: u64,
    pub directed: u64,
    pub km: f64,
}

pub fn disc_content<B: AsRef<[u8]>>(g: &GraphFile<B>, start: u32, radius_m: f64) -> DiscContent {
    let (slat, slon) = g.node_pos(start);
    let disc = Disc::new(slat, slon, radius_m);
    let zoom = g.header().zoom;
    let mut out = DiscContent::default();
    // Boîte englobante du disque, en degrés.
    let dlat = (radius_m / R_EARTH_M).to_degrees();
    let dlon = dlat / disc.cos0.max(0.01);
    for ci in 0..g.n_cells() {
        let (x, y) = g.cell_xy(ci);
        let (nlat, wlon) = grid::tile_origin_deg(x, y, zoom);
        let (slat_t, elon) = grid::tile_origin_deg(x + 1, y + 1, zoom);
        if nlat < disc.lat0 - dlat
            || slat_t > disc.lat0 + dlat
            || elon < disc.lon0 - dlon
            || wlon > disc.lon0 + dlon
        {
            continue;
        }
        let cv = g.cell(ci);
        let info = cv.info();
        for local in 0..info.n_nodes as usize {
            let (lat, lon) = cv.node(local);
            if !disc.contains(lat, lon) {
                continue;
            }
            for t in cv.out(local) {
                let (tl, tn) = g.node_pos(t.target());
                if disc.contains(tl, tn) {
                    out.directed += 1;
                }
            }
        }
        for i in 0..info.n_edges {
            let e = info.first_edge + i;
            let (a, b) = g.edge_ends(e);
            let (pa, pb) = (g.node_pos(a), g.node_pos(b));
            if disc.contains(pa.0, pa.1) && disc.contains(pb.0, pb.1) {
                out.edges += 1;
                out.km += f64::from(cv.owned(i as usize).len_dm) / 10_000.0;
            }
        }
    }
    out
}
