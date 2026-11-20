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
use rustc_hash::{FxHashMap, FxHashSet};

use crate::cost::{self, is_greenway, RideType, Taste, TASTE_CLASSES};
use crate::engine::{
    chain_strong_len, rides_of_view, strong_components, strong_lengths, Engine, EngineError,
};
use crate::geo::M_PER_DEG;
use crate::window::{tile_key_of, tiles_of_rect, Window};

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
    /// La construction a dû être refaite à `ZONE_BOX_M` après un échappement du garde-fou (§20.2) : à compter.
    pub(crate) retried: bool,
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

/// Ce qu'il faut construire pour une équipe : sa zone, ses cases chaudes (§20.2), et ce que la construction tiendra
/// en mémoire le temps qu'elle dure.
#[derive(Clone, Debug)]
pub struct BuildPlan {
    zone: Vec<u32>,
    hot: FxHashSet<u64>,
    /// Octets transitoires estimés de la construction.
    pub estimated_bytes: usize,
}

impl BuildPlan {
    /// Nombre de cases chaudes de la construction.
    pub fn hot_cells(&self) -> usize {
        self.hot.len()
    }
}

/// Octets transitoires par case chaude (§20.2) : incidence chaude (16 o par entrée), arêtes vivantes, tables de la
/// règle des trous et graphe strict, croissance des vecteurs comprise. Réglé sur les neuf équipes de la France au pic
/// anonyme mesuré (`ord-loops-lot6-mesures.md` §10) plus 25 %.
const BUILD_BYTES_PER_HOT_CASE: usize = 280;
/// Octets fixes de la construction (tables, tampons).
const BUILD_BYTES_FIXED: usize = 2_000_000;

/// Côté (E7) d'une case chaude : 0,002° (~220 m).
const HOT_CASE_E7: i32 = 20_000;
/// Marge (m) ajoutée à la plus grande des portées de la règle des trous : le Dijkstra ne développe pas un nœud plus
/// loin que `hole_m` d'un bout d'arête d'équipe, la boîte en prend de quoi couvrir l'arrondi des cases.
const HOT_MARGIN_M: f64 = 150.0;

/// Clé d'une case chaude.
fn case_key(p: (i32, i32)) -> u64 {
    (u64::from(p.0.div_euclid(HOT_CASE_E7) as u32) << 32) | u64::from(p.1.div_euclid(HOT_CASE_E7) as u32)
}

/// Les cases de 0,002° que touche la boîte de ± `hot_m` autour de **chaque bout** de chaque arête de la surcouche
/// dont un compte est non nul (§20.2).
fn hot_cells<B: AsRef<[u8]>>(e: &Engine<B>, ov: &Overlay, hot_m: f64) -> FxHashSet<u64> {
    let ky = M_PER_DEG * 1e-7;
    let dlat = (hot_m / ky) as i32 + 2;
    let mut cases: FxHashSet<u64> = FxHashSet::default();
    for (edge, c) in ov.iter() {
        if c.iter().all(|&x| x == 0) {
            continue;
        }
        let (a, b) = e.ends(edge);
        for n in [a, b] {
            let p = e.pos(n);
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

/// La portée chaude par défaut d'une équipe (m) : celle de la règle des trous la plus longue, plus la marge, sans
/// dépasser la boîte de la zone.
fn default_hot_m<B: AsRef<[u8]>>(e: &Engine<B>) -> f64 {
    (e.params.hole_m.max(e.params.connector_m) + HOT_MARGIN_M).min(ZONE_BOX_M)
}

impl TeamData {
    /// Construit les attributs d'équipe pour les trois types sur la zone de l'équipe ; refuse une surcouche d'un
    /// autre graphe.
    pub fn build<B: AsRef<[u8]>>(e: &Engine<B>, overlay: &Overlay) -> Result<Self, EngineError> {
        let plan = Self::plan_build(e, overlay)?;
        Self::build_planned(e, overlay, &plan)
    }

    /// La zone de l'équipe, ses cases chaudes et la mémoire transitoire de sa construction : l'appelant la réserve
    /// avant de construire (budget unique du service, §10.1).
    pub fn plan_build<B: AsRef<[u8]>>(e: &Engine<B>, overlay: &Overlay) -> Result<BuildPlan, EngineError> {
        overlay.check_graph(&e.base.graph_identity()).map_err(|_| EngineError::OverlayMismatch)?;
        let zone = zone_tiles(e, overlay);
        let hot_m = default_hot_m(e);
        let hot = hot_cells(e, overlay, hot_m);
        let estimated_bytes = BUILD_BYTES_FIXED + hot.len() * BUILD_BYTES_PER_HOT_CASE;
        Ok(BuildPlan { zone, hot, estimated_bytes })
    }

    /// Construit sur la zone et les cases chaudes d'un [`BuildPlan`]. Si le garde-fou lève (un Dijkstra développerait
    /// un nœud hors des cases chaudes), la construction est refaite **une** fois avec des cases à `ZONE_BOX_M` ;
    /// un second échappement rend [`EngineError::TeamZoneEscape`], jamais un résultat faux.
    pub fn build_planned<B: AsRef<[u8]>>(
        e: &Engine<B>,
        overlay: &Overlay,
        plan: &BuildPlan,
    ) -> Result<Self, EngineError> {
        match build_sparse(e, overlay, &plan.zone, &plan.hot) {
            Ok(t) => Ok(t),
            Err(Escape) => {
                let wide = hot_cells(e, overlay, ZONE_BOX_M);
                let mut t =
                    build_sparse(e, overlay, &plan.zone, &wide).map_err(|_| EngineError::TeamZoneEscape)?;
                t.retried = true;
                Ok(t)
            }
        }
    }

    /// Construit avec des cases chaudes de `hot_m` mètres, sans reprise : [`EngineError::TeamZoneEscape`] au premier
    /// échappement. Les tests forcent ainsi le garde-fou.
    #[cfg(test)]
    pub(crate) fn build_forced<B: AsRef<[u8]>>(
        e: &Engine<B>,
        overlay: &Overlay,
        hot_m: f64,
        retry: bool,
    ) -> Result<Self, EngineError> {
        let zone = zone_tiles(e, overlay);
        let hot = hot_cells(e, overlay, hot_m);
        match build_sparse(e, overlay, &zone, &hot) {
            Ok(t) => Ok(t),
            Err(Escape) if retry => {
                let wide = hot_cells(e, overlay, ZONE_BOX_M);
                let mut t =
                    build_sparse(e, overlay, &zone, &wide).map_err(|_| EngineError::TeamZoneEscape)?;
                t.retried = true;
                Ok(t)
            }
            Err(Escape) => Err(EngineError::TeamZoneEscape),
        }
    }

    /// Vrai si la construction a dû être refaite à `ZONE_BOX_M` (échappement du garde-fou ; doit rester rare).
    pub fn zone_retried(&self) -> bool {
        self.retried
    }

    /// Comme [`TeamData::build`], sur toutes les tuiles du graphe (`whole`) : le moteur à plat, référence des tests.
    #[cfg(any(test, feature = "reference"))]
    pub fn build_on<B: AsRef<[u8]>>(
        e: &Engine<B>,
        overlay: &Overlay,
        whole: bool,
    ) -> Result<Self, EngineError> {
        overlay.check_graph(&e.base.graph_identity()).map_err(|_| EngineError::OverlayMismatch)?;
        if whole {
            Self::build_reference_zone(e, overlay, None)
        } else {
            Self::build(e, overlay)
        }
    }

    /// La construction d'avant la finition 2 (fenêtres de toute la zone, incidence de toute la zone), gardée comme
    /// référence : mêmes octets canoniques que [`TeamData::build`], toujours.
    #[cfg(any(test, feature = "reference"))]
    pub fn build_reference<B: AsRef<[u8]>>(e: &Engine<B>, overlay: &Overlay) -> Result<Self, EngineError> {
        overlay.check_graph(&e.base.graph_identity()).map_err(|_| EngineError::OverlayMismatch)?;
        let zone = zone_tiles(e, overlay);
        Self::build_reference_zone(e, overlay, Some(&zone))
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
pub(crate) fn rough_surface(a: ord_graph::attrs::Attrs) -> bool {
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
    /// Sonde du goût (lot 4, §12) : poids sans équipe de chaque chaîne **au goût**, par type ; vide = pas de goût.
    w_taste: [Vec<u32>; 3],
    taste: Option<Box<Taste>>,
}

impl WindowTeam {
    /// Calcule les agrégats : les drapeaux de chaque lien viennent de [`TeamData`], par dichotomie.
    pub fn build<B: AsRef<[u8]>>(e: &Engine<B>, win: &Window, team: &TeamData) -> Self {
        Self::build_with_taste(e, win, team, None)
    }

    /// Comme [`WindowTeam::build`], avec un goût (sonde du lot 4, §12). Avec `Some(taste)`, pour **chaque** type servi
    /// par la vue (même si sa table est neutre : les tests y passent par l'arithmétique), le poids de chaque lien est
    /// `taste_weight(ride, attrs, len, &taste.q8[ride])` : `w_taste[ride][c]` est la somme sur tous les liens (bornée
    /// à `u32::MAX / 8` comme `w_osm`), et `a`, `b`, `z` de [`ChainTeam`] sont faits des **mêmes** poids au goût. Avec
    /// `None`, exactement le calcul sans goût. Le goût ne touche ni la [`Window`] ni la relaxation : il n'existe que
    /// dans ces agrégats, déjà propres à une équipe. Surcoût de temps : `attrs_len` de **tous** les liens (sans goût :
    /// ceux d'équipe ou comblés seulement).
    pub fn build_with_taste<B: AsRef<[u8]>>(
        e: &Engine<B>,
        win: &Window,
        team: &TeamData,
        taste: Option<&Taste>,
    ) -> Self {
        let v = &win.v;
        let mut per: [Vec<ChainTeam>; 3] = Default::default();
        let mut w_taste: [Vec<u32>; 3] = Default::default();
        for &ride in rides_of_view(win.vi) {
            let tt = team.of(ride);
            let q8 = taste.map(|t| &t.q8[ride.index()]);
            let mut chains = vec![ChainTeam::default(); v.n_chains];
            let mut wt: Vec<u32> = if q8.is_some() { Vec::with_capacity(v.n_chains) } else { Vec::new() };
            for (c, ct) in chains.iter_mut().enumerate() {
                let (mut a, mut b, mut zs, mut strict, mut all) = (0u64, 0u64, 0u64, true, 0u64);
                for l in v.links(c as u32) {
                    let ed = l & !DIR_BIT;
                    let (fl, z) = tt.flags(ed);
                    if fl & TEAM != 0 {
                        let (attrs, len) = e.attrs_len(ed);
                        let w = u64::from(match q8 {
                            None => e.base_weight(ride, attrs, len),
                            Some(q) => cost::taste_weight(ride, attrs, len, q),
                        });
                        a += w;
                        all += w;
                        zs += w * u64::from(z);
                    } else if fl & BRIDGED != 0 {
                        let (attrs, len) = e.attrs_len(ed);
                        let w = u64::from(match q8 {
                            None => e.base_weight(ride, attrs, len),
                            Some(q) => cost::taste_weight(ride, attrs, len, q),
                        });
                        b += w;
                        all += w;
                    } else {
                        strict = false;
                        if let Some(q) = q8 {
                            let (attrs, len) = e.attrs_len(ed);
                            all += u64::from(cost::taste_weight(ride, attrs, len, q));
                        }
                    }
                }
                *ct = ChainTeam {
                    a: a.min(u64::from(u32::MAX / 8)) as u32,
                    b: b.min(u64::from(u32::MAX / 8)) as u32,
                    z: (zs / a.max(1)).min(255) as u8,
                    strict,
                };
                if q8.is_some() {
                    wt.push(all.min(u64::from(u32::MAX / 8)) as u32);
                }
            }
            per[ride.index()] = chains;
            w_taste[ride.index()] = wt;
        }
        Self { per, w_taste, taste: taste.map(|t| Box::new(t.clone())) }
    }

    /// Octets que tiendront les agrégats de `win` (une `ChainTeam` par chaîne et par type de sortie de la vue) : de
    /// quoi les réserver **avant** de les construire.
    ///
    /// `with_taste` : les agrégats portent aussi un poids au goût par chaîne et par type (4 octets de plus), ce que fait
    /// la lecture d'un profil d'équipe (3b). Sans goût, le calcul d'avant.
    pub fn estimate_bytes(win: &Window, with_taste: bool) -> usize {
        let per = std::mem::size_of::<ChainTeam>() + if with_taste { std::mem::size_of::<u32>() } else { 0 };
        win.n_chains() * rides_of_view(win.vi).len() * per
    }

    pub(crate) fn chains_of(&self, ride: RideType) -> &[ChainTeam] {
        &self.per[ride.index()]
    }

    /// Octets tenus (capacités réelles).
    pub fn bytes(&self) -> usize {
        self.per.iter().map(|c| c.capacity() * std::mem::size_of::<ChainTeam>()).sum::<usize>()
            + self.w_taste.iter().map(|w| w.capacity() * std::mem::size_of::<u32>()).sum::<usize>()
    }

    /// `(poids au goût des chaînes, table du type)` si un goût a été donné et que la vue sert ce type.
    pub(crate) fn taste_of(&self, ride: RideType) -> Option<(&[u32], &[u16; TASTE_CLASSES])> {
        let t = self.taste.as_deref()?;
        let w = &self.w_taste[ride.index()];
        if w.is_empty() {
            return None;
        }
        Some((w, &t.q8[ride.index()]))
    }
}

/// Zone de l'équipe : les tuiles qui contiennent un point d'une arête roulée, et celles que touche la boîte de
/// ± `ZONE_BOX_M` autour de chacun de ces points (§7.1).
pub(crate) fn zone_tiles<B: AsRef<[u8]>>(e: &Engine<B>, ov: &Overlay) -> Vec<u32> {
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
pub(crate) struct Inc {
    pub node: u32,
    pub edge: u32,
    pub other: u32,
    pub len: u32,
}

/// Un Dijkstra de la règle des trous développerait un nœud hors des cases chaudes (§20.2) : la construction creuse
/// n'a pas son incidence.
pub(crate) struct Escape;

pub(crate) fn type_index(o: &Overlay, name: &str) -> Option<usize> {
    o.meta.types.iter().position(|t| t == name)
}

/// Cases chaudes et tuiles de la zone : un nœud de base est **chaud** si la case de sa position est chaude et si
/// sa tuile est une tuile de la zone.
struct Hot<'a> {
    cases: &'a FxHashSet<u64>,
    zone: &'a [u32],
    zoom: u8,
}

impl Hot<'_> {
    fn contains(&self, p: (i32, i32)) -> bool {
        self.cases.contains(&case_key(p)) && self.zone.binary_search(&tile_key_of(&p, self.zoom)).is_ok()
    }
}

/// Ce qu'il faut d'une vue pour la règle des trous, **autour du réseau de l'équipe seulement** (§20.2) : les arêtes
/// de base vivantes qui ont un bout chaud, et l'incidence des nœuds chauds, dans l'ordre croissant des arêtes.
struct SparseView<'a, B> {
    e: &'a Engine<B>,
    hot: Hot<'a>,
    alive: Vec<u32>,
    inc: Vec<Inc>,
}

impl<'a, B: AsRef<[u8]>> SparseView<'a, B> {
    /// Lit en flux les chaînes de la zone de la vue `vi`, comme `build_window_opts` les choisit, mais sans fenêtre.
    /// Rend aussi les chaînes candidates au réseau strict (tous les bouts de tous les liens sont chauds).
    fn read(e: &'a Engine<B>, vi: usize, zone: &'a [u32], cases: &'a FxHashSet<u64>) -> (Self, Vec<u32>) {
        let hot = Hot { cases, zone, zoom: e.zoom() };
        let view = &e.views_files[vi];
        let idx = &e.idx[vi];
        let mut alive_m: Vec<(u32, u8)> = Vec::new();
        let mut strict_cand: Vec<u32> = Vec::new();
        let mut chain = |c: u32| {
            let links = view.view_chain(c).expect("chaînes validées à l'ouverture");
            let mut all_hot = true;
            for l in links {
                let ed = l.get() & !DIR_BIT;
                let (a, b) = e.ends(ed);
                let (ha, hb) = (hot.contains(e.pos(a)), hot.contains(e.pos(b)));
                if ha || hb {
                    alive_m.push((ed, u8::from(ha) | (u8::from(hb) << 1)));
                }
                all_hot &= ha && hb;
            }
            if all_hot {
                strict_cand.push(c);
            }
        };
        for &key in zone {
            let (x, y) = ord_graph::grid::tile_of_key(key);
            if let Some(ci) = view.find_cell(x, y) {
                let i = view.cell_info(ci);
                for c in i.first_edge..i.first_edge + i.n_edges {
                    chain(c);
                }
            }
            for f in idx.foreign_of(key).iter() {
                chain(f.1);
            }
        }
        alive_m.sort_unstable();
        alive_m.dedup();
        strict_cand.sort_unstable();
        strict_cand.dedup();
        let mut inc: Vec<Inc> = Vec::with_capacity(alive_m.len() * 2);
        for &(ed, m) in &alive_m {
            let (a, b) = e.ends(ed);
            let len = e.len_dm(ed);
            if m & 1 != 0 {
                inc.push(Inc { node: a, edge: ed, other: b, len });
            }
            if m & 2 != 0 {
                inc.push(Inc { node: b, edge: ed, other: a, len });
            }
        }
        inc.sort_unstable_by_key(|i| (i.node, i.edge));
        let alive: Vec<u32> = alive_m.iter().map(|x| x.0).collect();
        (Self { e, hot, alive, inc }, strict_cand)
    }

    fn alive(&self, ed: u32) -> bool {
        self.alive.binary_search(&ed).is_ok()
    }

    /// Incidence du nœud `x` ; `Escape` s'il n'est pas chaud (son incidence serait incomplète).
    fn inc_of(&self, x: u32) -> Result<&[Inc], Escape> {
        if !self.hot.contains(self.e.pos(x)) {
            return Err(Escape);
        }
        let lo = self.inc.partition_point(|i| i.node < x);
        let hi = self.inc.partition_point(|i| i.node <= x);
        Ok(&self.inc[lo..hi])
    }
}

#[derive(Clone, Copy)]
struct EdgeInfo {
    fl: u8,
    n: u32,
    z: u8,
}

/// Construction creuse des trois types (§20.2) : une vue à la fois ; `road` sert `Road`, `gravel` sert `Gravel` et
/// `Mtb`.
fn build_sparse<B: AsRef<[u8]>>(
    e: &Engine<B>,
    ov: &Overlay,
    zone: &[u32],
    cases: &FxHashSet<u64>,
) -> Result<TeamData, Escape> {
    let mut per: [Option<TypeTeam>; 3] = [None, None, None];
    for vi in 0..2 {
        let (sp, strict_cand) = SparseView::read(e, vi, zone, cases);
        for r in RideType::ALL {
            if r.view_index() == vi {
                per[r.index()] = Some(build_type(e, ov, r, vi, &sp, &strict_cand)?);
            }
        }
    }
    Ok(TeamData { per: per.map(|t| t.expect("un type par vue")), retried: false })
}

fn build_type<B: AsRef<[u8]>>(
    e: &Engine<B>,
    ov: &Overlay,
    ride: RideType,
    vi: usize,
    sp: &SparseView<'_, B>,
    strict_cand: &[u32],
) -> Result<TypeTeam, Escape> {
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
        if !sp.alive(edge) {
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
    let mut starts: Vec<u32> = Vec::new();
    for (&x, &d) in &tdeg {
        if d == 1 && sp.inc_of(x)?.len() >= 2 {
            starts.push(x);
        }
    }
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
            for i in sp.inc_of(x)? {
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
        if !sp.inc_of(s)?.iter().any(|i| !is_team(i.edge) && i.len <= conn_dm) {
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
            for i in sp.inc_of(x)? {
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

    let strict_len = strict_components_of(e, vi, &edges, strict_cand);
    Ok(TypeTeam {
        edges,
        strict_len,
        team_edges,
        team_m: team_dm as f64 / 10.0,
        bridged_m: bridged_dm as f64 / 10.0,
        outside_m: (outside_dm.0 as f64 / 10.0, outside_dm.1 as f64 / 10.0),
    })
}

/// Composantes fortement connexes du réseau strict (sens respectés, virages ignorés), sur le **seul graphe des chaînes
/// strictes** (§20.2, point 4) : une chaîne est stricte si tous ses liens sont d'équipe ou comblés ; une composante
/// est une propriété du graphe, pas de l'ordre de parcours, et un nœud sans chaîne stricte n'entre dans aucune
/// longueur (§20.3, point 6). `(id global de chaîne stricte, longueur dm de sa composante)`, triés.
fn strict_components_of<B: AsRef<[u8]>>(
    e: &Engine<B>,
    vi: usize,
    edges: &[(u32, u8, u8)],
    strict_cand: &[u32],
) -> Vec<(u32, u64)> {
    let flagged = |ed: u32| match edges.binary_search_by_key(&ed, |x| x.0) {
        Ok(i) => edges[i].1 & (TEAM | BRIDGED) != 0,
        Err(_) => false,
    };
    let view = &e.views_files[vi];
    let (mut gid, mut ends_g, mut len_dm, mut open) = (Vec::new(), Vec::new(), Vec::new(), Vec::<u8>::new());
    for &c in strict_cand {
        let links = view.view_chain(c).expect("chaînes validées à l'ouverture");
        if !links.iter().all(|l| flagged(l.get() & !DIR_BIT)) {
            continue;
        }
        let ci = view.edge_cell(c);
        let first = view.cell_info(ci).first_edge;
        let o = view.cell(ci).owned((c - first) as usize);
        gid.push(c);
        ends_g.push(view.edge_ends(c));
        len_dm.push(o.len_dm);
        open.push(u8::from(o.attrs.fwd_ok()) | (u8::from(o.attrs.bwd_ok()) << 1));
    }
    let mut ngid: Vec<u32> = ends_g.iter().flat_map(|&(a, b): &(u32, u32)| [a, b]).collect();
    ngid.sort_unstable();
    ngid.dedup();
    let local = |g: u32| ngid.binary_search(&g).expect("bout d'une chaîne stricte") as u32;
    let ends: Vec<(u32, u32)> = ends_g.iter().map(|&(a, b)| (local(a), local(b))).collect();
    // Arcs dans l'ordre croissant des chaînes, sens ouverts comme `ViewData::out`.
    let mut arcs: Vec<(u32, u32)> = Vec::with_capacity(2 * ends.len());
    for (c, &(a, b)) in ends.iter().enumerate() {
        if open[c] & 1 != 0 {
            arcs.push((a, b));
        }
        if open[c] & 2 != 0 {
            arcs.push((b, a));
        }
    }
    arcs.sort_by_key(|x| x.0);
    let comp = strong_components(ngid.len(), &|x, s: &mut Vec<u32>| {
        let lo = arcs.partition_point(|a| a.0 < x);
        let hi = arcs.partition_point(|a| a.0 <= x);
        s.extend(arcs[lo..hi].iter().map(|a| a.1));
    });
    let lens = strong_lengths(&comp, &ends, &len_dm, &|_| true);
    let mut out: Vec<(u32, u64)> = Vec::new();
    for (k, &c) in gid.iter().enumerate() {
        let l = chain_strong_len(&comp, &lens, ends[k]);
        if l > 0 {
            out.push((c, l));
        }
    }
    out
}
