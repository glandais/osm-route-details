//! Vues par type de parcours (lot 0 bis, §11 de `ord-graph-lot0.md`) : un filtre d'arêtes du graphe de base,
//! puis la contraction des chaînes de nœuds de degré 2 que ce filtre crée.
//!
//! Une vue est un fichier `GraphFile` à part entière (mêmes cellules, mêmes sections, donc la même recherche) qui
//! porte en plus cinq sections optionnelles (format 1.1) : méta JSON, poids par arête, chaînes d'arêtes de base,
//! nœud de base de chaque nœud. Une arête de vue **est** une chaîne d'arêtes de base : elle garde sa longueur,
//! la liste ordonnée des arêtes de base (géométrie, attributs et surcouche par arête de base restent
//! accessibles), et ses deux sens d'ouverture.
//!
//! Règles de contraction (voir la note, §11.2) :
//! - un nœud n'est fondu que s'il a **exactement deux** arêtes retenues et que les deux ouvrent les **mêmes sens**
//!   le long de la chaîne (deux doubles sens, deux sens uniques de même orientation) : un double sens suivi d'un
//!   sens unique, ou deux sens uniques qui s'opposent (puits, source), gardent leur nœud, sans quoi un nœud
//!   atteignable cesserait de l'être ;
//! - vue `all` : en plus, les attributs doivent être **identiques** (relus dans le sens de la chaîne) ;
//! - une chaîne qui reboucle sur son point de départ est coupée en son milieu (jamais de boucle) ; deux chaînes
//!   parallèles entre les mêmes nœuds restent deux arêtes.

use std::collections::BinaryHeap;

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use zerocopy::little_endian::U32;
use zerocopy::FromBytes;

use crate::attrs::{Access, Attrs, HighwayClass};
use crate::build::{CellSpec, EdgeData, ExtraSection, GraphData, NodeData, SourceMeta};
use crate::format::{
    FormatError, GraphFile, DIR_BIT, KIND_VIEW_BASE_NODE, KIND_VIEW_CHAIN, KIND_VIEW_CHAIN_OFF,
    KIND_VIEW_HEADING, KIND_VIEW_META, KIND_VIEW_WEIGHT,
};
use crate::search::edge_cost;
use crate::stats::BuildStats;

/// Poids d'une arête de vue inutilisable (une arête de la chaîne n'a pas de coût : accès `No`).
pub const NO_WEIGHT: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ViewKind {
    /// Route revêtue : classes routières et pistes cyclables, voies vertes (`path`, `footway`, `pedestrian`,
    /// `bridleway` désignés aux vélos, sans surface connue non revêtue) et `track` revêtus (lot 2, §1.1).
    Road,
    /// Route + chemins, pistes et sentiers roulables.
    Gravel,
    /// Aucun filtre : témoin (la contraction ne fond que les coutures d'attributs identiques).
    All,
}

impl ViewKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Road => "road",
            Self::Gravel => "gravel",
            Self::All => "all",
        }
    }

    /// Vrai si l'arête de base est retenue par le filtre.
    ///
    /// Décisions (note §11.1) : `Destination` est **gardé** (légalement roulable, pénalisé ×1,25 par le coût
    /// synthétique) ; `No` et `Private` sont écartés ; un `surface` absent ou inconnu est **gardé** en `road` sur les
    /// classes routières (la classe laisse présumer un revêtement), un `surface` connu non revêtu l'écarte.
    pub fn keep(self, a: Attrs) -> bool {
        if self == Self::All {
            return true;
        }
        if matches!(a.access(), Access::No | Access::Private) {
            return false;
        }
        let Some(class) = a.class() else { return false };
        // `service=driveway|parking_aisle|drive-through|emergency_access` : jamais un itinéraire.
        if class == HighwayClass::Service && a.service() != 0 {
            return false;
        }
        let road_class = matches!(
            class,
            HighwayClass::Motorway
                | HighwayClass::Trunk
                | HighwayClass::Primary
                | HighwayClass::Secondary
                | HighwayClass::Tertiary
                | HighwayClass::Unclassified
                | HighwayClass::Residential
                | HighwayClass::LivingStreet
                | HighwayClass::Service
                | HighwayClass::Cycleway
                | HighwayClass::Road
                | HighwayClass::Busway
        );
        match self {
            Self::Road => {
                // Voies vertes : désignées aux vélos (`bicycle=designated`), jamais de non-revêtu connu.
                let greenway = matches!(
                    class,
                    HighwayClass::Path
                        | HighwayClass::Footway
                        | HighwayClass::Pedestrian
                        | HighwayClass::Bridleway
                ) && a.bicycle_tag() == 2;
                // `track` revêtu : `tracktype=grade1` ou surface connue revêtue (règle Java, plan §5.5).
                let paved_track = class == HighwayClass::Track
                    && (a.tracktype() == 1 || (a.surface() != 0 && !a.is_unpaved()));
                (road_class || greenway || paved_track) && !a.is_unpaved()
            }
            Self::Gravel => {
                road_class
                    || matches!(class, HighwayClass::Track | HighwayClass::Path | HighwayClass::Bridleway)
                    // `footway` / `pedestrian` seulement s'ils sont désignés aux vélos (`bicycle=designated`).
                    || (matches!(class, HighwayClass::Footway | HighwayClass::Pedestrian)
                        && a.bicycle_tag() == 2)
            }
            Self::All => true,
        }
    }
}

/// Une vue : un filtre, et l'élagage optionnel des culs-de-sac.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewSpec {
    pub kind: ViewKind,
    /// Retire, avant la contraction, les nœuds de degré 1 de façon itérative (impasses).
    pub prune_dead_ends: bool,
}

impl ViewSpec {
    /// `road`, `gravel`, `all`, avec le suffixe `-prune` pour l'élagage des impasses.
    pub fn parse(s: &str) -> Option<Self> {
        let (base, prune) = match s.strip_suffix("-prune") {
            Some(b) => (b, true),
            None => (s, false),
        };
        let kind = match base {
            "road" => ViewKind::Road,
            "gravel" => ViewKind::Gravel,
            "all" => ViewKind::All,
            _ => return None,
        };
        Some(Self { kind, prune_dead_ends: prune })
    }

    pub fn name(&self) -> String {
        format!("{}{}", self.kind.name(), if self.prune_dead_ends { "-prune" } else { "" })
    }
}

/// Méta d'une vue (section `VIEW_META`, JSON).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ViewMeta {
    pub name: String,
    pub kind: ViewKind,
    pub prune_dead_ends: bool,
    /// Empreinte du graphe de base : comptes, graine de l'extrait et date de construction.
    pub base_nodes: u64,
    pub base_edges: u64,
    pub base_pbf_xxh3: u64,
    pub base_build_epoch: i64,
    /// Arêtes de base retenues par le filtre, puis encore vivantes après l'élagage.
    pub filtered_edges: u64,
    pub pruned_edges: u64,
    pub view_nodes: u64,
    pub view_edges: u64,
    /// Nœuds de base fondus dans une chaîne.
    pub merged_nodes: u64,
    /// Chaînes qui rebouclaient (coupées en deux) et anneaux sans aucune jonction (coupés en deux).
    pub loops_broken: u64,
    pub rings: u64,
}

// --- Lecture des arêtes de base ------------------------------------------------------------------

/// Les arêtes du graphe de base à plat : extrémités, attributs, longueur.
#[derive(Debug)]
pub struct BaseEdges {
    pub n_nodes: usize,
    pub ends: Vec<(u32, u32)>,
    pub attrs: Vec<Attrs>,
    pub len_dm: Vec<u32>,
}

impl BaseEdges {
    pub fn load<B: AsRef<[u8]>>(g: &GraphFile<B>) -> Self {
        let h = g.header();
        let mut s = Self {
            n_nodes: h.nodes as usize,
            ends: Vec::with_capacity(h.edges as usize),
            attrs: Vec::with_capacity(h.edges as usize),
            len_dm: Vec::with_capacity(h.edges as usize),
        };
        for ci in 0..g.n_cells() {
            let cv = g.cell(ci);
            let info = cv.info();
            for i in 0..info.n_edges {
                let o = cv.owned(i as usize);
                s.ends.push(g.edge_ends(info.first_edge + i));
                s.attrs.push(o.attrs);
                s.len_dm.push(o.len_dm);
            }
        }
        s
    }

    fn other(&self, e: u32, v: u32) -> u32 {
        let (a, b) = self.ends[e as usize];
        if a == v {
            b
        } else {
            a
        }
    }

    /// Le sens `a → b` (`rev` faux) ou `b → a` (`rev` vrai) est ouvert.
    fn open(&self, e: u32, rev: bool) -> bool {
        let a = self.attrs[e as usize];
        if rev {
            a.bwd_ok()
        } else {
            a.fwd_ok()
        }
    }

    /// Sens de parcours de `e` quand on le quitte depuis `from` (faux : de `a` vers `b`).
    fn rev_from(&self, e: u32, from: u32) -> bool {
        self.ends[e as usize].0 != from
    }
}

/// Arêtes incidentes à chaque nœud (CSR), pour les seules arêtes `alive`.
#[derive(Debug)]
pub struct Incidence {
    off: Vec<u32>,
    edges: Vec<u32>,
}

impl Incidence {
    pub fn new(be: &BaseEdges, alive: &[bool]) -> Self {
        let mut off = vec![0u32; be.n_nodes + 1];
        for (e, &(a, b)) in be.ends.iter().enumerate() {
            if alive[e] {
                off[a as usize + 1] += 1;
                off[b as usize + 1] += 1;
            }
        }
        for v in 0..be.n_nodes {
            off[v + 1] += off[v];
        }
        let mut fill = off.clone();
        let mut edges = vec![0u32; off[be.n_nodes] as usize];
        for (e, &(a, b)) in be.ends.iter().enumerate() {
            if alive[e] {
                for v in [a, b] {
                    edges[fill[v as usize] as usize] = e as u32;
                    fill[v as usize] += 1;
                }
            }
        }
        Self { off, edges }
    }

    pub fn of(&self, v: u32) -> &[u32] {
        &self.edges[self.off[v as usize] as usize..self.off[v as usize + 1] as usize]
    }

    fn degree(&self, v: u32) -> usize {
        self.of(v).len()
    }
}

/// Arêtes de base vivantes : retenues par le filtre, puis, si demandé, sans impasses (élagage itératif des
/// nœuds de degré 1). Renvoie aussi le nombre d'arêtes retenues par le filtre seul.
pub fn alive_edges(be: &BaseEdges, spec: &ViewSpec) -> (Vec<bool>, u64) {
    let mut alive: Vec<bool> = be.attrs.iter().map(|&a| spec.kind.keep(a)).collect();
    let filtered = alive.iter().filter(|&&x| x).count() as u64;
    if spec.prune_dead_ends {
        let inc = Incidence::new(be, &alive);
        let mut deg: Vec<u32> = (0..be.n_nodes as u32).map(|v| inc.degree(v) as u32).collect();
        let mut queue: Vec<u32> = (0..be.n_nodes as u32).filter(|&v| deg[v as usize] == 1).collect();
        while let Some(v) = queue.pop() {
            if deg[v as usize] != 1 {
                continue;
            }
            let Some(&e) = inc.of(v).iter().find(|&&e| alive[e as usize]) else { continue };
            alive[e as usize] = false;
            let u = be.other(e, v);
            deg[v as usize] = 0;
            deg[u as usize] -= 1;
            if deg[u as usize] == 1 {
                queue.push(u);
            }
        }
    }
    (alive, filtered)
}

// --- Contraction ------------------------------------------------------------------------------------------------

/// Attributs relus dans l'autre sens : sens ouverts et sens voiture échangés.
fn oriented_attrs(a: Attrs, rev: bool) -> Attrs {
    if !rev {
        return a;
    }
    let mut f = a.unpack();
    std::mem::swap(&mut f.fwd_ok, &mut f.bwd_ok);
    f.car_oneway = match f.car_oneway {
        crate::attrs::CAR_FORWARD => crate::attrs::CAR_BACKWARD,
        crate::attrs::CAR_BACKWARD => crate::attrs::CAR_FORWARD,
        c => c,
    };
    Attrs::pack(&f)
}

/// Un maillon de chaîne : arête de base, parcourue de `b` vers `a` si `rev`.
type Link = (u32, bool);

struct Chain {
    links: Vec<Link>,
    from: u32,
    to: u32,
}

fn link_from(be: &BaseEdges, l: Link) -> u32 {
    let (a, b) = be.ends[l.0 as usize];
    if l.1 {
        b
    } else {
        a
    }
}

fn link_to(be: &BaseEdges, l: Link) -> u32 {
    let (a, b) = be.ends[l.0 as usize];
    if l.1 {
        a
    } else {
        b
    }
}

/// Le nœud `x` (de degré 2, arêtes `e1` et `e2`) peut-il être fondu ?
fn mergeable_at(be: &BaseEdges, kind: ViewKind, x: u32, e1: u32, e2: u32) -> bool {
    // Sens le long de la chaîne e1 → x → e2 : e1 est parcourue vers `x`, e2 depuis `x`.
    let into_e1 = be.open(e1, be.rev_from(e1, be.other(e1, x)));
    let out_e1 = be.open(e1, be.rev_from(e1, x));
    let into_e2 = be.open(e2, be.rev_from(e2, be.other(e2, x)));
    let out_e2 = be.open(e2, be.rev_from(e2, x));
    // Dans le sens e1 → e2 : (e1 vers x, x vers e2) ; dans le sens e2 → e1 : (e2 vers x, x vers e1).
    // Mêmes sens ouverts de part et d'autre : (into_e1, out_e1) == (out_e2, into_e2).
    if into_e1 != out_e2 || out_e1 != into_e2 {
        return false;
    }
    if kind == ViewKind::All {
        let a1 = oriented_attrs(be.attrs[e1 as usize], be.rev_from(e1, be.other(e1, x)));
        let a2 = oriented_attrs(be.attrs[e2 as usize], be.rev_from(e2, x));
        return a1 == a2;
    }
    true
}

#[derive(Default)]
struct ContractStats {
    merged_nodes: u64,
    loops_broken: u64,
    rings: u64,
}

/// Chaînes de la vue : depuis chaque ancre, chaque arête vivante non encore visitée ; puis les anneaux sans ancre.
/// Renvoie les chaînes et les nœuds ajoutés comme ancres pour couper une boucle.
fn contract(
    be: &BaseEdges,
    kind: ViewKind,
    alive: &[bool],
    st: &mut ContractStats,
) -> (Vec<Chain>, Vec<u32>) {
    let inc = Incidence::new(be, alive);
    let n = be.n_nodes;
    let mut mergeable = vec![false; n];
    for v in 0..n as u32 {
        if let [e1, e2] = inc.of(v) {
            if e1 != e2 && mergeable_at(be, kind, v, *e1, *e2) {
                mergeable[v as usize] = true;
            }
        }
    }
    st.merged_nodes = mergeable.iter().filter(|&&m| m).count() as u64;
    let mut visited = vec![false; be.ends.len()];
    let mut chains: Vec<Chain> = Vec::new();
    let mut extra_anchors: Vec<u32> = Vec::new();

    // Marche depuis `start` par l'arête `first`, jusqu'à une ancre (ou le retour au départ).
    let walk = |start: u32, first: u32, visited: &mut Vec<bool>| -> Chain {
        let mut links: Vec<Link> = Vec::new();
        let (mut cur, mut e) = (start, first);
        loop {
            let rev = be.rev_from(e, cur);
            links.push((e, rev));
            visited[e as usize] = true;
            let next = be.other(e, cur);
            if next == start || !mergeable[next as usize] {
                return Chain { links, from: start, to: next };
            }
            let &[x, y] = inc.of(next) else { unreachable!("nœud fondu de degré 2") };
            e = if x == e { y } else { x };
            cur = next;
        }
    };

    let push_chain = |c: Chain, chains: &mut Vec<Chain>, extra: &mut Vec<u32>, st: &mut ContractStats| {
        if c.from != c.to {
            chains.push(c);
            return;
        }
        // Boucle : coupée au milieu, le nœud du milieu devient une ancre. Deux arêtes au moins (la base n'a pas de boucle).
        st.loops_broken += 1;
        let m = c.links.len() / 2;
        debug_assert!(m >= 1 && m < c.links.len());
        let mid = link_from(be, c.links[m]);
        extra.push(mid);
        let (first, second) = c.links.split_at(m);
        chains.push(Chain { links: first.to_vec(), from: c.from, to: mid });
        chains.push(Chain { links: second.to_vec(), from: mid, to: c.to });
    };

    for v in 0..n as u32 {
        if mergeable[v as usize] || inc.degree(v) == 0 {
            continue;
        }
        for &e in inc.of(v) {
            if !visited[e as usize] {
                let c = walk(v, e, &mut visited);
                push_chain(c, &mut chains, &mut extra_anchors, st);
            }
        }
    }
    // Anneaux : arêtes vivantes restantes, dont tous les nœuds sont fondus. Ancre = le plus petit nœud.
    for e in 0..be.ends.len() as u32 {
        if !alive[e as usize] || visited[e as usize] {
            continue;
        }
        st.rings += 1;
        let start = be.ends[e as usize].0;
        let c = walk(start, e, &mut visited);
        debug_assert_eq!(c.from, c.to);
        // Rotation : la chaîne repart du plus petit nœud de l'anneau.
        let k = (0..c.links.len()).min_by_key(|&i| link_from(be, c.links[i])).unwrap_or(0);
        let mut links = c.links;
        links.rotate_left(k);
        let anchor = link_from(be, links[0]);
        extra_anchors.push(anchor);
        push_chain(Chain { links, from: anchor, to: anchor }, &mut chains, &mut extra_anchors, st);
    }
    (chains, extra_anchors)
}

fn u32_bytes(v: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

/// Somme des coûts des arêtes d'une chaîne (`NO_WEIGHT` si l'une n'en a pas).
fn chain_weight(be: &BaseEdges, links: &[Link]) -> u32 {
    let mut sum: u64 = 0;
    for &(e, _) in links {
        match edge_cost(be.attrs[e as usize], be.len_dm[e as usize]) {
            Some(w) => sum += u64::from(w),
            None => return NO_WEIGHT,
        }
    }
    sum.min(u64::from(NO_WEIGHT - 1)) as u32
}

struct ViewEdge {
    links: Vec<Link>,
    a: u32,
    b: u32,
    len_dm: u32,
    weight: u32,
    attrs: Attrs,
}

// --- Caps des chaînes (lot 2, §4) ----------------------------------------------------------------------------

/// Fenêtre de cap : les points à 30 m et à 50 m du bout de la chaîne (constantes Java, plan §2.2).
pub const HEADING_NEAR_M: f64 = 30.0;
pub const HEADING_FAR_M: f64 = 50.0;

/// Cap (degrés, 0 = nord, sens horaire, `[0, 360)`) d'une polyligne `(lat, lon)` en E7, à son début ou à sa
/// fin : direction entre les points situés à `near` et `far` mètres du bout (`RouteGuarantees.pathHeading`),
/// repli sur la corde de la polyligne si ces deux points sont à moins de 0,5 m l'un de l'autre.
pub fn polyline_heading(pts: &[(i32, i32)], at_start: bool) -> f64 {
    if pts.len() < 2 {
        return 0.0;
    }
    let lat0 = f64::from(pts[0].0) * 1e-7;
    let (ky, kx) = (111_194.926_6, 111_194.926_6 * lat0.to_radians().cos());
    let xy: Vec<(f64, f64)> = pts
        .iter()
        .map(|&(la, lo)| ((f64::from(lo - pts[0].1)) * 1e-7 * kx, (f64::from(la - pts[0].0)) * 1e-7 * ky))
        .collect();
    let seg: Vec<f64> = xy.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).collect();
    let total: f64 = seg.iter().sum();
    let far = HEADING_FAR_M.min(total);
    let near = HEADING_NEAR_M.min(far / 2.0);
    let (from, to) = if at_start { (near, far) } else { (total - far, total - near) };
    let along = |offset: f64| -> (f64, f64) {
        let mut acc = 0.0;
        for (i, &s) in seg.iter().enumerate() {
            if acc + s >= offset && s > 0.0 {
                let t = (offset - acc) / s;
                return (xy[i].0 + t * (xy[i + 1].0 - xy[i].0), xy[i].1 + t * (xy[i + 1].1 - xy[i].1));
            }
            acc += s;
        }
        xy[xy.len() - 1]
    };
    let (mut a, mut b) = (along(from), along(to));
    if (b.0 - a.0).hypot(b.1 - a.1) < 0.5 {
        a = xy[0];
        b = xy[xy.len() - 1];
    }
    let deg = (b.0 - a.0).atan2(b.1 - a.1).to_degrees();
    if deg < 0.0 {
        deg + 360.0
    } else {
        deg
    }
}

/// Cap en degrés vers sa forme stockée (`u16`, 360/65536 degrés).
pub fn heading_to_u16(deg: f64) -> u16 {
    ((deg.rem_euclid(360.0) / 360.0 * 65_536.0).round() as u32 & 0xffff) as u16
}

/// Cap stocké vers degrés.
pub fn heading_from_u16(h: u16) -> f64 {
    f64::from(h) * 360.0 / 65_536.0
}

/// `[début, fin]` des caps d'une polyligne, déjà en forme stockée.
pub fn polyline_heading_pair(pts: &[(i32, i32)]) -> [u16; 2] {
    [heading_to_u16(polyline_heading(pts, true)), heading_to_u16(polyline_heading(pts, false))]
}

/// Construit la vue `spec` du graphe de base `g` : les données prêtes pour `write_graph`.
pub fn build_view<B: AsRef<[u8]>>(g: &GraphFile<B>, spec: &ViewSpec) -> Result<GraphData, FormatError> {
    let be = BaseEdges::load(g);
    let (alive, filtered) = alive_edges(&be, spec);
    let pruned = alive.iter().filter(|&&x| x).count() as u64;
    let mut st = ContractStats::default();
    let (chains, extra_anchors) = contract(&be, spec.kind, &alive, &mut st);

    // Nœuds de la vue : extrémités de chaînes, dans l'ordre des ids de base (donc des cellules).
    let mut base_nodes: Vec<u32> = chains.iter().flat_map(|c| [c.from, c.to]).chain(extra_anchors).collect();
    base_nodes.sort_unstable();
    base_nodes.dedup();
    let mut view_id = vec![u32::MAX; be.n_nodes];
    for (i, &b) in base_nodes.iter().enumerate() {
        view_id[b as usize] = i as u32;
    }

    // Arêtes de la vue : orientées de l'id de vue le plus petit au plus grand, triées.
    let mut vedges: Vec<ViewEdge> = Vec::with_capacity(chains.len());
    for c in chains {
        let mut links = c.links;
        let (mut from, mut to) = (c.from, c.to);
        if view_id[from as usize] > view_id[to as usize] {
            links.reverse();
            for l in &mut links {
                l.1 = !l.1;
            }
            std::mem::swap(&mut from, &mut to);
        }
        let mut len: u64 = 0;
        let mut fwd = true;
        let mut bwd = true;
        let mut worst = 0u8;
        let mut routes = 0u8;
        let mut longest = (0u32, 0usize);
        for (i, &(e, rev)) in links.iter().enumerate() {
            let (ea, el) = (be.attrs[e as usize], be.len_dm[e as usize]);
            len += u64::from(el);
            fwd &= be.open(e, rev);
            bwd &= be.open(e, !rev);
            worst = worst.max(ea.access() as u8);
            routes |= ea.routes();
            if el > longest.0 || i == 0 {
                longest = (el, i);
            }
        }
        let len_dm = u32::try_from(len)
            .map_err(|_| FormatError::Build(format!("chaîne de {len} dm : plus longue que 2^32")))?;
        let (le, lrev) = links[longest.1];
        let mut f = oriented_attrs(be.attrs[le as usize], lrev).unpack();
        (f.fwd_ok, f.bwd_ok, f.access, f.routes) = (fwd, bwd, worst, routes);
        let weight = chain_weight(&be, &links);
        vedges.push(ViewEdge {
            a: view_id[from as usize],
            b: view_id[to as usize],
            links,
            len_dm,
            weight,
            attrs: Attrs::pack(&f),
        });
    }
    vedges.sort_by_key(|e| (e.a, e.b, e.links[0].0, e.links[0].1));

    // Cellules : celles des nœuds de base, dans le même ordre.
    let mut nodes = Vec::with_capacity(base_nodes.len());
    let mut cells: Vec<CellSpec> = Vec::new();
    let mut last_cell = usize::MAX;
    for (i, &b) in base_nodes.iter().enumerate() {
        let ci = g.node_cell(b);
        if ci != last_cell {
            let (x, y) = g.cell_xy(ci);
            cells.push(CellSpec { x, y, first_node: i as u32, n_nodes: 0 });
            last_cell = ci;
        }
        cells.last_mut().expect("cellule ouverte").n_nodes += 1;
        let (lat, lon) = g.node_pos(b);
        nodes.push(NodeData { osm_id: g.osm_node_id(b) as i64, lat, lon });
    }

    // Géométrie : polyligne complète de chaque chaîne (nœuds fondus compris).
    let pack = |lat: i32, lon: i32| (u64::from(lat as u32) << 32) | u64::from(lon as u32);
    let mut refs: Vec<u32> = Vec::new();
    let mut coords: Vec<std::sync::atomic::AtomicU64> = Vec::new();
    let mut put = |lat: i32, lon: i32, refs: &mut Vec<u32>| {
        refs.push(coords.len() as u32);
        coords.push(std::sync::atomic::AtomicU64::new(pack(lat, lon)));
    };
    let mut edges = Vec::with_capacity(vedges.len());
    let mut chain_off: Vec<u32> = vec![0];
    let mut chain_links: Vec<u32> = Vec::new();
    let mut weights: Vec<u32> = Vec::with_capacity(vedges.len());
    let mut headings: Vec<u8> = Vec::with_capacity(vedges.len() * 4);
    for ve in &vedges {
        let lo = refs.len() as u32;
        let (p0, p1) = g.node_pos(base_nodes[ve.a as usize]);
        put(p0, p1, &mut refs);
        let mut poly: Vec<(i32, i32)> = vec![(p0, p1)];
        for &(e, rev) in &ve.links {
            let mut inner = g.geometry(e);
            if rev {
                inner.reverse();
            }
            for (la, lo_) in inner {
                put(la, lo_, &mut refs);
                poly.push((la, lo_));
            }
            let (nl, nn) = g.node_pos(link_to(&be, (e, rev)));
            put(nl, nn, &mut refs);
            poly.push((nl, nn));
            chain_links.push(e | if rev { DIR_BIT } else { 0 });
        }
        let hi = refs.len() as u32 - 1;
        for h in polyline_heading_pair(&poly) {
            headings.extend_from_slice(&h.to_le_bytes());
        }
        chain_off.push(chain_links.len() as u32);
        weights.push(ve.weight);
        let way = g.identity(ve.links[0].0).0;
        edges.push(EdgeData {
            way_id: way as i64,
            a: ve.a,
            b: ve.b,
            attrs: ve.attrs.0,
            len_dm: ve.len_dm,
            lo,
            hi,
        });
    }

    let h = g.header();
    let meta = ViewMeta {
        name: spec.name(),
        kind: spec.kind,
        prune_dead_ends: spec.prune_dead_ends,
        base_nodes: h.nodes,
        base_edges: h.edges,
        base_pbf_xxh3: h.pbf_xxh3,
        base_build_epoch: h.build_epoch,
        filtered_edges: filtered,
        pruned_edges: filtered - pruned,
        view_nodes: nodes.len() as u64,
        view_edges: edges.len() as u64,
        merged_nodes: st.merged_nodes,
        loops_broken: st.loops_broken,
        rings: st.rings,
    };
    let stats = BuildStats {
        region: format!("vue {}", meta.name),
        vertices: nodes.len() as u64,
        edges: edges.len() as u64,
        ..BuildStats::default()
    };
    let extras = vec![
        ExtraSection {
            kind: KIND_VIEW_META,
            bytes: serde_json::to_vec(&meta).map_err(|e| FormatError::Build(e.to_string()))?,
        },
        ExtraSection { kind: KIND_VIEW_WEIGHT, bytes: u32_bytes(&weights) },
        ExtraSection { kind: KIND_VIEW_CHAIN_OFF, bytes: u32_bytes(&chain_off) },
        ExtraSection { kind: KIND_VIEW_CHAIN, bytes: u32_bytes(&chain_links) },
        ExtraSection { kind: KIND_VIEW_BASE_NODE, bytes: u32_bytes(&base_nodes) },
        ExtraSection { kind: KIND_VIEW_HEADING, bytes: headings },
    ];
    Ok(GraphData {
        zoom: h.zoom,
        nodes,
        cells,
        edges,
        refs,
        coords,
        stats,
        meta: SourceMeta {
            replication_timestamp: h.replication_timestamp,
            replication_sequence: h.replication_sequence,
            size: h.pbf_size,
            xxh3: h.pbf_xxh3,
        },
        extras,
    })
}

// --- Lecture d'une vue ------------------------------------------------------------------------------------------

impl<B: AsRef<[u8]>> GraphFile<B> {
    /// Vrai si ce fichier est une vue.
    pub fn is_view(&self) -> bool {
        self.optional_section(KIND_VIEW_META).is_some()
    }

    /// Méta de la vue, si ce fichier en est une.
    pub fn view_meta(&self) -> Option<ViewMeta> {
        serde_json::from_slice(self.optional_section(KIND_VIEW_META)?).ok()
    }

    /// Poids de chaque arête de la vue (`NO_WEIGHT` : inutilisable).
    pub fn view_weights(&self) -> Option<&[U32]> {
        <[U32]>::ref_from_bytes(self.optional_section(KIND_VIEW_WEIGHT)?).ok()
    }

    /// Caps de début et de fin de chaque arête de vue (`[début, fin]`, 360/65536 degrés, dans le sens `a → b`),
    /// si la vue les porte (format minor 2) ; voir [`polyline_heading`].
    pub fn view_headings(&self) -> Option<Vec<[u16; 2]>> {
        let b = self.optional_section(KIND_VIEW_HEADING)?;
        Some(
            b.chunks_exact(4)
                .map(|c| [u16::from_le_bytes([c[0], c[1]]), u16::from_le_bytes([c[2], c[3]])])
                .collect(),
        )
    }

    /// Nœud de base de chaque nœud de la vue (croissant).
    pub fn view_base_nodes(&self) -> Option<&[U32]> {
        <[U32]>::ref_from_bytes(self.optional_section(KIND_VIEW_BASE_NODE)?).ok()
    }

    /// Arêtes de base de l'arête `edge` de la vue, dans l'ordre de `a` vers `b` ; bit 31 : parcourue de `b` vers `a`.
    pub fn view_chain(&self, edge: u32) -> Option<&[U32]> {
        let off = <[U32]>::ref_from_bytes(self.optional_section(KIND_VIEW_CHAIN_OFF)?).ok()?;
        let all = <[U32]>::ref_from_bytes(self.optional_section(KIND_VIEW_CHAIN)?).ok()?;
        all.get(off.get(edge as usize)?.get() as usize..off.get(edge as usize + 1)?.get() as usize)
    }

    /// Cohérence des sections d'une vue (offsets croissants et bornés, ids dans les comptes de la base).
    pub(crate) fn validate_view(&self) -> Result<(), FormatError> {
        if !self.is_view() {
            return Ok(());
        }
        let bad = |what: &str| FormatError::Truncated(format!("vue : {what}"));
        let meta = self.view_meta().ok_or_else(|| bad("VIEW_META illisible"))?;
        let off =
            <[U32]>::ref_from_bytes(self.optional_section(KIND_VIEW_CHAIN_OFF).ok_or_else(|| bad("off"))?)
                .map_err(|_| bad("VIEW_CHAIN_OFF"))?;
        let all =
            <[U32]>::ref_from_bytes(self.optional_section(KIND_VIEW_CHAIN).ok_or_else(|| bad("chaînes"))?)
                .map_err(|_| bad("VIEW_CHAIN"))?;
        if off.first().map(|v| v.get()) != Some(0)
            || off.last().map(|v| v.get() as usize) != Some(all.len())
            || off.windows(2).any(|w| w[0].get() >= w[1].get())
        {
            return Err(bad("décalages des chaînes"));
        }
        if all.iter().any(|l| u64::from(l.get() & !DIR_BIT) >= meta.base_edges) {
            return Err(bad("arête de base hors comptes"));
        }
        let nodes = self.view_base_nodes().ok_or_else(|| bad("nœuds de base"))?;
        if nodes.iter().any(|n| u64::from(n.get()) >= meta.base_nodes)
            || nodes.windows(2).any(|w| w[0] >= w[1])
        {
            return Err(bad("nœuds de base"));
        }
        Ok(())
    }
}

/// Compte rendu de `check_view_structure`.
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct ViewStructure {
    pub view_edges: u64,
    pub base_edges_covered: u64,
    pub one_way_edges: u64,
    pub unusable_edges: u64,
    pub max_chain_len: u64,
    pub parallel_pairs: u64,
}

/// Contrôle exhaustif d'une vue contre son graphe de base : chaque chaîne est contiguë, joint bien ses deux
/// nœuds, somme ses longueurs, ses poids et ses sens ; chaque arête de base vivante est dans **une** chaîne et
/// aucune autre ; aucune boucle.
pub fn check_view_structure<B1: AsRef<[u8]>, B2: AsRef<[u8]>>(
    base: &GraphFile<B1>,
    view: &GraphFile<B2>,
) -> Result<ViewStructure, String> {
    let meta = view.view_meta().ok_or("ce fichier n'est pas une vue")?;
    let h = base.header();
    if (meta.base_nodes, meta.base_edges, meta.base_pbf_xxh3, meta.base_build_epoch)
        != (h.nodes, h.edges, h.pbf_xxh3, h.build_epoch)
    {
        return Err("la vue ne correspond pas à ce graphe de base (comptes ou empreinte)".into());
    }
    let spec = ViewSpec { kind: meta.kind, prune_dead_ends: meta.prune_dead_ends };
    let be = BaseEdges::load(base);
    let (alive, _) = alive_edges(&be, &spec);
    let weights = view.view_weights().ok_or("poids absents")?;
    let nodes = view.view_base_nodes().ok_or("nœuds de base absents")?;
    let mut seen = vec![false; be.ends.len()];
    let mut out = ViewStructure::default();
    let mut pairs: FxHashMap<(u32, u32), u32> = FxHashMap::default();
    for ve in 0..view.header().edges as u32 {
        let chain = view.view_chain(ve).ok_or("chaîne absente")?;
        if chain.is_empty() {
            return Err(format!("arête {ve} : chaîne vide"));
        }
        let (a, b) = view.edge_ends(ve);
        if a == b {
            return Err(format!("arête {ve} : boucle"));
        }
        *pairs.entry((a, b)).or_default() += 1;
        let (mut len, mut weight, mut fwd, mut bwd) = (0u64, 0u64, true, true);
        let mut no_weight = false;
        let mut at = nodes[a as usize].get();
        for l in chain {
            let (e, rev) = (l.get() & !DIR_BIT, l.get() & DIR_BIT != 0);
            if !alive[e as usize] {
                return Err(format!("arête {ve} : l'arête de base {e} n'est pas retenue par le filtre"));
            }
            if std::mem::replace(&mut seen[e as usize], true) {
                return Err(format!("arête de base {e} dans deux chaînes"));
            }
            if link_from(&be, (e, rev)) != at {
                return Err(format!("arête {ve} : chaîne non contiguë à l'arête de base {e}"));
            }
            at = link_to(&be, (e, rev));
            len += u64::from(be.len_dm[e as usize]);
            match edge_cost(be.attrs[e as usize], be.len_dm[e as usize]) {
                Some(w) => weight += u64::from(w),
                None => no_weight = true,
            }
            fwd &= be.open(e, rev);
            bwd &= be.open(e, !rev);
        }
        if at != nodes[b as usize].get() {
            return Err(format!("arête {ve} : la chaîne n'arrive pas au nœud b"));
        }
        let ev = view.edge(ve);
        if u64::from(ev.len_dm) != len {
            return Err(format!("arête {ve} : longueur {} ≠ somme {len}", ev.len_dm));
        }
        let want = if no_weight { NO_WEIGHT } else { weight.min(u64::from(NO_WEIGHT - 1)) as u32 };
        if weights[ve as usize].get() != want {
            return Err(format!("arête {ve} : poids {} ≠ somme {want}", weights[ve as usize].get()));
        }
        if (ev.attrs.fwd_ok(), ev.attrs.bwd_ok()) != (fwd, bwd) {
            return Err(format!(
                "arête {ve} : sens ({}, {}) ≠ chaîne ({fwd}, {bwd})",
                ev.attrs.fwd_ok(),
                ev.attrs.bwd_ok()
            ));
        }
        out.one_way_edges += u64::from(fwd != bwd);
        out.unusable_edges += u64::from(no_weight || (!fwd && !bwd));
        out.max_chain_len = out.max_chain_len.max(chain.len() as u64);
    }
    for (e, &al) in alive.iter().enumerate() {
        if al && !seen[e] {
            return Err(format!("arête de base vivante {e} absente de toute chaîne"));
        }
    }
    out.view_edges = view.header().edges;
    out.base_edges_covered = seen.iter().filter(|&&s| s).count() as u64;
    out.parallel_pairs = pairs.values().filter(|&&c| c > 1).count() as u64;
    Ok(out)
}

/// Distances de coût (mêmes poids que `edge_cost`) depuis `start` sur le graphe de base **filtré** : Dijkstra par
/// nœuds, sans disque ni interdiction de demi-tour ; `u32::MAX` si le nœud n'est pas atteint. C'est la
/// référence de la contraction : une vue doit donner les mêmes distances à ses nœuds.
pub fn filtered_reference_distances(be: &BaseEdges, inc: &Incidence, start: u32) -> Vec<u32> {
    let mut best = vec![u32::MAX; be.n_nodes];
    let mut heap = BinaryHeap::new();
    best[start as usize] = 0;
    heap.push(std::cmp::Reverse((0u32, start)));
    while let Some(std::cmp::Reverse((d, v))) = heap.pop() {
        if d > best[v as usize] {
            continue;
        }
        for &e in inc.of(v) {
            if !be.open(e, be.rev_from(e, v)) {
                continue;
            }
            let Some(w) = edge_cost(be.attrs[e as usize], be.len_dm[e as usize]) else { continue };
            let u = be.other(e, v);
            let nd = d.saturating_add(w);
            if nd < best[u as usize] {
                best[u as usize] = nd;
                heap.push(std::cmp::Reverse((nd, u)));
            }
        }
    }
    best
}

/// Rattache un nœud de base à un nœud d'une vue : lui-même s'il en est un ; sinon le nœud de vue le plus proche
/// en nombre d'arêtes, par un parcours (sens ignorés) sur les arêtes que le filtre retient.
#[derive(Debug)]
pub struct Snapper {
    inc: Incidence,
    be: BaseEdges,
}

impl Snapper {
    pub fn new<B: AsRef<[u8]>>(base: &GraphFile<B>, kind: ViewKind) -> Self {
        let be = BaseEdges::load(base);
        let keep: Vec<bool> = be.attrs.iter().map(|&a| kind.keep(a)).collect();
        Self { inc: Incidence::new(&be, &keep), be }
    }

    /// Nœud de la vue (id de vue) le plus proche de `start` (id de base), ou `None`. `view_nodes` : ids de base,
    /// croissants.
    pub fn snap(&self, view_nodes: &[U32], start: u32) -> Option<u32> {
        let find = |v: u32| view_nodes.binary_search_by_key(&v, |n| n.get()).ok().map(|i| i as u32);
        if let Some(i) = find(start) {
            return Some(i);
        }
        let mut seen: rustc_hash::FxHashSet<u32> = rustc_hash::FxHashSet::default();
        let mut frontier = vec![start];
        seen.insert(start);
        while !frontier.is_empty() && seen.len() < 50_000 {
            let mut next = Vec::new();
            let mut hit: Option<u32> = None;
            for &v in &frontier {
                for &e in self.inc.of(v) {
                    let u = self.be.other(e, v);
                    if !seen.insert(u) {
                        continue;
                    }
                    if let Some(i) = find(u) {
                        hit = Some(hit.map_or(i, |h| h.min(i)));
                    }
                    next.push(u);
                }
            }
            if hit.is_some() {
                return hit;
            }
            frontier = next;
        }
        None
    }
}
