//! Tables v1 de la classification (T03 §7) et règles d'accès vélo du graphe (T03 §4.2, étape 1).
//!
//! Accès et défauts repris d'osm-surface (`src/valhalla.rs`, Apache-2.0, © Gabriel Landais), règles
//! d'accès France : motorway, trunk, pedestrian, bridleway, busway fermés au vélo, footway ouvert,
//! `bicycle=*` prime, `rail` exclu. Toute modification change la sortie : `RULES_VERSION` monte et
//! l'empreinte du test `rules_fingerprint` est mise à jour dans le même commit.

use ord_domain::street::{PavedGroup, SurfaceTag, TrackType, WayAttrs};

use super::{SurfaceClass, SurfaceSource, WayKind};

/// Valeurs `kind` (highway) d'un réseau routable ; le reste (pistes, rails…) n'entre pas au graphe.
pub const ROUTABLE_KINDS: &[&str] = &[
    "motorway",
    "trunk",
    "primary",
    "secondary",
    "tertiary",
    "unclassified",
    "residential",
    "living_street",
    "pedestrian",
    "service",
    "track",
    "footway",
    "sidewalk",
    "crossing",
    "steps",
    "path",
    "cycleway",
    "bridleway",
    "busway",
    "road",
];

/// Accès vélo par défaut, France (adminconstants.h de Valhalla).
pub const BICYCLE_CLOSED_KINDS: &[&str] = &["motorway", "trunk", "pedestrian", "bridleway", "busway"];

/// Valeurs `bicycle=*` qui ouvrent la voie au vélo (table `access` de Valhalla).
pub const BICYCLE_YES: &[&str] = &[
    "yes",
    "private",
    "permissive",
    "use_sidepath",
    "delivery",
    "designated",
    "dismount",
    "destination",
    "customers",
    "official",
    "public",
    "restricted",
    "allowed",
    "permit",
    "residents",
    "mtb",
];

/// Valeurs `bicycle=*` qui la ferment, **sur une route motorisée seulement** (`way_kind`
/// `major_road` / `minor_road` : état « hors graphe », `bicycle_forbidden`). Sur une voie non
/// routière (track, path, cycleway, footway, steps…), elles ne ferment rien : l'accès par défaut de
/// la classe s'applique (décision de l'utilisateur, 2026-11-02, règles 2026.11.10 : shortbread@1.1
/// y met `no` pour un probable `access=private`, et la trace les suit). VersaTiles shortbread@1.1
/// normalise `bicycle` en `yes` / `limited` / `no` ; `limited` n'est dans aucune des deux listes, il
/// ne ferme rien et laisse l'accès par défaut de la classe.
pub const BICYCLE_NO: &[&str] = &["no", "agricultural", "discouraged", "forestry", "emergency", "psv"];

/// Voies piétonnes : malus d'émission (Valhalla rend le trottoir plus cher que la chaussée).
pub const FOOTWAY_LIKE: &[&str] = &["footway", "sidewalk", "crossing", "steps", "pedestrian"];

/// Revêtement inféré du `kind` (défauts de Valhalla, exploration 02 §6).
pub const INFERRED_PAVED: &[&str] = &[
    "motorway",
    "trunk",
    "primary",
    "secondary",
    "tertiary",
    "unclassified",
    "residential",
    "living_street",
    "service",
    "road",
    "busway",
    "cycleway",
    "steps",
];
pub const INFERRED_UNPAVED: &[&str] =
    &["track", "path", "footway", "pedestrian", "bridleway", "sidewalk", "crossing"];

/// Valeur `surface` brute → surface Valhalla (`pbfgraphparser.cc`, `tag_handlers_["surface"]`,
/// recopiée par osm-surface `surface_from_osm`, `src/valhalla.rs`), dans l'ordre de Valhalla : la
/// valeur passée en minuscules est cherchée comme **sous-chaîne** (`=` : égalité stricte), la
/// première entrée qui correspond gagne. La classe en découle comme au banc (`Class::of_valhalla`,
/// README du banc §4) : revêtu = `paved_smooth`, `paved`, `paved_rough` ; non revêtu = pire que
/// `paved_rough`. Une valeur qui ne correspond à rien (`bicouche`, `tiles`, `none`, fautes…) passe à
/// la règle suivante (`tracktype`, puis `kind`), comme chez Valhalla : sa source n'est pas `tagged`.
pub const RAW_SURFACE: &[(&str, &str)] = &[
    ("unpaved", "gravel"),
    ("paved", "paved_smooth"),
    ("pavement", "paved_smooth"),
    ("asphalt", "paved_smooth"),
    ("concrete", "paved_smooth"),
    ("cement", "paved_smooth"),
    ("chipseal", "paved_smooth"),
    ("metal", "paved_smooth"),
    ("tartan", "paved"),
    ("pavingstone", "paved"),
    ("paving_stones", "paved"),
    ("sett", "paved"),
    ("grass_paver", "paved"),
    ("rubber", "paved"),
    ("acrylic", "paved"),
    ("plastic", "paved"),
    ("cobblestone", "paved_rough"),
    ("brick", "paved_rough"),
    ("=stone", "paved_rough"),
    ("compacted", "compacted"),
    ("wood", "compacted"),
    ("boardwalk", "compacted"),
    ("dirt", "dirt"),
    ("natural", "dirt"),
    ("earth", "dirt"),
    ("ground", "dirt"),
    ("mud", "dirt"),
    ("clay", "dirt"),
    ("laterite", "dirt"),
    ("soil", "dirt"),
    ("gravel", "gravel"),
    ("pebblestone", "gravel"),
    ("sand", "gravel"),
    ("grass", "path"),
    ("artificial_turf", "path"),
    ("rock", "path"),
    ("stepping_stones", "path"),
];

/// Surfaces Valhalla revêtues (`unpaved` de Valhalla : pire que `paved_rough`).
pub const VALHALLA_PAVED: &[&str] = &["paved_smooth", "paved", "paved_rough"];

/// Surface Valhalla d'une valeur `surface` brute (table [`RAW_SURFACE`]) ; `None` : non reconnue.
pub fn valhalla_surface(raw: &str) -> Option<&'static str> {
    let v = raw.to_ascii_lowercase();
    RAW_SURFACE
        .iter()
        .find(|(pat, _)| match pat.strip_prefix('=') {
            Some(exact) => v == exact,
            None => v.contains(pat),
        })
        .map(|(_, s)| *s)
}

/// Classe d'une valeur `surface` brute ; `None` : non reconnue, règle suivante.
pub fn raw_surface_class(raw: &str) -> Option<SurfaceClass> {
    valhalla_surface(raw).map(|s| {
        if VALHALLA_PAVED.contains(&s) {
            SurfaceClass::Paved
        } else {
            SurfaceClass::Unpaved
        }
    })
}

/// `way_kind` par `kind` ; le reste donne `unknown`.
pub const WAY_KINDS: &[(&str, WayKind)] = &[
    ("motorway", WayKind::MajorRoad),
    ("trunk", WayKind::MajorRoad),
    ("primary", WayKind::MajorRoad),
    ("secondary", WayKind::MajorRoad),
    ("tertiary", WayKind::MajorRoad),
    ("unclassified", WayKind::MinorRoad),
    ("residential", WayKind::MinorRoad),
    ("living_street", WayKind::MinorRoad),
    ("service", WayKind::MinorRoad),
    ("road", WayKind::MinorRoad),
    ("busway", WayKind::MinorRoad),
    ("track", WayKind::Track),
    ("path", WayKind::Path),
    ("bridleway", WayKind::Path),
    ("cycleway", WayKind::Cycleway),
    ("footway", WayKind::Footway),
    ("pedestrian", WayKind::Footway),
    ("sidewalk", WayKind::Footway),
    ("crossing", WayKind::Footway),
    ("steps", WayKind::Steps),
];

/// La voie entre au graphe d'appariement du vélo.
pub fn bicycle_routable(a: &WayAttrs) -> bool {
    let kind = &*a.highway;
    if a.rail || !ROUTABLE_KINDS.contains(&kind) {
        return false;
    }
    match a.bicycle.as_deref() {
        Some(v) if BICYCLE_YES.contains(&v) => true,
        Some(v) if BICYCLE_NO.contains(&v) && is_road(kind) => false,
        _ => !BICYCLE_CLOSED_KINDS.contains(&kind),
    }
}

/// Route motorisée (`way_kind` `major_road` ou `minor_road`) : seule classe où `bicycle=no` ferme.
fn is_road(kind: &str) -> bool {
    matches!(way_kind(kind), WayKind::MajorRoad | WayKind::MinorRoad)
}

/// Route motorisée (`way_kind` route principale ou secondaire) connue des tuiles mais fermée au vélo
/// (autoroute, trunk, route `bicycle=no`…) : hors du graphe, elle reste un état « hors graphe » du
/// HMM (T03 §5.3). Les rails n'en sont pas, ni les voies non motorisées fermées au vélo par défaut
/// (rue piétonne, bridleway), que le cycliste emprunte à côté d'une voie du graphe ; un sentier
/// `bicycle=no` reste au graphe (règles 2026.11.10).
pub fn bicycle_closed(a: &WayAttrs) -> bool {
    !a.rail && is_road(&a.highway) && !bicycle_routable(a)
}

/// Revêtement : tag (regroupé ou brut), puis `tracktype`, puis `kind`, sinon inconnu.
pub fn surface(a: &WayAttrs) -> (SurfaceClass, SurfaceSource) {
    match &a.surface {
        SurfaceTag::Grouped(PavedGroup::Paved) => return (SurfaceClass::Paved, SurfaceSource::Tagged),
        SurfaceTag::Grouped(PavedGroup::Unpaved) => return (SurfaceClass::Unpaved, SurfaceSource::Tagged),
        // Valeur brute (VersaTiles shortbread@1.1, tuiles maison V2) : table de Valhalla ; une valeur
        // non reconnue passe à la règle suivante.
        SurfaceTag::Raw(v) => {
            if let Some(class) = raw_surface_class(v) {
                return (class, SurfaceSource::Tagged);
            }
        }
        SurfaceTag::Absent => {}
    }
    match a.tracktype {
        Some(TrackType::Grade1) => return (SurfaceClass::Paved, SurfaceSource::InferredTracktype),
        Some(_) => return (SurfaceClass::Unpaved, SurfaceSource::InferredTracktype),
        None => {}
    }
    let kind = &*a.highway;
    if INFERRED_PAVED.contains(&kind) {
        (SurfaceClass::Paved, SurfaceSource::InferredHighway)
    } else if INFERRED_UNPAVED.contains(&kind) {
        (SurfaceClass::Unpaved, SurfaceSource::InferredHighway)
    } else {
        (SurfaceClass::Unknown, SurfaceSource::None)
    }
}

pub fn way_kind(kind: &str) -> WayKind {
    WAY_KINDS.iter().find(|(k, _)| *k == kind).map_or(WayKind::Unknown, |(_, w)| *w)
}

/// Texte canonique des tables, pour l'empreinte des règles.
pub fn canonical() -> String {
    format!(
        "{ROUTABLE_KINDS:?}|{BICYCLE_CLOSED_KINDS:?}|{BICYCLE_YES:?}|{BICYCLE_NO:?}|{FOOTWAY_LIKE:?}|\
         {INFERRED_PAVED:?}|{INFERRED_UNPAVED:?}|{WAY_KINDS:?}|grade1=paved,grade2..5=unpaved|\
         bicycle_no=roads_only|\
         {RAW_SURFACE:?}|{VALHALLA_PAVED:?}|raw=lowercase,contains,first"
    )
}
