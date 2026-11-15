//! Coût entier d'une arête de base (note lot 2, §3) : `w = len × K_type(classe, surface présumée, accès) × E_mode`.
//!
//! Tout est en entiers pour que le natif et le WASM donnent les mêmes poids au bit près : `K` en soixante-quatrièmes,
//! et la table `M` des multiplicateurs de familiarité est calculée **sans `exp` flottant** (série de Taylor en
//! virgule fixe).

use ord_graph::attrs::{Access, Attrs, HighwayClass};

/// Type de parcours demandé.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RideType {
    Road,
    Gravel,
    Mtb,
}

impl RideType {
    pub const ALL: [RideType; 3] = [RideType::Road, RideType::Gravel, RideType::Mtb];

    /// Indice de la vue qui sert ce type : 0 `road`, 1 `gravel` (le VTT roule sur `gravel`, poids VTT).
    pub fn view_index(self) -> usize {
        match self {
            Self::Road => 0,
            Self::Gravel | Self::Mtb => 1,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Road => 0,
            Self::Gravel => 1,
            Self::Mtb => 2,
        }
    }
}

/// Voie verte : `path`, `footway`, `pedestrian` ou `bridleway` désignés aux vélos.
pub fn is_greenway(a: Attrs) -> bool {
    matches!(
        a.class(),
        Some(HighwayClass::Path | HighwayClass::Footway | HighwayClass::Pedestrian | HighwayClass::Bridleway)
    ) && a.bicycle_tag() == 2
}

/// Surface **présumée** hors classe routière : voie verte ou `track` sans `surface` (le `roadUncertainFactor` Java).
pub fn presumed_surface(a: Attrs) -> bool {
    a.surface() == 0 && (is_greenway(a) || a.class() == Some(HighwayClass::Track))
}

fn class_k64(c: Option<HighwayClass>) -> u32 {
    use HighwayClass::*;
    match c {
        Some(Cycleway) => 48,
        Some(LivingStreet) => 56,
        Some(Unclassified) => 60,
        Some(Residential) => 64,
        Some(Tertiary) => 68,
        Some(Road) => 72,
        Some(Path | Service) => 80,
        Some(Secondary | Track) => 88,
        Some(Busway) => 120,
        Some(Primary) => 128,
        Some(Footway | Pedestrian | Bridleway) => 160,
        Some(Trunk) => 192,
        Some(Motorway) => 256,
        Some(Steps) => 384,
        None => 128,
    }
}

fn big_road(c: Option<HighwayClass>) -> bool {
    matches!(
        c,
        Some(HighwayClass::Motorway | HighwayClass::Trunk | HighwayClass::Primary | HighwayClass::Secondary)
    )
}

fn off_road(a: Attrs) -> bool {
    a.is_unpaved()
        || matches!(a.class(), Some(HighwayClass::Track | HighwayClass::Path | HighwayClass::Bridleway))
}

/// `K` en soixante-quatrièmes (64 = 1,0) ; 0 si l'accès est `No` (jamais dans une vue).
pub fn k64(ride: RideType, a: Attrs) -> u32 {
    if a.access() == Access::No {
        return 0;
    }
    let class = a.class();
    let mut k = match ride {
        RideType::Road => {
            if is_greenway(a) {
                56
            } else {
                class_k64(class)
            }
        }
        RideType::Gravel | RideType::Mtb => match class {
            Some(HighwayClass::Track) => 72,
            Some(HighwayClass::Path) => 64,
            Some(HighwayClass::Bridleway | HighwayClass::Footway | HighwayClass::Pedestrian) => 80,
            c => class_k64(c),
        },
    };
    match ride {
        RideType::Road => {
            if presumed_surface(a) {
                k = (k * 13).div_ceil(10);
            }
        }
        RideType::Gravel => {
            if off_road(a) {
                k = (k * 3).div_ceil(4);
            } else if big_road(class) {
                k = (k * 5).div_ceil(4);
            }
        }
        RideType::Mtb => {
            if off_road(a) {
                k = (k * 3).div_ceil(5);
            } else if big_road(class) {
                k = (k * 7).div_ceil(5);
            }
        }
    }
    if a.access() == Access::Destination {
        k = (k * 5).div_ceil(4);
    }
    k
}

/// Poids d'une arête de base pour un type, sans équipe : `len_dm × K / 64`, arrondi au supérieur.
pub fn edge_weight(ride: RideType, a: Attrs, len_dm: u32) -> u32 {
    let k = u64::from(k64(ride, a));
    (u64::from(len_dm) * k).div_ceil(64).min(u64::from(u32::MAX / 8)) as u32
}

/// `e^x` en virgule fixe : `x` en Q20 (`|x| <= 2^20 * 2`), résultat en Q30. Série de Taylor, entiers seuls.
fn exp_q30(x_q20: i64) -> i64 {
    let one = 1i128 << 30;
    let mut term = one;
    let mut sum = one;
    for k in 1..=18i128 {
        term = term * i128::from(x_q20) / (k << 20);
        sum += term;
    }
    sum as i64
}

/// Table `M[z]`, `z` de 0 à 255 : multiplicateur de familiarité `exp(β (1 − 2f) z/255)` en 256èmes, avec
/// `f` la familiarité en millièmes (0 : l'équipe est évitée ; 1000 : préférée) et `β = 1`.
pub fn multiplier_table(familiarity_milli: u32) -> [u32; 256] {
    let f = i64::from(familiarity_milli.min(1000));
    let g_q20 = ((1000 - 2 * f) << 20) / 1000;
    let mut t = [256u32; 256];
    for (z, slot) in t.iter_mut().enumerate() {
        let x = g_q20 * z as i64 / 255;
        let e = exp_q30(x);
        *slot = ((e * 256 + (1 << 29)) >> 30).max(1) as u32;
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiplier_table_is_exact_and_golden() {
        // f = 1 (équipe préférée), z = 255 : e^-1 = 0,3679 -> 94 / 256 ; f = 0, z = 255 : e^1 = 2,718 -> 696.
        let t1 = multiplier_table(1000);
        let t0 = multiplier_table(0);
        assert_eq!((t1[0], t1[255]), (256, 94));
        assert_eq!((t0[0], t0[255]), (256, 696));
        // f = 0,5 : neutre.
        assert!(multiplier_table(500).iter().all(|&m| m == 256));
        // Golden : empreinte de la table (même valeur en natif et en WASM, la table est faite d'entiers).
        let sum: u64 = multiplier_table(700).iter().map(|&m| u64::from(m)).sum();
        assert_eq!(sum, GOLDEN_SUM_700);
        // Monotone en z.
        assert!(t1.windows(2).all(|w| w[0] >= w[1]) && t0.windows(2).all(|w| w[0] <= w[1]));
    }

    const GOLDEN_SUM_700: u64 = 54_014;
}
