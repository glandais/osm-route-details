//! Vues **hors mémoire** (`docs/design/technique/ord-graph-views-hors-memoire.md`) : mêmes octets que
//! [`crate::view::build_view`] suivi de [`crate::format::write_graph`], sans tenir le graphe en mémoire.
//!
//! Le graphe de base est un fichier projeté : attributs, longueurs, extrémités et géométrie s'y relisent
//! ([`EdgeSrc`] sur [`GraphFile`]). L'état par élément tient en bits ([`Bits`]) ; l'incidence, les chaînes et les
//! arêtes de vue passent par des fichiers temporaires et le tri externe ([`crate::extsort`]).
//!
//! Étapes (note §4) :
//! - **V1** filtre (bit `alive`) et incidence `(nœud, arête)` triée ;
//! - **V2** nœuds fondus (bit, et leurs deux arêtes dans la table `pairs`, au rang du nœud), ancres et leur incidence ;
//! - **V3** marche des chaînes dans l'ordre de `contract`, puis les anneaux ; maillons et chaînes en fichiers ;
//! - **V4** arêtes de vue orientées `a < b`, triées `(a, b, premier maillon)` ;
//! - **V5** nœuds et cellules, arêtes orientées triées, géométrie et sections de vue en flux, écriture.
//!
//! Les vues élaguées (`-prune`) ne sont pas prises en charge : l'élagage itératif lit l'incidence au hasard.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use zerocopy::little_endian::{I32, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

use crate::attrs::Attrs;
use crate::bits::Bits;
use crate::build::{CellSpec, SourceMeta};
use crate::external::ExternalError;
use crate::extsort::{ExtSort, Peekable, RecReader, RecWriter, TmpDir, IO_BUF};
use crate::format::{
    encode_geometry, EdgeIdRec, FormatError, GraphFile, NodeRec, StreamHeader, StreamWriter, DIR_BIT,
    KIND_VIEW_BASE_NODE, KIND_VIEW_CHAIN, KIND_VIEW_CHAIN_OFF, KIND_VIEW_HEADING, KIND_VIEW_META,
    KIND_VIEW_WEIGHT,
};
use crate::stats::{proc_status, BuildStats, Phase, PhaseClock};
use crate::view::{chain_summary, link_from, link_to, mergeable_at, polyline_heading_pair, EdgeSrc, Link};
use crate::view::{ViewMeta, ViewSpec};

/// Réglages des vues hors mémoire.
#[derive(Clone, Debug)]
pub struct ViewExternalOptions {
    /// Budget des tampons de tri, en octets.
    pub mem_budget: usize,
    /// Dossier où créer le dossier temporaire (verrouillé, nettoyé à la fin).
    pub tmp_parent: PathBuf,
}

/// Ce que rend une vue : sa méta, le pic du disque temporaire, les séries par tri, le pic de mémoire avant la
/// relecture finale (qui projette le fichier écrit).
#[derive(Debug)]
pub struct ViewExternalReport {
    pub meta: ViewMeta,
    pub tmp_peak_bytes: u64,
    pub runs: Vec<(String, u64)>,
    pub peak_rss_before_verify_kb: u64,
    /// Pic de `RssAnon` relevé pendant la construction (toutes les 2^20 itérations et à chaque étape) : la
    /// mémoire propre du processus, sans les pages du graphe de base projeté, que `VmHWM` compte.
    pub peak_rss_anon_kb: u64,
    pub phases: Vec<Phase>,
    pub stale_tmp_removed: Vec<PathBuf>,
}

/// Relevé du pic de mémoire anonyme, à intervalles réguliers des boucles.
#[derive(Default)]
struct AnonPeak {
    peak: std::cell::Cell<u64>,
    tick: std::cell::Cell<u64>,
}

impl AnonPeak {
    fn tick(&self) {
        let t = self.tick.get() + 1;
        self.tick.set(t);
        if t & ((1 << 20) - 1) == 0 {
            self.sample();
        }
    }
    fn sample(&self) {
        self.peak.set(self.peak.get().max(proc_status().rss_anon_kb));
    }
}

// --- Enregistrements -----------------------------------------------------------------------------------

/// Une arête retenue vue d'une de ses extrémités ; clé totale `(nœud, arête, côté)`.
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy, Debug)]
#[repr(C)]
struct Inc {
    node: U32,
    edge: U32,
    side: u8,
}

/// Les deux arêtes d'un nœud fondu, au rang du nœud parmi les nœuds fondus.
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy, Debug)]
#[repr(C)]
struct Pair {
    e1: U32,
    e2: U32,
}

/// Arête incidente à une ancre, dans l'ordre `(ancre, arête)`.
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy, Debug)]
#[repr(C)]
struct AnchorInc {
    node: U32,
    edge: U32,
}

/// Une chaîne, dans l'ordre de la marche : extrémités (ids de base) et plage de ses maillons.
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy, Debug)]
#[repr(C)]
struct ChainRec {
    from: U32,
    to: U32,
    off: U64,
    n: U32,
}

/// Arête de vue orientée `a < b` ; clé totale `(a, b, arête du premier maillon, sens)`. `flip` : ses maillons sont
/// ceux du fichier, pris à rebours.
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy, Debug)]
#[repr(C)]
struct VEdge {
    a: U32,
    b: U32,
    e0: U32,
    rev0: u8,
    base_a: U32,
    off: U64,
    n: U32,
    flip: u8,
    len_dm: U32,
    weight: U32,
    attrs: U64,
    way: U64,
}

/// Arête orientée de la vue ; clé totale `(source, cible, arête | sens)`, l'ordre de `write_graph`.
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy, Debug)]
#[repr(C)]
struct Dir {
    src: U32,
    dst: U32,
    edge: U32,
}

/// Arête de vue finale, dans l'ordre des ids : ce que le bloc de sa cellule porte.
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy, Debug)]
#[repr(C)]
struct EdgeOut {
    a: U32,
    attrs: U64,
    len_dm: U32,
    geom_len: U32,
}

// --- Lecture au hasard d'un fichier temporaire -------------------------------------------------------

/// Octets d'un bloc du cache.
const BLOCK: usize = 1 << 16;
/// Blocs du cache (correspondance directe) : 16 Mio.
const SLOTS: usize = 256;

/// Lecture au hasard d'un fichier temporaire par blocs, avec un petit cache : les accès de la marche et de
/// l'écriture suivent l'espace, donc des blocs voisins.
struct BlockCache {
    file: File,
    len: u64,
    slots: Vec<Option<(u64, Box<[u8]>)>>,
}

impl BlockCache {
    fn open(path: &Path) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        Ok(Self { file, len, slots: (0..SLOTS).map(|_| None).collect() })
    }

    fn read(&mut self, mut off: u64, out: &mut [u8]) -> std::io::Result<()> {
        let mut done = 0;
        while done < out.len() {
            let bi = off / BLOCK as u64;
            let slot = (bi % SLOTS as u64) as usize;
            if self.slots[slot].as_ref().is_none_or(|(b, _)| *b != bi) {
                let start = bi * BLOCK as u64;
                let n = (self.len.saturating_sub(start)).min(BLOCK as u64) as usize;
                let mut buf = vec![0u8; n].into_boxed_slice();
                self.file.seek(SeekFrom::Start(start))?;
                self.file.read_exact(&mut buf)?;
                self.slots[slot] = Some((bi, buf));
            }
            let (_, buf) = self.slots[slot].as_ref().expect("bloc chargé");
            let at = (off % BLOCK as u64) as usize;
            if at >= buf.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "lecture hors du fichier",
                ));
            }
            let n = (buf.len() - at).min(out.len() - done);
            out[done..done + n].copy_from_slice(&buf[at..at + n]);
            done += n;
            off += n as u64;
        }
        Ok(())
    }

    fn u32_at(&mut self, index: u64) -> std::io::Result<u32> {
        let mut b = [0u8; 4];
        self.read(index * 4, &mut b)?;
        Ok(u32::from_le_bytes(b))
    }

    fn pair(&mut self, rank: u64) -> std::io::Result<(u32, u32)> {
        let mut b = [0u8; 8];
        self.read(rank * 8, &mut b)?;
        Ok((u32::from_le_bytes(b[..4].try_into().unwrap()), u32::from_le_bytes(b[4..].try_into().unwrap())))
    }
}

fn link_word(l: Link) -> u32 {
    l.0 | if l.1 { DIR_BIT } else { 0 }
}

fn word_link(w: u32) -> Link {
    (w & !DIR_BIT, w & DIR_BIT != 0)
}

fn bad(msg: impl Into<String>) -> ExternalError {
    ExternalError::Format(FormatError::Build(msg.into()))
}

/// Construit la vue `spec` de `g` hors mémoire et l'écrit en `path` (`.ordv`).
pub fn build_view_external<B: AsRef<[u8]>>(
    g: &GraphFile<B>,
    spec: &ViewSpec,
    opts: &ViewExternalOptions,
    path: &Path,
    tool_version: &str,
    build_epoch: i64,
) -> Result<ViewExternalReport, ExternalError> {
    if spec.prune_dead_ends {
        return Err(bad(format!("vue {} : l'élagage (-prune) n'existe pas hors mémoire", spec.name())));
    }
    if g.is_view() {
        return Err(bad("le graphe de base est déjà une vue"));
    }
    let kind = spec.kind;
    let budget = opts.mem_budget.max(1);
    let tmp = TmpDir::create(&opts.tmp_parent)?;
    let h = g.header().clone();
    let anon = AnonPeak::default();
    let mut clock = PhaseClock::new();
    let mut phases: Vec<Phase> = Vec::new();
    let mut lap = |name: &str, phases: &mut Vec<Phase>| {
        anon.sample();
        phases.push(clock.lap(name));
    };
    let (n_nodes, n_base) = (h.nodes as usize, h.edges as usize);

    // V1 — filtre et incidence.
    let mut alive = Bits::new(n_base);
    let mut incs = ExtSort::new(&tmp, "inc", budget, |r: &Inc| (r.node.get(), r.edge.get(), r.side));
    for e in 0..n_base as u32 {
        anon.tick();
        if !kind.keep(g.attrs(e)) {
            continue;
        }
        alive.set(e as usize);
        let (a, b) = g.ends(e);
        incs.push(Inc { node: a.into(), edge: e.into(), side: 0 })?;
        incs.push(Inc { node: b.into(), edge: e.into(), side: 1 })?;
    }
    let filtered = alive.count_ones();
    lap("V1 filtre et incidence", &mut phases);

    // V2 — nœuds fondus (et leurs deux arêtes), ancres (et leur incidence).
    let mut merged = Bits::new(n_nodes);
    let mut anchor = Bits::new(n_nodes);
    let (pairs_path, anchor_inc_path, n_anchor_inc) = {
        let mut sorted = Peekable::new(incs.finish()?);
        let mut pairs = RecWriter::<Pair>::create(&tmp, "pairs", IO_BUF)?;
        let mut ainc = RecWriter::<AnchorInc>::create(&tmp, "anchor-inc", IO_BUF)?;
        let mut group: Vec<u32> = Vec::new();
        while let Some(first) = sorted.pull()? {
            anon.tick();
            let v = first.node.get();
            group.clear();
            group.push(first.edge.get());
            while let Some(r) = sorted.peek()? {
                if r.node.get() != v {
                    break;
                }
                group.push(r.edge.get());
                sorted.pull()?;
            }
            if let [e1, e2] = group[..] {
                if e1 != e2 && mergeable_at(g, kind, v, e1, e2) {
                    merged.set(v as usize);
                    pairs.push(&Pair { e1: e1.into(), e2: e2.into() })?;
                    continue;
                }
            }
            anchor.set(v as usize);
            for &e in &group {
                ainc.push(&AnchorInc { node: v.into(), edge: e.into() })?;
            }
        }
        let n = ainc.len();
        (pairs.finish()?.0, ainc.finish()?.0, n)
    };
    merged.build_rank();
    let merged_nodes = merged.count_ones();
    lap("V2 nœuds fondus et ancres", &mut phases);

    // V3 — chaînes : depuis chaque ancre, chaque arête non visitée ; puis les anneaux.
    let mut visited = Bits::new(n_base);
    let mut pairs = BlockCache::open(&pairs_path)?;
    let mut links_w = RecWriter::<U32>::create(&tmp, "links", IO_BUF)?;
    let mut chains_w = RecWriter::<ChainRec>::create(&tmp, "chains", IO_BUF)?;
    let (mut loops_broken, mut rings) = (0u64, 0u64);
    // Marche depuis `start` par `first` jusqu'à une ancre ou le retour au départ (`contract::walk`).
    let mut walk =
        |start: u32, first: u32, visited: &mut Bits, links: &mut Vec<Link>| -> Result<u32, ExternalError> {
            links.clear();
            let (mut cur, mut e) = (start, first);
            loop {
                links.push((e, g.rev_from(e, cur)));
                visited.set(e as usize);
                let next = g.other(e, cur);
                if next == start || !merged.get(next as usize) {
                    return Ok(next);
                }
                let (x, y) = pairs.pair(merged.rank(next as usize))?;
                e = if x == e { y } else { x };
                cur = next;
            }
        };
    // Une chaîne, ou une boucle coupée en son milieu (le nœud du milieu devient une ancre).
    let mut push_chain = |links: &[Link],
                          from: u32,
                          to: u32,
                          anchor: &mut Bits,
                          loops_broken: &mut u64|
     -> std::io::Result<()> {
        let off = links_w.len();
        for &l in links {
            links_w.push(&U32::new(link_word(l)))?;
        }
        let rec = |from: u32, to: u32, off: u64, n: usize| ChainRec {
            from: from.into(),
            to: to.into(),
            off: off.into(),
            n: (n as u32).into(),
        };
        if from != to {
            return chains_w.push(&rec(from, to, off, links.len()));
        }
        *loops_broken += 1;
        let m = links.len() / 2;
        let mid = link_from(g, links[m]);
        anchor.set(mid as usize);
        chains_w.push(&rec(from, mid, off, m))?;
        chains_w.push(&rec(mid, to, off + m as u64, links.len() - m))
    };
    let mut links: Vec<Link> = Vec::new();
    {
        let mut ainc =
            Peekable::new(RecReader::<AnchorInc>::open(&tmp, &anchor_inc_path, n_anchor_inc, IO_BUF, true)?);
        while let Some(r) = ainc.pull()? {
            anon.tick();
            let (v, e) = (r.node.get(), r.edge.get());
            if visited.get(e as usize) {
                continue;
            }
            let to = walk(v, e, &mut visited, &mut links)?;
            push_chain(&links, v, to, &mut anchor, &mut loops_broken)?;
        }
    }
    // Anneaux : arêtes vivantes restantes, dont tous les nœuds sont fondus. Ancre = le plus petit nœud.
    for e in 0..n_base as u32 {
        if !alive.get(e as usize) || visited.get(e as usize) {
            continue;
        }
        rings += 1;
        let start = g.ends(e).0;
        let to = walk(start, e, &mut visited, &mut links)?;
        if to != start {
            return Err(bad(format!("anneau de l'arête {e} : la marche n'est pas revenue au départ")));
        }
        let k = (0..links.len()).min_by_key(|&i| link_from(g, links[i])).unwrap_or(0);
        links.rotate_left(k);
        let a = link_from(g, links[0]);
        anchor.set(a as usize);
        push_chain(&links, a, a, &mut anchor, &mut loops_broken)?;
    }
    drop(pairs);
    tmp.remove(&pairs_path);
    let n_chains = chains_w.len();
    let (chains_path, _) = chains_w.finish()?;
    let (links_path, _) = links_w.finish()?;
    anchor.build_rank();
    let view_id = |b: u32| anchor.rank(b as usize) as u32;
    lap("V3 chaînes", &mut phases);

    // V4 — arêtes de vue orientées et triées.
    let mut vedges =
        ExtSort::new(&tmp, "vedges", budget, |r: &VEdge| (r.a.get(), r.b.get(), r.e0.get(), r.rev0));
    {
        let mut chains = RecReader::<ChainRec>::open(&tmp, &chains_path, n_chains, IO_BUF, true)?;
        let mut words = RecReader::<U32>::open(&tmp, &links_path, u64::MAX, IO_BUF, false)?;
        while let Some(c) = chains.pull()? {
            anon.tick();
            links.clear();
            for _ in 0..c.n.get() {
                let w = words.pull()?.ok_or_else(|| bad("maillons : fin prématurée"))?;
                links.push(word_link(w.get()));
            }
            let (mut from, mut to) = (c.from.get(), c.to.get());
            let flip = view_id(from) > view_id(to);
            if flip {
                links.reverse();
                for l in &mut links {
                    l.1 = !l.1;
                }
                std::mem::swap(&mut from, &mut to);
            }
            let (len_dm, attrs, weight) = chain_summary(g, &links)?;
            vedges.push(VEdge {
                a: view_id(from).into(),
                b: view_id(to).into(),
                e0: links[0].0.into(),
                rev0: u8::from(links[0].1),
                base_a: from.into(),
                off: c.off,
                n: c.n,
                flip: u8::from(flip),
                len_dm: len_dm.into(),
                weight: weight.into(),
                attrs: attrs.0.into(),
                way: g.edge_way(links[0].0).into(),
            })?;
        }
    }

    lap("V4 arêtes de vue", &mut phases);

    // V5 — nœuds et cellules de la vue (ancres, dans l'ordre des ids de base).
    let mut cells: Vec<CellSpec> = Vec::new();
    let (nodes_path, nids_path, bnodes_path, n_view_nodes) = {
        let mut nodes_w = RecWriter::<NodeRec>::create(&tmp, "nodes", IO_BUF)?;
        let mut nids_w = RecWriter::<U64>::create(&tmp, "nids", IO_BUF)?;
        let mut bnodes_w = RecWriter::<U32>::create(&tmp, "base-nodes", IO_BUF)?;
        let mut last_cell = usize::MAX;
        for (i, b) in anchor.ones().enumerate() {
            let b = b as u32;
            let ci = g.node_cell(b);
            if ci != last_cell {
                let (x, y) = g.cell_xy(ci);
                cells.push(CellSpec { x, y, first_node: i as u32, n_nodes: 0 });
                last_cell = ci;
            }
            cells.last_mut().expect("cellule ouverte").n_nodes += 1;
            let (lat, lon) = g.node_pos(b);
            nodes_w.push(&NodeRec { lat: I32::new(lat), lon: I32::new(lon) })?;
            nids_w.push(&U64::new(g.osm_node_id(b)))?;
            bnodes_w.push(&U32::new(b))?;
        }
        let n = nodes_w.len();
        (nodes_w.finish()?.0, nids_w.finish()?.0, bnodes_w.finish()?.0, n)
    };

    // Arêtes dans l'ordre final : arêtes orientées, géométrie, caps et sections de vue, en flux.
    let mut dirs =
        ExtSort::new(&tmp, "dir", (budget / 2).max(1), |d: &Dir| (d.src.get(), d.dst.get(), d.edge.get()));
    let mut links_r = BlockCache::open(&links_path)?;
    let mut edges_w = RecWriter::<EdgeOut>::create(&tmp, "edges", IO_BUF)?;
    let mut eids_w = RecWriter::<EdgeIdRec>::create(&tmp, "eids", IO_BUF)?;
    let mut geom_w = RecWriter::<u8>::create(&tmp, "geometry", IO_BUF)?;
    let mut weights_w = RecWriter::<U32>::create(&tmp, "weights", IO_BUF)?;
    let mut coff_w = RecWriter::<U32>::create(&tmp, "chain-off", IO_BUF)?;
    let mut chain_w = RecWriter::<U32>::create(&tmp, "chain", IO_BUF)?;
    let mut head_w = RecWriter::<u8>::create(&tmp, "headings", IO_BUF)?;
    coff_w.push(&U32::new(0))?;
    let (mut n_edges, mut n_dir, mut n_links) = (0u64, 0u64, 0u64);
    {
        let mut sorted = vedges.finish()?;
        let (mut poly, mut geom): (Vec<(i32, i32)>, Vec<u8>) = (Vec::new(), Vec::new());
        while let Some(ve) = sorted.pull()? {
            anon.tick();
            // Même limite que `write_graph` : moins de 2^31 arêtes (le bit 31 porte le sens).
            if n_edges + 1 >= 1 << 31 {
                return Err(bad("plus de 2^31 arêtes"));
            }
            let id = n_edges as u32;
            let (a, b) = (ve.a.get(), ve.b.get());
            links.clear();
            for k in 0..u64::from(ve.n.get()) {
                links.push(word_link(links_r.u32_at(ve.off.get() + k)?));
            }
            if ve.flip != 0 {
                links.reverse();
                for l in &mut links {
                    l.1 = !l.1;
                }
            }
            let attrs = Attrs(ve.attrs.get());
            if attrs.fwd_ok() {
                dirs.push(Dir { src: a.into(), dst: b.into(), edge: id.into() })?;
                n_dir += 1;
            }
            if attrs.bwd_ok() {
                dirs.push(Dir { src: b.into(), dst: a.into(), edge: (id | DIR_BIT).into() })?;
                n_dir += 1;
            }
            // Polyligne complète (nœuds fondus compris) : le point de `a`, puis pour chaque maillon sa géométrie
            // et le nœud où il arrive.
            poly.clear();
            poly.push(g.node_pos(ve.base_a.get()));
            for &(e, rev) in &links {
                let mut inner = g.geometry(e);
                if rev {
                    inner.reverse();
                }
                poly.extend_from_slice(&inner);
                poly.push(g.node_pos(link_to(g, (e, rev))));
                chain_w.push(&U32::new(link_word((e, rev))))?;
            }
            n_links += links.len() as u64;
            geom.clear();
            encode_geometry(&mut geom, poly[0], &poly[1..poly.len() - 1]);
            geom_w.push_all(&geom)?;
            for hd in polyline_heading_pair(&poly) {
                head_w.push_all(&hd.to_le_bytes())?;
            }
            let n_links32 = u32::try_from(n_links).map_err(|_| bad("plus de 2^32 maillons de chaîne"))?;
            coff_w.push(&U32::new(n_links32))?;
            weights_w.push(&ve.weight)?;
            eids_w.push(&EdgeIdRec { way: ve.way, a: ve.a, b: ve.b })?;
            let geom_len = u32::try_from(geom.len()).map_err(|_| bad("géométrie d'une arête > 4 Go"))?;
            edges_w.push(&EdgeOut {
                a: ve.a,
                attrs: ve.attrs,
                len_dm: ve.len_dm,
                geom_len: geom_len.into(),
            })?;
            n_edges += 1;
        }
    }
    drop(links_r);
    tmp.remove(&links_path);
    let (edges_path, _) = edges_w.finish()?;
    let (eids_path, _) = eids_w.finish()?;
    let (geom_path, _) = geom_w.finish()?;
    let (weights_path, _) = weights_w.finish()?;
    let (coff_path, _) = coff_w.finish()?;
    let (chain_path, _) = chain_w.finish()?;
    let (head_path, _) = head_w.finish()?;
    lap("V5 arêtes, géométrie et sections", &mut phases);

    // Écriture : blocs de cellule, géométrie, ids, sections de vue, STATS, en-tête.
    let mut writer = StreamWriter::create(path, cells.clone())?;
    {
        let mut nodes = RecReader::<NodeRec>::open(&tmp, &nodes_path, n_view_nodes, IO_BUF, true)?;
        let mut dirs = Peekable::new(dirs.finish()?);
        let mut edges = Peekable::new(RecReader::<EdgeOut>::open(&tmp, &edges_path, n_edges, IO_BUF, true)?);
        let (mut cn, mut cd, mut co) = (Vec::new(), Vec::new(), Vec::new());
        for cs in &cells {
            let nhi = cs.first_node + cs.n_nodes;
            cn.clear();
            for _ in 0..cs.n_nodes {
                let n = nodes.pull()?.ok_or_else(|| bad("nœuds de vue : fin prématurée"))?;
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
                let off = u32::try_from(geom_len).map_err(|_| bad("géométrie d'une cellule > 4 Go"))?;
                co.push((ed.attrs.get(), ed.len_dm.get(), off));
                geom_len += u64::from(ed.geom_len.get());
                edges.pull()?;
            }
            writer.put_cell(&cn, &cd, &co, geom_len)?;
        }
        if nodes.pull()?.is_some() || dirs.peek()?.is_some() || edges.peek()?.is_some() {
            return Err(bad("écriture : nœuds ou arêtes hors des cellules"));
        }
    }
    let open = |p: &Path| -> std::io::Result<BufReader<File>> {
        Ok(BufReader::with_capacity(IO_BUF, File::open(p)?))
    };
    let size = |p: &Path| -> std::io::Result<u64> { Ok(std::fs::metadata(p)?.len()) };
    writer.put_geometry(&mut open(&geom_path)?)?;
    tmp.remove(&geom_path);
    writer.put_node_ids(&mut open(&nids_path)?, n_view_nodes)?;
    tmp.remove(&nids_path);
    writer.put_edge_ids(&mut open(&eids_path)?, n_edges)?;
    tmp.remove(&eids_path);

    let meta = ViewMeta {
        name: spec.name(),
        kind: spec.kind,
        prune_dead_ends: spec.prune_dead_ends,
        base_nodes: h.nodes,
        base_edges: h.edges,
        base_pbf_xxh3: h.pbf_xxh3,
        base_build_epoch: h.build_epoch,
        filtered_edges: filtered,
        pruned_edges: 0,
        view_nodes: n_view_nodes,
        view_edges: n_edges,
        merged_nodes,
        loops_broken,
        rings,
    };
    let meta_json = serde_json::to_vec(&meta).map_err(|e| FormatError::Build(e.to_string()))?;
    writer.put_extra(KIND_VIEW_META, &mut meta_json.as_slice(), meta_json.len() as u64)?;
    for (kind, p) in [
        (KIND_VIEW_WEIGHT, &weights_path),
        (KIND_VIEW_CHAIN_OFF, &coff_path),
        (KIND_VIEW_CHAIN, &chain_path),
        (KIND_VIEW_BASE_NODE, &bnodes_path),
        (KIND_VIEW_HEADING, &head_path),
    ] {
        writer.put_extra(kind, &mut open(p)?, size(p)?)?;
        tmp.remove(p);
    }

    let stats = BuildStats {
        region: format!("vue {}", meta.name),
        vertices: n_view_nodes,
        edges: n_edges,
        ..BuildStats::default()
    };
    let stats_json = serde_json::to_vec(&stats).map_err(|e| FormatError::Build(e.to_string()))?;
    let header = StreamHeader {
        zoom: h.zoom,
        nodes: n_view_nodes,
        edges: n_edges,
        directed: n_dir,
        meta: SourceMeta {
            replication_timestamp: h.replication_timestamp,
            replication_sequence: h.replication_sequence,
            size: h.pbf_size,
            xxh3: h.pbf_xxh3,
        },
        tool_version,
        build_epoch,
    };
    lap("V5 écriture", &mut phases);
    let peak_rss_before_verify_kb = proc_status().vm_hwm_kb;
    writer.finish(&stats_json, &header)?;
    lap("en-tête, relecture et verify()", &mut phases);
    Ok(ViewExternalReport {
        meta,
        tmp_peak_bytes: tmp.peak_bytes(),
        runs: tmp.runs(),
        peak_rss_before_verify_kb,
        peak_rss_anon_kb: anon.peak.get(),
        phases,
        stale_tmp_removed: tmp.stale_removed().to_vec(),
    })
}
