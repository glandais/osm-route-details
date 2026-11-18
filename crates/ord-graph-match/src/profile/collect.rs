//! Retenue des boucles d'apprentissage (§14.4) : le jugement d'une trace appariée, la requête que le moteur joue pour
//! la boucle retenue. Code déplacé du banc de vraisemblance (`export::judge`, `play::request_of`) pour que le banc et
//! le service jugent et jouent de la même façon.

use ord_graph::mmap::MappedGraph;
use ord_loops::{Bearing8, Figures, LatLon, Mode, Request, RideType};

use crate::matcher::Piece;
use crate::plane::{self, Pt};
use crate::profile::shape::{ShapeAcc, ShapeProfile};
use crate::walk::{Trace, Walk};

/// Pourquoi une trace n'est pas retenue, dans l'ordre où c'est jugé.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Excluded,
    Unreadable,
    Type,
    Matched,
    Empty,
    Hole,
    Short,
    NotLoop,
    Long,
}

/// Paramètres de retenue. Défauts du banc : 0,95 ; 1 000 ; 5 ; aucun plafond ; 0,05. Le service passe
/// `max_km = distance_m_max / 1 000`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RetainParams {
    pub min_matched: f64,
    pub max_hole_m: f64,
    pub min_km: f64,
    pub max_km: Option<f64>,
    pub loop_gap: f64,
}

impl Default for RetainParams {
    fn default() -> Self {
        Self { min_matched: 0.95, max_hole_m: 1_000.0, min_km: 5.0, max_km: None, loop_gap: 0.05 }
    }
}

/// Juge une trace déjà appariée (§11.3 de la note) : la marche et sa trace plane, ou la cause du refus. `total_m` et
/// `matched_m` : mètres annoncés et retenus ; `pieces` : la suite appariée dans l'ordre de la trace.
#[allow(clippy::too_many_arguments)]
pub fn judge_pieces(
    gf: &MappedGraph,
    p: &RetainParams,
    ty: u8,
    excluded: bool,
    readable: bool,
    total_m: f64,
    matched_m: f64,
    pieces: &[Piece],
) -> Result<(Walk, Trace), Refusal> {
    if excluded {
        return Err(Refusal::Excluded);
    }
    if !readable {
        return Err(Refusal::Unreadable);
    }
    if ty > 2 {
        return Err(Refusal::Type);
    }
    let share = if total_m > 0.0 { matched_m / total_m } else { 0.0 };
    if share < p.min_matched {
        return Err(Refusal::Matched);
    }
    if pieces.is_empty() {
        return Err(Refusal::Empty);
    }
    let walk = Walk::from_pieces(gf, pieces);
    if walk.legs.is_empty() {
        return Err(Refusal::Empty);
    }
    let trace = walk.trace(gf);
    if trace.max_hole_m > p.max_hole_m {
        return Err(Refusal::Hole);
    }
    if trace.d_graph < p.min_km * 1000.0 {
        return Err(Refusal::Short);
    }
    let gap = plane::dist(trace.pts[0], trace.pts[trace.pts.len() - 1]);
    if gap >= p.loop_gap * trace.l_poly {
        return Err(Refusal::NotLoop);
    }
    if p.max_km.is_some_and(|k| trace.d_graph > k * 1000.0) {
        return Err(Refusal::Long);
    }
    Ok((walk, trace))
}

/// Le type de parcours d'un code de type de la surcouche (`doc::type_code`, 0 à 2).
pub fn ride_of(ty: u8) -> RideType {
    match ty {
        0 => RideType::Road,
        1 => RideType::Gravel,
        _ => RideType::Mtb,
    }
}

/// Cap arrondi à 45° (`None` si le barycentre est à moins de 2 % de la longueur du départ).
pub fn bearing_of(pts: &[Pt], l_poly: f64) -> Option<Bearing8> {
    let c = plane::barycentre(pts);
    let o = pts[0];
    if plane::dist(o, c) < 0.02 * l_poly {
        return None;
    }
    let h = plane::heading_deg(o, c);
    let k = ((h / 45.0).round() as i64).rem_euclid(8);
    Some(
        [
            Bearing8::N,
            Bearing8::NE,
            Bearing8::E,
            Bearing8::SE,
            Bearing8::S,
            Bearing8::SW,
            Bearing8::W,
            Bearing8::NW,
        ][k as usize],
    )
}

/// `D` de la requête d'une boucle : la longueur du graphe arrondie au kilomètre (au moins 1 km), m.
pub fn request_distance_m(d_graph: f64) -> f64 {
    (d_graph / 1000.0).round().max(1.0) * 1000.0
}

/// La requête qui rejoue une boucle réelle (§11.6 de la note) : départ, cap, `D` au kilomètre, type, et le curseur déjà
/// appliqué (`mode`, `familiarity`, `off_team_cost_q8`). `seed` : la graine du moteur.
#[allow(clippy::too_many_arguments)]
pub fn loop_request(
    start: (f64, f64),
    bearing: Option<Bearing8>,
    distance_m: f64,
    ty: u8,
    mode: Mode,
    familiarity: f32,
    off_team_cost_q8: u32,
    seed: u64,
    figures: Figures,
) -> Request {
    Request {
        start: LatLon::new(start.0, start.1),
        end: None,
        bearing,
        distance_m,
        ride_type: ride_of(ty),
        mode,
        familiarity,
        off_team_cost_q8,
        seed,
        figures,
    }
}

// --- Le collecteur ------------------------------------------------------------------------------------------------------

use std::collections::BinaryHeap;

use ord_loops::TASTE_CLASSES;
use xxhash_rust::xxh3::xxh3_64;

use crate::walk::{class_lengths, start_latlon, Join};

/// Plafond des boucles gardées pour l'apprentissage (§14.4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    /// Au plus `n` boucles par équipe, tous types confondus (décision 1 du §9.4 de la note).
    Team(usize),
    /// Au plus `n` boucles par type (ROAD, GRAVEL, MTB).
    PerType(usize),
    /// Toutes.
    None,
}

impl Cap {
    /// Texte canonique, pour l'empreinte des réglages d'apprentissage.
    pub fn canonical(self) -> String {
        match self {
            Self::Team(n) => format!("team:{n}"),
            Self::PerType(n) => format!("type:{n}"),
            Self::None => "none".to_string(),
        }
    }
}

/// Une boucle retenue, réduite à ce que le moteur rejoue : la marche et la trace sont lâchées.
#[derive(Clone, Debug, PartialEq)]
pub struct LearnLoop {
    /// Empreinte du contenu de la marche (§14.4.2) : la graine des requêtes et l'ordre du tirage.
    pub key: u64,
    pub ty: u8,
    pub start: (f64, f64),
    pub bearing: Option<Bearing8>,
    /// `D` de la requête : la longueur du graphe arrondie au kilomètre.
    pub distance_m: f64,
    /// Mètres du réel par classe de goût.
    pub real_m: [f64; TASTE_CLASSES],
}

/// Ce que le collecteur a vu : jamais un identifiant, que des décomptes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CollectCounts {
    /// Traces proposées.
    pub offered: u64,
    /// Refus, dans l'ordre de [`Refusal`] (`Excluded`, `Unreadable`, `Type`, `Matched`, `Empty`, `Hole`, `Short`,
    /// `NotLoop`, `Long`).
    pub refused: [u64; 9],
    /// Boucles retenues, avant plafond.
    pub retained: u64,
}

/// Les boucles gardées, triées par `(key, ty)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LearnSet {
    pub loops: Vec<LearnLoop>,
    pub counts: CollectCounts,
    /// Les formes de **toutes** les boucles retenues, plafond ou non (lot 4, 4c) : un agrégat par type.
    pub shapes: ShapeProfile,
}

impl LearnSet {
    /// Le même ensemble sous un plafond : exactement ce que le collecteur aurait gardé avec ce plafond (les plus petites
    /// empreintes, par équipe ou par type). Le banc collecte une fois sans plafond et en tire plusieurs.
    pub fn capped(&self, cap: Cap) -> LearnSet {
        let loops: Vec<LearnLoop> = match cap {
            Cap::None => self.loops.clone(),
            Cap::Team(n) => self.loops.iter().take(n).cloned().collect(),
            Cap::PerType(n) => {
                let mut seen = [0usize; 3];
                self.loops
                    .iter()
                    .filter(|l| {
                        let t = usize::from(l.ty).min(2);
                        seen[t] += 1;
                        seen[t] <= n
                    })
                    .cloned()
                    .collect()
            }
        };
        LearnSet { loops, counts: self.counts.clone(), shapes: self.shapes.clone() }
    }
}

/// Élément d'un tas max sur `(key, ty)`.
#[derive(Debug)]
struct Item(LearnLoop);

impl PartialEq for Item {
    fn eq(&self, o: &Self) -> bool {
        (self.0.key, self.0.ty) == (o.0.key, o.0.ty)
    }
}
impl Eq for Item {}
impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Item {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        (self.0.key, self.0.ty).cmp(&(o.0.key, o.0.ty))
    }
}

/// Retient les boucles des traces appariées au fil du dépôt et n'en garde que le plafond, tirées par empreinte de
/// contenu : le résultat ne dépend pas de l'ordre d'arrivée des lignes (§14.4.2).
#[derive(Debug)]
pub struct LoopCollector {
    params: RetainParams,
    cap: Cap,
    /// Un tas (équipe, ou aucun plafond) ou trois (un par type).
    heaps: Vec<BinaryHeap<Item>>,
    counts: CollectCounts,
    /// Formes des boucles retenues, comptées avant le plafond.
    shapes: ShapeAcc,
}

impl LoopCollector {
    pub fn new(params: RetainParams, cap: Cap) -> Self {
        let n = if matches!(cap, Cap::PerType(_)) { 3 } else { 1 };
        Self {
            params,
            cap,
            heaps: (0..n).map(|_| BinaryHeap::new()).collect(),
            counts: CollectCounts::default(),
            shapes: ShapeAcc::default(),
        }
    }

    /// Propose une trace appariée (mêmes arguments que [`judge_pieces`]).
    #[allow(clippy::too_many_arguments)]
    pub fn offer(
        &mut self,
        gf: &MappedGraph,
        ty: u8,
        excluded: bool,
        readable: bool,
        total_m: f64,
        matched_m: f64,
        pieces: &[Piece],
    ) {
        self.counts.offered += 1;
        let (walk, trace) =
            match judge_pieces(gf, &self.params, ty, excluded, readable, total_m, matched_m, pieces) {
                Ok(x) => x,
                Err(r) => {
                    self.counts.refused[r as usize] += 1;
                    return;
                }
            };
        self.counts.retained += 1;
        self.shapes.add(ty, &trace.pts);
        let Some(start) = start_latlon(gf, &walk) else { return };
        let item = LearnLoop {
            key: walk_key(ty, &walk),
            ty,
            start,
            bearing: bearing_of(&trace.pts, trace.l_poly),
            distance_m: request_distance_m(trace.d_graph),
            real_m: class_lengths(gf, &walk, &trace),
        };
        let (heap, cap) = match self.cap {
            Cap::Team(n) => (&mut self.heaps[0], Some(n)),
            Cap::PerType(n) => (&mut self.heaps[usize::from(ty).min(2)], Some(n)),
            Cap::None => (&mut self.heaps[0], None),
        };
        heap.push(Item(item));
        if cap.is_some_and(|n| heap.len() > n) {
            heap.pop();
        }
    }

    /// Les boucles gardées, triées par `(empreinte, type)`.
    pub fn finish(self) -> LearnSet {
        let mut loops: Vec<LearnLoop> =
            self.heaps.into_iter().flat_map(BinaryHeap::into_vec).map(|i| i.0).collect();
        loops.sort_by_key(|l| (l.key, l.ty));
        LearnSet { loops, counts: self.counts, shapes: self.shapes.finish() }
    }
}

/// Empreinte du contenu d'une marche (§14.4.2) : le type, puis par jambe l'arête, le sens et les bornes, par jonction
/// son étiquette et son nœud ou sa longueur de trou. Deux marches égales ont la même empreinte, la marche inverse une
/// autre.
pub fn walk_key(ty: u8, walk: &Walk) -> u64 {
    let mut b: Vec<u8> = Vec::with_capacity(16 + walk.legs.len() * 24);
    b.extend_from_slice(b"ordp/loop/v1");
    b.push(ty);
    for (i, l) in walk.legs.iter().enumerate() {
        b.extend_from_slice(&l.edge.to_le_bytes());
        b.push(u8::from(l.reverse));
        b.extend_from_slice(&l.lo.to_bits().to_le_bytes());
        b.extend_from_slice(&l.hi.to_bits().to_le_bytes());
        if let Some(j) = walk.joins.get(i) {
            match j {
                Join::Node(n) => {
                    b.push(0);
                    b.extend_from_slice(&n.to_le_bytes());
                }
                Join::Touch => b.push(1),
                Join::Hole(d) => {
                    b.push(2);
                    b.extend_from_slice(&d.to_bits().to_le_bytes());
                }
            }
        }
    }
    xxh3_64(&b)
}

#[cfg(test)]
pub(crate) mod tests {
    use ord_graph::mmap::MappedGraph;

    use super::*;
    use crate::fixture::{edge_between, graph, id, len_m};

    /// Un morceau qui parcourt l'arête entre deux nœuds OSM.
    pub(crate) fn full(gf: &MappedGraph, from: i64, to: i64) -> Piece {
        let (e, rev) = edge_between(gf, from, to);
        let elen = len_m(gf, e) as f32;
        let (from, to) = if rev { (elen, 0.0) } else { (0.0, elen) };
        Piece { edge: e, from, to, elen }
    }

    /// Les quatre nœuds du carré de coin `(r, c)`, dans le sens direct.
    pub(crate) fn square(r: i64, c: i64) -> [i64; 4] {
        [id(r, c), id(r, c + 1), id(r + 1, c + 1), id(r + 1, c)]
    }

    /// Le tour du carré à partir du nœud `start`, dans le sens `dir` : une boucle de 1 200 m.
    pub(crate) fn lap(gf: &MappedGraph, nodes: [i64; 4], start: usize, backwards: bool) -> Vec<Piece> {
        let mut ring: Vec<i64> = (0..4).map(|k| nodes[(start + k) % 4]).collect();
        if backwards {
            ring[1..].reverse();
        }
        (0..4).map(|k| full(gf, ring[k], ring[(k + 1) % 4])).collect()
    }

    fn params() -> RetainParams {
        RetainParams { min_km: 0.5, ..RetainParams::default() }
    }

    fn offer(c: &mut LoopCollector, gf: &MappedGraph, pieces: &[Piece]) {
        c.offer(gf, 0, false, true, 1_200.0, 1_200.0, pieces);
    }

    /// Toutes les boucles distinctes de la grille : 4 carrés × 4 départs × 2 sens.
    pub(crate) fn all_laps(gf: &MappedGraph) -> Vec<Vec<Piece>> {
        let mut v = Vec::new();
        for (r, c) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            for s in 0..4 {
                for b in [false, true] {
                    v.push(lap(gf, square(r, c), s, b));
                }
            }
        }
        v
    }

    #[test]
    fn each_refusal_of_the_bench_is_counted_in_its_own_order() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let lp = lap(&gf, square(0, 0), 0, false);
        let mut c = LoopCollector::new(params(), Cap::None);
        let t = |c: &mut LoopCollector, ty, ex, rd, tot: f64, mat: f64, p: &[Piece]| {
            c.offer(&gf, ty, ex, rd, tot, mat, p);
        };
        t(&mut c, 0, true, true, 1_200.0, 1_200.0, &lp); // Excluded
        t(&mut c, 0, false, false, 1_200.0, 1_200.0, &lp); // Unreadable
        t(&mut c, 3, false, true, 1_200.0, 1_200.0, &lp); // Type
        t(&mut c, 0, false, true, 1_200.0, 600.0, &lp); // Matched
        t(&mut c, 0, false, true, 1_200.0, 1_200.0, &[]); // Empty
                                                          // Un trou : les arêtes 1 et 3 seulement, de 300 m entre les deux.
        let holey = [lp[0], lp[2]];
        let mut strict = LoopCollector::new(RetainParams { max_hole_m: 100.0, ..params() }, Cap::None);
        offer(&mut strict, &gf, &holey);
        assert_eq!(strict.finish().counts.refused[Refusal::Hole as usize], 1);
        let mut short = LoopCollector::new(RetainParams { min_km: 2.0, ..params() }, Cap::None);
        offer(&mut short, &gf, &lp);
        assert_eq!(short.finish().counts.refused[Refusal::Short as usize], 1);
        // Pas une boucle : trois côtés du carré.
        offer(&mut c, &gf, &lp[..3]); // NotLoop
        let mut long = LoopCollector::new(RetainParams { max_km: Some(1.0), ..params() }, Cap::None);
        offer(&mut long, &gf, &lp);
        assert_eq!(long.finish().counts.refused[Refusal::Long as usize], 1);
        offer(&mut c, &gf, &lp); // retenue
        let set = c.finish();
        let r = set.counts.refused;
        assert_eq!(r[Refusal::Excluded as usize], 1);
        assert_eq!(r[Refusal::Unreadable as usize], 1);
        assert_eq!(r[Refusal::Type as usize], 1);
        assert_eq!(r[Refusal::Matched as usize], 1);
        assert_eq!(r[Refusal::Empty as usize], 1);
        assert_eq!(r[Refusal::NotLoop as usize], 1);
        assert_eq!(set.counts.offered, 7);
        assert_eq!((set.counts.retained, set.loops.len()), (1, 1));
        let l = &set.loops[0];
        assert_eq!((l.ty, l.distance_m), (0, 1_000.0));
        assert!((l.real_m.iter().sum::<f64>() - 1_200.0).abs() < 1.0);
    }

    #[test]
    fn the_key_is_the_content_of_the_walk() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let key = |p: &[Piece], ty| walk_key(ty, &Walk::from_pieces(&gf, p));
        let a = lap(&gf, square(0, 0), 0, false);
        assert_eq!(key(&a, 0), key(&lap(&gf, square(0, 0), 0, false), 0));
        assert_ne!(key(&a, 0), key(&lap(&gf, square(0, 0), 0, true), 0), "la marche inverse");
        assert_ne!(key(&a, 0), key(&lap(&gf, square(0, 0), 1, false), 0), "un autre départ");
        assert_ne!(key(&a, 0), key(&a, 1), "un autre type");
    }

    #[test]
    fn the_team_cap_keeps_the_smallest_keys_whatever_the_arrival_order() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let laps = all_laps(&gf);
        let run = |order: &[usize], cap: Cap| {
            let mut c = LoopCollector::new(params(), cap);
            for &i in order {
                offer(&mut c, &gf, &laps[i]);
            }
            c.finish()
        };
        let n = laps.len();
        let forward: Vec<usize> = (0..n).collect();
        let backward: Vec<usize> = (0..n).rev().collect();
        let shuffled: Vec<usize> = (0..n).map(|i| (i * 7 + 3) % n).collect();
        assert_eq!(shuffled.iter().collect::<std::collections::BTreeSet<_>>().len(), n);
        let all = run(&forward, Cap::None);
        assert_eq!(all.loops.len(), n);
        assert!(all.loops.windows(2).all(|w| (w[0].key, w[0].ty) <= (w[1].key, w[1].ty)));
        let cap = Cap::Team(10);
        let (a, b, c) = (run(&forward, cap), run(&backward, cap), run(&shuffled, cap));
        assert_eq!(a.loops.len(), 10);
        assert_eq!((a.loops.clone(), a.loops.clone()), (b.loops, c.loops));
        // Ce sont les dix plus petites empreintes de l'ensemble.
        assert_eq!(a.loops, all.loops[..10].to_vec());
    }

    #[test]
    fn the_per_type_cap_keeps_the_smallest_of_each_type() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let laps = all_laps(&gf);
        let mut c = LoopCollector::new(params(), Cap::PerType(5));
        for (i, l) in laps.iter().enumerate() {
            c.offer(&gf, (i % 2) as u8, false, true, 1_200.0, 1_200.0, l);
        }
        let set = c.finish();
        for ty in 0..2u8 {
            assert_eq!(set.loops.iter().filter(|l| l.ty == ty).count(), 5);
        }
        assert_eq!(set.loops.len(), 10, "pas de MTB offert");
        // Un plafond par type ne dépasse jamais.
        let mut team = LoopCollector::new(params(), Cap::Team(7));
        for l in &laps {
            offer(&mut team, &gf, l);
            assert!(team.heaps[0].len() <= 8);
        }
        assert_eq!(team.finish().loops.len(), 7);
    }

    #[test]
    fn the_shapes_count_every_retained_loop_whatever_the_cap_and_the_order() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let laps = all_laps(&gf);
        let run = |order: &[usize], cap: Cap| {
            let mut c = LoopCollector::new(params(), cap);
            for &i in order {
                offer(&mut c, &gf, &laps[i]);
            }
            c.finish()
        };
        let n = laps.len();
        let fwd: Vec<usize> = (0..n).collect();
        let rev: Vec<usize> = (0..n).rev().collect();
        let (all, capped, other_order) =
            (run(&fwd, Cap::None), run(&fwd, Cap::Team(3)), run(&rev, Cap::Team(3)));
        assert_eq!(capped.loops.len(), 3);
        assert_eq!(all.shapes, capped.shapes, "les formes ne dépendent pas du plafond");
        assert_eq!(capped.shapes, other_order.shapes, "ni de l'ordre d'arrivée");
        let t = &all.shapes.types[0];
        assert_eq!(u64::from(t.loops), all.counts.retained, "toutes les boucles retenues sont comptées");
        // Les tours d'un carré de 300 m de côté ont trois sommets à 10 % de D.
        assert_eq!((t.k1, t.k2, t.k3), (0, 0, t.loops));
        assert_eq!(all.capped(Cap::Team(2)).shapes, all.shapes);
    }

    #[test]
    fn capping_afterwards_equals_collecting_with_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let gf = graph(dir.path());
        let laps = all_laps(&gf);
        let run = |cap: Cap| {
            let mut c = LoopCollector::new(params(), cap);
            for (i, l) in laps.iter().enumerate() {
                c.offer(&gf, (i % 3) as u8, false, true, 1_200.0, 1_200.0, l);
            }
            c.finish()
        };
        let all = run(Cap::None);
        for cap in [Cap::Team(9), Cap::PerType(4), Cap::None] {
            assert_eq!(all.capped(cap).loops, run(cap).loops, "{cap:?}");
        }
    }
}
