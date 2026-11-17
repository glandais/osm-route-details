//! Construction **hors mémoire** du graphe (`docs/design/technique/ord-graph-build-hors-memoire.md`) : mêmes
//! octets que [`crate::build::build_graph`] suivi de [`crate::format::write_graph`], mais chaque grande structure
//! vit dans un fichier temporaire, parcouru en flux ou à travers un tri externe ([`crate::extsort`]). La mémoire
//! ne tient que les tampons des tris (bornés par le budget), le lot de blobs PBF décodés, la table des cellules et
//! le bloc de cellule en cours d'écriture.
//!
//! Étapes (note §5) :
//! - **E1** passe 1 : ways gardées (`ways`), références `(nœud, way, rang)`, membres d'itinéraire ;
//! - **E2–E3** passe 2 : jointure par fusion des références triées par nœud avec le flux des nœuds (id croissant),
//!   d'où coordonnées et jonctions (`multi` : au moins deux occurrences), puis retour à l'ordre (way, rang) ;
//! - **E4** découpe way par way (drapeaux de bout et de boucle locaux, note §5 E3), géométrie codée, sommets ;
//! - **E5** sommets triés (cellule, Morton, id OSM) : ids globaux, cellules ;
//! - **E6** renumérotation des extrémités, puis tri final des arêtes `(a, way, b, rang)` ;
//! - **E7** arêtes orientées, géométrie redistribuée par paquets dans l'ordre des arêtes, écriture en flux.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use crate::attrs::{classify, route_bits, Attrs};
use crate::build::{haversine_m, BuildError, CellSpec, ElementSink, OsmRelation, OsmSource, OsmWay};
use crate::extsort::{ExtSort, Peekable, RecReader, RecWriter, Record, TmpDir, IO_BUF};
use crate::format::{encode_geometry, FormatError, StreamHeader, StreamWriter, DIR_BIT};
use crate::grid;
use crate::stats::{BuildStats, PhaseClock};

/// Réglages de la construction hors mémoire.
#[derive(Clone, Debug)]
pub struct ExternalOptions {
    /// Zoom de la grille de cellules.
    pub zoom: u8,
    /// Budget des tampons de tri, en octets (réparti entre les tris actifs d'une étape).
    pub mem_budget: usize,
    /// Dossier où créer le dossier temporaire `.ordg-build-<pid>/`.
    pub tmp_parent: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum ExternalError {
    #[error(transparent)]
    Build(#[from] BuildError),
    #[error(transparent)]
    Format(#[from] FormatError),
}

impl From<std::io::Error> for ExternalError {
    fn from(e: std::io::Error) -> Self {
        Self::Build(BuildError::Io(e))
    }
}

/// Ce que rend la construction : compteurs (ceux de `build_graph`), pic du disque temporaire, séries par tri.
#[derive(Debug)]
pub struct ExternalReport {
    pub stats: BuildStats,
    pub tmp_peak_bytes: u64,
    pub runs: Vec<(String, u64)>,
}

/// Latitude d'une référence dont le nœud manque dans l'extrait (comme `MISSING` de `build.rs`).
const MISSING_LAT: i32 = i32::MIN;

// --- Enregistrements -----------------------------------------------------------------------------

fn rd_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().expect("4 octets"))
}
fn rd_i32(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes(b[o..o + 4].try_into().expect("4 octets"))
}
fn rd_u64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().expect("8 octets"))
}
fn rd_i64(b: &[u8], o: usize) -> i64 {
    i64::from_le_bytes(b[o..o + 8].try_into().expect("8 octets"))
}

/// Way gardée, dans l'ordre des ids : son indice est son rang dans le fichier.
#[derive(Clone, Copy, Debug)]
struct WayFile {
    id: i64,
    attrs: u64,
    n: u32,
}

impl Record for WayFile {
    const SIZE: usize = 20;
    fn put(&self, o: &mut [u8]) {
        o[0..8].copy_from_slice(&self.id.to_le_bytes());
        o[8..16].copy_from_slice(&self.attrs.to_le_bytes());
        o[16..20].copy_from_slice(&self.n.to_le_bytes());
    }
    fn get(b: &[u8]) -> Self {
        Self { id: rd_i64(b, 0), attrs: rd_u64(b, 8), n: rd_u32(b, 16) }
    }
}

/// Référence d'une way à un nœud ; clé totale `(nœud, way, rang)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct WayNode {
    node: i64,
    way: u32,
    rank: u32,
}

impl Record for WayNode {
    const SIZE: usize = 16;
    fn put(&self, o: &mut [u8]) {
        o[0..8].copy_from_slice(&self.node.to_le_bytes());
        o[8..12].copy_from_slice(&self.way.to_le_bytes());
        o[12..16].copy_from_slice(&self.rank.to_le_bytes());
    }
    fn get(b: &[u8]) -> Self {
        Self { node: rd_i64(b, 0), way: rd_u32(b, 8), rank: rd_u32(b, 12) }
    }
}

/// Membre way d'une relation d'itinéraire ; clé totale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct RouteBits {
    way_id: i64,
    bits: u8,
}

impl Record for RouteBits {
    const SIZE: usize = 9;
    fn put(&self, o: &mut [u8]) {
        o[0..8].copy_from_slice(&self.way_id.to_le_bytes());
        o[8] = self.bits;
    }
    fn get(b: &[u8]) -> Self {
        Self { way_id: rd_i64(b, 0), bits: b[8] }
    }
}

/// Référence avec sa coordonnée et son drapeau de jonction ; clé `(way, rang)`, unique.
#[derive(Clone, Copy, Debug)]
struct WayNodeC {
    way: u32,
    rank: u32,
    node: i64,
    lat: i32,
    lon: i32,
    multi: bool,
}

impl Record for WayNodeC {
    const SIZE: usize = 25;
    fn put(&self, o: &mut [u8]) {
        o[0..4].copy_from_slice(&self.way.to_le_bytes());
        o[4..8].copy_from_slice(&self.rank.to_le_bytes());
        o[8..16].copy_from_slice(&self.node.to_le_bytes());
        o[16..20].copy_from_slice(&self.lat.to_le_bytes());
        o[20..24].copy_from_slice(&self.lon.to_le_bytes());
        o[24] = u8::from(self.multi);
    }
    fn get(b: &[u8]) -> Self {
        Self {
            way: rd_u32(b, 0),
            rank: rd_u32(b, 4),
            node: rd_i64(b, 8),
            lat: rd_i32(b, 16),
            lon: rd_i32(b, 20),
            multi: b[24] != 0,
        }
    }
}

/// Arête en cours de numérotation : `a` et `b` sont des ids OSM, puis des ids globaux après E6.
#[derive(Clone, Copy, Debug)]
struct Edge0 {
    a: i64,
    way_id: i64,
    b: i64,
    rank_lo: u32,
    attrs: u64,
    len_dm: u32,
    geom_off: u64,
    geom_len: u32,
}

impl Record for Edge0 {
    const SIZE: usize = 52;
    fn put(&self, o: &mut [u8]) {
        o[0..8].copy_from_slice(&self.a.to_le_bytes());
        o[8..16].copy_from_slice(&self.way_id.to_le_bytes());
        o[16..24].copy_from_slice(&self.b.to_le_bytes());
        o[24..28].copy_from_slice(&self.rank_lo.to_le_bytes());
        o[28..36].copy_from_slice(&self.attrs.to_le_bytes());
        o[36..40].copy_from_slice(&self.len_dm.to_le_bytes());
        o[40..48].copy_from_slice(&self.geom_off.to_le_bytes());
        o[48..52].copy_from_slice(&self.geom_len.to_le_bytes());
    }
    fn get(b: &[u8]) -> Self {
        Self {
            a: rd_i64(b, 0),
            way_id: rd_i64(b, 8),
            b: rd_i64(b, 16),
            rank_lo: rd_u32(b, 24),
            attrs: rd_u64(b, 28),
            len_dm: rd_u32(b, 36),
            geom_off: rd_u64(b, 40),
            geom_len: rd_u32(b, 48),
        }
    }
}

/// Extrémité d'arête ; clé totale `(cellule en Morton, Morton du point, id OSM)` : deux copies d'un même nœud
/// sont identiques, donc adjacentes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Vert {
    cell: u64,
    point: u64,
    osm: i64,
    lat: i32,
    lon: i32,
}

impl Record for Vert {
    const SIZE: usize = 32;
    fn put(&self, o: &mut [u8]) {
        o[0..8].copy_from_slice(&self.cell.to_le_bytes());
        o[8..16].copy_from_slice(&self.point.to_le_bytes());
        o[16..24].copy_from_slice(&self.osm.to_le_bytes());
        o[24..28].copy_from_slice(&self.lat.to_le_bytes());
        o[28..32].copy_from_slice(&self.lon.to_le_bytes());
    }
    fn get(b: &[u8]) -> Self {
        Self {
            cell: rd_u64(b, 0),
            point: rd_u64(b, 8),
            osm: rd_i64(b, 16),
            lat: rd_i32(b, 24),
            lon: rd_i32(b, 28),
        }
    }
}

/// Id OSM d'un sommet et son id global ; clé `osm`, unique.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct OsmGid {
    osm: i64,
    gid: u32,
}

impl Record for OsmGid {
    const SIZE: usize = 12;
    fn put(&self, o: &mut [u8]) {
        o[0..8].copy_from_slice(&self.osm.to_le_bytes());
        o[8..12].copy_from_slice(&self.gid.to_le_bytes());
    }
    fn get(b: &[u8]) -> Self {
        Self { osm: rd_i64(b, 0), gid: rd_u32(b, 8) }
    }
}

/// Coordonnée d'un sommet, dans l'ordre des ids globaux.
#[derive(Clone, Copy, Debug)]
struct NodeC {
    lat: i32,
    lon: i32,
}

impl Record for NodeC {
    const SIZE: usize = 8;
    fn put(&self, o: &mut [u8]) {
        o[0..4].copy_from_slice(&self.lat.to_le_bytes());
        o[4..8].copy_from_slice(&self.lon.to_le_bytes());
    }
    fn get(b: &[u8]) -> Self {
        Self { lat: rd_i32(b, 0), lon: rd_i32(b, 4) }
    }
}

/// Arête orientée `(source, cible, arête|sens)` ; clé totale, celle du tri de `write_graph`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Dir(u32, u32, u32);

impl Record for Dir {
    const SIZE: usize = 12;
    fn put(&self, o: &mut [u8]) {
        o[0..4].copy_from_slice(&self.0.to_le_bytes());
        o[4..8].copy_from_slice(&self.1.to_le_bytes());
        o[8..12].copy_from_slice(&self.2.to_le_bytes());
    }
    fn get(b: &[u8]) -> Self {
        Self(rd_u32(b, 0), rd_u32(b, 4), rd_u32(b, 8))
    }
}

/// Arête finale, dans l'ordre des ids d'arête.
#[derive(Clone, Copy, Debug)]
struct EdgeRec {
    a: u32,
    attrs: u64,
    len_dm: u32,
    geom_len: u32,
}

impl Record for EdgeRec {
    const SIZE: usize = 20;
    fn put(&self, o: &mut [u8]) {
        o[0..4].copy_from_slice(&self.a.to_le_bytes());
        o[4..12].copy_from_slice(&self.attrs.to_le_bytes());
        o[12..16].copy_from_slice(&self.len_dm.to_le_bytes());
        o[16..20].copy_from_slice(&self.geom_len.to_le_bytes());
    }
    fn get(b: &[u8]) -> Self {
        Self { a: rd_u32(b, 0), attrs: rd_u64(b, 4), len_dm: rd_u32(b, 12), geom_len: rd_u32(b, 16) }
    }
}

/// Géométrie à déplacer : de `src` (dans `geom0`, ordre des ways) à `dst` (ordre des arêtes) ; clé `src`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct GeomReq {
    src: u64,
    dst: u64,
    len: u32,
}

impl Record for GeomReq {
    const SIZE: usize = 20;
    fn put(&self, o: &mut [u8]) {
        o[0..8].copy_from_slice(&self.src.to_le_bytes());
        o[8..16].copy_from_slice(&self.dst.to_le_bytes());
        o[16..20].copy_from_slice(&self.len.to_le_bytes());
    }
    fn get(b: &[u8]) -> Self {
        Self { src: rd_u64(b, 0), dst: rd_u64(b, 8), len: rd_u32(b, 16) }
    }
}

// --- E1 : passe 1 ----------------------------------------------------------------------------------

type WnSort<'t> = ExtSort<'t, WayNode, WayNode, fn(&WayNode) -> WayNode>;
type RbSort<'t> = ExtSort<'t, RouteBits, RouteBits, fn(&RouteBits) -> RouteBits>;

struct Pass1Ext<'t> {
    ways: RecWriter<'t, WayFile>,
    wn: WnSort<'t>,
    rb: RbSort<'t>,
    scratch: Vec<i64>,
    last_way: i64,
    last_relation: i64,
    err: Option<BuildError>,
    stats: BuildStats,
}

impl Pass1Ext<'_> {
    fn way_inner(&mut self, w: &OsmWay<'_>) -> Result<(), BuildError> {
        if w.id <= self.last_way {
            return Err(BuildError::NotSorted(format!("way {} après {}", w.id, self.last_way)));
        }
        self.last_way = w.id;
        self.stats.ways_total += 1;
        let info = match classify(w.tags) {
            Ok(i) => i,
            Err(r) => {
                *self.stats.rejects.entry(r.label()).or_default() += 1;
                return Ok(());
            }
        };
        self.scratch.clear();
        for &r in w.refs {
            if self.scratch.last() != Some(&r) {
                self.scratch.push(r);
            }
        }
        if self.scratch.len() < 2 {
            *self.stats.rejects.entry("moins de deux nœuds distincts".into()).or_default() += 1;
            return Ok(());
        }
        let way = u32::try_from(self.ways.len())
            .map_err(|_| BuildError::TooBig("plus de 2^32 ways gardées".into()))?;
        let n = u32::try_from(self.scratch.len())
            .map_err(|_| BuildError::TooBig(format!("way {} : plus de 2^32 nœuds", w.id)))?;
        let notes = &info.notes;
        self.stats.oneway_reversible += u64::from(notes.reversible);
        self.stats.ignored_conditional += u64::from(notes.conditional);
        self.stats.ignored_bicycle_directional += u64::from(notes.bicycle_directional);
        self.stats.ignored_motor_vehicle += u64::from(notes.motor_vehicle);
        self.ways.push(&WayFile { id: w.id, attrs: info.attrs.0, n })?;
        for (rank, &node) in self.scratch.iter().enumerate() {
            self.wn.push(WayNode { node, way, rank: rank as u32 })?;
        }
        Ok(())
    }

    fn relation_inner(&mut self, r: &OsmRelation<'_>) -> Result<(), BuildError> {
        if r.id <= self.last_relation {
            return Err(BuildError::NotSorted(format!("relation {} après {}", r.id, self.last_relation)));
        }
        self.last_relation = r.id;
        self.stats.relations_total += 1;
        let Some(bits) = route_bits(r.tags) else { return Ok(()) };
        self.stats.relations_routes += 1;
        for &m in r.way_members {
            self.rb.push(RouteBits { way_id: m, bits })?;
        }
        Ok(())
    }
}

impl ElementSink for Pass1Ext<'_> {
    fn way(&mut self, w: &OsmWay<'_>) {
        if self.err.is_none() {
            if let Err(e) = self.way_inner(w) {
                self.err = Some(e);
            }
        }
    }
    fn relation(&mut self, r: &OsmRelation<'_>) {
        if self.err.is_none() {
            if let Err(e) = self.relation_inner(r) {
                self.err = Some(e);
            }
        }
    }
}

// --- E4 : découpe d'une way --------------------------------------------------------------------------

struct Cutter<'t, FE: Fn(&Edge0) -> (i64, i64, u32) + Sync, FV: Fn(&Vert) -> Vert + Sync> {
    zoom: u8,
    geom0: BufWriter<File>,
    geom_pos: u64,
    geom_buf: Vec<u8>,
    interior: Vec<(i32, i32)>,
    edges: ExtSort<'t, Edge0, (i64, i64, u32), FE>,
    verts: ExtSort<'t, Vert, Vert, FV>,
    tmp: &'t TmpDir,
}

impl<FE: Fn(&Edge0) -> (i64, i64, u32) + Sync, FV: Fn(&Vert) -> Vert + Sync> Cutter<'_, FE, FV> {
    fn vert(&self, osm: i64, lat: i32, lon: i32) -> Vert {
        let (x, y) = grid::tile_of_e7(lat, lon, self.zoom);
        Vert { cell: grid::morton(x, y), point: grid::morton_e7(lat, lon), osm, lat, lon }
    }

    /// Arête de `lo` à `hi` (rangs de la way) : longueur, géométrie, deux extrémités.
    fn emit(
        &mut self,
        way: &WayFile,
        attrs: u64,
        nodes: &[WayNodeC],
        lo: usize,
        hi: usize,
    ) -> Result<(), BuildError> {
        let mut len = 0.0;
        for k in lo..hi {
            len += haversine_m((nodes[k].lat, nodes[k].lon), (nodes[k + 1].lat, nodes[k + 1].lon));
        }
        let len_dm = (len * 10.0).round().min(f64::from(u32::MAX)) as u32;
        self.interior.clear();
        self.interior.extend(nodes[lo + 1..hi].iter().map(|n| (n.lat, n.lon)));
        self.geom_buf.clear();
        encode_geometry(&mut self.geom_buf, (nodes[lo].lat, nodes[lo].lon), &self.interior);
        self.geom0.write_all(&self.geom_buf)?;
        self.tmp.added(self.geom_buf.len() as u64);
        let (a, b) = (&nodes[lo], &nodes[hi]);
        self.edges.push(Edge0 {
            a: a.node,
            way_id: way.id,
            b: b.node,
            rank_lo: lo as u32,
            attrs,
            len_dm,
            geom_off: self.geom_pos,
            geom_len: self.geom_buf.len() as u32,
        })?;
        self.geom_pos += self.geom_buf.len() as u64;
        let (va, vb) = (self.vert(a.node, a.lat, a.lon), self.vert(b.node, b.lat, b.lon));
        self.verts.push(va)?;
        self.verts.push(vb)?;
        Ok(())
    }

    /// Même découpe que `build_graph` : segments de nœuds présents, coupe aux jonctions (`multi`) et aux bouts de
    /// segment, boucle coupée à son nœud intérieur médian. Les drapeaux de bout et de boucle n'ont d'effet que
    /// dans la way qui les pose : un nœud vu ailleurs est déjà `multi`.
    #[allow(clippy::needless_range_loop)] // même parcours que `build_graph`, `k = n` compris
    fn way(
        &mut self,
        way: &WayFile,
        attrs: u64,
        nodes: &[WayNodeC],
        stats: &mut BuildStats,
    ) -> Result<(), BuildError> {
        let n = nodes.len();
        let (mut run, mut cut) = (None::<usize>, false);
        let mut segs: Vec<(usize, usize)> = Vec::new();
        for k in 0..=n {
            let present = k < n && nodes[k].lat != MISSING_LAT;
            if present {
                run.get_or_insert(k);
                continue;
            }
            if k < n {
                cut = true;
                stats.nodes_referenced_missing += 1;
            }
            if let Some(lo) = run.take() {
                if k - 1 > lo {
                    segs.push((lo, k - 1));
                }
            }
        }
        stats.ways_cut_by_missing_nodes += u64::from(cut);
        for (lo, hi) in segs {
            let mut start = lo;
            for k in lo + 1..=hi {
                if !nodes[k].multi && k != hi {
                    continue;
                }
                if nodes[start].node == nodes[k].node && k - start >= 2 {
                    let m = start + (k - start) / 2;
                    stats.loops_cut += 1;
                    self.emit(way, attrs, nodes, start, m)?;
                    self.emit(way, attrs, nodes, m, k)?;
                } else {
                    self.emit(way, attrs, nodes, start, k)?;
                }
                start = k;
            }
        }
        Ok(())
    }
}

// --- Construction ------------------------------------------------------------------------------------

fn edge_key_a(e: &Edge0) -> (i64, i64, u32) {
    (e.a, e.way_id, e.rank_lo)
}
fn edge_key_b(e: &Edge0) -> (i64, i64, u32) {
    (e.b, e.way_id, e.rank_lo)
}
fn edge_key_final(e: &Edge0) -> (i64, i64, i64, u32) {
    (e.a, e.way_id, e.b, e.rank_lo)
}

/// Remplace un id OSM par son id global, en avançant dans `ids` (trié par OSM) au rythme des arêtes (triées par
/// la même extrémité).
fn gid_of(ids: &mut Peekable<RecReader<'_, OsmGid>, OsmGid>, osm: i64) -> Result<i64, BuildError> {
    while let Some(g) = ids.peek()? {
        if g.osm < osm {
            ids.pull()?;
            continue;
        }
        if g.osm == osm {
            return Ok(i64::from(g.gid));
        }
        break;
    }
    Err(BuildError::Source(format!("extrémité {osm} absente des sommets")))
}

/// Construit le graphe de `src` hors mémoire et l'écrit dans `path` (mêmes octets que `build_graph` puis
/// `write_graph`, hors section `STATS`). `phases_before` : phases déjà chronométrées par l'appelant (ouverture
/// de la source), recopiées en tête de `STATS`. `on_phase` est appelé en fin de chaque étape.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub fn build_external(
    src: &mut dyn OsmSource,
    opts: &ExternalOptions,
    region: &str,
    path: &Path,
    tool_version: &str,
    build_epoch: i64,
    phases_before: Vec<crate::stats::Phase>,
    on_phase: &mut dyn FnMut(&str),
) -> Result<ExternalReport, ExternalError> {
    if !(1..=16).contains(&opts.zoom) {
        return Err(BuildError::TooBig(format!("zoom {} hors de 1..=16", opts.zoom)).into());
    }
    let zoom = opts.zoom;
    let budget = opts.mem_budget.max(1);
    let half = (budget / 2).max(1);
    let tmp = TmpDir::create(&opts.tmp_parent)?;
    let mut clock = PhaseClock::new();
    let mut stats = BuildStats {
        region: region.to_string(),
        threads: rayon::current_num_threads(),
        ..Default::default()
    };
    stats.phases = phases_before;
    let mut lap = |stats: &mut BuildStats, clock: &mut PhaseClock, name: &str| {
        stats.phases.push(clock.lap(name));
        on_phase(name);
    };
    let meta = src.meta();

    // E1 — ways et relations.
    let mut p1 = Pass1Ext {
        ways: RecWriter::create(&tmp, "ways", IO_BUF)?,
        wn: ExtSort::new(&tmp, "waynode", half, (|r: &WayNode| *r) as fn(&WayNode) -> WayNode),
        rb: ExtSort::new(
            &tmp,
            "routes",
            (budget / 8).max(1),
            (|r: &RouteBits| *r) as fn(&RouteBits) -> RouteBits,
        ),
        scratch: Vec::new(),
        last_way: i64::MIN,
        last_relation: i64::MIN,
        err: None,
        stats,
    };
    src.scan(&mut p1)?;
    if let Some(e) = p1.err.take() {
        return Err(e.into());
    }
    let Pass1Ext { ways, wn, rb, stats: s, .. } = p1;
    stats = s;
    let (ways_path, n_ways) = ways.finish()?;
    stats.ways_kept = n_ways;
    lap(&mut stats, &mut clock, "E1 passe 1 : ways et relations");

    // E2–E3 — coordonnées par jointure avec le flux des nœuds, jonctions, retour à l'ordre des ways.
    let mut wn = Peekable::new(wn.finish()?);
    let mut wnc = ExtSort::new(&tmp, "waynodec", half, |r: &WayNodeC| (r.way, r.rank));
    let mut group: Vec<WayNode> = Vec::new();
    let mut used_nodes = 0u64;
    let mut last_node = i64::MIN;
    // Une série de références au même nœud : sa coordonnée (ou l'absence) et `multi`.
    let flush = |group: &mut Vec<WayNode>, coord: Option<(i32, i32)>, wnc: &mut ExtSort<_, _, _>| {
        let multi = group.len() >= 2;
        let (lat, lon) = coord.unwrap_or((MISSING_LAT, 0));
        for r in group.drain(..) {
            wnc.push(WayNodeC { way: r.way, rank: r.rank, node: r.node, lat, lon, multi })?;
        }
        Ok::<(), std::io::Error>(())
    };
    // Prend dans `group` toutes les références au prochain nœud référencé.
    fn take_group(
        wn: &mut Peekable<impl crate::extsort::Stream<WayNode>, WayNode>,
        group: &mut Vec<WayNode>,
    ) -> std::io::Result<Option<i64>> {
        let Some(first) = wn.pull()? else { return Ok(None) };
        group.push(first);
        while let Some(r) = wn.peek()? {
            if r.node != first.node {
                break;
            }
            group.push(r);
            wn.pull()?;
        }
        Ok(Some(first.node))
    }
    src.nodes_sorted(&mut |batch| {
        for &(id, lat, lon) in batch {
            if id <= last_node {
                return Err(BuildError::NotSorted(format!("nœud {id} après {last_node}")));
            }
            last_node = id;
            // Références à des nœuds d'id plus petit : absents de l'extrait.
            while let Some(r) = wn.peek()? {
                if r.node > id {
                    break;
                }
                take_group(&mut wn, &mut group)?;
                used_nodes += 1;
                let coord = (group[0].node == id).then_some((lat, lon));
                flush(&mut group, coord, &mut wnc)?;
            }
        }
        Ok(())
    })?;
    while take_group(&mut wn, &mut group)?.is_some() {
        used_nodes += 1;
        flush(&mut group, None, &mut wnc)?;
    }
    drop(wn);
    if used_nodes >= 1 << 31 {
        return Err(BuildError::TooBig("plus de 2^31 nœuds utilisés".into()).into());
    }
    stats.used_nodes = used_nodes;
    lap(&mut stats, &mut clock, "E2-E3 passe 2 : nœuds, jonctions");

    // E4 — arêtes, way par way.
    let geom0_path = tmp.file("geom0");
    let mut cutter = Cutter {
        zoom,
        geom0: BufWriter::with_capacity(IO_BUF, File::create(&geom0_path)?),
        geom_pos: 0,
        geom_buf: Vec::new(),
        interior: Vec::new(),
        edges: ExtSort::new(&tmp, "edges-a", (budget * 5 / 16).max(1), edge_key_a),
        verts: ExtSort::new(&tmp, "verts", (budget * 3 / 16).max(1), |v: &Vert| *v),
        tmp: &tmp,
    };
    {
        let mut ways = RecReader::<WayFile>::open(&tmp, &ways_path, n_ways, IO_BUF, true)?;
        let mut wnc = wnc.finish()?;
        let mut rb = Peekable::new(rb.finish()?);
        let mut nodes: Vec<WayNodeC> = Vec::new();
        for w in 0..n_ways as u32 {
            let way = ways.pull()?.ok_or_else(|| BuildError::Source("ways : fin prématurée".into()))?;
            let mut attrs = Attrs(way.attrs);
            while let Some(r) = rb.peek()? {
                if r.way_id > way.id {
                    break;
                }
                rb.pull()?;
                if r.way_id == way.id {
                    attrs = attrs.with_routes(r.bits);
                    stats.relation_way_members_matched += 1;
                } else {
                    stats.relation_way_members_not_in_graph += 1;
                }
            }
            nodes.clear();
            for rank in 0..way.n {
                let r =
                    wnc.pull()?.ok_or_else(|| BuildError::Source("références : fin prématurée".into()))?;
                if r.way != w || r.rank != rank {
                    return Err(
                        BuildError::Source(format!("références désordonnées : way {w} rang {rank}")).into()
                    );
                }
                nodes.push(r);
            }
            cutter.way(&way, attrs.0, &nodes, &mut stats)?;
        }
        while rb.pull()?.is_some() {
            stats.relation_way_members_not_in_graph += 1;
        }
    }
    let Cutter { mut geom0, edges, verts, .. } = cutter;
    geom0.flush()?;
    drop(geom0);
    lap(&mut stats, &mut clock, "E4 découpe en arêtes");

    // E5 — sommets : ids globaux, cellules.
    let nodes_path = tmp.file("nodes");
    let nids_path = tmp.file("nids");
    let mut cells: Vec<CellSpec> = Vec::new();
    let mut ids_sort = ExtSort::new(&tmp, "osmgid", half, |g: &OsmGid| *g);
    let n_vertices = {
        let mut verts = verts.finish()?;
        let mut nodes_w = RecWriter::<NodeC>::create(&tmp, "nodes", IO_BUF)?;
        let mut nids_w = BufWriter::with_capacity(IO_BUF, File::create(&nids_path)?);
        let mut gid = 0u64;
        let mut last_osm: Option<i64> = None;
        let mut last_cell = u64::MAX;
        while let Some(v) = verts.pull()? {
            if last_osm == Some(v.osm) {
                continue;
            }
            last_osm = Some(v.osm);
            let g = u32::try_from(gid).map_err(|_| BuildError::TooBig("plus de 2^32 sommets".into()))?;
            if v.cell != last_cell || cells.is_empty() {
                let (x, y) = grid::tile_of_e7(v.lat, v.lon, zoom);
                cells.push(CellSpec { x, y, first_node: g, n_nodes: 0 });
                last_cell = v.cell;
            }
            cells.last_mut().expect("cellule ouverte").n_nodes += 1;
            nodes_w.push(&NodeC { lat: v.lat, lon: v.lon })?;
            nids_w.write_all(&(v.osm as u64).to_le_bytes())?;
            tmp.added(8);
            ids_sort.push(OsmGid { osm: v.osm, gid: g })?;
            gid += 1;
        }
        nids_w.flush()?;
        let (p, _) = nodes_w.finish()?;
        std::fs::rename(&p, &nodes_path)?;
        gid
    };
    let (ids_path, n_ids) = {
        let mut sorted = ids_sort.finish()?;
        let mut w = RecWriter::<OsmGid>::create(&tmp, "osmgid", IO_BUF)?;
        while let Some(g) = sorted.pull()? {
            w.push(&g)?;
        }
        w.finish()?
    };
    stats.vertices = n_vertices;
    lap(&mut stats, &mut clock, "E5 sommets, cellules");

    // E6 — renumérotation des extrémités, tri final des arêtes.
    let mut by_b = ExtSort::new(&tmp, "edges-b", half, edge_key_b);
    {
        let mut edges = edges.finish()?;
        let mut ids = Peekable::new(RecReader::<OsmGid>::open(&tmp, &ids_path, n_ids, IO_BUF, false)?);
        while let Some(mut e) = edges.pull()? {
            e.a = gid_of(&mut ids, e.a)?;
            by_b.push(e)?;
        }
    }
    let mut final_sort = ExtSort::new(&tmp, "edges-final", half, edge_key_final);
    {
        let mut edges = by_b.finish()?;
        let mut ids = Peekable::new(RecReader::<OsmGid>::open(&tmp, &ids_path, n_ids, IO_BUF, true)?);
        while let Some(mut e) = edges.pull()? {
            e.b = gid_of(&mut ids, e.b)?;
            final_sort.push(e)?;
        }
    }
    lap(&mut stats, &mut clock, "E6 renumérotation des arêtes");

    // E7 — arêtes orientées, géométrie dans l'ordre des arêtes, écriture.
    let mut dirs = ExtSort::new(&tmp, "dir", (budget * 3 / 8).max(1), |d: &Dir| *d);
    let mut greqs = ExtSort::new(&tmp, "geomreq", (budget / 8).max(1), |g: &GeomReq| *g);
    let eids_path = tmp.file("eids");
    let (edges_path, n_edges, n_directed, geom_total) = {
        let mut sorted = final_sort.finish()?;
        let mut edges_w = RecWriter::<EdgeRec>::create(&tmp, "edges", IO_BUF)?;
        let mut eids_w = BufWriter::with_capacity(IO_BUF, File::create(&eids_path)?);
        let (mut n_dir, mut dst) = (0u64, 0u64);
        let mut prev: Option<(i64, i64, i64)> = None;
        let mut e = 0u64;
        while let Some(ed) = sorted.pull()? {
            if e >= 1 << 31 {
                return Err(FormatError::Build("plus de 2^31 arêtes".into()).into());
            }
            let ident = (ed.a, ed.way_id, ed.b);
            stats.duplicate_identities += u64::from(prev == Some(ident));
            prev = Some(ident);
            let (a, b, id) = (ed.a as u32, ed.b as u32, e as u32);
            let attrs = Attrs(ed.attrs);
            if attrs.fwd_ok() {
                dirs.push(Dir(a, b, id))?;
                n_dir += 1;
            }
            if attrs.bwd_ok() {
                dirs.push(Dir(b, a, id | DIR_BIT))?;
                n_dir += 1;
            }
            edges_w.push(&EdgeRec { a, attrs: ed.attrs, len_dm: ed.len_dm, geom_len: ed.geom_len })?;
            eids_w.write_all(&(ed.way_id as u64).to_le_bytes())?;
            eids_w.write_all(&a.to_le_bytes())?;
            eids_w.write_all(&b.to_le_bytes())?;
            tmp.added(16);
            greqs.push(GeomReq { src: ed.geom_off, dst, len: ed.geom_len })?;
            dst += u64::from(ed.geom_len);
            e += 1;
        }
        eids_w.flush()?;
        let (p, n) = edges_w.finish()?;
        (p, n, n_dir, dst)
    };
    stats.edges = n_edges;
    let geom_path = redistribute_geometry(&tmp, greqs, &geom0_path, geom_total, budget)?;

    let mut writer = StreamWriter::create(path, cells.clone())?;
    {
        let mut nodes = RecReader::<NodeC>::open(&tmp, &nodes_path, n_vertices, IO_BUF, true)?;
        let mut dirs = Peekable::new(dirs.finish()?);
        let mut edges = Peekable::new(RecReader::<EdgeRec>::open(&tmp, &edges_path, n_edges, IO_BUF, true)?);
        let (mut cn, mut cd, mut co) = (Vec::new(), Vec::new(), Vec::new());
        for cs in &cells {
            let nhi = cs.first_node + cs.n_nodes;
            cn.clear();
            for _ in 0..cs.n_nodes {
                let n = nodes.pull()?.ok_or_else(|| BuildError::Source("sommets : fin prématurée".into()))?;
                cn.push((n.lat, n.lon));
            }
            cd.clear();
            while let Some(d) = dirs.peek()? {
                if d.0 >= nhi {
                    break;
                }
                cd.push((d.0, d.1, d.2));
                dirs.pull()?;
            }
            co.clear();
            let mut geom_len = 0u64;
            while let Some(ed) = edges.peek()? {
                if ed.a >= nhi {
                    break;
                }
                let off = u32::try_from(geom_len)
                    .map_err(|_| FormatError::Build("géométrie d'une cellule > 4 Go".into()))?;
                co.push((ed.attrs, ed.len_dm, off));
                geom_len += u64::from(ed.geom_len);
                edges.pull()?;
            }
            writer.put_cell(&cn, &cd, &co, geom_len)?;
        }
    }
    let mut geom = BufReader::with_capacity(IO_BUF, File::open(&geom_path)?);
    writer.put_geometry(&mut geom)?;
    drop(geom);
    tmp.remove(&geom_path);
    writer.put_node_ids(&mut BufReader::with_capacity(IO_BUF, File::open(&nids_path)?), n_vertices)?;
    tmp.remove(&nids_path);
    writer.put_edge_ids(&mut BufReader::with_capacity(IO_BUF, File::open(&eids_path)?), n_edges)?;
    tmp.remove(&eids_path);
    lap(&mut stats, &mut clock, "E7 écriture (blocs, géométrie, ids)");

    let stats_json = serde_json::to_vec(&stats).map_err(|e| FormatError::Build(e.to_string()))?;
    let header = StreamHeader {
        zoom,
        nodes: n_vertices,
        edges: n_edges,
        directed: n_directed,
        meta,
        tool_version,
        build_epoch,
    };
    let mut write_clock = PhaseClock::new();
    writer.finish(&stats_json, &header)?;
    stats.phases.push(write_clock.lap("STATS, en-tête, relecture et verify()"));
    Ok(ExternalReport { stats, tmp_peak_bytes: tmp.peak_bytes(), runs: tmp.runs() })
}

/// Recopie la géométrie de `geom0` (ordre des ways) dans l'ordre des arêtes, par paquets de taille bornée : on
/// parcourt `geom0` séquentiellement (requêtes triées par source), chaque géométrie part dans le paquet de sa
/// destination, puis chaque paquet est rangé en mémoire et ajouté à la suite.
fn redistribute_geometry<K: Ord + Send, F: Fn(&GeomReq) -> K + Sync>(
    tmp: &TmpDir,
    greqs: ExtSort<'_, GeomReq, K, F>,
    geom0_path: &Path,
    total: u64,
    budget: usize,
) -> Result<PathBuf, ExternalError> {
    const MAX_BUCKETS: u64 = 256;
    let bucket = ((budget / 2) as u64).max(total.div_ceil(MAX_BUCKETS)).max(1);
    let n_buckets = total.div_ceil(bucket).max(1) as usize;
    let per_buf = (budget / 2 / n_buckets).max(16 << 10);
    let mut paths = Vec::with_capacity(n_buckets);
    let mut writers = Vec::with_capacity(n_buckets);
    for i in 0..n_buckets {
        let p = tmp.file(&format!("geom-bucket-{i}"));
        writers.push(BufWriter::with_capacity(per_buf, File::create(&p)?));
        paths.push(p);
    }
    {
        let mut sorted = greqs.finish()?;
        let mut g0 = BufReader::with_capacity(IO_BUF, File::open(geom0_path)?);
        let mut pos = 0u64;
        let mut buf = Vec::new();
        while let Some(r) = sorted.pull()? {
            if r.src != pos {
                return Err(BuildError::Source(format!(
                    "géométrie : décalage {} attendu, {} lu",
                    pos, r.src
                ))
                .into());
            }
            buf.resize(r.len as usize, 0);
            g0.read_exact(&mut buf)?;
            pos += u64::from(r.len);
            let w = &mut writers[(r.dst / bucket) as usize];
            w.write_all(&r.dst.to_le_bytes())?;
            w.write_all(&r.len.to_le_bytes())?;
            w.write_all(&buf)?;
            tmp.added(12 + u64::from(r.len));
        }
    }
    tmp.remove(geom0_path);
    for w in &mut writers {
        w.flush()?;
    }
    drop(writers);
    let out_path = tmp.file("geom");
    let mut out = BufWriter::with_capacity(IO_BUF, File::create(&out_path)?);
    let mut written = 0u64;
    for p in &paths {
        let bytes = std::fs::read(p)?;
        tmp.remove(p);
        let mut recs: Vec<(u64, &[u8])> = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            let dst = rd_u64(&bytes, i);
            let len = rd_u32(&bytes, i + 8) as usize;
            recs.push((dst, &bytes[i + 12..i + 12 + len]));
            i += 12 + len;
        }
        recs.sort_unstable_by_key(|r| r.0);
        let before = written;
        for (dst, b) in recs {
            if dst != written {
                return Err(
                    BuildError::Source(format!("géométrie : destination {dst}, attendu {written}")).into()
                );
            }
            out.write_all(b)?;
            written += b.len() as u64;
        }
        tmp.added(written - before);
    }
    out.flush()?;
    if written != total {
        return Err(BuildError::Source(format!("géométrie : {written} octets sur {total}")).into());
    }
    Ok(out_path)
}
