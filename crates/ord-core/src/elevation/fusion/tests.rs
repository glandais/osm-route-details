//! Les 10 invariants de doc 01 §5.5 (T05 §2.1), un test chacun, sur des réseaux et des terrains
//! synthétiques : pont de 120 m (−60 à +60 m est) à 30 m au-dessus d'une vallée, sol à 100 m.

use ord_domain::elevation::{DemKind, DsmSample, Missing, TerrainOrigin, TerrainSample};
use ord_domain::geo::EARTH_RADIUS_M;
use ord_domain::report::WarningCode;
use ord_domain::request::RequestOptions;
use ord_domain::route::PreparedRoute;
use ord_domain::LatLon;

use super::plan::{plan, AxisPoint};
use super::OutcomeMethod;
use crate::classification::StructureSource;
use crate::elevation::{fuse, EleMethod, ElevationProfile, RawProfile, StructureSamples};
use crate::rules::Rules;
use crate::typology::tests::{bridge, overpass, tunnel, typology_of, with_surface, Net, ORIGIN};
use crate::typology::{PassageDoubt, TypologyOutput};
use ord_domain::street::PavedGroup;

/// Mètres est et nord d'un point.
fn en(p: LatLon) -> (f64, f64) {
    let n = (p.lat - ORIGIN.lat).to_radians() * EARTH_RADIUS_M;
    let e = (p.lon - ORIGIN.lon).to_radians() * EARTH_RADIUS_M * ORIGIN.lat.to_radians().cos();
    (e, n)
}

/// Terrain : vallée de 30 m sous le pont est-ouest ; MNS = tablier à 100 m sur le pont, sol ailleurs.
fn mnt(p: LatLon) -> f64 {
    let (e, n) = en(p);
    if n.abs() < 30.0 && e.abs() < 55.0 {
        70.0
    } else {
        100.0
    }
}

fn mns(p: LatLon) -> f64 {
    let (e, n) = en(p);
    if n.abs() < 4.0 && e.abs() <= 60.0 {
        100.0 + 0.01 * e
    } else {
        mnt(p)
    }
}

#[derive(Clone, Copy)]
struct Terrain {
    origin: fn(LatLon) -> TerrainOrigin,
    /// MNT lu.
    ground: fn(LatLon) -> f64,
}

fn ign(_: LatLon) -> TerrainOrigin {
    TerrainOrigin::IgnLidarHd
}

fn sample(p: LatLon, t: Terrain) -> TerrainSample {
    TerrainSample {
        ele: Some((t.ground)(p) as f32),
        origin: (t.origin)(p),
        fallback_after_error: false,
        dem_kind: DemKind::Dtm,
        missing: None,
    }
}

struct Run {
    route: PreparedRoute,
    typo: TypologyOutput,
    profile: ElevationProfile,
    raw: RawProfile,
}

fn run_with(
    net: Net,
    pts: &[(f64, f64)],
    t: Terrain,
    opts: &RequestOptions,
    edit: impl Fn(&mut RawProfile),
) -> Run {
    let (route, typo) = typology_of(net, pts);
    let rules = Rules::default();
    let terrain: Vec<TerrainSample> = route.samples.iter().map(|s| sample(s.at, t)).collect();
    let mut raw = RawProfile { terrain, structures: None, dsm: None };
    if opts.structure_correction {
        let pixel = |_: LatLon, _| 3.0;
        let plan = plan(&route, &raw.terrain, &typo, &rules, &pixel);
        let dsm = |p: LatLon| {
            if (t.origin)(p) == TerrainOrigin::IgnLidarHd {
                DsmSample::Value(mns(p) as f32)
            } else {
                DsmSample::NoData
            }
        };
        let route_dsm = plan.route_dsm.iter().map(|&i| dsm(route.samples[i].at)).collect();
        let off_terrain = plan.off_route.iter().map(|o| sample(o.at, t)).collect();
        let off_dsm = plan.off_route.iter().map(|o| o.dsm.then(|| dsm(o.at))).collect();
        raw.structures = Some(StructureSamples { plan, route_dsm, off_terrain, off_dsm });
    }
    edit(&mut raw);
    let profile = fuse(&route, &raw, Some(&typo), opts, &rules).expect("fusion");
    Run { route, typo, profile, raw }
}

fn run(net: Net, pts: &[(f64, f64)]) -> Run {
    run_with(net, pts, Terrain { origin: ign, ground: mnt }, &RequestOptions::default(), |_| {})
}

/// Altitude et méthode à la station la plus proche de `d`.
fn at_d(r: &Run, d: f64) -> (f64, EleMethod) {
    let i = r.route.samples.iter().position(|s| s.d_m >= d - 1e-6).unwrap_or(0);
    (r.profile.ele[i], r.profile.method[i])
}

fn codes(r: &Run) -> Vec<WarningCode> {
    r.profile.warnings.iter().map(|w| w.code).collect()
}

#[test]
fn deck_read_in_the_mns_on_a_crossed_bridge() {
    let r = run(overpass(), &[(-300.0, 0.0), (300.0, 0.0)]);
    let (z, m) = at_d(&r, 300.0);
    assert_eq!(m, EleMethod::DsmDeck);
    assert!((z - 100.0).abs() < 0.5, "{z}");
    assert_eq!(r.profile.structures.len(), 1);
    assert_eq!(r.profile.structures[0].method, OutcomeMethod::DsmDeck);
    assert_eq!(r.profile.correction.bridges, super::CorrectionStatus::Applied);
    assert_eq!(r.profile.correction.tunnels, super::CorrectionStatus::NoneFound);
}

#[test]
fn inv1_correction_before_smoothing() {
    // Viaduc à −30 m dans le MNT : aucune trace au-delà de la demi-fenêtre de lissage.
    let opts = RequestOptions { smoothing_m: 20.0, ..RequestOptions::default() };
    let r = run_with(
        overpass(),
        &[(-300.0, 0.0), (300.0, 0.0)],
        Terrain { origin: ign, ground: mnt },
        &opts,
        |_| {},
    );
    let s = r.profile.ele_smoothed.as_ref().expect("lissage");
    for (i, x) in r.route.samples.iter().enumerate() {
        let (e, _) = en(x.at);
        if e.abs() < 90.0 {
            continue;
        }
        assert!((s[i] - 100.0).abs() < 1.0, "station à {e:.0} m : {}", s[i]);
    }
}

#[test]
fn inv2_ends_are_the_structure_ends_not_the_trace() {
    // Départ au milieu du tablier : sonde ouest hors du parcours, sur la voie au-delà de la culée.
    // Départ à 20 m de la route au sol qui passe dessous (sinon l'appariement hésite).
    let r = run(overpass(), &[(20.0, 0.0), (300.0, 0.0)]);
    let (z, m) = at_d(&r, 0.0);
    assert_eq!(m, EleMethod::DsmDeck);
    assert!((z - 100.0).abs() < 0.5, "{z}");
    let plan = &r.raw.structures.as_ref().unwrap().plan;
    assert!(plan.groups[0].probes[0].is_some_and(|p| matches!(p.at, AxisPoint::Off(_))));
}

#[test]
fn inv3_contiguous_structures_are_one_group_with_one_entry_per_kind() {
    let net = Net::new()
        .add(with_surface("tertiary", PavedGroup::Paved), &[(-400.0, 0.0), (-60.0, 0.0)])
        .add(bridge("tertiary"), &[(-60.0, 0.0), (0.0, 0.0)])
        .add(tunnel("tertiary"), &[(0.0, 0.0), (200.0, 0.0)])
        .add(with_surface("tertiary", PavedGroup::Paved), &[(200.0, 0.0), (500.0, 0.0)]);
    let r = run(net, &[(-300.0, 0.0), (400.0, 0.0)]);
    let o = &r.profile.structures;
    assert_eq!(o.len(), 2, "{o:?}");
    assert_eq!(
        (o[0].kind, o[1].kind),
        (crate::typology::StructureKind::Bridge, crate::typology::StructureKind::Tunnel)
    );
    assert!((o[0].to_m - o[1].from_m).abs() < 1e-9, "entrées contiguës");
    assert_eq!(o[1].method, OutcomeMethod::InterpolatedStructure);
}

#[test]
fn inv4_doubt_keeps_the_raw_dem_and_warns() {
    let r = run_with(
        overpass(),
        &[(-300.0, 0.0), (300.0, 0.0)],
        Terrain { origin: ign, ground: mnt },
        &RequestOptions::default(),
        |raw| {
            let s = raw.structures.as_mut().unwrap();
            s.plan.groups[0].uncorrected = Some(super::plan::UncorrectedReason::Doubt);
        },
    );
    let (z, m) = at_d(&r, 300.0);
    assert_eq!((z, m), (70.0, EleMethod::Dem));
    assert!(codes(&r).contains(&WarningCode::StructureUncorrected));
    assert_eq!(r.profile.correction.bridges, super::CorrectionStatus::Partial);
    let _ = PassageDoubt::JumpInside;
}

#[test]
fn inv4_probe_without_altitude_keeps_the_raw_dem() {
    let r = run_with(
        overpass(),
        &[(-300.0, 0.0), (300.0, 0.0)],
        Terrain { origin: ign, ground: mnt },
        &RequestOptions::default(),
        |raw| {
            let p = raw.structures.as_ref().unwrap().plan.groups[0].probes[0].unwrap();
            if let AxisPoint::Route(i) = p.at {
                raw.terrain[i].ele = None;
                raw.terrain[i].missing = Some(Missing::NoData);
            }
        },
    );
    assert_eq!(at_d(&r, 300.0).1, EleMethod::Dem);
    assert!(codes(&r).contains(&WarningCode::StructureUncorrected));
}

#[test]
fn inv5_disabled_correction_keeps_the_dem() {
    let opts = RequestOptions { structure_correction: false, ..RequestOptions::default() };
    let r = run_with(
        overpass(),
        &[(-300.0, 0.0), (300.0, 0.0)],
        Terrain { origin: ign, ground: mnt },
        &opts,
        |_| {},
    );
    assert!(r.profile.method.iter().all(|m| *m == EleMethod::Dem));
    assert_eq!(r.profile.correction.bridges, super::CorrectionStatus::Disabled);
    assert_eq!(r.profile.structures[0].method.as_str(), "none");
    assert!(!r.typo.passages.is_empty(), "la typologie tourne toujours");
    assert!(!codes(&r).contains(&WarningCode::StructureUncorrected));
}

#[test]
fn inv6_passing_under_a_bridge_keeps_the_dem() {
    let r = run(overpass(), &[(0.0, -300.0), (0.0, 300.0)]);
    assert!(r.profile.method.iter().all(|m| *m == EleMethod::Dem));
    assert!(r.profile.structures.is_empty());
}

#[test]
fn inv7_every_station_carries_its_method() {
    let r = run(overpass(), &[(-300.0, 0.0), (300.0, 0.0)]);
    assert_eq!(r.profile.method.len(), r.route.samples.len());
    assert!(r.profile.method.iter().zip(&r.profile.ele).all(|(m, z)| (*m == EleMethod::None) == z.is_nan()));
    assert!(r.profile.method.contains(&EleMethod::DsmDeck) && r.profile.method.contains(&EleMethod::Dem));
}

#[test]
fn inv8_same_altitude_at_each_passage() {
    let r = run(overpass(), &[(-300.0, 0.0), (300.0, 0.0), (-300.0, 0.0)]);
    assert_eq!(r.typo.passages.len(), 2);
    let l = r.route.length_m;
    for d in [260.0, 300.0, 340.0] {
        let (a, ma) = at_d(&r, d);
        let (b, mb) = at_d(&r, l - d);
        assert_eq!((ma, mb), (EleMethod::DsmDeck, EleMethod::DsmDeck));
        assert!((a - b).abs() < 0.05, "{d} : {a} ≠ {b}");
    }
}

#[test]
fn inv9_mns_is_read_on_bridges_only() {
    let r = run(overpass(), &[(0.0, -300.0), (0.0, 300.0)]);
    let s = r.raw.structures.as_ref().unwrap();
    assert!(s.plan.route_dsm.is_empty() && s.plan.off_route.is_empty(), "aucune lecture MNS sans pont");
    // Sur un pont : seulement les stations de l'axe et ses sondes.
    let r = run(overpass(), &[(-300.0, 0.0), (300.0, 0.0)]);
    let s = r.raw.structures.as_ref().unwrap();
    for &i in &s.plan.route_dsm {
        let (e, _) = en(r.route.samples[i].at);
        assert!(e.abs() <= 75.0, "MNS lu à {e:.0} m de l'ouvrage");
    }
}

#[test]
fn inv10_no_ign_mns_against_a_foreign_ground() {
    // Culée ouest lue dans Mapterhorn : le pont passe en interpolation.
    fn west_mapterhorn(p: LatLon) -> TerrainOrigin {
        if en(p).0 < -55.0 {
            TerrainOrigin::Mapterhorn
        } else {
            TerrainOrigin::IgnLidarHd
        }
    }
    let r = run_with(
        overpass(),
        &[(-300.0, 0.0), (300.0, 0.0)],
        Terrain { origin: west_mapterhorn, ground: mnt },
        &RequestOptions::default(),
        |_| {},
    );
    let (z, m) = at_d(&r, 300.0);
    assert_eq!(m, EleMethod::InterpolatedStructure);
    assert!((z - 100.0).abs() < 0.5, "droite entre sondes : {z}");
    assert_eq!(r.profile.structures[0].method, OutcomeMethod::InterpolatedStructure);
}

#[test]
fn tunnel_is_a_straight_line_between_portals() {
    let net = Net::new()
        .add(with_surface("tertiary", PavedGroup::Paved), &[(-400.0, 0.0), (-60.0, 0.0)])
        .add(tunnel("tertiary"), &[(-60.0, 0.0), (60.0, 0.0)])
        .add(with_surface("tertiary", PavedGroup::Paved), &[(60.0, 0.0), (400.0, 0.0)]);
    let r = run(net, &[(-300.0, 0.0), (300.0, 0.0)]);
    let (z, m) = at_d(&r, 300.0);
    assert_eq!(m, EleMethod::InterpolatedStructure);
    assert!((z - 100.0).abs() < 1e-6, "{z}");
    assert_eq!(r.profile.correction.tunnels, super::CorrectionStatus::Applied);
}

/// Route en falaise : sol qui monte de 5 % vers l'est, paroi de 40 m au-dessus de la route entre
/// −200 et +200 m (tunnels et balcons confondus, le MNT ne voit pas la chaussée).
fn cliff(p: LatLon) -> f64 {
    let (e, _) = en(p);
    let road = 100.0 + 0.05 * e;
    if e.abs() < 200.0 {
        road + 40.0
    } else {
        road
    }
}

/// Deux tunnels de 150 m de part et d'autre d'un balcon à l'air libre de `gap` m, centré en 0.
fn tunnel_series(gap: f64) -> Net {
    let h = gap / 2.0;
    Net::new()
        .add(with_surface("tertiary", PavedGroup::Paved), &[(-600.0, 0.0), (-150.0 - h, 0.0)])
        .add(tunnel("tertiary"), &[(-150.0 - h, 0.0), (-h, 0.0)])
        .add(with_surface("tertiary", PavedGroup::Paved), &[(-h, 0.0), (h, 0.0)])
        .add(tunnel("tertiary"), &[(h, 0.0), (150.0 + h, 0.0)])
        .add(with_surface("tertiary", PavedGroup::Paved), &[(150.0 + h, 0.0), (600.0, 0.0)])
}

fn run_cliff(net: Net) -> Run {
    run_with(
        net,
        &[(-500.0, 0.0), (500.0, 0.0)],
        Terrain { origin: ign, ground: cliff },
        &RequestOptions::default(),
        |_| {},
    )
}

#[test]
fn tunnel_series_follows_one_line_across_a_short_balcony() {
    // Balcon de 100 m (< 120 m) : la série est une droite entre les portails extérieurs, balcon
    // compris ; le balcon reste une route (aucune entrée d'ouvrage n'y est inventée).
    let r = run_cliff(tunnel_series(100.0));
    let (z, m) = at_d(&r, 500.0);
    assert_eq!(m, EleMethod::InterpolatedStructure);
    assert!((z - 100.0).abs() < 0.5, "balcon sur la droite : {z}");
    for d in [300.0, 400.0, 600.0, 700.0] {
        let (z, m) = at_d(&r, d);
        let want = 100.0 + 0.05 * (d - 500.0);
        assert_eq!(m, EleMethod::InterpolatedStructure, "{d}");
        assert!((z - want).abs() < 0.6, "{d} : {z} ≠ {want}");
    }
    let o = &r.profile.structures;
    assert_eq!(o.len(), 2, "{o:?}");
    assert!(o.iter().all(|x| x.kind == crate::typology::StructureKind::Tunnel));
    assert!(o[0].to_m < 460.0 && o[1].from_m > 540.0, "balcon hors des entrées : {o:?}");
    assert_eq!(r.profile.correction.tunnels, super::CorrectionStatus::Applied);
    let plan = &r.raw.structures.as_ref().unwrap().plan;
    assert_eq!(plan.groups.len(), 1);
    assert_eq!(plan.groups[0].passages.len(), 2);
}

#[test]
fn tunnels_further_apart_are_corrected_one_by_one() {
    // Tronçon de 140 m (≥ 120 m) : deux groupes, le tronçon garde son MNT.
    let r = run_cliff(tunnel_series(140.0));
    let plan = &r.raw.structures.as_ref().unwrap().plan;
    assert_eq!(plan.groups.len(), 2);
    // OSM seul : le tronçon garde son MNT (paroi). Les heuristiques V1 (règles 2026.11.2) y voient
    // un tunnel (série de vcyclist, balcon compris) : la part hors des tunnels OSM (> 40 m) devient
    // un ouvrage `heuristic` corrigé, avec `structure_untagged`.
    let (z, m) = at_d(&r, 500.0);
    assert_eq!(m, EleMethod::InterpolatedStructure, "{z}");
    let h: Vec<_> = r.profile.structures.iter().filter(|o| o.source == StructureSource::Heuristic).collect();
    assert_eq!(h.len(), 1, "{:?}", r.profile.structures);
    assert!(r.profile.warnings.iter().any(|w| w.code == WarningCode::StructureUntagged));
}

#[test]
fn a_doubt_on_one_tunnel_leaves_the_whole_series_uncorrected() {
    let (route, mut typo) = typology_of(tunnel_series(100.0), &[(-500.0, 0.0), (500.0, 0.0)]);
    assert_eq!(typo.passages.len(), 2);
    typo.passages[1].doubts.push(PassageDoubt::JumpInside);
    let terrain: Vec<TerrainSample> =
        route.samples.iter().map(|s| sample(s.at, Terrain { origin: ign, ground: cliff })).collect();
    let p = plan(&route, &terrain, &typo, &Rules::default(), &|_: LatLon, _| 3.0);
    assert_eq!(p.groups.len(), 1);
    assert_eq!(p.groups[0].uncorrected, Some(super::plan::UncorrectedReason::Doubt));
}

#[test]
fn the_chord_grade_guard_judges_the_whole_series() {
    // Sol à l'est relevé de 60 m : chaque tunnel seul aurait une droite raide, la série (400 m
    // entre portails extérieurs, 60 m de dénivelé, 15 %) aussi : signalée une fois sur la série.
    fn steep(p: LatLon) -> f64 {
        let (e, _) = en(p);
        if e >= 200.0 {
            160.0
        } else if e > -200.0 {
            200.0
        } else {
            100.0
        }
    }
    let r = run_with(
        tunnel_series(100.0),
        &[(-500.0, 0.0), (500.0, 0.0)],
        Terrain { origin: ign, ground: steep },
        &RequestOptions::default(),
        |_| {},
    );
    let w: Vec<_> =
        r.profile.warnings.iter().filter(|w| w.code == WarningCode::StructureSlopeImplausible).collect();
    assert_eq!(w.len(), 1, "{w:?}");
}

/// Sol plat à 100 m ; sous le pont est-ouest (|nord| < 8 m), le MNT lit une bosse de 6 m (tablier
/// interpolé par le LiDAR) sur la route nord-sud.
fn bump_under_deck(p: LatLon) -> f64 {
    let (e, n) = en(p);
    if n.abs() < 8.0 && e.abs() < 10.0 {
        100.0 + 6.0 * (1.0 - n.abs() / 8.0)
    } else {
        100.0
    }
}

/// Même bosse, de 0,8 m : sous la rugosité du MNT d'une chaussée.
fn small_bump(p: LatLon) -> f64 {
    let (e, n) = en(p);
    if n.abs() < 8.0 && e.abs() < 10.0 {
        100.0 + 0.8 * (1.0 - n.abs() / 8.0)
    } else {
        100.0
    }
}

fn overpass_codes(r: &Run) -> usize {
    codes(r).iter().filter(|c| **c == WarningCode::ElevationOverpassCorrected).count()
}

#[test]
fn a_dem_bump_under_an_overpass_is_replaced_by_the_line_between_the_approaches() {
    let r = run_with(
        overpass(),
        &[(0.0, -300.0), (0.0, 300.0)],
        Terrain { origin: ign, ground: bump_under_deck },
        &RequestOptions::default(),
        |_| {},
    );
    let (z, m) = at_d(&r, 300.0);
    assert_eq!(m, EleMethod::GapFilled);
    assert!((z - 100.0).abs() < 0.01, "{z}");
    assert!(r.profile.ele.iter().all(|z| (z - 100.0).abs() < 0.01), "bosse retirée partout");
    assert_eq!(overpass_codes(&r), 1, "{:?}", codes(&r));
    let w = r.profile.warnings.iter().find(|w| w.code == WarningCode::ElevationOverpassCorrected).unwrap();
    let range = w.range.clone().unwrap();
    assert!(range.start > 285.0 && range.end < 315.0, "{range:?}");
    // Altitude seule : aucun ouvrage, la trace reste au sol.
    assert!(r.profile.structures.is_empty());
}

#[test]
fn the_overpass_correction_keeps_real_relief_and_its_own_bridge() {
    // Cuvette sous le pont (passage creusé) : gardée.
    let r = run(overpass(), &[(0.0, -300.0), (0.0, 300.0)]);
    assert!(r.profile.method.iter().all(|m| *m == EleMethod::Dem));
    assert_eq!(overpass_codes(&r), 0);
    // Vraie bosse sans pont au-dessus : la route est-ouest est au sol.
    let net = Net::new()
        .add(with_surface("residential", PavedGroup::Paved), &[(0.0, -400.0), (0.0, 400.0)])
        .add(with_surface("tertiary", PavedGroup::Paved), &[(-400.0, 0.0), (400.0, 0.0)]);
    let r = run_with(
        net,
        &[(0.0, -300.0), (0.0, 300.0)],
        Terrain { origin: ign, ground: bump_under_deck },
        &RequestOptions::default(),
        |_| {},
    );
    assert!((at_d(&r, 300.0).0 - 106.0).abs() < 0.5);
    assert_eq!(overpass_codes(&r), 0);
    // Trace sur le pont (ici sans sursol, sol gardé) : jamais la droite d'un passage dessous.
    let r = run_with(
        overpass(),
        &[(-300.0, 0.0), (300.0, 0.0)],
        Terrain { origin: ign, ground: bump_under_deck },
        &RequestOptions::default(),
        |_| {},
    );
    assert_eq!(r.profile.structures.len(), 1);
    assert!(!r.profile.method.contains(&EleMethod::GapFilled));
    assert_eq!(overpass_codes(&r), 0);
    // Bosse sous la rugosité du MNT : rien n'est réécrit.
    let r = run_with(
        overpass(),
        &[(0.0, -300.0), (0.0, 300.0)],
        Terrain { origin: ign, ground: small_bump },
        &RequestOptions::default(),
        |_| {},
    );
    assert!(r.profile.method.iter().all(|m| *m == EleMethod::Dem));
    // Correction désactivée : MNT brut.
    let opts = RequestOptions { structure_correction: false, ..RequestOptions::default() };
    let r = run_with(
        overpass(),
        &[(0.0, -300.0), (0.0, 300.0)],
        Terrain { origin: ign, ground: bump_under_deck },
        &opts,
        |_| {},
    );
    assert!((at_d(&r, 300.0).0 - 106.0).abs() < 0.5);
    assert_eq!(overpass_codes(&r), 0);
}

#[test]
fn the_overpass_correction_needs_the_lidar_dtm() {
    fn mapterhorn(_: LatLon) -> TerrainOrigin {
        TerrainOrigin::Mapterhorn
    }
    let r = run_with(
        overpass(),
        &[(0.0, -300.0), (0.0, 300.0)],
        Terrain { origin: mapterhorn, ground: bump_under_deck },
        &RequestOptions::default(),
        |_| {},
    );
    assert_eq!(overpass_codes(&r), 0);
}

/// Points `input_with_breakpoints` d'un passage, au croisement d'un pont qui enjambe la trace.
fn crossing_points(r: &Run) -> Vec<crate::elevation::OutputPoint> {
    use crate::elevation::{select_breakpoints, BreakpointReason};
    select_breakpoints(&r.route, &r.profile, &[], &Rules::default().elevation.breakpoints)
        .into_iter()
        .filter(|q| q.reasons.contains(&BreakpointReason::OverpassCrossing))
        .collect()
}

#[test]
fn a_breakpoint_carries_the_distance_mode_altitude_where_a_bridge_crosses_above() {
    use crate::elevation::{sample_output, PointOrigin};
    use ord_domain::request::Sampling;
    // Bosse de 0,8 m gardée (sous `min_bump_m`), sous la tolérance des ruptures de pente et
    // l'hystérésis des extrema : seul le croisement l'insère. Les points d'entrée en sont à 300 m.
    let r = run_with(
        overpass(),
        &[(0.0, -300.0), (0.0, 300.0)],
        Terrain { origin: ign, ground: small_bump },
        &RequestOptions::default(),
        |_| {},
    );
    assert_eq!(r.profile.overpass_crossings.len(), 1);
    let pts = crossing_points(&r);
    assert_eq!(pts.len(), 1, "{pts:?}");
    let q = &pts[0];
    assert_eq!(q.origin, Some(PointOrigin::Inserted));
    assert!((q.d_m - 300.0).abs() < 1.0, "{}", q.d_m);
    // Altitude du mode `distance` au même endroit.
    let distance = sample_output(
        &r.route,
        &r.profile,
        &Sampling::Distance { step_m: q.d_m },
        &[],
        &Rules::default().elevation.breakpoints,
    );
    assert!((distance[1].d_m - q.d_m).abs() < 1e-9);
    assert_eq!((q.ele, q.ele_method), (distance[1].ele, distance[1].ele_method));
    assert!((q.ele.unwrap() - 100.8).abs() < 0.1, "{:?}", q.ele);
    // Bosse corrigée : le point porte la droite entre les approches.
    let r = run_with(
        overpass(),
        &[(0.0, -300.0), (0.0, 300.0)],
        Terrain { origin: ign, ground: bump_under_deck },
        &RequestOptions::default(),
        |_| {},
    );
    let pts = crossing_points(&r);
    assert_eq!(pts.len(), 1);
    assert!((pts[0].ele.unwrap() - 100.0).abs() < 0.01 && pts[0].ele_method == EleMethod::GapFilled);
    // Trace sur le pont : aucun croisement par-dessous.
    let r = run(overpass(), &[(-300.0, 0.0), (300.0, 0.0)]);
    assert!(r.profile.overpass_crossings.is_empty() && crossing_points(&r).is_empty());
    // Correction désactivée : le point reste, au MNT brut.
    let opts = RequestOptions { structure_correction: false, ..RequestOptions::default() };
    let r = run_with(
        overpass(),
        &[(0.0, -300.0), (0.0, 300.0)],
        Terrain { origin: ign, ground: bump_under_deck },
        &opts,
        |_| {},
    );
    let pts = crossing_points(&r);
    assert_eq!(pts.len(), 1);
    assert!((pts[0].ele.unwrap() - 106.0).abs() < 0.5, "{:?}", pts[0].ele);
}

#[test]
fn a_crossing_a_few_metres_from_an_input_point_is_still_inserted() {
    use crate::elevation::PointOrigin;
    // Point d'entrée à 3 m du croisement, sur la bosse de 6 m gardée (correction désactivée) : il
    // ne porte pas le croisement (`overpass_snap_m`), un point est inséré au croisement.
    let opts = RequestOptions { structure_correction: false, ..RequestOptions::default() };
    let r = run_with(
        overpass(),
        &[(0.0, -300.0), (0.0, 3.0), (0.0, 300.0)],
        Terrain { origin: ign, ground: bump_under_deck },
        &opts,
        |_| {},
    );
    let pts = crossing_points(&r);
    assert_eq!(pts.len(), 1, "{pts:?}");
    assert_eq!(pts[0].origin, Some(PointOrigin::Inserted));
    assert!((pts[0].d_m - 300.0).abs() < 1.0, "{}", pts[0].d_m);
    assert_eq!(pts[0].ele, r.profile.at(pts[0].d_m).ele);
}
