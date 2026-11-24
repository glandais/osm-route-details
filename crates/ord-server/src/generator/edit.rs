//! Route `POST /v1/generator/routes/edit` (api 1.17) : retoucher un parcours enregistré (lot 7b, note de conception
//! `docs/design/technique/ord-retouche-lot1.md` §15, plan Pédalons `2026-11-22-retouche-parcours.md` §6.3).
//!
//! - **Deux opérations** : `resize { distance_m }` (la distance totale visée, raccourcir ou allonger d'un tronçon recollé
//!   par des coutures exactes) et `move_end { point }` (nouvelle arrivée). Le moteur est `ord_loops::{resize_in,
//!   move_end_in}` ; le service apparie le document `network_edges` de la trace au graphe comme le dépôt de la surcouche
//!   ([`ord_graph_match::posed`]), pose le parcours sur **la polyligne reçue**, puis recolle.
//! - **Même file, même budget, mêmes fenêtres que la génération** ([`Ready::slot`], `budget_ms`) : une retouche coûte comme
//!   une petite génération. Les bornes qui ne demandent aucun calcul de moteur (changement trop petit ou trop grand,
//!   parcours hors couverture) sont jugées **avant** la file.
//! - **En-têtes d'équipe facultatifs** : sans eux, ou sans surcouche servable, la découverte au goût commun
//!   (`overlay: null`) ; le curseur suit l'itinéraire ([`cursor::apply_route`], jamais le mode strict).
//! - **Le contrôleur indépendant juge chaque tronçon neuf** ; un tronçon refusé est retiré et compté (`dropped`).
//! - **Un refus est une réponse** : raccourcir échoue dans 4 cas sur 10 (`no_economy`), et 5 % des traces ne se posent pas
//!   (`order_anomaly`) : 422 `generation_failed` avec une raison que le client lit, jamais un 500.
//!
//! Rien de privé n'est journalisé : ni coordonnée, ni polyligne, ni document, ni distance exacte, ni clé, ni équipe ; une
//! ligne par requête. Les `///` des types sérialisés vont au contrat : ils sont en anglais.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use ord_graph_match::doc::{Document, ParsedEdge};
use ord_graph_match::geo::{decode_polyline6, encode_polyline6};
use ord_graph_match::posed::{doc_walk, posed_parts, DocWalk};
use ord_graph_match::CellIndex;
use ord_loops::{
    move_end_in, resize_in, route_geometry_spans, Figures, LatLon, Mode, MoveEndVariant, PoseError, PoseTune,
    PosedInput, PosedRoute, Request as EngineRequest, ResizePlan, Retouch, RetouchDiagnostics,
    RetouchFailure, RetouchKind, RetouchRequest, RetouchTune, Retouched, RideType, TeamCtx, WindowPlan,
};
use ord_loops_check::{haversine_m, Checker};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::crypto::Credentials;
use super::cursor::{self, Applied};
use super::generate::{band, mode_v1, overloaded, r1, ride_name, unavailable, CancelOnDrop};
use super::routes::{
    credentials, point, GeoPointV1, ModeV1, OverlayInfoV1, RideTypeV1, TasteV1, KEY_HEADER, TEAM_HEADER,
};
use super::store::{Entry, ReadyError};
use super::windows::{AcquireError, TeamRef};
use super::{Ready, EDIT_BODY_MAX};
use crate::app::AppState;
use crate::problem::{
    CoverageSource, FieldError, GenerationReason, LimitKind, Problem, ProblemCode, ProblemDetails,
};
use crate::strict_json::read_json;

/// Points de la polyligne reçue, au plus (après décodage) : au-delà, 413 `route_too_large`.
pub(crate) const EDIT_POINTS_MAX: usize = 50_000;

/// What the service publishes about route edits (`/v1/capabilities`, api 1.17).
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct EditCapabilities {
    /// Accepted `operation` keys: `resize` and `move_end`.
    pub operations: Vec<&'static str>,
    /// Accepted ride types (`ride_type`).
    pub ride_types: Vec<&'static str>,
    /// Largest change of `resize`, as a share of the route length: beyond, 422 `generation_failed` (reason
    /// `change_too_large`); generate a new route instead.
    pub change_share_max: f64,
    /// Smallest change of `resize`, in metres: below, 422 `generation_failed` (reason `change_too_small`).
    pub change_m_min: f64,
    /// Largest relative gap between the distance asked (`resize`) and the one of a returned route.
    pub tolerance: f64,
    /// Longest distance from the asked end of `move_end` to a way of the ride type, in metres: beyond, 422
    /// `generation_failed` (reason `end_too_far`).
    pub end_snap_m: f64,
    /// Routes returned at most.
    pub proposals_max: u32,
    /// Points of the polyline at most (413 `route_too_large`, `details.limit = points`).
    pub points_max: usize,
    /// Body of an edit request, in bytes.
    pub request_body_bytes: usize,
}

impl EditCapabilities {
    pub(super) fn new() -> Self {
        let tune = RetouchTune::default();
        Self {
            operations: vec!["resize", "move_end"],
            ride_types: vec!["road", "gravel", "mtb"],
            change_share_max: tune.max_change,
            change_m_min: tune.min_change_m,
            tolerance: tune.tolerance,
            end_snap_m: tune.end_snap_m,
            proposals_max: u32::try_from(tune.proposals).unwrap_or(u32::MAX),
            points_max: EDIT_POINTS_MAX,
            request_body_bytes: EDIT_BODY_MAX,
        }
    }
}

/// Request to edit a saved route.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct EditRequestV1 {
    /// The saved route, as a Google polyline of precision 6 (at least two distinct points, at most
    /// `capabilities.generator.edit.points_max`, a length within `capabilities.generator.limits.distance_m_max`).
    pub polyline: String,
    /// The `network_edges` document of the route's track, as the overlay deposit takes it (the edges
    /// `/v1/route-details` matched along this polyline, with `fm`, `tm` and `trv`). Unknown keys are tolerated. Without
    /// `fm` and `tm` on every edge, `unmatched_m` is an upper bound.
    #[schema(value_type = Object)]
    pub network_edges: serde_json::Value,
    pub ride_type: RideTypeV1,
    /// As for a route from A to B (`POST /v1/router/route`): with the team headers only, 0 routes team first with the
    /// highest off-team cost (strict mode is not offered), 1 is discovery. Absent: 1. Ignored without the team headers,
    /// but still validated within [0, 1].
    #[serde(default)]
    pub cursor: Option<f64>,
    /// Seed of the choice among equivalent edits (default 0); another seed asks for other proposals.
    #[serde(default)]
    pub seed: Option<u64>,
    /// Exactly one of `resize` or `move_end`.
    pub operation: EditOperationV1,
}

/// What to do to the route: exactly one key.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct EditOperationV1 {
    /// Reach a total distance by replacing a stretch of the route.
    #[serde(default)]
    pub resize: Option<ResizeV1>,
    /// Finish elsewhere: the route is kept up to a junction and a new stretch leads to the point.
    #[serde(default)]
    pub move_end: Option<MoveEndV1>,
}

#[derive(Clone, Copy, Debug, Deserialize, ToSchema)]
pub struct ResizeV1 {
    /// The total distance aimed at, in metres (not the change): within 1,000 m and 30 percent of the route length
    /// (`capabilities.generator.edit`), and at most `capabilities.generator.limits.distance_m_max`.
    pub distance_m: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, ToSchema)]
pub struct MoveEndV1 {
    /// The new end. Within the covered polygon (422 `out_of_coverage` otherwise) and within
    /// `capabilities.generator.edit.end_snap_m` of a way of the ride type (422 `end_too_far`). On the route itself, the
    /// route is cut there (kind `truncate`).
    pub point: GeoPointV1,
}

/// The operation an edit response answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EditOperationKindV1 {
    Resize,
    MoveEnd,
}

/// What changed in a returned route.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EditKindV1 {
    /// A stretch was replaced by a shorter one (`resize`).
    Shorten,
    /// A stretch was replaced by a longer one (`resize`).
    Lengthen,
    /// The route is kept up to a junction and a new stretch leads to the new end (`move_end`).
    MoveEnd,
    /// The new end is on the route: it is cut there, with no new stretch (`move_end`).
    Truncate,
}

/// Which choice a `move_end` route makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MoveEndVariantV1 {
    /// Least of the length dropped plus the new stretch.
    Best,
    /// Keeps the most of the route: leaves it at the last junction reached.
    KeepAll,
    /// Shortest new stretch: leaves the route nearest to the new end.
    Nearest,
}

/// The new stretch of a returned route.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ChangedV1 {
    /// The stretch of the saved route that was replaced, as distances along the saved polyline, in metres. `to_m` is the
    /// route length for `move_end` and `truncate` (the rest is dropped).
    pub from_m: f64,
    pub to_m: f64,
    /// Length of the new stretch, in metres (0 for `truncate`).
    pub new_m: f64,
    /// Indices, in the returned `polyline`, of the first and the last point of the new stretch. Up to the first one the
    /// polyline is the saved one; a gap of 15 m at most may remain between the cut of the saved line and the junction.
    pub points: [u32; 2],
    /// Measures of the new stretch alone, by the independent checker (the kept parts of the route are not a path of the
    /// graph, their measures are not known here).
    pub team_m: f64,
    pub bridged_m: f64,
    /// (team_m + bridged_m) / new_m.
    pub team_share: f64,
    pub offroad_m: f64,
    pub offroad_share: f64,
    /// Ranges of indices of the returned `polyline` ridden off pavement in the new stretch (as in a route from A to B).
    pub unpaved: Vec<[u32; 2]>,
    pub crossings: u32,
}

/// An edited route.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct EditedRouteV1 {
    pub kind: EditKindV1,
    /// For `move_end` only.
    pub variant: Option<MoveEndVariantV1>,
    /// The whole edited route, Google polyline of precision 6: the saved polyline up to the change, the new stretch, then
    /// (`shorten` and `lengthen`) the saved polyline again.
    pub polyline: String,
    /// Distance of the edited route, in metres: the saved parts as they were drawn, plus the new stretch on the graph.
    pub distance_m: f64,
    /// `|distance_m - distance asked| / distance asked` (`resize` only).
    pub error_share: Option<f64>,
    /// Metres of the saved route kept, and their share of it.
    pub kept_m: f64,
    pub kept_share: f64,
    /// Metres of the saved route dropped after the junction (`move_end` and `truncate` only).
    pub abandoned_m: Option<f64>,
    /// Distance from the asked end to the last point of the polyline, in metres (`move_end` only).
    pub end_snap_m: Option<f64>,
    /// Metres of the kept parts that `network_edges` did not match to the graph: there, the route may ride an edge twice
    /// and no check was made. An upper bound when an edge of the document has no `fm` or `tm`.
    pub unmatched_m: f64,
    pub changed: ChangedV1,
}

/// Edited routes, the best first.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct EditResponseV1 {
    pub graph_id: String,
    /// Mode applied: `discovery` without the team headers, or when the team overlay is absent or stale (then `overlay`
    /// is null); never `strict`.
    pub mode: ModeV1,
    pub overlay: Option<OverlayInfoV1>,
    /// The taste the edit was served with.
    pub taste: TasteV1,
    pub operation: EditOperationKindV1,
    /// Length of the saved polyline, in metres.
    pub original_m: f64,
    /// The distance asked (`resize`), null otherwise.
    pub target_m: Option<f64>,
    /// Routes the engine found and the independent checker refused.
    pub dropped: u32,
    /// One to three routes.
    pub routes: Vec<EditedRouteV1>,
}

/// L'opération demandée, validée.
#[derive(Clone, Copy, Debug)]
pub(crate) enum EditOp {
    Resize { target_m: f64 },
    MoveEnd { end: LatLon },
}

impl EditOp {
    fn name(self) -> &'static str {
        match self {
            EditOp::Resize { .. } => "resize",
            EditOp::MoveEnd { .. } => "move_end",
        }
    }
}

/// Une demande validée. **Privée** : porte la polyligne et le document de la trace, jamais journalisés.
pub(crate) struct EditInput {
    /// Points décodés, sans doublon consécutif en E7.
    line: Vec<LatLon>,
    /// Longueur haversine de `line`, m.
    original_m: f64,
    edges: Vec<ParsedEdge>,
    doc_len: Option<f64>,
    ride: RideType,
    /// Curseur en centièmes (100 sans valeur).
    cursor: u32,
    seed: u64,
    op: EditOp,
}

fn hav(a: LatLon, b: LatLon) -> f64 {
    haversine_m((a.lat, a.lon), (b.lat, b.lon))
}

fn line_len(line: &[LatLon]) -> f64 {
    line.windows(2).map(|w| hav(w[0], w[1])).sum()
}

/// Marge, en mètres, sur la longueur de la polyligne : les arêtes du document sont comptées entières, leurs bouts
/// dépassent un peu.
const DOC_SLACK_M: f64 = 2_000.0;
/// Rapport maximal entre la longueur des arêtes du document (`trv` ou géométrie décodée) et la polyligne reçue.
const DOC_RATIO_MAX: f64 = 1.5;

/// Vrai si une arête, ou le total des arêtes, est nettement plus longue que la polyligne reçue : la dérive de la pose le
/// refuserait de toute façon, et l'appariement d'une géométrie démesurée ne s'interrompt pas.
fn document_overruns(edges: &[ParsedEdge], original_m: f64) -> bool {
    let cap = original_m * DOC_RATIO_MAX + DOC_SLACK_M;
    let (mut trv, mut geometry) = (0.0f64, 0.0f64);
    for e in edges {
        let t = f64::from(e.trv);
        if t.is_finite() {
            trv += t.max(0.0);
        }
        if trv > cap {
            return true;
        }
        if let Some(g) = decode_polyline6(&e.g) {
            let pts: Vec<LatLon> = g.iter().map(|p| LatLon::new(p[0], p[1])).collect();
            geometry += line_len(&pts);
            if geometry > cap {
                return true;
            }
        }
    }
    false
}

fn validate(req: &EditRequestV1, ready: &Ready) -> Result<EditInput, Problem> {
    let mut errors = Vec::new();
    // La polyligne : décodée (les bornes de latitude et de longitude y sont jugées), les doublons en E7 retirés.
    let decoded = decode_polyline6(&req.polyline);
    let raw_points = decoded.as_ref().map_or(0, Vec::len);
    let mut line: Vec<LatLon> = Vec::new();
    match decoded {
        None => errors.push(FieldError::new("/polyline", "not a valid Google polyline of precision 6")),
        Some(points) => {
            let mut last = None;
            for p in points {
                let e7 = LatLon::new(p[0], p[1]).to_e7();
                if last != Some(e7) {
                    line.push(LatLon::from_e7(e7));
                    last = Some(e7);
                }
            }
            if line.len() < 2 {
                errors.push(FieldError::new("/polyline", "needs at least two distinct points"));
            }
        }
    }
    // Le document : un objet dont `edges`, s'il est là, est un tableau d'objets lisibles.
    let (edges, doc_len) =
        match req.network_edges.is_object().then(|| Document::deserialize(&req.network_edges).ok()).flatten()
        {
            Some(doc) => {
                let len = doc.len_m();
                (doc.into_edges(), len)
            }
            None => {
                errors.push(FieldError::new("/network_edges", "unreadable network_edges document"));
                (Vec::new(), None)
            }
        };
    let ride = match req.ride_type {
        RideTypeV1::Road => RideType::Road,
        RideTypeV1::Gravel => RideType::Gravel,
        RideTypeV1::Mtb => RideType::Mtb,
    };
    let cursor = match req.cursor {
        None => Some(100),
        Some(c) => cursor::hundredths(c),
    };
    if cursor.is_none() {
        errors.push(FieldError::new("/cursor", "must be within [0, 1]"));
    }
    let op = match (&req.operation.resize, &req.operation.move_end) {
        (Some(r), None) => {
            if !(r.distance_m.is_finite() && r.distance_m > 0.0 && r.distance_m <= ready.cfg.distance_m_max) {
                errors.push(FieldError::new(
                    "/operation/resize/distance_m",
                    "must be positive and at most limits.distance_m_max (see capabilities)",
                ));
            }
            Some(EditOp::Resize { target_m: r.distance_m })
        }
        (None, Some(m)) => {
            let end = point(m.point, "/operation/move_end/point", &mut errors);
            Some(EditOp::MoveEnd { end })
        }
        _ => {
            errors.push(FieldError::new("/operation", "exactly one of resize or move_end"));
            None
        }
    };
    if !errors.is_empty() {
        return Err(Problem::invalid_input("invalid request", errors));
    }
    // Les bornes d'entrée : 413.
    if raw_points > EDIT_POINTS_MAX {
        return Err(Problem::too_large(LimitKind::Points, "too many points in the polyline"));
    }
    let original_m = line_len(&line);
    if original_m > ready.cfg.distance_m_max {
        return Err(Problem::too_large(LimitKind::LengthM, "the route is longer than the service serves"));
    }
    // Avant la file et avant tout appariement : un document dont les arêtes sont bien plus longues que la polyligne ne
    // peut pas passer le contrôle de dérive de la pose, et l'apparier occuperait une place des générations pour rien.
    if document_overruns(&edges, original_m) {
        return Err(failed(GenerationReason::RouteMismatch));
    }
    match (op, cursor) {
        (Some(op), Some(cursor)) => {
            Ok(EditInput { line, original_m, edges, doc_len, ride, cursor, seed: req.seed.unwrap_or(0), op })
        }
        // Jamais : une erreur de champ est déjà rendue.
        _ => Err(Problem::internal()),
    }
}

fn timeout() -> Problem {
    Problem::new(ProblemCode::Timeout, "route edit budget exceeded")
}

/// 422 `generation_failed` : le code de la génération, avec les raisons de la retouche.
fn failed(reason: GenerationReason) -> Problem {
    Problem::with_details(
        ProblemCode::GenerationFailed,
        "no edit could be made for this route",
        ProblemDetails { reason: Some(reason), ..Default::default() },
    )
}

fn pose_problem(e: PoseError) -> Problem {
    match e {
        PoseError::Empty => failed(GenerationReason::RouteNotMatched),
        PoseError::OrderAnomaly { .. } => failed(GenerationReason::OrderAnomaly),
        PoseError::Drift => failed(GenerationReason::RouteMismatch),
        // Jamais attendus : les jambes viennent du graphe du service et les trois types sont servis.
        PoseError::NotOnGraph | PoseError::WrongType => Problem::internal(),
    }
}

/// Un `match` exhaustif, sans `_` : un échec ajouté au moteur casse la compilation du service.
fn problem_of(f: RetouchFailure) -> Problem {
    match f {
        RetouchFailure::Pose(e) => pose_problem(e),
        RetouchFailure::NoSeam => failed(GenerationReason::NoSeam),
        RetouchFailure::EndTooFar => failed(GenerationReason::EndTooFar),
        RetouchFailure::NoSeamReached => failed(GenerationReason::EndUnreachable),
        RetouchFailure::JunctionCut => failed(GenerationReason::JunctionCut),
        RetouchFailure::ChangeTooSmall => failed(GenerationReason::ChangeTooSmall),
        RetouchFailure::ChangeTooLarge => failed(GenerationReason::ChangeTooLarge),
        // Le cas normal : raccourcir réussit 57,2 % du temps, 38,9 % sur un aller simple.
        RetouchFailure::NoEconomy => failed(GenerationReason::NoEconomy),
        RetouchFailure::NoWindow => failed(GenerationReason::NoWindow),
        RetouchFailure::NoProposal { .. } => failed(GenerationReason::NoCandidate),
        RetouchFailure::AllRefused { .. } => failed(GenerationReason::CheckerRejected),
        RetouchFailure::OffTarget { .. } => failed(GenerationReason::OffTarget),
        RetouchFailure::Timeout => failed(GenerationReason::Budget),
        RetouchFailure::Stopped => timeout(),
        // Jamais attendus : le mode d'équipe n'est demandé qu'avec ses agrégats, l'échappement est rejoué, et la longueur
        // est bornée avant la file.
        RetouchFailure::NoTeamData | RetouchFailure::WindowEscape | RetouchFailure::TooLong => {
            Problem::internal()
        }
    }
}

/// Bande du changement demandé (`resize`), seule mesure de la distance visée qui soit journalisée.
fn change_band(original_m: f64, target_m: f64) -> &'static str {
    let share = (target_m - original_m) / original_m.max(1.0);
    match share {
        s if s < -0.30 => "<-30%",
        s if s < -0.10 => "-30..-10%",
        s if s < 0.0 => "-10..0%",
        s if s < 0.10 => "0..10%",
        s if s <= 0.30 => "10..30%",
        _ => ">30%",
    }
}

fn mode_name(m: Mode) -> &'static str {
    match m {
        Mode::Strict => "strict",
        Mode::TeamFirst => "team_first",
        Mode::Discovery => "discovery",
    }
}

fn kind_name(r: &Retouched) -> &'static str {
    match r.kind {
        RetouchKind::Shorten => "shorten",
        RetouchKind::Lengthen => "lengthen",
        RetouchKind::MoveEnd(MoveEndVariant::Best) => "move_end_best",
        RetouchKind::MoveEnd(MoveEndVariant::KeepAll) => "move_end_keep_all",
        RetouchKind::MoveEnd(MoveEndVariant::Nearest) => "move_end_nearest",
        RetouchKind::Truncate => "truncate",
    }
}

fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0 + 0.0
}

/// Ce qu'une requête a coûté et vu, pour sa ligne de journal : des comptes et des durées, rien de la requête.
#[derive(Clone, Copy, Debug)]
struct EditStats {
    mode: Mode,
    seams: usize,
    holes: u32,
    routes: usize,
    dropped: u32,
    match_ms: u64,
    pose_ms: u64,
    engine_ms: u64,
    check_ms: u64,
}

fn ms(t: Instant) -> u64 {
    u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn stage(name: &'static str, t: Instant) {
    metrics::histogram!("ord_generator_edit_stage_seconds", "stage" => name)
        .record(t.elapsed().as_secs_f64());
}

/// Les compteurs du moteur : les contrôles du recollé qui ont refusé un chemin, par code.
fn record_diagnostics(d: &RetouchDiagnostics) {
    for (violation, n) in &d.spliced_refused {
        metrics::counter!("ord_generator_edit_spliced_refused_total", "violation" => violation.code())
            .increment(u64::from(*n));
    }
}

// --- Géométrie rendue -----------------------------------------------------------------------------------------------

/// Des points sans doublon consécutif en E7, et l'indice où chaque point poussé se trouve.
struct Points {
    out: Vec<LatLon>,
    last: Option<(i32, i32)>,
}

impl Points {
    fn push(&mut self, p: LatLon) -> u32 {
        let e7 = p.to_e7();
        if self.last != Some(e7) {
            self.out.push(p);
            self.last = Some(e7);
        }
        u32::try_from(self.out.len() - 1).unwrap_or(u32::MAX)
    }
}

/// La polyligne complète : l'origine jusqu'à la coupe, le tronçon neuf (avec ses plages non revêtues, en indices de son
/// propre tracé), puis le reste de l'origine. Rend les points, les indices du premier et du dernier point du tronçon neuf
/// et ses plages non revêtues, **d'après le retrait des doublons**. Sans tronçon (`truncate`), les deux indices sont ceux du
/// dernier point.
fn assemble(
    head: &[LatLon],
    new: Option<(&[LatLon], &[[u32; 2]])>,
    tail: &[LatLon],
) -> (Vec<LatLon>, [u32; 2], Vec<[u32; 2]>) {
    let mut pts = Points { out: Vec::with_capacity(head.len() + tail.len()), last: None };
    for &p in head {
        pts.push(p);
    }
    let last_head = u32::try_from(pts.out.len().saturating_sub(1)).unwrap_or(u32::MAX);
    let (mut span, mut unpaved) = ([last_head, last_head], Vec::new());
    if let Some((new_pts, ranges)) = new.filter(|(p, _)| !p.is_empty()) {
        let at: Vec<u32> = new_pts.iter().map(|&p| pts.push(p)).collect();
        span = [at[0], at[at.len() - 1]];
        unpaved =
            ranges.iter().filter_map(|r| Some([*at.get(r[0] as usize)?, *at.get(r[1] as usize)?])).collect();
    }
    for &p in tail {
        pts.push(p);
    }
    (pts.out, span, unpaved)
}

/// Mètres des étendues non appariées du document (ramenées à la polyligne par `k = L / axe`) qui tombent dans les parties
/// gardées. Sans positions (`unmatched` absent), la somme des `trv` non comptés, plafonnée aux mètres gardés : une borne
/// haute.
fn unmatched_in_kept(dw: &DocWalk, original_m: f64, kept: &[(f64, f64)]) -> f64 {
    let Some(ext) = &dw.unmatched else {
        let kept_m: f64 = kept.iter().map(|(a, b)| (b - a).max(0.0)).sum();
        return dw.unmatched_trv_m.min(kept_m);
    };
    let k = if dw.axis_m > 0.0 { original_m / dw.axis_m } else { 1.0 };
    let mut total = 0.0;
    for &(a, b) in ext {
        let (a, b) = (a * k, b * k);
        for &(ka, kb) in kept {
            total += (b.min(kb) - a.max(ka)).max(0.0);
        }
    }
    total
}

/// Le plan joué : la fenêtre de `move_end`, ou le plan de `resize`.
enum Plan {
    Move { window: WindowPlan, end: LatLon },
    Resize(ResizePlan),
}

impl Plan {
    fn window(&self) -> &WindowPlan {
        match self {
            Plan::Move { window, .. } => window,
            Plan::Resize(p) => p.window(),
        }
    }
}

impl Ready {
    /// Retouche un parcours : la file des générations, son budget, ses fenêtres.
    pub(crate) async fn edit_route(
        self: &Arc<Self>,
        input: EditInput,
        creds: Option<Credentials>,
    ) -> Result<EditResponseV1, Problem> {
        let started = Instant::now();
        let deadline = started + Duration::from_millis(self.cfg.budget_ms);
        let ns8 = creds.as_ref().map(|c| c.ns8().to_string());
        // Ce qui se journalise, avant que le calcul prenne la demande : des étiquettes grossières, jamais de valeur.
        let operation = input.op.name();
        let ride = ride_name(input.ride);
        let distance_band = band(input.original_m);
        let change = match input.op {
            EditOp::Resize { target_m } => change_band(input.original_m, target_m),
            EditOp::MoveEnd { .. } => "",
        };
        let asked = if creds.is_some() { cursor::apply_route(input.cursor).mode } else { Mode::Discovery };
        let (outcome, stats) = self.edit_inner(input, creds, deadline).await;
        let stats = stats.unwrap_or(EditStats {
            mode: asked,
            seams: 0,
            holes: 0,
            routes: 0,
            dropped: 0,
            match_ms: 0,
            pose_ms: 0,
            engine_ms: 0,
            check_ms: 0,
        });
        let duration = started.elapsed();
        let mode = mode_name(stats.mode);
        metrics::histogram!("ord_generator_edit_duration_seconds", "operation" => operation, "ride_type" => ride)
            .record(duration.as_secs_f64());
        let (result, reason) = match &outcome {
            Ok(_) => ("ok", String::new()),
            Err(p) => {
                let reason = p
                    .details
                    .as_ref()
                    .and_then(|d| d.reason.map(|r| serde_json::to_value(r).ok()))
                    .flatten()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default();
                (p.code.as_str(), reason)
            }
        };
        metrics::counter!("ord_generator_edit_requests_total", "operation" => operation, "outcome" => result, "reason" => reason.clone())
            .increment(1);
        tracing::info!(
            operation,
            ride_type = ride,
            mode,
            distance_band,
            change_band = change,
            outcome = result,
            reason = reason.as_str(),
            routes = stats.routes,
            dropped = stats.dropped,
            seams = stats.seams,
            holes = stats.holes,
            match_ms = stats.match_ms,
            pose_ms = stats.pose_ms,
            engine_ms = stats.engine_ms,
            check_ms = stats.check_ms,
            duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
            ns8 = ns8.as_deref().unwrap_or(""),
            "route edit"
        );
        outcome
    }

    async fn edit_inner(
        self: &Arc<Self>,
        input: EditInput,
        creds: Option<Credentials>,
        deadline: Instant,
    ) -> (Result<EditResponseV1, Problem>, Option<EditStats>) {
        // Les bornes qui ne demandent aucun calcul de moteur, avant la file : aucune place prise.
        if let Err(p) = self.edit_precheck(&input) {
            return (Err(p), None);
        }
        let permit = match self.slot(deadline).await {
            Ok(p) => p,
            Err(p) => return (Err(p), None),
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let _guard = CancelOnDrop(Arc::clone(&cancel));
        let me = Arc::clone(self);
        let joined = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut stats = None;
            let result = me.compute_edit(&input, creds.as_ref(), deadline, &cancel, &mut stats);
            (result, stats)
        })
        .await;
        match joined {
            Ok((result, stats)) => (result, stats),
            Err(_) => (Err(Problem::internal()), None),
        }
    }

    /// Couverture et bornes du changement : jugées avec les réglages du moteur, pour que le bord exact rende la même raison
    /// que `plan_resize`.
    fn edit_precheck(&self, input: &EditInput) -> Result<(), Problem> {
        if let EditOp::MoveEnd { end } = input.op {
            if !self.in_coverage(end) {
                return Err(Problem::with_details(
                    ProblemCode::OutOfCoverage,
                    "the new end is outside the area the generator covers",
                    ProblemDetails { coverage: Some(CoverageSource::Graph), ..Default::default() },
                ));
            }
        }
        // Un seul point dehors suffit : une coupe au bord fausserait la pose sans le dire.
        if input.line.iter().any(|&p| !self.in_coverage(p)) {
            return Err(failed(GenerationReason::RouteOffCoverage));
        }
        if let EditOp::Resize { target_m } = input.op {
            let tune = RetouchTune::default();
            let big = target_m - input.original_m;
            if big.abs() < tune.min_change_m {
                return Err(failed(GenerationReason::ChangeTooSmall));
            }
            if big.abs() > tune.max_change * input.original_m {
                return Err(failed(GenerationReason::ChangeTooLarge));
            }
        }
        Ok(())
    }

    /// Le calcul, sur un fil bloquant : l'équipe, l'appariement du document, la pose, le moteur, le contrôleur, le rendu.
    fn compute_edit(
        &self,
        input: &EditInput,
        creds: Option<&Credentials>,
        deadline: Instant,
        cancel: &AtomicBool,
        out_stats: &mut Option<EditStats>,
    ) -> Result<EditResponseV1, Problem> {
        let entry: Option<Arc<Entry>> = match creds {
            Some(c) => match self.team_entry(c, deadline, cancel) {
                Ok(e) => Some(e),
                // Comme une génération : sans la mémoire de l'équipe, la réponse dépendrait de la charge du service.
                Err(ReadyError::Overloaded) => return Err(overloaded()),
                Err(ReadyError::Stopped) => return Err(timeout()),
                Err(ReadyError::Failed) => return Err(Problem::internal()),
                // Absente ou périmée : découverte, et la réponse le dit (`overlay: null`).
                Err(ReadyError::State(_)) => None,
            },
            None => None,
        };
        if Instant::now() >= deadline {
            return Err(timeout());
        }
        let applied: Applied =
            if entry.is_some() { cursor::apply_route(input.cursor) } else { cursor::discovery() };
        let mut stats = EditStats {
            mode: applied.mode,
            seams: 0,
            holes: 0,
            routes: 0,
            dropped: 0,
            match_ms: 0,
            pose_ms: 0,
            engine_ms: 0,
            check_ms: 0,
        };
        *out_stats = Some(stats);
        let halt = || cancel.load(Ordering::Relaxed) || Instant::now() >= deadline;

        // 1. Le document, apparié comme le dépôt : la marche que la surcouche a comptée.
        let t = Instant::now();
        let gf = self.engine.base();
        let Some(wide) = &self.cell_wide else { return Err(unavailable()) };
        let idx = CellIndex::with_wide(gf, self.cfg.edit_index_cache_mb * 1024 * 1024, Arc::clone(wide));
        let Some(dw) = doc_walk(gf, &idx, &self.poly, &input.edges, input.doc_len, &halt) else {
            return Err(timeout());
        };
        drop(idx);
        stats.match_ms = ms(t);
        stage("match", t);
        if dw.walk.legs.is_empty() {
            *out_stats = Some(stats);
            return Err(failed(GenerationReason::RouteNotMatched));
        }

        // 2. La pose, sur la polyligne reçue.
        let t = Instant::now();
        let (legs, joins) = posed_parts(&dw.walk);
        let pin = PosedInput { ride_type: input.ride, legs: &legs, joins: &joins, line: &input.line };
        let posed = PosedRoute::pose(&self.engine, &pin, &PoseTune::default());
        stats.pose_ms = ms(t);
        stage("pose", t);
        let posed = match posed {
            Ok(p) => p,
            Err(e) => {
                *out_stats = Some(stats);
                return Err(pose_problem(e));
            }
        };
        stats.seams = posed.seams().len();
        stats.holes = posed.stats().holes;
        *out_stats = Some(stats);

        // 3. Le moteur.
        let tune = RetouchTune::default();
        let rreq = RetouchRequest {
            ride_type: input.ride,
            mode: applied.mode,
            familiarity: applied.familiarity,
            off_team_cost_q8: applied.off_team_cost_q8,
            seed: input.seed,
        };
        let t = Instant::now();
        let mut stop = || halt();
        let retouch =
            self.retouch_windowed(&posed, input.op, &rreq, &tune, entry.as_deref(), deadline, &mut stop);
        stats.engine_ms = ms(t);
        stage("engine", t);
        *out_stats = Some(stats);
        let retouch = retouch?;
        record_diagnostics(&retouch.diagnostics);

        // 4. Le contrôleur indépendant, sur chaque tronçon neuf.
        let t = Instant::now();
        let (routes, dropped) = self.check_and_render(input, &posed, &dw, &retouch, &rreq, entry.as_deref());
        stats.check_ms = ms(t);
        stage("check", t);
        stats.routes = routes.len();
        stats.dropped = dropped;
        *out_stats = Some(stats);
        if routes.is_empty() {
            return Err(failed(GenerationReason::CheckerRejected));
        }
        Ok(EditResponseV1 {
            graph_id: self.graph_id.clone(),
            mode: mode_v1(applied.mode),
            overlay: entry
                .as_ref()
                .map(|e| OverlayInfoV1 { deposited_at: super::iso8601(e.meta.deposited_at) }),
            taste: if entry.as_ref().is_some_and(|e| e.profile.is_some()) {
                TasteV1::Team
            } else {
                TasteV1::Common
            },
            operation: match input.op {
                EditOp::Resize { .. } => EditOperationKindV1::Resize,
                EditOp::MoveEnd { .. } => EditOperationKindV1::MoveEnd,
            },
            original_m: r1(posed.length_m()),
            target_m: match input.op {
                EditOp::Resize { target_m } => Some(r1(target_m)),
                EditOp::MoveEnd { .. } => None,
            },
            dropped,
            routes,
        })
    }

    /// Le moteur dans une fenêtre du cache : une fois, puis, si la recherche touche le bord de la fenêtre (le garde-fou,
    /// jamais attendu), une fois de plus dans une fenêtre élargie de la plus longue chaîne qui touche les tuiles d'accroche
    /// et de 10 km, comme une génération ; un second échappement rend une erreur interne.
    #[allow(clippy::too_many_arguments)]
    fn retouch_windowed(
        &self,
        posed: &PosedRoute,
        op: EditOp,
        rreq: &RetouchRequest,
        tune: &RetouchTune,
        entry: Option<&Entry>,
        deadline: Instant,
        stop: &mut dyn FnMut() -> bool,
    ) -> Result<Retouch, Problem> {
        let mut extra_m = 0.0;
        for attempt in 0..2 {
            let step = self.cfg.window_radius_step_m;
            let plan = match op {
                EditOp::MoveEnd { end } => {
                    Plan::Move { window: self.engine.plan_move_end(posed, end, tune, step, extra_m), end }
                }
                EditOp::Resize { target_m } => {
                    match self.engine.plan_resize(posed, target_m, rreq, tune, step, extra_m) {
                        Ok(p) => Plan::Resize(p),
                        Err(f) => return Err(problem_of(f)),
                    }
                }
            };
            let team = entry.map(|e| TeamRef { serial: e.serial, data: &e.team, taste: e.profile.as_ref() });
            let held = match self.windows.acquire(&self.engine, plan.window(), team, deadline, stop) {
                Ok(h) => h,
                Err(AcquireError::Stopped) => return Err(timeout()),
                Err(AcquireError::Overloaded) => return Err(overloaded()),
            };
            let mut ws = self.take_workspace();
            let ctx = entry.zip(held.wt.as_deref()).map(|(e, wt)| TeamCtx { data: &e.team, win: wt });
            let result = match &plan {
                Plan::Move { end, .. } => {
                    move_end_in(&self.engine, &held.win, ctx, posed, *end, rreq, tune, &mut ws, stop)
                }
                Plan::Resize(p) => {
                    resize_in(&self.engine, &held.win, ctx, posed, p, rreq, tune, &mut ws, stop)
                }
            };
            self.give_workspace(ws);
            self.windows.release(held);
            match result {
                Ok(r) => return Ok(r),
                Err(refused) if refused.failure == RetouchFailure::WindowEscape => {
                    metrics::counter!("ord_generator_edit_window_escapes_total").increment(1);
                    tracing::warn!(attempt, "route edit window escape");
                    extra_m = plan.window().snap_lmax_m + 10_000.0;
                }
                Err(refused) => {
                    record_diagnostics(&refused.diagnostics);
                    return Err(problem_of(refused.failure));
                }
            }
        }
        Err(Problem::internal())
    }

    /// Chaque proposition passe le contrôleur indépendant sur son tronçon neuf seul, puis se rend. Les refusées sont
    /// comptées.
    #[allow(clippy::too_many_arguments)]
    fn check_and_render(
        &self,
        input: &EditInput,
        posed: &PosedRoute,
        dw: &DocWalk,
        retouch: &Retouch,
        rreq: &RetouchRequest,
        entry: Option<&Entry>,
    ) -> (Vec<EditedRouteV1>, u32) {
        let base = self.engine.base();
        let checker = Checker::with_tables(
            base,
            entry.map(|e| &e.overlay),
            super::check_config(&self.engine),
            &self.graph_index,
            entry.map(|e| &e.check),
        );
        let proto = EngineRequest {
            start: input.line[0],
            end: None,
            bearing: None,
            distance_m: 0.0,
            ride_type: input.ride,
            mode: rreq.mode,
            familiarity: rreq.familiarity,
            off_team_cost_q8: rreq.off_team_cost_q8,
            seed: rreq.seed,
            figures: Figures::Classic,
        };
        let node = |k: Option<usize>| k.map(|k| LatLon::from_e7(base.node_pos(posed.seams()[k].node)));
        let big = match input.op {
            EditOp::Resize { target_m } => target_m - posed.length_m(),
            EditOp::MoveEnd { .. } => 0.0,
        };
        let (mut routes, mut dropped) = (Vec::new(), 0u32);
        let corrupt_first = self.hooks.corrupt_first_route.load(Ordering::Relaxed);
        for (i, r) in retouch.proposals.iter().enumerate() {
            let report = if r.kind == RetouchKind::Truncate {
                // Pas de tronçon : rien à contrôler.
                None
            } else {
                let mut route = r.new.clone();
                if (self.hooks.corrupt_routes.load(Ordering::Relaxed) || (corrupt_first && i == 0))
                    && route.steps.len() > 1
                {
                    route.steps.remove(0);
                }
                let (end, distance_m) = match (input.op, r.to_seam) {
                    (EditOp::MoveEnd { end }, _) => (Some(end), r.new.length_m),
                    (EditOp::Resize { .. }, to) => (node(to), (r.to_s_m - r.from_s_m) + big),
                };
                let req = EngineRequest {
                    start: node(r.from_seam).unwrap_or(proto.start),
                    end,
                    distance_m: distance_m.max(1.0),
                    ..proto.clone()
                };
                let report = checker.check(&req, &route);
                if !report.ok() {
                    dropped += 1;
                    let codes: Vec<&str> = report.violations.iter().map(|v| v.code()).collect();
                    for code in &codes {
                        metrics::counter!("ord_generator_edit_routes_rejected_total", "violation" => (*code))
                            .increment(1);
                    }
                    tracing::warn!(
                        violations = codes.join(",").as_str(),
                        ride_type = ride_name(input.ride),
                        "edited route rejected by the independent checker"
                    );
                    continue;
                }
                Some(report)
            };
            metrics::counter!("ord_generator_edit_routes_total", "kind" => kind_name(r)).increment(1);
            routes.push(self.render_edit(input, posed, dw, r, report.as_ref()));
        }
        (routes, dropped)
    }

    fn render_edit(
        &self,
        input: &EditInput,
        posed: &PosedRoute,
        dw: &DocWalk,
        r: &Retouched,
        report: Option<&ord_loops_check::Report>,
    ) -> EditedRouteV1 {
        let l = posed.length_m();
        let head = posed.line_between(0.0, r.from_s_m);
        let truncated = r.kind == RetouchKind::Truncate;
        let rejoins = matches!(r.kind, RetouchKind::Shorten | RetouchKind::Lengthen);
        let tail = if rejoins { posed.line_between(r.to_s_m, l) } else { Vec::new() };
        let new = (!truncated).then(|| route_geometry_spans(self.engine.base(), &r.new));
        let (points, span, unpaved) =
            assemble(&head, new.as_ref().map(|(p, u)| (p.as_slice(), u.as_slice())), &tail);
        let polyline = encode_polyline6(&points.iter().map(|p| [p.lat, p.lon]).collect::<Vec<_>>());
        let mut kept_parts = vec![(0.0, r.from_s_m)];
        if rejoins {
            kept_parts.push((r.to_s_m, l));
        }
        let new_m = if truncated { 0.0 } else { r.new.length_m };
        let (team, bridged, offroad, crossings) = report
            .map_or((0.0, 0.0, 0.0, 0), |rep| (rep.team_m, rep.bridged_m, rep.offroad_m, rep.crossings));
        // Les parts se disent sur le tronçon neuf (`new_m`), comme le contrat.
        let share = |x: f64| if new_m > 0.0 { r3(x / new_m) } else { 0.0 };
        let end_snap = match input.op {
            EditOp::MoveEnd { end } => points.last().map(|p| r1(hav(end, *p))),
            EditOp::Resize { .. } => None,
        };
        EditedRouteV1 {
            kind: match r.kind {
                RetouchKind::Shorten => EditKindV1::Shorten,
                RetouchKind::Lengthen => EditKindV1::Lengthen,
                RetouchKind::MoveEnd(_) => EditKindV1::MoveEnd,
                RetouchKind::Truncate => EditKindV1::Truncate,
            },
            variant: match r.kind {
                RetouchKind::MoveEnd(MoveEndVariant::Best) => Some(MoveEndVariantV1::Best),
                RetouchKind::MoveEnd(MoveEndVariant::KeepAll) => Some(MoveEndVariantV1::KeepAll),
                RetouchKind::MoveEnd(MoveEndVariant::Nearest) => Some(MoveEndVariantV1::Nearest),
                _ => None,
            },
            polyline,
            distance_m: r1(r.length_m),
            error_share: r.err.map(r3),
            kept_m: r1(r.kept_m),
            kept_share: r3(r.kept_m / l.max(1.0)),
            abandoned_m: r.abandon_m.map(r1),
            end_snap_m: end_snap,
            unmatched_m: r1(unmatched_in_kept(dw, l, &kept_parts)),
            changed: ChangedV1 {
                from_m: r1(r.from_s_m),
                to_m: r1(r.to_s_m),
                new_m: r1(new_m),
                points: span,
                team_m: r1(team),
                bridged_m: r1(bridged),
                team_share: share(team + bridged),
                offroad_m: r1(offroad),
                offroad_share: share(offroad),
                unpaved,
                crossings,
            },
        }
    }
}

// Retouche d'un parcours.
#[utoipa::path(
    post,
    path = "/v1/generator/routes/edit",
    summary = "Edit a saved route",
    description = "Changes a saved route and keeps the rest of it exactly as it was drawn. `resize` aims at a total \
distance by replacing one stretch of the route by a shorter or longer one that leaves and rejoins it at a junction; \
`move_end` keeps the route up to a junction and leads from there to a new end (or cuts the route when the new end is on \
it). The route is the `polyline` and the `network_edges` document of its track (the edges the matcher found along it, as \
the overlay deposit takes them): the service matches the document to its graph the way a deposit does, so a route that \
cannot be matched is refused. Up to three routes are returned, each with the whole polyline, the stretch that changed \
(`changed`) and the share of the saved route that was kept. A refusal is an answer, not a failure. Measured on recorded tracks: moving the end succeeds in 98.4 percent of \
cases, lengthening in 96.1 percent, shortening in 57.2 percent (38.9 percent for a one-way ride; most often the reason is \
`no_economy`, otherwise `off_target`, `checker_rejected`, `no_seam` or `order_anomaly`), and 5 percent of tracks cannot be posed at all (reason `order_anomaly`). The change \
of a `resize` is bounded (`capabilities.generator.edit`): beyond, generate a new route. Without the team headers the \
edit is a discovery with the common taste; with them, `cursor` maps to the engine as for a route from A to B, and an \
absent or stale overlay does not block (the answer says `mode` and `overlay: null`). The engine's P90 for a `resize` is 1.1 \
seconds, without the matching of the document. Edits share the queue and the budget \
of the generations. Nothing from the request is logged.",
    operation_id = "postGeneratorRoutesEdit",
    tag = "generator",
    params(
        ("Ord-Namespace-Key" = Option<String>, Header, description = "Environment secret; with `Ord-Team`, weights the edit with the team overlay. Never logged."),
        ("Ord-Team" = Option<String>, Header, description = "Opaque team reference, [A-Za-z0-9_-]{1,64}; with `Ord-Namespace-Key`. Never logged.")
    ),
    request_body(content = EditRequestV1, content_type = "application/json"),
    responses(
        (status = 200, description = "Edited routes", body = EditResponseV1),
        (status = 400, description = "Invalid input (fields listed by JSON pointer, headers by `/headers/<name>`)", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 413, description = "Request body above `capabilities.generator.edit.request_body_bytes`, more points than `points_max`, or a route longer than `limits.distance_m_max` (`details.limit`)", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 415, description = "Content-Type is not application/json", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 422, description = "Out of coverage (`out_of_coverage`, `details.coverage = graph`: the new end) or no edit (`generation_failed`, `details.reason`: `change_too_small`, `change_too_large`, `route_off_coverage`, `route_not_matched`, `order_anomaly`, `route_mismatch`, `no_seam`, `end_too_far`, `end_unreachable`, `junction_cut`, `no_economy`, `no_window`, `no_candidate`, `checker_rejected`, `off_target` or `budget`)", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 500, description = "Unexpected error", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
        (status = 503, description = "Generator unavailable or without a per-cell spatial index (`data_unavailable`, `details.source = graph`) or queue full (`overloaded`); with `Retry-After`", body = crate::problem::ProblemBody, content_type = "application/problem+json",
            headers(("Retry-After" = u64, description = "Seconds to wait before retrying"))),
        (status = 504, description = "Generation budget exceeded (`timeout`)", body = crate::problem::ProblemBody, content_type = "application/problem+json"),
    )
)]
pub(crate) async fn post_edit(State(state): State<AppState>, req: Request) -> Result<Response, Problem> {
    let ready = Arc::clone(state.generator().ready().ok_or_else(unavailable)?);
    // Sans index par cellule, apparier le document bâtirait l'index entier à chaque requête : la retouche est éteinte.
    if ready.cell_wide.is_none() {
        return Err(unavailable());
    }
    let headers = req.headers().clone();
    let body: EditRequestV1 = read_json(req, EDIT_BODY_MAX).await?;
    let input = validate(&body, &ready)?;
    drop(body);
    // Facultatifs, mais validés dès que l'un est là : une clé sans équipe, ou mal formée, est un 400.
    let creds = if headers.contains_key(KEY_HEADER) || headers.contains_key(TEAM_HEADER) {
        Some(credentials(&headers, true)?)
    } else {
        None
    };
    ready.edit_route(input, creds).await.map(|r| Json(r).into_response())
}

/// Chemins des handlers pour la dérive OpenAPI.
pub(crate) mod paths {
    pub(crate) use super::__path_post_edit;
}

#[cfg(test)]
mod tests {
    use ord_loops::Violation;

    use super::*;

    fn reason(f: RetouchFailure) -> Option<GenerationReason> {
        problem_of(f).details.and_then(|d| d.reason)
    }

    /// Chaque échec du moteur a son statut et sa raison (la table du §15.5).
    #[test]
    fn every_engine_failure_has_its_status_and_reason() {
        use GenerationReason as R;
        let table = [
            (RetouchFailure::NoSeam, R::NoSeam),
            (RetouchFailure::EndTooFar, R::EndTooFar),
            (RetouchFailure::NoSeamReached, R::EndUnreachable),
            (RetouchFailure::JunctionCut, R::JunctionCut),
            (RetouchFailure::ChangeTooSmall, R::ChangeTooSmall),
            (RetouchFailure::ChangeTooLarge, R::ChangeTooLarge),
            (RetouchFailure::NoEconomy, R::NoEconomy),
            (RetouchFailure::NoWindow, R::NoWindow),
            (RetouchFailure::NoProposal { dominant: None }, R::NoCandidate),
            (RetouchFailure::AllRefused { dominant: Violation::SharpTurn }, R::CheckerRejected),
            (RetouchFailure::OffTarget { best_err: 0.5 }, R::OffTarget),
            (RetouchFailure::Timeout, R::Budget),
            (RetouchFailure::Pose(PoseError::Empty), R::RouteNotMatched),
            (RetouchFailure::Pose(PoseError::OrderAnomaly { seams: 2 }), R::OrderAnomaly),
            (RetouchFailure::Pose(PoseError::Drift), R::RouteMismatch),
        ];
        for (failure, expected) in table {
            let p = problem_of(failure);
            assert_eq!(p.code, ProblemCode::GenerationFailed, "{failure:?}");
            assert_eq!(p.details.and_then(|d| d.reason), Some(expected), "{failure:?}");
        }
        assert_eq!(problem_of(RetouchFailure::Stopped).code, ProblemCode::Timeout);
        for internal in [
            RetouchFailure::NoTeamData,
            RetouchFailure::WindowEscape,
            RetouchFailure::TooLong,
            RetouchFailure::Pose(PoseError::NotOnGraph),
            RetouchFailure::Pose(PoseError::WrongType),
        ] {
            assert_eq!(problem_of(internal).code, ProblemCode::Internal, "{internal:?}");
            assert_eq!(reason(internal), None);
        }
    }

    #[test]
    fn a_problem_never_carries_a_value_of_the_request() {
        let p = problem_of(RetouchFailure::OffTarget { best_err: 0.123_456 });
        let text = serde_json::to_string(&p.body()).unwrap();
        assert!(!text.contains("0.123"), "{text}");
    }

    #[test]
    fn the_change_bands_are_coarse_and_cover_every_change() {
        assert_eq!(change_band(10_000.0, 7_000.0), "-30..-10%");
        assert_eq!(change_band(10_000.0, 9_500.0), "-10..0%");
        assert_eq!(change_band(10_000.0, 10_500.0), "0..10%");
        assert_eq!(change_band(10_000.0, 12_000.0), "10..30%");
        assert_eq!(change_band(10_000.0, 13_000.0), "10..30%");
        assert_eq!(change_band(10_000.0, 13_001.0), ">30%");
        assert_eq!(change_band(10_000.0, 6_900.0), "<-30%");
    }

    fn pts(v: &[(i32, i32)]) -> Vec<LatLon> {
        v.iter().map(|&p| LatLon::from_e7(p)).collect()
    }

    #[test]
    fn a_repeated_point_is_dropped_and_the_indices_follow() {
        let head = pts(&[(10, 10), (20, 20), (30, 30)]);
        // Le tronçon neuf commence au dernier point de la tête (doublon), puis deux points ; la queue en répète un.
        let new = pts(&[(30, 30), (40, 40), (50, 50)]);
        let tail = pts(&[(50, 50), (60, 60)]);
        let unpaved = [[1u32, 2u32]];
        let (out, span, ranges) = assemble(&head, Some((&new, &unpaved)), &tail);
        let e7: Vec<(i32, i32)> = out.iter().map(|p| p.to_e7()).collect();
        assert_eq!(e7, vec![(10, 10), (20, 20), (30, 30), (40, 40), (50, 50), (60, 60)]);
        assert_eq!(span, [2, 4]);
        assert_eq!(ranges, vec![[3, 4]]);
        // Sans tronçon : les deux indices sont ceux du dernier point.
        let (out, span, ranges) = assemble(&head, None, &[]);
        assert_eq!((out.len(), span, ranges.is_empty()), (3, [2, 2], true));
    }

    fn walk(unmatched: Option<Vec<(f64, f64)>>, trv: f64, axis: f64) -> DocWalk {
        DocWalk {
            walk: ord_graph_match::walk::Walk { legs: Vec::new(), joins: Vec::new() },
            unmatched,
            unmatched_trv_m: trv,
            axis_m: axis,
        }
    }

    #[test]
    fn unmatched_metres_are_those_inside_the_kept_parts() {
        // Axe du document de 1 000 m pour une polyligne de 2 000 m : k = 2.
        let dw = walk(Some(vec![(100.0, 200.0), (450.0, 550.0)]), 0.0, 1_000.0);
        // Parties gardées [0, 300] et [1 000, 2 000] : le premier trou (200 à 400 m) y est entier, le second (900 à 1 100)
        // n'y est que de 1 000 à 1 100.
        let kept = [(0.0, 300.0), (1_000.0, 2_000.0)];
        let m = unmatched_in_kept(&dw, 2_000.0, &kept);
        assert!((m - (100.0 + 100.0)).abs() < 1e-9, "{m}");
        // Dans la partie remplacée seulement : rien.
        assert_eq!(unmatched_in_kept(&dw, 2_000.0, &[(400.0, 900.0)]), 0.0);
        // Sans positions : la somme des `trv`, plafonnée aux mètres gardés.
        let blind = walk(None, 700.0, 1_000.0);
        assert_eq!(unmatched_in_kept(&blind, 2_000.0, &kept), 700.0);
        assert_eq!(unmatched_in_kept(&blind, 2_000.0, &[(0.0, 300.0)]), 300.0);
    }

    #[test]
    fn the_capabilities_come_from_the_engine_tune() {
        let c = EditCapabilities::new();
        let t = RetouchTune::default();
        assert_eq!(
            (c.change_share_max, c.change_m_min, c.tolerance, c.end_snap_m),
            (t.max_change, t.min_change_m, t.tolerance, t.end_snap_m)
        );
        assert_eq!((c.proposals_max, c.points_max, c.request_body_bytes), (3, 50_000, 4 * 1024 * 1024));
    }
}
