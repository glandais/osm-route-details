//! Construction **hors mémoire** du graphe (`docs/design/technique/ord-graph-build-hors-memoire.md`) : mêmes
//! octets que [`crate::build::build_graph`] suivi de [`crate::format::write_graph`], mais chaque grande structure
//! vit dans un fichier temporaire, parcouru en flux ou à travers un tri externe ([`crate::extsort`]). La mémoire
//! ne tient que les tampons des tris (bornés par le budget), le lot de blobs PBF décodés, la table des cellules,
//! le bloc de cellule en cours d'écriture et un paquet de géométrie (borné par le budget).
//!
//! Étapes (note §5) :
//! - **E1** passe 1 : ways gardées (`ways`), références `(nœud, way, rang)`, membres d'itinéraire ;
//! - **E2–E3** passe 2 : jointure par fusion des références triées par nœud avec le flux des nœuds (id croissant),
//!   d'où coordonnées et jonctions (`multi` : au moins deux occurrences), puis retour à l'ordre (way, rang) ;
//! - **E4** découpe way par way (drapeaux de bout et de boucle locaux, note §5 E3), géométrie codée, sommets ;
//! - **E5** sommets triés (cellule, Morton, id OSM) : ids globaux, cellules ;
//! - **E6** renumérotation des extrémités, puis tri final des arêtes `(a, way, b, rang)` ;
//! - **E7** arêtes orientées, écriture en flux, géométrie redistribuée par paquets dans l'ordre des arêtes.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use zerocopy::little_endian::{I32, I64, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

use crate::attrs::Attrs;
use crate::build::{
    cut_way, keep_way, len_dm, route_relation, BuildError, CellSpec, ElementSink, OsmRelation, OsmSource,
    OsmWay,
};
use crate::extsort::{ExtSort, Peekable, RecReader, RecWriter, Stream, TmpDir, IO_BUF};
use crate::format::{encode_geometry, EdgeIdRec, FormatError, NodeRec, StreamHeader, StreamWriter, DIR_BIT};
use crate::grid;
use crate::stats::{proc_status, BuildStats, PhaseClock};

/// Réglages de la construction hors mémoire.
#[derive(Clone, Debug)]
pub struct ExternalOptions {
    /// Zoom de la grille de cellules.
    pub zoom: u8,
    /// Budget des tampons de tri, en octets (réparti entre les tris actifs d'une étape).
    pub mem_budget: usize,
    /// Dossier où créer le dossier temporaire `.ordg-build-<pid>-<unique>/`.
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
    /// `VmHWM` juste avant la relecture finale. Celle-ci projette le fichier écrit en mémoire : ses pages
    /// comptent ensuite dans `VmHWM`, qui ne dit donc plus le pic de la construction elle-même.
    pub peak_rss_before_verify_kb: u64,
    /// Dossiers temporaires laissés par des constructions mortes, supprimés au départ.
    pub stale_tmp_removed: Vec<PathBuf>,
}

/// Latitude d'une référence dont le nœud manque dans l'extrait (comme `MISSING` de `build.rs`).
const MISSING_LAT: i32 = i32::MIN;

// --- Enregistrements -----------------------------------------------------------------------------
//
// Structures `zerocopy` d'alignement 1, champs petit-boutistes : les octets en mémoire sont ceux du fichier
// (`extsort::Record`). Les sections `ID_NODES` et `ID_EDGES` et les sommets s'écrivent avec les types de
// `format.rs` (`U64`, `EdgeIdRec`, `NodeRec`).

/// Way gardée, dans l'ordre des ids : son indice est son rang dans le fichier.
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy, Debug)]
#[repr(C)]
struct WayFile {
    id: I64,
    attrs: U64,
    n: U32,
}

/// Référence d'une way à un nœud ; clé totale `(nœud, way, rang)`.
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy, Debug)]
#[repr(C)]
struct WayNode {
    node: I64,
    way: U32,
    rank: U32,
}

/// Membre way d'une relation d'itinéraire ; clé totale `(way, bits)`.
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy, Debug)]
#[repr(C)]
struct RouteBits {
    way_id: I64,
    bits: u8,
}

/// Référence avec sa coordonnée et son drapeau de jonction (`multi` : 0 ou 1) ; clé `(way, rang)`, unique.
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy, Debug)]
#[repr(C)]
struct WayNodeC {
    way: U32,
    rank: U32,
    node: I64,
    lat: I32,
    lon: I32,
    multi: u8,
}

/// Arête en cours de numérotation : `a` et `b` sont des ids OSM, puis des ids globaux après E6.
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy, Debug)]
#[repr(C)]
struct Edge0 {
    a: I64,
    way_id: I64,
    b: I64,
    rank_lo: U32,
    attrs: U64,
    len_dm: U32,
    geom_off: U64,
    geom_len: U32,
}

/// Extrémité d'arête ; clé totale `(cellule en Morton, Morton du point, id OSM)` : deux copies d'un même nœud
/// sont identiques, donc adjacentes.
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy, Debug)]
#[repr(C)]
struct Vert {
    cell: U64,
    point: U64,
    osm: I64,
    lat: I32,
    lon: I32,
}

/// Id OSM d'un sommet et son id global ; clé `osm`, unique.
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy, Debug)]
#[repr(C)]
struct OsmGid {
    osm: I64,
    gid: U32,
}

/// Arête orientée `(source, cible, arête|sens)` ; clé totale, celle du tri de `write_graph`.
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy, Debug)]
#[repr(C)]
struct Dir {
    src: U32,
    dst: U32,
    edge: U32,
}

/// Arête finale, dans l'ordre des ids d'arête.
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy, Debug)]
#[repr(C)]
struct EdgeRec {
    a: U32,
    attrs: U64,
    len_dm: U32,
    geom_len: U32,
}

/// Géométrie à déplacer : de `src` (dans `geom0`, ordre des ways) à `dst` (ordre des arêtes), `edge` étant l'id
/// de l'arête ; clé `src`, unique (une géométrie fait au moins un octet).
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy, Debug)]
#[repr(C)]
struct GeomReq {
    src: U64,
    dst: U64,
    len: U32,
    edge: U32,
}

// --- E1 : passe 1 ----------------------------------------------------------------------------------

type WnKey = (i64, u32, u32);
type RbKey = (i64, u8);
type WnSort<'t> = ExtSort<'t, WayNode, WnKey, fn(&WayNode) -> WnKey>;
type RbSort<'t> = ExtSort<'t, RouteBits, RbKey, fn(&RouteBits) -> RbKey>;

fn wn_key(r: &WayNode) -> WnKey {
    (r.node.get(), r.way.get(), r.rank.get())
}
fn rb_key(r: &RouteBits) -> RbKey {
    (r.way_id.get(), r.bits)
}

/// Passe 1 : même filtre que `build_graph` (`keep_way`, `route_relation`), sorties sur fichier. Une erreur
/// (écriture refusée, disque plein, source non triée) arrête le parcours de la source.
struct Pass1Ext<'t> {
    ways: RecWriter<'t, WayFile>,
    wn: WnSort<'t>,
    rb: RbSort<'t>,
    scratch: Vec<i64>,
    last_way: i64,
    last_relation: i64,
    stats: BuildStats,
}

impl ElementSink for Pass1Ext<'_> {
    fn way(&mut self, w: &OsmWay<'_>) -> Result<(), BuildError> {
        self.scratch.clear();
        let Some(attrs) = keep_way(w, &mut self.last_way, &mut self.stats, &mut self.scratch)? else {
            return Ok(());
        };
        let way = u32::try_from(self.ways.len())
            .map_err(|_| BuildError::TooBig("plus de 2^32 ways gardées".into()))?;
        let n = u32::try_from(self.scratch.len())
            .map_err(|_| BuildError::TooBig(format!("way {} : plus de 2^32 nœuds", w.id)))?;
        self.ways.push(&WayFile { id: w.id.into(), attrs: attrs.into(), n: n.into() })?;
        for (rank, &node) in self.scratch.iter().enumerate() {
            self.wn.push(WayNode { node: node.into(), way: way.into(), rank: (rank as u32).into() })?;
        }
        Ok(())
    }

    fn relation(&mut self, r: &OsmRelation<'_>) -> Result<(), BuildError> {
        let Some(bits) = route_relation(r, &mut self.last_relation, &mut self.stats)? else { return Ok(()) };
        for &m in r.way_members {
            self.rb.push(RouteBits { way_id: m.into(), bits })?;
        }
        Ok(())
    }
}

// --- E4 : découpe d'une way --------------------------------------------------------------------------

struct Cutter<'t, FE: Fn(&Edge0) -> (i64, i64, u32) + Sync, FV: Fn(&Vert) -> (u64, u64, i64) + Sync> {
    zoom: u8,
    geom0: BufWriter<File>,
    geom_pos: u64,
    geom_buf: Vec<u8>,
    interior: Vec<(i32, i32)>,
    edges: ExtSort<'t, Edge0, (i64, i64, u32), FE>,
    verts: ExtSort<'t, Vert, (u64, u64, i64), FV>,
    tmp: &'t TmpDir,
}

impl<FE: Fn(&Edge0) -> (i64, i64, u32) + Sync, FV: Fn(&Vert) -> (u64, u64, i64) + Sync> Cutter<'_, FE, FV> {
    fn vert(&self, n: &WayNodeC) -> Vert {
        let (lat, lon) = (n.lat.get(), n.lon.get());
        let (x, y) = grid::tile_of_e7(lat, lon, self.zoom);
        Vert {
            cell: grid::morton(x, y).into(),
            point: grid::morton_e7(lat, lon).into(),
            osm: n.node,
            lat: n.lat,
            lon: n.lon,
        }
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
        let point = |n: &WayNodeC| (n.lat.get(), n.lon.get());
        self.interior.clear();
        self.interior.extend(nodes[lo + 1..hi].iter().map(point));
        self.geom_buf.clear();
        encode_geometry(&mut self.geom_buf, point(&nodes[lo]), &self.interior);
        self.geom0.write_all(&self.geom_buf)?;
        self.tmp.added(self.geom_buf.len() as u64);
        let (a, b) = (&nodes[lo], &nodes[hi]);
        self.edges.push(Edge0 {
            a: a.node,
            way_id: way.id,
            b: b.node,
            rank_lo: (lo as u32).into(),
            attrs: attrs.into(),
            len_dm: len_dm(nodes[lo..=hi].iter().map(point)).into(),
            geom_off: self.geom_pos.into(),
            geom_len: (self.geom_buf.len() as u32).into(),
        })?;
        self.geom_pos += self.geom_buf.len() as u64;
        let (va, vb) = (self.vert(a), self.vert(b));
        self.verts.push(va)?;
        self.verts.push(vb)?;
        Ok(())
    }

    /// Découpe de `build_graph` (`cut_way`), sur les références de la way.
    fn way(
        &mut self,
        way: &WayFile,
        attrs: u64,
        nodes: &[WayNodeC],
        stats: &mut BuildStats,
    ) -> Result<(), BuildError> {
        cut_way(
            nodes.len(),
            |k| nodes[k].lat.get() != MISSING_LAT,
            |k| nodes[k].multi != 0,
            |i, k| nodes[i].node == nodes[k].node,
            stats,
            |lo, hi| self.emit(way, attrs, nodes, lo, hi),
        )
    }
}

// --- Construction ------------------------------------------------------------------------------------

fn edge_key_a(e: &Edge0) -> (i64, i64, u32) {
    (e.a.get(), e.way_id.get(), e.rank_lo.get())
}
fn edge_key_b(e: &Edge0) -> (i64, i64, u32) {
    (e.b.get(), e.way_id.get(), e.rank_lo.get())
}
fn edge_key_final(e: &Edge0) -> (i64, i64, i64, u32) {
    (e.a.get(), e.way_id.get(), e.b.get(), e.rank_lo.get())
}

/// Id global d'un id OSM, en avançant dans `ids` (trié par OSM) au rythme des arêtes (triées par la même
/// extrémité).
fn gid_of(ids: &mut Peekable<RecReader<'_, OsmGid>, OsmGid>, osm: i64) -> Result<I64, BuildError> {
    while let Some(g) = ids.peek()? {
        if g.osm.get() < osm {
            ids.pull()?;
            continue;
        }
        if g.osm.get() == osm {
            return Ok(i64::from(g.gid.get()).into());
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
        wn: ExtSort::new(&tmp, "waynode", half, wn_key as fn(&WayNode) -> WnKey),
        rb: ExtSort::new(&tmp, "routes", (budget / 8).max(1), rb_key as fn(&RouteBits) -> RbKey),
        scratch: Vec::new(),
        last_way: i64::MIN,
        last_relation: i64::MIN,
        stats,
    };
    src.scan(&mut p1)?;
    let Pass1Ext { ways, wn, rb, stats: s, .. } = p1;
    stats = s;
    let (ways_path, n_ways) = ways.finish()?;
    stats.ways_kept = n_ways;
    lap(&mut stats, &mut clock, "E1 passe 1 : ways et relations");

    // E2–E3 — coordonnées par jointure avec le flux des nœuds, jonctions, retour à l'ordre des ways.
    let mut wn = Peekable::new(wn.finish()?);
    let mut wnc = ExtSort::new(&tmp, "waynodec", half, |r: &WayNodeC| (r.way.get(), r.rank.get()));
    let mut group: Vec<WayNode> = Vec::new();
    // Compté sur 64 bits : aucun index des nœuds utilisés ici, donc pas la limite de 2^31 de `build_graph`.
    let mut used_nodes = 0u64;
    let mut last_node = i64::MIN;
    // Une série de références au même nœud : sa coordonnée (ou l'absence) et `multi`.
    let flush = |group: &mut Vec<WayNode>, coord: Option<(i32, i32)>, wnc: &mut ExtSort<_, _, _>| {
        let multi = u8::from(group.len() >= 2);
        let (lat, lon) = coord.unwrap_or((MISSING_LAT, 0));
        for r in group.drain(..) {
            wnc.push(WayNodeC {
                way: r.way,
                rank: r.rank,
                node: r.node,
                lat: lat.into(),
                lon: lon.into(),
                multi,
            })?;
        }
        Ok::<(), std::io::Error>(())
    };
    // Prend dans `group` toutes les références au prochain nœud référencé.
    fn take_group(
        wn: &mut Peekable<impl Stream<WayNode>, WayNode>,
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
        Ok(Some(first.node.get()))
    }
    src.nodes_sorted(&mut |batch| {
        for &(id, lat, lon) in batch {
            if id <= last_node {
                return Err(BuildError::NotSorted(format!("nœud {id} après {last_node}")));
            }
            last_node = id;
            // Références à des nœuds d'id plus petit : absents de l'extrait.
            while let Some(r) = wn.peek()? {
                if r.node.get() > id {
                    break;
                }
                let node = take_group(&mut wn, &mut group)?;
                used_nodes += 1;
                flush(&mut group, (node == Some(id)).then_some((lat, lon)), &mut wnc)?;
            }
        }
        Ok(())
    })?;
    while take_group(&mut wn, &mut group)?.is_some() {
        used_nodes += 1;
        flush(&mut group, None, &mut wnc)?;
    }
    drop(wn);
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
        verts: ExtSort::new(&tmp, "verts", (budget * 3 / 16).max(1), |v: &Vert| {
            (v.cell.get(), v.point.get(), v.osm.get())
        }),
        tmp: &tmp,
    };
    {
        let mut ways = RecReader::<WayFile>::open(&tmp, &ways_path, n_ways, IO_BUF, true)?;
        let mut wnc = wnc.finish()?;
        let mut rb = Peekable::new(rb.finish()?);
        let mut nodes: Vec<WayNodeC> = Vec::new();
        for w in 0..n_ways as u32 {
            let way = ways.pull()?.ok_or_else(|| BuildError::Source("ways : fin prématurée".into()))?;
            let way_id = way.id.get();
            let mut attrs = Attrs(way.attrs.get());
            while let Some(r) = rb.peek()? {
                if r.way_id.get() > way_id {
                    break;
                }
                rb.pull()?;
                if r.way_id.get() == way_id {
                    attrs = attrs.with_routes(r.bits);
                    stats.relation_way_members_matched += 1;
                } else {
                    stats.relation_way_members_not_in_graph += 1;
                }
            }
            nodes.clear();
            for rank in 0..way.n.get() {
                let r =
                    wnc.pull()?.ok_or_else(|| BuildError::Source("références : fin prématurée".into()))?;
                if r.way.get() != w || r.rank.get() != rank {
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
    let mut cells: Vec<CellSpec> = Vec::new();
    let mut ids_sort = ExtSort::new(&tmp, "osmgid", half, |g: &OsmGid| g.osm.get());
    let (nodes_path, nids_path, n_vertices) = {
        let mut verts = verts.finish()?;
        let mut nodes_w = RecWriter::<NodeRec>::create(&tmp, "nodes", IO_BUF)?;
        let mut nids_w = RecWriter::<U64>::create(&tmp, "nids", IO_BUF)?;
        let mut gid = 0u64;
        let mut last_osm: Option<i64> = None;
        let mut last_cell = u64::MAX;
        while let Some(v) = verts.pull()? {
            let osm = v.osm.get();
            if last_osm == Some(osm) {
                continue;
            }
            last_osm = Some(osm);
            let g = u32::try_from(gid).map_err(|_| BuildError::TooBig("plus de 2^32 sommets".into()))?;
            if v.cell.get() != last_cell || cells.is_empty() {
                let (x, y) = grid::tile_of_e7(v.lat.get(), v.lon.get(), zoom);
                cells.push(CellSpec { x, y, first_node: g, n_nodes: 0 });
                last_cell = v.cell.get();
            }
            cells.last_mut().expect("cellule ouverte").n_nodes += 1;
            nodes_w.push(&NodeRec { lat: v.lat, lon: v.lon })?;
            nids_w.push(&U64::new(osm as u64))?;
            ids_sort.push(OsmGid { osm: v.osm, gid: g.into() })?;
            gid += 1;
        }
        (nodes_w.finish()?.0, nids_w.finish()?.0, gid)
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
            e.a = gid_of(&mut ids, e.a.get())?;
            by_b.push(e)?;
        }
    }
    let mut final_sort = ExtSort::new(&tmp, "edges-final", half, edge_key_final);
    {
        let mut edges = by_b.finish()?;
        let mut ids = Peekable::new(RecReader::<OsmGid>::open(&tmp, &ids_path, n_ids, IO_BUF, true)?);
        while let Some(mut e) = edges.pull()? {
            e.b = gid_of(&mut ids, e.b.get())?;
            final_sort.push(e)?;
        }
    }
    lap(&mut stats, &mut clock, "E6 renumérotation des arêtes");

    // E7 — arêtes orientées, géométrie dans l'ordre des arêtes, écriture.
    let mut dirs = ExtSort::new(&tmp, "dir", (budget * 3 / 8).max(1), |d: &Dir| {
        (d.src.get(), d.dst.get(), d.edge.get())
    });
    let mut greqs = ExtSort::new(&tmp, "geomreq", (budget / 8).max(1), |g: &GeomReq| g.src.get());
    let (edges_path, eids_path, n_edges, n_directed, geom_total) = {
        let mut sorted = final_sort.finish()?;
        let mut edges_w = RecWriter::<EdgeRec>::create(&tmp, "edges", IO_BUF)?;
        let mut eids_w = RecWriter::<EdgeIdRec>::create(&tmp, "eids", IO_BUF)?;
        let (mut n_dir, mut dst) = (0u64, 0u64);
        let mut prev: Option<(i64, i64, i64)> = None;
        let mut e = 0u64;
        while let Some(ed) = sorted.pull()? {
            // Même limite que `write_graph` : moins de 2^31 arêtes (le bit 31 porte le sens).
            if e + 1 >= 1 << 31 {
                return Err(FormatError::Build("plus de 2^31 arêtes".into()).into());
            }
            let ident = (ed.a.get(), ed.way_id.get(), ed.b.get());
            stats.duplicate_identities += u64::from(prev == Some(ident));
            prev = Some(ident);
            let (a, b, id) = (ident.0 as u32, ident.2 as u32, e as u32);
            let attrs = Attrs(ed.attrs.get());
            if attrs.fwd_ok() {
                dirs.push(Dir { src: a.into(), dst: b.into(), edge: id.into() })?;
                n_dir += 1;
            }
            if attrs.bwd_ok() {
                dirs.push(Dir { src: b.into(), dst: a.into(), edge: (id | DIR_BIT).into() })?;
                n_dir += 1;
            }
            edges_w.push(&EdgeRec {
                a: a.into(),
                attrs: ed.attrs,
                len_dm: ed.len_dm,
                geom_len: ed.geom_len,
            })?;
            eids_w.push(&EdgeIdRec { way: U64::new(ident.1 as u64), a: a.into(), b: b.into() })?;
            greqs.push(GeomReq { src: ed.geom_off, dst: dst.into(), len: ed.geom_len, edge: id.into() })?;
            dst += u64::from(ed.geom_len.get());
            e += 1;
        }
        let (p, n) = edges_w.finish()?;
        (p, eids_w.finish()?.0, n, n_dir, dst)
    };
    stats.edges = n_edges;

    let mut writer = StreamWriter::create(path, cells.clone())?;
    {
        let mut nodes = RecReader::<NodeRec>::open(&tmp, &nodes_path, n_vertices, IO_BUF, true)?;
        let mut dirs = Peekable::new(dirs.finish()?);
        let mut edges = Peekable::new(RecReader::<EdgeRec>::open(&tmp, &edges_path, n_edges, IO_BUF, true)?);
        let (mut cn, mut cd, mut co) = (Vec::new(), Vec::new(), Vec::new());
        for cs in &cells {
            let nhi = cs.first_node + cs.n_nodes;
            cn.clear();
            for _ in 0..cs.n_nodes {
                let n = nodes.pull()?.ok_or_else(|| BuildError::Source("sommets : fin prématurée".into()))?;
                cn.push((n.lat.get(), n.lon.get()));
            }
            cd.clear();
            while let Some(d) = dirs.peek()? {
                if d.src.get() >= nhi {
                    break;
                }
                cd.push((d.src.get(), d.dst.get(), d.edge.get()));
                dirs.pull()?;
            }
            co.clear();
            let mut geom_len = 0u64;
            while let Some(ed) = edges.peek()? {
                if ed.a.get() >= nhi {
                    break;
                }
                let off = u32::try_from(geom_len)
                    .map_err(|_| FormatError::Build("géométrie d'une cellule > 4 Go".into()))?;
                co.push((ed.attrs.get(), ed.len_dm.get(), off));
                geom_len += u64::from(ed.geom_len.get());
                edges.pull()?;
            }
            writer.put_cell(&cn, &cd, &co, geom_len)?;
        }
        // Les cellules couvrent tous les sommets : rien ne doit rester dans les trois flux.
        if nodes.pull()?.is_some() || dirs.peek()?.is_some() || edges.peek()?.is_some() {
            return Err(BuildError::Source("écriture : sommets ou arêtes hors des cellules".into()).into());
        }
    }
    // La géométrie passe de l'ordre des ways à celui des arêtes, droit dans le fichier de sortie. Le tri des
    // arêtes orientées est fini : les paquets de géométrie ont la moitié du budget.
    let mut geom = GeomStream::new(
        &tmp,
        greqs.finish()?,
        geom0_path,
        GeomPlan {
            total: geom_total,
            n_edges,
            bucket: (half as u64).max(MIN_GEOM_BUCKET),
            max_open: MAX_OPEN_BUCKETS,
            io_budget: budget / 4,
        },
    )?;
    writer.put_geometry(&mut geom)?;
    geom.finish()?;
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
    let peak_rss_before_verify_kb = proc_status().vm_hwm_kb;
    let mut write_clock = PhaseClock::new();
    writer.finish(&stats_json, &header)?;
    stats.phases.push(write_clock.lap("STATS, en-tête, relecture et verify()"));
    Ok(ExternalReport {
        stats,
        tmp_peak_bytes: tmp.peak_bytes(),
        runs: tmp.runs(),
        peak_rss_before_verify_kb,
        stale_tmp_removed: tmp.stale_removed().to_vec(),
    })
}

// --- E7 : géométrie dans l'ordre des arêtes --------------------------------------------------------------

/// En-tête d'une géométrie dans un paquet : destination (`u64`) puis longueur (`u32`).
const GEOM_HEAD: usize = 12;
/// Ce qu'une géométrie coûte en mémoire, en plus de ses octets, quand son paquet est chargé : son en-tête et
/// son entrée d'index `(destination, position)`.
const GEOM_REC_COST: u64 = GEOM_HEAD as u64 + size_of::<(u64, usize)>() as u64;
/// Plancher du coût d'un paquet : en dessous, un budget minuscule ferait un fichier par géométrie.
const MIN_GEOM_BUCKET: u64 = 4 << 10;
/// Paquets ouverts à la fois (descripteurs de fichier) ; au-delà, `geom0` est relu en plusieurs passes.
const MAX_OPEN_BUCKETS: usize = 256;
/// Tampon d'écriture minimal d'un paquet.
const MIN_BUCKET_BUF: usize = 4 << 10;

/// Dimensions de la redistribution de la géométrie.
#[derive(Clone, Copy, Debug)]
struct GeomPlan {
    /// Octets de géométrie, toutes arêtes confondues.
    total: u64,
    n_edges: u64,
    /// Coût en mémoire d'un paquet chargé : octets de géométrie plus [`GEOM_REC_COST`] par arête.
    bucket: u64,
    /// Paquets remplis par passe sur `geom0`.
    max_open: usize,
    /// Octets des tampons d'écriture des paquets d'une passe, au total.
    io_budget: usize,
}

/// D'où viennent les requêtes triées par source : la sortie du tri, lue une fois, ou un fichier relu à chaque
/// passe.
enum Reqs<S> {
    Once(Option<S>),
    File(PathBuf, u64),
}

/// La géométrie dans l'ordre des arêtes, lue en flux par `StreamWriter::put_geometry`.
///
/// `geom0` est dans l'ordre des ways. On le parcourt séquentiellement (requêtes triées par source) et chaque
/// géométrie part dans le **paquet** de sa destination ; chaque paquet est ensuite chargé, rangé par destination
/// et rendu. Les paquets se découpent sur le **coût en mémoire** cumulé dans l'ordre des arêtes (octets de
/// géométrie plus [`GEOM_REC_COST`] par arête) : un paquet chargé tient dans `plan.bucket`, à une géométrie
/// près, que les géométries soient longues ou d'un octet. S'il faut plus de `plan.max_open` paquets, `geom0`
/// est relu en plusieurs passes, `max_open` paquets par passe : le budget tient quelle que soit la couverture.
struct GeomStream<'t, S: Stream<GeomReq>> {
    tmp: &'t TmpDir,
    geom0: PathBuf,
    reqs: Reqs<S>,
    plan: GeomPlan,
    n_buckets: u64,
    /// Prochain paquet à charger.
    next: u64,
    /// Paquets de la passe en cours, à partir du paquet `group_lo` : chemin et nombre de géométries, `None`
    /// pour un paquet vide ou déjà chargé.
    group: Vec<Option<(PathBuf, usize)>>,
    group_lo: u64,
    /// Paquet chargé, son index trié `(destination, position de l'en-tête)`, et où en est la lecture.
    bytes: Vec<u8>,
    order: Vec<(u64, usize)>,
    k: usize,
    off: usize,
    /// Octets de géométrie des paquets chargés jusqu'ici : la destination attendue de la suite.
    loaded: u64,
}

impl<'t, S: Stream<GeomReq>> GeomStream<'t, S> {
    fn new(tmp: &'t TmpDir, mut sorted: S, geom0: PathBuf, plan: GeomPlan) -> Result<Self, BuildError> {
        let cost = plan.total + plan.n_edges * GEOM_REC_COST;
        let n_buckets = cost.div_ceil(plan.bucket.max(1)).max(1);
        let reqs = if n_buckets > plan.max_open as u64 {
            // Plusieurs passes : les requêtes triées sont relues à chacune.
            let mut w = RecWriter::<GeomReq>::create(tmp, "geomreq", IO_BUF)?;
            while let Some(r) = sorted.next_rec()? {
                w.push(&r)?;
            }
            drop(sorted);
            let (path, n) = w.finish()?;
            Reqs::File(path, n)
        } else {
            Reqs::Once(Some(sorted))
        };
        Ok(Self {
            tmp,
            geom0,
            reqs,
            plan,
            n_buckets,
            next: 0,
            group: Vec::new(),
            group_lo: 0,
            bytes: Vec::new(),
            order: Vec::new(),
            k: 0,
            off: 0,
            loaded: 0,
        })
    }

    /// Une passe sur `geom0` : remplit les paquets `next..next + max_open`.
    fn distribute(&mut self) -> Result<(), BuildError> {
        let lo = self.next;
        let hi = (lo + self.plan.max_open as u64).min(self.n_buckets);
        let n = (hi - lo) as usize;
        let per_buf = (self.plan.io_budget / n).max(MIN_BUCKET_BUF);
        let mut writers: Vec<Option<(BufWriter<File>, PathBuf, usize)>> = (0..n).map(|_| None).collect();
        let mut once = match &mut self.reqs {
            Reqs::Once(s) => s.take(),
            Reqs::File(..) => None,
        };
        let mut file = match &self.reqs {
            Reqs::File(path, count) => Some(RecReader::open(self.tmp, path, *count, IO_BUF, false)?),
            Reqs::Once(_) => None,
        };
        let reqs: &mut dyn Stream<GeomReq> = match (&mut once, &mut file) {
            (Some(s), _) => s,
            (None, Some(f)) => f,
            (None, None) => return Err(BuildError::Source("géométrie : requêtes déjà consommées".into())),
        };
        let mut g0 = BufReader::with_capacity(IO_BUF, File::open(&self.geom0)?);
        let mut pos = 0u64;
        let mut buf = Vec::new();
        while let Some(r) = reqs.next_rec()? {
            let (dst, len) = (r.dst.get(), r.len.get());
            if r.src.get() != pos {
                return Err(BuildError::Source(format!(
                    "géométrie : décalage {pos} attendu, {} lu",
                    r.src.get()
                )));
            }
            pos += u64::from(len);
            let b = (dst + u64::from(r.edge.get()) * GEOM_REC_COST) / self.plan.bucket.max(1);
            if b < lo || b >= hi {
                g0.seek_relative(i64::from(len))?;
                continue;
            }
            buf.resize(len as usize, 0);
            g0.read_exact(&mut buf)?;
            let slot = &mut writers[(b - lo) as usize];
            if slot.is_none() {
                let p = self.tmp.file(&format!("geom-bucket-{b}"));
                *slot = Some((BufWriter::with_capacity(per_buf, File::create(&p)?), p, 0));
            }
            let (w, _, count) = slot.as_mut().expect("paquet ouvert");
            w.write_all(&dst.to_le_bytes())?;
            w.write_all(&len.to_le_bytes())?;
            w.write_all(&buf)?;
            *count += 1;
            self.tmp.added(GEOM_HEAD as u64 + u64::from(len));
        }
        drop((once, file));
        self.group.clear();
        for slot in writers {
            self.group.push(match slot {
                Some((mut w, p, count)) => {
                    w.flush()?;
                    Some((p, count))
                }
                None => None,
            });
        }
        self.group_lo = lo;
        if hi == self.n_buckets {
            // Dernière passe : les sources ne servent plus.
            self.tmp.remove(&self.geom0);
            if let Reqs::File(path, _) = &self.reqs {
                self.tmp.remove(path);
            }
        }
        Ok(())
    }

    /// Charge le prochain paquet non vide, rangé par destination ; `false` s'il n'en reste pas.
    fn load(&mut self) -> Result<bool, BuildError> {
        let bad = |what: &str| BuildError::Source(format!("géométrie : paquet {what}"));
        loop {
            if self.next == self.n_buckets {
                return Ok(false);
            }
            if self.next >= self.group_lo + self.group.len() as u64 {
                self.distribute()?;
            }
            let slot = self.group[(self.next - self.group_lo) as usize].take();
            self.next += 1;
            let Some((path, count)) = slot else { continue };
            // Le paquet précédent est rendu avant de charger celui-ci : un seul en mémoire.
            (self.bytes, self.order) = (Vec::new(), Vec::new());
            self.bytes = std::fs::read(&path)?;
            self.tmp.remove(&path);
            self.order = Vec::with_capacity(count);
            let mut i = 0;
            while i < self.bytes.len() {
                let head = self.bytes.get(i..i + GEOM_HEAD).ok_or_else(|| bad("tronqué"))?;
                let dst = u64::from_le_bytes(head[..8].try_into().expect("8 octets"));
                let len = u32::from_le_bytes(head[8..].try_into().expect("4 octets")) as usize;
                self.order.push((dst, i));
                i += GEOM_HEAD + len;
            }
            if i != self.bytes.len() || self.order.len() != count {
                return Err(bad("tronqué"));
            }
            self.order.sort_unstable();
            for &(dst, at) in &self.order {
                if dst != self.loaded {
                    return Err(BuildError::Source(format!(
                        "géométrie : destination {dst}, attendu {}",
                        self.loaded
                    )));
                }
                self.loaded += self.len_at(at) as u64;
            }
            (self.k, self.off) = (0, 0);
            return Ok(true);
        }
    }

    /// Longueur de la géométrie dont l'en-tête est à `at` dans le paquet chargé.
    fn len_at(&self, at: usize) -> usize {
        u32::from_le_bytes(self.bytes[at + 8..at + GEOM_HEAD].try_into().expect("4 octets")) as usize
    }

    /// Fin de la lecture : tout a été rendu, les sources sont supprimées.
    fn finish(mut self) -> Result<(), BuildError> {
        if self.k != self.order.len() || self.load()? {
            return Err(BuildError::Source("géométrie : octets non consommés".into()));
        }
        if self.loaded != self.plan.total {
            return Err(BuildError::Source(format!(
                "géométrie : {} octets sur {}",
                self.loaded, self.plan.total
            )));
        }
        Ok(())
    }
}

impl<S: Stream<GeomReq>> Read for GeomStream<'_, S> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let mut n = 0;
        while n < out.len() {
            if self.k == self.order.len() {
                // On rend ce qu'on a avant de charger le paquet suivant.
                if n > 0 {
                    break;
                }
                let more = self.load().map_err(|e| match e {
                    BuildError::Io(e) => e,
                    e => std::io::Error::other(e.to_string()),
                })?;
                if !more {
                    break;
                }
                continue;
            }
            let at = self.order[self.k].1;
            let len = self.len_at(at);
            let rest = &self.bytes[at + GEOM_HEAD + self.off..at + GEOM_HEAD + len];
            let take = rest.len().min(out.len() - n);
            out[n..n + take].copy_from_slice(&rest[..take]);
            n += take;
            self.off += take;
            if self.off == len {
                (self.k, self.off) = (self.k + 1, 0);
            }
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Requêtes en mémoire, déjà triées par source.
    struct Mem(std::vec::IntoIter<GeomReq>);

    impl Stream<GeomReq> for Mem {
        fn next_rec(&mut self) -> std::io::Result<Option<GeomReq>> {
            Ok(self.0.next())
        }
    }

    /// `n` géométries de 1 à 40 octets, écrites dans `geom0` dans un ordre mélangé : la sortie doit les rendre
    /// dans l'ordre des arêtes, quels que soient la taille des paquets et le nombre de passes.
    fn redistributed(n: u32, bucket: u64, max_open: usize) -> (u64, u64) {
        let parent = tempfile::tempdir().unwrap();
        let tmp = TmpDir::create(parent.path()).unwrap();
        let mut x = 0x2545_F491_4F6C_DD1Du64;
        let mut rnd = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        // Arête e : `len` octets de valeur `e`. Destinations dans l'ordre des arêtes.
        let lens: Vec<u32> = (0..n).map(|_| 1 + (rnd() % 40) as u32).collect();
        let mut dst = Vec::new();
        let mut total = 0u64;
        for &l in &lens {
            dst.push(total);
            total += u64::from(l);
        }
        let mut src_order: Vec<u32> = (0..n).collect();
        for i in (1..src_order.len()).rev() {
            src_order.swap(i, (rnd() % (i as u64 + 1)) as usize);
        }
        let geom0 = tmp.file("geom0");
        let (mut bytes, mut reqs) = (Vec::new(), Vec::new());
        for &e in &src_order {
            reqs.push(GeomReq {
                src: (bytes.len() as u64).into(),
                dst: dst[e as usize].into(),
                len: lens[e as usize].into(),
                edge: e.into(),
            });
            bytes.extend(std::iter::repeat_n(e as u8, lens[e as usize] as usize));
        }
        std::fs::write(&geom0, &bytes).unwrap();
        tmp.added(bytes.len() as u64);

        let plan = GeomPlan { total, n_edges: u64::from(n), bucket, max_open, io_budget: 1 };
        let mut g = GeomStream::new(&tmp, Mem(reqs.into_iter()), geom0, plan).unwrap();
        let n_buckets = g.n_buckets;
        // Lu par petits morceaux ; un paquet chargé (octets et index) tient dans son coût, à une géométrie près.
        let mut out = vec![0u8; total as usize];
        for chunk in out.chunks_mut(64) {
            g.read_exact(chunk).unwrap();
            let held = (g.bytes.len() + g.order.capacity() * size_of::<(u64, usize)>()) as u64;
            assert!(held <= bucket.saturating_add(40 + GEOM_REC_COST), "{held} octets tenus");
        }
        let passes = g.group_lo / max_open as u64 + 1;
        g.finish().unwrap();
        let want: Vec<u8> =
            (0..n).flat_map(|e| std::iter::repeat_n(e as u8, lens[e as usize] as usize)).collect();
        assert_eq!(out, want);
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0, "fichiers temporaires restants");
        (n_buckets, passes)
    }

    #[test]
    fn geometry_is_reordered_in_one_bucket() {
        assert_eq!(redistributed(500, u64::MAX, 256), (1, 1));
    }

    #[test]
    fn geometry_is_reordered_across_buckets_and_passes() {
        // Paquets de 300 octets de coût : des dizaines de paquets, 4 par passe.
        let (n_buckets, passes) = redistributed(500, 300, 4);
        assert!(n_buckets > 40, "{n_buckets}");
        assert_eq!(passes, n_buckets.div_ceil(4));
        // Paquet plus petit qu'une géométrie : un fichier par géométrie au plus, toujours le bon ordre.
        redistributed(200, 1, 7);
    }

    #[test]
    fn no_geometry_at_all() {
        assert_eq!(redistributed(0, 1 << 20, 256), (1, 1));
    }
}
