//! Le contrôle lui-même (note lot 2 §6, points 1 à 8). Les noms des violations sont ceux de `RouteGuarantees.java`
//! quand ils existent, en gardant le vocabulaire de la note (`NOT_ON_GRAPH`, `NOT_ADMITTED`, `OFF_TEAM`).

use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap, HashMap, HashSet};
use std::sync::OnceLock;

use ord_graph::attrs::Attrs;
use ord_graph::format::GraphFile;
use ord_graph::overlay::Overlay;
use ord_loops::{Mode, Request, RideType, Route};

use crate::geo::{
    across_m, along, clip, cumulative_m, dist, has_connector_run, haversine_m, path_heading, point_at,
    poly_len, turn_angle_deg, LatLon, Proj, RunRule,
};
use crate::rules::{admitted, direction_allowed, team_edge, Admission, TypeIndex};

/// Les garanties, par code. L'ordre sert au tri des rapports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Violation {
    Empty,
    NotOnGraph,
    Discontinuous,
    OneWay,
    NotAdmitted,
    WrongStart,
    WrongEnd,
    /// Position de départ ou d'arrivée hors de son arête, ou incohérente avec la suite (ajout au §6).
    BadOffset,
    OffTeam,
    UTurn,
    SharpTurn,
    DeadEnd,
    EdgeUsedMoreThanTwice,
    ReuseOutsideTail,
    TailTooLong,
    TooManyCrossings,
    CorridorDoubled,
    DistanceOutOfTolerance,
}

impl Violation {
    pub fn code(self) -> &'static str {
        match self {
            Self::Empty => "EMPTY",
            Self::NotOnGraph => "NOT_ON_GRAPH",
            Self::Discontinuous => "DISCONTINUOUS",
            Self::OneWay => "ONE_WAY",
            Self::NotAdmitted => "NOT_ADMITTED",
            Self::WrongStart => "WRONG_START",
            Self::WrongEnd => "WRONG_END",
            Self::BadOffset => "BAD_OFFSET",
            Self::OffTeam => "OFF_TEAM",
            Self::UTurn => "U_TURN",
            Self::SharpTurn => "SHARP_TURN",
            Self::DeadEnd => "DEAD_END",
            Self::EdgeUsedMoreThanTwice => "EDGE_USED_MORE_THAN_TWICE",
            Self::ReuseOutsideTail => "REUSE_OUTSIDE_TAIL",
            Self::TailTooLong => "TAIL_TOO_LONG",
            Self::TooManyCrossings => "TOO_MANY_CROSSINGS",
            Self::CorridorDoubled => "CORRIDOR_DOUBLED",
            Self::DistanceOutOfTolerance => "DISTANCE_OUT_OF_TOLERANCE",
        }
    }
}

/// Les constantes du §6 (celles de Java, plus l'accroche à 2 km et le trou de 100 m).
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub max_turn_deg: f64,
    pub head_near_m: f64,
    pub head_far_m: f64,
    pub cluster_m: f64,
    /// Bouts du parcours où la règle des grappes ne s'applique pas (Java `CLUSTER_FREE_ENDS_M`).
    pub cluster_free_ends_m: f64,
    pub tail_share: f64,
    pub max_crossings: u32,
    pub tolerance: f64,
    pub corridor_dist_m: f64,
    pub corridor_step_m: f64,
    pub corridor_gap_m: f64,
    pub corridor_end_ignore_m: f64,
    pub corridor_max_share: f64,
    pub snap_m: f64,
    pub hole_m: f64,
    /// Jonction courte comblée : chemin non roulé de cette longueur au plus entre deux nœuds d'équipe (§2.3).
    pub connector_m: f64,
    /// Deux morceaux d'une arête se recouvrent s'ils partagent plus de ce nombre de mètres.
    pub overlap_m: f64,
    pub hairpin_chord_m: f64,
    pub connector_min_m: f64,
    pub connector_tol_deg: f64,
    pub run_bend_deg: f64,
    pub run_min_piece_m: f64,
    /// Le départ et l'arrivée d'une boucle sont le même point à ce nombre de mètres près.
    pub loop_close_m: f64,
    /// L'exemption « épingle d'une même way » vaut aussi pour un nœud seul (l'apex d'une épingle de degré 3),
    /// comme `RouteGuarantees.turnApplies` exempte un nœud que l'équipe traverse ; sans elle, la note lue à la
    /// lettre ne l'exempte que pour une grappe de nœuds.
    pub hairpin_single_node: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_turn_deg: 150.0,
            head_near_m: 30.0,
            head_far_m: 50.0,
            cluster_m: 100.0,
            cluster_free_ends_m: 200.0,
            tail_share: 0.15,
            max_crossings: 2,
            tolerance: 0.10,
            corridor_dist_m: 25.0,
            corridor_step_m: 10.0,
            corridor_gap_m: 400.0,
            corridor_end_ignore_m: 30.0,
            corridor_max_share: 0.20,
            snap_m: 2000.0,
            hole_m: 100.0,
            connector_m: 30.0,
            overlap_m: 1.0,
            hairpin_chord_m: 30.0,
            connector_min_m: 15.0,
            connector_tol_deg: 25.0,
            run_bend_deg: 20.0,
            run_min_piece_m: 1.0,
            loop_close_m: 1.0,
            hairpin_single_node: false,
        }
    }
}

/// Ce que le contrôle a mesuré, qu'il y ait des violations ou non.
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// Violations, sans doublon, triées.
    pub violations: Vec<Violation>,
    pub length_m: f64,
    /// Longueur recouverte (arêtes prises deux fois, comptées une fois).
    pub shared_m: f64,
    pub crossings: u32,
    /// Longueur sur des arêtes d'équipe pour le type (0 sans surcouche).
    pub team_m: f64,
    /// Longueur hors équipe admise par la règle du trou (mode strict seulement).
    pub bridged_m: f64,
    /// Longueur hors équipe, quelle qu'en soit l'issue.
    pub off_team_m: f64,
    pub corridor_share: f64,
    pub start_zone_m: f64,
    pub end_zone_m: f64,
    pub pieces: usize,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.violations.is_empty()
    }

    pub fn has(&self, v: Violation) -> bool {
        self.violations.contains(&v)
    }
}

/// Une arête de base lue une fois : extrémités, attributs, géométrie complète `a → b`.
struct EdgeInfo {
    a: u32,
    b: u32,
    attrs: Attrs,
    /// `len_dm / 10`.
    len_m: f64,
    way: u64,
    pts: Vec<LatLon>,
    cum: Vec<f64>,
}

impl EdgeInfo {
    fn length_h(&self) -> f64 {
        *self.cum.last().unwrap_or(&0.0)
    }
}

/// Un morceau parcouru d'une arête, dans le sens de la marche.
struct Piece {
    edge: u32,
    rev: bool,
    len: f64,
    /// Intervalle d'abscisse (haversine, de `a` vers `b`) couvert.
    lo: f64,
    hi: f64,
    to: u32,
    way: u64,
    pts: Vec<LatLon>,
}

pub struct Checker<'a, B> {
    g: &'a GraphFile<B>,
    overlay: Option<&'a Overlay>,
    cfg: Config,
    types: TypeIndex,
    adm_deg: [OnceLock<Vec<u8>>; 2],
    team_deg: [OnceLock<Vec<u8>>; 3],
    /// Arêtes admises incidentes à chaque nœud (CSR : décalages, arêtes), par admission.
    incidence: [OnceLock<(Vec<u32>, Vec<u32>)>; 2],
    /// Arêtes comblées (note §2.3), recalculées depuis la surcouche, par type.
    bridged: [OnceLock<HashSet<u32>>; 3],
}

fn ride_slot(r: RideType) -> usize {
    match r {
        RideType::Road => 0,
        RideType::Gravel => 1,
        RideType::Mtb => 2,
    }
}

impl<B> std::fmt::Debug for Checker<'_, B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Checker")
            .field("cfg", &self.cfg)
            .field("has_overlay", &self.overlay.is_some())
            .finish_non_exhaustive()
    }
}

impl<'a, B: AsRef<[u8]>> Checker<'a, B> {
    pub fn new(graph: &'a GraphFile<B>, overlay: Option<&'a Overlay>, cfg: Config) -> Self {
        let types =
            overlay.map_or(TypeIndex { road: None, gravel: None, mtb: None, mixed: None }, TypeIndex::of);
        Self {
            g: graph,
            overlay,
            cfg,
            types,
            adm_deg: [OnceLock::new(), OnceLock::new()],
            team_deg: [OnceLock::new(), OnceLock::new(), OnceLock::new()],
            incidence: [OnceLock::new(), OnceLock::new()],
            bridged: [OnceLock::new(), OnceLock::new(), OnceLock::new()],
        }
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    fn edge_info(&self, e: u32) -> EdgeInfo {
        let ev = self.g.edge(e);
        let (a, b) = self.g.edge_ends(e);
        let e7 = |p: (i32, i32)| (f64::from(p.0) * 1e-7, f64::from(p.1) * 1e-7);
        let mut pts = vec![e7(self.g.node_pos(a))];
        pts.extend(self.g.geometry(e).into_iter().map(e7));
        pts.push(e7(self.g.node_pos(b)));
        let cum = cumulative_m(&pts);
        EdgeInfo {
            a,
            b,
            attrs: ev.attrs,
            len_m: f64::from(ev.len_dm) / 10.0,
            way: self.g.identity(e).0,
            pts,
            cum,
        }
    }

    /// Degré admis de chaque nœud : arêtes admises incidentes, sens ignorés (une boucle compte deux fois).
    fn admitted_degrees(&self, adm: Admission) -> &Vec<u8> {
        self.adm_deg[adm.index()].get_or_init(|| {
            let mut deg = vec![0u8; self.g.header().nodes as usize];
            for ci in 0..self.g.n_cells() {
                let info = *self.g.cell_info(ci);
                let view = self.g.cell(ci);
                for k in 0..info.n_edges {
                    if admitted(adm, view.owned(k as usize).attrs) {
                        let (a, b) = self.g.edge_ends(info.first_edge + k);
                        deg[a as usize] = deg[a as usize].saturating_add(1);
                        deg[b as usize] = deg[b as usize].saturating_add(1);
                    }
                }
            }
            deg
        })
    }

    /// Degré d'équipe de chaque nœud pour un type : arêtes d'équipe incidentes.
    fn team_degrees(&self, ride: RideType) -> &Vec<u8> {
        self.team_deg[ride_slot(ride)].get_or_init(|| {
            let mut deg = vec![0u8; self.g.header().nodes as usize];
            if let Some(o) = self.overlay {
                for (e, counts) in o.iter() {
                    if team_edge(ride, self.g.edge(e).attrs, Some(counts), &self.types) {
                        let (a, b) = self.g.edge_ends(e);
                        deg[a as usize] = deg[a as usize].saturating_add(1);
                        deg[b as usize] = deg[b as usize].saturating_add(1);
                    }
                }
            }
            deg
        })
    }

    /// Arêtes admises autour de chaque nœud, sens ignorés (une boucle compte une fois).
    fn incidence(&self, adm: Admission) -> &(Vec<u32>, Vec<u32>) {
        self.incidence[adm.index()].get_or_init(|| {
            let n_nodes = self.g.header().nodes as usize;
            let mut off = vec![0u32; n_nodes + 1];
            let mut list: Vec<(u32, u32)> = Vec::new();
            for ci in 0..self.g.n_cells() {
                let info = *self.g.cell_info(ci);
                let view = self.g.cell(ci);
                for k in 0..info.n_edges {
                    if admitted(adm, view.owned(k as usize).attrs) {
                        let e = info.first_edge + k;
                        let (a, b) = self.g.edge_ends(e);
                        list.push((a, e));
                        if a != b {
                            list.push((b, e));
                        }
                    }
                }
            }
            list.sort_unstable();
            for &(n, _) in &list {
                off[n as usize + 1] += 1;
            }
            for i in 0..n_nodes {
                off[i + 1] += off[i];
            }
            (off, list.into_iter().map(|x| x.1).collect())
        })
    }

    /// Les arêtes comblées en strict, recalculées d'après la surcouche et la règle d'admission, sans rien du moteur
    /// (note §2.3) : (1) le plus court chemin non roulé de `hole_m` au plus, d'un cul-de-sac fictif (un seul bout
    /// d'arête d'équipe, au moins deux arêtes admises) au plus proche nœud d'équipe ; (2) tout chemin non roulé de
    /// `connector_m` au plus entre deux nœuds d'équipe. Un parcours ne passe que par ces arêtes hors équipe.
    fn bridged_edges(&self, ride: RideType) -> &HashSet<u32> {
        self.bridged[ride_slot(ride)].get_or_init(|| {
            let mut out = HashSet::new();
            let Some(o) = self.overlay else { return out };
            let adm = Admission::of(ride);
            let (off, inc) = self.incidence(adm);
            let tdeg = self.team_degrees(ride);
            let adm_deg = self.admitted_degrees(adm);
            let mut team = vec![false; self.g.header().edges as usize];
            for (e, counts) in o.iter() {
                if team_edge(ride, self.g.edge(e).attrs, Some(counts), &self.types) {
                    team[e as usize] = true;
                }
            }
            let edges_at = |n: u32| &inc[off[n as usize] as usize..off[n as usize + 1] as usize];
            let len_dm = |e: u32| self.g.edge(e).len_dm;
            // Dijkstra borné sur les arêtes non roulées ; `all` : tous les nœuds d'équipe atteints, sinon le premier.
            let walk = |s: u32, limit_dm: u32, all: bool, out: &mut HashSet<u32>| {
                let mut dist: HashMap<u32, (u32, u32)> = HashMap::new();
                let mut heap = BinaryHeap::new();
                dist.insert(s, (0, u32::MAX));
                heap.push(Reverse((0u32, s)));
                while let Some(Reverse((d, x))) = heap.pop() {
                    if dist[&x].0 < d {
                        continue;
                    }
                    if x != s && tdeg[x as usize] >= 1 {
                        let mut cur = x;
                        while cur != s {
                            let (_, pe) = dist[&cur];
                            out.insert(pe);
                            let (a, b) = self.g.edge_ends(pe);
                            cur = if a == cur { b } else { a };
                        }
                        if all {
                            continue;
                        }
                        break;
                    }
                    for &be in edges_at(x) {
                        if team[be as usize] {
                            continue;
                        }
                        let nd = d + len_dm(be);
                        if nd > limit_dm {
                            continue;
                        }
                        let (a, b) = self.g.edge_ends(be);
                        let y = if a == x { b } else { a };
                        if dist.get(&y).is_none_or(|&(od, _)| nd < od) {
                            dist.insert(y, (nd, be));
                            heap.push(Reverse((nd, y)));
                        }
                    }
                }
            };
            let hole_dm = (self.cfg.hole_m * 10.0) as u32;
            let conn_dm = (self.cfg.connector_m * 10.0) as u32;
            for n in 0..self.g.header().nodes as u32 {
                if tdeg[n as usize] == 1 && adm_deg[n as usize] >= 2 {
                    walk(n, hole_dm, false, &mut out);
                }
            }
            for n in 0..self.g.header().nodes as u32 {
                if tdeg[n as usize] >= 1
                    && edges_at(n).iter().any(|&e| !team[e as usize] && len_dm(e) <= conn_dm)
                {
                    walk(n, conn_dm, true, &mut out);
                }
            }
            out
        })
    }

    fn is_team(&self, ride: RideType, e: u32, attrs: Attrs) -> bool {
        match self.overlay {
            Some(o) => team_edge(ride, attrs, o.get(e), &self.types),
            None => false,
        }
    }

    /// Contrôle un parcours rendu pour la requête `req`.
    pub fn check(&self, req: &Request, route: &Route) -> Report {
        let mut v: BTreeSet<Violation> = BTreeSet::new();
        let mut rep = Report::default();
        let n = route.steps.len();
        let finish = |mut rep: Report, v: BTreeSet<Violation>| {
            rep.violations = v.into_iter().collect();
            rep
        };
        if n == 0 {
            v.insert(Violation::Empty);
            return finish(rep, v);
        }
        // 1. Structure
        let n_edges = self.g.header().edges;
        let mut on_graph = route.steps.iter().map(|s| s.base_edge).chain([route.start.edge, route.end.edge]);
        if on_graph.any(|e| u64::from(e) >= n_edges) {
            v.insert(Violation::NotOnGraph);
            return finish(rep, v);
        }
        let mut infos: HashMap<u32, EdgeInfo> = HashMap::new();
        for e in route.steps.iter().map(|s| s.base_edge).chain([route.start.edge, route.end.edge]) {
            infos.entry(e).or_insert_with(|| self.edge_info(e));
        }
        let adm = Admission::of(req.ride_type);
        for s in &route.steps {
            let info = &infos[&s.base_edge];
            if !direction_allowed(info.attrs, !s.reverse) {
                v.insert(Violation::OneWay);
            }
            if !admitted(adm, info.attrs) {
                v.insert(Violation::NotAdmitted);
            }
        }
        let walk_from = |s: &ord_loops::Step| {
            let i = &infos[&s.base_edge];
            if s.reverse {
                (i.b, i.a)
            } else {
                (i.a, i.b)
            }
        };
        for w in route.steps.windows(2) {
            if walk_from(&w[0]).1 != walk_from(&w[1]).0 {
                v.insert(Violation::Discontinuous);
            }
        }
        if v.contains(&Violation::Discontinuous) {
            return finish(rep, v);
        }
        // Positions : la position de départ est sur la première arête, celle d'arrivée sur la dernière.
        let (first, last) = (&route.steps[0], &route.steps[n - 1]);
        if route.start.edge != first.base_edge || route.end.edge != last.base_edge {
            v.insert(Violation::BadOffset);
            return finish(rep, v);
        }
        let slack = |lh: f64| 1.0 + 0.001 * lh;
        let (si, ei) = (&infos[&first.base_edge], &infos[&last.base_edge]);
        let (lsh, leh) = (si.length_h(), ei.length_h());
        if !(route.start.offset_m >= -slack(lsh) && route.start.offset_m <= lsh + slack(lsh))
            || !(route.end.offset_m >= -slack(leh) && route.end.offset_m <= leh + slack(leh))
        {
            v.insert(Violation::BadOffset);
            return finish(rep, v);
        }
        let os = route.start.offset_m.clamp(0.0, lsh);
        let oe = route.end.offset_m.clamp(0.0, leh);
        let p_start = point_at(&si.pts, &si.cum, os);
        let p_end = point_at(&ei.pts, &ei.cum, oe);
        if haversine_m(p_start, (req.start.lat, req.start.lon)) > self.cfg.snap_m {
            v.insert(Violation::WrongStart);
        }
        let is_loop = req.end.is_none_or(|e| e.lat == req.start.lat && e.lon == req.start.lon);
        if is_loop {
            if route.start.edge != route.end.edge || (os - oe).abs() > self.cfg.loop_close_m {
                v.insert(Violation::WrongEnd);
            }
        } else if let Some(e) = req.end {
            if haversine_m(p_end, (e.lat, e.lon)) > self.cfg.snap_m {
                v.insert(Violation::WrongEnd);
            }
        }

        // Morceaux
        let mut pieces: Vec<Piece> = Vec::with_capacity(n);
        for (i, s) in route.steps.iter().enumerate() {
            let info = &infos[&s.base_edge];
            let lh = info.length_h();
            let (mut w0, mut w1) = if s.reverse { (lh, 0.0) } else { (0.0, lh) };
            if i == 0 {
                w0 = os;
            }
            if i == n - 1 {
                w1 = oe;
            }
            // On ne marche jamais à contre-sens de l'arête dans son propre sens de marche.
            let backwards = if s.reverse { w1 > w0 + 1e-6 } else { w1 < w0 - 1e-6 };
            if backwards {
                v.insert(Violation::BadOffset);
                return finish(rep, v);
            }
            let (lo, hi) = (w0.min(w1), w0.max(w1));
            let full = hi - lo >= lh - 1e-6;
            let len = if full { info.len_m } else { hi - lo };
            let mut pts = clip(&info.pts, &info.cum, lo, hi);
            if s.reverse {
                pts.reverse();
            }
            let to = walk_from(s).1;
            pieces.push(Piece { edge: s.base_edge, rev: s.reverse, len, lo, hi, to, way: info.way, pts });
        }
        rep.pieces = n;
        let total: f64 = pieces.iter().map(|p| p.len).sum();
        rep.length_m = total;

        // 2. Équipe (strict) ; l'équipe est mesurée dans tous les modes
        let team: Vec<bool> =
            pieces.iter().map(|p| self.is_team(req.ride_type, p.edge, infos[&p.edge].attrs)).collect();
        rep.team_m = pieces.iter().zip(&team).filter(|(_, t)| **t).map(|(p, _)| p.len).sum();
        rep.off_team_m = total - rep.team_m;
        let adm_deg = self.admitted_degrees(adm);
        let node_deg = |node: u32| adm_deg[node as usize];
        // nœud entre les morceaux j-1 et j (1 <= j <= n-1)
        let node_at = |j: usize| pieces[j - 1].to;
        if req.mode == Mode::Strict {
            let bridged = self.bridged_edges(req.ride_type);
            for (p, &t) in pieces.iter().zip(&team) {
                if t {
                    continue;
                }
                if bridged.contains(&p.edge) {
                    rep.bridged_m += p.len;
                } else {
                    v.insert(Violation::OffTeam);
                }
            }
        }
        if req.mode != Mode::Strict {
            rep.bridged_m = 0.0;
        }

        // 3. Demi-tour, virage ; 4. impasse
        let mut head = HeadCache::new(n);
        let u_turn: Vec<bool> = (1..n)
            .map(|j| pieces[j].edge == pieces[j - 1].edge && pieces[j].rev != pieces[j - 1].rev)
            .collect();
        for j in 1..n {
            let node = node_at(j);
            let deg = node_deg(node);
            if u_turn[j - 1] {
                v.insert(Violation::UTurn);
            } else if deg >= 3 {
                let a = self.heading(&pieces, j, true, &mut head);
                let b = self.heading(&pieces, j, false, &mut head);
                if turn_angle_deg(a, b) >= self.cfg.max_turn_deg
                    && !(self.cfg.hairpin_single_node && self.single_hairpin(&pieces, j, a, b))
                {
                    v.insert(Violation::SharpTurn);
                }
            }
            if deg < 2 {
                v.insert(Violation::DeadEnd);
            }
        }
        // grappes de nœuds à moins de 100 m le long du parcours
        // Mètres parcourus avant chaque nœud, pour lever la règle aux deux bouts du parcours : une grappe dont le
        // second nœud est à 200 m au plus du départ, ou le premier à 200 m au plus de l'arrivée, ne compte pas.
        let total_m: f64 = pieces.iter().map(|p| p.len).sum();
        let mut before = vec![0.0; n + 1];
        for k in 0..n {
            before[k + 1] = before[k] + pieces[k].len;
        }
        for a in 1..n {
            if node_deg(node_at(a)) < 3 || u_turn[a - 1] {
                continue;
            }
            let mut run = 0.0;
            for b in a + 1..n {
                run += pieces[b - 1].len;
                if run > self.cfg.cluster_m {
                    break;
                }
                if node_deg(node_at(b)) < 3 {
                    continue;
                }
                if before[b] <= self.cfg.cluster_free_ends_m
                    || total_m - before[a] <= self.cfg.cluster_free_ends_m
                {
                    continue;
                }
                let h_in = self.heading(&pieces, a, true, &mut head);
                let h_out = self.heading(&pieces, b, false, &mut head);
                if turn_angle_deg(h_in, h_out) >= self.cfg.max_turn_deg
                    && !self.hairpin(&pieces, a, b, h_in, h_out)
                {
                    v.insert(Violation::SharpTurn);
                }
            }
        }

        // 5. Réemprunt
        let mut by_edge: HashMap<u32, Vec<usize>> = HashMap::new();
        for (i, p) in pieces.iter().enumerate() {
            by_edge.entry(p.edge).or_default().push(i);
        }
        let mut marked = vec![false; n];
        let mut shared = 0.0;
        for (e, idx) in &by_edge {
            if idx.len() < 2 {
                continue;
            }
            let info = &infos[e];
            let scale = if info.length_h() > 0.0 { info.len_m / info.length_h() } else { 1.0 };
            let overlap = |x: usize, y: usize| {
                (pieces[x].hi.min(pieces[y].hi) - pieces[x].lo.max(pieces[y].lo)).max(0.0)
            };
            let mut spans: Vec<(f64, f64)> = Vec::new();
            for (k, &x) in idx.iter().enumerate() {
                for &y in &idx[k + 1..] {
                    if overlap(x, y) > self.cfg.overlap_m {
                        marked[x] = true;
                        marked[y] = true;
                        spans.push((pieces[x].lo.max(pieces[y].lo), pieces[x].hi.min(pieces[y].hi)));
                        for &z in idx {
                            if z > y
                                && overlap(x, z) > self.cfg.overlap_m
                                && overlap(y, z) > self.cfg.overlap_m
                            {
                                v.insert(Violation::EdgeUsedMoreThanTwice);
                            }
                        }
                    }
                }
            }
            spans.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut end = f64::NEG_INFINITY;
            for (lo, hi) in spans {
                let lo = lo.max(end);
                if hi > lo {
                    shared += (hi - lo) * scale;
                }
                end = end.max(hi);
            }
        }
        rep.shared_m = shared;
        // cœur : plus longue suite de morceaux non marqués, la première en cas d'égalité
        let (mut core_from, mut core_to, mut core_len) = (usize::MAX, 0usize, -1.0f64);
        let mut run_start: Option<usize> = None;
        let mut run_len = 0.0;
        for i in 0..=n {
            if i < n && !marked[i] {
                if run_start.is_none() {
                    run_start = Some(i);
                    run_len = 0.0;
                }
                run_len += pieces[i].len;
            } else if let Some(s) = run_start.take() {
                if run_len > core_len {
                    core_len = run_len;
                    core_from = s;
                    core_to = i - 1;
                }
            }
        }
        let limit = self.cfg.tail_share * req.distance_m;
        let has_core = core_from != usize::MAX;
        if !has_core {
            v.insert(Violation::ReuseOutsideTail);
        } else {
            rep.start_zone_m = pieces[..core_from].iter().map(|p| p.len).sum();
            rep.end_zone_m = pieces[core_to + 1..].iter().map(|p| p.len).sum();
            if rep.start_zone_m > limit || rep.end_zone_m > limit {
                v.insert(Violation::ReuseOutsideTail);
            }
        }
        if shared > limit {
            v.insert(Violation::TailTooLong);
        }

        // 6. Croisements. Comme en Java (`RouteGuarantees`, §5), la position de départ et celle d'arrivée sont des nœuds
        // du graphe de requête : un seul nœud pour une boucle, deux pour un aller simple.
        if has_core {
            let (v_start, v_end) = (u32::MAX, if is_loop { u32::MAX } else { u32::MAX - 1 });
            let at = |j: usize| -> u32 {
                if j == 0 {
                    v_start
                } else if j == n {
                    v_end
                } else {
                    node_at(j)
                }
            };
            let (k, m) = (core_from, core_to + 1);
            let mut tail_inner: HashSet<u32> = HashSet::new();
            for j in 0..k {
                tail_inner.insert(at(j));
            }
            for j in m + 1..=n {
                tail_inner.insert(at(j));
            }
            let first_node = at(k);
            let mut seen: HashSet<u32> = HashSet::new();
            seen.insert(first_node);
            let mut crossings = 0u32;
            for j in k + 1..=m {
                let node = at(j);
                let closure = j == m && node == first_node;
                if !closure && (seen.contains(&node) || tail_inner.contains(&node)) {
                    crossings += 1;
                }
                seen.insert(node);
            }
            rep.crossings = crossings;
            if crossings > self.cfg.max_crossings {
                v.insert(Violation::TooManyCrossings);
            }
            // 7. Couloir
            rep.corridor_share = self.corridor_share(&pieces[core_from..=core_to]);
            if rep.corridor_share > self.cfg.corridor_max_share {
                v.insert(Violation::CorridorDoubled);
            }
        }

        // 8. Distance
        if (total - req.distance_m).abs() > self.cfg.tolerance * req.distance_m {
            v.insert(Violation::DistanceOutOfTolerance);
        }
        finish(rep, v)
    }

    /// Polyligne `(x, y)` du parcours avant (`before`, dans l'ordre de marche, finissant au nœud) ou après le nœud
    /// entre les morceaux `j - 1` et `j`, sur autant de morceaux qu'il en faut pour atteindre `head_far_m`.
    fn window_xy(&self, pieces: &[Piece], j: usize, before: bool, proj: &Proj) -> Vec<(f64, f64)> {
        let far = self.cfg.head_far_m;
        let mut seq: Vec<Vec<(f64, f64)>> = Vec::new();
        let mut total = 0.0;
        if before {
            let mut i = j;
            while i > 0 && total < far {
                i -= 1;
                let xy: Vec<_> = pieces[i].pts.iter().map(|&p| proj.xy(p)).collect();
                total += poly_len(&xy);
                seq.push(xy);
            }
            seq.reverse();
        } else {
            let mut i = j;
            while i < pieces.len() && total < far {
                let xy: Vec<_> = pieces[i].pts.iter().map(|&p| proj.xy(p)).collect();
                total += poly_len(&xy);
                seq.push(xy);
                i += 1;
            }
        }
        seq.into_iter().flatten().collect()
    }

    /// Cap avant (`before`) ou après le nœud entre les morceaux `j - 1` et `j`, sur la fenêtre `[near, far]` prise
    /// le long du parcours, sur autant de morceaux qu'il en faut.
    fn heading(&self, pieces: &[Piece], j: usize, before: bool, cache: &mut HeadCache) -> f64 {
        let cached = if before { cache.before[j] } else { cache.after[j] };
        if let Some(h) = cached {
            return h;
        }
        let origin = if before { *pieces[j - 1].pts.last().unwrap() } else { pieces[j].pts[0] };
        let pts = self.window_xy(pieces, j, before, &Proj::new(origin));
        let h = path_heading(&pts, !before, self.cfg.head_near_m, self.cfg.head_far_m);
        if before {
            cache.before[j] = Some(h);
        } else {
            cache.after[j] = Some(h);
        }
        h
    }

    /// Apex d'une épingle sur un seul nœud : les deux morceaux sont de la même way et aucune portion droite de
    /// 15 m ou plus, à 90 ± 25° des caps, ne se trouve dans les 60 m de part et d'autre du nœud.
    fn single_hairpin(&self, pieces: &[Piece], j: usize, h_in: f64, h_out: f64) -> bool {
        if pieces[j - 1].way != pieces[j].way {
            return false;
        }
        let proj = Proj::new(*pieces[j - 1].pts.last().unwrap());
        let mut poly = tail_xy(&self.window_xy(pieces, j, true, &proj), 60.0);
        poly.extend(head_xy(&self.window_xy(pieces, j, false, &proj), 60.0));
        let rule = RunRule {
            min_m: self.cfg.connector_min_m,
            tol_deg: self.cfg.connector_tol_deg,
            bend_deg: self.cfg.run_bend_deg,
            min_piece_m: self.cfg.run_min_piece_m,
        };
        !has_connector_run(&poly, h_in, h_out, &rule)
    }

    /// Épingle d'une même way (§6, point 3) : les morceaux `a - 1` à `b` sont de la même way OSM, la corde entre
    /// les nœuds `a` et `b` fait moins de 30 m en travers de chacun des deux caps, et aucune portion droite de
    /// 15 m ou plus n'est à 90 ± 25° de l'un d'eux.
    fn hairpin(&self, pieces: &[Piece], a: usize, b: usize, h_in: f64, h_out: f64) -> bool {
        let way = pieces[a - 1].way;
        if pieces[a - 1..=b].iter().any(|p| p.way != way) {
            return false;
        }
        let p_a = *pieces[a - 1].pts.last().unwrap();
        let p_b = *pieces[b - 1].pts.last().unwrap();
        let proj = Proj::new(p_a);
        let (xa, xb) = (proj.xy(p_a), proj.xy(p_b));
        let (dx, dy) = (xb.0 - xa.0, xb.1 - xa.1);
        if across_m(dx, dy, h_in) >= self.cfg.hairpin_chord_m
            || across_m(dx, dy, h_out) >= self.cfg.hairpin_chord_m
        {
            return false;
        }
        let mut poly: Vec<(f64, f64)> = Vec::new();
        for p in &pieces[a..b] {
            poly.extend(p.pts.iter().skip(usize::from(!poly.is_empty())).map(|&q| proj.xy(q)));
        }
        let rule = RunRule {
            min_m: self.cfg.connector_min_m,
            tol_deg: self.cfg.connector_tol_deg,
            bend_deg: self.cfg.run_bend_deg,
            min_piece_m: self.cfg.run_min_piece_m,
        };
        !has_connector_run(&poly, h_in, h_out, &rule)
    }

    /// Part du cœur à moins de 25 m d'un autre tronçon du cœur situé à 400 m ou plus le long du parcours :
    /// échantillons tous les 10 m, sans les 30 premiers et derniers mètres ; `10 × doublés / (2 × longueur)`.
    fn corridor_share(&self, core: &[Piece]) -> f64 {
        let c = &self.cfg;
        let Some(first) = core.first().and_then(|p| p.pts.first()) else { return 0.0 };
        let proj = Proj::new(*first);
        let mut samples: Vec<(f64, f64, f64)> = Vec::new(); // x, y, abscisse
        let mut offset = 0.0;
        let mut next = 0.0;
        for p in core {
            let xy: Vec<_> = p.pts.iter().map(|&q| proj.xy(q)).collect();
            let len = poly_len(&xy);
            while next <= offset + len {
                let pt = along(&xy, next - offset);
                samples.push((pt.0, pt.1, next));
                next += c.corridor_step_m;
            }
            offset += len;
        }
        let core_len = offset;
        if samples.is_empty() || core_len <= 2.0 * c.corridor_end_ignore_m {
            return 0.0;
        }
        let kept: Vec<&(f64, f64, f64)> = samples
            .iter()
            .filter(|s| s.2 >= c.corridor_end_ignore_m && s.2 <= core_len - c.corridor_end_ignore_m)
            .collect();
        let cell = |v: f64| (v / c.corridor_dist_m).floor() as i64;
        let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (i, s) in kept.iter().enumerate() {
            grid.entry((cell(s.0), cell(s.1))).or_default().push(i);
        }
        let mut doubled = 0usize;
        for s in &kept {
            let (cx, cy) = (cell(s.0), cell(s.1));
            let hit = (-1..=1).any(|dx| {
                (-1..=1).any(|dy| {
                    grid.get(&(cx + dx, cy + dy)).is_some_and(|list| {
                        list.iter().any(|&j| {
                            let q = kept[j];
                            (q.2 - s.2).abs() >= c.corridor_gap_m
                                && dist((q.0, q.1), (s.0, s.1)) <= c.corridor_dist_m
                        })
                    })
                })
            });
            if hit {
                doubled += 1;
            }
        }
        c.corridor_step_m * doubled as f64 / (2.0 * core_len)
    }
}

/// Caps déjà calculés, par nœud (`before[j]` : avant le nœud entre les morceaux `j - 1` et `j`).
struct HeadCache {
    before: Vec<Option<f64>>,
    after: Vec<Option<f64>>,
}

impl HeadCache {
    fn new(n: usize) -> Self {
        Self { before: vec![None; n + 1], after: vec![None; n + 1] }
    }
}

/// Les `m` derniers mètres d'une polyligne.
fn tail_xy(pts: &[(f64, f64)], m: f64) -> Vec<(f64, f64)> {
    let total = poly_len(pts);
    if total <= m {
        return pts.to_vec();
    }
    let cut = total - m;
    let mut out = vec![along(pts, cut)];
    let mut acc = 0.0;
    for w in pts.windows(2) {
        acc += dist(w[0], w[1]);
        if acc > cut {
            out.push(w[1]);
        }
    }
    out
}

/// Les `m` premiers mètres d'une polyligne.
fn head_xy(pts: &[(f64, f64)], m: f64) -> Vec<(f64, f64)> {
    let total = poly_len(pts);
    if total <= m {
        return pts.to_vec();
    }
    let mut out = vec![pts[0]];
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let seg = dist(w[0], w[1]);
        if acc + seg >= m {
            out.push(along(pts, m));
            break;
        }
        acc += seg;
        out.push(w[1]);
    }
    out
}
