//! Les tables du contrôleur, **bornées** (note lot 6, §8) : avant, il reconstruisait à chaque requête des tableaux
//! sur tout le graphe (degrés de tous les nœuds, incidence de toutes les arêtes admises, un booléen par arête), soit
//! 0,3 à 0,45 Go et une à deux secondes sur la France. Maintenant :
//!
//! - le **degré admis** ne se calcule que pour les nœuds du parcours, en balayant les arêtes des cellules de ces
//!   nœuds et de leurs voisines ;
//! - les **arêtes comblées** (règle des trous) se calculent une fois par équipe, sur la zone de l'équipe (tuiles de
//!   ses arêtes et voisines à ± 1 km, puis leurs voisines), et se gardent en [`TeamCheck`] ;
//! - une arête plus large qu'une cellule ([`GraphIndex`]) est ajoutée à l'incidence de ses deux bouts : une arête
//!   possédée par `C` n'a ses bouts que dans `C` et ses voisines tant qu'elle n'est pas plus large qu'une cellule.
//!
//! Écrit depuis la même règle que le reste du contrôleur, sans rien du moteur ni de ses fenêtres.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use ord_graph::format::GraphFile;
use ord_graph::grid::{tile_key, tile_of_e7, tile_of_key};
use ord_graph::overlay::Overlay;
use ord_loops::RideType;

use crate::check::Config;
use crate::rules::{admitted, team_edge, Admission, TypeIndex};

const M_PER_DEG: f64 = 6_371_008.8 * std::f64::consts::PI / 180.0;
/// Demi-côté (m) de la boîte autour de chaque point d'une arête de la surcouche.
const ZONE_BOX_M: f64 = 1_000.0;

/// Ce qui est vrai de tout le graphe : les arêtes plus larges qu'une cellule.
#[derive(Debug, Default)]
pub struct GraphIndex {
    /// Arêtes dont un bout n'est ni dans leur cellule ni dans une voisine, croissantes.
    wide: Vec<u32>,
}

impl GraphIndex {
    /// Un balayage des enregistrements d'arêtes ; seules les arêtes de plus de la moitié d'une tuile sont examinées.
    pub fn build<B: AsRef<[u8]>>(g: &GraphFile<B>) -> Self {
        let zoom = g.header().zoom;
        let mut wide = Vec::new();
        for ci in 0..g.n_cells() {
            let info = *g.cell_info(ci);
            let thr_dm = ord_graph::grid::wide_threshold_dm(info.y, zoom);
            let cv = g.cell(ci);
            for k in 0..info.n_edges {
                if cv.owned(k as usize).len_dm <= thr_dm {
                    continue;
                }
                let e = info.first_edge + k;
                let (a, b) = g.edge_ends(e);
                let far = |n: u32| {
                    let (x, y) = g.cell_xy(g.node_cell(n));
                    x.abs_diff(info.x) > 1 || y.abs_diff(info.y) > 1
                };
                if far(a) || far(b) {
                    wide.push(e);
                }
            }
        }
        Self { wide }
    }

    pub fn wide_edges(&self) -> &[u32] {
        &self.wide
    }
}

/// Cellules (indices) des tuiles `keys` et de leurs 8 voisines.
fn cells_around<B: AsRef<[u8]>>(g: &GraphFile<B>, keys: impl IntoIterator<Item = u32>) -> Vec<usize> {
    let mut tiles: Vec<u32> = Vec::new();
    for k in keys {
        let (x, y) = tile_of_key(k);
        for nx in x.saturating_sub(1)..=x + 1 {
            for ny in y.saturating_sub(1)..=y + 1 {
                tiles.push(tile_key(nx, ny));
            }
        }
    }
    tiles.sort_unstable();
    tiles.dedup();
    tiles
        .into_iter()
        .filter_map(|k| {
            let (x, y) = tile_of_key(k);
            g.find_cell(x, y)
        })
        .collect()
}

/// Arêtes admises incidentes à chaque nœud, des cellules `cells` : `(nœud, arête)` triés, sens ignorés (une boucle
/// compte une fois), plus les arêtes plus larges qu'une cellule que ces cellules ne possèdent pas.
pub(crate) fn incidence_of<B: AsRef<[u8]>>(
    g: &GraphFile<B>,
    gi: &GraphIndex,
    adm: Admission,
    cells: &[usize],
) -> Vec<(u32, u32)> {
    let mut list: Vec<(u32, u32)> = Vec::new();
    let mut scanned = HashSet::new();
    for &ci in cells {
        scanned.insert(ci);
        let info = *g.cell_info(ci);
        let cv = g.cell(ci);
        for k in 0..info.n_edges {
            if admitted(adm, cv.owned(k as usize).attrs) {
                let e = info.first_edge + k;
                let (a, b) = g.edge_ends(e);
                list.push((a, e));
                if a != b {
                    list.push((b, e));
                }
            }
        }
    }
    for &e in &gi.wide {
        if scanned.contains(&g.edge_cell(e)) {
            continue;
        }
        if admitted(adm, g.edge(e).attrs) {
            let (a, b) = g.edge_ends(e);
            list.push((a, e));
            if a != b {
                list.push((b, e));
            }
        }
    }
    list.sort_unstable();
    list.dedup();
    list
}

pub(crate) fn edges_at(inc: &[(u32, u32)], n: u32) -> impl Iterator<Item = u32> + '_ {
    let lo = inc.partition_point(|x| x.0 < n);
    let hi = inc.partition_point(|x| x.0 <= n);
    inc[lo..hi].iter().map(|x| x.1)
}

/// Degré admis des nœuds `nodes` : arêtes admises incidentes, sens ignorés, une boucle comptant deux fois.
pub(crate) fn degrees_of<B: AsRef<[u8]>>(
    g: &GraphFile<B>,
    gi: &GraphIndex,
    adm: Admission,
    nodes: &[u32],
) -> HashMap<u32, u8> {
    let want: HashSet<u32> = nodes.iter().copied().collect();
    let keys: HashSet<u32> = nodes
        .iter()
        .map(|&n| {
            let (x, y) = g.cell_xy(g.node_cell(n));
            tile_key(x, y)
        })
        .collect();
    let cells = cells_around(g, keys);
    let inc = incidence_of(g, gi, adm, &cells);
    let mut deg: HashMap<u32, u8> = want.iter().map(|&n| (n, 0)).collect();
    // Une boucle (a == b) compte deux fois.
    for &n in &want {
        for e in edges_at(&inc, n) {
            let (a, b) = g.edge_ends(e);
            let d = deg.get_mut(&n).expect("nœud voulu");
            *d = d.saturating_add(1 + u8::from(a == b));
        }
    }
    deg
}

/// Octets transitoires par case chaude (§20.2) : les deux listes d'incidence chaude `(nœud, arête)` (8 o par entrée),
/// les tables d'équipe et les parcours bornés, croissance des vecteurs comprise. Réglé sur les neuf équipes de la
/// France au pic anonyme mesuré (`ord-loops-lot6-mesures.md` §10) plus 25 %.
const BUILD_BYTES_PER_HOT_CASE: usize = 340;
/// Octets fixes de la construction (tables, tampons).
const BUILD_BYTES_FIXED: usize = 2_000_000;
/// Côté (E7) d'une case chaude : 0,002° (~220 m).
const HOT_CASE_E7: i32 = 20_000;
/// Marge (m) ajoutée à la plus grande des portées de la règle des trous.
const HOT_MARGIN_M: f64 = 150.0;

/// Clé d'une case chaude.
fn case_key(p: (i32, i32)) -> u64 {
    (u64::from(p.0.div_euclid(HOT_CASE_E7) as u32) << 32) | u64::from(p.1.div_euclid(HOT_CASE_E7) as u32)
}

/// Les cases de 0,002° que touche la boîte de ± `hot_m` autour de **chaque bout** de chaque arête de la surcouche
/// dont un compte est non nul (§20.2) : écrit pour le contrôleur, sans rien emprunter au moteur.
fn hot_cases<B: AsRef<[u8]>>(g: &GraphFile<B>, overlay: &Overlay, hot_m: f64) -> HashSet<u64> {
    let ky = M_PER_DEG * 1e-7;
    let dlat = (hot_m / ky) as i32 + 2;
    let mut cases: HashSet<u64> = HashSet::new();
    for (e, c) in overlay.iter() {
        if c.iter().all(|&x| x == 0) {
            continue;
        }
        let (a, b) = g.edge_ends(e);
        for n in [a, b] {
            let p = g.node_pos(n);
            let dlon = (hot_m / (ky * (f64::from(p.0) * 1e-7).to_radians().cos().max(0.05))) as i32 + 2;
            let (la0, la1) = (
                p.0.saturating_sub(dlat).div_euclid(HOT_CASE_E7),
                p.0.saturating_add(dlat).div_euclid(HOT_CASE_E7),
            );
            let (lo0, lo1) = (
                p.1.saturating_sub(dlon).div_euclid(HOT_CASE_E7),
                p.1.saturating_add(dlon).div_euclid(HOT_CASE_E7),
            );
            for la in la0..=la1 {
                for lo in lo0..=lo1 {
                    cases.insert((u64::from(la as u32) << 32) | u64::from(lo as u32));
                }
            }
        }
    }
    cases
}

/// Portée chaude par défaut (m) : la plus longue des deux règles, plus la marge, sans dépasser la boîte de la zone.
fn default_hot_m(cfg: &Config) -> f64 {
    (cfg.hole_m.max(cfg.connector_m) + HOT_MARGIN_M).min(ZONE_BOX_M)
}

/// Une marche de la règle des trous développerait un nœud hors des cases chaudes (§20.2).
#[derive(Debug)]
pub struct TeamCheckEscape;

impl std::fmt::Display for TeamCheckEscape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "les tables du contrôleur ont quitté le voisinage de l'équipe")
    }
}

impl std::error::Error for TeamCheckEscape {}

/// Nœuds chauds : la case de leur position est chaude et leur tuile est une tuile de la zone.
struct Hot<'a> {
    cases: &'a HashSet<u64>,
    zone: &'a [u32],
    zoom: u8,
}

impl Hot<'_> {
    fn contains<B: AsRef<[u8]>>(&self, g: &GraphFile<B>, n: u32) -> bool {
        let p = g.node_pos(n);
        let (x, y) = tile_of_e7(p.0, p.1, self.zoom);
        self.cases.contains(&case_key(p)) && self.zone.binary_search(&tile_key(x, y)).is_ok()
    }
}

/// `None` : tout nœud est chaud (aucun filtre : la construction d'avant, sur les cellules données).
fn is_hot<B: AsRef<[u8]>>(hot: Option<&Hot<'_>>, g: &GraphFile<B>, n: u32) -> bool {
    hot.is_none_or(|h| h.contains(g, n))
}

/// Cellules de la zone de l'équipe (tuiles de ses arêtes et leurs voisines à ± 1 km, puis les cellules de leurs
/// voisines) et tuiles de la zone.
pub(crate) fn zone_cells<B: AsRef<[u8]>>(g: &GraphFile<B>, overlay: &Overlay) -> (Vec<usize>, Vec<u32>) {
    let zoom = g.header().zoom;
    // Tuiles qui contiennent un point d'une arête de la surcouche, ou que touche la boîte de ± 1 km.
    let ky = M_PER_DEG * 1e-7;
    let dlat = (ZONE_BOX_M / ky) as i32 + 2;
    let mut tiles: Vec<u32> = Vec::new();
    for (e, c) in overlay.iter() {
        if c.iter().all(|&x| x == 0) {
            continue;
        }
        let (a, b) = g.edge_ends(e);
        let mut pts = vec![g.node_pos(a)];
        pts.extend(g.geometry(e));
        pts.push(g.node_pos(b));
        for p in pts {
            let dlon = (ZONE_BOX_M / (ky * (f64::from(p.0) * 1e-7).to_radians().cos().max(0.05))) as i32 + 2;
            let (x0, y0) = tile_of_e7(p.0.saturating_add(dlat), p.1.saturating_sub(dlon), zoom);
            let (x1, y1) = tile_of_e7(p.0.saturating_sub(dlat), p.1.saturating_add(dlon), zoom);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    tiles.push(tile_key(x, y));
                }
            }
        }
        if tiles.len() > 1 << 16 {
            tiles.sort_unstable();
            tiles.dedup();
        }
    }
    tiles.sort_unstable();
    tiles.dedup();
    let cells = cells_around(g, tiles.iter().copied());
    (cells, tiles)
}

/// Ce qu'il faut construire pour les tables du contrôleur d'une équipe : les cellules de sa zone, ses cases chaudes
/// (§20.2), et la mémoire transitoire de la construction.
#[derive(Clone, Debug)]
pub struct CheckPlan {
    cells: Vec<usize>,
    zone: Vec<u32>,
    /// `None` : pas de filtre (toutes les cellules du graphe, la référence des tests).
    hot: Option<HashSet<u64>>,
    /// Octets transitoires estimés de la construction.
    pub estimated_bytes: usize,
}

impl CheckPlan {
    /// Nombre de cases chaudes de la construction (0 sans filtre).
    pub fn hot_cells(&self) -> usize {
        self.hot.as_ref().map_or(0, HashSet::len)
    }
}

/// Les arêtes comblées par la règle des trous, par type : une fois par équipe, sur la zone de l'équipe.
#[derive(Debug, Default)]
pub struct TeamCheck {
    bridged: [HashSet<u32>; 3],
    retried: bool,
}

fn slot(r: RideType) -> usize {
    match r {
        RideType::Road => 0,
        RideType::Gravel => 1,
        RideType::Mtb => 2,
    }
}

impl TeamCheck {
    /// Calcule les arêtes comblées des trois types sur la zone de l'équipe. Sans erreur possible : si le garde-fou
    /// des cases chaudes lève deux fois, la construction se fait sans filtre (celle d'avant).
    pub fn build<B: AsRef<[u8]>>(g: &GraphFile<B>, gi: &GraphIndex, overlay: &Overlay, cfg: &Config) -> Self {
        let plan = Self::plan(g, overlay, cfg);
        match Self::build_planned(g, gi, overlay, cfg, &plan) {
            Ok(t) => t,
            Err(TeamCheckEscape) => {
                let none = CheckPlan { hot: None, ..plan };
                Self::build_planned(g, gi, overlay, cfg, &none).expect("sans filtre, aucun échappement")
            }
        }
    }

    /// Comme [`TeamCheck::build`] ; `whole` : sur toutes les cellules du graphe, sans filtre (la référence des tests).
    pub fn build_on<B: AsRef<[u8]>>(
        g: &GraphFile<B>,
        gi: &GraphIndex,
        overlay: &Overlay,
        cfg: &Config,
        whole: bool,
    ) -> Self {
        if !whole {
            return Self::build(g, gi, overlay, cfg);
        }
        let plan =
            CheckPlan { cells: (0..g.n_cells()).collect(), zone: Vec::new(), hot: None, estimated_bytes: 0 };
        Self::build_planned(g, gi, overlay, cfg, &plan).expect("sans filtre, aucun échappement")
    }

    /// Les cellules de la zone de l'équipe, ses cases chaudes et la mémoire transitoire de la construction :
    /// l'appelant la réserve avant de construire (budget unique du service, lot 6 §10.1), comme pour
    /// `TeamData::plan_build`.
    pub fn plan<B: AsRef<[u8]>>(g: &GraphFile<B>, overlay: &Overlay, cfg: &Config) -> CheckPlan {
        let (cells, zone) = zone_cells(g, overlay);
        let hot = hot_cases(g, overlay, default_hot_m(cfg));
        let estimated_bytes = BUILD_BYTES_FIXED + hot.len() * BUILD_BYTES_PER_HOT_CASE;
        CheckPlan { cells, zone, hot: Some(hot), estimated_bytes }
    }

    /// Construit sur les cellules et les cases chaudes d'un [`CheckPlan`]. Si le garde-fou lève (une marche
    /// développerait un nœud hors des cases chaudes), la construction est refaite **une** fois avec des cases à
    /// `ZONE_BOX_M` ; un second échappement rend [`TeamCheckEscape`], jamais un résultat faux.
    pub fn build_planned<B: AsRef<[u8]>>(
        g: &GraphFile<B>,
        gi: &GraphIndex,
        overlay: &Overlay,
        cfg: &Config,
        plan: &CheckPlan,
    ) -> Result<Self, TeamCheckEscape> {
        let zoom = g.header().zoom;
        let run = |cases: Option<&HashSet<u64>>| {
            let hot = cases.map(|c| Hot { cases: c, zone: &plan.zone, zoom });
            build_tables(g, gi, overlay, cfg, &plan.cells, hot.as_ref())
        };
        match run(plan.hot.as_ref()) {
            Ok(bridged) => Ok(Self { bridged, retried: false }),
            Err(TeamCheckEscape) => {
                let wide = hot_cases(g, overlay, ZONE_BOX_M);
                let bridged = run(Some(&wide))?;
                Ok(Self { bridged, retried: true })
            }
        }
    }

    /// Construit avec des cases chaudes de `hot_m` mètres, avec ou sans reprise (les tests forcent le garde-fou).
    #[cfg(test)]
    pub(crate) fn build_forced<B: AsRef<[u8]>>(
        g: &GraphFile<B>,
        gi: &GraphIndex,
        overlay: &Overlay,
        cfg: &Config,
        hot_m: f64,
        retry: bool,
    ) -> Result<Self, TeamCheckEscape> {
        let mut plan = Self::plan(g, overlay, cfg);
        plan.hot = Some(hot_cases(g, overlay, hot_m));
        if retry {
            Self::build_planned(g, gi, overlay, cfg, &plan)
        } else {
            let hot =
                Hot { cases: plan.hot.as_ref().expect("filtre"), zone: &plan.zone, zoom: g.header().zoom };
            let bridged = build_tables(g, gi, overlay, cfg, &plan.cells, Some(&hot))?;
            Ok(Self { bridged, retried: false })
        }
    }

    /// Vrai si la construction a dû être refaite à `ZONE_BOX_M` (échappement du garde-fou ; doit rester rare).
    pub fn zone_retried(&self) -> bool {
        self.retried
    }

    pub fn bridged(&self, ride: RideType) -> &HashSet<u32> {
        &self.bridged[slot(ride)]
    }

    #[cfg(any(test, feature = "reference"))]
    pub(crate) fn set_bridged(&mut self, ride: RideType, set: HashSet<u32>) {
        self.bridged[slot(ride)] = set;
    }

    /// Forme canonique en octets (finition 2, §20.4) : les trois ensembles d'arêtes comblées, triés, petit-boutiste.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for set in &self.bridged {
            let mut v: Vec<u32> = set.iter().copied().collect();
            v.sort_unstable();
            out.extend((v.len() as u64).to_le_bytes());
            for e in v {
                out.extend(e.to_le_bytes());
            }
        }
        out
    }

    /// Octets tenus : une table de hachage de `u32` coûte 5 octets par alvéole (valeur et octet de contrôle) et ses
    /// alvéoles sont les 8/7 de sa capacité.
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.bridged.iter().map(|s| s.capacity() * 8 / 7 * 5 + 16).sum::<usize>()
    }
}

/// Incidence admise des **nœuds chauds** des cellules `cells`, pour les deux admissions à la fois (un seul balayage :
/// route, puis gravel et VTT) : `(nœud, arête)` triés et dédoublonnés, sens ignorés (une boucle compte une fois),
/// plus les arêtes plus larges qu'une cellule que ces cellules ne possèdent pas (§20.2).
fn incidence_hot<B: AsRef<[u8]>>(
    g: &GraphFile<B>,
    gi: &GraphIndex,
    cells: &[usize],
    hot: Option<&Hot<'_>>,
) -> [Vec<(u32, u32)>; 2] {
    let mut lists: [Vec<(u32, u32)>; 2] = [Vec::new(), Vec::new()];
    let add = |lists: &mut [Vec<(u32, u32)>; 2], e: u32, attrs| {
        let adm = [admitted(Admission::Road, attrs), admitted(Admission::Gravel, attrs)];
        if !adm[0] && !adm[1] {
            return;
        }
        let (a, b) = g.edge_ends(e);
        let ha = is_hot(hot, g, a);
        let hb = a != b && is_hot(hot, g, b);
        for (list, ok) in lists.iter_mut().zip(adm) {
            if ok {
                if ha {
                    list.push((a, e));
                }
                if hb {
                    list.push((b, e));
                }
            }
        }
    };
    let mut scanned = HashSet::new();
    for &ci in cells {
        scanned.insert(ci);
        let info = *g.cell_info(ci);
        let cv = g.cell(ci);
        for k in 0..info.n_edges {
            add(&mut lists, info.first_edge + k, cv.owned(k as usize).attrs);
        }
    }
    for &e in &gi.wide {
        if !scanned.contains(&g.edge_cell(e)) {
            add(&mut lists, e, g.edge(e).attrs);
        }
    }
    for l in &mut lists {
        l.sort_unstable();
        l.dedup();
    }
    lists
}

fn build_tables<B: AsRef<[u8]>>(
    g: &GraphFile<B>,
    gi: &GraphIndex,
    o: &Overlay,
    cfg: &Config,
    cells: &[usize],
    hot: Option<&Hot<'_>>,
) -> Result<[HashSet<u32>; 3], TeamCheckEscape> {
    let types = TypeIndex::of(o);
    let incs = incidence_hot(g, gi, cells, hot);
    let mut out: [HashSet<u32>; 3] = Default::default();
    for ride in [RideType::Road, RideType::Gravel, RideType::Mtb] {
        let inc = &incs[usize::from(Admission::of(ride) == Admission::Gravel)];
        out[slot(ride)] = bridged_edges(g, o, &types, cfg, ride, inc, hot)?;
    }
    Ok(out)
}

/// Les arêtes comblées en strict, recalculées d'après la surcouche et la règle d'admission (note lot 2 §2.3) :
/// (1) le plus court chemin non roulé de `hole_m` au plus, d'un cul-de-sac fictif (un seul bout d'arête d'équipe, au
/// moins deux arêtes admises) au plus proche nœud d'équipe ; (2) tout chemin non roulé de `connector_m` au plus entre
/// deux nœuds d'équipe. Un parcours ne passe que par ces arêtes hors équipe. Une marche qui développe un nœud non
/// chaud lève [`TeamCheckEscape`] au lieu de lire une incidence incomplète.
fn bridged_edges<B: AsRef<[u8]>>(
    g: &GraphFile<B>,
    o: &Overlay,
    types: &TypeIndex,
    cfg: &Config,
    ride: RideType,
    inc: &[(u32, u32)],
    hot: Option<&Hot<'_>>,
) -> Result<HashSet<u32>, TeamCheckEscape> {
    let mut out = HashSet::new();
    let mut team: HashSet<u32> = HashSet::new();
    let mut tdeg: HashMap<u32, u8> = HashMap::new();
    for (e, counts) in o.iter() {
        if team_edge(ride, g.edge(e).attrs, Some(counts), types) {
            team.insert(e);
            let (a, b) = g.edge_ends(e);
            for n in [a, b] {
                let d = tdeg.entry(n).or_insert(0);
                *d = d.saturating_add(1);
            }
        }
    }
    let len_dm = |e: u32| g.edge(e).len_dm;
    // Arêtes admises du nœud `n`, ou l'échappement s'il n'est pas chaud.
    let at = |n: u32| -> Result<std::ops::Range<usize>, TeamCheckEscape> {
        if !is_hot(hot, g, n) {
            return Err(TeamCheckEscape);
        }
        Ok(inc.partition_point(|x| x.0 < n)..inc.partition_point(|x| x.0 <= n))
    };
    // Degré admis d'un nœud, une boucle comptant deux fois.
    let adm_deg = |n: u32| -> Result<u32, TeamCheckEscape> {
        Ok(inc[at(n)?]
            .iter()
            .map(|x| {
                let (a, b) = g.edge_ends(x.1);
                1 + u32::from(a == b)
            })
            .sum())
    };
    // Dijkstra borné sur les arêtes non roulées ; `all` : tous les nœuds d'équipe atteints, sinon le premier.
    let walk = |s: u32, limit_dm: u32, all: bool, out: &mut HashSet<u32>| -> Result<(), TeamCheckEscape> {
        let mut dist: HashMap<u32, (u32, u32)> = HashMap::new();
        let mut heap = BinaryHeap::new();
        dist.insert(s, (0, u32::MAX));
        heap.push(Reverse((0u32, s)));
        while let Some(Reverse((d, x))) = heap.pop() {
            if dist[&x].0 < d {
                continue;
            }
            if x != s && tdeg.get(&x).is_some_and(|&t| t >= 1) {
                let mut cur = x;
                while cur != s {
                    let (_, pe) = dist[&cur];
                    out.insert(pe);
                    let (a, b) = g.edge_ends(pe);
                    cur = if a == cur { b } else { a };
                }
                if all {
                    continue;
                }
                break;
            }
            for &(_, be) in &inc[at(x)?] {
                if team.contains(&be) {
                    continue;
                }
                let nd = d + len_dm(be);
                if nd > limit_dm {
                    continue;
                }
                let (a, b) = g.edge_ends(be);
                let y = if a == x { b } else { a };
                if dist.get(&y).is_none_or(|&(od, _)| nd < od) {
                    dist.insert(y, (nd, be));
                    heap.push(Reverse((nd, y)));
                }
            }
        }
        Ok(())
    };
    let hole_dm = (cfg.hole_m * 10.0) as u32;
    let conn_dm = (cfg.connector_m * 10.0) as u32;
    let mut nodes: Vec<u32> = tdeg.keys().copied().collect();
    nodes.sort_unstable();
    for &n in &nodes {
        if tdeg[&n] == 1 && adm_deg(n)? >= 2 {
            walk(n, hole_dm, false, &mut out)?;
        }
    }
    for &n in &nodes {
        if inc[at(n)?].iter().any(|&(_, e)| !team.contains(&e) && len_dm(e) <= conn_dm) {
            walk(n, conn_dm, true, &mut out)?;
        }
    }
    Ok(out)
}
