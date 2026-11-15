//! Ce qu'une arête de base permet à un type de parcours, recalculé depuis ses attributs et la surcouche
//! (note lot 2 §1.1 pour « admise », §2.1 pour « d'équipe »). Rien n'est lu dans les vues ni dans les poids
//! du moteur.

use ord_graph::attrs::{Access, Attrs, HighwayClass};
use ord_graph::overlay::Overlay;
use ord_loops::RideType;

/// Le filtre de vue dont dépend un type : la route a le sien, le gravel et le VTT partagent l'autre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    Road,
    Gravel,
}

impl Admission {
    pub fn of(ride: RideType) -> Self {
        match ride {
            RideType::Road => Self::Road,
            RideType::Gravel | RideType::Mtb => Self::Gravel,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Road => 0,
            Self::Gravel => 1,
        }
    }
}

fn road_class(c: HighwayClass) -> bool {
    use HighwayClass::*;
    matches!(
        c,
        Motorway
            | Trunk
            | Primary
            | Secondary
            | Tertiary
            | Unclassified
            | Residential
            | LivingStreet
            | Service
            | Cycleway
            | Road
            | Busway
    )
}

/// `bicycle=designated` (code 2 du champ `bicycle_tag`).
fn designated(a: Attrs) -> bool {
    a.bicycle_tag() == 2
}

/// L'arête est-elle admise pour ce type (§1.1) ? `road` : classes routières, voies vertes désignées, `track`
/// `grade1` ou de surface connue revêtue, jamais de surface connue non revêtue ; `gravel` : les classes
/// routières, `track`, `path`, `bridleway`, et `footway` / `pedestrian` désignés. Accès `No` / `Private` et
/// `service=*` : jamais.
pub fn admitted(adm: Admission, a: Attrs) -> bool {
    if matches!(a.access(), Access::No | Access::Private) {
        return false;
    }
    let Some(class) = a.class() else { return false };
    if class == HighwayClass::Service && a.service() != 0 {
        return false;
    }
    use HighwayClass::*;
    match adm {
        Admission::Road => {
            let greenway = matches!(class, Path | Footway | Pedestrian | Bridleway) && designated(a);
            let paved_track = class == Track && (a.tracktype() == 1 || (a.surface() != 0 && !a.is_unpaved()));
            (road_class(class) || greenway || paved_track) && !a.is_unpaved()
        }
        Admission::Gravel => {
            road_class(class)
                || matches!(class, Track | Path | Bridleway)
                || (matches!(class, Footway | Pedestrian) && designated(a))
        }
    }
}

/// Sens permis au vélo : `a → b` si `forward`, sinon `b → a` (contresens cyclables compris dans ces bits).
pub fn direction_allowed(a: Attrs, forward: bool) -> bool {
    if forward {
        a.fwd_ok()
    } else {
        a.bwd_ok()
    }
}

/// Surface connue, non revêtue et rugueuse : ce que l'équipe n'a pas roulé en gravel ne s'y présume pas
/// (règle Java : `dirt`, `sand`, `grass`, `rock`, étendue ici à leurs voisines). Ni `compacted`, `fine_gravel`,
/// `gravel`, `unpaved` (non revêtu roulant), ni les pavés.
fn rough_surface(a: Attrs) -> bool {
    use ord_graph::attrs::SURFACES;
    // `tracktype` grade4 et grade5 : rugueux quelle que soit la surface (Java : `roughUnpaved`).
    if a.tracktype() >= 4 {
        return true;
    }
    let code = usize::from(a.surface());
    if code == 0 {
        return false;
    }
    matches!(
        SURFACES.get(code - 1).copied(),
        Some(
            "ground"
                | "dirt"
                | "earth"
                | "grass"
                | "sand"
                | "mud"
                | "rock"
                | "wood"
                | "clay"
                | "woodchips"
                | "salt"
                | "ice"
        )
    )
}

/// La surcouche d'une équipe lue pour un contrôle : les indices de types par nom.
#[derive(Clone, Copy, Debug)]
pub struct TypeIndex {
    pub road: Option<usize>,
    pub gravel: Option<usize>,
    pub mtb: Option<usize>,
    pub mixed: Option<usize>,
}

impl TypeIndex {
    pub fn of(o: &Overlay) -> Self {
        let find = |n: &str| o.meta.types.iter().position(|t| t == n);
        Self { road: find("ROAD"), gravel: find("GRAVEL"), mtb: find("MTB"), mixed: find("MIXED") }
    }

    fn rode_by(&self, counts: &[u16], types: &[Option<usize>]) -> bool {
        types.iter().flatten().any(|&t| {
            counts.get(2 * t).copied().unwrap_or(0) > 0 || counts.get(2 * t + 1).copied().unwrap_or(0) > 0
        })
    }
}

/// L'arête est-elle « d'équipe » pour ce type (§2.1) ? Un parcours de n'importe quel type et sens l'a prise ;
/// elle est admise pour le type ; et, pour les arêtes dont le revêtement est seulement présumé (en route : voie
/// verte ou `track` sans `surface` ; en gravel et VTT : `path`, `footway` ou surface rugueuse), un parcours du
/// bon type l'a prise (route : `ROAD` ou `MIXED` ; gravel : `GRAVEL` ou `MIXED` ; VTT : aussi `MTB`).
pub fn team_edge(ride: RideType, attrs: Attrs, counts: Option<&[u16]>, ti: &TypeIndex) -> bool {
    let Some(counts) = counts else { return false };
    if !counts.iter().any(|&c| c > 0) {
        return false;
    }
    if !admitted(Admission::of(ride), attrs) {
        return false;
    }
    let Some(class) = attrs.class() else { return false };
    use HighwayClass::*;
    match ride {
        RideType::Road => {
            let presumed =
                matches!(class, Path | Footway | Pedestrian | Bridleway | Track) && attrs.surface() == 0;
            !presumed || ti.rode_by(counts, &[ti.road, ti.mixed])
        }
        RideType::Gravel => {
            let presumed = matches!(class, Path | Footway) || rough_surface(attrs);
            !presumed || ti.rode_by(counts, &[ti.gravel, ti.mixed])
        }
        RideType::Mtb => {
            let presumed = matches!(class, Path | Footway) || rough_surface(attrs);
            !presumed || ti.rode_by(counts, &[ti.mtb, ti.gravel, ti.mixed])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ord_graph::attrs::classify;

    fn attrs(tags: &[(&str, &str)]) -> Attrs {
        classify(tags).expect("way gardée").attrs
    }

    const TI: TypeIndex = TypeIndex { road: Some(0), gravel: Some(1), mtb: Some(2), mixed: Some(3) };
    // comptes : ROAD a→b, ROAD b→a, GRAVEL.., MTB.., MIXED..
    const BY_ROAD: [u16; 8] = [1, 0, 0, 0, 0, 0, 0, 0];
    const BY_GRAVEL: [u16; 8] = [0, 0, 0, 1, 0, 0, 0, 0];
    const BY_MIXED: [u16; 8] = [0, 0, 0, 0, 0, 0, 1, 0];
    const BY_MTB: [u16; 8] = [0, 0, 0, 0, 1, 0, 0, 0];

    #[test]
    fn views_follow_the_definition_of_the_note() {
        let green = attrs(&[("highway", "path"), ("bicycle", "designated")]);
        let green_dirt = attrs(&[("highway", "path"), ("bicycle", "designated"), ("surface", "dirt")]);
        let plain_path = attrs(&[("highway", "path")]);
        let grade1 = attrs(&[("highway", "track"), ("tracktype", "grade1")]);
        let bare_track = attrs(&[("highway", "track")]);
        let street = attrs(&[("highway", "residential")]);
        let private = attrs(&[("highway", "residential"), ("access", "private")]);
        assert!(admitted(Admission::Road, green));
        assert!(!admitted(Admission::Road, green_dirt));
        assert!(!admitted(Admission::Road, plain_path));
        assert!(admitted(Admission::Road, grade1));
        assert!(!admitted(Admission::Road, bare_track));
        assert!(admitted(Admission::Gravel, bare_track) && admitted(Admission::Gravel, plain_path));
        assert!(admitted(Admission::Road, street) && admitted(Admission::Gravel, street));
        assert!(!admitted(Admission::Road, private) && !admitted(Admission::Gravel, private));
    }

    #[test]
    fn in_gravel_only_a_rough_surface_is_presumed_like_java() {
        let track = |extra: &[(&str, &str)]| {
            let mut t = vec![("highway", "track")];
            t.extend_from_slice(extra);
            attrs(&t)
        };
        // pas rugueux : compacted, gravel, fine_gravel, unpaved et grade2 comptent dès qu'un parcours quelconque les a pris
        for extra in [
            &[("surface", "compacted")][..],
            &[("surface", "gravel")][..],
            &[("surface", "fine_gravel")][..],
            &[("surface", "unpaved")][..],
            &[("tracktype", "grade2")][..],
        ] {
            assert!(team_edge(RideType::Gravel, track(extra), Some(&BY_ROAD), &TI), "{extra:?}");
        }
        // rugueux : dirt, sand, wood, tracktype grade4 et grade5 demandent un parcours gravel ou mixte
        for extra in [
            &[("surface", "dirt")][..],
            &[("surface", "sand")][..],
            &[("surface", "wood")][..],
            &[("tracktype", "grade4")][..],
            &[("tracktype", "grade5")][..],
        ] {
            assert!(!team_edge(RideType::Gravel, track(extra), Some(&BY_ROAD), &TI), "{extra:?}");
            assert!(team_edge(RideType::Gravel, track(extra), Some(&BY_GRAVEL), &TI), "{extra:?}");
        }
    }

    #[test]
    fn a_presumed_surface_needs_a_ride_of_the_right_type() {
        let green = attrs(&[("highway", "path"), ("bicycle", "designated")]);
        // route : une voie verte sans surface ne compte que roulée en ROAD ou MIXED
        assert!(!team_edge(RideType::Road, green, Some(&BY_GRAVEL), &TI));
        assert!(team_edge(RideType::Road, green, Some(&BY_ROAD), &TI));
        assert!(team_edge(RideType::Road, green, Some(&BY_MIXED), &TI));
        // gravel : un `path` seulement roulé en GRAVEL ou MIXED ; le VTT accepte aussi MTB
        assert!(!team_edge(RideType::Gravel, green, Some(&BY_ROAD), &TI));
        assert!(team_edge(RideType::Gravel, green, Some(&BY_GRAVEL), &TI));
        assert!(!team_edge(RideType::Gravel, green, Some(&BY_MTB), &TI));
        assert!(team_edge(RideType::Mtb, green, Some(&BY_MTB), &TI));
        // une rue ordinaire compte quel que soit le type du parcours
        let street = attrs(&[("highway", "residential")]);
        assert!(team_edge(RideType::Road, street, Some(&BY_GRAVEL), &TI));
        // aucune surcouche : jamais d'équipe
        assert!(!team_edge(RideType::Road, street, None, &TI));
    }
}
