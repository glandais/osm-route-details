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
}

fn ign(_: LatLon) -> TerrainOrigin {
    TerrainOrigin::IgnLidarHd
}

fn sample(p: LatLon, t: Terrain) -> TerrainSample {
    TerrainSample {
        ele: Some(mnt(p) as f32),
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
    let mut raw = RawProfile { terrain, structures: None };
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
    run_with(net, pts, Terrain { origin: ign }, &RequestOptions::default(), |_| {})
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
    let r = run_with(overpass(), &[(-300.0, 0.0), (300.0, 0.0)], Terrain { origin: ign }, &opts, |_| {});
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
        Terrain { origin: ign },
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
        Terrain { origin: ign },
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
    let r = run_with(overpass(), &[(-300.0, 0.0), (300.0, 0.0)], Terrain { origin: ign }, &opts, |_| {});
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
        Terrain { origin: west_mapterhorn },
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
