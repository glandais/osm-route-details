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

/// Valeurs `bicycle=*` qui la ferment.
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
        Some(v) if BICYCLE_NO.contains(&v) => false,
        _ => !BICYCLE_CLOSED_KINDS.contains(&kind),
    }
}

/// Route motorisée (`way_kind` route principale ou secondaire) connue des tuiles mais fermée au vélo
/// (autoroute, trunk, route `bicycle=no`…) : hors du graphe, elle reste un état « hors graphe » du
/// HMM (T03 §5.3). Les rails n'en sont pas, ni les voies non motorisées fermées au vélo (rue
/// piétonne, sentier `bicycle=no`), que le cycliste emprunte à côté d'une voie du graphe.
pub fn bicycle_closed(a: &WayAttrs) -> bool {
    !a.rail && matches!(way_kind(&a.highway), WayKind::MajorRoad | WayKind::MinorRoad) && !bicycle_routable(a)
}

/// Revêtement : tag regroupé, puis `tracktype`, puis `kind`, sinon inconnu.
pub fn surface(a: &WayAttrs) -> (SurfaceClass, SurfaceSource) {
    match &a.surface {
        SurfaceTag::Grouped(PavedGroup::Paved) => return (SurfaceClass::Paved, SurfaceSource::Tagged),
        SurfaceTag::Grouped(PavedGroup::Unpaved) => return (SurfaceClass::Unpaved, SurfaceSource::Tagged),
        // `Raw` (V2) : table brute à venir ; une valeur inconnue passe à la règle suivante.
        SurfaceTag::Raw(_) | SurfaceTag::Absent => {}
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
         {INFERRED_PAVED:?}|{INFERRED_UNPAVED:?}|{WAY_KINDS:?}|grade1=paved,grade2..5=unpaved"
    )
}
