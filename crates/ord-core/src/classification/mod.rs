//! Classification (T03 §7) : attributs neutres d'une voie → enums publiques du contrat. Appelée une
//! fois par classe d'attributs à la construction du graphe, jamais par segment.
//!
//! Les défauts de revêtement sont ceux de Valhalla, repris par osm-surface (`src/valhalla.rs`,
//! Apache-2.0, © Gabriel Landais) : la classe est `unpaved` dès que la surface Valhalla est pire que
//! `paved_rough`.

pub mod reconcile;
pub mod tables;

use ord_domain::street::{SchemaCapabilities, StructureTag, WayAttrs};
use serde::Serialize;

/// Nature d'un niveau du graphe : sol, pont et tunnel ne se relient qu'aux culées (T03 §4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LevelKind {
    Ground,
    Bridge,
    Tunnel,
}

/// Niveau d'une voie ; `layer` vaut 0 tant que la source ne le fournit pas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct Level {
    pub kind: LevelKind,
    pub layer: i8,
}

impl Level {
    pub const GROUND: Self = Self { kind: LevelKind::Ground, layer: 0 };

    pub fn is_structure(self) -> bool {
        self.kind != LevelKind::Ground
    }
}

macro_rules! public_enum {
    ($(#[$m:meta])* $name:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// Valeur publique (`snake_case`) du contrat.
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $s),+ }
            }
        }
    };
}

public_enum!(
    /// `segments[].structure` ; `ford` est réservé (V2), `unknown` = non apparié sans ouvrage.
    StructureClass { Ground => "ground", Bridge => "bridge", Tunnel => "tunnel", Ford => "ford", Unknown => "unknown" }
);
public_enum!(
    /// `surface.class`, gelée en v1.
    SurfaceClass { Paved => "paved", Unpaved => "unpaved", Unknown => "unknown" }
);
public_enum!(
    /// `surface.source`.
    SurfaceSource {
        Tagged => "tagged",
        InferredTracktype => "inferred_tracktype",
        InferredHighway => "inferred_highway",
        None => "none",
    }
);
public_enum!(
    /// `surface.detail` (api 1.6) : revêtement fin, rempli seulement depuis un tag `surface` brut
    /// reconnu (source `tagged`), jamais déduit. Vocabulaire fermé côté service, ouvert à
    /// l'extension (contrat §6) ; chaque valeur implique une seule `surface.class` (table
    /// [`tables::RAW_SURFACE`]).
    SurfaceDetail {
        Asphalt => "asphalt",
        Concrete => "concrete",
        PavingStones => "paving_stones",
        Sett => "sett",
        Cobblestone => "cobblestone",
        Compacted => "compacted",
        FineGravel => "fine_gravel",
        Gravel => "gravel",
        Dirt => "dirt",
        Sand => "sand",
        Grass => "grass",
        Wood => "wood",
        Rock => "rock",
    }
);

impl SurfaceDetail {
    /// Classe impliquée par la valeur fine.
    pub fn class(self) -> SurfaceClass {
        match self {
            Self::Asphalt | Self::Concrete | Self::PavingStones | Self::Sett | Self::Cobblestone => {
                SurfaceClass::Paved
            }
            _ => SurfaceClass::Unpaved,
        }
    }
}

public_enum!(
    /// `way_kind` (exploration 02 §9).
    WayKind {
        MajorRoad => "major_road",
        MinorRoad => "minor_road",
        Track => "track",
        Path => "path",
        Cycleway => "cycleway",
        Footway => "footway",
        Steps => "steps",
        Unknown => "unknown",
    }
);
public_enum!(
    /// `structure_source` : au MVP, tout ouvrage est `osm`.
    StructureSource { Osm => "osm", Heuristic => "heuristic", OsmHeuristic => "osm_heuristic" }
);

/// Résultat de la classification d'une classe d'attributs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Classified {
    pub level: Level,
    pub structure: StructureClass,
    pub surface_class: SurfaceClass,
    pub surface_source: SurfaceSource,
    /// Revêtement fin (api 1.6) : seulement depuis un tag brut reconnu.
    pub surface_detail: Option<SurfaceDetail>,
    pub way_kind: WayKind,
    /// Trottoir, escalier ou rue piétonne : malus d'émission du HMM (T03 §5.2, `footway_cost`).
    pub footway_like: bool,
}

impl Classified {
    /// Portion non appariée (sans passer par les règles).
    pub const UNMATCHED: Self = Self {
        level: Level::GROUND,
        structure: StructureClass::Unknown,
        surface_class: SurfaceClass::Unknown,
        surface_source: SurfaceSource::None,
        surface_detail: None,
        way_kind: WayKind::Unknown,
        footway_like: false,
    };
}

/// Niveau d'une voie (table `level_of`, T03 §4.2) ; `layer` seulement si la source le fournit.
pub fn level_of(a: &WayAttrs, caps: &SchemaCapabilities) -> Level {
    let kind = match a.structure {
        StructureTag::Bridge => LevelKind::Bridge,
        StructureTag::Tunnel | StructureTag::Covered | StructureTag::BuildingPassage => LevelKind::Tunnel,
        StructureTag::Ford | StructureTag::None => LevelKind::Ground,
    };
    let layer = if caps.layer { a.layer.unwrap_or(0) } else { 0 };
    Level { kind, layer }
}

/// Classe une voie (table v1 de T03 §7, première règle applicable).
pub fn classify(a: &WayAttrs, caps: &SchemaCapabilities) -> Classified {
    let structure = match a.structure {
        StructureTag::Bridge => StructureClass::Bridge,
        StructureTag::Tunnel | StructureTag::Covered | StructureTag::BuildingPassage => {
            StructureClass::Tunnel
        }
        StructureTag::Ford if caps.ford_ways => StructureClass::Ford,
        StructureTag::Ford | StructureTag::None => StructureClass::Ground,
    };
    let (surface_class, surface_source, surface_detail) = tables::surface(a);
    let kind = &*a.highway;
    Classified {
        level: level_of(a, caps),
        structure,
        surface_class,
        surface_source,
        surface_detail,
        way_kind: tables::way_kind(kind),
        footway_like: tables::FOOTWAY_LIKE.contains(&kind),
    }
}

pub use reconcile::{reconcile, DetectedStructure, GroupRef, StructureVerdict};

#[cfg(test)]
pub(crate) mod tests;
