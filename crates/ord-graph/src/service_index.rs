//! Fichier d'index de service (`.ordi`, note `docs/design/technique/ord-loops-planete.md` §5) : ce que le générateur
//! calculait à chaque démarrage et qui ne dépend que des fichiers du graphe, calculé une fois hors ligne et projeté
//! par le service.
//!
//! Un fichier par graphe de base, pour ses deux vues `road` et `gravel` :
//! - par vue, l'index des chaînes **étrangères** à chaque tuile `(clé de tuile, id de chaîne)` et les longueurs de
//!   composante fortement connexe **courtes** `(id de chaîne, longueur dm)` (sous `cap_dm`), et la plus longue
//!   chaîne ;
//! - pour la base, les arêtes plus larges qu'une cellule (par un segment, par un bout) et un nœud sur
//!   `node_sample_every` avec la boîte de tous les nœuds (le contrôle du polygone de couverture).
//!
//! Le fichier est lié à la base et aux deux vues (comptes, empreinte du PBF, dates de construction) :
//! [`IndexFile::check_for`] le refuse sinon. Ce module ne calcule rien : le moteur fournit les tables
//! ([`IndexData`]), ce module les écrit et les relit.

use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use xxhash_rust::xxh3::{xxh3_64, Xxh3};
use zerocopy::little_endian::{I32, I64, U16, U32, U64};
use zerocopy::{FromBytes, FromZeros, Immutable, IntoBytes, KnownLayout, Unaligned};

use crate::format::GraphFile;
use crate::view::ViewKind;

pub const INDEX_MAGIC: [u8; 8] = *b"ORDINDX\0";
pub const INDEX_MAJOR: u16 = 1;
pub const INDEX_MINOR: u16 = 0;
/// Un nœud de base sur 16 dans l'échantillon (celui du contrôle du polygone du service).
pub const NODE_SAMPLE_EVERY: u32 = 16;

const HEADER_LEN: usize = 256;
const N_SECTIONS: usize = 8;
const ENTRY_LEN: usize = 24;

/// Sections, dans l'ordre du fichier.
const SECTION_NAMES: [&str; N_SECTIONS] = [
    "FOREIGN_ROAD",
    "SHORT_ROAD",
    "FOREIGN_GRAVEL",
    "SHORT_GRAVEL",
    "WIDE_SEG",
    "WIDE_END",
    "NODE_SAMPLE",
    "META",
];
const S_FOREIGN: [usize; 2] = [0, 2];
const S_SHORT: [usize; 2] = [1, 3];
const S_WIDE_SEG: usize = 4;
const S_WIDE_END: usize = 5;
const S_NODE_SAMPLE: usize = 6;
const S_META: usize = 7;

/// Une paire d'entiers du fichier : `(clé de tuile, chaîne)` ou `(chaîne, longueur dm)`, ou `(lat, lon)` E7.
pub type Pair = [U32; 2];

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("fichier d'index de service tronqué : {0}")]
    Truncated(String),
    #[error("ce n'est pas un fichier d'index de service (signature)")]
    BadMagic,
    #[error("version majeure {0} du fichier d'index de service non gérée")]
    UnsupportedMajor(u16),
    #[error("fichier d'index de service corrompu : {0}")]
    Corrupt(String),
    #[error("fichier d'index de service d'un autre graphe : {0}")]
    Mismatch(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

fn corrupt(s: impl Into<String>) -> IndexError {
    IndexError::Corrupt(s.into())
}

#[derive(Clone, Copy, Debug, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C)]
struct RawView {
    kind: u8,
    pad: [u8; 3],
    cells: U32,
    nodes: U64,
    edges: U64,
    build_epoch: I64,
    lmax_dm: U32,
    n_foreign: U32,
    n_short: U32,
    reserved: [u8; 4],
}

#[derive(Clone, Copy, Debug, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C)]
struct RawHeader {
    magic: [u8; 8],
    major: U16,
    minor: U16,
    zoom: u8,
    pad: [u8; 3],
    base_nodes: U64,
    base_edges: U64,
    base_pbf_size: U64,
    base_pbf_xxh3: U64,
    base_build_epoch: I64,
    cap_dm: U32,
    node_sample_every: U32,
    /// `(lat_min, lat_max, lon_min, lon_max)` E7 de tous les nœuds de la base.
    node_bbox: [I32; 4],
    n_wide_seg: U32,
    n_wide_end: U32,
    n_sample: U32,
    meta_len: U32,
    built_epoch: I64,
    views: [RawView; 2],
    /// Empreinte de la table des sections.
    table_xxh3: U64,
    reserved: [u8; 48],
}

const _: () = assert!(std::mem::size_of::<RawHeader>() == HEADER_LEN);

#[derive(Clone, Copy, Debug, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C)]
struct RawEntry {
    offset: U64,
    len: U64,
    xxh3: U64,
}

const _: () = assert!(std::mem::size_of::<RawEntry>() == ENTRY_LEN);

/// Ce que le fichier dit d'une vue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexView {
    pub kind: ViewKind,
    pub cells: u32,
    pub nodes: u64,
    pub edges: u64,
    pub build_epoch: i64,
    pub lmax_dm: u32,
}

/// En-tête décodé.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexHeader {
    pub major: u16,
    pub minor: u16,
    pub zoom: u8,
    pub base_nodes: u64,
    pub base_edges: u64,
    pub base_pbf_size: u64,
    pub base_pbf_xxh3: u64,
    pub base_build_epoch: i64,
    pub cap_dm: u32,
    pub node_sample_every: u32,
    pub node_bbox: [i32; 4],
    pub built_epoch: i64,
    pub views: [IndexView; 2],
}

/// Les tables d'une vue, telles que le moteur les calcule.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ViewTables {
    /// `(clé de tuile, id de chaîne)`, strictement croissants.
    pub foreign: Vec<(u32, u32)>,
    /// `(id de chaîne, longueur dm de sa composante)`, ids strictement croissants, longueurs `< cap_dm`.
    pub short: Vec<(u32, u32)>,
    pub lmax_dm: u32,
}

/// Tout ce que le fichier porte.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexData {
    /// `[road, gravel]`.
    pub views: [ViewTables; 2],
    /// Seuil des composantes courtes (dm) : au-delà, une longueur vaut « au moins `cap_dm` ».
    pub cap_dm: u32,
    /// Arêtes de base dont un segment touche une tuile hors des voisines de leur cellule, croissantes.
    pub wide_seg: Vec<u32>,
    /// Arêtes de base dont un bout n'est ni dans leur cellule ni dans une voisine, croissantes.
    pub wide_end: Vec<u32>,
    /// `(lat_min, lat_max, lon_min, lon_max)` E7 de tous les nœuds ; `None` sans nœud.
    pub node_bbox: Option<[i32; 4]>,
    /// Un nœud sur [`NODE_SAMPLE_EVERY`], dans l'ordre des ids, `(lat, lon)` E7.
    pub node_sample: Vec<(i32, i32)>,
}

fn view_code(k: ViewKind) -> Option<u8> {
    match k {
        ViewKind::Road => Some(0),
        ViewKind::Gravel => Some(1),
        ViewKind::All => None,
    }
}

const VIEW_KINDS: [ViewKind; 2] = [ViewKind::Road, ViewKind::Gravel];

fn align8(n: usize) -> usize {
    n.div_ceil(8) * 8
}

/// Écrit des octets en tenant l'empreinte de la section en cours et la position.
struct SectionWriter<W: Write> {
    out: W,
    at: u64,
    hasher: Xxh3,
    start: u64,
}

impl<W: Write> SectionWriter<W> {
    fn put(&mut self, b: &[u8]) -> std::io::Result<()> {
        self.out.write_all(b)?;
        self.hasher.update(b);
        self.at += b.len() as u64;
        Ok(())
    }

    fn begin(&mut self) {
        self.hasher = Xxh3::new();
        self.start = self.at;
    }

    /// Ferme la section : bourre à 8 octets (hors empreinte), rend son entrée.
    fn end(&mut self) -> std::io::Result<RawEntry> {
        let e = RawEntry {
            offset: U64::new(self.start),
            len: U64::new(self.at - self.start),
            xxh3: U64::new(self.hasher.digest()),
        };
        let pad = align8(self.at as usize) - self.at as usize;
        self.out.write_all(&[0u8; 8][..pad])?;
        self.at += pad as u64;
        Ok(e)
    }
}

fn check_increasing<T: PartialOrd + Copy + std::fmt::Debug>(name: &str, xs: &[T]) -> Result<(), IndexError> {
    if let Some(w) = xs.windows(2).find(|w| w[0] >= w[1]) {
        return Err(corrupt(format!("{name} : pas strictement croissant ({:?} puis {:?})", w[0], w[1])));
    }
    Ok(())
}

/// Écrit le fichier à côté de sa destination (`<path>.tmp`, `fsync`, 0444), puis le renomme ; rend sa taille.
/// `views` : `[road, gravel]`, des vues non élaguées de `base`.
pub fn write_index<B: AsRef<[u8]>>(
    path: &Path,
    base: &GraphFile<B>,
    views: [&GraphFile<B>; 2],
    data: &IndexData,
    meta: &[u8],
    built_epoch: i64,
) -> Result<u64, IndexError> {
    let bh = base.header();
    let mut raw_views = [RawView::new_zeroed(), RawView::new_zeroed()];
    for (vi, v) in views.iter().enumerate() {
        let m =
            v.view_meta().ok_or_else(|| IndexError::Mismatch(format!("vue {vi} : ce n'est pas une vue")))?;
        if m.kind != VIEW_KINDS[vi] || m.prune_dead_ends {
            return Err(IndexError::Mismatch(format!(
                "vue {vi} : attendu `{}` non élaguée, reçu `{}`",
                VIEW_KINDS[vi].name(),
                m.name
            )));
        }
        let t = &data.views[vi];
        check_increasing("FOREIGN", &t.foreign)?;
        check_increasing("SHORT", &t.short.iter().map(|s| s.0).collect::<Vec<_>>())?;
        if t.short.iter().any(|s| s.1 >= data.cap_dm) {
            return Err(corrupt("SHORT : une longueur atteint cap_dm"));
        }
        let vh = v.header();
        raw_views[vi] = RawView {
            kind: view_code(m.kind).expect("road ou gravel"),
            pad: [0; 3],
            cells: U32::new(v.n_cells() as u32),
            nodes: U64::new(vh.nodes),
            edges: U64::new(vh.edges),
            build_epoch: I64::new(vh.build_epoch),
            lmax_dm: U32::new(t.lmax_dm),
            n_foreign: U32::new(t.foreign.len() as u32),
            n_short: U32::new(t.short.len() as u32),
            reserved: [0; 4],
        };
    }
    check_increasing("WIDE_SEG", &data.wide_seg)?;
    check_increasing("WIDE_END", &data.wide_end)?;
    let bbox = data.node_bbox.unwrap_or([0; 4]);

    let tmp = path.with_extension("ordi.tmp");
    match std::fs::remove_file(&tmp) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    let file = std::fs::File::create(&tmp)?;
    let mut w = SectionWriter {
        out: std::io::BufWriter::with_capacity(1 << 20, file),
        at: 0,
        hasher: Xxh3::new(),
        start: 0,
    };
    // En-tête et table, réécrits à la fin.
    w.out.write_all(&[0u8; HEADER_LEN + N_SECTIONS * ENTRY_LEN])?;
    w.at = (HEADER_LEN + N_SECTIONS * ENTRY_LEN) as u64;
    let mut table: Vec<RawEntry> = Vec::with_capacity(N_SECTIONS);
    let pairs = |w: &mut SectionWriter<_>, xs: &[(u32, u32)]| -> std::io::Result<RawEntry> {
        w.begin();
        for &(a, b) in xs {
            let mut buf = [0u8; 8];
            buf[..4].copy_from_slice(&a.to_le_bytes());
            buf[4..].copy_from_slice(&b.to_le_bytes());
            w.put(&buf)?;
        }
        w.end()
    };
    for t in &data.views {
        table.push(pairs(&mut w, &t.foreign)?);
        table.push(pairs(&mut w, &t.short)?);
    }
    for list in [&data.wide_seg, &data.wide_end] {
        w.begin();
        for &e in list {
            w.put(&e.to_le_bytes())?;
        }
        table.push(w.end()?);
    }
    w.begin();
    for &(lat, lon) in &data.node_sample {
        w.put(&lat.to_le_bytes())?;
        w.put(&lon.to_le_bytes())?;
    }
    table.push(w.end()?);
    w.begin();
    w.put(meta)?;
    table.push(w.end()?);
    debug_assert_eq!(table.len(), N_SECTIONS);

    let table_bytes: Vec<u8> = table.iter().flat_map(|e| e.as_bytes().to_vec()).collect();
    let header = RawHeader {
        magic: INDEX_MAGIC,
        major: U16::new(INDEX_MAJOR),
        minor: U16::new(INDEX_MINOR),
        zoom: bh.zoom,
        pad: [0; 3],
        base_nodes: U64::new(bh.nodes),
        base_edges: U64::new(bh.edges),
        base_pbf_size: U64::new(bh.pbf_size),
        base_pbf_xxh3: U64::new(bh.pbf_xxh3),
        base_build_epoch: I64::new(bh.build_epoch),
        cap_dm: U32::new(data.cap_dm),
        node_sample_every: U32::new(NODE_SAMPLE_EVERY),
        node_bbox: bbox.map(I32::new),
        n_wide_seg: U32::new(data.wide_seg.len() as u32),
        n_wide_end: U32::new(data.wide_end.len() as u32),
        n_sample: U32::new(data.node_sample.len() as u32),
        meta_len: U32::new(meta.len() as u32),
        built_epoch: I64::new(built_epoch),
        views: raw_views,
        table_xxh3: U64::new(xxh3_64(&table_bytes)),
        reserved: [0; 48],
    };
    let mut file = w.out.into_inner().map_err(|e| e.into_error())?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(header.as_bytes())?;
    file.write_all(&table_bytes)?;
    file.sync_all()?;
    let len = file.metadata()?.len();
    drop(file);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o444))?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(len)
}

/// Un fichier d'index de service lu : les tables sont des tranches de l'octet-tableau `B`.
#[derive(Debug)]
pub struct IndexFile<B> {
    data: B,
    header: IndexHeader,
    sections: [(usize, usize); N_SECTIONS],
}

#[cfg(feature = "mmap")]
impl IndexFile<memmap2::Mmap> {
    /// Ouvre un fichier d'index projeté en mémoire, et le valide en entier.
    pub fn open_mmap(path: &Path) -> Result<Self, IndexError> {
        let file = std::fs::File::open(path)?;
        let map = crate::mmap::map_readonly(&file)?;
        IndexFile::open(map)
    }
}

impl<B: AsRef<[u8]>> IndexFile<B> {
    /// Ouvre et valide tout le fichier : signature, version, tailles, empreintes de chaque section, tri des tables,
    /// ids sous les comptes de la base et des vues.
    pub fn open(data: B) -> Result<Self, IndexError> {
        let bytes = data.as_ref();
        let table_end = HEADER_LEN + N_SECTIONS * ENTRY_LEN;
        if bytes.len() < table_end {
            return Err(IndexError::Truncated(format!("{} octets", bytes.len())));
        }
        let raw = RawHeader::ref_from_bytes(&bytes[..HEADER_LEN]).expect("256 octets");
        if raw.magic != INDEX_MAGIC {
            return Err(IndexError::BadMagic);
        }
        if raw.major.get() != INDEX_MAJOR {
            return Err(IndexError::UnsupportedMajor(raw.major.get()));
        }
        if raw.reserved.iter().any(|&b| b != 0) {
            return Err(corrupt("octets réservés non nuls"));
        }
        let table_bytes = &bytes[HEADER_LEN..table_end];
        if xxh3_64(table_bytes) != raw.table_xxh3.get() {
            return Err(corrupt("empreinte de la table des sections fausse"));
        }
        let mut views = [None, None];
        for (vi, rv) in raw.views.iter().enumerate() {
            if rv.kind != vi as u8 {
                return Err(corrupt(format!("vue {vi} : type {}", rv.kind)));
            }
            views[vi] = Some(IndexView {
                kind: VIEW_KINDS[vi],
                cells: rv.cells.get(),
                nodes: rv.nodes.get(),
                edges: rv.edges.get(),
                build_epoch: rv.build_epoch.get(),
                lmax_dm: rv.lmax_dm.get(),
            });
        }
        let header = IndexHeader {
            major: raw.major.get(),
            minor: raw.minor.get(),
            zoom: raw.zoom,
            base_nodes: raw.base_nodes.get(),
            base_edges: raw.base_edges.get(),
            base_pbf_size: raw.base_pbf_size.get(),
            base_pbf_xxh3: raw.base_pbf_xxh3.get(),
            base_build_epoch: raw.base_build_epoch.get(),
            cap_dm: raw.cap_dm.get(),
            node_sample_every: raw.node_sample_every.get(),
            node_bbox: raw.node_bbox.map(|v| v.get()),
            built_epoch: raw.built_epoch.get(),
            views: views.map(|v| v.expect("deux vues")),
        };
        let entries = <[RawEntry]>::ref_from_bytes(table_bytes).expect("table de 8 entrées");
        let want_len = |s: usize| -> u64 {
            let rv = |vi: usize| &raw.views[vi];
            match s {
                0 | 2 => 8 * u64::from(rv(s / 2).n_foreign.get()),
                1 | 3 => 8 * u64::from(rv(s / 2).n_short.get()),
                S_WIDE_SEG => 4 * u64::from(raw.n_wide_seg.get()),
                S_WIDE_END => 4 * u64::from(raw.n_wide_end.get()),
                S_NODE_SAMPLE => 8 * u64::from(raw.n_sample.get()),
                _ => u64::from(raw.meta_len.get()),
            }
        };
        let mut sections = [(0usize, 0usize); N_SECTIONS];
        let mut at = table_end as u64;
        for (s, e) in entries.iter().enumerate() {
            let (off, len) = (e.offset.get(), e.len.get());
            if off != at || len != want_len(s) {
                return Err(corrupt(format!("{} : place ou longueur", SECTION_NAMES[s])));
            }
            let end = off + len;
            if end > bytes.len() as u64 {
                return Err(IndexError::Truncated(format!("{} hors du fichier", SECTION_NAMES[s])));
            }
            let body = &bytes[off as usize..end as usize];
            if xxh3_64(body) != e.xxh3.get() {
                return Err(corrupt(format!("{} : empreinte fausse", SECTION_NAMES[s])));
            }
            sections[s] = (off as usize, len as usize);
            at = align8(end as usize) as u64;
        }
        if at != bytes.len() as u64 {
            return Err(IndexError::Truncated(format!("{} octets, {at} attendus", bytes.len())));
        }
        let f = Self { data, header, sections };
        f.validate()?;
        Ok(f)
    }

    fn validate(&self) -> Result<(), IndexError> {
        let h = &self.header;
        let max_tile = 1u32 << h.zoom;
        for vi in 0..2 {
            let edges = h.views[vi].edges;
            let fo = self.foreign(vi);
            let key = |p: &Pair| (p[0].get(), p[1].get());
            if fo.windows(2).any(|w| key(&w[0]) >= key(&w[1])) {
                return Err(corrupt(format!("{} : pas strictement croissant", SECTION_NAMES[S_FOREIGN[vi]])));
            }
            if fo.iter().any(|p| {
                let k = p[0].get();
                (k & 0xffff) >= max_tile || (k >> 16) >= max_tile || u64::from(p[1].get()) >= edges
            }) {
                return Err(corrupt(format!(
                    "{} : tuile ou chaîne hors bornes",
                    SECTION_NAMES[S_FOREIGN[vi]]
                )));
            }
            let sh = self.short(vi);
            if sh.windows(2).any(|w| w[0][0].get() >= w[1][0].get()) {
                return Err(corrupt(format!("{} : pas strictement croissant", SECTION_NAMES[S_SHORT[vi]])));
            }
            if sh.iter().any(|p| u64::from(p[0].get()) >= edges || p[1].get() >= h.cap_dm) {
                return Err(corrupt(format!(
                    "{} : chaîne ou longueur hors bornes",
                    SECTION_NAMES[S_SHORT[vi]]
                )));
            }
        }
        for (s, list) in [(S_WIDE_SEG, self.wide_seg()), (S_WIDE_END, self.wide_end())] {
            if list.windows(2).any(|w| w[0].get() >= w[1].get())
                || list.last().is_some_and(|e| u64::from(e.get()) >= h.base_edges)
            {
                return Err(corrupt(format!("{} : pas croissant ou hors bornes", SECTION_NAMES[s])));
            }
        }
        if h.node_sample_every != NODE_SAMPLE_EVERY {
            return Err(corrupt(format!("échantillon d'un nœud sur {}", h.node_sample_every)));
        }
        let want = h.base_nodes.div_ceil(u64::from(NODE_SAMPLE_EVERY));
        if self.node_sample().len() as u64 != want {
            return Err(corrupt(format!(
                "{} nœuds échantillonnés, {want} attendus",
                self.node_sample().len()
            )));
        }
        if serde_json::from_slice::<serde::de::IgnoredAny>(self.meta()).is_err() {
            return Err(corrupt("META n'est pas du JSON"));
        }
        Ok(())
    }

    fn section(&self, s: usize) -> &[u8] {
        let (off, len) = self.sections[s];
        &self.data.as_ref()[off..off + len]
    }

    fn pairs(&self, s: usize) -> &[Pair] {
        <[Pair]>::ref_from_bytes(self.section(s)).expect("longueur validée à l'ouverture")
    }

    fn u32s(&self, s: usize) -> &[U32] {
        <[U32]>::ref_from_bytes(self.section(s)).expect("longueur validée à l'ouverture")
    }

    pub fn header(&self) -> &IndexHeader {
        &self.header
    }

    /// Chaînes étrangères de la vue `vi` (0 `road`, 1 `gravel`) : `(clé de tuile, id de chaîne)`, croissantes.
    pub fn foreign(&self, vi: usize) -> &[Pair] {
        self.pairs(S_FOREIGN[vi])
    }

    /// Composantes courtes de la vue `vi` : `(id de chaîne, longueur dm)`, ids croissants.
    pub fn short(&self, vi: usize) -> &[Pair] {
        self.pairs(S_SHORT[vi])
    }

    pub fn lmax_dm(&self, vi: usize) -> u32 {
        self.header.views[vi].lmax_dm
    }

    pub fn cap_dm(&self) -> u32 {
        self.header.cap_dm
    }

    pub fn wide_seg(&self) -> &[U32] {
        self.u32s(S_WIDE_SEG)
    }

    pub fn wide_end(&self) -> &[U32] {
        self.u32s(S_WIDE_END)
    }

    /// Un nœud de base sur [`NODE_SAMPLE_EVERY`], `[lat, lon]` E7 (en `U32` : relire par `as i32`).
    pub fn node_sample(&self) -> &[Pair] {
        self.pairs(S_NODE_SAMPLE)
    }

    /// Boîte `(lat_min, lat_max, lon_min, lon_max)` E7 de tous les nœuds ; `None` pour un graphe sans nœud.
    pub fn node_bbox(&self) -> Option<[i32; 4]> {
        (self.header.base_nodes > 0).then_some(self.header.node_bbox)
    }

    /// JSON de provenance (outil, réglages, durées).
    pub fn meta(&self) -> &[u8] {
        self.section(S_META)
    }

    /// Octets du fichier.
    pub fn len_bytes(&self) -> usize {
        self.data.as_ref().len()
    }

    /// Le fichier est celui de cette base et de ces deux vues `[road, gravel]`.
    pub fn check_for<B1: AsRef<[u8]>, B2: AsRef<[u8]>>(
        &self,
        base: &GraphFile<B1>,
        views: [&GraphFile<B2>; 2],
    ) -> Result<(), IndexError> {
        let h = &self.header;
        let bh = base.header();
        if (h.base_nodes, h.base_edges, h.base_pbf_size, h.base_pbf_xxh3, h.base_build_epoch, h.zoom)
            != (bh.nodes, bh.edges, bh.pbf_size, bh.pbf_xxh3, bh.build_epoch, bh.zoom)
        {
            return Err(IndexError::Mismatch(
                "graphe de base (comptes, extrait, date de construction ou zoom)".into(),
            ));
        }
        for (vi, v) in views.iter().enumerate() {
            let iv = &h.views[vi];
            let Some(m) = v.view_meta() else {
                return Err(IndexError::Mismatch(format!("vue {vi} : ce n'est pas une vue")));
            };
            let vh = v.header();
            if m.kind != iv.kind
                || m.prune_dead_ends
                || (iv.nodes, iv.edges, iv.build_epoch, u64::from(iv.cells))
                    != (vh.nodes, vh.edges, vh.build_epoch, v.n_cells() as u64)
            {
                return Err(IndexError::Mismatch(format!(
                    "vue `{}` (type, comptes, date de construction ou cellules)",
                    iv.kind.name()
                )));
            }
        }
        Ok(())
    }
}
