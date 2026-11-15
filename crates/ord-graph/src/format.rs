//! Format de fichier (§3 de la note) : écriture atomique et lecteur `GraphFile`.
//!
//! Petit-boutiste, lu par `zerocopy` avec des types d'alignement 1 : aucune transmutation, aucune
//! hypothèse sur l'hôte. Le lecteur ne contient que l'en-tête, la table des sections et l'index des
//! cellules ; tout le reste est lu à la demande dans l'octet-tableau `B` (un `Mmap` ou un `Vec<u8>`).

use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use xxhash_rust::xxh3::{xxh3_64, Xxh3};
use zerocopy::little_endian::{I32, I64, U16, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

use crate::attrs::{Attrs, ATTR_LAYOUT};
use crate::build::GraphData;

pub const MAGIC: [u8; 8] = *b"ORDGRAPH";
pub const FORMAT_MAJOR: u16 = 1;
/// 1 : sections optionnelles d'une vue (`VIEW_*`, voir `view.rs`) ; un fichier de minor 0 reste lisible.
pub const FORMAT_MINOR: u16 = 1;
const ENDIAN_MARK: u32 = 0x0102_0304;
const HEADER_LEN: usize = 256;
const BLOCK_ALIGN: u64 = 4096;
const SCHEME_SLIPPY: u8 = 1;

const KIND_CELL_INDEX: u32 = 1;
const KIND_CELL_BLOCKS: u32 = 2;
const KIND_GEOMETRY: u32 = 3;
const KIND_ID_NODES: u32 = 4;
const KIND_ID_EDGES: u32 = 5;
const KIND_STATS: u32 = 6;
/// Sections d'une vue (minor 1) : méta JSON, poids par arête, chaînes d'arêtes de base, nœud de base par nœud.
pub const KIND_VIEW_META: u32 = 7;
pub const KIND_VIEW_WEIGHT: u32 = 8;
pub const KIND_VIEW_CHAIN_OFF: u32 = 9;
pub const KIND_VIEW_CHAIN: u32 = 10;
pub const KIND_VIEW_BASE_NODE: u32 = 11;
/// Drapeau : pas de somme au niveau de la section, une par entrée de l'index des cellules.
const FLAG_PER_ENTRY_HASH: u32 = 1;

const SECTION_NAMES: [(u32, &str); 11] = [
    (KIND_CELL_INDEX, "CELL_INDEX"),
    (KIND_CELL_BLOCKS, "CELL_BLOCKS"),
    (KIND_GEOMETRY, "GEOMETRY"),
    (KIND_ID_NODES, "ID_NODES"),
    (KIND_ID_EDGES, "ID_EDGES"),
    (KIND_STATS, "STATS"),
    (KIND_VIEW_META, "VIEW_META"),
    (KIND_VIEW_WEIGHT, "VIEW_WEIGHT"),
    (KIND_VIEW_CHAIN_OFF, "VIEW_CHAIN_OFF"),
    (KIND_VIEW_CHAIN, "VIEW_CHAIN"),
    (KIND_VIEW_BASE_NODE, "VIEW_BASE_NODE"),
];

fn section_name(kind: u32) -> &'static str {
    SECTION_NAMES.iter().find(|(k, _)| *k == kind).map_or("?", |(_, n)| n)
}

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("E/S : {0}")]
    Io(#[from] std::io::Error),
    #[error("ce n'est pas un fichier de graphe (magie)")]
    BadMagic,
    #[error("format_major {found} non pris en charge (attendu {expected})")]
    UnsupportedMajor { found: u16, expected: u16 },
    #[error("attr_layout {found} non pris en charge (attendu {expected})")]
    AttrLayout { found: u16, expected: u16 },
    #[error("marque d'endianness invalide")]
    Endian,
    #[error("fichier tronqué ou incohérent : {0}")]
    Truncated(String),
    #[error("somme de contrôle fausse : {0}")]
    Checksum(String),
    #[error("construction : {0}")]
    Build(String),
}

// --- Structures sur disque -------------------------------------------------------------------

#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy)]
#[repr(C)]
struct RawHeader {
    magic: [u8; 8],
    major: U16,
    minor: U16,
    attr_layout: U16,
    scheme: u8,
    zoom: u8,
    endian_mark: U32,
    nodes: U64,
    edges: U64,
    directed: U64,
    cells: U64,
    replication_timestamp: I64,
    replication_sequence: I64,
    pbf_size: U64,
    pbf_xxh3: U64,
    tool_version: [u8; 16],
    build_epoch: I64,
    sections_off: U64,
    sections_count: U32,
    reserved: [u8; 128],
    header_xxh3: U64,
}

#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy)]
#[repr(C)]
struct RawSection {
    kind: U32,
    flags: U32,
    off: U64,
    len: U64,
    xxh3: U64,
}

#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy)]
#[repr(C)]
struct RawCell {
    x: U16,
    y: U16,
    first_node: U32,
    n_nodes: U32,
    first_edge: U32,
    n_edges: U32,
    first_directed: U32,
    n_directed: U32,
    n_border: U32,
    block_off: U64,
    geom_off: U64,
    xxh3: U64,
    block_len: U32,
    geom_len: U32,
}

#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy)]
#[repr(C)]
struct NodeRec {
    lat: I32,
    lon: I32,
}

/// Arête orientée du CSR : cible (id global de nœud) et arête (31 bits) + sens (bit 31).
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy, Debug)]
#[repr(C)]
pub struct DirEdge {
    target: U32,
    edge: U32,
}

impl DirEdge {
    pub fn new(target: u32, edge_field: u32) -> Self {
        Self { target: U32::new(target), edge: U32::new(edge_field) }
    }
    pub fn target(&self) -> u32 {
        self.target.get()
    }
    /// Id d'arête avec le bit de sens (31) : l'identité de l'arête orientée.
    pub fn edge_field(&self) -> u32 {
        self.edge.get()
    }
    pub fn edge_id(&self) -> u32 {
        self.edge.get() & EDGE_MASK
    }
    /// Vrai si le sens va de `a` vers `b` (sens de la way).
    pub fn forward(&self) -> bool {
        self.edge.get() & DIR_BIT == 0
    }
}

pub const DIR_BIT: u32 = 1 << 31;
pub const EDGE_MASK: u32 = !DIR_BIT;

#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy)]
#[repr(C)]
struct OwnedRec {
    attrs: U64,
    len_dm: U32,
    geom_off: U32,
}

#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy)]
#[repr(C)]
struct BorderRec {
    directed: U32,
    cell: U32,
}

/// Clé de la section `ID_EDGES` : way OSM et extrémités (ids globaux de nœud).
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy)]
#[repr(C)]
struct EdgeIdRec {
    way: U64,
    a: U32,
    b: U32,
}

const _: () = assert!(size_of::<RawHeader>() == HEADER_LEN);
const _: () = assert!(size_of::<RawSection>() == 32);
const _: () = assert!(size_of::<RawCell>() == 64);

// --- Disposition d'un bloc ----------------------------------------------------------------------

fn align(v: u64, a: u64) -> u64 {
    v.div_ceil(a) * a
}

/// Positions des tableaux d'un bloc de cellule, déduites des seuls comptes.
#[derive(Clone, Copy, Debug)]
pub struct BlockLayout {
    pub csr_off: u64,
    pub directed_off: u64,
    pub owned_off: u64,
    pub border_off: u64,
    pub len: u64,
}

impl BlockLayout {
    pub fn new(n_nodes: u32, n_directed: u32, n_edges: u32, n_border: u32) -> Self {
        let csr_off = u64::from(n_nodes) * 8;
        let directed_off = align(csr_off + (u64::from(n_nodes) + 1) * 4, 8);
        let owned_off = directed_off + u64::from(n_directed) * 8;
        let border_off = owned_off + u64::from(n_edges) * 16;
        let len = border_off + u64::from(n_border) * 8;
        Self { csr_off, directed_off, owned_off, border_off, len }
    }
}

// --- Lecteur --------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub major: u16,
    pub minor: u16,
    pub attr_layout: u16,
    pub zoom: u8,
    pub nodes: u64,
    pub edges: u64,
    pub directed: u64,
    pub cells: u64,
    pub replication_timestamp: i64,
    pub replication_sequence: i64,
    pub pbf_size: u64,
    pub pbf_xxh3: u64,
    pub tool_version: String,
    pub build_epoch: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellInfo {
    pub x: u32,
    pub y: u32,
    pub first_node: u32,
    pub n_nodes: u32,
    pub first_edge: u32,
    pub n_edges: u32,
    pub first_directed: u32,
    pub n_directed: u32,
    pub n_border: u32,
    pub block_off: u64,
    pub geom_off: u64,
    pub xxh3: u64,
    pub block_len: u32,
    pub geom_len: u32,
}

impl CellInfo {
    pub fn padded_block_len(&self) -> u64 {
        align(u64::from(self.block_len), BLOCK_ALIGN)
    }
    fn layout(&self) -> BlockLayout {
        BlockLayout::new(self.n_nodes, self.n_directed, self.n_edges, self.n_border)
    }
}

#[derive(Clone, Copy, Debug)]
struct SectionInfo {
    kind: u32,
    flags: u32,
    off: u64,
    len: u64,
    xxh3: u64,
}

/// Une arête non orientée, vue depuis la cellule qui la possède.
#[derive(Clone, Copy, Debug)]
pub struct EdgeView {
    pub cell: usize,
    pub attrs: Attrs,
    pub len_dm: u32,
    pub geom_off: u32,
}

/// Les tableaux d'une cellule : des tranches directement dans l'octet-tableau.
#[derive(Clone, Copy)]
pub struct CellView<'a> {
    info: &'a CellInfo,
    nodes: &'a [NodeRec],
    csr: &'a [U32],
    directed: &'a [DirEdge],
    owned: &'a [OwnedRec],
    border: &'a [BorderRec],
}

impl std::fmt::Debug for CellView<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CellView").field("info", self.info).finish_non_exhaustive()
    }
}

/// Une arête possédée, décodée.
#[derive(Clone, Copy, Debug)]
pub struct OwnedView {
    pub attrs: Attrs,
    pub len_dm: u32,
    pub geom_off: u32,
}

impl<'a> CellView<'a> {
    pub fn info(&self) -> &'a CellInfo {
        self.info
    }
    pub fn node(&self, local: usize) -> (i32, i32) {
        let n = &self.nodes[local];
        (n.lat.get(), n.lon.get())
    }
    /// Arêtes orientées sortant du nœud local `local`.
    fn csr_at(&self, k: usize) -> u32 {
        self.csr[k].get()
    }
    pub fn out(&self, local: usize) -> &'a [DirEdge] {
        &self.directed[self.csr[local].get() as usize..self.csr[local + 1].get() as usize]
    }
    pub fn directed(&self) -> &'a [DirEdge] {
        self.directed
    }
    pub fn owned(&self, local: usize) -> OwnedView {
        let o = &self.owned[local];
        OwnedView { attrs: Attrs(o.attrs.get()), len_dm: o.len_dm.get(), geom_off: o.geom_off.get() }
    }
    /// Table de bord : `(arête orientée locale, cellule cible)`.
    pub fn border(&self) -> impl ExactSizeIterator<Item = (u32, u32)> + 'a {
        self.border.iter().map(|b| (b.directed.get(), b.cell.get()))
    }
}

#[derive(Debug)]
pub struct GraphFile<B> {
    data: B,
    header: Header,
    sections: Vec<SectionInfo>,
    cells: Vec<CellInfo>,
    first_node: Vec<u32>,
    first_edge: Vec<u32>,
}

/// Ce qui fixe le sens d'un id d'arête de base. `pbf_xxh3` et `replication_timestamp` sont de la provenance :
/// ils ne comptent pas dans [`GraphIdentity::same_ids`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphIdentity {
    pub format_major: u16,
    pub attr_layout: u16,
    pub zoom: u8,
    pub nodes: u64,
    pub edges: u64,
    pub directed: u64,
    pub id_edges_xxh3: u64,
    pub id_nodes_xxh3: u64,
    pub pbf_xxh3: u64,
    pub replication_timestamp: i64,
}

impl GraphIdentity {
    /// Deux graphes donnent le même sens aux ids d'arête de base (provenance exclue).
    pub fn same_ids(&self, other: &GraphIdentity) -> bool {
        (self.format_major, self.attr_layout, self.zoom, self.nodes, self.edges, self.directed)
            == (other.format_major, other.attr_layout, other.zoom, other.nodes, other.edges, other.directed)
            && (self.id_edges_xxh3, self.id_nodes_xxh3) == (other.id_edges_xxh3, other.id_nodes_xxh3)
    }
}

#[derive(Clone, Debug, Default)]
pub struct VerifyReport {
    pub sections_checked: usize,
    pub cells_checked: usize,
}

fn truncated(what: impl Into<String>) -> FormatError {
    FormatError::Truncated(what.into())
}

impl<B: AsRef<[u8]>> GraphFile<B> {
    /// Ouvre et valide l'en-tête, la table des sections et l'index des cellules (pas les blocs).
    pub fn open(data: B) -> Result<Self, FormatError> {
        let bytes = data.as_ref();
        let total = bytes.len() as u64;
        if bytes.len() < HEADER_LEN {
            return Err(truncated("en-tête"));
        }
        let raw = RawHeader::ref_from_bytes(&bytes[..HEADER_LEN]).map_err(|_| truncated("en-tête"))?;
        if raw.magic != MAGIC {
            return Err(FormatError::BadMagic);
        }
        if raw.major.get() != FORMAT_MAJOR {
            return Err(FormatError::UnsupportedMajor { found: raw.major.get(), expected: FORMAT_MAJOR });
        }
        if raw.attr_layout.get() != ATTR_LAYOUT {
            return Err(FormatError::AttrLayout { found: raw.attr_layout.get(), expected: ATTR_LAYOUT });
        }
        if raw.endian_mark.get() != ENDIAN_MARK {
            return Err(FormatError::Endian);
        }
        if xxh3_64(&bytes[..HEADER_LEN - 8]) != raw.header_xxh3.get() {
            return Err(FormatError::Checksum("en-tête".into()));
        }
        let header = Header {
            major: raw.major.get(),
            minor: raw.minor.get(),
            attr_layout: raw.attr_layout.get(),
            zoom: raw.zoom,
            nodes: raw.nodes.get(),
            edges: raw.edges.get(),
            directed: raw.directed.get(),
            cells: raw.cells.get(),
            replication_timestamp: raw.replication_timestamp.get(),
            replication_sequence: raw.replication_sequence.get(),
            pbf_size: raw.pbf_size.get(),
            pbf_xxh3: raw.pbf_xxh3.get(),
            tool_version: String::from_utf8_lossy(&raw.tool_version).trim_end_matches('\0').to_string(),
            build_epoch: raw.build_epoch.get(),
        };
        let sec_off = raw.sections_off.get();
        let sec_len = u64::from(raw.sections_count.get()) * size_of::<RawSection>() as u64;
        if sec_off.checked_add(sec_len).is_none_or(|end| end > total) {
            return Err(truncated("table des sections"));
        }
        let raw_secs = <[RawSection]>::ref_from_bytes(&bytes[sec_off as usize..(sec_off + sec_len) as usize])
            .map_err(|_| truncated("table des sections"))?;
        let mut sections = Vec::new();
        for s in raw_secs {
            let (off, len) = (s.off.get(), s.len.get());
            if off.checked_add(len).is_none_or(|end| end > total) {
                return Err(truncated(format!("section {}", section_name(s.kind.get()))));
            }
            sections.push(SectionInfo {
                kind: s.kind.get(),
                flags: s.flags.get(),
                off,
                len,
                xxh3: s.xxh3.get(),
            });
        }
        let mut g = Self {
            data,
            header,
            sections,
            cells: Vec::new(),
            first_node: Vec::new(),
            first_edge: Vec::new(),
        };
        g.load_index()?;
        Ok(g)
    }

    fn section(&self, kind: u32) -> Result<&SectionInfo, FormatError> {
        self.sections
            .iter()
            .find(|s| s.kind == kind)
            .ok_or_else(|| truncated(format!("section {} absente", section_name(kind))))
    }

    /// Octets d'une section optionnelle, si le fichier la porte.
    pub(crate) fn optional_section(&self, kind: u32) -> Option<&[u8]> {
        self.sections.iter().find(|s| s.kind == kind).map(|s| self.section_bytes(s))
    }

    /// Position et longueur `(offset, longueur)` d'une section, par son nom (tests, outils).
    pub fn section_range(&self, name: &str) -> Option<(u64, u64)> {
        self.sections.iter().find(|s| section_name(s.kind) == name).map(|s| (s.off, s.len))
    }

    fn section_bytes(&self, s: &SectionInfo) -> &[u8] {
        &self.data.as_ref()[s.off as usize..(s.off + s.len) as usize]
    }

    fn load_index(&mut self) -> Result<(), FormatError> {
        let total = self.data.as_ref().len() as u64;
        let idx = *self.section(KIND_CELL_INDEX)?;
        let bytes = self.section_bytes(&idx);
        if xxh3_64(bytes) != idx.xxh3 {
            return Err(FormatError::Checksum("index des cellules".into()));
        }
        let raw = <[RawCell]>::ref_from_bytes(bytes).map_err(|_| truncated("index des cellules"))?;
        if raw.len() as u64 != self.header.cells {
            return Err(truncated("nombre de cellules"));
        }
        let (mut nodes, mut edges, mut directed) = (0u64, 0u64, 0u64);
        let mut cells = Vec::with_capacity(raw.len());
        for (i, c) in raw.iter().enumerate() {
            let info = CellInfo {
                x: u32::from(c.x.get()),
                y: u32::from(c.y.get()),
                first_node: c.first_node.get(),
                n_nodes: c.n_nodes.get(),
                first_edge: c.first_edge.get(),
                n_edges: c.n_edges.get(),
                first_directed: c.first_directed.get(),
                n_directed: c.n_directed.get(),
                n_border: c.n_border.get(),
                block_off: c.block_off.get(),
                geom_off: c.geom_off.get(),
                xxh3: c.xxh3.get(),
                block_len: c.block_len.get(),
                geom_len: c.geom_len.get(),
            };
            let coherent = u64::from(info.first_node) == nodes
                && u64::from(info.first_edge) == edges
                && u64::from(info.first_directed) == directed
                && u64::from(info.block_len) == info.layout().len
                && info.n_nodes > 0;
            if !coherent {
                return Err(truncated(format!("cellule {i} : comptes incohérents")));
            }
            if info.block_off.checked_add(u64::from(info.block_len)).is_none_or(|e| e > total)
                || info.geom_off.checked_add(u64::from(info.geom_len)).is_none_or(|e| e > total)
            {
                return Err(truncated(format!("cellule {i} (x={}, y={}) hors du fichier", info.x, info.y)));
            }
            nodes += u64::from(info.n_nodes);
            edges += u64::from(info.n_edges);
            directed += u64::from(info.n_directed);
            cells.push(info);
        }
        if nodes != self.header.nodes || edges != self.header.edges || directed != self.header.directed {
            return Err(truncated("totaux de l'en-tête"));
        }
        if self.section(KIND_ID_NODES)?.len != nodes * 8 || self.section(KIND_ID_EDGES)?.len != edges * 16 {
            return Err(truncated("sections d'identifiants"));
        }
        // Sections d'une vue : présentes toutes ou aucune, longueurs liées aux comptes.
        let view_kinds =
            [KIND_VIEW_META, KIND_VIEW_WEIGHT, KIND_VIEW_CHAIN_OFF, KIND_VIEW_CHAIN, KIND_VIEW_BASE_NODE];
        let present = view_kinds.iter().filter(|k| self.sections.iter().any(|s| s.kind == **k)).count();
        if present != 0 {
            let len_of = |k: u32| self.sections.iter().find(|s| s.kind == k).map(|s| s.len);
            let ok = present == view_kinds.len()
                && len_of(KIND_VIEW_WEIGHT) == Some(edges * 4)
                && len_of(KIND_VIEW_CHAIN_OFF) == Some((edges + 1) * 4)
                && len_of(KIND_VIEW_BASE_NODE) == Some(nodes * 4)
                && len_of(KIND_VIEW_CHAIN).is_some_and(|l| l % 4 == 0);
            if !ok {
                return Err(truncated("sections de vue"));
            }
        }
        self.first_node = cells.iter().map(|c| c.first_node).collect();
        self.first_edge = cells.iter().map(|c| c.first_edge).collect();
        self.cells = cells;
        Ok(())
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Taille du fichier en octets.
    pub fn len(&self) -> u64 {
        self.data.as_ref().len() as u64
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn section_lengths(&self) -> Vec<(&'static str, u64)> {
        self.sections.iter().map(|s| (section_name(s.kind), s.len)).collect()
    }

    pub fn n_cells(&self) -> usize {
        self.cells.len()
    }

    pub fn cell_info(&self, i: usize) -> &CellInfo {
        &self.cells[i]
    }

    pub fn cell_xy(&self, i: usize) -> (u32, u32) {
        (self.cells[i].x, self.cells[i].y)
    }

    /// Index de la cellule d'une tuile `(x, y)`, si elle existe (dichotomie sur l'ordre de Morton).
    pub fn find_cell(&self, x: u32, y: u32) -> Option<usize> {
        let key = crate::grid::morton(x, y);
        self.cells.binary_search_by_key(&key, |c| crate::grid::morton(c.x, c.y)).ok()
    }

    pub fn cell(&self, i: usize) -> CellView<'_> {
        let info = &self.cells[i];
        let lay = info.layout();
        let block = &self.data.as_ref()[info.block_off as usize..(info.block_off + lay.len) as usize];
        let sl = |from: u64, to: u64| &block[from as usize..to as usize];
        const OK: &str = "disposition validée à l'ouverture";
        CellView {
            info,
            nodes: <[NodeRec]>::ref_from_bytes(sl(0, lay.csr_off)).expect(OK),
            csr: <[U32]>::ref_from_bytes(sl(lay.csr_off, lay.csr_off + (u64::from(info.n_nodes) + 1) * 4))
                .expect(OK),
            directed: <[DirEdge]>::ref_from_bytes(sl(lay.directed_off, lay.owned_off)).expect(OK),
            owned: <[OwnedRec]>::ref_from_bytes(sl(lay.owned_off, lay.border_off)).expect(OK),
            border: <[BorderRec]>::ref_from_bytes(sl(lay.border_off, lay.len)).expect(OK),
        }
    }

    /// Cellule d'un nœud global.
    pub fn node_cell(&self, gid: u32) -> usize {
        self.first_node.partition_point(|&f| f <= gid) - 1
    }

    /// Cellule d'une arête globale.
    pub fn edge_cell(&self, edge: u32) -> usize {
        self.first_edge.partition_point(|&f| f <= edge) - 1
    }

    pub fn node_pos(&self, gid: u32) -> (i32, i32) {
        let ci = self.node_cell(gid);
        self.cell(ci).node((gid - self.cells[ci].first_node) as usize)
    }

    pub fn edge(&self, edge: u32) -> EdgeView {
        let ci = self.edge_cell(edge);
        let o = self.cell(ci).owned((edge - self.cells[ci].first_edge) as usize);
        EdgeView { cell: ci, attrs: o.attrs, len_dm: o.len_dm, geom_off: o.geom_off }
    }

    fn id_edges(&self) -> &[EdgeIdRec] {
        let s = self.section(KIND_ID_EDGES).expect("validée à l'ouverture");
        <[EdgeIdRec]>::ref_from_bytes(self.section_bytes(s)).expect("longueur validée")
    }

    fn id_nodes(&self) -> &[U64] {
        let s = self.section(KIND_ID_NODES).expect("validée à l'ouverture");
        <[U64]>::ref_from_bytes(self.section_bytes(s)).expect("longueur validée")
    }

    /// Extrémités `(a, b)` d'une arête, en ids globaux de nœud (ordre de la way).
    pub fn edge_ends(&self, edge: u32) -> (u32, u32) {
        let r = &self.id_edges()[edge as usize];
        (r.a.get(), r.b.get())
    }

    pub fn osm_node_id(&self, gid: u32) -> u64 {
        self.id_nodes()[gid as usize].get()
    }

    /// Identité stable d'une arête : `(id de way, id OSM du nœud de départ, id OSM du nœud d'arrivée)`.
    pub fn identity(&self, edge: u32) -> (u64, u64, u64) {
        let r = &self.id_edges()[edge as usize];
        (r.way.get(), self.osm_node_id(r.a.get()), self.osm_node_id(r.b.get()))
    }

    /// Octets de géométrie de la cellule `cell`, ou vide si l'index les situe hors de la section (fichier
    /// non vérifié par `verify()`).
    fn geometry_bytes(&self, cell: usize) -> &[u8] {
        let info = &self.cells[cell];
        let Ok(s) = self.section(KIND_GEOMETRY) else { return &[] };
        let all = self.section_bytes(s);
        let start = info.geom_off.checked_sub(s.off).map(|v| v as usize);
        start.and_then(|st| all.get(st..st.checked_add(info.geom_len as usize)?)).unwrap_or(&[])
    }

    /// Points intérieurs de l'arête (extrémités exclues), de `a` vers `b`, en E7 `(lat, lon)`.
    ///
    /// Sur un fichier non vérifié, une géométrie hors limites donne une liste vide ou tronquée, pas une panique.
    pub fn geometry(&self, edge: u32) -> Vec<(i32, i32)> {
        let ev = self.edge(edge);
        let buf = self.geometry_bytes(ev.cell);
        let mut pos = ev.geom_off as usize;
        let (mut lat, mut lon) = self.node_pos(self.edge_ends(edge).0);
        let n = (read_varint(buf, &mut pos) as usize).min(buf.len());
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            if pos >= buf.len() {
                break;
            }
            lat = lat.wrapping_add(unzigzag(read_varint(buf, &mut pos)));
            lon = lon.wrapping_add(unzigzag(read_varint(buf, &mut pos)));
            out.push((lat, lon));
        }
        out
    }

    /// Cohérence interne des blocs, que `open()` ne vérifie pas : décalages CSR croissants et bornés, cibles
    /// et arêtes dans les comptes de l'en-tête, table de bord dans le bloc, géométrie dans sa section.
    /// **Seul un fichier qui a passé `verify()` (donc ce contrôle) se cherche sans risque de panique** : sur un
    /// fichier corrompu, le lecteur paresseux (`cell`, `out`, `geometry`) peut sortir de ses tableaux.
    pub fn validate_structure(&self) -> Result<(), FormatError> {
        let (nodes, edges) = (self.header.nodes, self.header.edges);
        let bad = |i: usize, c: &CellInfo, what: &str| {
            truncated(format!("cellule {i} (x={}, y={}) : {what}", c.x, c.y))
        };
        // Extrémités des arêtes (ID_EDGES) : `geometry` et les comptes les lisent comme des nœuds globaux.
        if self.id_edges().iter().any(|r| u64::from(r.a.get()) >= nodes || u64::from(r.b.get()) >= nodes) {
            return Err(truncated("ID_EDGES : extrémité d'arête hors des nœuds"));
        }
        for (i, c) in self.cells.iter().enumerate() {
            let v = self.cell(i);
            let n = c.n_nodes as usize;
            let csr: Vec<u32> = (0..=n).map(|k| v.csr_at(k)).collect();
            if csr[0] != 0 || csr[n] != c.n_directed || csr.windows(2).any(|w| w[0] > w[1]) {
                return Err(bad(i, c, "décalages CSR"));
            }
            if v.directed().iter().any(|t| u64::from(t.target()) >= nodes || u64::from(t.edge_id()) >= edges)
            {
                return Err(bad(i, c, "arête orientée hors comptes"));
            }
            if v.border().any(|(d, cell)| d >= c.n_directed || cell as usize >= self.cells.len()) {
                return Err(bad(i, c, "table de bord"));
            }
            let in_section = self.section(KIND_GEOMETRY).is_ok_and(|s| {
                c.geom_off >= s.off
                    && c.geom_off.checked_add(u64::from(c.geom_len)).is_some_and(|e| e <= s.off + s.len)
            });
            if !in_section {
                return Err(bad(i, c, "géométrie hors section"));
            }
            let geom = self.geometry_bytes(i);
            for k in 0..c.n_edges as usize {
                let mut pos = v.owned(k).geom_off as usize;
                let Some(count) = checked_varint(geom, &mut pos) else {
                    return Err(bad(i, c, "géométrie tronquée"));
                };
                for _ in 0..u64::from(count) * 2 {
                    if checked_varint(geom, &mut pos).is_none() {
                        return Err(bad(i, c, "géométrie tronquée"));
                    }
                }
            }
        }
        self.validate_view()
    }

    /// Vérifie les sommes de toutes les sections et de tous les blocs ; l'erreur nomme la cellule.
    pub fn verify(&self) -> Result<VerifyReport, FormatError> {
        let mut report = VerifyReport::default();
        for s in &self.sections {
            if s.flags & FLAG_PER_ENTRY_HASH != 0 {
                continue;
            }
            if xxh3_64(self.section_bytes(s)) != s.xxh3 {
                return Err(FormatError::Checksum(format!("section {}", section_name(s.kind))));
            }
            report.sections_checked += 1;
        }
        let bytes = self.data.as_ref();
        for (i, c) in self.cells.iter().enumerate() {
            let block = &bytes[c.block_off as usize..(c.block_off + u64::from(c.block_len)) as usize];
            if xxh3_64(block) != c.xxh3 {
                return Err(FormatError::Checksum(format!("cellule {i} (x={}, y={})", c.x, c.y)));
            }
            report.cells_checked += 1;
        }
        self.validate_structure()?;
        Ok(report)
    }

    /// Somme de chaque section hors `STATS` (seule partie non déterministe : durées, RSS, nombre de fils).
    pub fn section_digests(&self) -> Vec<(&'static str, u64)> {
        self.sections
            .iter()
            .filter(|s| s.kind != KIND_STATS)
            .map(|s| (section_name(s.kind), xxh3_64(self.section_bytes(s))))
            .collect()
    }

    /// Identité du graphe pour les fichiers qui référencent ses ids d'arête (surcouches) : comptes et sommes
    /// **stockées** de `ID_EDGES` et `ID_NODES`, sans recalcul. Fiable si `verify()` a passé.
    pub fn graph_identity(&self) -> GraphIdentity {
        let sum = |kind| self.section(kind).map_or(0, |s| s.xxh3);
        GraphIdentity {
            format_major: self.header.major,
            attr_layout: self.header.attr_layout,
            zoom: self.header.zoom,
            nodes: self.header.nodes,
            edges: self.header.edges,
            directed: self.header.directed,
            id_edges_xxh3: sum(KIND_ID_EDGES),
            id_nodes_xxh3: sum(KIND_ID_NODES),
            pbf_xxh3: self.header.pbf_xxh3,
            replication_timestamp: self.header.replication_timestamp,
        }
    }

    /// Contenu brut de la section `STATS` (JSON de construction).
    pub fn stats_json(&self) -> Result<&[u8], FormatError> {
        Ok(self.section_bytes(self.section(KIND_STATS)?))
    }

    pub fn into_inner(self) -> B {
        self.data
    }
}

// --- Varint -----------------------------------------------------------------------------------------

fn zigzag(v: i32) -> u32 {
    ((v << 1) ^ (v >> 31)) as u32
}

fn unzigzag(v: u32) -> i32 {
    ((v >> 1) as i32) ^ -((v & 1) as i32)
}

fn put_varint(out: &mut Vec<u8>, mut v: u32) {
    while v >= 0x80 {
        out.push((v & 0x7f) as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// Comme `read_varint`, mais `None` si le varint sort du tampon ou dépasse 5 octets.
fn checked_varint(buf: &[u8], pos: &mut usize) -> Option<u32> {
    let (mut v, mut shift) = (0u32, 0);
    loop {
        let b = *buf.get(*pos)?;
        *pos += 1;
        v |= u32::from(b & 0x7f).checked_shl(shift).unwrap_or(0);
        if b & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
        if shift > 28 {
            return None;
        }
    }
}

fn read_varint(buf: &[u8], pos: &mut usize) -> u32 {
    let (mut v, mut shift) = (0u32, 0);
    while let Some(&b) = buf.get(*pos) {
        *pos += 1;
        v |= u32::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 28 {
            break;
        }
    }
    v
}

// --- Écriture ---------------------------------------------------------------------------------------

struct Out<W: Write> {
    w: W,
    pos: u64,
}

impl<W: Write> Out<W> {
    fn put(&mut self, b: &[u8]) -> std::io::Result<()> {
        self.w.write_all(b)?;
        self.pos += b.len() as u64;
        Ok(())
    }
    fn pad_to(&mut self, a: u64) -> std::io::Result<()> {
        let n = align(self.pos, a) - self.pos;
        self.put(&vec![0u8; n as usize])
    }
}

fn node_cell_of(cells: &[crate::build::CellSpec], gid: u32) -> usize {
    cells.partition_point(|c| c.first_node <= gid) - 1
}

/// Écrit le graphe : fichier temporaire à côté, en-tête en dernier, `fsync`, relecture et `verify()`,
/// `chmod 0444`, `rename`. Jamais de réécriture sur place (le lecteur projette le fichier en mémoire).
pub fn write_graph(
    path: &Path,
    data: &mut GraphData,
    tool_version: &str,
    build_epoch: i64,
) -> Result<(), FormatError> {
    let tmp = path.with_extension("ordg.tmp");
    let _ = std::fs::remove_file(&tmp);
    let file = std::fs::OpenOptions::new().create_new(true).write(true).open(&tmp)?;
    let result = write_inner(file, &tmp, data, tool_version, build_epoch).and_then(|()| {
        let bytes = std::fs::read(&tmp)?;
        let g = GraphFile::open(bytes)?;
        g.verify()?;
        Ok(())
    });
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o444))?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn write_inner(
    mut file: std::fs::File,
    _tmp: &Path,
    data: &mut GraphData,
    tool_version: &str,
    build_epoch: i64,
) -> Result<(), FormatError> {
    if data.edges.len() >= 1 << 31 {
        return Err(FormatError::Build("plus de 2^31 arêtes".into()));
    }
    // Arêtes orientées, triées par (source, cible, arête).
    let mut dir: Vec<(u32, u32, u32)> = Vec::with_capacity(data.edges.len() * 2);
    for (e, ed) in data.edges.iter().enumerate() {
        let a = Attrs(ed.attrs);
        if a.fwd_ok() {
            dir.push((ed.a, ed.b, e as u32));
        }
        if a.bwd_ok() {
            dir.push((ed.b, ed.a, e as u32 | DIR_BIT));
        }
    }
    dir.sort_unstable();

    let mut out = Out { w: std::io::BufWriter::with_capacity(1 << 20, &mut file), pos: 0 };
    out.put(&vec![0u8; BLOCK_ALIGN as usize])?; // en-tête et table des sections, écrits en dernier

    let mut cell_entries: Vec<RawCell> = Vec::with_capacity(data.cells.len());
    let mut geoms: Vec<Vec<u8>> = Vec::with_capacity(data.cells.len());
    let (mut dpos, mut epos, mut first_directed) = (0usize, 0usize, 0u32);
    let mut block = Vec::new();
    for (ci, cs) in data.cells.iter().enumerate() {
        let (nlo, nhi) = (cs.first_node, cs.first_node + cs.n_nodes);
        let dend = dpos + dir[dpos..].partition_point(|d| d.0 < nhi);
        let cell_dir = &dir[dpos..dend];
        let eend = epos + data.edges[epos..].partition_point(|e| e.a < nhi);
        let cell_edges = &data.edges[epos..eend];
        debug_assert!(cell_edges.iter().all(|e| e.a >= nlo));

        block.clear();
        for gid in nlo..nhi {
            let n = &data.nodes[gid as usize];
            block.extend_from_slice(NodeRec { lat: I32::new(n.lat), lon: I32::new(n.lon) }.as_bytes());
        }
        let mut k = 0usize;
        for local in 0..cs.n_nodes {
            block.extend_from_slice(U32::new(k as u32).as_bytes());
            while k < cell_dir.len() && cell_dir[k].0 == nlo + local {
                k += 1;
            }
        }
        block.extend_from_slice(U32::new(cell_dir.len() as u32).as_bytes());
        while block.len() % 8 != 0 {
            block.push(0);
        }
        for d in cell_dir {
            block.extend_from_slice(DirEdge::new(d.1, d.2).as_bytes());
        }
        let mut geom = Vec::new();
        for ed in cell_edges {
            let off = u32::try_from(geom.len())
                .map_err(|_| FormatError::Build("géométrie d'une cellule > 4 Go".into()))?;
            block.extend_from_slice(
                OwnedRec { attrs: U64::new(ed.attrs), len_dm: U32::new(ed.len_dm), geom_off: U32::new(off) }
                    .as_bytes(),
            );
            let interior = data.interior(ed);
            put_varint(&mut geom, interior.len() as u32);
            let na = &data.nodes[ed.a as usize];
            let (mut lat, mut lon) = (na.lat, na.lon);
            for (la, lo) in interior {
                put_varint(&mut geom, zigzag(la.wrapping_sub(lat)));
                put_varint(&mut geom, zigzag(lo.wrapping_sub(lon)));
                (lat, lon) = (la, lo);
            }
        }
        let mut n_border = 0u32;
        for (li, d) in cell_dir.iter().enumerate() {
            let tc = node_cell_of(&data.cells, d.1);
            if tc != ci {
                block.extend_from_slice(
                    BorderRec { directed: U32::new(li as u32), cell: U32::new(tc as u32) }.as_bytes(),
                );
                n_border += 1;
            }
        }
        let layout = BlockLayout::new(cs.n_nodes, cell_dir.len() as u32, cell_edges.len() as u32, n_border);
        debug_assert_eq!(block.len() as u64, layout.len);
        let block_off = out.pos;
        out.put(&block)?;
        out.pad_to(BLOCK_ALIGN)?;
        cell_entries.push(RawCell {
            x: U16::new(cs.x as u16),
            y: U16::new(cs.y as u16),
            first_node: U32::new(nlo),
            n_nodes: U32::new(cs.n_nodes),
            first_edge: U32::new(epos as u32),
            n_edges: U32::new(cell_edges.len() as u32),
            first_directed: U32::new(first_directed),
            n_directed: U32::new(cell_dir.len() as u32),
            n_border: U32::new(n_border),
            block_off: U64::new(block_off),
            geom_off: U64::new(0),
            xxh3: U64::new(xxh3_64(&block)),
            block_len: U32::new(block.len() as u32),
            geom_len: U32::new(geom.len() as u32),
        });
        geoms.push(geom);
        first_directed += cell_dir.len() as u32;
        (dpos, epos) = (dend, eend);
    }
    let blocks_end = out.pos;

    // Géométrie : 4 Kio par cellule, somme de la section entière.
    out.pad_to(BLOCK_ALIGN)?;
    let geom_start = out.pos;
    let mut geom_hash = Xxh3::new();
    for (entry, geom) in cell_entries.iter_mut().zip(&geoms) {
        entry.geom_off = U64::new(out.pos);
        if geom.is_empty() {
            continue;
        }
        out.put(geom)?;
        geom_hash.update(geom);
        let pad = (align(out.pos, BLOCK_ALIGN) - out.pos) as usize;
        let zeros = vec![0u8; pad];
        out.put(&zeros)?;
        geom_hash.update(&zeros);
    }
    let geom_end = out.pos;
    drop(geoms);

    let mut sections: Vec<RawSection> = Vec::new();
    sections.push(RawSection {
        kind: U32::new(KIND_CELL_BLOCKS),
        flags: U32::new(FLAG_PER_ENTRY_HASH),
        off: U64::new(BLOCK_ALIGN),
        len: U64::new(blocks_end - BLOCK_ALIGN),
        xxh3: U64::new(0),
    });
    sections.push(RawSection {
        kind: U32::new(KIND_GEOMETRY),
        flags: U32::new(0),
        off: U64::new(geom_start),
        len: U64::new(geom_end - geom_start),
        xxh3: U64::new(geom_hash.digest()),
    });

    let small = |kind: u32, bytes: &[u8], out: &mut Out<_>| -> std::io::Result<RawSection> {
        out.pad_to(64)?;
        let off = out.pos;
        out.put(bytes)?;
        Ok(RawSection {
            kind: U32::new(kind),
            flags: U32::new(0),
            off: U64::new(off),
            len: U64::new(bytes.len() as u64),
            xxh3: U64::new(xxh3_64(bytes)),
        })
    };
    sections.push(small(KIND_CELL_INDEX, cell_entries.as_slice().as_bytes(), &mut out)?);
    let ids: Vec<U64> = data.nodes.iter().map(|n| U64::new(n.osm_id as u64)).collect();
    sections.push(small(KIND_ID_NODES, ids.as_slice().as_bytes(), &mut out)?);
    drop(ids);
    let eids: Vec<EdgeIdRec> = data
        .edges
        .iter()
        .map(|e| EdgeIdRec { way: U64::new(e.way_id as u64), a: U32::new(e.a), b: U32::new(e.b) })
        .collect();
    sections.push(small(KIND_ID_EDGES, eids.as_slice().as_bytes(), &mut out)?);
    drop(eids);
    for extra in &data.extras {
        sections.push(small(extra.kind, &extra.bytes, &mut out)?);
    }
    let stats_json = serde_json::to_vec(&data.stats).map_err(|e| FormatError::Build(e.to_string()))?;
    sections.push(small(KIND_STATS, &stats_json, &mut out)?);
    out.w.flush()?;
    drop(out);

    // En-tête en dernier.
    let table = sections.as_slice().as_bytes();
    let mut tv = [0u8; 16];
    let n = tool_version.len().min(16);
    tv[..n].copy_from_slice(&tool_version.as_bytes()[..n]);
    let mut header = RawHeader {
        magic: MAGIC,
        major: U16::new(FORMAT_MAJOR),
        minor: U16::new(FORMAT_MINOR),
        attr_layout: U16::new(ATTR_LAYOUT),
        scheme: SCHEME_SLIPPY,
        zoom: data.zoom,
        endian_mark: U32::new(ENDIAN_MARK),
        nodes: U64::new(data.nodes.len() as u64),
        edges: U64::new(data.edges.len() as u64),
        directed: U64::new(dir.len() as u64),
        cells: U64::new(data.cells.len() as u64),
        replication_timestamp: I64::new(data.meta.replication_timestamp),
        replication_sequence: I64::new(data.meta.replication_sequence),
        pbf_size: U64::new(data.meta.size),
        pbf_xxh3: U64::new(data.meta.xxh3),
        tool_version: tv,
        build_epoch: I64::new(build_epoch),
        sections_off: U64::new(HEADER_LEN as u64),
        sections_count: U32::new(sections.len() as u32),
        reserved: [0; 128],
        header_xxh3: U64::new(0),
    };
    header.header_xxh3 = U64::new(xxh3_64(&header.as_bytes()[..HEADER_LEN - 8]));
    file.seek(SeekFrom::Start(0))?;
    file.write_all(header.as_bytes())?;
    file.write_all(table)?;
    file.sync_all()?;
    Ok(())
}
