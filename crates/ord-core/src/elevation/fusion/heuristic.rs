//! Heuristiques V1 dans la fusion (T04 §4) : détecteurs de tablier (LiDAR) et de tunnel (MNT) sur
//! les stations uniformes du profil **brut**, détections passées à `reconcile` (T03 §7.1), puis
//! correction des ouvrages `heuristic` (hors de tout ouvrage OSM) : tablier lu dans le MNS par le
//! constructeur de T04 §3.2 (sinon droite entre sondes), tunnel en droite entre portails. Le
//! détecteur de couverture (plateau bas du MNT, MNS ≈ MNT, T04 §4.2 bis) corrige l'altitude seule :
//! droite entre ses extrémités, méthode `gap_filled`, avertissement `elevation_cover_corrected`,
//! sans ouvrage ni changement de nature du segment.
//!
//! Choix simples, notés :
//! - stations uniformes = stations du parcours à `k × station_step_m` (à 1 cm près) ;
//! - MNS d'une station = MNS de l'enveloppe LiDAR (`RawProfile::dsm`) si son MNT est IGN LiDAR HD
//!   sans repli, `NaN` sinon (invariant 10) ;
//! - une détection qui touche une plage sans typologie (`TypologyCoverage`) est écartée (doc 01
//!   §6 : élévation non corrigée là où la typologie manque) ;
//! - un tunnel détecté n'est gardé que si toutes les stations de `[portail − 100 m, portail +
//!   100 m]` sont sur terrain mesuré (`tunnelScope = measured-terrain`), s'il ne contient aucun
//!   tablier (pont OSM ou détecté), et, s'il ne touche aucun tunnel OSM, si le MNT lu en travers
//!   de l'axe (`TunnelCross`, lu par le pipeline aux points de `tunnel_cross_points`) est
//!   au-dessus de la route des deux côtés (règles 2026.11.11) : au pied d'une falaise ou d'un
//!   talus, sous un surplomb, le terrain ne remonte que d'un côté. Sans profil en travers lu,
//!   aucun tunnel détecté hors OSM n'est gardé ;
//! - une couverture détectée n'est gardée que si ses approches sont sur terrain IGN LiDAR HD (MNS
//!   lu, le détecteur l'exige), si elle ne touche, approches comprises, aucun passage OSM (pont ou
//!   tunnel : l'ouvrage OSM prime) ni tablier détecté, et ne recouvre aucun tunnel détecté ;
//! - sondes d'un ouvrage heuristique : stations du parcours à `abutment_probe_min_m` au-delà de
//!   chaque extrémité, lues sur le profil déjà corrigé des ouvrages OSM (continuité au raccord d'un
//!   ouvrage OSM prolongé) ; un ouvrage sans sonde ou sans altitude de sonde, ou un tablier sans
//!   sursol pour le constructeur (`kept_ground`), n'est ni corrigé ni rendu ;
//! - seules les stations restées `dem` sont corrigées : un ouvrage OSM corrigé n'est jamais
//!   réécrit.

use ord_domain::elevation::{DsmSample, TerrainOrigin, TerrainSample};
use ord_domain::geo::EARTH_RADIUS_M;
use ord_domain::report::{Warning, WarningCode};
use ord_domain::route::PreparedRoute;
use ord_domain::LatLon;

use crate::classification::{DetectedStructure, StructureSource, StructureVerdict};
use crate::elevation::heuristics::{detect_decks, detect_galleries, detect_tunnels, TunnelSpan};
use crate::rules::Rules;
use crate::typology::{StructureKind, TypologyOutput};

use super::deck::{build_deck, DeckOutcome, Probe};
use super::{OutcomeMethod, StructureOutcome};
use crate::elevation::EleMethod;

/// Demi-fenêtre de terrain mesuré exigée autour de chaque portail d'un tunnel détecté
/// (`vc:E/path/TunnelStep.kt:72, 136-141`).
const TUNNEL_SCOPE_M: f64 = 100.0;

/// Stations uniformes : rang dans `route.samples` de la station à `k × step`, pour `k = 0, 1, …`
/// jusqu'à la dernière station uniforme ; `None` si une station manque (grille inattendue).
fn uniform_stations(route: &PreparedRoute, step: f64) -> Option<Vec<usize>> {
    let mut out = Vec::new();
    for (i, s) in route.samples.iter().enumerate() {
        let k = (s.d_m / step).round();
        if (s.d_m - k * step).abs() <= 0.011 && k as usize == out.len() {
            out.push(i);
        }
    }
    (!out.is_empty()).then_some(out)
}

fn ign(t: &TerrainSample) -> bool {
    t.origin == TerrainOrigin::IgnLidarHd && !t.fallback_after_error
}

fn measured(t: &TerrainSample) -> bool {
    matches!(t.origin, TerrainOrigin::IgnLidarHd | TerrainOrigin::MapterhornMeasured)
        && !t.fallback_after_error
}

/// Côté d'une lecture en travers de l'axe, dans le sens du parcours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossSide {
    Left,
    Right,
}

/// Point où lire le MNT en travers de l'axe d'un tunnel candidat (T04 §4.2, règles 2026.11.11).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CrossPoint {
    /// Station du parcours (rang dans `route.samples`) dont la normale porte la lecture.
    pub station: usize,
    pub side: CrossSide,
    pub at: LatLon,
}

/// Profil en travers des tunnels candidats : points de `tunnel_cross_points` et terrain lu à
/// chacun (même ordre), par le pipeline avant la fusion.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TunnelCross {
    pub points: Vec<CrossPoint>,
    pub terrain: Vec<TerrainSample>,
}

/// Entrées du détecteur de tunnel : stations uniformes, MNT, latitudes et longitudes. Partagées
/// par `detect` et `tunnel_cross_points`, qui voient ainsi les mêmes candidats.
struct TunnelInputs {
    u: Vec<usize>,
    mnt: Vec<f64>,
    lat: Vec<f64>,
    lon: Vec<f64>,
}

fn tunnel_inputs(route: &PreparedRoute, terrain: &[TerrainSample], rules: &Rules) -> Option<TunnelInputs> {
    let u = uniform_stations(route, rules.elevation.station_step_m)?;
    if u.len() < 3 {
        return None;
    }
    Some(TunnelInputs {
        mnt: u.iter().map(|&i| terrain[i].ele.map_or(f64::NAN, f64::from)).collect(),
        lat: u.iter().map(|&i| route.samples[i].at.lat).collect(),
        lon: u.iter().map(|&i| route.samples[i].at.lon).collect(),
        u,
    })
}

fn candidate_tunnels(x: &TunnelInputs, rules: &Rules) -> Vec<TunnelSpan> {
    detect_tunnels(&x.mnt, &x.lat, &x.lon, &rules.elevation.tunnel_detector)
}

/// Points du profil en travers des tunnels candidats du profil brut (T04 §4.2, règles
/// 2026.11.11) : stations uniformes entre les portails, au pas `tunnel_cross.step_m`, et, de chaque
/// côté, un point à chaque distance de `tunnel_cross.offsets_m` sur la normale à l'axe (tangente
/// entre les stations uniformes à ± 2 pas). Vide sans heuristiques ni candidat.
pub fn tunnel_cross_points(
    route: &PreparedRoute,
    terrain: &[TerrainSample],
    rules: &Rules,
) -> Vec<CrossPoint> {
    let r = &rules.elevation;
    if !r.heuristics || terrain.len() != route.samples.len() {
        return Vec::new();
    }
    let Some(x) = tunnel_inputs(route, terrain, rules) else { return Vec::new() };
    let stride = ((r.tunnel_cross.step_m / r.station_step_m).round() as usize).max(1);
    let n = x.u.len();
    let mut out = Vec::new();
    for t in candidate_tunnels(&x, rules) {
        let mut k = t.first + stride;
        while k < t.last {
            let (a, b) = (k.saturating_sub(2), (k + 2).min(n - 1));
            let c = route.samples[x.u[k]].at;
            let cos = c.lat.to_radians().cos();
            let north = (x.lat[b] - x.lat[a]).to_radians() * EARTH_RADIUS_M;
            let east = (x.lon[b] - x.lon[a]).to_radians() * EARTH_RADIUS_M * cos;
            let norm = (north * north + east * east).sqrt();
            if norm > 1e-6 {
                // Normale à gauche (est, nord) = tangente tournée de +90°.
                let (ne, nn) = (-north / norm, east / norm);
                for (side, sign) in [(CrossSide::Left, 1.0), (CrossSide::Right, -1.0)] {
                    for &o in &r.tunnel_cross.offsets_m {
                        let (de, dn) = (sign * o * ne, sign * o * nn);
                        let at = LatLon::new(
                            c.lat + (dn / EARTH_RADIUS_M).to_degrees(),
                            c.lon + (de / (EARTH_RADIUS_M * cos)).to_degrees(),
                        );
                        out.push(CrossPoint { station: x.u[k], side, at });
                    }
                }
            }
            k += stride;
        }
    }
    out
}

/// Médiane des valeurs finies.
fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n == 0 {
        f64::NAN
    } else if n % 2 == 1 {
        v[n / 2]
    } else {
        0.5 * (v[n / 2 - 1] + v[n / 2])
    }
}

/// Le MNT lu en travers de l'axe est-il au-dessus de la route des deux côtés, entre les portails
/// `ia` et `ib` (rangs dans `route.samples`) ? Route = droite entre le MNT des portails ; de chaque
/// côté, médiane de `MNT − droite` sur les lectures d'altitude, au moins `min_height_m`, avec au
/// moins `min_readable_share` des lectures lues. Sans profil en travers : non.
fn above_on_both_sides(
    cross: Option<&TunnelCross>,
    route: &PreparedRoute,
    terrain: &[TerrainSample],
    (ia, ib): (usize, usize),
    rules: &Rules,
) -> bool {
    let Some(cross) = cross else { return false };
    let r = &rules.elevation.tunnel_cross;
    let z = |i: usize| terrain[i].ele.map_or(f64::NAN, f64::from);
    let (da, db) = (route.samples[ia].d_m, route.samples[ib].d_m);
    let (za, zb) = (z(ia), z(ib));
    if !(za.is_finite() && zb.is_finite() && db > da) {
        return false;
    }
    [CrossSide::Left, CrossSide::Right].into_iter().all(|side| {
        let mut total = 0usize;
        let mut above = Vec::new();
        for (p, t) in cross.points.iter().zip(&cross.terrain) {
            if p.side != side || p.station <= ia || p.station >= ib {
                continue;
            }
            total += 1;
            if let Some(v) = t.ele {
                let d = route.samples[p.station].d_m;
                above.push(f64::from(v) - (za + (zb - za) * (d - da) / (db - da)));
            }
        }
        total > 0
            && above.len() as f64 >= r.min_readable_share * total as f64
            && median(above) >= r.min_height_m
    })
}

/// Détections des heuristiques sur le profil brut, en distance de parcours. `cross` : profil en
/// travers des tunnels candidats (`tunnel_cross_points`) ; sans lui, aucun tunnel n'est gardé.
pub fn detect(
    route: &PreparedRoute,
    terrain: &[TerrainSample],
    dsm: Option<&[DsmSample]>,
    cross: Option<&TunnelCross>,
    typo: &TypologyOutput,
    rules: &Rules,
) -> Detections {
    let r = &rules.elevation;
    if !r.heuristics {
        return Detections::default();
    }
    let Some(inputs) = tunnel_inputs(route, terrain, rules) else { return Detections::default() };
    let u = &inputs.u;
    let d: Vec<f64> = u.iter().map(|&i| route.samples[i].d_m).collect();
    let mnt = &inputs.mnt;
    let mns: Vec<f64> = u
        .iter()
        .map(|&i| match dsm.and_then(|v| v.get(i)) {
            Some(DsmSample::Value(v)) if ign(&terrain[i]) => f64::from(*v),
            _ => f64::NAN,
        })
        .collect();
    let gaps: Vec<(f64, f64)> =
        typo.coverage.missing.iter().chain(&typo.coverage.unavailable).map(|g| (g.start, g.end)).collect();
    let touches_gap = |a: f64, b: f64| gaps.iter().any(|g| g.1 > a && g.0 < b);
    let mut out: Vec<DetectedStructure> = Vec::new();
    let decks = detect_decks(mnt, &mns, &r.deck_detector);
    let mut deck_spans: Vec<(f64, f64)> = Vec::new();
    for c in &decks.accepted {
        let (a, b) = (d[c.first], d[c.last]);
        deck_spans.push((a, b));
        if !touches_gap(a, b) {
            out.push(DetectedStructure {
                id: out.len() as u32,
                kind: StructureKind::Bridge,
                from_m: a,
                to_m: b,
            });
        }
    }
    // Ponts OSM (passages dont l'élément au milieu est un pont) : un tunnel qui en contient un est
    // abandonné (piles gardées par le MNT). Tunnels OSM : un tunnel détecté qui en touche un n'a
    // pas à prouver son profil en travers (l'ouvrage OSM le confirme).
    let mut osm_tunnels: Vec<(f64, f64)> = Vec::new();
    for p in &typo.passages {
        let kind = typo
            .structures
            .iter()
            .find(|s| s.id == p.structure)
            .map(|s| s.kind_at(p.chainage_at(0.5 * (p.from_m + p.to_m))));
        match kind {
            Some(StructureKind::Bridge) => deck_spans.push((p.from_m, p.to_m)),
            Some(StructureKind::Tunnel) => osm_tunnels.push((p.from_m, p.to_m)),
            None => {}
        }
    }
    let meas: Vec<bool> = u.iter().map(|&i| measured(&terrain[i])).collect();
    for t in candidate_tunnels(&inputs, rules) {
        let (ia, ib) = (t.first, t.last);
        let (a, b) = (d[ia], d[ib]);
        let scope =
            |p: usize| (0..d.len()).filter(|&k| (d[k] - d[p]).abs() <= TUNNEL_SCOPE_M).all(|k| meas[k]);
        if !scope(ia) || !scope(ib) || touches_gap(a, b) || deck_spans.iter().any(|s| s.1 > a && s.0 < b) {
            continue;
        }
        // Profil en travers (règles 2026.11.11) : hors de tout tunnel OSM, terrain au-dessus de la
        // route des deux côtés.
        let on_osm_tunnel = osm_tunnels.iter().any(|s| s.1 > a && s.0 < b);
        if !on_osm_tunnel && !above_on_both_sides(cross, route, terrain, (u[ia], u[ib]), rules) {
            continue;
        }
        out.push(DetectedStructure { id: out.len() as u32, kind: StructureKind::Tunnel, from_m: a, to_m: b });
    }
    // Couvertures (T04 §4.2 bis) : altitude seule, jamais un ouvrage. Hors OSM, rien ne distingue
    // une galerie d'un passage inférieur ou d'une couverture non cartographiée lus dans le MNT.
    // Écartées si elles ou leurs approches touchent un passage OSM (l'ouvrage OSM prime ; le
    // plateau entre deux ponts est une levée) ou un tablier détecté, ou si elles recouvrent un
    // tunnel détecté.
    let reach = r.gallery_detector.approach_m;
    let tunnels: Vec<(f64, f64)> =
        out.iter().filter(|x| x.kind == StructureKind::Tunnel).map(|x| (x.from_m, x.to_m)).collect();
    let osm: Vec<(f64, f64)> = typo.passages.iter().map(|p| (p.from_m, p.to_m)).collect();
    let mut covers = Vec::new();
    for g in detect_galleries(mnt, &mns, &r.gallery_detector) {
        let (a, b) = (d[g.first], d[g.last]);
        if touches_gap(a, b)
            || deck_spans.iter().chain(&osm).any(|s| s.1 > a - reach && s.0 < b + reach)
            || tunnels.iter().any(|t| t.1 > a && t.0 < b)
        {
            continue;
        }
        covers.push((a, b));
    }
    out.sort_by(|x, y| x.from_m.total_cmp(&y.from_m));
    Detections { structures: out, covers }
}

/// Détections des heuristiques : ouvrages (passés à `reconcile`) et couvertures (altitude seule).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Detections {
    pub structures: Vec<DetectedStructure>,
    /// Plages `(de, à)` des couvertures lues dans le MNT, en distance de parcours.
    pub covers: Vec<(f64, f64)>,
}

/// Couvertures lues dans le MNT (T04 §4.2 bis) : droite entre les stations à
/// `abutment_probe_min_m` au-delà de chaque extrémité, sur les seules stations restées `dem`,
/// méthode `gap_filled`, avertissement `elevation_cover_corrected`. Ni ouvrage ni verdict : la
/// nature des segments ne change pas. Sans sonde ou sans altitude de sonde, rien n'est corrigé.
/// Une couverture qui touche, approches comprises, une fenêtre de pont au-dessus de la trace déjà
/// corrigée (`overpasses`, T04 §3.4) est écartée : le pont OSM prime.
pub fn correct_covers(
    covers: &[(f64, f64)],
    overpasses: &[(f64, f64)],
    route: &PreparedRoute,
    rules: &Rules,
    ele: &mut [f64],
    method: &mut [EleMethod],
    warnings: &mut Vec<Warning>,
) {
    let probe_m = rules.elevation.structures.abutment_probe_min_m;
    let step = rules.elevation.station_step_m;
    let reach = rules.elevation.gallery_detector.approach_m;
    for &(a, b) in covers {
        if overpasses.iter().any(|w| w.1 > a - reach && w.0 < b + reach) {
            continue;
        }
        let (Some(pa), Some(pb)) =
            (station_at(route, a - probe_m, step), station_at(route, b + probe_m, step))
        else {
            continue;
        };
        let (za, zb) = (ele[pa], ele[pb]);
        if za.is_nan()
            || zb.is_nan()
            || pb <= pa + 1
            || method[pa] != EleMethod::Dem
            || method[pb] != EleMethod::Dem
        {
            continue;
        }
        let line = [(route.samples[pa].d_m, za), (route.samples[pb].d_m, zb)];
        let inner: Vec<usize> = (pa + 1..pb).filter(|&i| method[i] == EleMethod::Dem).collect();
        if inner.is_empty() {
            continue;
        }
        for i in inner {
            ele[i] = interp(&line, route.samples[i].d_m);
            method[i] = EleMethod::GapFilled;
        }
        warnings.push(Warning {
            code: WarningCode::ElevationCoverCorrected,
            message: "terrain model reads a cover above the road; elevation set to the line between its ends"
                .into(),
            range: Some(a..b),
        });
    }
}

/// Interpolation linéaire de `(x, z)` triés, bornée aux extrémités.
fn interp(pts: &[(f64, f64)], x: f64) -> f64 {
    let i = pts.partition_point(|p| p.0 <= x);
    if i == 0 {
        return pts[0].1;
    }
    if i >= pts.len() {
        return pts[pts.len() - 1].1;
    }
    let (a, b) = (pts[i - 1], pts[i]);
    if b.0 > a.0 {
        a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0)
    } else {
        b.1
    }
}

/// Station du parcours la plus proche de `x` (distance), si elle est à moins d'un pas.
fn station_at(route: &PreparedRoute, x: f64, tol: f64) -> Option<usize> {
    let s = &route.samples;
    let i = s.partition_point(|p| p.d_m < x);
    [i.checked_sub(1), (i < s.len()).then_some(i)]
        .into_iter()
        .flatten()
        .min_by(|&a, &b| (s[a].d_m - x).abs().total_cmp(&(s[b].d_m - x).abs()))
        .filter(|&k| (s[k].d_m - x).abs() <= tol)
}

/// Corrige les ouvrages `heuristic` des verdicts sur `ele` et `method` ; renvoie les verdicts
/// gardés (un ouvrage heuristique non corrigeable est retiré) et ajoute issues et avertissements
/// (`structure_untagged`).
#[allow(clippy::too_many_arguments)]
pub fn correct_heuristics(
    verdicts: Vec<StructureVerdict>,
    route: &PreparedRoute,
    terrain: &[TerrainSample],
    dsm: Option<&[DsmSample]>,
    rules: &Rules,
    ele: &mut [f64],
    method: &mut [EleMethod],
    outcomes: &mut Vec<StructureOutcome>,
    warnings: &mut Vec<Warning>,
) -> Vec<StructureVerdict> {
    let probe_m = rules.elevation.structures.abutment_probe_min_m;
    let step = rules.elevation.station_step_m;
    // Plages des passages OSM : leurs stations ne sont jamais réécrites. Une station corrigée par un
    // pont OSM hors de ses passages est une rampe (ou marge de sonde) vers la sonde au sol ; quand
    // un tablier détecté prolonge ce pont, la rampe descendrait sur le tablier non tagué : elle est
    // reprise par le tablier détecté (T04 §4.3 bis, raccord rampe OSM / tablier détecté).
    let osm: Vec<(f64, f64, StructureKind)> =
        verdicts.iter().filter(|v| v.passage.is_some()).map(|v| (v.from_m, v.to_m, v.kind)).collect();
    let in_osm = |x: f64| osm.iter().any(|o| x >= o.0 - 1e-6 && x <= o.1 + 1e-6);
    let near_osm_bridge = |x: f64, reach: f64| {
        osm.iter().any(|o| o.2 == StructureKind::Bridge && x >= o.0 - reach && x <= o.1 + reach)
    };
    let mut kept = Vec::with_capacity(verdicts.len());
    for v in verdicts {
        if v.source != StructureSource::Heuristic {
            kept.push(v);
            continue;
        }
        let (Some(mut pa), Some(mut pb)) =
            (station_at(route, v.from_m - probe_m, step), station_at(route, v.to_m + probe_m, step))
        else {
            continue;
        };
        // Rampe d'un pont OSM voisin, reprise par un tablier détecté (voir plus haut).
        let ramp = |i: usize, method: &[EleMethod]| {
            v.kind == StructureKind::Bridge
                && method[i] != EleMethod::Dem
                && !in_osm(route.samples[i].d_m)
                && near_osm_bridge(route.samples[i].d_m, 2.0 * probe_m + step)
        };
        // Sonde posée sur une telle rampe : reculée jusqu'au pont OSM (tablier corrigé) ou au sol.
        let mut guard = 0;
        while pa > 0 && ramp(pa, method) && guard < 20 {
            pa -= 1;
            guard += 1;
        }
        guard = 0;
        while pb + 1 < route.samples.len() && ramp(pb, method) && guard < 20 {
            pb += 1;
            guard += 1;
        }
        let (za, zb) = (ele[pa], ele[pb]);
        if za.is_nan() || zb.is_nan() || pb <= pa + 1 {
            continue;
        }
        let (ca, cb) = (route.samples[pa].d_m, route.samples[pb].d_m);
        // Stations strictement entre les sondes, restées au MNT ou rampes d'un pont OSM voisin ;
        // jamais une station d'un passage OSM.
        let inner: Vec<usize> =
            (pa + 1..pb).filter(|&i| method[i] == EleMethod::Dem || ramp(i, method)).collect();
        if inner.is_empty() {
            continue;
        }
        let line = [(ca, za), (cb, zb)];
        let result: (OutcomeMethod, Vec<(usize, f64, EleMethod)>) = match v.kind {
            StructureKind::Tunnel => (
                OutcomeMethod::InterpolatedStructure,
                inner
                    .iter()
                    .map(|&i| (i, interp(&line, route.samples[i].d_m), EleMethod::InterpolatedStructure))
                    .collect(),
            ),
            StructureKind::Bridge => {
                let c: Vec<f64> = inner.iter().map(|&i| route.samples[i].d_m).collect();
                let mnt: Vec<f64> =
                    inner.iter().map(|&i| terrain[i].ele.map_or(f64::NAN, f64::from)).collect();
                let mns: Vec<f64> = inner
                    .iter()
                    .map(|&i| match dsm.and_then(|d| d.get(i)) {
                        Some(DsmSample::Value(x)) if ign(&terrain[i]) => f64::from(*x),
                        _ => f64::NAN,
                    })
                    .collect();
                let probe_mns = |i: usize| match dsm.and_then(|d| d.get(i)) {
                    Some(DsmSample::Value(x)) => f64::from(*x),
                    _ => f64::NAN,
                };
                let probes = [
                    Some(Probe { c: ca, z: za, mns: probe_mns(pa) }),
                    Some(Probe { c: cb, z: zb, mns: probe_mns(pb) }),
                ];
                match build_deck(&c, &mnt, &mns, probes, &rules.elevation.deck) {
                    DeckOutcome::Deck { z, first, last } => {
                        let mut known: Vec<(f64, f64)> = vec![(ca, za), (cb, zb)];
                        known.extend((first..=last).map(|j| (c[j], z[j])));
                        known.sort_by(|a, b| a.0.total_cmp(&b.0));
                        known.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-9);
                        if known.iter().any(|p| p.1.is_nan()) {
                            continue;
                        }
                        (
                            OutcomeMethod::DsmDeck,
                            inner
                                .iter()
                                .map(|&i| (i, interp(&known, route.samples[i].d_m), EleMethod::DsmDeck))
                                .collect(),
                        )
                    }
                    // Pas de sursol selon le constructeur : la détection n'est pas un pont (part
                    // d'une détection qui déborde d'un pont OSM sur la route au sol, par exemple).
                    DeckOutcome::KeptGround => continue,
                    DeckOutcome::Fallback(_) => (
                        OutcomeMethod::InterpolatedStructure,
                        inner
                            .iter()
                            .map(|&i| {
                                (i, interp(&line, route.samples[i].d_m), EleMethod::InterpolatedStructure)
                            })
                            .collect(),
                    ),
                }
            }
        };
        for (i, z, m) in result.1 {
            ele[i] = z;
            method[i] = m;
        }
        outcomes.push(StructureOutcome {
            kind: v.kind,
            from_m: v.from_m,
            to_m: v.to_m,
            source: StructureSource::Heuristic,
            method: result.0,
        });
        warnings.push(Warning {
            code: WarningCode::StructureUntagged,
            message: "structure detected from terrain data, not tagged in OSM; corrected".into(),
            range: Some(v.from_m..v.to_m),
        });
        kept.push(v);
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classification::GroupRef;
    use crate::elevation::tests::line;
    use ord_domain::elevation::DemKind;

    fn ign_sample(z: f64) -> TerrainSample {
        TerrainSample {
            ele: Some(z as f32),
            origin: TerrainOrigin::IgnLidarHd,
            fallback_after_error: false,
            dem_kind: DemKind::Dtm,
            missing: None,
        }
    }

    fn verdict(kind: StructureKind, from_m: f64, to_m: f64) -> StructureVerdict {
        StructureVerdict {
            target: GroupRef::Heuristic(0),
            kind,
            from_m,
            to_m,
            passage: None,
            source: StructureSource::Heuristic,
        }
    }

    #[test]
    fn an_untagged_deck_is_read_in_the_dsm_and_warned() {
        // Parcours de 1 km ; vallée de 30 m sous un tablier plat à 50 m entre 400 et 600 m.
        let route = line(1_000.0);
        let d: Vec<f64> = route.samples.iter().map(|s| s.d_m).collect();
        let valley = |x: f64| if (400.0..=600.0).contains(&x) { 20.0 } else { 50.0 };
        let terrain: Vec<TerrainSample> = d.iter().map(|&x| ign_sample(valley(x))).collect();
        let dsm: Vec<DsmSample> = d.iter().map(|_| DsmSample::Value(50.0)).collect();
        let mut ele: Vec<f64> = d.iter().map(|&x| valley(x)).collect();
        let mut method = vec![EleMethod::Dem; d.len()];
        let (mut outcomes, mut warnings) = (Vec::new(), Vec::new());
        let kept = correct_heuristics(
            vec![verdict(StructureKind::Bridge, 400.0, 600.0)],
            &route,
            &terrain,
            Some(&dsm),
            &Rules::default(),
            &mut ele,
            &mut method,
            &mut outcomes,
            &mut warnings,
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(outcomes[0].method, OutcomeMethod::DsmDeck);
        assert_eq!(outcomes[0].source, StructureSource::Heuristic);
        let mid = d.iter().position(|&x| (x - 500.0).abs() < 1e-6).unwrap();
        assert!((ele[mid] - 50.0).abs() < 0.5, "{}", ele[mid]);
        assert_eq!(method[mid], EleMethod::DsmDeck);
        assert_eq!(warnings[0].code, WarningCode::StructureUntagged);
        assert_eq!(warnings[0].range, Some(400.0..600.0));
    }

    #[test]
    fn an_untagged_tunnel_is_a_line_between_its_probes_and_osm_stations_are_kept() {
        let route = line(1_000.0);
        let d: Vec<f64> = route.samples.iter().map(|s| s.d_m).collect();
        // Montagne au-dessus du tunnel (300–700 m), route de 100 à 104 m.
        let terrain: Vec<TerrainSample> = d
            .iter()
            .map(|&x| ign_sample(if (300.0..=700.0).contains(&x) { 300.0 } else { 100.0 + x / 250.0 }))
            .collect();
        let mut ele: Vec<f64> = terrain.iter().map(|t| f64::from(t.ele.unwrap())).collect();
        let mut method = vec![EleMethod::Dem; d.len()];
        // Une station déjà corrigée par un ouvrage OSM n'est jamais réécrite.
        let osm = d.iter().position(|&x| (x - 650.0).abs() < 1e-6).unwrap();
        method[osm] = EleMethod::InterpolatedStructure;
        ele[osm] = -1.0;
        let (mut outcomes, mut warnings) = (Vec::new(), Vec::new());
        correct_heuristics(
            vec![verdict(StructureKind::Tunnel, 300.0, 700.0)],
            &route,
            &terrain,
            None,
            &Rules::default(),
            &mut ele,
            &mut method,
            &mut outcomes,
            &mut warnings,
        );
        let mid = d.iter().position(|&x| (x - 500.0).abs() < 1e-6).unwrap();
        assert!((ele[mid] - 102.0).abs() < 0.1, "{}", ele[mid]);
        assert_eq!(method[mid], EleMethod::InterpolatedStructure);
        assert_eq!(ele[osm], -1.0);
        assert_eq!(outcomes[0].method, OutcomeMethod::InterpolatedStructure);
    }

    #[test]
    fn an_osm_ramp_rests_on_a_detected_deck_that_extends_the_bridge() {
        // Viaduc continu à 50 m de 300 à 800 m sur une vallée à 20 m ; le way OSM ne couvre que
        // 600–800 m : sa rampe (590–600 m, vers la sonde au sol à 590 m) plonge sur le viaduc non
        // tagué. Le tablier détecté (300–600 m) reprend la rampe ; le passage OSM n'est pas touché.
        let route = line(1_000.0);
        let d: Vec<f64> = route.samples.iter().map(|s| s.d_m).collect();
        let valley = |x: f64| if (300.0..=800.0).contains(&x) { 20.0 } else { 50.0 };
        let terrain: Vec<TerrainSample> = d.iter().map(|&x| ign_sample(valley(x))).collect();
        let dsm: Vec<DsmSample> = d.iter().map(|_| DsmSample::Value(50.0)).collect();
        let mut ele: Vec<f64> = d.iter().map(|&x| valley(x)).collect();
        let mut method = vec![EleMethod::Dem; d.len()];
        for (i, &x) in d.iter().enumerate() {
            if (600.0..=800.0).contains(&x) {
                (ele[i], method[i]) = (50.0, EleMethod::DsmDeck);
            } else if (590.0..600.0).contains(&x) {
                // Rampe OSM vers la sonde au sol (20 m à 590 m).
                (ele[i], method[i]) = (20.0 + 3.0 * (x - 590.0), EleMethod::DsmDeck);
            }
        }
        let osm = StructureVerdict {
            target: GroupRef::Osm(1),
            kind: StructureKind::Bridge,
            from_m: 600.0,
            to_m: 800.0,
            passage: Some(0),
            source: StructureSource::Osm,
        };
        let (mut outcomes, mut warnings) = (Vec::new(), Vec::new());
        let kept = correct_heuristics(
            vec![osm, verdict(StructureKind::Bridge, 300.0, 600.0)],
            &route,
            &terrain,
            Some(&dsm),
            &Rules::default(),
            &mut ele,
            &mut method,
            &mut outcomes,
            &mut warnings,
        );
        assert_eq!(kept.len(), 2);
        for (i, &x) in d.iter().enumerate() {
            if (350.0..=800.0).contains(&x) {
                assert!((ele[i] - 50.0).abs() < 0.5, "{x} : {}", ele[i]);
            }
        }
        assert_eq!(outcomes[0].method, OutcomeMethod::DsmDeck);
    }

    #[test]
    fn a_cover_is_corrected_in_elevation_only() {
        // Route à 1 % sur 600 m, plateau de 7 m entre 250 et 350 m (MNT lu sur un toit).
        let route = line(600.0);
        let d: Vec<f64> = route.samples.iter().map(|s| s.d_m).collect();
        let road = |x: f64| 100.0 + 0.01 * x;
        let mut ele: Vec<f64> =
            d.iter().map(|&x| road(x) + if (255.0..=345.0).contains(&x) { 7.0 } else { 0.0 }).collect();
        let mut method = vec![EleMethod::Dem; d.len()];
        // Une station d'un ouvrage OSM n'est jamais réécrite.
        let osm = d.iter().position(|&x| (x - 300.0).abs() < 1e-6).unwrap();
        method[osm] = EleMethod::InterpolatedStructure;
        let mut warnings = Vec::new();
        correct_covers(
            &[(250.0, 350.0)],
            &[],
            &route,
            &Rules::default(),
            &mut ele,
            &mut method,
            &mut warnings,
        );
        for (i, &x) in d.iter().enumerate() {
            if i != osm && (245.0..=355.0).contains(&x) {
                assert!((ele[i] - road(x)).abs() < 0.05, "{x} : {}", ele[i]);
            }
        }
        let mid = d.iter().position(|&x| (x - 280.0).abs() < 1e-6).unwrap();
        assert_eq!(method[mid], EleMethod::GapFilled);
        assert_eq!(method[osm], EleMethod::InterpolatedStructure);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, WarningCode::ElevationCoverCorrected);
        assert_eq!(warnings[0].range, Some(250.0..350.0));
    }

    /// Parcours de 1 km vers le nord, route à 5 %, paroi de 40 m sur l'axe entre 300 et 700 m.
    fn walled(route: &PreparedRoute) -> Vec<TerrainSample> {
        route
            .samples
            .iter()
            .map(|s| {
                let x = s.d_m;
                ign_sample(100.0 + 0.05 * x + if (x - 500.0).abs() < 200.0 { 40.0 } else { 0.0 })
            })
            .collect()
    }

    /// Profil en travers : `left` et `right` ajoutés à la route (5 %) de chaque côté.
    fn cross_with(route: &PreparedRoute, left: f64, right: f64, read: bool) -> TunnelCross {
        let points = tunnel_cross_points(route, &walled(route), &Rules::default());
        let terrain = points
            .iter()
            .map(|p| {
                let road = 100.0 + 0.05 * route.samples[p.station].d_m;
                let mut t = ign_sample(road + if p.side == CrossSide::Left { left } else { right });
                if !read {
                    t.ele = None;
                }
                t
            })
            .collect();
        TunnelCross { points, terrain }
    }

    #[test]
    fn cross_points_lie_on_both_sides_of_each_candidate() {
        let route = line(1_000.0);
        let rules = Rules::default();
        let points = tunnel_cross_points(&route, &walled(&route), &rules);
        assert!(!points.is_empty());
        let offsets = rules.elevation.tunnel_cross.offsets_m.len();
        assert_eq!(points.len() % (2 * offsets), 0);
        for p in &points {
            let c = route.samples[p.station];
            // Entre les portails.
            assert!(c.d_m > 280.0 && c.d_m < 720.0, "{}", c.d_m);
            // Vers le nord : la gauche est à l'ouest, à 10, 20 ou 30 m de l'axe.
            let dx = (p.at.lon - c.at.lon).to_radians() * EARTH_RADIUS_M * c.at.lat.to_radians().cos();
            assert!((p.at.lat - c.at.lat).abs() < 1e-7);
            assert_eq!(p.side == CrossSide::Left, dx < 0.0);
            assert!([10.0, 20.0, 30.0].iter().any(|o| (dx.abs() - o).abs() < 0.01), "{dx}");
        }
        // Stations au pas de 10 m.
        let mut d: Vec<f64> = points.iter().map(|p| route.samples[p.station].d_m).collect();
        d.dedup();
        assert!(d.windows(2).all(|w| (w[1] - w[0] - 10.0).abs() < 1e-6), "{d:?}");
        // Sans heuristiques : rien à lire.
        let mut off = Rules::default();
        off.elevation.heuristics = false;
        assert!(tunnel_cross_points(&route, &walled(&route), &off).is_empty());
    }

    #[test]
    fn a_tunnel_needs_terrain_above_the_road_on_both_sides() {
        let route = line(1_000.0);
        let terrain = walled(&route);
        let typo = TypologyOutput {
            runs: Vec::new(),
            coverage: Default::default(),
            classes: Vec::new().into(),
            stats: Default::default(),
            structures: Vec::new(),
            passages: Vec::new(),
            overpasses: Vec::new(),
        };
        let rules = Rules::default();
        let tunnels = |cross: Option<&TunnelCross>| {
            detect(&route, &terrain, None, cross, &typo, &rules)
                .structures
                .into_iter()
                .filter(|s| s.kind == StructureKind::Tunnel)
                .count()
        };
        // Montagne des deux côtés : tunnel gardé.
        assert_eq!(tunnels(Some(&cross_with(&route, 30.0, 25.0, true))), 1);
        // Pied de falaise : paroi d'un côté, vallée de l'autre ; écarté.
        assert_eq!(tunnels(Some(&cross_with(&route, 40.0, -8.0, true))), 0);
        // Talus d'un côté, terrain au niveau de la route de l'autre (sous la clé d'un tube) : écarté.
        assert_eq!(tunnels(Some(&cross_with(&route, 40.0, 3.0, true))), 0);
        // Profil en travers illisible, ou non lu : écarté.
        assert_eq!(tunnels(Some(&cross_with(&route, 30.0, 25.0, false))), 0);
        assert_eq!(tunnels(None), 0);
    }

    #[test]
    fn uniform_stations_skip_input_points() {
        let route = line(52.0);
        let u = uniform_stations(&route, 5.0).unwrap();
        assert_eq!(u.len(), 11, "0 à 50 m");
        assert!(u.iter().enumerate().all(|(k, &i)| (route.samples[i].d_m - 5.0 * k as f64).abs() < 0.011));
    }
}
