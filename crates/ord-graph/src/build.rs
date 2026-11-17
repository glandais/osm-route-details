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

/// Reçoit les ways puis les relations, dans l'ordre des ids. Une erreur arrête le parcours : la source la rend
/// telle quelle, sans lire la suite.
pub trait ElementSink {
    fn way(&mut self, way: &OsmWay<'_>) -> Result<(), BuildError>;
    fn relation(&mut self, relation: &OsmRelation<'_>) -> Result<(), BuildError>;
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
            sink.way(&OsmWay { id: *id, refs, tags: &t })?;
        }
        let mut rels: Vec<_> = self.relations.iter().collect();
        rels.sort_by_key(|r| r.0);
        for (id, tags, members) in rels {
            let t: Vec<(&str, &str)> = tags.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            sink.relation(&OsmRelation { id: *id, tags: &t, way_members: members })?;
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

/// Filtre d'une way (passe 1), commun aux deux constructions : ordre des ids, filtre vélo, références
/// consécutives dédoublonnées ajoutées à la suite de `refs`. Rend les attributs d'une way gardée ; `None` si elle
/// est rejetée (`refs` est alors inchangé).
pub(crate) fn keep_way(
    w: &OsmWay<'_>,
    last_way: &mut i64,
    stats: &mut BuildStats,
    refs: &mut Vec<i64>,
) -> Result<Option<u64>, BuildError> {
    if w.id <= *last_way {
        return Err(BuildError::NotSorted(format!("way {} après {}", w.id, last_way)));
    }
    *last_way = w.id;
    stats.ways_total += 1;
    let info = match classify(w.tags) {
        Ok(i) => i,
        Err(r) => {
            *stats.rejects.entry(r.label()).or_default() += 1;
            return Ok(None);
        }
    };
    let start = refs.len();
    for &r in w.refs {
        if refs.len() == start || refs.last() != Some(&r) {
            refs.push(r);
        }
    }
    if refs.len() - start < 2 {
        refs.truncate(start);
        *stats.rejects.entry("moins de deux nœuds distincts".into()).or_default() += 1;
        return Ok(None);
    }
    let n = &info.notes;
    stats.oneway_reversible += u64::from(n.reversible);
    stats.ignored_conditional += u64::from(n.conditional);
    stats.ignored_bicycle_directional += u64::from(n.bicycle_directional);
    stats.ignored_motor_vehicle += u64::from(n.motor_vehicle);
    Ok(Some(info.attrs.0))
}

/// Relation (passe 1), commune aux deux constructions : ordre des ids, puis les bits d'itinéraire à poser sur
/// ses membres, `None` si ce n'est pas un itinéraire retenu.
pub(crate) fn route_relation(
    r: &OsmRelation<'_>,
    last_relation: &mut i64,
    stats: &mut BuildStats,
) -> Result<Option<u8>, BuildError> {
    if r.id <= *last_relation {
        return Err(BuildError::NotSorted(format!("relation {} après {}", r.id, last_relation)));
    }
    *last_relation = r.id;
    stats.relations_total += 1;
    let bits = route_bits(r.tags);
    stats.relations_routes += u64::from(bits.is_some());
    Ok(bits)
}

/// Passe 1 : ways filtrées puis relations.
struct Pass1 {
    ways: Vec<WayRec>,
    refs: Vec<i64>,
    last_way: i64,
    last_relation: i64,
    stats: BuildStats,
}

impl ElementSink for Pass1 {
    fn way(&mut self, w: &OsmWay<'_>) -> Result<(), BuildError> {
        let start = self.refs.len();
        let Some(attrs) = keep_way(w, &mut self.last_way, &mut self.stats, &mut self.refs)? else {
            return Ok(());
        };
        if self.refs.len() >= u32::MAX as usize {
            return Err(BuildError::TooBig("plus de 2^32 références de nœud".into()));
        }
        self.ways.push(WayRec { id: w.id, attrs, start: start as u32 });
        Ok(())
    }

    fn relation(&mut self, r: &OsmRelation<'_>) -> Result<(), BuildError> {
        let Some(bits) = route_relation(r, &mut self.last_relation, &mut self.stats)? else { return Ok(()) };
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
        Ok(())
    }
}

pub(crate) fn haversine_m(a: (i32, i32), b: (i32, i32)) -> f64 {
    const R: f64 = 6_371_008.8;
    let (p1, p2) = (f64::from(a.0) * 1e-7, f64::from(b.0) * 1e-7);
    let (dphi, dl) = ((p2 - p1).to_radians(), (f64::from(b.1) - f64::from(a.1)).to_radians() * 1e-7);
    let h =
        (dphi / 2.0).sin().powi(2) + p1.to_radians().cos() * p2.to_radians().cos() * (dl / 2.0).sin().powi(2);
    2.0 * R * h.sqrt().min(1.0).asin()
}

/// Longueur d'une ligne de points E7, en décimètres (haversine, arrondi au plus proche).
pub(crate) fn len_dm(mut points: impl Iterator<Item = (i32, i32)>) -> u32 {
    let mut len = 0.0;
    if let Some(mut prev) = points.next() {
        for p in points {
            len += haversine_m(prev, p);
            prev = p;
        }
    }
    (len * 10.0).round().min(f64::from(u32::MAX)) as u32
}

/// Découpe d'une way de `n` nœuds en arêtes, commune aux deux constructions. `present(k)` : le nœud de rang `k`
/// est dans l'extrait ; `multi(k)` : il apparaît au moins deux fois dans les ways gardées ; `same(i, k)` : même
/// nœud aux deux rangs. `emit(lo, hi)` reçoit chaque arête, par ses rangs dans la way.
///
/// Segments continus de nœuds présents, coupés aux jonctions (`multi`) et à leurs bouts ; une boucle est coupée à
/// son nœud intérieur médian. Ni les bouts de segment ni ce milieu n'ont besoin d'un drapeau vu des autres ways :
/// un nœud revu ailleurs, ou plus loin dans la même way, est déjà `multi`.
pub(crate) fn cut_way(
    n: usize,
    present: impl Fn(usize) -> bool,
    multi: impl Fn(usize) -> bool,
    same: impl Fn(usize, usize) -> bool,
    stats: &mut BuildStats,
    mut emit: impl FnMut(usize, usize) -> Result<(), BuildError>,
) -> Result<(), BuildError> {
    let (mut run, mut cut) = (None::<usize>, false);
    for k in 0..=n {
        if k < n && present(k) {
            run.get_or_insert(k);
            continue;
        }
        if k < n {
            cut = true;
            stats.nodes_referenced_missing += 1;
        }
        let Some(lo) = run.take() else { continue };
        let hi = k - 1;
        let mut start = lo;
        for j in lo + 1..=hi {
            if !multi(j) && j != hi {
                continue;
            }
            if same(start, j) && j - start >= 2 {
                let m = start + (j - start) / 2;
                stats.loops_cut += 1;
                emit(start, m)?;
                emit(m, j)?;
            } else {
                emit(start, j)?;
            }
            start = j;
        }
    }
    stats.ways_cut_by_missing_nodes += u64::from(cut);
    Ok(())
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
    let mut p1 =
        Pass1 { ways: Vec::new(), refs: Vec::new(), last_way: i64::MIN, last_relation: i64::MIN, stats };
    src.scan(&mut p1)?;
    let Pass1 { ways, refs: refs_i64, stats: s, .. } = p1;
    stats = s;
    stats.ways_kept = ways.len() as u64;
    lap(&mut stats, &mut clock, "passe 1 : ways et relations");

    // Nœuds utilisés : tri d'une copie, séries, drapeau « apparaît au moins deux fois ».
    let mut sorted = refs_i64.clone();
    sort_i64(&mut sorted);
    let mut uniq: Vec<i64> = Vec::new();
    let mut multi: Vec<bool> = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let mut j = i + 1;
        while j < sorted.len() && sorted[j] == sorted[i] {
            j += 1;
        }
        uniq.push(sorted[i]);
        multi.push(j - i >= 2);
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

    // Découpe way par way : segments de nœuds présents, coupe aux jonctions.
    let coord = |u: u32| unpack_coord(coords[u as usize].load(Ordering::Relaxed));
    let mut raw: Vec<RawEdge> = Vec::new();
    for (wi, w) in ways.iter().enumerate() {
        let start = w.start as usize;
        let end = ways.get(wi + 1).map_or(refs.len(), |n| n.start as usize);
        let r = &refs[start..end];
        cut_way(
            r.len(),
            |k| !is_missing(coords[r[k] as usize].load(Ordering::Relaxed)),
            |k| multi[r[k] as usize],
            |i, k| r[i] == r[k],
            &mut stats,
            |lo, hi| {
                raw.push(RawEdge {
                    way: wi as u32,
                    a: r[lo],
                    b: r[hi],
                    lo: (start + lo) as u32,
                    hi: (start + hi) as u32,
                    len_dm: len_dm(r[lo..=hi].iter().map(|&u| coord(u))),
                });
                Ok(())
            },
        )?;
    }
    drop(multi);
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
