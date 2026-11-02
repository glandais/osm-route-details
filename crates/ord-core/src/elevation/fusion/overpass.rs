//! Pont au-dessus de la trace au sol (T04 §3.4, règles 2026.11.6). Le MNT LiDAR HD de l'IGN est
//! interpolé sous les tabliers : sous un pont qui enjambe la voie suivie, il lit une bosse (remblais
//! d'accès et culées, classés sol) au lieu de la chaussée. Quand les tuiles montrent un pont qui
//! croise la trace (graphe par niveaux, T03 §4.2 : relié au sol par ses seules culées) et que la
//! trace y est au sol, la bosse est remplacée par la droite entre les approches de part et d'autre.
//!
//! Garde-fous :
//! - trace au sol au croisement : tronçon apparié à une voie de niveau sol (ou voie fermée au vélo
//!   suivie, de niveau sol), hors de tout élément d'ouvrage ; aucun passage d'ouvrage OSM dans la
//!   fenêtre (la trace sur un pont garde la correction de son pont) ; aucune plage sans typologie ;
//! - angle d'au moins `min_angle_deg` (en deçà, la trace longe le pont) ; croisement à plus de
//!   `end_clearance_m` d'une extrémité réelle du pont (sinon c'est sa culée) ;
//! - fenêtre bornée par la géométrie : emprise du tablier sur la trace
//!   `deck_half_width_m / sin(angle)` de chaque côté, plus la sonde `abutment_probe_min_m` ;
//!   croisements dont les fenêtres se touchent (chaussées séparées) réunis ;
//! - terrain IGN LiDAR HD sans repli de l'approche à l'approche (le défaut est celui de ce MNT) ;
//!   toutes les stations encore `dem` (un ouvrage, un tablier détecté, ne sont jamais réécrits) ;
//! - seule la bosse est retirée : une station sous la droite (cuvette, passage creusé) garde son
//!   altitude ; rien n'est corrigé si la bosse ne dépasse pas `min_bump_m` (vraie route, bruit
//!   du MNT).
//!
//! Écriture : altitude seule, méthode `gap_filled`, avertissement `elevation_overpass_corrected`
//! sur la plage réécrite ; ni ouvrage ni changement de nature du segment.

use ord_domain::elevation::{TerrainOrigin, TerrainSample};
use ord_domain::report::{Warning, WarningCode};
use ord_domain::route::PreparedRoute;

use crate::classification::LevelKind;
use crate::elevation::EleMethod;
use crate::rules::Rules;
use crate::typology::{Overpass, RunMatch, TypologyOutput};

/// La trace est-elle au sol, hors élément d'ouvrage, à la distance `d` ?
fn on_ground(typo: &TypologyOutput, d: f64) -> bool {
    let Some(run) = typo.runs.iter().find(|r| r.from_m <= d && d <= r.to_m) else { return false };
    let class = match run.matched {
        RunMatch::Matched { class, element: None } | RunMatch::OffGraph { class } => class,
        _ => return false,
    };
    typo.classes.get(class as usize).is_some_and(|c| c.classified.level.kind == LevelKind::Ground)
}

/// Ponts des tuiles qui enjambent la trace au sol : angle d'au moins `min_angle_deg`, croisement
/// à plus de `end_clearance_m` d'une culée, trace au sol hors élément d'ouvrage au croisement.
/// Lu par la correction (ci-dessous) et par les points de rupture (`overpass_crossing`, T04 §6.1).
pub fn crossings_below<'a>(typo: &'a TypologyOutput, rules: &Rules) -> impl Iterator<Item = &'a Overpass> {
    let r = rules.elevation.structures.overpass.clone();
    typo.overpasses.iter().filter(move |o| {
        o.angle_deg >= r.min_angle_deg && o.end_m > r.end_clearance_m && on_ground(typo, o.d_m)
    })
}

/// Station du parcours la plus proche de `x`, à moins de `tol`.
fn station_at(route: &PreparedRoute, x: f64, tol: f64) -> Option<usize> {
    let s = &route.samples;
    let i = s.partition_point(|p| p.d_m < x);
    [i.checked_sub(1), (i < s.len()).then_some(i)]
        .into_iter()
        .flatten()
        .min_by(|&a, &b| (s[a].d_m - x).abs().total_cmp(&(s[b].d_m - x).abs()))
        .filter(|&k| (s[k].d_m - x).abs() <= tol)
}

/// Corrige les bosses du MNT sous les ponts qui enjambent la trace ; renvoie les fenêtres
/// corrigées `(approche amont, approche aval)`, en distance de parcours.
pub fn correct_overpasses(
    typo: &TypologyOutput,
    route: &PreparedRoute,
    terrain: &[TerrainSample],
    rules: &Rules,
    ele: &mut [f64],
    method: &mut [EleMethod],
    warnings: &mut Vec<Warning>,
) -> Vec<(f64, f64)> {
    let r = &rules.elevation.structures.overpass;
    let probe = rules.elevation.structures.abutment_probe_min_m;
    let step = rules.elevation.station_step_m;
    // Fenêtres des croisements retenus, réunies quand elles se touchent.
    let mut windows: Vec<(f64, f64)> = Vec::new();
    for o in crossings_below(typo, rules) {
        let half = r.deck_half_width_m / o.angle_deg.to_radians().sin() + probe;
        let (lo, hi) = (o.d_m - half, o.d_m + half);
        match windows.last_mut() {
            Some(w) if lo <= w.1 => *w = (w.0.min(lo), w.1.max(hi)),
            _ => windows.push((lo, hi)),
        }
    }
    let gaps: Vec<(f64, f64)> =
        typo.coverage.missing.iter().chain(&typo.coverage.unavailable).map(|g| (g.start, g.end)).collect();
    let ign = |t: &TerrainSample| t.origin == TerrainOrigin::IgnLidarHd && !t.fallback_after_error;
    let mut done = Vec::new();
    for (lo, hi) in windows {
        if typo
            .passages
            .iter()
            .map(|p| (p.from_m, p.to_m))
            .chain(gaps.iter().copied())
            .any(|s| s.1 > lo && s.0 < hi)
        {
            continue;
        }
        let (Some(pa), Some(pb)) = (station_at(route, lo, step), station_at(route, hi, step)) else {
            continue;
        };
        if pb <= pa + 1
            || !(pa..=pb).all(|i| method[i] == EleMethod::Dem && ele[i].is_finite() && ign(&terrain[i]))
        {
            continue;
        }
        let (da, db) = (route.samples[pa].d_m, route.samples[pb].d_m);
        let (za, zb) = (ele[pa], ele[pb]);
        let chord = |i: usize| za + (zb - za) * (route.samples[i].d_m - da) / (db - da);
        let bump = (pa + 1..pb).map(|i| ele[i] - chord(i)).fold(f64::NEG_INFINITY, f64::max);
        if bump.is_nan() || bump <= r.min_bump_m {
            continue;
        }
        let mut span: Option<(f64, f64)> = None;
        for i in pa + 1..pb {
            let z = chord(i);
            if ele[i] > z {
                ele[i] = z;
                method[i] = EleMethod::GapFilled;
                let d = route.samples[i].d_m;
                span = Some(span.map_or((d, d), |(a, _)| (a, d)));
            }
        }
        if let Some((a, b)) = span {
            warnings.push(Warning {
                code: WarningCode::ElevationOverpassCorrected,
                message: "terrain model reads a bridge deck above the road; elevation set to the line between the approaches"
                    .into(),
                range: Some(a..b),
            });
            done.push((da, db));
        }
    }
    done
}
