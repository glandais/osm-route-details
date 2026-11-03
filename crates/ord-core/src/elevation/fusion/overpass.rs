//! Pont au-dessus de la trace au sol (T04 §3.4, règles 2026.11.6). Le MNT LiDAR HD de l'IGN est
//! interpolé sous les tabliers (Mapterhorn aussi, sur ses sources mesurées) : sous un pont qui enjambe la voie suivie, il lit une bosse (remblais
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
//!   `deck_half_width_m / sin(angle)` de chaque côté, plus la sonde `abutment_probe_min_m` sur
//!   l'IGN, `mapterhorn_probe_m` dès qu'une station de la fenêtre est lue ailleurs (règles
//!   2026.11.11 : Mapterhorn z12 étale la bosse d'un pixel de 12 à 14 m) ; croisements dont les
//!   fenêtres se touchent (chaussées séparées) réunis ;
//! - fenêtre rognée (règles 2026.11.11) du côté d'un ouvrage emprunté par la trace avant ou après
//!   le croisement : elle s'arrête à la fin de son passage OSM et de sa rampe (stations déjà
//!   corrigées, `interpolated_structure` ou `dsm_deck`), l'approche est la première station `dem`
//!   au-delà ; écartée si un passage OSM ou une station corrigée touche le croisement lui-même ;
//! - terrain mesuré de l'approche à l'approche : IGN LiDAR HD ou Mapterhorn mesuré (repli
//!   compris, règles 2026.11.11 : Mapterhorn lit la même bosse, lissée), jamais GLO-30 ; toutes
//!   les stations encore `dem` (un ouvrage, un tablier détecté, ne sont jamais réécrits) ;
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

/// Terrain où la bosse se corrige : IGN LiDAR HD ou Mapterhorn mesuré (repli compris).
fn measured(t: &TerrainSample) -> bool {
    matches!(t.origin, TerrainOrigin::IgnLidarHd | TerrainOrigin::MapterhornMeasured)
}

fn ign(t: &TerrainSample) -> bool {
    t.origin == TerrainOrigin::IgnLidarHd && !t.fallback_after_error
}

/// Station déjà corrigée par un ouvrage (passage, rampe ou marge de sonde).
fn corrected(m: EleMethod) -> bool {
    matches!(m, EleMethod::DsmDeck | EleMethod::InterpolatedStructure)
}

/// Fenêtre d'un ou plusieurs croisements réunis : bornes et premier et dernier croisement.
#[derive(Clone, Copy, Debug)]
struct Window {
    lo: f64,
    hi: f64,
    first: f64,
    last: f64,
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
    let probe_ign = rules.elevation.structures.abutment_probe_min_m;
    let step = rules.elevation.station_step_m;
    let d = |i: usize| route.samples[i].d_m;
    // Fenêtres des croisements retenus, réunies quand elles se touchent. Sonde IGN si toute la
    // fenêtre (sonde IGN comprise) est lue à l'IGN, sinon sonde Mapterhorn.
    let mut windows: Vec<Window> = Vec::new();
    for o in crossings_below(typo, rules) {
        let deck = r.deck_half_width_m / o.angle_deg.to_radians().sin();
        let (a, b) = (o.d_m - deck - probe_ign, o.d_m + deck + probe_ign);
        let start = route.samples.partition_point(|x| x.d_m < a - step);
        let all_ign =
            (start..route.samples.len()).take_while(|&i| d(i) <= b + step).all(|i| ign(&terrain[i]));
        let probe = if all_ign { probe_ign } else { r.mapterhorn_probe_m };
        let w = Window { lo: o.d_m - deck - probe, hi: o.d_m + deck + probe, first: o.d_m, last: o.d_m };
        match windows.last_mut() {
            Some(x) if w.lo <= x.hi => {
                *x = Window {
                    lo: x.lo.min(w.lo),
                    hi: x.hi.max(w.hi),
                    first: x.first.min(w.first),
                    last: x.last.max(w.last),
                }
            }
            _ => windows.push(w),
        }
    }
    let gaps: Vec<(f64, f64)> =
        typo.coverage.missing.iter().chain(&typo.coverage.unavailable).map(|g| (g.start, g.end)).collect();
    let mut done = Vec::new();
    'windows: for mut w in windows {
        if gaps.iter().any(|g| g.1 > w.lo && g.0 < w.hi) {
            continue;
        }
        // Passage OSM dans la fenêtre : écartée s'il touche les croisements, rognée sinon (approche
        // à la première station strictement hors du passage).
        let (mut cut_lo, mut cut_hi) = (false, false);
        for p in &typo.passages {
            if !(p.to_m > w.lo && p.from_m < w.hi) {
                continue;
            }
            if p.to_m < w.first {
                (w.lo, cut_lo) = (w.lo.max(p.to_m), true);
            } else if p.from_m > w.last {
                (w.hi, cut_hi) = (w.hi.min(p.from_m), true);
            } else {
                continue 'windows;
            }
        }
        let n = route.samples.len();
        let pa = if cut_lo {
            Some(route.samples.partition_point(|x| x.d_m <= w.lo)).filter(|&i| i < n)
        } else {
            station_at(route, w.lo, step)
        };
        let pb = if cut_hi {
            route.samples.partition_point(|x| x.d_m < w.hi).checked_sub(1)
        } else {
            station_at(route, w.hi, step)
        };
        let (Some(mut pa), Some(mut pb)) = (pa, pb) else {
            continue;
        };
        // Rampe d'un ouvrage emprunté (stations déjà corrigées) : approche reculée à la première
        // station `dem` au-delà ; écartée si la rampe atteint un croisement.
        if let Some(i) = (pa..=pb).filter(|&i| d(i) < w.first && corrected(method[i])).max() {
            pa = i + 1;
        }
        if let Some(i) = (pa..=pb).filter(|&i| d(i) > w.last && corrected(method[i])).min() {
            pb = i.saturating_sub(1);
        }
        if pb <= pa + 1
            || !(d(pa) < w.first && d(pb) > w.last)
            || !(pa..=pb).all(|i| method[i] == EleMethod::Dem && ele[i].is_finite() && measured(&terrain[i]))
        {
            continue;
        }
        let (da, db) = (d(pa), d(pb));
        let (za, zb) = (ele[pa], ele[pb]);
        let chord = |i: usize| za + (zb - za) * (d(i) - da) / (db - da);
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
                let x = d(i);
                span = Some(span.map_or((x, x), |(a, _)| (a, x)));
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
