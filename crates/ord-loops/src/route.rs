//! Calcul d'itinéraire de A à B (lot 1 du routage, plan Pédalons `2026-11-23-itineraire-velo-osm.md`) : un A* sur
//! les arêtes de vue orientées, du morceau partiel de départ au morceau partiel d'arrivée, avec **les poids du
//! générateur** ([`Weighter`] : classe, revêtement, accès, goût, équipe, roulabilité) et sa règle de virage.
//!
//! Rien de la génération n'y passe et la génération n'appelle rien d'ici : elle rend les mêmes parcours qu'avant.
//!
//! - **Zone** : une ellipse de foyers départ et arrivée, de somme `reach_m` ([`RouteRequest::reach_m`]) ; l'appelant
//!   bâtit la fenêtre de [`Engine::plan_route`] et rejoue avec le pas suivant si rien n'est trouvé.
//! - **Heuristique** : vol d'oiseau jusqu'à l'arrivée × plus petit coût par décimètre des chaînes de la fenêtre. Elle
//!   minore le coût restant (les virages coûtent ≥ 0) ; seul le morceau partiel d'arrivée, pesé arête par arête, peut
//!   passer sous le minimum des chaînes quand sa chaîne mêle des arêtes de poids différents : écart borné par ce morceau.
//! - **Virages** : le coût du générateur ([`turn_cost`]) sans son refus. Un virage de 150° et plus à un carrefour
//!   coûte le maximum (200 à 250 m équivalents) au lieu d'être interdit : la génération peut toujours dessiner une
//!   autre boucle, l'itinéraire doit relier deux points donnés, et un refus lui faisait faire un détour de plusieurs
//!   fois la distance pour éviter un virage qu'on prend à vélo en ralentissant. Le demi-tour sur la même voie n'est
//!   permis qu'au bout d'une impasse. [`Routed::sharp_relaxed`] dit si l'itinéraire rendu en contient un.

use std::cell::Cell;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

use ord_graph::format::GraphFile;

use crate::cost::RideType;
use crate::engine::{Engine, Snap, ViewData};
use crate::generate::steps_of;
use crate::geo::{haversine_m, LatLon};
use crate::search::{
    angle16, in_head_ext, make_anchor, out_head, retro_ok, turn_cost, Anchor, Ctx, Disc, Forbidden, Stopped,
    Tree, Weighter, Workspace, INF, NONE, POLL, TURN_COST_MAX, TURN_FREE16, TURN_MAX16,
};
use crate::team::{TeamCtx, TeamData, WindowTeam};
use crate::verify::{slice, trim_piece};
use crate::window::{Window, WindowPlan};
use crate::{Family, Figures, Mode, Position, Request, Route};

/// Une demande d'itinéraire.
#[derive(Clone, Copy, Debug)]
pub struct RouteRequest {
    pub start: LatLon,
    pub end: LatLon,
    pub ride_type: RideType,
    /// [`Mode::Discovery`] sans équipe ; [`Mode::TeamFirst`] préfère les routes de l'équipe ; [`Mode::Strict`] ne prend
    /// qu'elles (un tronçon qui en sort n'a alors pas d'itinéraire).
    pub mode: Mode,
    /// Comme [`Request::familiarity`] et [`Request::off_team_cost_q8`].
    pub familiarity: f32,
    pub off_team_cost_q8: u32,
    /// Multiplicateur du poids par voie, en 256es : les six voies du goût ([`crate::taste_way`] : 0 grande route,
    /// 1 `tertiary`, 2 petite route, 3 rue, 4 voie cyclable ou verte, 5 chemin), `secondary` ayant sa case à part (6)
    /// au lieu de la 0. [`WAY_NEUTRAL`] : les poids du générateur, au bit près.
    pub way_q8: WayQ8,
}

/// Aucune préférence de voie : 256 partout.
pub const WAY_NEUTRAL: WayQ8 = [256; WAYS];

/// Nombre de cases d'une préférence de voie.
pub const WAYS: usize = 7;
/// Case de `secondary` (et `secondary_link`) : voie 0 du goût, tenue à part pour pouvoir l'éviter seule.
pub const WAY_SECONDARY: usize = 6;
/// Une préférence de voie : un multiplicateur en 256es par case.
pub type WayQ8 = [u16; WAYS];

/// Case d'une arête de base dans une préférence de voie.
fn way_index(a: ord_graph::attrs::Attrs) -> usize {
    if a.class() == Some(ord_graph::attrs::HighwayClass::Secondary) {
        WAY_SECONDARY
    } else {
        crate::cost::taste_way(a)
    }
}

/// Évite les grandes routes sans les interdire : `secondary` coûte deux fois plus, les autres grandes routes
/// (`primary`, `trunk`) une fois et demie, par-dessus la préférence `q` ; borné à 1 024. Sans la pénalité, le goût
/// commun met `secondary` au prix d'une rue (0,77 × 88/64 en route) : la doubler la passe derrière toute petite route,
/// et `primary` reste derrière elle. Une grande route reste prise quand rien d'autre ne passe. **Estimation**, comme
/// [`way_factors`].
pub fn avoid_main_roads(mut q: WayQ8) -> WayQ8 {
    q[WAY_SECONDARY] = (u32::from(q[WAY_SECONDARY]) * 2).min(1024) as u16;
    q[0] = (u32::from(q[0]) * 3 / 2).min(1024) as u16;
    q
}

/// Les multiplicateurs par voie d'un réglage « routes ou pistes » `use_roads` dans [0 ; 1] (0 : pistes, 1 : routes),
/// rapporté au réglage `neutral` du profil, pour lequel tout vaut 256.
///
/// Transposé du `road_factor` de Valhalla (`bicyclecost.cc`) : `f(u) = 1,5 − u` si `u ≥ 0,5`, sinon `2 − 2u` ;
/// `g = f(use_roads) / f(neutral)` ; le multiplicateur d'une voie vaut `g^α`, avec α = 1 (grande route), 0,6
/// (`tertiary`), 0,4 (petite route), 0,3 (rue), −0,5 (voie cyclable ou verte) et, pour le chemin, 0 en route et −0,5
/// en gravel et VTT. Borné à [64 ; 1 024] comme une table de goût. **Estimation** : les α ne sont calés sur aucune
/// mesure (note `ord-route-lot1.md`, §8).
pub fn way_factors(ride: RideType, use_roads: f64, neutral: f64) -> WayQ8 {
    let f = |u: f64| {
        let u = u.clamp(0.0, 1.0);
        if u >= 0.5 {
            1.5 - u
        } else {
            2.0 - 2.0 * u
        }
    };
    let g = f(use_roads) / f(neutral);
    let path = if ride == RideType::Road { 0.0 } else { -0.5 };
    // `secondary` (dernière case) suit les autres grandes routes.
    [1.0, 0.6, 0.4, 0.3, -0.5, path, 1.0].map(|a: f64| (g.powf(a) * 256.0).round().clamp(64.0, 1024.0) as u16)
}

/// Les multiplicateurs par voie d'une requête, ramenés à la chaîne : la moyenne, pondérée par la longueur, de ceux de
/// ses arêtes de base. Calculée à la première lecture de chaque chaîne, puis gardée pour la requête.
struct WayScale<'a, B> {
    e: &'a Engine<B>,
    v: &'a ViewData,
    q8: WayQ8,
    /// 0 : pas encore calculé.
    memo: Vec<Cell<u16>>,
}

impl<B: AsRef<[u8]>> WayScale<'_, B> {
    fn of(&self, c: u32) -> u64 {
        let slot = &self.memo[c as usize];
        if slot.get() == 0 {
            let (mut num, mut den) = (0u64, 0u64);
            for &l in self.v.links(c) {
                let (a, len) = self.e.attrs_len(l & !ord_graph::format::DIR_BIT);
                num += u64::from(len) * u64::from(self.q8[way_index(a)]);
                den += u64::from(len);
            }
            let m = (num + den / 2).checked_div(den).unwrap_or(256);
            slot.set(m.clamp(1, u64::from(u16::MAX)) as u16);
        }
        u64::from(slot.get())
    }

    /// `w × M(c) / 256`, arrondi, jamais nul ; `INF` reste `INF`.
    fn apply(&self, w: u32, c: u32) -> u32 {
        if w == INF {
            return INF;
        }
        ((u64::from(w) * self.of(c) + 128) >> 8).clamp(1, u64::from(u32::MAX / 2)) as u32
    }
}

/// Poids d'une chaîne pour la requête : celui du générateur, puis la préférence de voie s'il y en a une.
fn chain_w<B: AsRef<[u8]>>(w: &Weighter<'_>, scale: Option<&WayScale<'_, B>>, c: u32) -> u32 {
    match scale {
        None => w.chain(c),
        Some(s) => s.apply(w.chain(c), c),
    }
}

/// Réglages du calcul d'itinéraire ; `Default` = les valeurs du service.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteTune {
    /// Accroche : distance maximale du point à la voie (m). Plus courte que celle d'une génération : un point de passage
    /// est posé sur la carte, pas « dans le coin ».
    pub snap_m: f64,
    /// Vol d'oiseau maximal (m) : au-delà, [`RouteFailure::TooLong`].
    pub crow_max_m: f64,
    /// Zones successives : la somme des distances aux foyers vaut `max(facteur × vol d'oiseau, vol d'oiseau + marge)`.
    pub reach: [(f64, f64); 2],
    /// Composante minimale d'accroche : cette part du vol d'oiseau, plafonnée (m).
    pub min_component_share: f64,
    pub min_component_cap_m: f64,
}

impl Default for RouteTune {
    fn default() -> Self {
        Self {
            snap_m: 500.0,
            crow_max_m: 100_000.0,
            reach: [(1.5, 4_000.0), (2.5, 15_000.0)],
            min_component_share: 0.9,
            min_component_cap_m: 5_000.0,
        }
    }
}

impl RouteRequest {
    /// Vol d'oiseau du départ à l'arrivée (m).
    pub fn crow_m(&self) -> f64 {
        haversine_m(self.start.to_e7(), self.end.to_e7())
    }

    /// Somme des distances aux foyers de la zone du pas `step` (0 ou 1) ; `None` au-delà du dernier pas.
    pub fn reach_m(&self, tune: &RouteTune, step: usize) -> Option<f64> {
        let (factor, extra) = *tune.reach.get(step)?;
        let crow = self.crow_m();
        Some((factor * crow).max(crow + extra))
    }

    /// La requête de génération qui a la même fenêtre : un aller simple de distance `reach_m`.
    fn window_request(&self, reach_m: f64) -> Request {
        Request {
            start: self.start,
            end: Some(self.end),
            bearing: None,
            distance_m: reach_m,
            ride_type: self.ride_type,
            mode: self.mode,
            familiarity: self.familiarity,
            off_team_cost_q8: self.off_team_cost_q8,
            seed: 0,
            figures: Figures::Classic,
        }
    }
}

/// Pourquoi aucun itinéraire n'est rendu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteFailure {
    /// Aucune voie roulable par le type à moins de [`RouteTune::snap_m`] du départ, ou de l'arrivée.
    StartTooFar,
    EndTooFar,
    /// Vol d'oiseau au-delà de [`RouteTune::crow_max_m`].
    TooLong,
    /// Aucun chemin dans la zone, virages serrés permis compris : l'appelant rejoue avec la zone suivante.
    NoRoute,
    /// Mode strict ou équipe d'abord sans surcouche.
    NoTeamData,
    /// Coupé par l'appelant.
    Stopped,
    /// La recherche a touché le bord de la fenêtre : l'appelant rejoue dans une fenêtre plus grande.
    WindowEscape,
}

/// Un itinéraire et ce que la recherche a coûté.
#[derive(Clone, Debug)]
pub struct Routed {
    /// Suite d'arêtes de base orientées, comme un parcours généré (`family` : [`Family::Turnaround`], sans objet) ;
    /// `length_m`, `team_m`, `bridged_m`, `unpaved_m`, `offroad_m` et `presumed_surface_m` sont mesurés.
    pub route: Route,
    /// Coût du chemin (décimètres pondérés, virages compris).
    pub cost: u32,
    /// États fixés par les recherches.
    pub settled: u64,
    /// L'itinéraire contient un virage de 150° et plus à un carrefour, ou un demi-tour au bout d'une impasse.
    pub sharp_relaxed: bool,
    pub start_snap_m: f64,
    pub end_snap_m: f64,
}

impl<B: AsRef<[u8]>> Engine<B> {
    /// Les tuiles dont l'itinéraire a besoin pour une zone de somme `reach_m`, et la classe de fenêtre qui les contient :
    /// celles d'un aller simple de cette distance ([`Engine::plan_with`]).
    pub fn plan_route(&self, r: &RouteRequest, reach_m: f64, radius_step_m: f64, extra_m: f64) -> WindowPlan {
        self.plan_with(&r.window_request(reach_m), radius_step_m, extra_m)
    }
}

/// Coût de virage de l'itinéraire : jamais de refus pour l'angle (le maximum à la place) ; le demi-tour sur la même
/// chaîne seulement au bout d'une impasse.
fn turn_relaxed(
    v: &ViewData,
    fe: u32,
    pred: u32,
    fo: u32,
    hin_over: Option<u16>,
    hout_over: Option<u16>,
) -> Option<u32> {
    if fo == fe ^ 1 {
        return (v.deg[v.tgt(fe) as usize] <= 1).then_some(TURN_COST_MAX + 500);
    }
    let hin = hin_over.unwrap_or_else(|| in_head_ext(v, fe, pred));
    let hout = hout_over.unwrap_or_else(|| out_head(v, fo));
    let a = angle16(hin, hout);
    Some(if a > TURN_FREE16 {
        (((a - TURN_FREE16) as u64 * u64::from(TURN_COST_MAX) / u64::from(TURN_MAX16 - TURN_FREE16)) as u32)
            .min(TURN_COST_MAX + 500)
    } else {
        0
    })
}

/// Plus petit coût par décimètre des chaînes ouvertes de la fenêtre, à peine minoré.
fn min_factor<B: AsRef<[u8]>>(v: &ViewData, w: &Weighter<'_>, scale: Option<&WayScale<'_, B>>) -> f64 {
    let mut m = f64::INFINITY;
    for c in 0..v.n_chains as u32 {
        let l = v.len_dm[c as usize];
        if l == 0 {
            continue;
        }
        let wc = chain_w(w, scale, c);
        if wc != INF {
            m = m.min(f64::from(wc) / f64::from(l));
        }
    }
    if m.is_finite() {
        0.999 * m
    } else {
        0.0
    }
}

struct Found {
    fields: Vec<u32>,
    cost: u32,
}

/// A* du départ à l'arrivée. Les états d'arrivée ont leurs propres cases (`2 n_chains + sens`) : la chaîne d'arrivée
/// peut aussi être celle du départ, ou être traversée en entier avant d'y revenir.
#[allow(clippy::too_many_arguments)]
fn search<B: AsRef<[u8]>>(
    cx: &Ctx<'_>,
    scale: Option<&WayScale<'_, B>>,
    ta: &mut Tree,
    end_xy: (f64, f64),
    f_min: f64,
    settled: &mut u64,
    stop: &mut dyn FnMut() -> bool,
) -> Result<Option<Found>, Stopped> {
    let v = cx.v;
    let terminal0 = 2 * v.n_chains as u32;
    ta.begin(2 * v.n_chains + 2, v.n_nodes);
    let end_chain = cx.e.snap.chain;
    let h = |node: u32| -> u32 {
        let (x, y) = cx.disc.xy_of(v.pos[node as usize]);
        let crow_dm = (x - end_xy.0).hypot(y - end_xy.1) * 10.0;
        (crow_dm * f_min).floor().min(f64::from(u32::MAX / 4)) as u32
    };
    let mut heap: BinaryHeap<Reverse<(u32, u32, u32)>> = BinaryHeap::new();
    for dir in 0..2u32 {
        let c = cx.s.out_cost[dir as usize];
        if c != INF {
            let f = (cx.s.snap.chain << 1) | dir;
            ta.set(f, c, NONE, cx.s.out_len[dir as usize]);
            heap.push(Reverse((c.saturating_add(h(v.tgt(f))), c, f)));
        }
    }
    let mut polls = 0u32;
    while let Some(Reverse((_, g, f))) = heap.pop() {
        if g > ta.get(f) {
            continue;
        }
        *settled += 1;
        polls += 1;
        if polls.is_multiple_of(POLL) && stop() {
            return Err(Stopped);
        }
        if f >= terminal0 {
            let last = (end_chain << 1) | (f - terminal0);
            let mut fields = vec![last];
            let mut cur = ta.pred(f);
            while cur != NONE {
                fields.push(cur);
                cur = ta.pred(cur);
            }
            fields.reverse();
            return Ok(Some(Found { fields, cost: g }));
        }
        let node = v.tgt(f);
        if !v.complete[node as usize] {
            cx.esc.set(true);
            return Err(Stopped);
        }
        let (pred, m) = (ta.pred(f), ta.meters(f));
        let hin_over = (pred == NONE).then(|| cx.s.out_head[(f & 1) as usize]);
        for &fo in v.out(node) {
            let c = fo >> 1;
            let turn = |hout_over: Option<u16>| turn_relaxed(v, f, pred, fo, hin_over, hout_over);
            if c == end_chain {
                let dir = (fo & 1) as usize;
                let w = cx.e.in_cost[dir];
                if w != INF {
                    if let Some(tc) = turn(Some(cx.e.in_head[dir])) {
                        let ng = g.saturating_add(w).saturating_add(tc);
                        let t = terminal0 + dir as u32;
                        if ng < ta.get(t) {
                            ta.set(t, ng, f, m + cx.e.in_len[dir]);
                            heap.push(Reverse((ng, ng, t)));
                        }
                    }
                }
            }
            let w = chain_w(cx.w, scale, c);
            if w == INF {
                continue;
            }
            let tn = v.tgt(fo);
            if !cx.disc.inside(v.pos[tn as usize]) {
                continue;
            }
            let Some(tc) = turn(None) else { continue };
            let ng = g.saturating_add(w).saturating_add(tc);
            if ng < ta.get(fo) {
                ta.set(fo, ng, f, m + v.len_dm[c as usize]);
                heap.push(Reverse((ng.saturating_add(h(tn)), ng, fo)));
            }
        }
    }
    Ok(None)
}

/// L'itinéraire contient-il un virage que la règle du générateur aurait refusé (150° et plus à un carrefour, demi-tour
/// au bout d'une impasse) ?
fn has_sharp(cx: &Ctx<'_>, fields: &[u32]) -> bool {
    let v = cx.v;
    (1..fields.len()).any(|i| {
        let (fe, fo) = (fields[i - 1], fields[i]);
        let pred = if i >= 2 { fields[i - 2] } else { NONE };
        let hin_over = (i == 1).then(|| cx.s.out_head[(fe & 1) as usize]);
        let hout_over = (i == fields.len() - 1).then(|| cx.e.in_head[(fo & 1) as usize]);
        turn_cost(v, cx.forb, fe, pred, fo, hin_over, hout_over).is_none() || !retro_ok(v, pred, fe, fo)
    })
}

/// Le morceau direct quand départ et arrivée sont sur la même chaîne et que l'arrivée est devant le départ dans un sens
/// ouvert : `(état, coût)`.
fn direct(s: &Anchor, e: &Anchor) -> Option<(u32, u32)> {
    if s.snap.chain != e.snap.chain {
        return None;
    }
    (0..2usize)
        .filter(|&dir| {
            s.out_cost[dir] != INF
                && e.in_cost[dir] != INF
                && e.out_cost[dir] != INF
                && s.out_len[dir] >= e.out_len[dir]
        })
        .map(|dir| ((s.snap.chain << 1) | dir as u32, s.out_cost[dir].saturating_sub(e.out_cost[dir])))
        .min_by_key(|&(f, c)| (c, f))
}

/// Mesure un itinéraire : longueurs totale, d'équipe, comblée, non revêtue.
fn measure<B: AsRef<[u8]>>(e: &Engine<B>, team: Option<&TeamData>, ride: RideType, route: &mut Route) {
    let (mut len, mut team_m, mut bridged, mut unpaved, mut offroad, mut presumed) =
        (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for i in 0..route.steps.len() {
        let st = route.steps[i];
        let (pts, _, _, whole) = trim_piece(route, i, e.edge_polyline(st.base_edge));
        let l = if whole {
            f64::from(e.len_dm(st.base_edge)) / 10.0
        } else {
            pts.windows(2).map(|w| haversine_m(w[0], w[1])).sum()
        };
        let a = e.attrs(st.base_edge);
        len += l;
        if a.is_unpaved() {
            unpaved += l;
        }
        if a.offroad() {
            offroad += l;
        }
        if crate::cost::presumed_surface(a) {
            presumed += l;
        }
        if let Some(t) = team {
            if t.is_team_edge(ride, st.base_edge) {
                team_m += l;
            } else if t.is_bridged_edge(ride, st.base_edge) {
                bridged += l;
            }
        }
    }
    route.length_m = len;
    route.team_m = team_m;
    route.bridged_m = bridged;
    route.unpaved_m = unpaved;
    route.offroad_m = offroad;
    route.presumed_surface_m = presumed;
}

/// Calcule l'itinéraire dans une fenêtre qui contient les tuiles de [`Engine::plan_route`] pour `reach_m`.
#[allow(clippy::too_many_arguments)]
pub fn route_in<B: AsRef<[u8]>>(
    e: &Engine<B>,
    win: &Window,
    team: Option<TeamCtx<'_>>,
    r: &RouteRequest,
    tune: &RouteTune,
    reach_m: f64,
    ws: &mut Workspace,
    stop: &mut dyn FnMut() -> bool,
) -> Result<Routed, RouteFailure> {
    if r.mode != Mode::Discovery && team.is_none() {
        return Err(RouteFailure::NoTeamData);
    }
    let crow = r.crow_m();
    if crow > tune.crow_max_m || reach_m > e.params.max_distance_m {
        return Err(RouteFailure::TooLong);
    }
    let ride = r.ride_type;
    let vi = ride.view_index();
    assert_eq!(win.vi, vi, "la fenêtre n'est pas celle de la vue du type demandé");
    let v = &win.v;
    let w = Weighter::new(v, team, ride, r.mode, r.familiarity, r.off_team_cost_q8);
    let min_dm = ((tune.min_component_share * crow).min(tune.min_component_cap_m) * 10.0) as u64;
    let ok = |c: u32| w.chain(c) != INF;
    let snap = |p: LatLon, far: RouteFailure| -> Result<Snap, RouteFailure> {
        e.snap(win, p, &ok, min_dm)
            .map_err(|()| RouteFailure::WindowEscape)?
            .filter(|s| s.dist_m <= tune.snap_m)
            .ok_or(far)
    };
    let s_snap = snap(r.start, RouteFailure::StartTooFar)?;
    let e_snap = snap(r.end, RouteFailure::EndTooFar)?;
    let scale = (r.way_q8 != WAY_NEUTRAL).then(|| WayScale {
        e,
        v,
        q8: r.way_q8,
        memo: vec![Cell::new(0); v.n_chains],
    });
    let scale = scale.as_ref();
    // Les morceaux partiels suivent la préférence de leur chaîne (sa moyenne : un écart borné par le morceau).
    let anchor = |snap: Snap| {
        let mut a = make_anchor(e, v, &w, snap);
        if let Some(s) = scale {
            for cost in a.out_cost.iter_mut().chain(a.in_cost.iter_mut()) {
                *cost = s.apply(*cost, snap.chain);
            }
        }
        a
    };
    let sa = anchor(s_snap);
    let ea = anchor(e_snap);
    let disc = Disc::new(r.start.to_e7(), r.end.to_e7(), reach_m, false);
    let esc = Cell::new(false);
    let forb = Forbidden::default();
    let cx = Ctx { v, w: &w, disc: &disc, s: &sa, e: &ea, forb: &forb, esc: &esc, org: None };
    let mut settled = 0u64;
    let mut found: Option<(Found, bool)> =
        direct(&sa, &ea).map(|(f, cost)| (Found { fields: vec![f], cost }, false));
    if found.is_none() {
        let f_min = min_factor(v, &w, scale);
        // Position réelle de l'arrivée sur sa voie, dans le repère de la zone.
        let end_pt = slice(&e.edge_polyline(e_snap.edge), 0.0, e_snap.offset_m).last().copied();
        let end_xy = disc.xy_of(end_pt.unwrap_or_else(|| r.end.to_e7()));
        let ta = &mut ws.views[vi].ta;
        match search(&cx, scale, ta, end_xy, f_min, &mut settled, stop) {
            Ok(Some(f)) => {
                let sharp = has_sharp(&cx, &f.fields);
                found = Some((f, sharp));
            }
            Ok(None) => {}
            Err(Stopped) => {
                return Err(if esc.get() { RouteFailure::WindowEscape } else { RouteFailure::Stopped });
            }
        }
    }
    let Some((found, sharp_relaxed)) = found else { return Err(RouteFailure::NoRoute) };
    let (steps, _) = steps_of(v, &found.fields, &s_snap, &e_snap);
    let mut route = Route {
        start: Position { edge: s_snap.edge, offset_m: s_snap.offset_m },
        end: Position { edge: e_snap.edge, offset_m: e_snap.offset_m },
        steps,
        length_m: 0.0,
        team_m: 0.0,
        bridged_m: 0.0,
        shared_m: 0.0,
        crossings: 0,
        unpaved_m: 0.0,
        offroad_m: 0.0,
        presumed_surface_m: 0.0,
        bearing_deviation_deg: None,
        familiarity: r.familiarity,
        family: Family::Turnaround,
    };
    measure(e, team.map(|t| t.data), ride, &mut route);
    Ok(Routed {
        route,
        cost: found.cost,
        settled,
        sharp_relaxed,
        start_snap_m: s_snap.dist_m,
        end_snap_m: e_snap.dist_m,
    })
}

/// Calcule l'itinéraire en bâtissant la fenêtre de chaque zone (et les agrégats de l'équipe) : pour les tests et les
/// outils ; le service garde ses fenêtres en cache et appelle [`route_in`].
pub fn route<B: AsRef<[u8]>>(
    e: &Engine<B>,
    team: Option<&TeamData>,
    r: &RouteRequest,
    tune: &RouteTune,
    ws: &mut Workspace,
    stop: &mut dyn FnMut() -> bool,
) -> Result<Routed, RouteFailure> {
    let mut last = RouteFailure::NoRoute;
    for step in 0..tune.reach.len() {
        let reach_m = r.reach_m(tune, step).expect("pas dans la table");
        if reach_m > e.params.max_distance_m {
            return Err(RouteFailure::TooLong);
        }
        let mut extra_m = 0.0;
        for _ in 0..2 {
            let plan = e.plan_route(r, reach_m, 0.0, extra_m);
            let Some(win) = e.build_window(plan.view, &plan.need, stop) else {
                return Err(RouteFailure::Stopped);
            };
            let wt = team.map(|t| WindowTeam::build(e, &win, t));
            let ctx = team.zip(wt.as_ref()).map(|(data, win)| TeamCtx { data, win });
            match route_in(e, &win, ctx, r, tune, reach_m, ws, stop) {
                Err(RouteFailure::WindowEscape) => {
                    last = RouteFailure::WindowEscape;
                    extra_m = plan.snap_lmax_m + 10_000.0;
                }
                Err(RouteFailure::NoRoute) => {
                    last = RouteFailure::NoRoute;
                    break;
                }
                other => return other,
            }
        }
    }
    Err(last)
}

/// Géométrie d'un itinéraire et plages **non revêtues** (au sens route-details, [`ord_graph::attrs::Attrs::offroad`]) en
/// indices de cette géométrie : `[i, j]` couvre les points `i` à `j` compris ; deux arêtes non revêtues qui se suivent
/// font une seule plage.
pub fn route_geometry_spans<B: AsRef<[u8]>>(base: &GraphFile<B>, r: &Route) -> (Vec<LatLon>, Vec<[u32; 2]>) {
    let mut out: Vec<(i32, i32)> = Vec::new();
    let mut spans: Vec<[u32; 2]> = Vec::new();
    for (i, st) in r.steps.iter().enumerate() {
        let (a, b) = base.edge_ends(st.base_edge);
        let mut nat = vec![base.node_pos(a)];
        nat.extend(base.geometry(st.base_edge));
        nat.push(base.node_pos(b));
        let (pts, ..) = trim_piece(r, i, nat);
        let from = out.len().saturating_sub(1);
        for p in pts {
            if out.last() != Some(&p) {
                out.push(p);
            }
        }
        let to = out.len().saturating_sub(1);
        if to > from && base.edge(st.base_edge).attrs.offroad() {
            match spans.last_mut() {
                Some(last) if last[1] as usize >= from => last[1] = to as u32,
                _ => spans.push([from as u32, to as u32]),
            }
        }
    }
    (out.into_iter().map(LatLon::from_e7).collect(), spans)
}
