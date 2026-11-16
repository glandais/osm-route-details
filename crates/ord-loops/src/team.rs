//! Attributs d'équipe (note lot 2, §2 ; lot 6, §7) : arêtes d'équipe par type, trous comblés (règle « cul-de-sac
//! fictif, 100 m »), composantes du réseau strict, et les agrégats par chaîne d'une fenêtre.
//!
//! Depuis le lot 6, la construction ne touche que la **zone de l'équipe** : les tuiles de ses arêtes et leurs voisines
//! à ± 1 km. Ce qui en sort est **global et creux** (ids de base et de chaîne du fichier) : les arêtes d'équipe ou
//! comblées avec leur fréquence, la longueur de la composante de chaque chaîne stricte, des statistiques. Les agrégats
//! par chaîne ([`WindowTeam`]) se calculent ensuite par fenêtre, à la première génération de l'équipe dans cette
//! fenêtre.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use ord_graph::attrs::HighwayClass;
use ord_graph::format::DIR_BIT;
use ord_graph::overlay::Overlay;
use rustc_hash::FxHashMap;

use crate::cost::{self, is_greenway, RideType};
use crate::engine::{
    chain_strong_len, rides_of_view, strong_components, strong_lengths, Engine, EngineError, ViewData,
};
use crate::geo::M_PER_DEG;
use crate::window::{tiles_of_rect, Window};

pub(crate) const TEAM: u8 = 1;
pub(crate) const BRIDGED: u8 = 2;

/// Demi-côté (m) de la boîte autour de chaque point d'une arête de la surcouche : la règle des trous ne regarde pas
/// plus loin que 100 m, la zone en prend dix fois plus.
const ZONE_BOX_M: f64 = 1_000.0;

/// Agrégat d'équipe d'une arête de vue pour un type.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ChainTeam {
    /// Somme des poids sans équipe des arêtes de base d'équipe de la chaîne.
    pub a: u32,
    /// Somme de celles qui sont comblées.
    pub b: u32,
    /// Fréquence moyenne (pondérée par `a`) des arêtes d'équipe, 0 à 255.
    pub z: u8,
    /// Toutes les arêtes de base sont d'équipe ou comblées.
    pub strict: bool,
}

#[derive(Debug)]
pub(crate) struct TypeTeam {
    /// Arêtes de base d'équipe ou comblées : `(arête, drapeaux, z)`, triées.
    pub edges: Vec<(u32, u8, u8)>,
    /// `(id global de chaîne stricte, longueur dm de sa composante stricte)`, triés ; absent : 0.
    pub strict_len: Vec<(u32, u64)>,
    pub team_edges: u64,
    pub team_m: f64,
    pub bridged_m: f64,
    /// Arêtes roulées par l'équipe (type demandé ou tous types) mais hors de la vue du type : (m du type, m tous types).
    pub outside_m: (f64, f64),
}

impl TypeTeam {
    pub fn flags(&self, e: u32) -> (u8, u8) {
        match self.edges.binary_search_by_key(&e, |x| x.0) {
            Ok(i) => (self.edges[i].1, self.edges[i].2),
            Err(_) => (0, 0),
        }
    }

    /// Longueur de la composante stricte fortement connexe de la chaîne (0 si ses bouts n'en partagent aucune), par
    /// son id **global**.
    pub fn strict_len_of(&self, gid: u32) -> u64 {
        match self.strict_len.binary_search_by_key(&gid, |x| x.0) {
            Ok(i) => self.strict_len[i].1,
            Err(_) => 0,
        }
    }
}

/// Ce que le moteur sait d'une équipe, pour un graphe : immuable, partageable.
#[derive(Debug)]
pub struct TeamData {
    pub(crate) per: [TypeTeam; 3],
}

/// Résumé pour le rapport et les tests.
#[derive(Clone, Copy, Debug, Default)]
pub struct TeamStats {
    /// Arêtes de base d'équipe pour le type.
    pub team_edges: u64,
    pub team_km: f64,
    /// Longueur comblée par la règle des trous.
    pub bridged_km: f64,
    /// Km roulés par l'équipe hors de la vue du type, par un parcours du type (ou de n'importe quel type).
    pub outside_typed_km: f64,
    pub outside_any_km: f64,
}

/// Les données d'équipe d'une génération : globales, et celles de la fenêtre.
#[derive(Clone, Copy, Debug)]
pub struct TeamCtx<'a> {
    pub data: &'a TeamData,
    pub win: &'a WindowTeam,
}

/// Ce qu'il faut construire pour une équipe : sa zone, et ce que la construction tiendra en mémoire le temps
/// qu'elle dure (§7.1 : fenêtres de la zone, incidence des arêtes de base vivantes, composantes strictes).
#[derive(Clone, Debug)]
pub struct BuildPlan {
    zone: Vec<u32>,
    /// Octets transitoires estimés de la construction.
    pub estimated_bytes: usize,
}

/// Octets transitoires par chaîne de la zone (la plus grande des deux vues) : fenêtre sans poids (≈ 61 o), arêtes
/// vivantes et incidence (≈ 47 o), composantes strictes (≈ 20 o), tables de hachage de la règle des trous. Réglé sur
/// les neuf équipes de la France (`ord-loops-lot6-mesures.md` §5) : le pic anonyme mesuré, moins les 32 Mo du banc,
/// reste sous l'estimation pour chacune.
const BUILD_BYTES_PER_CHAIN: usize = 210;

impl TeamData {
    /// Construit les attributs d'équipe pour les trois types sur la zone de l'équipe ; refuse une surcouche d'un
    /// autre graphe.
    pub fn build<B: AsRef<[u8]>>(e: &Engine<B>, overlay: &Overlay) -> Result<Self, EngineError> {
        let plan = Self::plan_build(e, overlay)?;
        Self::build_planned(e, overlay, &plan)
    }

    /// La zone de l'équipe et la mémoire transitoire de sa construction : l'appelant la réserve avant de construire
    /// (budget unique du service, §10.1).
    pub fn plan_build<B: AsRef<[u8]>>(e: &Engine<B>, overlay: &Overlay) -> Result<BuildPlan, EngineError> {
        overlay.check_graph(&e.base.graph_identity()).map_err(|_| EngineError::OverlayMismatch)?;
        let zone = zone_tiles(e, overlay);
        let chains: usize = (0..2).map(|vi| e.window_chain_count(vi, &zone)).max().unwrap_or(0);
        Ok(BuildPlan { estimated_bytes: chains * BUILD_BYTES_PER_CHAIN, zone })
    }

    /// Construit sur la zone d'un [`BuildPlan`].
    pub fn build_planned<B: AsRef<[u8]>>(
        e: &Engine<B>,
        overlay: &Overlay,
        plan: &BuildPlan,
    ) -> Result<Self, EngineError> {
        Self::build_zone(e, overlay, Some(&plan.zone))
    }

    /// Comme [`TeamData::build`], sur toutes les tuiles du graphe (`whole`) : le moteur à plat, référence des tests.
    pub fn build_on<B: AsRef<[u8]>>(
        e: &Engine<B>,
        overlay: &Overlay,
        whole: bool,
    ) -> Result<Self, EngineError> {
        overlay.check_graph(&e.base.graph_identity()).map_err(|_| EngineError::OverlayMismatch)?;
        let zone = (!whole).then(|| zone_tiles(e, overlay));
        Self::build_zone(e, overlay, zone.as_deref())
    }

    fn build_zone<B: AsRef<[u8]>>(
        e: &Engine<B>,
        overlay: &Overlay,
        zone: Option<&[u32]>,
    ) -> Result<Self, EngineError> {
        let mut views: Vec<(Window, ZoneView)> = Vec::new();
        for vi in 0..2 {
            let win = match zone {
                Some(z) => e.build_window_opts(vi, z, false, &mut || false).expect("jamais coupée"),
                None => e.full_window(vi),
            };
            let zv = ZoneView::new(e, &win.v);
            views.push((win, zv));
        }
        let per = RideType::ALL.map(|r| {
            let (win, zv) = &views[r.view_index()];
            build_type(e, overlay, r, &win.v, zv)
        });
        Ok(Self { per })
    }

    pub fn stats(&self, ride: RideType) -> TeamStats {
        let t = self.of(ride);
        TeamStats {
            team_edges: t.team_edges,
            team_km: t.team_m / 1000.0,
            bridged_km: t.bridged_m / 1000.0,
            outside_typed_km: t.outside_m.0 / 1000.0,
            outside_any_km: t.outside_m.1 / 1000.0,
        }
    }

    pub(crate) fn of(&self, ride: RideType) -> &TypeTeam {
        &self.per[ride.index()]
    }

    /// Vrai si l'arête de base est d'équipe pour le type (sans les comblements).
    pub fn is_team_edge(&self, ride: RideType, edge: u32) -> bool {
        self.of(ride).flags(edge).0 & TEAM != 0
    }

    /// Vrai si l'arête de base est comblée par la règle des trous.
    pub fn is_bridged_edge(&self, ride: RideType, edge: u32) -> bool {
        self.of(ride).flags(edge).0 & BRIDGED != 0
    }

    /// Octets tenus (capacités réelles).
    pub fn bytes(&self) -> usize {
        self.per.iter().map(|t| t.edges.capacity() * 8 + t.strict_len.capacity() * 16).sum()
    }

    /// Forme canonique en octets (finition 2, §20.4) : tout ce que la construction décide, par type, petit-boutiste,
    /// sans les capacités. Deux constructions égales rendent les mêmes octets ; sert de référence figée aux tests et
    /// à l'empreinte du banc (`--dump-teams`).
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for t in &self.per {
            out.extend((t.edges.len() as u64).to_le_bytes());
            for &(e, fl, z) in &t.edges {
                out.extend(e.to_le_bytes());
                out.push(fl);
                out.push(z);
            }
            out.extend((t.strict_len.len() as u64).to_le_bytes());
            for &(g, l) in &t.strict_len {
                out.extend(g.to_le_bytes());
                out.extend(l.to_le_bytes());
            }
            out.extend(t.team_edges.to_le_bytes());
            for f in [t.team_m, t.bridged_m, t.outside_m.0, t.outside_m.1] {
                out.extend(f.to_bits().to_le_bytes());
            }
        }
        out
    }
}

/// Surface rugueuse (note §2.1, Java `EdgeWeights.roughUnpaved`) : seule celle-là n'est présumée d'équipe en gravel
/// et VTT que si un parcours du bon type l'a prise. `compacted`, `fine_gravel`, `gravel`, `unpaved`, les pavés
/// n'en sont pas. Réécrite à part dans le contrôleur, qui n'importe pas le moteur.
fn rough_surface(a: ord_graph::attrs::Attrs) -> bool {
    use ord_graph::attrs::SURFACES;
    if a.tracktype() >= 4 {
        return true;
    }
    let code = usize::from(a.surface());
    if code == 0 {
        return false;
    }
    matches!(
        SURFACES.get(code - 1).copied(),
        Some(
            "ground"
                | "dirt"
                | "earth"
                | "grass"
                | "sand"
                | "mud"
                | "rock"
                | "wood"
                | "clay"
                | "woodchips"
                | "salt"
                | "ice"
        )
    )
}

/// Les agrégats d'équipe de chaque chaîne d'une fenêtre, par type servi par sa vue (lot 6, §7.2).
#[derive(Debug, Default)]
pub struct WindowTeam {
    per: [Vec<ChainTeam>; 3],
}

impl WindowTeam {
    /// Calcule les agrégats : les drapeaux de chaque lien viennent de [`TeamData`], par dichotomie.
    pub fn build<B: AsRef<[u8]>>(e: &Engine<B>, win: &Window, team: &TeamData) -> Self {
        let v = &win.v;
        let mut per: [Vec<ChainTeam>; 3] = Default::default();
        for &ride in rides_of_view(win.vi) {
            let tt = team.of(ride);
            let mut chains = vec![ChainTeam::default(); v.n_chains];
            for (c, ct) in chains.iter_mut().enumerate() {
                let (mut a, mut b, mut zs, mut strict) = (0u64, 0u64, 0u64, true);
                for l in v.links(c as u32) {
                    let ed = l & !DIR_BIT;
                    let (fl, z) = tt.flags(ed);
                    if fl & TEAM != 0 {
                        let (attrs, len) = e.attrs_len(ed);
                        let w = u64::from(cost::edge_weight(ride, attrs, len));
                        a += w;
                        zs += w * u64::from(z);
                    } else if fl & BRIDGED != 0 {
                        let (attrs, len) = e.attrs_len(ed);
                        b += u64::from(cost::edge_weight(ride, attrs, len));
                    } else {
                        strict = false;
                    }
                }
                *ct = ChainTeam {
                    a: a.min(u64::from(u32::MAX / 8)) as u32,
                    b: b.min(u64::from(u32::MAX / 8)) as u32,
                    z: (zs / a.max(1)).min(255) as u8,
                    strict,
                };
            }
            per[ride.index()] = chains;
        }
        Self { per }
    }

    /// Octets que tiendront les agrégats de `win` (une `ChainTeam` par chaîne et par type de sortie de la vue) : de
    /// quoi les réserver **avant** de les construire.
    pub fn estimate_bytes(win: &Window) -> usize {
        win.n_chains() * rides_of_view(win.vi).len() * std::mem::size_of::<ChainTeam>()
    }

    pub(crate) fn chains_of(&self, ride: RideType) -> &[ChainTeam] {
        &self.per[ride.index()]
    }

    /// Octets tenus (capacités réelles).
    pub fn bytes(&self) -> usize {
        self.per.iter().map(|c| c.capacity() * std::mem::size_of::<ChainTeam>()).sum()
    }
}

/// Zone de l'équipe : les tuiles qui contiennent un point d'une arête roulée, et celles que touche la boîte de
/// ± `ZONE_BOX_M` autour de chacun de ces points (§7.1).
fn zone_tiles<B: AsRef<[u8]>>(e: &Engine<B>, ov: &Overlay) -> Vec<u32> {
    let zoom = e.zoom();
    let ky = M_PER_DEG * 1e-7;
    let dlat = (ZONE_BOX_M / ky) as i32 + 2;
    let mut tiles: Vec<u32> = Vec::new();
    for (edge, c) in ov.iter() {
        if c.iter().all(|&x| x == 0) {
            continue;
        }
        let mut before = tiles.len();
        for p in e.edge_polyline(edge) {
            let dlon = (ZONE_BOX_M / (ky * (f64::from(p.0) * 1e-7).to_radians().cos().max(0.05))) as i32 + 2;
            tiles_of_rect(
                zoom,
                (p.0.saturating_sub(dlat), p.0.saturating_add(dlat)),
                (p.1.saturating_sub(dlon), p.1.saturating_add(dlon)),
                &mut tiles,
            );
            // Une longue arête repousse beaucoup de doublons : on les retire par paquets.
            if tiles.len() - before > 4096 {
                tiles.sort_unstable();
                tiles.dedup();
                before = tiles.len();
            }
        }
    }
    tiles.sort_unstable();
    tiles.dedup();
    tiles
}

/// Incidence d'une arête de base vivante en un nœud de base.
#[derive(Clone, Copy)]
struct Inc {
    node: u32,
    edge: u32,
    other: u32,
    len: u32,
}

/// Ce qu'il faut de la vue d'une zone pour la règle des trous : les arêtes de base vivantes (liens des chaînes) et
/// leur incidence par nœud de base, dans l'ordre croissant des arêtes (§7.1).
struct ZoneView {
    alive: Vec<u32>,
    inc: Vec<Inc>,
}

impl ZoneView {
    fn new<B: AsRef<[u8]>>(e: &Engine<B>, v: &ViewData) -> Self {
        let mut alive: Vec<u32> = v.chain_links.iter().map(|l| l & !DIR_BIT).collect();
        alive.sort_unstable();
        alive.dedup();
        let mut inc = Vec::with_capacity(2 * alive.len());
        for &ed in &alive {
            let (a, b) = e.ends(ed);
            let len = e.len_dm(ed);
            inc.push(Inc { node: a, edge: ed, other: b, len });
            inc.push(Inc { node: b, edge: ed, other: a, len });
        }
        inc.sort_unstable_by_key(|i| (i.node, i.edge));
        Self { alive, inc }
    }

    fn alive(&self, e: u32) -> bool {
        self.alive.binary_search(&e).is_ok()
    }

    fn inc_of(&self, x: u32) -> &[Inc] {
        let lo = self.inc.partition_point(|i| i.node < x);
        let hi = self.inc.partition_point(|i| i.node <= x);
        &self.inc[lo..hi]
    }
}

#[derive(Clone, Copy)]
struct EdgeInfo {
    fl: u8,
    n: u32,
    z: u8,
}

fn type_index(o: &Overlay, name: &str) -> Option<usize> {
    o.meta.types.iter().position(|t| t == name)
}

fn build_type<B: AsRef<[u8]>>(
    e: &Engine<B>,
    ov: &Overlay,
    ride: RideType,
    v: &ViewData,
    zv: &ZoneView,
) -> TypeTeam {
    let (i_road, i_gravel, i_mtb, i_mixed) =
        (type_index(ov, "ROAD"), type_index(ov, "GRAVEL"), type_index(ov, "MTB"), type_index(ov, "MIXED"));
    let cnt = |c: &[u16], idx: Option<usize>, dir: usize| idx.map_or(0u32, |i| u32::from(c[2 * i + dir]));
    let mut info: FxHashMap<u32, EdgeInfo> = FxHashMap::default();
    let mut max_n = 1u32;
    let mut outside_dm = (0u64, 0u64);
    for (edge, c) in ov.iter() {
        let any = c.iter().any(|&x| x > 0);
        if !any {
            continue;
        }
        if !zv.alive(edge) {
            let typed = match ride {
                RideType::Road => {
                    cnt(c, i_road, 0).max(cnt(c, i_road, 1)).max(cnt(c, i_mixed, 0)).max(cnt(c, i_mixed, 1))
                }
                RideType::Gravel => cnt(c, i_gravel, 0)
                    .max(cnt(c, i_gravel, 1))
                    .max(cnt(c, i_mixed, 0))
                    .max(cnt(c, i_mixed, 1)),
                RideType::Mtb => {
                    cnt(c, i_mtb, 0).max(cnt(c, i_mtb, 1)).max(cnt(c, i_mixed, 0)).max(cnt(c, i_mixed, 1))
                }
            };
            let l = u64::from(e.len_dm(edge));
            outside_dm.1 += l;
            if typed > 0 {
                outside_dm.0 += l;
            }
            continue;
        }
        let dirs = |f: &dyn Fn(usize) -> u32| f(0).max(f(1));
        let mut typed = match ride {
            RideType::Road => dirs(&|d| cnt(c, i_road, d) + cnt(c, i_mixed, d)),
            RideType::Gravel => dirs(&|d| cnt(c, i_gravel, d) + cnt(c, i_mixed, d)),
            RideType::Mtb => dirs(&|d| cnt(c, i_mtb, d) + cnt(c, i_mixed, d)),
        };
        if ride == RideType::Mtb && typed == 0 {
            typed = dirs(&|d| cnt(c, i_gravel, d));
        }
        let a = e.attrs(edge);
        // Exceptions Java (plan §5.5) : une surface présumée n'est acceptée que d'un parcours du bon type.
        let exception = match ride {
            RideType::Road => (is_greenway(a) || a.class() == Some(HighwayClass::Track)) && a.surface() == 0,
            RideType::Gravel | RideType::Mtb => {
                matches!(a.class(), Some(HighwayClass::Path | HighwayClass::Footway)) || rough_surface(a)
            }
        };
        let team = if exception { typed > 0 } else { true };
        if !team {
            continue;
        }
        let n = if typed > 0 {
            typed
        } else {
            dirs(&|d| (0..ov.meta.types.len()).map(|i| u32::from(c[2 * i + d])).sum())
        };
        info.insert(edge, EdgeInfo { fl: TEAM, n, z: 0 });
        max_n = max_n.max(n);
    }
    let lm = f64::from(max_n).ln_1p();
    for inf in info.values_mut() {
        inf.z = (255.0 * f64::from(inf.n).ln_1p() / lm).round().clamp(0.0, 255.0) as u8;
    }

    // Trous : culs-de-sac fictifs et plus court chemin non roulé de 100 m au plus vers un autre nœud d'équipe.
    let mut tdeg: FxHashMap<u32, u8> = FxHashMap::default();
    for &edge in info.keys() {
        let (a, b) = e.ends(edge);
        for x in [a, b] {
            let d = tdeg.entry(x).or_insert(0);
            *d = d.saturating_add(1);
        }
    }
    let is_team = |be: u32| info.contains_key(&be);
    let hole_dm = (e.params.hole_m * 10.0) as u32;
    let mut starts: Vec<u32> =
        tdeg.iter().filter(|&(&x, &d)| d == 1 && zv.inc_of(x).len() >= 2).map(|(&x, _)| x).collect();
    starts.sort_unstable();
    starts.dedup();
    let mut to_bridge: Vec<u32> = Vec::new();
    let walk_back = |dist: &FxHashMap<u32, (u32, u32)>, from: u32, s: u32, out: &mut Vec<u32>| {
        let mut cur = from;
        while cur != s {
            let (_, pe) = dist[&cur];
            out.push(pe);
            let (a, b) = e.ends(pe);
            cur = if a == cur { b } else { a };
        }
    };
    for &s in &starts {
        let mut dist: FxHashMap<u32, (u32, u32)> = FxHashMap::default();
        let mut heap: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
        dist.insert(s, (0, u32::MAX));
        heap.push(Reverse((0, s)));
        while let Some(Reverse((d, x))) = heap.pop() {
            if dist[&x].0 < d {
                continue;
            }
            if x != s && tdeg.contains_key(&x) {
                walk_back(&dist, x, s, &mut to_bridge);
                break;
            }
            for i in zv.inc_of(x) {
                if is_team(i.edge) {
                    continue;
                }
                let nd = d + i.len;
                if nd > hole_dm {
                    continue;
                }
                let y = i.other;
                if dist.get(&y).is_none_or(|&(od, _)| nd < od) {
                    dist.insert(y, (nd, i.edge));
                    heap.push(Reverse((nd, y)));
                }
            }
        }
    }
    // Jonctions courtes : un chemin non roulé de `connector_m` au plus entre deux nœuds d'équipe (seconde chaussée
    // d'une 2 × 2, raccord d'un carrefour que l'appariement a manqué) ; sans elle une chaussée à sens unique
    // roulée dans un sens ne se rejoint pas dans l'autre.
    let conn_dm = (e.params.connector_m * 10.0) as u32;
    let mut team_nodes: Vec<u32> = tdeg.keys().copied().collect();
    team_nodes.sort_unstable();
    for &s in &team_nodes {
        if !zv.inc_of(s).iter().any(|i| !is_team(i.edge) && i.len <= conn_dm) {
            continue;
        }
        let mut dist: FxHashMap<u32, (u32, u32)> = FxHashMap::default();
        let mut heap: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
        dist.insert(s, (0, u32::MAX));
        heap.push(Reverse((0, s)));
        while let Some(Reverse((d, x))) = heap.pop() {
            if dist[&x].0 < d {
                continue;
            }
            if x != s && tdeg.contains_key(&x) {
                walk_back(&dist, x, s, &mut to_bridge);
                continue;
            }
            for i in zv.inc_of(x) {
                if is_team(i.edge) {
                    continue;
                }
                let nd = d + i.len;
                if nd > conn_dm {
                    continue;
                }
                let y = i.other;
                if dist.get(&y).is_none_or(|&(od, _)| nd < od) {
                    dist.insert(y, (nd, i.edge));
                    heap.push(Reverse((nd, y)));
                }
            }
        }
    }
    for be in to_bridge {
        info.entry(be).and_modify(|i| i.fl |= BRIDGED).or_insert(EdgeInfo { fl: BRIDGED, n: 0, z: 0 });
    }

    let mut edges: Vec<(u32, u8, u8)> =
        info.iter().filter(|(_, i)| i.fl != 0).map(|(&ed, i)| (ed, i.fl, i.z)).collect();
    edges.sort_unstable_by_key(|x| x.0);
    let (mut team_dm, mut bridged_dm, mut team_edges) = (0u64, 0u64, 0u64);
    for &(ed, fl, _) in &edges {
        if fl & TEAM != 0 {
            team_dm += u64::from(e.len_dm(ed));
            team_edges += 1;
        } else {
            bridged_dm += u64::from(e.len_dm(ed));
        }
    }

    // Composantes fortement connexes du réseau strict (sens respectés, virages ignorés) : un réseau strict est fait
    // d'arêtes d'équipe ou comblées, toutes dans la zone ; le calcul sur la fenêtre de la zone rend les composantes
    // du graphe entier.
    let flagged = |ed: u32| match edges.binary_search_by_key(&ed, |x| x.0) {
        Ok(i) => edges[i].1 & (TEAM | BRIDGED) != 0,
        Err(_) => false,
    };
    let strict: Vec<bool> =
        (0..v.n_chains as u32).map(|c| v.links(c).iter().all(|l| flagged(l & !DIR_BIT))).collect();
    let comp = strong_components(v.n_nodes, &|x, s: &mut Vec<u32>| {
        s.extend(v.out(x).iter().filter(|&&f| strict[(f >> 1) as usize]).map(|&f| v.tgt(f)));
    });
    let lens = strong_lengths(&comp, &v.ends, &v.len_dm, &|c| strict[c]);
    let mut strict_len: Vec<(u32, u64)> = Vec::new();
    for c in 0..v.n_chains {
        if strict[c] {
            let l = chain_strong_len(&comp, &lens, v.ends[c]);
            if l > 0 {
                strict_len.push((v.gid[c], l));
            }
        }
    }
    TypeTeam {
        edges,
        strict_len,
        team_edges,
        team_m: team_dm as f64 / 10.0,
        bridged_m: bridged_dm as f64 / 10.0,
        outside_m: (outside_dm.0 as f64 / 10.0, outside_dm.1 as f64 / 10.0),
    }
}
