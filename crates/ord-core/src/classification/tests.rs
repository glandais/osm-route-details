//! Table de T03 §7 exécutée ligne à ligne, exhaustivité, combinaisons de la calibration
//! d'osm-surface (`bench/data/osm-surface/eval-report.md`) en non-régression.

use std::sync::Arc;

use ord_domain::street::{
    PavedGroup, SchemaCapabilities, StructureTag, SurfaceFidelity, SurfaceTag, TrackType, WayAttrs,
};

use super::tables::{bicycle_routable, ROUTABLE_KINDS};
use super::*;

pub(crate) fn caps() -> SchemaCapabilities {
    SchemaCapabilities {
        surface: SurfaceFidelity::Grouped,
        tracktype: true,
        layer: false,
        ford_ways: false,
        ford_nodes: false,
        covered_distinct: false,
        osm_id: false,
        smoothness: false,
        lines_merged: true,
    }
}

pub(crate) fn way(kind: &str) -> WayAttrs {
    WayAttrs {
        highway: Arc::from(kind),
        surface: SurfaceTag::Absent,
        tracktype: None,
        bicycle: None,
        service: None,
        link: false,
        rail: false,
        structure: StructureTag::None,
        layer: None,
        smoothness: None,
        osm_id: None,
        raw: None,
    }
}

fn with(kind: &str, surface: Option<PavedGroup>, tracktype: Option<TrackType>) -> WayAttrs {
    WayAttrs { surface: surface.map_or(SurfaceTag::Absent, SurfaceTag::Grouped), tracktype, ..way(kind) }
}

use PavedGroup::{Paved as P, Unpaved as U};
use SurfaceClass as C;
use SurfaceSource as S;

/// (kind, surface regroupée, tracktype) → (classe, source, way_kind)
type Row = (&'static str, Option<PavedGroup>, Option<TrackType>, C, S, WayKind);

#[test]
fn surface_table_row_by_row() {
    let rows: &[Row] = &[
        ("track", Some(P), Some(TrackType::Grade4), C::Paved, S::Tagged, WayKind::Track),
        ("residential", Some(U), None, C::Unpaved, S::Tagged, WayKind::MinorRoad),
        ("track", None, Some(TrackType::Grade1), C::Paved, S::InferredTracktype, WayKind::Track),
        ("track", None, Some(TrackType::Grade2), C::Unpaved, S::InferredTracktype, WayKind::Track),
        ("unclassified", None, Some(TrackType::Grade5), C::Unpaved, S::InferredTracktype, WayKind::MinorRoad),
        ("primary", None, None, C::Paved, S::InferredHighway, WayKind::MajorRoad),
        ("tertiary", None, None, C::Paved, S::InferredHighway, WayKind::MajorRoad),
        ("living_street", None, None, C::Paved, S::InferredHighway, WayKind::MinorRoad),
        ("service", None, None, C::Paved, S::InferredHighway, WayKind::MinorRoad),
        ("road", None, None, C::Paved, S::InferredHighway, WayKind::MinorRoad),
        ("busway", None, None, C::Paved, S::InferredHighway, WayKind::MinorRoad),
        ("cycleway", None, None, C::Paved, S::InferredHighway, WayKind::Cycleway),
        ("steps", None, None, C::Paved, S::InferredHighway, WayKind::Steps),
        ("track", None, None, C::Unpaved, S::InferredHighway, WayKind::Track),
        ("path", None, None, C::Unpaved, S::InferredHighway, WayKind::Path),
        ("bridleway", None, None, C::Unpaved, S::InferredHighway, WayKind::Path),
        ("footway", None, None, C::Unpaved, S::InferredHighway, WayKind::Footway),
        ("pedestrian", None, None, C::Unpaved, S::InferredHighway, WayKind::Footway),
        ("sidewalk", None, None, C::Unpaved, S::InferredHighway, WayKind::Footway),
        ("crossing", None, None, C::Unpaved, S::InferredHighway, WayKind::Footway),
        ("raceway", None, None, C::Unknown, S::None, WayKind::Unknown),
    ];
    for (kind, surface, tt, class, source, wk) in rows {
        let c = classify(&with(kind, *surface, *tt), &caps());
        assert_eq!((c.surface_class, c.surface_source, c.way_kind), (*class, *source, *wk), "{kind}");
    }
}

#[test]
fn structure_and_level() {
    let mut a = way("primary");
    for (tag, class, level) in [
        (StructureTag::None, StructureClass::Ground, LevelKind::Ground),
        (StructureTag::Bridge, StructureClass::Bridge, LevelKind::Bridge),
        (StructureTag::Tunnel, StructureClass::Tunnel, LevelKind::Tunnel),
        (StructureTag::Covered, StructureClass::Tunnel, LevelKind::Tunnel),
        (StructureTag::BuildingPassage, StructureClass::Tunnel, LevelKind::Tunnel),
        (StructureTag::Ford, StructureClass::Ground, LevelKind::Ground),
    ] {
        a.structure = tag;
        let c = classify(&a, &caps());
        assert_eq!((c.structure, c.level.kind), (class, level), "{tag:?}");
    }
    a.structure = StructureTag::Ford;
    let fords = SchemaCapabilities { ford_ways: true, ..caps() };
    assert_eq!(classify(&a, &fords).structure, StructureClass::Ford);
    // Route en crête de barrage taguée bridge=yes, layer=1 (`roselend-barrage`) : un pont.
    a.structure = StructureTag::Bridge;
    a.layer = Some(1);
    assert_eq!(classify(&a, &caps()).structure, StructureClass::Bridge);
    a.layer = Some(2);
    assert_eq!(classify(&a, &caps()).level.layer, 0, "layer ignoré sans capacité");
    assert_eq!(classify(&a, &SchemaCapabilities { layer: true, ..caps() }).level.layer, 2);
}

#[test]
fn every_known_combination_is_consistent() {
    let kinds = ROUTABLE_KINDS.iter().copied().chain(["raceway", "", "rail"]);
    let surfaces = [None, Some(P), Some(U)];
    let tts = [
        None,
        Some(TrackType::Grade1),
        Some(TrackType::Grade2),
        Some(TrackType::Grade3),
        Some(TrackType::Grade4),
        Some(TrackType::Grade5),
    ];
    for kind in kinds {
        for s in surfaces {
            for tt in tts {
                let c = classify(&with(kind, s, tt), &caps());
                assert_eq!(c.surface_source == S::None, c.surface_class == C::Unknown, "{kind} {s:?} {tt:?}");
            }
        }
    }
}

#[test]
fn bicycle_access_france() {
    for kind in ["motorway", "trunk", "pedestrian", "bridleway", "busway", "raceway"] {
        assert!(!bicycle_routable(&way(kind)), "{kind}");
    }
    for kind in ["primary", "residential", "track", "footway", "sidewalk", "steps", "path", "cycleway"] {
        assert!(bicycle_routable(&way(kind)), "{kind}");
    }
    let mut ped = way("pedestrian");
    ped.bicycle = Some(Arc::from("yes"));
    assert!(bicycle_routable(&ped), "bicycle=* prime");
    let mut path = way("path");
    path.bicycle = Some(Arc::from("no"));
    assert!(bicycle_routable(&path), "bicycle=no ne ferme que les routes (2026.11.10)");
    let mut road = way("residential");
    road.bicycle = Some(Arc::from("no"));
    assert!(!bicycle_routable(&road));
    let mut rail = way("service");
    rail.rail = true;
    assert!(!bicycle_routable(&rail));
}

/// Combinaisons les plus longues de la calibration d'osm-surface (classe Valhalla regroupée).
#[test]
fn osm_surface_calibration_combinations() {
    let mut designated = with("path", Some(U), Some(TrackType::Grade2));
    designated.bicycle = Some(Arc::from("designated"));
    let rows: Vec<(WayAttrs, C)> = vec![
        (with("track", None, None), C::Unpaved),
        (with("path", None, None), C::Unpaved),
        (with("path", Some(U), None), C::Unpaved),
        (with("unclassified", Some(P), Some(TrackType::Grade2)), C::Paved),
        (with("track", Some(U), None), C::Unpaved),
        (with("footway", None, None), C::Unpaved),
        (with("service", Some(P), Some(TrackType::Grade1)), C::Paved),
        (with("residential", None, None), C::Paved),
        (designated, C::Unpaved),
    ];
    for (a, class) in rows {
        assert_eq!(classify(&a, &caps()).surface_class, class, "{a:?}");
    }
    assert!(classify(&way("footway"), &caps()).footway_like);
    assert!(!classify(&way("cycleway"), &caps()).footway_like);
}

#[test]
fn public_values_are_snake_case() {
    assert_eq!(StructureClass::Unknown.as_str(), "unknown");
    assert_eq!(SurfaceSource::InferredTracktype.as_str(), "inferred_tracktype");
    assert_eq!(WayKind::MajorRoad.as_str(), "major_road");
    assert_eq!(StructureSource::OsmHeuristic.as_str(), "osm_heuristic");
}

fn raw(kind: &str, surface: &str, tracktype: Option<TrackType>) -> WayAttrs {
    WayAttrs { surface: SurfaceTag::Raw(Arc::from(surface)), tracktype, ..way(kind) }
}

/// Valeurs brutes (VersaTiles shortbread@1.1) : classe du regroupement de Valhalla, celui du banc
/// (`Class::of_valhalla`), source `tagged` ; les valeurs les plus fréquentes du cache de banc et
/// les cas ambigus.
#[test]
fn raw_surface_follows_the_valhalla_grouping() {
    use super::tables::valhalla_surface;
    let rows: &[(&str, Option<&str>, C)] = &[
        ("asphalt", Some("paved_smooth"), C::Paved),
        ("concrete", Some("paved_smooth"), C::Paved),
        ("concrete:plates", Some("paved_smooth"), C::Paved),
        ("chipseal", Some("paved_smooth"), C::Paved),
        ("metal", Some("paved_smooth"), C::Paved),
        ("metal_grid", Some("paved_smooth"), C::Paved),
        ("paving_stones", Some("paved"), C::Paved),
        ("sett", Some("paved"), C::Paved),
        ("grass_paver", Some("paved"), C::Paved),
        ("cobblestone", Some("paved_rough"), C::Paved),
        ("unhewn_cobblestone", Some("paved_rough"), C::Paved),
        ("brick", Some("paved_rough"), C::Paved),
        ("stone", Some("paved_rough"), C::Paved),
        ("compacted", Some("compacted"), C::Unpaved),
        ("fine_gravel", Some("gravel"), C::Unpaved),
        ("gravel", Some("gravel"), C::Unpaved),
        ("pebblestone", Some("gravel"), C::Unpaved),
        ("sand", Some("gravel"), C::Unpaved),
        ("wood", Some("compacted"), C::Unpaved),
        ("woodchips", Some("compacted"), C::Unpaved),
        ("boardwalk", Some("compacted"), C::Unpaved),
        ("ground", Some("dirt"), C::Unpaved),
        ("dirt", Some("dirt"), C::Unpaved),
        ("earth", Some("dirt"), C::Unpaved),
        ("mud", Some("dirt"), C::Unpaved),
        ("grass", Some("path"), C::Unpaved),
        ("rock", Some("path"), C::Unpaved),
        ("stepping_stones", Some("path"), C::Unpaved),
        // Valeurs composées : première entrée de la table de Valhalla, pas premier terme.
        ("asphalt;ground", Some("paved_smooth"), C::Paved),
        ("dirt;sand", Some("dirt"), C::Unpaved),
        ("compacted;asphalt", Some("paved_smooth"), C::Paved),
        ("unpaved", Some("gravel"), C::Unpaved),
        ("paved", Some("paved_smooth"), C::Paved),
        ("Asphalt", Some("paved_smooth"), C::Paved),
    ];
    for &(v, valhalla, class) in rows {
        assert_eq!(valhalla_surface(v), valhalla, "{v}");
        let c = classify(&raw("track", v, Some(TrackType::Grade5)), &caps());
        assert_eq!((c.surface_class, c.surface_source), (class, S::Tagged), "{v}");
    }
}

/// Valeur brute non reconnue par Valhalla : règle suivante (`tracktype`, puis `kind`), source
/// inférée, jamais `tagged`.
#[test]
fn unknown_raw_surface_falls_through() {
    for v in ["bicouche", "tiles", "none", "no", "slate", "aslphalt", "4", "stones"] {
        assert_eq!(super::tables::valhalla_surface(v), None, "{v}");
        let c = classify(&raw("track", v, Some(TrackType::Grade1)), &caps());
        assert_eq!((c.surface_class, c.surface_source), (C::Paved, S::InferredTracktype), "{v}");
        let c = classify(&raw("path", v, None), &caps());
        assert_eq!((c.surface_class, c.surface_source), (C::Unpaved, S::InferredHighway), "{v}");
        let c = classify(&raw("raceway", v, None), &caps());
        assert_eq!((c.surface_class, c.surface_source), (C::Unknown, S::None), "{v}");
    }
}

/// `bicycle` normalisé de shortbread@1.1 (`yes` / `limited` / `no`) : seul `no` ferme, et sur une
/// route motorisée seulement (état « hors graphe », `bicycle_forbidden`) ; sur une voie non routière,
/// `no` ne ferme rien (règles 2026.11.10) ; `limited` laisse l'accès par défaut de la classe.
#[test]
fn normalized_bicycle_access() {
    use super::tables::bicycle_closed;
    let with_bike = |kind: &str, v: &str| WayAttrs { bicycle: Some(Arc::from(v)), ..way(kind) };
    for kind in ["primary", "residential", "service", "unclassified"] {
        assert!(
            !bicycle_routable(&with_bike(kind, "no")) && bicycle_closed(&with_bike(kind, "no")),
            "{kind}"
        );
        assert!(bicycle_routable(&with_bike(kind, "yes")), "{kind}");
        assert!(bicycle_routable(&with_bike(kind, "limited")), "{kind}");
    }
    for kind in ["path", "track", "footway", "cycleway", "steps", "sidewalk", "crossing"] {
        assert!(
            bicycle_routable(&with_bike(kind, "no")) && !bicycle_closed(&with_bike(kind, "no")),
            "{kind}"
        );
        assert!(bicycle_routable(&with_bike(kind, "limited")), "{kind}");
    }
    // Classes fermées par défaut : `no` n'y change rien, elles restent fermées ; pedestrian et
    // bridleway hors de l'état « hors graphe », motorway, trunk et busway dans cet état.
    for kind in ["pedestrian", "bridleway"] {
        assert!(
            !bicycle_routable(&with_bike(kind, "no")) && !bicycle_closed(&with_bike(kind, "no")),
            "{kind}"
        );
    }
    for kind in ["motorway", "trunk", "busway", "secondary"] {
        assert!(
            !bicycle_routable(&with_bike(kind, "no")) && bicycle_closed(&with_bike(kind, "no")),
            "{kind}"
        );
    }
    assert!(bicycle_routable(&with_bike("pedestrian", "yes")));
    // `limited` : accès par défaut de la classe (trunk fermé, hors graphe).
    assert!(
        !bicycle_routable(&with_bike("trunk", "limited")) && bicycle_closed(&with_bike("trunk", "limited"))
    );
    assert!(!bicycle_routable(&with_bike("pedestrian", "limited")));
}
