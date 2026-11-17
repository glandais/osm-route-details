//! Construction du graphe en mémoire bornée (§2 de la note) : deux passes sur une source OSM abstraite,
//! contraction aux jonctions, numérotation en cellules (ordre de Morton).
//!
//! La source (`OsmSource`) est abstraite pour tester à la main et pour compiler sans `osmpbf`.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::attrs::{classify, route_bits, Attrs};
use crate::grid;
use crate::stats::{BuildStats, PhaseClock};

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("lecture de la source : {0}")]
    Source(String),
    #[error("source non triée (Type_then_ID) : {0}")]
    NotSorted(String),
    #[error("trop gros pour le format : {0}")]
    TooBig(String),
    #[error("fichier temporaire : {0}")]
    Io(#[from] std::io::Error),
}

/// Une way telle que la source la livre.
#[derive(Clone, Copy, Debug)]
pub struct OsmWay<'a> {
    pub id: i64,
    pub refs: &'a [i64],
    pub tags: &'a [(&'a str, &'a str)],
}

/// Une relation, réduite à ses membres de type way.
#[derive(Clone, Copy, Debug)]
pub struct OsmRelation<'a> {
    pub id: i64,
    pub tags: &'a [(&'a str, &'a str)],
    pub way_members: &'a [i64],
}

/// Reçoit les ways puis les relations, dans l'ordre des ids.
pub trait ElementSink {
    fn way(&mut self, way: &OsmWay<'_>);
    fn relation(&mut self, relation: &OsmRelation<'_>);
}

/// Ce que l'extrait dit de lui-même, recopié dans l'en-tête du fichier.
#[derive(Clone, Copy, Debug, Default)]
pub struct SourceMeta {
    pub replication_timestamp: i64,
    pub replication_sequence: i64,
    pub size: u64,
    pub xxh3: u64,
}

/// Reçoit un lot de nœuds `(id, décimicro-latitude, décimicro-longitude)` par id croissant.
pub type NodeBatchFn<'a> = dyn FnMut(&[(i64, i32, i32)]) -> Result<(), BuildError> + 'a;

pub trait OsmSource {
    fn meta(&self) -> SourceMeta;
    /// Passe 1 : toutes les ways, puis toutes les relations, par id croissant.
    fn scan(&mut self, sink: &mut dyn ElementSink) -> Result<(), BuildError>;
    /// Passe 2 : les nœuds `(id, décimicro-latitude, décimicro-longitude)`, dans n'importe quel ordre
    /// et éventuellement en parallèle (d'où `Sync`).
    fn nodes(&mut self, f: &(dyn Fn(i64, i32, i32) + Sync)) -> Result<(), BuildError>;
    /// Comme `nodes`, avec les étiquettes du nœud (vides pour la plupart) : `f(id, lat, lon, étiquettes)`.
    /// Sert à l'extraction des attracteurs (lot 4d) ; la construction du graphe n'en a pas besoin.
    fn tagged_nodes(&mut self, f: &TaggedNodeFn<'_>) -> Result<(), BuildError>;
    /// Passe 2 de la construction hors mémoire : les nœuds par **id croissant**, livrés par lots dans l'ordre.
    /// Par défaut : tout `nodes`, trié, en un lot (sources en mémoire seulement).
    fn nodes_sorted(&mut self, f: &mut NodeBatchFn<'_>) -> Result<(), BuildError> {
        let all = std::sync::Mutex::new(Vec::new());
        self.nodes(&|id, lat, lon| all.lock().expect("verrou des nœuds").push((id, lat, lon)))?;
        let mut all = all.into_inner().expect("verrou des nœuds");
        all.sort_unstable();
        f(&all)
    }
}

/// Reçoit un nœud et ses étiquettes (`OsmSource::tagged_nodes`).
pub type TaggedNodeFn<'a> = dyn Fn(i64, i32, i32, &[(&str, &str)]) + Sync + 'a;

/// Source en mémoire, pour les tests et les petits cas à la main.
#[derive(Clone, Debug, Default)]
pub struct MemSource {
    pub nodes: Vec<(i64, i32, i32)>,
    pub ways: Vec<MemWay>,
    pub relations: Vec<MemRelation>,
    /// Étiquettes de certains nœuds (lot 4d) : `(id, étiquettes)`.
    pub node_tags: Vec<MemNodeTags>,
}

/// Les étiquettes d'un nœud de `MemSource`.
pub type MemNodeTags = (i64, Vec<(String, String)>);

/// Une way de `MemSource` : id, nœuds, tags.
pub type MemWay = (i64, Vec<i64>, Vec<(String, String)>);
/// Une relation de `MemSource` : id, tags, membres de type way.
pub type MemRelation = (i64, Vec<(String, String)>, Vec<i64>);

fn owned_tags(tags: &[(&str, &str)]) -> Vec<(String, String)> {
    tags.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect()
}

impl MemSource {
    /// Nœud en degrés (convertis en décimicro-degrés).
    pub fn node(&mut self, id: i64, lat: f64, lon: f64) -> &mut Self {
        self.nodes.push((id, (lat * 1e7).round() as i32, (lon * 1e7).round() as i32));
        self
    }
    pub fn way(&mut self, id: i64, refs: &[i64], tags: &[(&str, &str)]) -> &mut Self {
        self.ways.push((id, refs.to_vec(), owned_tags(tags)));
        self
    }
    /// Étiquettes d'un nœud déjà ajouté par `node`.
    pub fn node_tag(&mut self, id: i64, tags: &[(&str, &str)]) -> &mut Self {
        self.node_tags.push((id, owned_tags(tags)));
        self
    }
    pub fn relation(&mut self, id: i64, tags: &[(&str, &str)], way_members: &[i64]) -> &mut Self {
        self.relations.push((id, owned_tags(tags), way_members.to_vec()));
        self
    }
}

impl OsmSource for MemSource {
    fn meta(&self) -> SourceMeta {
        SourceMeta::default()
    }

    fn scan(&mut self, sink: &mut dyn ElementSink) -> Result<(), BuildError> {
        let mut ways: Vec<_> = self.ways.iter().collect();
        ways.sort_by_key(|w| w.0);
        for (id, refs, tags) in ways {
            let t: Vec<(&str, &str)> = tags.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            sink.way(&OsmWay { id: *id, refs, tags: &t });
        }
        let mut rels: Vec<_> = self.relations.iter().collect();
        rels.sort_by_key(|r| r.0);
        for (id, tags, members) in rels {
            let t: Vec<(&str, &str)> = tags.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            sink.relation(&OsmRelation { id: *id, tags: &t, way_members: members });
        }
        Ok(())
    }

    fn nodes(&mut self, f: &(dyn Fn(i64, i32, i32) + Sync)) -> Result<(), BuildError> {
        for &(id, lat, lon) in &self.nodes {
            f(id, lat, lon);
        }
        Ok(())
    }

    fn tagged_nodes(&mut self, f: &TaggedNodeFn<'_>) -> Result<(), BuildError> {
        for &(id, lat, lon) in &self.nodes {
            let tags: Vec<(&str, &str)> = self
                .node_tags
                .iter()
                .filter(|(nid, _)| *nid == id)
                .flat_map(|(_, t)| t.iter().map(|(k, v)| (k.as_str(), v.as_str())))
                .collect();
            f(id, lat, lon, &tags);
        }
        Ok(())
    }
}

// --- Résultat de la construction ---------------------------------------------------------------

/// Un sommet du graphe (jonction ou extrémité), numéroté par cellule en ordre de Morton.
#[derive(Clone, Copy, Debug)]
pub struct NodeData {
    pub osm_id: i64,
    pub lat: i32,
    pub lon: i32,
}

/// Une cellule de la grille : nœuds `first_node..first_node + n_nodes`.
#[derive(Clone, Copy, Debug)]
pub struct CellSpec {
    pub x: u32,
    pub y: u32,
    pub first_node: u32,
    pub n_nodes: u32,
}

/// Une arête non orientée : way OSM, extrémités (ids globaux), géométrie = plage `lo..=hi` de `refs`.
#[derive(Clone, Copy, Debug)]
pub struct EdgeData {
    pub way_id: i64,
    pub a: u32,
    pub b: u32,
    pub attrs: u64,
    pub len_dm: u32,
    pub lo: u32,
    pub hi: u32,
}

/// Section supplémentaire écrite telle quelle après les identifiants (sections d'une vue).
#[derive(Clone, Debug)]
pub struct ExtraSection {
    pub kind: u32,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub struct GraphData {
    pub zoom: u8,
    pub nodes: Vec<NodeData>,
    pub cells: Vec<CellSpec>,
    pub edges: Vec<EdgeData>,
    /// Indices de nœuds utilisés, concaténés par segment ; les arêtes en désignent des plages.
    pub refs: Vec<u32>,
    /// Coordonnée de chaque nœud utilisé (indexée comme `refs`), `lat << 32 | lon`.
    pub coords: Vec<AtomicU64>,
    pub stats: BuildStats,
    pub meta: SourceMeta,
    /// Sections optionnelles (vide pour le graphe de base).
    pub extras: Vec<ExtraSection>,
}

const MISSING: u32 = 0x8000_0000;

fn pack_coord(lat: i32, lon: i32) -> u64 {
    (u64::from(lat as u32) << 32) | u64::from(lon as u32)
}

fn unpack_coord(v: u64) -> (i32, i32) {
    ((v >> 32) as u32 as i32, v as u32 as i32)
}

fn is_missing(v: u64) -> bool {
    (v >> 32) as u32 == MISSING
}

impl GraphData {
    /// Points intérieurs d'une arête (extrémités exclues), en E7.
    pub fn interior(&self, e: &EdgeData) -> Vec<(i32, i32)> {
        self.refs[e.lo as usize + 1..e.hi as usize]
            .iter()
            .map(|&r| unpack_coord(self.coords[r as usize].load(Ordering::Relaxed)))
            .collect()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BuildOptions {
    /// Zoom de la grille de cellules (tuiles Web Mercator).
    pub zoom: u8,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self { zoom: 11 }
    }
}

#[cfg(feature = "parallel")]
fn sort_unstable_by_key<T: Send, K: Ord, F: Fn(&T) -> K + Sync>(v: &mut [T], f: F) {
    use rayon::slice::ParallelSliceMut;
    v.par_sort_unstable_by_key(f);
}

#[cfg(not(feature = "parallel"))]
fn sort_unstable_by_key<T, K: Ord, F: Fn(&T) -> K>(v: &mut [T], f: F) {
    v.sort_unstable_by_key(f);
}

#[cfg(feature = "parallel")]
fn sort_i64(v: &mut [i64]) {
    use rayon::slice::ParallelSliceMut;
    v.par_sort_unstable();
}

#[cfg(not(feature = "parallel"))]
fn sort_i64(v: &mut [i64]) {
    v.sort_unstable();
}

#[cfg(feature = "parallel")]
fn map_refs(refs: &[i64], uniq: &[i64]) -> Vec<u32> {
    use rayon::prelude::*;
    refs.par_iter().map(|id| uniq.binary_search(id).expect("référence présente") as u32).collect()
}

#[cfg(not(feature = "parallel"))]
fn map_refs(refs: &[i64], uniq: &[i64]) -> Vec<u32> {
    refs.iter().map(|id| uniq.binary_search(id).expect("référence présente") as u32).collect()
}

struct WayRec {
    id: i64,
    attrs: u64,
    start: u32,
}

/// Passe 1 : ways filtrées puis relations.
struct Pass1 {
    ways: Vec<WayRec>,
    refs: Vec<i64>,
    last_way: i64,
    last_relation: i64,
    err: Option<BuildError>,
    stats: BuildStats,
}

impl ElementSink for Pass1 {
    fn way(&mut self, w: &OsmWay<'_>) {
        if self.err.is_some() {
            return;
        }
        if w.id <= self.last_way {
            self.err = Some(BuildError::NotSorted(format!("way {} après {}", w.id, self.last_way)));
            return;
        }
        self.last_way = w.id;
        self.stats.ways_total += 1;
        let info = match classify(w.tags) {
            Ok(i) => i,
            Err(r) => {
                *self.stats.rejects.entry(r.label()).or_default() += 1;
                return;
            }
        };
        let start = self.refs.len();
        for &r in w.refs {
            if self.refs.len() == start || self.refs.last() != Some(&r) {
                self.refs.push(r);
            }
        }
        if self.refs.len() - start < 2 {
            self.refs.truncate(start);
            *self.stats.rejects.entry("moins de deux nœuds distincts".into()).or_default() += 1;
            return;
        }
        if self.refs.len() >= u32::MAX as usize {
            self.err = Some(BuildError::TooBig("plus de 2^32 références de nœud".into()));
            return;
        }
        let n = &info.notes;
        self.stats.oneway_reversible += u64::from(n.reversible);
        self.stats.ignored_conditional += u64::from(n.conditional);
        self.stats.ignored_bicycle_directional += u64::from(n.bicycle_directional);
        self.stats.ignored_motor_vehicle += u64::from(n.motor_vehicle);
        self.ways.push(WayRec { id: w.id, attrs: info.attrs.0, start: start as u32 });
    }

    fn relation(&mut self, r: &OsmRelation<'_>) {
        if self.err.is_some() {
            return;
        }
        if r.id <= self.last_relation {
            self.err = Some(BuildError::NotSorted(format!("relation {} après {}", r.id, self.last_relation)));
            return;
        }
        self.last_relation = r.id;
        self.stats.relations_total += 1;
        let Some(bits) = route_bits(r.tags) else { return };
        self.stats.relations_routes += 1;
        for &m in r.way_members {
            match self.ways.binary_search_by_key(&m, |w| w.id) {
                Ok(i) => {
                    let w = &mut self.ways[i];
                    w.attrs = Attrs(w.attrs).with_routes(bits).0;
                    self.stats.relation_way_members_matched += 1;
                }
                Err(_) => self.stats.relation_way_members_not_in_graph += 1,
            }
        }
    }
}

const FLAG_MULTI: u8 = 1;
const FLAG_END: u8 = 2;
const FLAG_LOOP: u8 = 4;

pub(crate) fn haversine_m(a: (i32, i32), b: (i32, i32)) -> f64 {
    const R: f64 = 6_371_008.8;
    let (p1, p2) = (f64::from(a.0) * 1e-7, f64::from(b.0) * 1e-7);
    let (dphi, dl) = ((p2 - p1).to_radians(), (f64::from(b.1) - f64::from(a.1)).to_radians() * 1e-7);
    let h =
        (dphi / 2.0).sin().powi(2) + p1.to_radians().cos() * p2.to_radians().cos() * (dl / 2.0).sin().powi(2);
    2.0 * R * h.sqrt().min(1.0).asin()
}

struct Seg {
    way: u32,
    lo: u32,
    hi: u32,
}

struct RawEdge {
    way: u32,
    a: u32,
    b: u32,
    lo: u32,
    hi: u32,
    len_dm: u32,
}

/// Construit le graphe d'une source. `on_phase` est appelé en fin de chaque phase (pour la trace).
pub fn build_graph(
    src: &mut dyn OsmSource,
    opts: &BuildOptions,
    region: &str,
    on_phase: &mut dyn FnMut(&str),
) -> Result<GraphData, BuildError> {
    // Les coordonnées de tuile tiennent sur 16 bits dans l'index des cellules.
    if !(1..=16).contains(&opts.zoom) {
        return Err(BuildError::TooBig(format!("zoom {} hors de 1..=16", opts.zoom)));
    }
    let mut clock = PhaseClock::new();
    let mut stats = BuildStats { region: region.to_string(), ..Default::default() };
    #[cfg(feature = "parallel")]
    {
        stats.threads = rayon::current_num_threads();
    }
    #[cfg(not(feature = "parallel"))]
    {
        stats.threads = 1;
    }
    let mut lap = |stats: &mut BuildStats, clock: &mut PhaseClock, name: &str| {
        stats.phases.push(clock.lap(name));
        on_phase(name);
    };
    let meta = src.meta();

    // Passe 1 : ways puis relations.
    let mut p1 = Pass1 {
        ways: Vec::new(),
        refs: Vec::new(),
        last_way: i64::MIN,
        last_relation: i64::MIN,
        err: None,
        stats,
    };
    src.scan(&mut p1)?;
    if let Some(e) = p1.err.take() {
        return Err(e);
    }
    let Pass1 { ways, refs: refs_i64, stats: s, .. } = p1;
    stats = s;
    stats.ways_kept = ways.len() as u64;
    lap(&mut stats, &mut clock, "passe 1 : ways et relations");

    // Nœuds utilisés : tri d'une copie, séries, drapeau « apparaît au moins deux fois ».
    let mut sorted = refs_i64.clone();
    sort_i64(&mut sorted);
    let mut uniq: Vec<i64> = Vec::new();
    let mut flags: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let mut j = i + 1;
        while j < sorted.len() && sorted[j] == sorted[i] {
            j += 1;
        }
        uniq.push(sorted[i]);
        flags.push(if j - i >= 2 { FLAG_MULTI } else { 0 });
        i = j;
    }
    drop(sorted);
    if uniq.len() >= (1 << 31) {
        return Err(BuildError::TooBig("plus de 2^31 nœuds utilisés".into()));
    }
    let refs: Vec<u32> = map_refs(&refs_i64, &uniq);
    drop(refs_i64);
    stats.used_nodes = uniq.len() as u64;
    lap(&mut stats, &mut clock, "nœuds utilisés (tri, jonctions)");

    // Passe 2 : coordonnées des seuls nœuds utilisés.
    let coords: Vec<AtomicU64> = (0..uniq.len()).map(|_| AtomicU64::new(pack_coord(i32::MIN, 0))).collect();
    let (min_id, max_id) = (uniq.first().copied().unwrap_or(0), uniq.last().copied().unwrap_or(-1));
    src.nodes(&|id, lat, lon| {
        if id < min_id || id > max_id {
            return;
        }
        if let Ok(i) = uniq.binary_search(&id) {
            coords[i].store(pack_coord(lat, lon), Ordering::Relaxed);
        }
    })?;
    lap(&mut stats, &mut clock, "passe 2 : nœuds");

    // Segments continus de nœuds présents : une way dont un nœud manque est coupée.
    let mut segs: Vec<Seg> = Vec::with_capacity(ways.len());
    for (wi, w) in ways.iter().enumerate() {
        let end = ways.get(wi + 1).map_or(refs.len(), |n| n.start as usize);
        let (mut run, mut cut) = (None::<usize>, false);
        for k in w.start as usize..=end {
            let present = k < end && !is_missing(coords[refs[k] as usize].load(Ordering::Relaxed));
            if present {
                run.get_or_insert(k);
                continue;
            }
            if k < end {
                cut = true;
                stats.nodes_referenced_missing += 1;
            }
            if let Some(lo) = run.take() {
                if k - 1 > lo {
                    segs.push(Seg { way: wi as u32, lo: lo as u32, hi: (k - 1) as u32 });
                    flags[refs[lo] as usize] |= FLAG_END;
                    flags[refs[k - 1] as usize] |= FLAG_END;
                }
            }
        }
        stats.ways_cut_by_missing_nodes += u64::from(cut);
    }

    // Découpe aux jonctions.
    let coord = |u: u32| unpack_coord(coords[u as usize].load(Ordering::Relaxed));
    let mut raw: Vec<RawEdge> = Vec::new();
    let emit = |raw: &mut Vec<RawEdge>, way: u32, lo: u32, hi: u32| {
        let mut len = 0.0;
        for k in lo..hi {
            len += haversine_m(coord(refs[k as usize]), coord(refs[k as usize + 1]));
        }
        let len_dm = (len * 10.0).round().min(f64::from(u32::MAX)) as u32;
        raw.push(RawEdge { way, a: refs[lo as usize], b: refs[hi as usize], lo, hi, len_dm });
    };
    for seg in &segs {
        let mut start = seg.lo;
        for k in seg.lo + 1..=seg.hi {
            if flags[refs[k as usize] as usize] == 0 {
                continue;
            }
            let (a, b) = (refs[start as usize], refs[k as usize]);
            if a == b && k - start >= 2 {
                // Boucle : coupée à son nœud intérieur médian, promu jonction.
                let m = start + (k - start) / 2;
                flags[refs[m as usize] as usize] |= FLAG_LOOP;
                stats.loops_cut += 1;
                emit(&mut raw, seg.way, start, m);
                emit(&mut raw, seg.way, m, k);
            } else {
                emit(&mut raw, seg.way, start, k);
            }
            start = k;
        }
    }
    drop(segs);
    lap(&mut stats, &mut clock, "découpe en arêtes");

    // Sommets : ceux qui portent une arête, triés (cellule en Morton, Morton du point, id OSM).
    let mut used = vec![false; uniq.len()];
    for e in &raw {
        used[e.a as usize] = true;
        used[e.b as usize] = true;
    }
    let zoom = opts.zoom;
    let mut verts: Vec<(u64, u64, i64, u32)> = used
        .iter()
        .enumerate()
        .filter(|(_, u)| **u)
        .map(|(i, _)| {
            let (lat, lon) = coord(i as u32);
            let (x, y) = grid::tile_of_e7(lat, lon, zoom);
            (grid::morton(x, y), grid::morton_e7(lat, lon), uniq[i], i as u32)
        })
        .collect();
    drop(used);
    sort_unstable_by_key(&mut verts, |v| (v.0, v.1, v.2));
    let mut gid_of = vec![u32::MAX; uniq.len()];
    let mut nodes = Vec::with_capacity(verts.len());
    let mut cells: Vec<CellSpec> = Vec::new();
    let mut last_cell_key = u64::MAX;
    for (gid, &(ck, _, osm_id, u)) in verts.iter().enumerate() {
        gid_of[u as usize] = gid as u32;
        let (lat, lon) = coord(u);
        nodes.push(NodeData { osm_id, lat, lon });
        if ck != last_cell_key || cells.is_empty() {
            let (x, y) = grid::tile_of_e7(lat, lon, zoom);
            cells.push(CellSpec { x, y, first_node: gid as u32, n_nodes: 0 });
            last_cell_key = ck;
        }
        cells.last_mut().expect("cellule ouverte").n_nodes += 1;
    }
    drop(verts);
    drop(uniq);

    let mut edges: Vec<EdgeData> = raw
        .iter()
        .map(|e| EdgeData {
            way_id: ways[e.way as usize].id,
            a: gid_of[e.a as usize],
            b: gid_of[e.b as usize],
            attrs: ways[e.way as usize].attrs,
            len_dm: e.len_dm,
            lo: e.lo,
            hi: e.hi,
        })
        .collect();
    drop(raw);
    drop(gid_of);
    sort_unstable_by_key(&mut edges, |e| (e.a, e.way_id, e.b, e.lo));
    stats.duplicate_identities =
        edges.windows(2).filter(|w| (w[0].a, w[0].way_id, w[0].b) == (w[1].a, w[1].way_id, w[1].b)).count()
            as u64;
    stats.vertices = nodes.len() as u64;
    stats.edges = edges.len() as u64;
    lap(&mut stats, &mut clock, "sommets, cellules, tri des arêtes");

    Ok(GraphData { zoom, nodes, cells, edges, refs, coords, stats, meta, extras: Vec::new() })
}
