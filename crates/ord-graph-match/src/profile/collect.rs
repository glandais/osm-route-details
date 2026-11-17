//! Retenue des boucles d'apprentissage (§14.4) : le jugement d'une trace appariée, la requête que le moteur joue pour
//! la boucle retenue. Code déplacé du banc de vraisemblance (`export::judge`, `play::request_of`) pour que le banc et
//! le service jugent et jouent de la même façon.

use ord_graph::mmap::MappedGraph;
use ord_loops::{Bearing8, LatLon, Mode, Request, RideType};

use crate::matcher::Piece;
use crate::plane::{self, Pt};
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
    }
}
