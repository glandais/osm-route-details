//! Le moteur : le graphe de base et ses deux vues (`road` corrigée, `gravel`) relus **à plat** une fois pour toutes
//! (tableaux de nœuds, d'arêtes de vue, chaînes d'arêtes de base, caps), plus ce qui ne dépend ni de la requête ni de
//! l'équipe : composantes connexes, incidence des arêtes de base vivantes, index d'accroche paresseux.
//!
//! Écart à la note (§7) : la recherche lit ces tableaux à plat et non les cellules (`mmap`) : 36 ms par arbre avec le
//! lecteur par cellules, 18 ms à plat (note §4). Le fichier reste le format d'échange ; la mémoire du moteur est
//! de l'ordre de 150 Mo pour la région.

use std::fmt;
use std::sync::OnceLock;

use ord_graph::attrs::Attrs;
use ord_graph::format::{GraphFile, DIR_BIT};
use ord_graph::view::{heading_to_u16, polyline_heading, BaseEdges, ViewKind};

use crate::cost::{self, RideType};
use crate::geo::{haversine_m, point_segment, polyline_m, LatLon, Local, M_PER_DEG};

/// Constantes Java reprises telles quelles (note §6) et réglages du moteur.
#[derive(Clone, Debug)]
pub struct Params {
    /// Virage refusé à partir de cet angle, aux nœuds de degré ≥ 3.
    pub max_turn_deg: f64,
    /// Queue maximale (part de D) et part recouverte maximale.
    pub tail_share: f64,
    pub max_crossings: u32,
    pub tolerance: f64,
    /// Accroche : distance maximale du point à l'arête.
    pub snap_m: f64,
    /// Trou comblé en strict : longueur maximale (m).
    pub hole_m: f64,
    /// Jonction courte comblée en strict : chemin non roulé de cette longueur au plus entre deux nœuds d'équipe (m).
    pub connector_m: f64,
    /// Rapport retour / aller initial des demi-tours.
    pub rho0: f64,
    /// Budget de recherches ciblées, en multiple des états fixés de l'arbre aller.
    pub astar_budget: u32,
    /// Queue permise au retour : part de D de l'aller que le retour peut reprendre (au coût × 2).
    pub tail_reuse_share: f64,
    /// Composante minimale d'accroche : part de D.
    pub min_component_share: f64,
    /// Points de demi-tour essayés au plus par secteur de 30° (le ρ du candidat corrige la cible à chaque essai).
    pub tries_per_sector: usize,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            max_turn_deg: 150.0,
            tail_share: 0.15,
            max_crossings: 2,
            tolerance: 0.10,
            snap_m: 2000.0,
            hole_m: 100.0,
            connector_m: 30.0,
            rho0: 1.09,
            astar_budget: 6,
            tail_reuse_share: 0.15,
            min_component_share: 0.9,
            tries_per_sector: 6,
        }
    }
}

#[derive(Debug)]
pub enum EngineError {
    /// Le fichier n'est pas une vue, ou pas du bon genre.
    BadView(String),
    /// La vue ne vient pas de ce graphe de base.
    ViewMismatch,
    /// La surcouche vient d'un autre graphe.
    OverlayMismatch,
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadView(s) => write!(f, "vue inutilisable : {s}"),
            Self::ViewMismatch => write!(f, "la vue ne correspond pas à ce graphe de base"),
            Self::OverlayMismatch => write!(f, "la surcouche vient d'un autre graphe"),
        }
    }
}

impl std::error::Error for EngineError {}

/// Les arêtes et les nœuds du graphe de base, à plat.
#[derive(Debug)]
pub(crate) struct BaseData {
    pub ends: Vec<(u32, u32)>,
    pub attrs: Vec<Attrs>,
    pub len_dm: Vec<u32>,
    pub way: Vec<u64>,
    pub pos: Vec<(i32, i32)>,
}

fn node_positions<B: AsRef<[u8]>>(g: &GraphFile<B>) -> Vec<(i32, i32)> {
    let mut pos = Vec::with_capacity(g.header().nodes as usize);
    for ci in 0..g.n_cells() {
        let cv = g.cell(ci);
        for local in 0..cv.info().n_nodes as usize {
            pos.push(cv.node(local));
        }
    }
    pos
}

impl BaseData {
    fn load<B: AsRef<[u8]>>(g: &GraphFile<B>) -> Self {
        let be = BaseEdges::load(g);
        let way = (0..be.ends.len() as u32).map(|e| g.identity(e).0).collect();
        Self { ends: be.ends, attrs: be.attrs, len_dm: be.len_dm, way, pos: node_positions(g) }
    }
}

/// Index d'accroche : grille de 0,01° ; chaque arête de vue est rangée dans les cases que sa polyligne traverse.
#[derive(Debug)]
struct SnapGrid {
    keys: Vec<u32>,
    chains: Vec<u32>,
}

const CELL_E7: i32 = 100_000;

fn cell_key(lat: i32, lon: i32) -> u32 {
    let (ilat, ilon) = (lat.div_euclid(CELL_E7) + 9_000, lon.div_euclid(CELL_E7) + 18_000);
    (ilat as u32) * 36_000 + ilon as u32
}

/// Une vue, à plat.
#[derive(Debug)]
pub(crate) struct ViewData {
    pub kind: ViewKind,
    pub n_nodes: usize,
    pub n_chains: usize,
    pub pos: Vec<(i32, i32)>,
    pub ends: Vec<(u32, u32)>,
    pub len_dm: Vec<u32>,
    /// Bit 0 : sens `a → b` ouvert ; bit 1 : sens `b → a`.
    pub open: Vec<u8>,
    pub out_off: Vec<u32>,
    pub out_f: Vec<u32>,
    pub in_off: Vec<u32>,
    pub in_f: Vec<u32>,
    pub chain_off: Vec<u32>,
    pub chain_links: Vec<u32>,
    pub head: Vec<[u16; 2]>,
    pub deg: Vec<u8>,
    /// Poids sans équipe par type servi par la vue, indexé par `RideType::index` (vide si non servi).
    pub w_osm: [Vec<u32>; 3],
    /// Composante fortement connexe (sens respectés, virages ignorés) par nœud, et longueur (dm) par composante.
    pub comp: Vec<u32>,
    pub comp_len_dm: Vec<u64>,
    /// Arêtes de base vivantes autour de chaque nœud de base (CSR), et chaîne de chaque arête de base vivante.
    pub binc_off: Vec<u32>,
    pub binc: Vec<u32>,
    pub edge_chain: Vec<u32>,
    grid: OnceLock<SnapGrid>,
}

pub(crate) const NO_CHAIN: u32 = u32::MAX;

impl ViewData {
    #[inline]
    pub fn tgt(&self, f: u32) -> u32 {
        let e = self.ends[(f >> 1) as usize];
        if f & 1 == 0 {
            e.1
        } else {
            e.0
        }
    }

    #[inline]
    pub fn src(&self, f: u32) -> u32 {
        let e = self.ends[(f >> 1) as usize];
        if f & 1 == 0 {
            e.0
        } else {
            e.1
        }
    }

    #[inline]
    pub fn out(&self, v: u32) -> &[u32] {
        &self.out_f[self.out_off[v as usize] as usize..self.out_off[v as usize + 1] as usize]
    }

    #[inline]
    pub fn inn(&self, v: u32) -> &[u32] {
        &self.in_f[self.in_off[v as usize] as usize..self.in_off[v as usize + 1] as usize]
    }

    /// Arêtes de base de la chaîne, de `a` vers `b` ; bit 31 : parcourue de `b` vers `a`.
    #[inline]
    pub fn links(&self, c: u32) -> &[u32] {
        &self.chain_links[self.chain_off[c as usize] as usize..self.chain_off[c as usize + 1] as usize]
    }

    /// Arêtes de base vivantes autour d'un nœud de base.
    #[inline]
    pub fn base_inc(&self, v: u32) -> &[u32] {
        &self.binc[self.binc_off[v as usize] as usize..self.binc_off[v as usize + 1] as usize]
    }

    /// Longueur de la composante fortement connexe de la chaîne (0 si ses bouts sont dans deux composantes).
    pub fn comp_len_of_chain(&self, c: u32) -> u64 {
        chain_strong_len(&self.comp, &self.comp_len_dm, self.ends[c as usize])
    }

    /// Liens d'un sens de parcours : `(arête de base, parcourue à l'envers)` dans l'ordre du parcours.
    pub fn oriented_links(&self, f: u32) -> Vec<(u32, bool)> {
        let l = self.links(f >> 1);
        if f & 1 == 0 {
            l.iter().map(|&x| (x & !DIR_BIT, x & DIR_BIT != 0)).collect()
        } else {
            l.iter().rev().map(|&x| (x & !DIR_BIT, x & DIR_BIT == 0)).collect()
        }
    }
}

/// Composantes fortement connexes (Tarjan itératif) d'un graphe de `n` nœuds dont `succ(x, out)` remplit les
/// successeurs ; rend l'indice de composante de chaque nœud. Sens respectés, virages ignorés : une accroche dans une
/// poche orientée (rue à sens unique roulée une fois, qui ne mène nulle part) n'est pas dans la grande composante,
/// alors qu'elle y est sens ignorés (campagne du lot 2, `ord-loops-lot2-mesures.md`).
pub(crate) fn strong_components(n: usize, succ: &dyn Fn(u32, &mut Vec<u32>)) -> Vec<u32> {
    const UNSEEN: u32 = u32::MAX;
    let mut index = vec![UNSEEN; n];
    let mut low = vec![0u32; n];
    let mut on_stack = vec![false; n];
    let mut comp = vec![UNSEEN; n];
    let mut stack: Vec<u32> = Vec::new();
    // pile d'appels : (nœud, successeurs, position dans les successeurs)
    let mut call: Vec<(u32, Vec<u32>, usize)> = Vec::new();
    let (mut next_index, mut next_comp) = (0u32, 0u32);
    for root in 0..n as u32 {
        if index[root as usize] != UNSEEN {
            continue;
        }
        let mut s = Vec::new();
        succ(root, &mut s);
        index[root as usize] = next_index;
        low[root as usize] = next_index;
        next_index += 1;
        stack.push(root);
        on_stack[root as usize] = true;
        call.push((root, s, 0));
        while let Some(top) = call.last_mut() {
            let x = top.0;
            if top.2 < top.1.len() {
                let y = top.1[top.2];
                top.2 += 1;
                if index[y as usize] == UNSEEN {
                    let mut s = Vec::new();
                    succ(y, &mut s);
                    index[y as usize] = next_index;
                    low[y as usize] = next_index;
                    next_index += 1;
                    stack.push(y);
                    on_stack[y as usize] = true;
                    call.push((y, s, 0));
                } else if on_stack[y as usize] {
                    low[x as usize] = low[x as usize].min(index[y as usize]);
                }
                continue;
            }
            call.pop();
            if let Some(parent) = call.last() {
                let p = parent.0 as usize;
                low[p] = low[p].min(low[x as usize]);
            }
            if low[x as usize] == index[x as usize] {
                loop {
                    let y = stack.pop().expect("pile de Tarjan non vide");
                    on_stack[y as usize] = false;
                    comp[y as usize] = next_comp;
                    if y == x {
                        break;
                    }
                }
                next_comp += 1;
            }
        }
    }
    comp
}

/// Longueur (dm) de chaque composante fortement connexe : les chaînes gardées par `keep` dont les deux bouts y sont.
pub(crate) fn strong_lengths(
    comp: &[u32],
    ends: &[(u32, u32)],
    len_dm: &[u32],
    keep: &dyn Fn(usize) -> bool,
) -> Vec<u64> {
    let mut out = vec![0u64; comp.len()];
    for (c, &(a, b)) in ends.iter().enumerate() {
        if keep(c) && comp[a as usize] == comp[b as usize] {
            out[comp[a as usize] as usize] += u64::from(len_dm[c]);
        }
    }
    out
}

/// Longueur (dm) de la composante d'une chaîne, 0 si ses deux bouts ne sont pas dans la même.
pub(crate) fn chain_strong_len(comp: &[u32], len: &[u64], ends: (u32, u32)) -> u64 {
    let (ca, cb) = (comp[ends.0 as usize], comp[ends.1 as usize]);
    if ca == cb {
        len[ca as usize]
    } else {
        0
    }
}

fn load_view<B: AsRef<[u8]>>(
    base: &GraphFile<B>,
    bd: &BaseData,
    view: &GraphFile<B>,
    kind: ViewKind,
    rides: &[RideType],
) -> Result<ViewData, EngineError> {
    let meta = view.view_meta().ok_or_else(|| EngineError::BadView("pas une vue".into()))?;
    let h = base.header();
    if (meta.base_nodes, meta.base_edges, meta.base_pbf_xxh3, meta.base_build_epoch)
        != (h.nodes, h.edges, h.pbf_xxh3, h.build_epoch)
    {
        return Err(EngineError::ViewMismatch);
    }
    if meta.kind != kind || meta.prune_dead_ends {
        return Err(EngineError::BadView(format!(
            "attendu une vue `{}` non élaguée, reçu `{}`",
            kind.name(),
            meta.name
        )));
    }
    let ve = BaseEdges::load(view);
    let (n_nodes, n_chains) = (view.header().nodes as usize, view.header().edges as usize);
    let pos = node_positions(view);
    let open: Vec<u8> = ve.attrs.iter().map(|a| u8::from(a.fwd_ok()) | (u8::from(a.bwd_ok()) << 1)).collect();

    // Chaînes.
    let mut chain_off = Vec::with_capacity(n_chains + 1);
    let mut chain_links: Vec<u32> = Vec::new();
    chain_off.push(0u32);
    for c in 0..n_chains as u32 {
        let l = view.view_chain(c).ok_or_else(|| EngineError::BadView("chaînes absentes".into()))?;
        chain_links.extend(l.iter().map(|x| x.get()));
        chain_off.push(chain_links.len() as u32);
    }

    // Caps : section stockée, sinon recalcul depuis la géométrie (vue de format minor 1).
    let head: Vec<[u16; 2]> = match view.view_headings() {
        Some(h) if h.len() == n_chains => h,
        _ => (0..n_chains as u32)
            .map(|c| {
                let mut pts = vec![pos[ve.ends[c as usize].0 as usize]];
                pts.extend(view.geometry(c));
                pts.push(pos[ve.ends[c as usize].1 as usize]);
                [heading_to_u16(polyline_heading(&pts, true)), heading_to_u16(polyline_heading(&pts, false))]
            })
            .collect(),
    };

    // CSR sortantes et entrantes.
    let (mut out_off, mut in_off) = (vec![0u32; n_nodes + 1], vec![0u32; n_nodes + 1]);
    let mut deg = vec![0u8; n_nodes];
    for (c, &(a, b)) in ve.ends.iter().enumerate() {
        deg[a as usize] = deg[a as usize].saturating_add(1);
        deg[b as usize] = deg[b as usize].saturating_add(1);
        if open[c] & 1 != 0 {
            out_off[a as usize + 1] += 1;
            in_off[b as usize + 1] += 1;
        }
        if open[c] & 2 != 0 {
            out_off[b as usize + 1] += 1;
            in_off[a as usize + 1] += 1;
        }
    }
    for v in 0..n_nodes {
        out_off[v + 1] += out_off[v];
        in_off[v + 1] += in_off[v];
    }
    let (mut out_f, mut in_f) = (vec![0u32; out_off[n_nodes] as usize], vec![0u32; in_off[n_nodes] as usize]);
    let (mut of, mut inf) = (out_off.clone(), in_off.clone());
    for (c, &(a, b)) in ve.ends.iter().enumerate() {
        let c2 = (c as u32) << 1;
        if open[c] & 1 != 0 {
            out_f[of[a as usize] as usize] = c2;
            of[a as usize] += 1;
            in_f[inf[b as usize] as usize] = c2;
            inf[b as usize] += 1;
        }
        if open[c] & 2 != 0 {
            out_f[of[b as usize] as usize] = c2 | 1;
            of[b as usize] += 1;
            in_f[inf[a as usize] as usize] = c2 | 1;
            inf[a as usize] += 1;
        }
    }

    // Composantes fortement connexes (sens respectés).
    let comp = strong_components(n_nodes, &|x, s: &mut Vec<u32>| {
        let ends = &ve.ends;
        let f_tgt = |f: u32| {
            let e = ends[(f >> 1) as usize];
            if f & 1 == 0 {
                e.1
            } else {
                e.0
            }
        };
        s.extend(
            out_f[out_off[x as usize] as usize..out_off[x as usize + 1] as usize].iter().map(|&f| f_tgt(f)),
        );
    });
    let comp_len_dm = strong_lengths(&comp, &ve.ends, &ve.len_dm, &|_| true);

    // Incidence des arêtes de base vivantes.
    let n_base_nodes = bd.pos.len();
    let mut edge_chain = vec![NO_CHAIN; bd.ends.len()];
    for c in 0..n_chains {
        for l in &chain_links[chain_off[c] as usize..chain_off[c + 1] as usize] {
            edge_chain[(l & !DIR_BIT) as usize] = c as u32;
        }
    }
    let mut binc_off = vec![0u32; n_base_nodes + 1];
    for (e, &(a, b)) in bd.ends.iter().enumerate() {
        if edge_chain[e] != NO_CHAIN {
            binc_off[a as usize + 1] += 1;
            binc_off[b as usize + 1] += 1;
        }
    }
    for v in 0..n_base_nodes {
        binc_off[v + 1] += binc_off[v];
    }
    let mut binc = vec![0u32; binc_off[n_base_nodes] as usize];
    let mut fill = binc_off.clone();
    for (e, &(a, b)) in bd.ends.iter().enumerate() {
        if edge_chain[e] != NO_CHAIN {
            for v in [a, b] {
                binc[fill[v as usize] as usize] = e as u32;
                fill[v as usize] += 1;
            }
        }
    }

    // Poids sans équipe par type.
    let mut w_osm: [Vec<u32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for &r in rides {
        let w: Vec<u32> = (0..n_chains)
            .map(|c| {
                let mut s = 0u64;
                for l in &chain_links[chain_off[c] as usize..chain_off[c + 1] as usize] {
                    let e = (l & !DIR_BIT) as usize;
                    s += u64::from(cost::edge_weight(r, bd.attrs[e], bd.len_dm[e]));
                }
                s.min(u64::from(u32::MAX / 8)) as u32
            })
            .collect();
        w_osm[r.index()] = w;
    }

    Ok(ViewData {
        kind,
        n_nodes,
        n_chains,
        pos,
        ends: ve.ends,
        len_dm: ve.len_dm,
        open,
        out_off,
        out_f,
        in_off,
        in_f,
        chain_off,
        chain_links,
        head,
        deg,
        w_osm,
        comp,
        comp_len_dm,
        binc_off,
        binc,
        edge_chain,
        grid: OnceLock::new(),
    })
}

/// Résultat d'une accroche : arête de base la plus proche, à `offset_m` de son extrémité `a`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Snap {
    pub chain: u32,
    pub link: usize,
    pub edge: u32,
    pub rev: bool,
    pub offset_m: f64,
    pub dist_m: f64,
}

/// Le moteur : immuable, partageable entre fils (un `Workspace` par fil).
#[derive(Debug)]
pub struct Engine<B> {
    pub(crate) base: GraphFile<B>,
    pub(crate) views_files: [GraphFile<B>; 2],
    pub(crate) bd: BaseData,
    pub(crate) views: [ViewData; 2],
    pub(crate) params: Params,
}

impl<B: AsRef<[u8]>> Engine<B> {
    /// Relit le graphe de base et ses deux vues non élaguées : `road` (corrigée, lot 2) et `gravel`.
    pub fn new(
        base: GraphFile<B>,
        road: GraphFile<B>,
        gravel: GraphFile<B>,
        params: Params,
    ) -> Result<Self, EngineError> {
        if base.is_view() {
            return Err(EngineError::BadView("le graphe de base est une vue".into()));
        }
        let bd = BaseData::load(&base);
        let r = load_view(&base, &bd, &road, ViewKind::Road, &[RideType::Road])?;
        let g = load_view(&base, &bd, &gravel, ViewKind::Gravel, &[RideType::Gravel, RideType::Mtb])?;
        Ok(Self { base, views_files: [road, gravel], bd, views: [r, g], params })
    }

    /// Construit tout de suite les index paresseux (accroche) des deux vues : à appeler une fois après `new` pour que
    /// la première génération ne paie pas leur construction (~0,3 s par vue sur la région).
    pub fn warm(&self) {
        for vi in 0..2 {
            let _ = self.grid(vi);
        }
    }

    pub fn params(&self) -> &Params {
        &self.params
    }

    pub fn base(&self) -> &GraphFile<B> {
        &self.base
    }

    pub(crate) fn view(&self, ride: RideType) -> &ViewData {
        &self.views[ride.view_index()]
    }

    /// Longueur de la géométrie d'une arête de base, en mètres (haversine).
    pub(crate) fn geom_len_m(&self, e: u32) -> f64 {
        polyline_m(&self.edge_polyline(e))
    }

    /// Polyligne complète d'une arête de base, de `a` vers `b`.
    pub(crate) fn edge_polyline(&self, e: u32) -> Vec<(i32, i32)> {
        let (a, b) = self.bd.ends[e as usize];
        let mut pts = vec![self.bd.pos[a as usize]];
        pts.extend(self.base.geometry(e));
        pts.push(self.bd.pos[b as usize]);
        pts
    }

    fn grid(&self, vi: usize) -> &SnapGrid {
        let v = &self.views[vi];
        v.grid.get_or_init(|| {
            let mut entries: Vec<(u32, u32)> = Vec::new();
            for c in 0..v.n_chains as u32 {
                let mut pts = vec![v.pos[v.ends[c as usize].0 as usize]];
                pts.extend(self.views_files[vi].geometry(c));
                pts.push(v.pos[v.ends[c as usize].1 as usize]);
                let mut last = u32::MAX;
                for w in pts.windows(2) {
                    let n = (haversine_m(w[0], w[1]) / 400.0).ceil().max(1.0) as i32;
                    for k in 0..=n {
                        let t = f64::from(k) / f64::from(n);
                        let lat = w[0].0 + (f64::from(w[1].0 - w[0].0) * t) as i32;
                        let lon = w[0].1 + (f64::from(w[1].1 - w[0].1) * t) as i32;
                        let key = cell_key(lat, lon);
                        if key != last {
                            entries.push((key, c));
                            last = key;
                        }
                    }
                }
            }
            entries.sort_unstable();
            entries.dedup();
            SnapGrid {
                keys: entries.iter().map(|e| e.0).collect(),
                chains: entries.iter().map(|e| e.1).collect(),
            }
        })
    }

    /// Accroche `p` sur l'arête de base la plus proche (≤ `snap_m`) parmi celles dont la chaîne est acceptée
    /// par `ok` et dont la composante fait au moins `min_comp_dm` ; égalité : plus petit id de chaîne.
    pub(crate) fn snap(
        &self,
        ride: RideType,
        p: LatLon,
        ok: &dyn Fn(u32) -> bool,
        min_comp_dm: u64,
    ) -> Option<Snap> {
        let vi = ride.view_index();
        let v = &self.views[vi];
        let grid = self.grid(vi);
        let pe = p.to_e7();
        let loc = Local::new(pe);
        let dlat = (self.params.snap_m / (M_PER_DEG * 1e-7)) as i32 + 1;
        let dlon = (dlat as f64 / (f64::from(pe.0) * 1e-7).to_radians().cos().max(0.05)) as i32 + 1;
        let mut cand: Vec<u32> = Vec::new();
        let (ilat0, ilat1) = ((pe.0 - dlat).div_euclid(CELL_E7), (pe.0 + dlat).div_euclid(CELL_E7));
        let (ilon0, ilon1) = ((pe.1 - dlon).div_euclid(CELL_E7), (pe.1 + dlon).div_euclid(CELL_E7));
        for ilat in ilat0..=ilat1 {
            for ilon in ilon0..=ilon1 {
                let key = cell_key(ilat * CELL_E7, ilon * CELL_E7);
                let lo = grid.keys.partition_point(|&k| k < key);
                let hi = grid.keys.partition_point(|&k| k <= key);
                cand.extend_from_slice(&grid.chains[lo..hi]);
            }
        }
        cand.sort_unstable();
        cand.dedup();
        let pxy = (0.0, 0.0);
        // Jamais à moins d'un mètre d'un bout d'arête : pas de morceau de longueur nulle au départ ni à l'arrivée.
        let margin: f64 = 1.0;
        let mut best: Option<Snap> = None;
        for c in cand {
            if !ok(c) || v.comp_len_of_chain(c) < min_comp_dm {
                continue;
            }
            for (li, &l) in v.links(c).iter().enumerate() {
                let (e, rev) = (l & !DIR_BIT, l & DIR_BIT != 0);
                let pts = self.edge_polyline(e);
                let xy: Vec<(f64, f64)> = pts.iter().map(|&q| loc.xy(q)).collect();
                let mut acc = 0.0;
                let total: f64 = pts.windows(2).map(|w| haversine_m(w[0], w[1])).sum();
                let mut seg_best: Option<(f64, f64)> = None; // (distance, abscisse naturelle)
                for (i, w) in xy.windows(2).enumerate() {
                    let seg_len = haversine_m(pts[i], pts[i + 1]);
                    let (d, t) = point_segment(pxy, w[0], w[1]);
                    if seg_best.is_none_or(|(bd, _)| d < bd) {
                        seg_best = Some((d, acc + t * seg_len));
                    }
                    acc += seg_len;
                }
                let Some((d, off)) = seg_best else { continue };
                if d > self.params.snap_m {
                    continue;
                }
                let better = match best {
                    None => true,
                    Some(b) => d < b.dist_m - 1e-9 || ((d - b.dist_m).abs() <= 1e-9 && c < b.chain),
                };
                if better {
                    best = Some(Snap {
                        chain: c,
                        link: li,
                        edge: e,
                        rev,
                        offset_m: off.clamp(margin.min(total / 2.0), total - margin.min(total / 2.0)),
                        dist_m: d,
                    });
                }
            }
        }
        best
    }
}
