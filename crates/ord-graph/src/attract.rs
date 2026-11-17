//! Fichier d'attracteurs d'une vue (`.orda`, lot 4d, §17.3 à §17.5 de `ord-loops-lot4.md`) : les objets OSM
//! que les parcours d'un type visent (cafés, boulangeries, points de vue, bourgs, cols, sommets), accrochés aux
//! nœuds de la vue.
//!
//! Le fichier porte deux tables, toutes deux **par nœud**, indexées par cellule de la vue :
//! - les **ancrages** `(nœud, catégorie)` : le nœud de vue le plus proche d'un objet, à `snap_m` au plus (le banc
//!   et le contrôle) ;
//! - la **zone** : chaque nœud de vue à `reach_m` au plus d'un nœud d'ancrage, avec le masque des catégories (le
//!   moteur). Un nœud a le même masque dans toute fenêtre qui le contient : la lecture ne dépend pas de la
//!   fenêtre.
//!
//! Le fichier est lié au graphe de base **et** à la vue (comptes, empreinte du PBF, époques de construction) ;
//! `AttrFile::check_for` le refuse sinon. L'extraction des objets d'un extrait (`extract_objects`) ne dépend que
//! d'`OsmSource` : elle sert aussi aux tests, sur des sources en mémoire.

use std::cmp::Ordering;
use std::path::Path;

use xxhash_rust::xxh3::xxh3_64;
pub use zerocopy::little_endian::U32;
use zerocopy::little_endian::{I64, U16, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

use crate::build::{haversine_m, BuildError, ElementSink, OsmRelation, OsmSource, OsmWay};
use crate::format::{GraphFile, Header};
use crate::view::ViewKind;

pub const ATTR_MAGIC: [u8; 8] = *b"ORDATTR\0";
pub const ATTR_MAJOR: u16 = 1;
pub const ATTR_MINOR: u16 = 0;
const HEADER_LEN: usize = 128;
const CAT_LEN: usize = 16;

/// Catégories écrites, dans l'ordre des bits du masque (figée par le format 1).
pub const CATS: [&str; 7] = ["cafe", "bakery", "viewpoint", "town", "village", "mountain_pass", "peak"];
pub const N_CATS: u8 = 7;
pub const CAT_CAFE: u8 = 0;
pub const CAT_BAKERY: u8 = 1;
pub const CAT_VIEWPOINT: u8 = 2;
pub const CAT_TOWN: u8 = 3;
pub const CAT_VILLAGE: u8 = 4;
pub const CAT_MOUNTAIN_PASS: u8 = 5;
pub const CAT_PEAK: u8 = 6;

pub const DEFAULT_SNAP_M: u16 = 300;
pub const DEFAULT_REACH_M: u16 = 300;

#[derive(Debug, thiserror::Error)]
pub enum AttrError {
    #[error("fichier d'attracteurs tronqué : {0}")]
    Truncated(String),
    #[error("ce n'est pas un fichier d'attracteurs (signature)")]
    BadMagic,
    #[error("version majeure {0} du fichier d'attracteurs non gérée")]
    UnsupportedMajor(u16),
    #[error("fichier d'attracteurs corrompu : {0}")]
    Corrupt(String),
    #[error("fichier d'attracteurs d'un autre graphe ou d'une autre vue : {0}")]
    Mismatch(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

fn corrupt(s: impl Into<String>) -> AttrError {
    AttrError::Corrupt(s.into())
}

// --- Catégories d'une étiquette ----------------------------------------------------------------------------------

/// Bits de catégorie d'un objet OSM selon ses étiquettes. `peak` ne vaut que pour un nœud (`is_node`).
pub fn cats_of<'a>(tags: impl IntoIterator<Item = (&'a str, &'a str)>, is_node: bool) -> u8 {
    let mut mask = 0u8;
    for (k, v) in tags {
        let bit = match (k, v) {
            ("amenity", "cafe") => CAT_CAFE,
            ("shop", "bakery") => CAT_BAKERY,
            ("tourism", "viewpoint") => CAT_VIEWPOINT,
            ("place", "town") => CAT_TOWN,
            ("place", "village") => CAT_VILLAGE,
            ("mountain_pass", "yes") => CAT_MOUNTAIN_PASS,
            ("natural", "peak") if is_node => CAT_PEAK,
            _ => continue,
        };
        mask |= 1 << bit;
    }
    mask
}

// --- Extraction des objets -----------------------------------------------------------------------------------------

/// Un objet OSM porteur d'une catégorie : un point en décimicro-degrés. `kind` : 0 nœud, 1 way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Object {
    pub cat: u8,
    pub lat: i32,
    pub lon: i32,
    pub kind: u8,
    pub id: i64,
}

/// Ce que l'extraction a vu, pour le rapport.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtractStats {
    /// Ways portant au moins une catégorie.
    pub ways_tagged: u64,
    /// … dont aucun nœud n'a été retrouvé (écartées).
    pub ways_without_node: u64,
    /// Nœuds de ways demandés, puis retrouvés.
    pub nodes_wanted: u64,
    pub nodes_found: u64,
}

/// Ramasse les ways étiquetées de la passe 1 (relations ignorées).
#[derive(Default)]
struct WaySink {
    /// `(id, masque, nœuds)`.
    ways: Vec<(i64, u8, Vec<i64>)>,
}

impl ElementSink for WaySink {
    fn way(&mut self, way: &OsmWay<'_>) -> Result<(), BuildError> {
        let mask = cats_of(way.tags.iter().copied(), false);
        if mask != 0 {
            self.ways.push((way.id, mask, way.refs.to_vec()));
        }
        Ok(())
    }
    fn relation(&mut self, _relation: &OsmRelation<'_>) -> Result<(), BuildError> {
        Ok(())
    }
}

/// Filtre d'appartenance rapide (un bit par hachage d'id) devant la dichotomie des nœuds demandés.
struct IdFilter {
    bits: Vec<u64>,
    shift: u32,
}

impl IdFilter {
    fn new(ids: &[i64]) -> Self {
        // ~16 bits par id, au plus 2^30 bits.
        let want = (ids.len() as u64).saturating_mul(16).clamp(1 << 10, 1 << 30);
        let nbits = want.next_power_of_two();
        let shift = 64 - nbits.trailing_zeros();
        let mut f = Self { bits: vec![0; (nbits / 64) as usize], shift };
        for &id in ids {
            let h = f.hash(id);
            f.bits[h >> 6] |= 1 << (h & 63);
        }
        f
    }
    fn hash(&self, id: i64) -> usize {
        ((id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> self.shift) as usize
    }
    fn may_contain(&self, id: i64) -> bool {
        let h = self.hash(id);
        self.bits[h >> 6] >> (h & 63) & 1 != 0
    }
}

const NO_COORD: u64 = u64::MAX;

fn pack(lat: i32, lon: i32) -> u64 {
    (u64::from(lat as u32) << 32) | u64::from(lon as u32)
}

fn unpack(v: u64) -> (i32, i32) {
    ((v >> 32) as u32 as i32, v as u32 as i32)
}

/// Les objets d'un extrait, triés par `(catégorie, lat, lon, type, id)` (§17.5, points 2 à 4). Deux passes sur
/// la source : les ways étiquetées, puis les nœuds (étiquetés, et ceux des ways retenues). La sortie ne dépend
/// ni du nombre de fils ni de la taille des lots.
pub fn extract_objects(src: &mut dyn OsmSource) -> Result<(Vec<Object>, ExtractStats), BuildError> {
    use std::sync::atomic::{AtomicU64, Ordering as AO};
    use std::sync::Mutex;

    let mut sink = WaySink::default();
    src.scan(&mut sink)?;
    let ways = sink.ways;
    let mut wanted: Vec<i64> = ways.iter().flat_map(|w| w.2.iter().copied()).collect();
    wanted.sort_unstable();
    wanted.dedup();
    let filter = IdFilter::new(&wanted);
    let found: Vec<AtomicU64> = (0..wanted.len()).map(|_| AtomicU64::new(NO_COORD)).collect();
    let node_objects: Mutex<Vec<Object>> = Mutex::new(Vec::new());

    src.tagged_nodes(&|id, lat, lon, tags| {
        if !tags.is_empty() {
            let mask = cats_of(tags.iter().copied(), true);
            if mask != 0 {
                let mut out = node_objects.lock().expect("verrou sain");
                for cat in 0..N_CATS {
                    if mask >> cat & 1 != 0 {
                        out.push(Object { cat, lat, lon, kind: 0, id });
                    }
                }
            }
        }
        if filter.may_contain(id) {
            if let Ok(rank) = wanted.binary_search(&id) {
                found[rank].store(pack(lat, lon), AO::Relaxed);
            }
        }
    })?;

    let mut stats = ExtractStats {
        ways_tagged: ways.len() as u64,
        nodes_wanted: wanted.len() as u64,
        nodes_found: found.iter().filter(|c| c.load(AO::Relaxed) != NO_COORD).count() as u64,
        ..Default::default()
    };
    let mut objects = node_objects.into_inner().expect("verrou sain");
    let mut distinct: Vec<i64> = Vec::new();
    for (id, mask, refs) in &ways {
        distinct.clear();
        distinct.extend_from_slice(refs);
        distinct.sort_unstable();
        distinct.dedup();
        let (mut n, mut sl, mut so) = (0i64, 0i64, 0i64);
        for r in &distinct {
            if let Ok(rank) = wanted.binary_search(r) {
                let c = found[rank].load(AO::Relaxed);
                if c != NO_COORD {
                    let (lat, lon) = unpack(c);
                    n += 1;
                    sl += i64::from(lat);
                    so += i64::from(lon);
                }
            }
        }
        if n == 0 {
            stats.ways_without_node += 1;
            continue;
        }
        let (lat, lon) = ((sl / n) as i32, (so / n) as i32);
        for cat in 0..N_CATS {
            if mask >> cat & 1 != 0 {
                objects.push(Object { cat, lat, lon, kind: 1, id: *id });
            }
        }
    }
    objects.sort_unstable();
    Ok((objects, stats))
}

// --- Grille spatiale ---------------------------------------------------------------------------------------------

const CELL_E7: i32 = 50_000; // 0,005°
const LON_CELLS: i64 = 72_000;
const EARTH_M_PER_DEG: f64 = 111_194.93;

#[derive(Clone, Copy, Debug)]
struct Entry {
    key: u32,
    id: u32,
    lat: i32,
    lon: i32,
    tag: u8,
}

fn cell_key(lat: i32, lon: i32) -> u32 {
    let la = (i64::from(lat) + 900_000_000).div_euclid(i64::from(CELL_E7));
    let lo = (i64::from(lon) + 1_800_000_000).div_euclid(i64::from(CELL_E7));
    (la.clamp(0, 35_999) * LON_CELLS + lo.clamp(0, LON_CELLS - 1)) as u32
}

fn cell_coords(lat: i32, lon: i32) -> (i64, i64) {
    (
        (i64::from(lat) + 900_000_000).div_euclid(i64::from(CELL_E7)).clamp(0, 35_999),
        (i64::from(lon) + 1_800_000_000).div_euclid(i64::from(CELL_E7)).clamp(0, LON_CELLS - 1),
    )
}

/// Points rangés par case de 0,005° de côté, triés par `(case, id, tag)`.
#[derive(Debug)]
struct Grid {
    entries: Vec<Entry>,
}

impl Grid {
    fn new(mut entries: Vec<Entry>) -> Self {
        entries.sort_unstable_by_key(|e| (e.key, e.id, e.tag));
        Self { entries }
    }

    /// Les points à `radius_m` au plus (haversine) de `(lat, lon)`.
    fn within(&self, lat: i32, lon: i32, radius_m: f64, mut f: impl FnMut(&Entry)) {
        let dlat_deg = radius_m / EARTH_M_PER_DEG;
        let lat_deg = f64::from(lat) * 1e-7;
        let cos = (lat_deg.abs() + dlat_deg).min(89.9).to_radians().cos().max(0.01);
        let dlon_deg = dlat_deg / cos;
        let (dlat_e7, dlon_e7) = ((dlat_deg * 1e7).ceil() as i32 + 1, (dlon_deg * 1e7).ceil() as i32 + 1);
        let (la0, lo0) = cell_coords(lat.saturating_sub(dlat_e7), lon.saturating_sub(dlon_e7));
        let (la1, lo1) = cell_coords(lat.saturating_add(dlat_e7), lon.saturating_add(dlon_e7));
        for la in la0..=la1 {
            let lo_key = (la * LON_CELLS + lo0) as u32;
            let hi_key = (la * LON_CELLS + lo1) as u32;
            let from = self.entries.partition_point(|e| e.key < lo_key);
            for e in &self.entries[from..] {
                if e.key > hi_key {
                    break;
                }
                if haversine_m((lat, lon), (e.lat, e.lon)) <= radius_m {
                    f(e);
                }
            }
        }
    }
}

/// Les nœuds d'une vue en grille.
fn node_grid<B: AsRef<[u8]>>(view: &GraphFile<B>) -> Grid {
    let mut entries = Vec::with_capacity(view.header().nodes as usize);
    for ci in 0..view.n_cells() {
        let cell = view.cell(ci);
        let first = cell.info().first_node;
        for local in 0..cell.info().n_nodes as usize {
            let (lat, lon) = cell.node(local);
            entries.push(Entry { key: cell_key(lat, lon), id: first + local as u32, lat, lon, tag: 0 });
        }
    }
    Grid::new(entries)
}

// --- Construction ------------------------------------------------------------------------------------------------

/// Ancrages et zone d'une vue.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AttrData {
    /// `(nœud, catégorie)`, triés, sans doublon.
    pub anchors: Vec<(u32, u8)>,
    /// `(nœud, masque non nul)`, nœuds strictement croissants.
    pub zone: Vec<(u32, u8)>,
}

impl AttrData {
    /// Ancrages, puis nœuds de zone, par catégorie (bits `0..n_cats`).
    pub fn counts_by_cat(&self, n_cats: usize) -> (Vec<u64>, Vec<u64>) {
        let (mut a, mut z) = (vec![0u64; n_cats], vec![0u64; n_cats]);
        for &(_, c) in &self.anchors {
            a[c as usize] += 1;
        }
        for &(_, m) in &self.zone {
            for (c, slot) in z.iter_mut().enumerate() {
                if m >> c & 1 != 0 {
                    *slot += 1;
                }
            }
        }
        (a, z)
    }
}

/// Ancrages : pour chaque objet, le nœud de vue le plus proche à `snap_m` au plus (égalité : plus petit id).
pub fn anchors_for<B: AsRef<[u8]>>(view: &GraphFile<B>, objects: &[Object], snap_m: u16) -> Vec<(u32, u8)> {
    let grid = node_grid(view);
    anchors_in(&grid, objects, snap_m)
}

fn snap_in(grid: &Grid, objects: &[Object], snap_m: u16) -> Vec<Option<u32>> {
    let r = f64::from(snap_m);
    objects
        .iter()
        .map(|o| {
            let mut best: Option<(f64, u32)> = None;
            grid.within(o.lat, o.lon, r, |e| {
                let d = haversine_m((o.lat, o.lon), (e.lat, e.lon));
                let better = match best {
                    None => true,
                    Some((bd, bid)) => match d.partial_cmp(&bd) {
                        Some(Ordering::Less) => true,
                        Some(Ordering::Equal) => e.id < bid,
                        _ => false,
                    },
                };
                if better {
                    best = Some((d, e.id));
                }
            });
            best.map(|b| b.1)
        })
        .collect()
}

fn anchors_from(objects: &[Object], snapped: &[Option<u32>]) -> Vec<(u32, u8)> {
    let mut out: Vec<(u32, u8)> =
        objects.iter().zip(snapped).filter_map(|(o, s)| s.map(|n| (n, o.cat))).collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn anchors_in(grid: &Grid, objects: &[Object], snap_m: u16) -> Vec<(u32, u8)> {
    anchors_from(objects, &snap_in(grid, objects, snap_m))
}

/// Zone : chaque nœud de vue à `reach_m` au plus d'un nœud d'ancrage (lui compris), masque des catégories.
pub fn zone_for<B: AsRef<[u8]>>(view: &GraphFile<B>, anchors: &[(u32, u8)], reach_m: u16) -> Vec<(u32, u8)> {
    let grid = node_grid(view);
    zone_in(view, &grid, anchors, reach_m)
}

fn zone_in<B: AsRef<[u8]>>(
    view: &GraphFile<B>,
    grid: &Grid,
    anchors: &[(u32, u8)],
    reach_m: u16,
) -> Vec<(u32, u8)> {
    let r = f64::from(reach_m);
    let mut hits: Vec<(u32, u8)> = Vec::new();
    for &(node, cat) in anchors {
        let (lat, lon) = view.node_pos(node);
        grid.within(lat, lon, r, |e| hits.push((e.id, 1u8 << cat)));
    }
    hits.sort_unstable();
    let mut zone: Vec<(u32, u8)> = Vec::new();
    for (node, mask) in hits {
        match zone.last_mut() {
            Some(last) if last.0 == node => last.1 |= mask,
            _ => zone.push((node, mask)),
        }
    }
    zone
}

/// Ancrages puis zone des objets pour une vue.
pub fn build_attr<B: AsRef<[u8]>>(
    view: &GraphFile<B>,
    objects: &[Object],
    snap_m: u16,
    reach_m: u16,
) -> AttrData {
    build_attr_counted(view, objects, snap_m, reach_m).0
}

/// Comme `build_attr`, et le nombre d'objets accrochés par catégorie (bits `0..8`).
pub fn build_attr_counted<B: AsRef<[u8]>>(
    view: &GraphFile<B>,
    objects: &[Object],
    snap_m: u16,
    reach_m: u16,
) -> (AttrData, [u64; 8]) {
    let grid = node_grid(view);
    let snapped = snap_in(&grid, objects, snap_m);
    let mut per_cat = [0u64; 8];
    for (o, s) in objects.iter().zip(&snapped) {
        per_cat[usize::from(o.cat & 7)] += u64::from(s.is_some());
    }
    let anchors = anchors_from(objects, &snapped);
    let zone = zone_in(view, &grid, &anchors, reach_m);
    (AttrData { anchors, zone }, per_cat)
}

// --- Format ------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C)]
struct RawHeader {
    magic: [u8; 8],
    major: U16,
    minor: U16,
    view_kind: u8,
    n_cats: u8,
    snap_m: U16,
    reach_m: U16,
    base_nodes: U64,
    base_edges: U64,
    base_pbf_size: U64,
    base_pbf_xxh3: U64,
    base_build_epoch: I64,
    view_nodes: U64,
    view_edges: U64,
    view_build_epoch: I64,
    view_cells: U32,
    n_anchors: U32,
    n_zone: U32,
    meta_len: U32,
    built_epoch: I64,
    reserved: [u8; 22],
}

const _: () = assert!(std::mem::size_of::<RawHeader>() == HEADER_LEN);

/// En-tête décodé d'un fichier d'attracteurs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttrHeader {
    pub major: u16,
    pub minor: u16,
    /// 0 `road`, 1 `gravel`.
    pub view_kind: u8,
    pub n_cats: u8,
    pub snap_m: u16,
    pub reach_m: u16,
    pub base_nodes: u64,
    pub base_edges: u64,
    pub base_pbf_size: u64,
    pub base_pbf_xxh3: u64,
    pub base_build_epoch: i64,
    pub view_nodes: u64,
    pub view_edges: u64,
    pub view_build_epoch: i64,
    pub view_cells: u32,
    pub n_anchors: u32,
    pub n_zone: u32,
    pub meta_len: u32,
    pub built_epoch: i64,
}

fn kind_code(k: ViewKind) -> Option<u8> {
    match k {
        ViewKind::Road => Some(0),
        ViewKind::Gravel => Some(1),
        ViewKind::All => None,
    }
}

/// Ce que l'écriture ajoute aux ancrages et à la zone.
#[derive(Clone, Debug)]
pub struct AttrWrite<'a> {
    pub snap_m: u16,
    pub reach_m: u16,
    /// JSON de provenance et de comptes (rapport), recopié tel quel.
    pub meta: &'a [u8],
    /// Date d'écriture (provenance, hors identité).
    pub built_epoch: i64,
}

fn pad8(v: &mut Vec<u8>) {
    while !v.len().is_multiple_of(8) {
        v.push(0);
    }
}

/// Encode le fichier : en-tête, sections alignées sur 8 octets, empreinte xxh3 de fin. `base` est l'en-tête du
/// graphe de base, `view` la vue (type `road` ou `gravel`, non élaguée).
pub fn encode_attr<B: AsRef<[u8]>>(
    base: &Header,
    view: &GraphFile<B>,
    data: &AttrData,
    w: &AttrWrite<'_>,
) -> Result<Vec<u8>, AttrError> {
    let meta = view.view_meta().ok_or_else(|| AttrError::Mismatch("ce n'est pas une vue".into()))?;
    let kind = kind_code(meta.kind).ok_or_else(|| {
        AttrError::Mismatch(format!("vue `{}` : seuls road et gravel ont un fichier", meta.name))
    })?;
    if meta.prune_dead_ends {
        return Err(AttrError::Mismatch(format!(
            "vue `{}` élaguée : pas de fichier d'attracteurs",
            meta.name
        )));
    }
    let vh = view.header();
    let view_cells = view.n_cells() as u32;
    let first_nodes: Vec<u32> = (0..view.n_cells()).map(|c| view.cell_info(c).first_node).collect();
    let header = RawHeader {
        magic: ATTR_MAGIC,
        major: U16::new(ATTR_MAJOR),
        minor: U16::new(ATTR_MINOR),
        view_kind: kind,
        n_cats: N_CATS,
        snap_m: U16::new(w.snap_m),
        reach_m: U16::new(w.reach_m),
        base_nodes: U64::new(base.nodes),
        base_edges: U64::new(base.edges),
        base_pbf_size: U64::new(base.pbf_size),
        base_pbf_xxh3: U64::new(base.pbf_xxh3),
        base_build_epoch: I64::new(base.build_epoch),
        view_nodes: U64::new(vh.nodes),
        view_edges: U64::new(vh.edges),
        view_build_epoch: I64::new(vh.build_epoch),
        view_cells: U32::new(view_cells),
        n_anchors: U32::new(data.anchors.len() as u32),
        n_zone: U32::new(data.zone.len() as u32),
        meta_len: U32::new(w.meta.len() as u32),
        built_epoch: I64::new(w.built_epoch),
        reserved: [0; 22],
    };
    let mut out: Vec<u8> = Vec::with_capacity(
        HEADER_LEN + 4 * (2 * view_cells as usize + 2) + 8 * (data.anchors.len() + data.zone.len()) + 1024,
    );
    out.extend_from_slice(header.as_bytes());
    for name in CATS {
        let mut b = [0u8; CAT_LEN];
        b[..name.len()].copy_from_slice(name.as_bytes());
        out.extend_from_slice(&b);
    }
    pad8(&mut out);
    // Décalages par cellule : premier élément dont le nœud est dans la cellule ou après.
    let offsets = |nodes: &mut dyn Iterator<Item = u32>| -> Vec<u32> {
        let nodes: Vec<u32> = nodes.collect();
        let mut off: Vec<u32> =
            first_nodes.iter().map(|&f| nodes.partition_point(|&n| n < f) as u32).collect();
        off.push(nodes.len() as u32);
        off
    };
    for v in offsets(&mut data.anchors.iter().map(|a| a.0)) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    pad8(&mut out);
    for a in &data.anchors {
        out.extend_from_slice(&a.0.to_le_bytes());
    }
    pad8(&mut out);
    out.extend(data.anchors.iter().map(|a| a.1));
    pad8(&mut out);
    for v in offsets(&mut data.zone.iter().map(|z| z.0)) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    pad8(&mut out);
    for z in &data.zone {
        out.extend_from_slice(&z.0.to_le_bytes());
    }
    pad8(&mut out);
    out.extend(data.zone.iter().map(|z| z.1));
    pad8(&mut out);
    out.extend_from_slice(w.meta);
    pad8(&mut out);
    let sum = xxh3_64(&out);
    out.extend_from_slice(&sum.to_le_bytes());
    Ok(out)
}

/// Écrit le fichier à côté de sa destination, en 0444, puis le renomme (atomique) ; rend sa taille.
pub fn write_attr<B: AsRef<[u8]>>(
    path: &Path,
    base: &Header,
    view: &GraphFile<B>,
    data: &AttrData,
    w: &AttrWrite<'_>,
) -> Result<u64, AttrError> {
    let bytes = encode_attr(base, view, data, w)?;
    let tmp = path.with_extension("orda.tmp");
    // Un temporaire laissé par une écriture interrompue est en 0444 : `write` ne pourrait pas l'ouvrir.
    match std::fs::remove_file(&tmp) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    std::fs::write(&tmp, &bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o444))?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(bytes.len() as u64)
}

#[cfg(feature = "mmap")]
impl AttrFile<memmap2::Mmap> {
    /// Ouvre un fichier d'attracteurs projeté en mémoire.
    pub fn open_mmap(path: &Path) -> Result<Self, AttrError> {
        let file = std::fs::File::open(path)?;
        let map = crate::mmap::map_readonly(&file)?;
        AttrFile::open(map)
    }
}

fn align8(n: usize) -> usize {
    n.div_ceil(8) * 8
}

/// Un fichier d'attracteurs lu : les tableaux sont des tranches de l'octet-tableau `B`.
#[derive(Debug)]
pub struct AttrFile<B> {
    data: B,
    header: AttrHeader,
    cats_at: usize,
    a_off_at: usize,
    a_node_at: usize,
    a_cat_at: usize,
    z_off_at: usize,
    z_node_at: usize,
    z_mask_at: usize,
    meta_at: usize,
}

fn u32s(bytes: &[u8], at: usize, n: usize) -> &[U32] {
    <[U32]>::ref_from_bytes(&bytes[at..at + 4 * n]).expect("tranche validée à l'ouverture")
}

impl<B: AsRef<[u8]>> AttrFile<B> {
    /// Ouvre et valide tout le fichier : signature, version, tailles, décalages, tri, masques, empreinte.
    pub fn open(data: B) -> Result<Self, AttrError> {
        let bytes = data.as_ref();
        if bytes.len() < HEADER_LEN + 8 {
            return Err(AttrError::Truncated(format!("{} octets", bytes.len())));
        }
        let raw = RawHeader::ref_from_bytes(&bytes[..HEADER_LEN]).expect("128 octets");
        if raw.magic != ATTR_MAGIC {
            return Err(AttrError::BadMagic);
        }
        if raw.major.get() != ATTR_MAJOR {
            return Err(AttrError::UnsupportedMajor(raw.major.get()));
        }
        if raw.reserved.iter().any(|&b| b != 0) {
            return Err(corrupt("octets réservés non nuls"));
        }
        let h = AttrHeader {
            major: raw.major.get(),
            minor: raw.minor.get(),
            view_kind: raw.view_kind,
            n_cats: raw.n_cats,
            snap_m: raw.snap_m.get(),
            reach_m: raw.reach_m.get(),
            base_nodes: raw.base_nodes.get(),
            base_edges: raw.base_edges.get(),
            base_pbf_size: raw.base_pbf_size.get(),
            base_pbf_xxh3: raw.base_pbf_xxh3.get(),
            base_build_epoch: raw.base_build_epoch.get(),
            view_nodes: raw.view_nodes.get(),
            view_edges: raw.view_edges.get(),
            view_build_epoch: raw.view_build_epoch.get(),
            view_cells: raw.view_cells.get(),
            n_anchors: raw.n_anchors.get(),
            n_zone: raw.n_zone.get(),
            meta_len: raw.meta_len.get(),
            built_epoch: raw.built_epoch.get(),
        };
        if h.n_cats == 0 || h.n_cats > 8 || h.view_kind > 1 {
            return Err(corrupt(format!("n_cats {} ou type de vue {}", h.n_cats, h.view_kind)));
        }
        let (nc, na, nz, ml) =
            (usize::from(h.n_cats), h.n_anchors as usize, h.n_zone as usize, h.meta_len as usize);
        let cells = h.view_cells as usize + 1;
        let cats_at = HEADER_LEN;
        let a_off_at = cats_at + align8(nc * CAT_LEN);
        let a_node_at = a_off_at + align8(4 * cells);
        let a_cat_at = a_node_at + align8(4 * na);
        let z_off_at = a_cat_at + align8(na);
        let z_node_at = z_off_at + align8(4 * cells);
        let z_mask_at = z_node_at + align8(4 * nz);
        let meta_at = z_mask_at + align8(nz);
        let end = meta_at + align8(ml);
        if bytes.len() != end + 8 {
            return Err(AttrError::Truncated(format!("{} octets, {} attendus", bytes.len(), end + 8)));
        }
        let sum = u64::from_le_bytes(bytes[end..end + 8].try_into().expect("8 octets"));
        if xxh3_64(&bytes[..end]) != sum {
            return Err(corrupt("empreinte de fin fausse"));
        }
        let f = Self {
            data,
            header: h,
            cats_at,
            a_off_at,
            a_node_at,
            a_cat_at,
            z_off_at,
            z_node_at,
            z_mask_at,
            meta_at,
        };
        f.validate()?;
        Ok(f)
    }

    fn validate(&self) -> Result<(), AttrError> {
        let bytes = self.data.as_ref();
        let h = &self.header;
        let check_off = |name: &str, at: usize, n: u32| -> Result<(), AttrError> {
            let off = u32s(bytes, at, h.view_cells as usize + 1);
            if off[0].get() != 0 || off[h.view_cells as usize].get() != n {
                return Err(corrupt(format!("{name} : bornes")));
            }
            if off.windows(2).any(|w| w[0].get() > w[1].get()) {
                return Err(corrupt(format!("{name} : décalages décroissants")));
            }
            Ok(())
        };
        check_off("ANCHOR_OFF", self.a_off_at, h.n_anchors)?;
        check_off("ZONE_OFF", self.z_off_at, h.n_zone)?;
        let an = self.anchor_nodes();
        let ac = self.anchor_cats();
        for i in 0..an.len() {
            if ac[i] >= h.n_cats {
                return Err(corrupt("ANCHOR_CAT : catégorie hors table"));
            }
            if i > 0 && (an[i - 1].get(), ac[i - 1]) >= (an[i].get(), ac[i]) {
                return Err(corrupt("ancrages non triés ou en double"));
            }
        }
        let zn = self.zone_nodes();
        let zm = self.zone_masks();
        let limit = if h.n_cats == 8 { 0xFF } else { (1u16 << h.n_cats) - 1 } as u8;
        for i in 0..zn.len() {
            if zm[i] == 0 || zm[i] & !limit != 0 {
                return Err(corrupt("ZONE_MASK : masque nul ou bit hors table"));
            }
            if i > 0 && zn[i - 1].get() >= zn[i].get() {
                return Err(corrupt("nœuds de zone non strictement croissants"));
            }
        }
        // La table des catégories est figée par le format 1 : un bit connu doit porter le nom de `CATS` (un `minor` plus grand
        // peut en ajouter après, jamais renuméroter), sinon le moteur pèserait une catégorie pour une autre.
        let names = self.cats();
        if names.iter().zip(CATS).any(|(got, want)| got != want) {
            return Err(corrupt(format!("CATS : table {names:?}, attendue {CATS:?}")));
        }
        if serde_json::from_slice::<serde::de::IgnoredAny>(self.meta()).is_err() {
            return Err(corrupt("META n'est pas du JSON"));
        }
        Ok(())
    }

    pub fn header(&self) -> &AttrHeader {
        &self.header
    }

    pub fn reach_m(&self) -> u16 {
        self.header.reach_m
    }

    pub fn snap_m(&self) -> u16 {
        self.header.snap_m
    }

    pub fn n_anchors(&self) -> usize {
        self.header.n_anchors as usize
    }

    pub fn n_zone(&self) -> usize {
        self.header.n_zone as usize
    }

    pub fn len_bytes(&self) -> usize {
        self.data.as_ref().len()
    }

    /// Noms des catégories, dans l'ordre des bits.
    pub fn cats(&self) -> Vec<String> {
        let b = &self.data.as_ref()[self.cats_at..];
        (0..usize::from(self.header.n_cats))
            .map(|i| {
                let c = &b[i * CAT_LEN..(i + 1) * CAT_LEN];
                let n = c.iter().position(|&x| x == 0).unwrap_or(CAT_LEN);
                String::from_utf8_lossy(&c[..n]).into_owned()
            })
            .collect()
    }

    pub fn meta(&self) -> &[u8] {
        &self.data.as_ref()[self.meta_at..self.meta_at + self.header.meta_len as usize]
    }

    pub fn anchor_nodes(&self) -> &[U32] {
        u32s(self.data.as_ref(), self.a_node_at, self.n_anchors())
    }

    pub fn anchor_cats(&self) -> &[u8] {
        &self.data.as_ref()[self.a_cat_at..self.a_cat_at + self.n_anchors()]
    }

    /// Les ancrages `(nœud, catégorie)`, triés.
    pub fn anchors(&self) -> impl Iterator<Item = (u32, u8)> + '_ {
        self.anchor_nodes().iter().zip(self.anchor_cats()).map(|(n, &c)| (n.get(), c))
    }

    pub fn zone_nodes(&self) -> &[U32] {
        u32s(self.data.as_ref(), self.z_node_at, self.n_zone())
    }

    pub fn zone_masks(&self) -> &[u8] {
        &self.data.as_ref()[self.z_mask_at..self.z_mask_at + self.n_zone()]
    }

    fn zone_range(&self, cell: usize) -> (usize, usize) {
        let off = u32s(self.data.as_ref(), self.z_off_at, self.header.view_cells as usize + 1);
        (off[cell].get() as usize, off[cell + 1].get() as usize)
    }

    /// Nœuds de zone d'une cellule de la vue (ids croissants) et leurs masques.
    pub fn zone_of_cell(&self, cell: usize) -> (&[U32], &[u8]) {
        let (a, b) = self.zone_range(cell);
        (&self.zone_nodes()[a..b], &self.zone_masks()[a..b])
    }

    /// Masque d'un nœud (`cell` : sa cellule dans la vue) ; 0 hors zone.
    pub fn zone_mask(&self, cell: usize, gid: u32) -> u8 {
        let (nodes, masks) = self.zone_of_cell(cell);
        match nodes.binary_search_by_key(&gid, |n| n.get()) {
            Ok(i) => masks[i],
            Err(_) => 0,
        }
    }

    /// Le fichier est celui de ce graphe de base et de cette vue : les cinq champs de base, le type et les
    /// quatre champs de la vue, puis la place de chaque nœud dans sa cellule.
    pub fn check_for<B2: AsRef<[u8]>>(&self, base: &Header, view: &GraphFile<B2>) -> Result<(), AttrError> {
        let h = &self.header;
        let mism = |what: &str| Err(AttrError::Mismatch(what.to_string()));
        if (h.base_nodes, h.base_edges, h.base_pbf_size, h.base_pbf_xxh3, h.base_build_epoch)
            != (base.nodes, base.edges, base.pbf_size, base.pbf_xxh3, base.build_epoch)
        {
            return mism("graphe de base (comptes, extrait ou date de construction)");
        }
        let Some(meta) = view.view_meta() else { return mism("ce n'est pas une vue") };
        if kind_code(meta.kind) != Some(h.view_kind) || meta.prune_dead_ends {
            return mism("type de vue");
        }
        let vh = view.header();
        if (h.view_nodes, h.view_edges, h.view_build_epoch, u64::from(h.view_cells))
            != (vh.nodes, vh.edges, vh.build_epoch, view.n_cells() as u64)
        {
            return mism("vue (comptes, date de construction ou cellules)");
        }
        for c in 0..view.n_cells() {
            let info = view.cell_info(c);
            let (lo, hi) = (info.first_node, info.first_node + info.n_nodes);
            let (zn, _) = self.zone_of_cell(c);
            let a = self.anchor_range(c);
            let an = &self.anchor_nodes()[a.0..a.1];
            for seg in [zn, an] {
                if let (Some(f), Some(l)) = (seg.first(), seg.last()) {
                    if f.get() < lo || l.get() >= hi {
                        return mism("nœud hors de sa cellule");
                    }
                }
            }
        }
        Ok(())
    }

    fn anchor_range(&self, cell: usize) -> (usize, usize) {
        let off = u32s(self.data.as_ref(), self.a_off_at, self.header.view_cells as usize + 1);
        (off[cell].get() as usize, off[cell + 1].get() as usize)
    }

    /// Décalages et tableaux, pour la comparaison de fichiers.
    pub fn digest(&self) -> u64 {
        xxh3_64(self.data.as_ref())
    }
}

// --- Grille des ancrages (banc, contrôle) ---------------------------------------------------------------------------

/// Les ancrages d'un fichier, en grille de 0,005° : `hits` rend le masque des catégories dont un ancrage est à
/// `reach_m` au plus d'un point.
#[derive(Debug)]
pub struct AnchorGrid {
    grid: Grid,
    reach_m: f64,
}

impl AnchorGrid {
    pub fn new<B1: AsRef<[u8]>, B2: AsRef<[u8]>>(file: &AttrFile<B1>, view: &GraphFile<B2>) -> Self {
        let entries = file
            .anchors()
            .map(|(node, cat)| {
                let (lat, lon) = view.node_pos(node);
                Entry { key: cell_key(lat, lon), id: node, lat, lon, tag: cat }
            })
            .collect();
        Self { grid: Grid::new(entries), reach_m: f64::from(file.reach_m()) }
    }

    pub fn hits(&self, lat_e7: i32, lon_e7: i32) -> u8 {
        self.hits_within(lat_e7, lon_e7, self.reach_m)
    }

    /// Comme `hits`, à `radius_m` (le banc rapporte aussi la part à 1 km).
    pub fn hits_within(&self, lat_e7: i32, lon_e7: i32, radius_m: f64) -> u8 {
        let mut mask = 0u8;
        self.grid.within(lat_e7, lon_e7, radius_m, |e| mask |= 1 << e.tag);
        mask
    }
}
