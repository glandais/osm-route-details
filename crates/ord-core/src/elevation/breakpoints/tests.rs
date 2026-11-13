//! Sélection des points de rupture (T04 §6) : événements, fusion, rattachement, espacement,
//! plafond ; propriétés de T05 §2.2 (`d_m` strictement croissant, points d'entrée jamais
//! déplacés, en nombre égal aux entrées dédoublonnées).

use ord_domain::elevation::{Missing, TerrainSample};
use ord_domain::geo::EARTH_RADIUS_M;
use ord_domain::request::RequestOptions;
use ord_domain::route::PreparedRoute;
use ord_domain::LatLon;

use super::*;
use crate::classification::{StructureSource, SurfaceClass, SurfaceSource, WayKind};
use crate::elevation::derive::accumulate;
use crate::elevation::fusion::{OutcomeMethod, StructureOutcome};
use crate::elevation::tests::sample;
use crate::elevation::{fuse, RawProfile};
use crate::prepare::prepare;
use crate::rules::Rules;
use crate::typology::StructureKind;

/// Point à `m` mètres au nord du départ.
fn north(m: f64) -> LatLon {
    LatLon::new(45.0 + (m / EARTH_RADIUS_M).to_degrees(), 6.0)
}

/// Parcours rectiligne vers le nord, points d'entrée aux distances données.
fn route(at: &[f64]) -> PreparedRoute {
    prepare(&at.iter().map(|&m| north(m)).collect::<Vec<_>>(), 5.0)
}

/// Profil fusionné d'un terrain analytique (`None` : no-data), sans comblement des trous.
fn profile(r: &PreparedRoute, z: impl Fn(f64) -> Option<f64>) -> ElevationProfile {
    let terrain: Vec<TerrainSample> = r
        .samples
        .iter()
        .map(|s| {
            let e = z(s.d_m).map(|v| v as f32);
            sample(e, e.is_none().then_some(Missing::NoData))
        })
        .collect();
    let mut rules = Rules::default();
    rules.elevation.gap_fill_max_m = 0.0;
    fuse(
        r,
        &RawProfile { terrain, structures: None, dsm: None, tunnel_cross: None },
        None,
        &RequestOptions::default(),
        &rules,
    )
    .unwrap()
}

fn select(r: &PreparedRoute, p: &ElevationProfile, segs: &[TypedSegment]) -> Vec<OutputPoint> {
    select_breakpoints(r, p, segs, &BreakpointRules::default())
}

fn inserted(pts: &[OutputPoint]) -> Vec<&OutputPoint> {
    pts.iter().filter(|q| q.origin == Some(PointOrigin::Inserted)).collect()
}

fn with(pts: &[OutputPoint], reason: BreakpointReason) -> Vec<f64> {
    pts.iter().filter(|q| q.reasons.contains(&reason)).map(|q| q.d_m).collect()
}

/// Propriétés communes à toute sortie (T05 §2.2).
fn check_properties(r: &PreparedRoute, pts: &[OutputPoint]) {
    assert!(pts.windows(2).all(|w| w[1].d_m > w[0].d_m), "d_m strictement croissant");
    let inputs: Vec<&OutputPoint> = pts.iter().filter(|q| q.origin == Some(PointOrigin::Input)).collect();
    assert_eq!(inputs.len(), r.input_pos.len(), "un point par entrée dédoublonnée");
    for (q, &pos) in inputs.iter().zip(&r.input_pos) {
        let s = &r.samples[pos as usize];
        assert_eq!(q.d_m, s.d_m, "point d'entrée jamais déplacé");
        assert_eq!(Some(q.index), s.input_index);
        assert!(q.latlon.is_none());
    }
    for q in inserted(pts) {
        assert!(!q.reasons.is_empty() && q.latlon.is_some(), "{q:?}");
        let prev = r.input_pos.iter().map(|&i| &r.samples[i as usize]).rfind(|s| s.d_m <= q.d_m);
        assert_eq!(Some(q.index), prev.and_then(|s| s.input_index));
    }
    assert!(pts.windows(2).all(|w| w[1].index >= w[0].index), "indices croissants");
}

#[test]
fn summit_between_distant_input_points_is_inserted() {
    let r = route(&[0.0, 2_000.0]);
    // Sommet arrondi (parabole de 200 m) à 700 m.
    let p = profile(&r, |d| {
        Some(match d {
            d if d < 600.0 => 100.0 + d * 0.1,
            d if d < 800.0 => 165.0 + 0.1 * (d - 600.0) - 0.075 * (d - 600.0).powi(2) / 200.0 + 0.0,
            d => 170.0 - (d - 800.0) * 0.05,
        })
    });
    let pts = select(&r, &p, &[]);
    check_properties(&r, &pts);
    let max = with(&pts, BreakpointReason::LocalMax);
    assert_eq!(max.len(), 1, "{pts:?}");
    let q = pts.iter().find(|q| q.reasons.contains(&BreakpointReason::LocalMax)).unwrap();
    let top = (600..800).map(|x| p.at(x as f64).ele.unwrap()).fold(f64::MIN, f64::max);
    assert!((q.ele.unwrap() - top).abs() < 0.01, "{q:?} {top}");
    let pos = q.latlon.unwrap();
    assert!((pos.lat - north(q.d_m).lat).abs() < 1e-7, "position sur la trace d'origine");
    // Les extrémités des jambes redonnent le D+.
    let e: Vec<f64> = pts.iter().map(|q| q.ele.unwrap()).collect();
    let gain: f64 = e.windows(2).map(|w| (w[1] - w[0]).max(0.0)).sum();
    assert!((gain - p.gain.gain_m).abs() / p.gain.gain_m < 0.02, "{gain} vs {}", p.gain.gain_m);
}

#[test]
fn grade_change_on_a_monotonic_climb() {
    let r = route(&[0.0, 2_000.0]);
    let p = profile(&r, |d| Some(if d < 600.0 { d * 0.02 } else { 12.0 + (d - 600.0) * 0.08 }));
    let pts = select(&r, &p, &[]);
    check_properties(&r, &pts);
    let g = with(&pts, BreakpointReason::GradeChange);
    assert_eq!(g.len(), 1, "{pts:?}");
    assert!((g[0] - 600.0).abs() <= 5.0);
    // Pièce trop courte : pas de rupture.
    let p = profile(&r, |d| Some(if d < 30.0 { 0.0 } else { (d - 30.0) * 0.08 }));
    assert!(with(&select(&r, &p, &[]), BreakpointReason::GradeChange).is_empty());
}

fn outcome(from_m: f64, to_m: f64, kind: StructureKind) -> StructureOutcome {
    StructureOutcome { kind, from_m, to_m, source: StructureSource::Osm, method: OutcomeMethod::DsmDeck }
}

#[test]
fn structure_ends_are_class_a_and_snap_to_input_points() {
    let r = route(&[0.0, 1_000.0, 2_000.0]);
    let mut p = profile(&r, |_| Some(10.0));
    // Pont du départ (borne 0 rattachée au point d'entrée), pont et tunnel contigus.
    p.structures = vec![
        outcome(0.0, 102.0, StructureKind::Bridge),
        outcome(300.25, 812.5, StructureKind::Bridge),
        outcome(812.5, 997.0, StructureKind::Tunnel),
    ];
    let pts = select(&r, &p, &[]);
    check_properties(&r, &pts);
    assert_eq!(pts[0].reasons, [BreakpointReason::StructureStart]);
    assert_eq!(with(&pts, BreakpointReason::StructureStart), [0.0, 300.25, 812.5]);
    let ends = with(&pts, BreakpointReason::StructureEnd);
    assert!(ends.len() == 3 && (ends[2] - 1_000.0).abs() < 1e-6, "{ends:?}");
    assert_eq!(ends[..2], [102.0, 812.5]);
    let shared = pts.iter().find(|q| q.d_m == 812.5).unwrap();
    assert_eq!(shared.reasons, [BreakpointReason::StructureStart, BreakpointReason::StructureEnd]);
    // 997 m est à 3 m du point d'entrée de 1 000 m : rattaché, aucun point créé.
    assert_eq!(pts.iter().find(|q| (q.d_m - 1_000.0).abs() < 1e-6).unwrap().origin, Some(PointOrigin::Input));
    assert_eq!(inserted(&pts).len(), 3);
}

#[test]
fn gap_ends_carry_the_last_and_first_elevations() {
    let r = route(&[0.0, 2_000.0]);
    let p = profile(&r, |d| (!(800.0..1_100.0).contains(&d)).then_some(50.0 + d * 0.01));
    let pts = select(&r, &p, &[]);
    check_properties(&r, &pts);
    let start = pts.iter().find(|q| q.reasons.contains(&BreakpointReason::ElevationGapStart)).unwrap();
    let end = pts.iter().find(|q| q.reasons.contains(&BreakpointReason::ElevationGapEnd)).unwrap();
    assert_eq!((start.d_m, end.d_m), (795.0, 1_100.0));
    assert!(start.ele.is_some() && end.ele.is_some());
    let m = &p.missing[0];
    assert!((start.d_m - m.from_m).abs() <= 5.0 && (end.d_m - m.to_m).abs() <= 5.0);
    // Origine du terrain : `none` dans le trou, au bord du trou (fusionnée).
    assert!(start.reasons.contains(&BreakpointReason::TerrainSourceChange), "{start:?}");
}

fn segment(from_m: f64, to_m: f64, surface: SurfaceClass) -> TypedSegment {
    TypedSegment {
        from_m,
        to_m,
        from_index: 0,
        to_index: 1,
        structure: StructureClass::Ground,
        structure_source: None,
        surface_class: surface,
        surface_source: SurfaceSource::Tagged,
        surface_detail: None,
        way_kind: WayKind::MinorRoad,
        matched: true,
        off_graph: false,
        unmatched_reason: None,
        element: None,
        class: None,
    }
}

#[test]
fn surface_change_is_class_b_and_survives_spacing() {
    let r = route(&[0.0, 1_000.0]);
    let p = profile(&r, |_| Some(1.0));
    let segs = [segment(0.0, 512.0, SurfaceClass::Paved), segment(512.0, 1_000.0, SurfaceClass::Unpaved)];
    let pts = select(&r, &p, &segs);
    assert_eq!(with(&pts, BreakpointReason::SurfaceChange), [512.0]);
    // Espacement à 20 m : un extremum à 12 m d'un point d'entrée est abandonné ; à 5 m (défaut),
    // il est inséré ; à 3 m, rattaché.
    let r = route(&[0.0, 488.0, 1_000.0]);
    let bump = |c: f64| move |d: f64| Some(100.0 - (d - c).abs() * 0.05);
    let wide = BreakpointRules { min_spacing_m: 20.0, ..BreakpointRules::default() };
    let pts = select_breakpoints(&r, &profile(&r, bump(500.0)), &[], &wide);
    assert!(with(&pts, BreakpointReason::LocalMax).is_empty(), "{pts:?}");
    assert_eq!(inserted(&pts).len(), 0);
    let pts = select(&r, &profile(&r, bump(500.0)), &[]);
    assert_eq!(with(&pts, BreakpointReason::LocalMax), [500.0]);
    let pts = select(&r, &profile(&r, bump(491.0)), &[]);
    let max = with(&pts, BreakpointReason::LocalMax);
    assert!(max.len() == 1 && (max[0] - 488.0).abs() < 1e-6, "{max:?}");
    assert_eq!(inserted(&pts).len(), 0);
}

#[test]
fn density_cap_keeps_class_a() {
    let r = route(&[0.0, 3_000.0]);
    // Bosses de 8 m tous les 80 m sur le premier km : ≈ 25 extrema.
    let p0 =
        profile(&r, |d| Some(if d < 1_000.0 { 4.0 * (d / 80.0 * std::f64::consts::TAU).sin() } else { 0.0 }));
    let pts = select(&r, &p0, &[]);
    check_properties(&r, &pts);
    let extrema = inserted(&pts).len();
    assert!(extrema <= 20, "{extrema}");
    let turns = p0.gain.turning_points.len();
    assert!(turns > 20, "{turns} : le cas doit dépasser le plafond");
    // 30 ouvrages sur un km : tous gardés (classe A), extrema retirés.
    let mut p = p0.clone();
    p.structures = (0..15)
        .map(|k| outcome(k as f64 * 60.0 + 10.0, k as f64 * 60.0 + 40.0, StructureKind::Bridge))
        .collect();
    let pts = select(&r, &p, &[]);
    assert_eq!(with(&pts, BreakpointReason::StructureStart).len(), 15);
    assert_eq!(with(&pts, BreakpointReason::StructureEnd).len(), 15);
    let c = inserted(&pts)
        .iter()
        .filter(|q| {
            q.reasons.iter().all(|x| matches!(x, BreakpointReason::LocalMax | BreakpointReason::LocalMin))
        })
        .count();
    assert_eq!(c, 0, "fenêtre déjà pleine de classe A : aucun extremum");
}

#[test]
fn each_counted_leg_has_both_ends_in_points() {
    // Points d'entrée tous les 400 m, relief de 30 m de période 700 m.
    let at: Vec<f64> = (0..=20).map(|k| k as f64 * 400.0).collect();
    let r = route(&at);
    let p = profile(&r, |d| Some(200.0 + 15.0 * (d / 700.0 * std::f64::consts::TAU).sin() + d * 0.002));
    let rules = BreakpointRules::default();
    let pts = select(&r, &p, &[]);
    check_properties(&r, &pts);
    for t in &p.gain.turning_points {
        let d = p.d[t.station];
        assert!(
            pts.iter().any(|q| (q.d_m - d).abs() <= p.gain.params.smoothing_m + rules.min_spacing_m),
            "extrémité à {d} m sans point"
        );
    }
    // D+ des points sous le seuil du preset : à 2 % du D+ du profil.
    let e: Vec<f64> = pts.iter().map(|q| q.ele.unwrap()).collect();
    let (g, _) = accumulate(&e, p.gain.params.threshold_m);
    assert!((g - p.gain.gain_m).abs() / p.gain.gain_m < 0.02, "{g} vs {}", p.gain.gain_m);
}

/// Générateur déterministe (xorshift) : pas de dépendance `proptest` hors ligne.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[test]
fn properties_on_random_routes_and_terrains() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for case in 0..200 {
        let n = 2 + (rng.next() * 60.0) as usize;
        let mut at = vec![0.0];
        for _ in 1..n {
            // Pas de 0 (doublon consécutif) à 3 km, demi-tours compris.
            let step = if rng.next() < 0.1 { 0.0 } else { rng.next().powi(3) * 3_000.0 };
            let back = rng.next() < 0.1;
            let last = *at.last().unwrap();
            at.push(if back { (last - step).max(0.0) } else { last + step });
        }
        let r = route(&at);
        if r.input_pos.len() < 2 {
            continue;
        }
        let (a, b, c) = (rng.next() * 40.0, 200.0 + rng.next() * 2_000.0, rng.next() * 0.05);
        let hole = rng.next() * r.length_m;
        let hole_len = if rng.next() < 0.3 { rng.next() * 400.0 } else { 0.0 };
        let p = profile(&r, |d| {
            (!(hole..hole + hole_len).contains(&d))
                .then_some(a * (d / b * std::f64::consts::TAU).sin() + c * d + (d * 0.37).sin())
        });
        let mut p = p;
        let l = r.length_m;
        p.structures = vec![
            outcome(l * 0.2, l * 0.25, StructureKind::Bridge),
            outcome(l * 0.6, l * 0.7, StructureKind::Tunnel),
        ];
        let segs = [segment(0.0, l * 0.5, SurfaceClass::Paved), segment(l * 0.5, l, SurfaceClass::Unpaved)];
        let pts = select(&r, &p, &segs);
        check_properties(&r, &pts);
        // Bornes d'ouvrage et de trou portées par un point (à `snap_m` près, ou par la fusion).
        let rules = BreakpointRules::default();
        let near = |d: f64, reason: BreakpointReason| {
            pts.iter()
                .any(|q| q.reasons.contains(&reason) && (q.d_m - d).abs() <= rules.merge_m + rules.snap_m)
        };
        for o in &p.structures {
            assert!(near(o.from_m, BreakpointReason::StructureStart), "cas {case}");
            assert!(near(o.to_m, BreakpointReason::StructureEnd), "cas {case}");
        }
        for m in &p.missing {
            assert!(near(m.from_m, BreakpointReason::ElevationGapStart), "cas {case}");
            assert!(near(m.to_m, BreakpointReason::ElevationGapEnd), "cas {case}");
        }
        // Plafond par km glissant : seuls des A et B peuvent le dépasser.
        let ins: Vec<f64> = inserted(&pts).iter().map(|q| q.d_m).collect();
        let c_only: Vec<f64> = inserted(&pts)
            .iter()
            .filter(|q| q.reasons.iter().all(|x| x.class() == Class::C))
            .map(|q| q.d_m)
            .collect();
        for &x in &c_only {
            for &s in ins.iter().filter(|&&s| s <= x && x < s + 1_000.0) {
                let count = ins.iter().filter(|&&y| y >= s && y < s + 1_000.0).count();
                assert!(count <= rules.max_inserted_per_km, "cas {case} : {count} points sur un km");
            }
        }
        // Déterminisme.
        assert_eq!(pts, select(&r, &p, &segs));
    }
}
