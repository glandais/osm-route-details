//! Attributs d'une arête : filtre vélo (§1 de la note de conception) et empaquetage dans un `u64`.
//!
//! Ce module ne sait rien d'OSM au-delà de ses tags : il reçoit des paires `(clé, valeur)`.

use std::fmt;

/// Version de la disposition des bits (champ `attr_layout` de l'en-tête de fichier).
pub const ATTR_LAYOUT: u16 = 1;

/// Classe de route gardée (`ROUTABLE_KINDS` de T03). Le code 0 est réservé.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum HighwayClass {
    Motorway = 1,
    Trunk,
    Primary,
    Secondary,
    Tertiary,
    Unclassified,
    Residential,
    LivingStreet,
    Service,
    Track,
    Path,
    Cycleway,
    Footway,
    Bridleway,
    Steps,
    Pedestrian,
    Road,
    Busway,
}

impl HighwayClass {
    pub const ALL: [HighwayClass; 18] = [
        Self::Motorway,
        Self::Trunk,
        Self::Primary,
        Self::Secondary,
        Self::Tertiary,
        Self::Unclassified,
        Self::Residential,
        Self::LivingStreet,
        Self::Service,
        Self::Track,
        Self::Path,
        Self::Cycleway,
        Self::Footway,
        Self::Bridleway,
        Self::Steps,
        Self::Pedestrian,
        Self::Road,
        Self::Busway,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Motorway => "motorway",
            Self::Trunk => "trunk",
            Self::Primary => "primary",
            Self::Secondary => "secondary",
            Self::Tertiary => "tertiary",
            Self::Unclassified => "unclassified",
            Self::Residential => "residential",
            Self::LivingStreet => "living_street",
            Self::Service => "service",
            Self::Track => "track",
            Self::Path => "path",
            Self::Cycleway => "cycleway",
            Self::Footway => "footway",
            Self::Bridleway => "bridleway",
            Self::Steps => "steps",
            Self::Pedestrian => "pedestrian",
            Self::Road => "road",
            Self::Busway => "busway",
        }
    }

    /// Classe d'une valeur de `highway`, `_link` retiré ; le booléen dit si le suffixe était présent
    /// (seules les quatre classes routières principales ont un `_link`).
    pub fn from_tag(value: &str) -> Option<(Self, bool)> {
        let (base, link) = match value.strip_suffix("_link") {
            Some(base) => (base, true),
            None => (value, false),
        };
        let class = Self::ALL.iter().copied().find(|c| c.name() == base)?;
        let linkable =
            matches!(class, Self::Motorway | Self::Trunk | Self::Primary | Self::Secondary | Self::Tertiary);
        (!link || linkable).then_some((class, link))
    }

    pub fn from_code(code: u8) -> Option<Self> {
        Self::ALL.get(usize::from(code).checked_sub(1)?).copied()
    }

    /// Fermée aux vélos sans tag explicite (T03 §4.2).
    fn closed_by_default(self) -> bool {
        matches!(self, Self::Motorway | Self::Trunk | Self::Pedestrian | Self::Bridleway | Self::Busway)
    }
}

/// Accès vélo résolu (`bicycle` > `vehicle` > `access`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Access {
    Yes = 0,
    Destination = 1,
    Private = 2,
    No = 3,
}

impl Access {
    pub const ALL: [Access; 4] = [Self::Yes, Self::Destination, Self::Private, Self::No];

    pub fn name(self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::Destination => "destination",
            Self::Private => "private",
            Self::No => "no",
        }
    }

    fn from_code(code: u64) -> Self {
        Self::ALL[(code & 3) as usize]
    }

    fn from_tag(value: &str) -> Option<Self> {
        match value {
            "yes" | "permissive" | "designated" | "official" | "public" | "allowed" | "use_sidepath"
            | "dismount" | "discouraged" | "mtb" => Some(Self::Yes),
            "destination" | "delivery" | "customers" => Some(Self::Destination),
            "private" | "permit" | "residents" | "restricted" => Some(Self::Private),
            "no" | "agricultural" | "forestry" | "emergency" | "psv" | "bus" => Some(Self::No),
            _ => None,
        }
    }
}

/// Valeurs exactes de `surface` (code = position + 1) ; 31 = autre, 0 = absent.
pub const SURFACES: [&str; 30] = [
    "asphalt",
    "paved",
    "concrete",
    "paving_stones",
    "sett",
    "cobblestone",
    "unhewn_cobblestone",
    "compacted",
    "fine_gravel",
    "gravel",
    "unpaved",
    "ground",
    "dirt",
    "earth",
    "grass",
    "sand",
    "mud",
    "wood",
    "metal",
    "pebblestone",
    "rock",
    "concrete:plates",
    "concrete:lanes",
    "clay",
    "tartan",
    "rubber",
    "woodchips",
    "salt",
    "ice",
    "grass_paver",
];
pub const SURFACE_OTHER: u8 = 31;

const SMOOTHNESS: [&str; 8] =
    ["excellent", "good", "intermediate", "bad", "very_bad", "horrible", "very_horrible", "impassable"];

/// Routes mémorisées : bits relatifs (décalés de 32 dans l'`u64`).
pub const ROUTE_LOCAL: u8 = 1;
pub const ROUTE_REGIONAL: u8 = 2;
pub const ROUTE_NATIONAL: u8 = 4;
pub const ROUTE_MTB: u8 = 8;

/// Sens voiture.
pub const CAR_NONE: u8 = 0;
pub const CAR_FORWARD: u8 = 1;
pub const CAR_BACKWARD: u8 = 2;
pub const CAR_BOTH: u8 = 3;

/// Les attributs d'une arête non orientée, déployés. `pack` / `unpack` sont inverses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fields {
    pub class: u8,
    pub link: bool,
    pub surface: u8,
    pub tracktype: u8,
    pub smoothness: u8,
    pub mtb_scale: u8,
    pub car_oneway: u8,
    pub fwd_ok: bool,
    pub bwd_ok: bool,
    pub contraflow: bool,
    pub access: u8,
    pub bicycle_tag: u8,
    pub service: u8,
    pub routes: u8,
    pub infra: u8,
    pub bridge: bool,
    pub tunnel: bool,
}

/// Les `u64` d'arête, 40 bits utilisés (voir §1 de la note) ; les 24 autres sont réservés.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Attrs(pub u64);

impl fmt::Debug for Attrs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Attrs({:#012x})", self.0)
    }
}

fn bits(v: u64, shift: u32, width: u32) -> u64 {
    (v >> shift) & ((1 << width) - 1)
}

impl Attrs {
    pub fn class(self) -> Option<HighwayClass> {
        HighwayClass::from_code(bits(self.0, 0, 5) as u8)
    }
    pub fn link(self) -> bool {
        bits(self.0, 5, 1) == 1
    }
    pub fn surface(self) -> u8 {
        bits(self.0, 6, 5) as u8
    }
    pub fn tracktype(self) -> u8 {
        bits(self.0, 11, 3) as u8
    }
    pub fn smoothness(self) -> u8 {
        bits(self.0, 14, 4) as u8
    }
    pub fn mtb_scale(self) -> u8 {
        bits(self.0, 18, 3) as u8
    }
    pub fn car_oneway(self) -> u8 {
        bits(self.0, 21, 2) as u8
    }
    pub fn fwd_ok(self) -> bool {
        bits(self.0, 23, 1) == 1
    }
    pub fn bwd_ok(self) -> bool {
        bits(self.0, 24, 1) == 1
    }
    pub fn contraflow(self) -> bool {
        bits(self.0, 25, 1) == 1
    }
    pub fn access(self) -> Access {
        Access::from_code(bits(self.0, 26, 2))
    }
    pub fn bicycle_tag(self) -> u8 {
        bits(self.0, 28, 2) as u8
    }
    pub fn service(self) -> u8 {
        bits(self.0, 30, 2) as u8
    }
    pub fn routes(self) -> u8 {
        bits(self.0, 32, 4) as u8
    }
    pub fn infra(self) -> u8 {
        bits(self.0, 36, 2) as u8
    }
    pub fn bridge(self) -> bool {
        bits(self.0, 38, 1) == 1
    }
    pub fn tunnel(self) -> bool {
        bits(self.0, 39, 1) == 1
    }

    /// Revêtement non revêtu, par la valeur brute de `surface` (approximation de T03 §7).
    pub fn is_unpaved(self) -> bool {
        matches!(
            SURFACES.get(usize::from(self.surface()).wrapping_sub(1)).copied(),
            Some(
                "compacted"
                    | "fine_gravel"
                    | "gravel"
                    | "unpaved"
                    | "ground"
                    | "dirt"
                    | "earth"
                    | "grass"
                    | "sand"
                    | "mud"
                    | "pebblestone"
                    | "rock"
                    | "clay"
                    | "woodchips"
                    | "salt"
                    | "ice"
                    | "unhewn_cobblestone"
            )
        )
    }

    pub fn pack(f: &Fields) -> Attrs {
        let mut v = 0u64;
        let mut put = |value: u64, shift: u32, width: u32| {
            debug_assert!(value < (1 << width), "champ trop large au bit {shift}");
            v |= (value & ((1 << width) - 1)) << shift;
        };
        put(u64::from(f.class), 0, 5);
        put(u64::from(f.link), 5, 1);
        put(u64::from(f.surface), 6, 5);
        put(u64::from(f.tracktype), 11, 3);
        put(u64::from(f.smoothness), 14, 4);
        put(u64::from(f.mtb_scale), 18, 3);
        put(u64::from(f.car_oneway), 21, 2);
        put(u64::from(f.fwd_ok), 23, 1);
        put(u64::from(f.bwd_ok), 24, 1);
        put(u64::from(f.contraflow), 25, 1);
        put(u64::from(f.access), 26, 2);
        put(u64::from(f.bicycle_tag), 28, 2);
        put(u64::from(f.service), 30, 2);
        put(u64::from(f.routes), 32, 4);
        put(u64::from(f.infra), 36, 2);
        put(u64::from(f.bridge), 38, 1);
        put(u64::from(f.tunnel), 39, 1);
        Attrs(v)
    }

    pub fn unpack(self) -> Fields {
        Fields {
            class: bits(self.0, 0, 5) as u8,
            link: self.link(),
            surface: self.surface(),
            tracktype: self.tracktype(),
            smoothness: self.smoothness(),
            mtb_scale: self.mtb_scale(),
            car_oneway: self.car_oneway(),
            fwd_ok: self.fwd_ok(),
            bwd_ok: self.bwd_ok(),
            contraflow: self.contraflow(),
            access: self.access() as u8,
            bicycle_tag: self.bicycle_tag(),
            service: self.service(),
            routes: self.routes(),
            infra: self.infra(),
            bridge: self.bridge(),
            tunnel: self.tunnel(),
        }
    }

    /// OU des bits d'itinéraire (relatifs : `ROUTE_*`).
    pub fn with_routes(self, routes: u8) -> Attrs {
        Attrs(self.0 | (u64::from(routes & 0xf) << 32))
    }
}

/// Pourquoi une way n'entre pas dans le graphe (comptée par motif dans les statistiques).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reject {
    NoHighway,
    Area,
    Class(String),
    ClosedMotorway,
}

impl Reject {
    pub fn label(&self) -> String {
        match self {
            Reject::NoHighway => "sans highway".into(),
            Reject::Area => "area=yes".into(),
            Reject::Class(c) => format!("highway={c}"),
            Reject::ClosedMotorway => "motorway/trunk fermée aux vélos".into(),
        }
    }
}

/// Tags rencontrés mais ignorés ou normalisés (comptés).
#[derive(Clone, Copy, Debug, Default)]
pub struct Notes {
    pub reversible: bool,
    pub conditional: bool,
    pub bicycle_directional: bool,
    pub motor_vehicle: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct WayInfo {
    pub attrs: Attrs,
    pub notes: Notes,
}

fn get<'a>(tags: &[(&'a str, &'a str)], key: &str) -> Option<&'a str> {
    tags.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

fn surface_code(value: Option<&str>) -> u8 {
    match value {
        None => 0,
        Some(v) => SURFACES.iter().position(|s| *s == v).map_or(SURFACE_OTHER, |p| p as u8 + 1),
    }
}

fn smoothness_code(value: Option<&str>) -> u8 {
    match value {
        None => 0,
        Some(v) => SMOOTHNESS.iter().position(|s| *s == v).map_or(15, |p| p as u8 + 1),
    }
}

fn tracktype_code(value: Option<&str>) -> u8 {
    match value {
        None => 0,
        Some("grade1") => 1,
        Some("grade2") => 2,
        Some("grade3") => 3,
        Some("grade4") => 4,
        Some("grade5") => 5,
        Some(_) => 7,
    }
}

/// `mtb:scale` : `0`–`6`, avec `+` / `-` retirés ; 0 = absent ou illisible, sinon échelle + 1.
fn mtb_code(value: Option<&str>) -> u8 {
    value
        .and_then(|v| v.trim().chars().next())
        .and_then(|c| c.to_digit(10))
        .filter(|d| *d <= 6)
        .map_or(0, |d| d as u8 + 1)
}

fn bicycle_tag_code(value: Option<&str>) -> u8 {
    match value {
        Some("yes" | "permissive" | "official" | "public" | "allowed" | "use_sidepath" | "mtb") => 1,
        Some("designated") => 2,
        Some("dismount" | "discouraged") => 3,
        _ => 0,
    }
}

fn service_code(value: Option<&str>) -> u8 {
    match value {
        Some("driveway") => 1,
        Some("parking_aisle") => 2,
        Some("drive-through" | "emergency_access") => 3,
        _ => 0,
    }
}

fn infra_level(value: &str) -> u8 {
    match value {
        "track" | "opposite_track" => 3,
        "lane" | "opposite_lane" => 2,
        "shared_lane" | "share_busway" | "shared" => 1,
        _ => 0,
    }
}

fn is_true(v: &str) -> bool {
    !matches!(v, "" | "no" | "false" | "0")
}

/// Filtre vélo et empaquetage d'une way. `Err` : la way n'entre pas dans le graphe.
pub fn classify(tags: &[(&str, &str)]) -> Result<WayInfo, Reject> {
    let highway = get(tags, "highway").ok_or(Reject::NoHighway)?;
    if get(tags, "area") == Some("yes") {
        return Err(Reject::Area);
    }
    let (class, link) = HighwayClass::from_tag(highway).ok_or_else(|| Reject::Class(highway.to_string()))?;

    // Accès résolu : la clé la plus précise dont la valeur est connue.
    let access = ["bicycle", "vehicle", "access"]
        .iter()
        .find_map(|k| get(tags, k).and_then(Access::from_tag))
        .unwrap_or(if class.closed_by_default() { Access::No } else { Access::Yes });
    if matches!(class, HighwayClass::Motorway | HighwayClass::Trunk) && access == Access::No {
        return Err(Reject::ClosedMotorway);
    }

    let mut notes = Notes::default();

    // Sens voiture.
    let implicit =
        matches!(get(tags, "junction"), Some("roundabout" | "circular")) || class == HighwayClass::Motorway;
    let car = match get(tags, "oneway") {
        Some("yes" | "true" | "1") => CAR_FORWARD,
        Some("-1" | "reverse") => CAR_BACKWARD,
        Some("reversible" | "alternating") => {
            notes.reversible = true;
            CAR_BOTH
        }
        Some("no") => CAR_NONE,
        _ if implicit => CAR_FORWARD,
        _ => CAR_NONE,
    };
    let (mut fwd_ok, mut bwd_ok) = match car {
        CAR_FORWARD => (true, false),
        CAR_BACKWARD => (false, true),
        _ => (true, true),
    };
    // Sens vélo : `oneway:bicycle` d'abord, puis les contresens aménagés.
    match get(tags, "oneway:bicycle") {
        Some("no") => (fwd_ok, bwd_ok) = (true, true),
        Some("yes" | "true" | "1") => (fwd_ok, bwd_ok) = (true, false),
        Some("-1" | "reverse") => (fwd_ok, bwd_ok) = (false, true),
        _ => {
            let opposite = ["cycleway", "cycleway:left", "cycleway:right", "cycleway:both"]
                .iter()
                .any(|k| get(tags, k).is_some_and(|v| v.starts_with("opposite")))
                || ["cycleway:left:oneway", "cycleway:right:oneway", "cycleway:both:oneway"]
                    .iter()
                    .any(|k| matches!(get(tags, k), Some("no" | "-1")));
            if opposite {
                (fwd_ok, bwd_ok) = (true, true);
            }
        }
    }
    let contraflow = (fwd_ok && car == CAR_BACKWARD) || (bwd_ok && car == CAR_FORWARD);

    // Aménagement cyclable : le plus fort des `cycleway*`.
    let infra = ["cycleway", "cycleway:left", "cycleway:right", "cycleway:both"]
        .iter()
        .filter_map(|k| get(tags, k))
        .map(infra_level)
        .max()
        .unwrap_or(0);

    // Tags vus et ignorés, pour les statistiques.
    notes.conditional = tags.iter().any(|(k, _)| {
        k.ends_with(":conditional")
            && ["oneway", "access", "bicycle", "vehicle", "motor_vehicle"].iter().any(|p| k.starts_with(p))
    });
    notes.bicycle_directional =
        tags.iter().any(|(k, _)| matches!(*k, "bicycle:forward" | "bicycle:backward"));
    notes.motor_vehicle = get(tags, "motor_vehicle").is_some();

    let fields = Fields {
        class: class as u8,
        link,
        surface: surface_code(get(tags, "surface")),
        tracktype: tracktype_code(get(tags, "tracktype")),
        smoothness: smoothness_code(get(tags, "smoothness")),
        mtb_scale: mtb_code(get(tags, "mtb:scale")),
        car_oneway: car,
        fwd_ok,
        bwd_ok,
        contraflow,
        access: access as u8,
        bicycle_tag: bicycle_tag_code(get(tags, "bicycle")),
        service: service_code(get(tags, "service")),
        routes: 0,
        infra,
        bridge: get(tags, "bridge").is_some_and(is_true),
        tunnel: get(tags, "tunnel").is_some_and(is_true),
    };
    Ok(WayInfo { attrs: Attrs::pack(&fields), notes })
}

/// Bits d'itinéraire (`ROUTE_*`) d'une relation `type=route`, `route=bicycle|mtb`, hors `state=proposed`.
pub fn route_bits(tags: &[(&str, &str)]) -> Option<u8> {
    if get(tags, "type") != Some("route") || get(tags, "state") == Some("proposed") {
        return None;
    }
    match get(tags, "route")? {
        "mtb" => Some(ROUTE_MTB),
        "bicycle" => Some(match get(tags, "network") {
            Some("icn" | "ncn") => ROUTE_NATIONAL,
            Some("rcn") => ROUTE_REGIONAL,
            _ => ROUTE_LOCAL,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(tags: &[(&str, &str)]) -> Attrs {
        classify(tags).expect("way gardée").attrs
    }

    fn rejected(tags: &[(&str, &str)]) -> Reject {
        classify(tags).expect_err("way écartée")
    }

    #[test]
    fn every_listed_class_is_kept() {
        for c in HighwayClass::ALL {
            let a = ok(&[("highway", c.name()), ("bicycle", "yes")]);
            assert_eq!(a.class(), Some(c), "{}", c.name());
        }
        let a = ok(&[("highway", "primary_link")]);
        assert_eq!((a.class(), a.link()), (Some(HighwayClass::Primary), true));
    }

    #[test]
    fn unlisted_classes_areas_and_untagged_ways_are_rejected() {
        assert_eq!(rejected(&[("highway", "construction")]), Reject::Class("construction".into()));
        assert_eq!(rejected(&[("highway", "proposed")]), Reject::Class("proposed".into()));
        assert_eq!(rejected(&[("highway", "platform")]), Reject::Class("platform".into()));
        assert_eq!(rejected(&[("highway", "footway"), ("area", "yes")]), Reject::Area);
        assert_eq!(rejected(&[("building", "yes")]), Reject::NoHighway);
        assert_eq!(rejected(&[("highway", "residential_link")]), Reject::Class("residential_link".into()));
    }

    #[test]
    fn access_resolution() {
        let a = ok(&[("highway", "residential"), ("access", "no"), ("bicycle", "yes")]);
        assert_eq!(a.access(), Access::Yes);
        assert_eq!(ok(&[("highway", "residential"), ("access", "private")]).access(), Access::Private);
        assert_eq!(
            ok(&[("highway", "residential"), ("vehicle", "destination")]).access(),
            Access::Destination
        );
        assert_eq!(ok(&[("highway", "footway"), ("bicycle", "no")]).access(), Access::No);
        // Valeur inconnue : clé suivante.
        assert_eq!(
            ok(&[("highway", "residential"), ("bicycle", "bof"), ("access", "private")]).access(),
            Access::Private
        );
        assert_eq!(ok(&[("highway", "residential"), ("bicycle", "bof")]).access(), Access::Yes);
        // Défaut de classe.
        assert_eq!(ok(&[("highway", "pedestrian")]).access(), Access::No);
        assert_eq!(ok(&[("highway", "footway")]).access(), Access::Yes);
    }

    #[test]
    fn bare_trunk_is_absent_but_trunk_with_bicycle_yes_is_kept() {
        assert_eq!(rejected(&[("highway", "trunk")]), Reject::ClosedMotorway);
        assert_eq!(rejected(&[("highway", "motorway")]), Reject::ClosedMotorway);
        assert_eq!(rejected(&[("highway", "trunk"), ("access", "no")]), Reject::ClosedMotorway);
        assert!(classify(&[("highway", "trunk"), ("bicycle", "yes")]).is_ok());
        assert!(classify(&[("highway", "trunk"), ("bicycle", "designated")]).is_ok());
    }

    #[test]
    fn pack_roundtrip_at_maximum_values() {
        let max = Fields {
            class: 18,
            link: true,
            surface: 31,
            tracktype: 7,
            smoothness: 15,
            mtb_scale: 7,
            car_oneway: 3,
            fwd_ok: true,
            bwd_ok: true,
            contraflow: true,
            access: 3,
            bicycle_tag: 3,
            service: 3,
            routes: 15,
            infra: 3,
            bridge: true,
            tunnel: true,
        };
        let packed = Attrs::pack(&max);
        assert_eq!(packed.unpack(), max);
        assert!(packed.0 < 1 << 40, "40 bits utilisés");
        let zero = Fields::default();
        assert_eq!(Attrs::pack(&zero).unpack(), zero);
        // Chaque champ seul : pas de recouvrement.
        for shift in 0..40 {
            let a = Attrs(1 << shift);
            assert_eq!(Attrs::pack(&a.unpack()), a, "bit {shift}");
        }
    }

    #[test]
    fn mtb_scale_and_unknown_surface() {
        assert_eq!(ok(&[("highway", "path"), ("mtb:scale", "2+")]).mtb_scale(), 3);
        assert_eq!(ok(&[("highway", "path"), ("mtb:scale", "0")]).mtb_scale(), 1);
        assert_eq!(ok(&[("highway", "path"), ("mtb:scale", "6-")]).mtb_scale(), 7);
        assert_eq!(ok(&[("highway", "path")]).mtb_scale(), 0);
        assert_eq!(ok(&[("highway", "path"), ("surface", "lunar_regolith")]).surface(), SURFACE_OTHER);
        assert_eq!(ok(&[("highway", "path"), ("surface", "asphalt")]).surface(), 1);
        assert_eq!(ok(&[("highway", "path"), ("surface", "grass_paver")]).surface(), 30);
        assert_eq!(ok(&[("highway", "path")]).surface(), 0);
        assert_eq!(ok(&[("highway", "track"), ("tracktype", "grade3")]).tracktype(), 3);
        assert_eq!(ok(&[("highway", "track"), ("tracktype", "grade9")]).tracktype(), 7);
        assert_eq!(ok(&[("highway", "path"), ("smoothness", "bad")]).smoothness(), 4);
        assert_eq!(ok(&[("highway", "path"), ("smoothness", "meh")]).smoothness(), 15);
        assert!(ok(&[("highway", "path"), ("surface", "gravel")]).is_unpaved());
        assert!(!ok(&[("highway", "path"), ("surface", "asphalt")]).is_unpaved());
    }

    fn dirs(tags: &[(&str, &str)]) -> (bool, bool, bool) {
        let a = ok(tags);
        (a.fwd_ok(), a.bwd_ok(), a.contraflow())
    }

    #[test]
    fn oneway_rules() {
        let r = |extra: &[(&'static str, &'static str)]| {
            let mut t = vec![("highway", "residential")];
            t.extend_from_slice(extra);
            dirs(&t)
        };
        assert_eq!(r(&[]), (true, true, false));
        assert_eq!(r(&[("oneway", "yes")]), (true, false, false));
        assert_eq!(r(&[("oneway", "-1")]), (false, true, false));
        assert_eq!(r(&[("oneway", "yes"), ("oneway:bicycle", "no")]), (true, true, true));
        assert_eq!(r(&[("oneway", "-1"), ("oneway:bicycle", "no")]), (true, true, true));
        assert_eq!(r(&[("oneway", "yes"), ("cycleway", "opposite_lane")]), (true, true, true));
        assert_eq!(r(&[("oneway", "yes"), ("cycleway:left", "opposite_track")]), (true, true, true));
        assert_eq!(r(&[("oneway", "yes"), ("cycleway:left:oneway", "-1")]), (true, true, true));
        assert_eq!(r(&[("junction", "roundabout")]), (true, false, false));
        assert_eq!(r(&[("junction", "roundabout"), ("oneway", "no")]), (true, true, false));
        assert_eq!(r(&[("oneway:bicycle", "yes")]), (true, false, false));
        assert_eq!(r(&[("oneway:bicycle", "-1")]), (false, true, false));
        assert_eq!(r(&[("oneway", "reversible")]), (true, true, false));
        assert!(classify(&[("highway", "residential"), ("oneway", "reversible")]).unwrap().notes.reversible);
        // Autoroute : sens unique implicite, mais ouverte ici par `bicycle=yes`.
        assert_eq!(dirs(&[("highway", "motorway"), ("bicycle", "yes")]), (true, false, false));
    }

    #[test]
    fn ignored_tags_are_noted() {
        let n = classify(&[
            ("highway", "residential"),
            ("oneway:conditional", "no @ (Mo-Fr)"),
            ("bicycle:forward", "yes"),
            ("motor_vehicle", "no"),
        ])
        .unwrap()
        .notes;
        assert!(n.conditional && n.bicycle_directional && n.motor_vehicle);
    }

    #[test]
    fn infrastructure_service_bridge_tunnel() {
        let a = ok(&[
            ("highway", "service"),
            ("service", "parking_aisle"),
            ("cycleway", "lane"),
            ("bridge", "yes"),
        ]);
        assert_eq!((a.service(), a.infra(), a.bridge(), a.tunnel()), (2, 2, true, false));
        let a =
            ok(&[("highway", "residential"), ("cycleway:right", "track"), ("cycleway:left", "shared_lane")]);
        assert_eq!(a.infra(), 3);
        let a = ok(&[("highway", "residential"), ("tunnel", "building_passage"), ("bridge", "no")]);
        assert_eq!((a.tunnel(), a.bridge()), (true, false));
    }

    #[test]
    fn route_relations() {
        let rb = |t: &[(&str, &str)]| route_bits(t);
        assert_eq!(rb(&[("type", "route"), ("route", "bicycle"), ("network", "rcn")]), Some(ROUTE_REGIONAL));
        assert_eq!(rb(&[("type", "route"), ("route", "bicycle"), ("network", "icn")]), Some(ROUTE_NATIONAL));
        assert_eq!(rb(&[("type", "route"), ("route", "bicycle"), ("network", "ncn")]), Some(ROUTE_NATIONAL));
        assert_eq!(rb(&[("type", "route"), ("route", "bicycle"), ("network", "lcn")]), Some(ROUTE_LOCAL));
        assert_eq!(rb(&[("type", "route"), ("route", "bicycle")]), Some(ROUTE_LOCAL));
        assert_eq!(rb(&[("type", "route"), ("route", "mtb"), ("network", "rwn")]), Some(ROUTE_MTB));
        assert_eq!(rb(&[("type", "route"), ("route", "bicycle"), ("state", "proposed")]), None);
        assert_eq!(rb(&[("type", "route"), ("route", "hiking")]), None);
        assert_eq!(rb(&[("type", "multipolygon")]), None);
        let a = ok(&[("highway", "path")]).with_routes(ROUTE_REGIONAL | ROUTE_MTB);
        assert_eq!(a.routes(), ROUTE_REGIONAL | ROUTE_MTB);
    }
}
