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

/// Poids d'une arête de base pour un type, sans goût : la référence de 1.9.2, que le moteur n'appelle plus (il passe
/// par `Engine::base_weight`, qui lui est égal avec `Taste::neutral()`) ; gardée pour les tests.
/// `len_dm × K / 64`, arrondi au supérieur.
#[cfg_attr(not(test), allow(dead_code))]
pub fn edge_weight(ride: RideType, a: Attrs, len_dm: u32) -> u32 {
    let k = u64::from(k64(ride, a));
    (u64::from(len_dm) * k).div_ceil(64).min(u64::from(u32::MAX / 8)) as u32
}

/// Nombre de classes de goût : 6 voies × 3 revêtements × balisé × destination (note lot 4, §3.2 et §12).
pub const TASTE_CLASSES: usize = 72;

/// Voie (§3.2), dans cet ordre : voie cyclable ou verte (`Cycleway` ou [`is_greenway`], la définition du moteur) → 4 ;
/// `Primary | Secondary | Trunk | Motorway` → 0 (grande route ; `Motorway`, fermée au vélo, n'y arrive que par une
/// trace réelle) ; `Tertiary` → 1 ; `Unclassified | Road` → 2 (petite route) ; `Residential | LivingStreet | Service`
/// → 3 (rue) ; tout le reste et classe absente → 5 (chemin). Les `_link` suivent leur classe.
pub fn taste_way(a: Attrs) -> usize {
    use HighwayClass::*;
    if a.class() == Some(Cycleway) || is_greenway(a) {
        return 4;
    }
    match a.class() {
        Some(Primary | Secondary | Trunk | Motorway) => 0,
        Some(Tertiary) => 1,
        Some(Unclassified | Road) => 2,
        Some(Residential | LivingStreet | Service) => 3,
        _ => 5,
    }
}

/// Groupe de revêtement : `surface() == 0` → 1 (présumé) ; sinon non revêtu connu → 2 ; sinon 0 (revêtu connu).
pub fn taste_surface(a: Attrs) -> usize {
    if a.surface() == 0 {
        1
    } else if a.is_unpaved() {
        2
    } else {
        0
    }
}

/// Classe de goût d'une arête de base : `((voie × 3 + revêtement) × 2 + balisé) × 2 + destination`, balisé =
/// `routes() != 0`, destination = accès `destination`. Pure, dans `[0, 72)`.
pub fn taste_class(a: Attrs) -> usize {
    let marked = usize::from(a.routes() != 0);
    let dest = usize::from(a.access() == Access::Destination);
    ((taste_way(a) * 3 + taste_surface(a)) * 2 + marked) * 2 + dest
}

/// Goût par classe : multiplicateur du poids d'une arête de base, en 256es, par type (`RideType::index()`) et par
/// classe de goût. 256 = neutre. Pas de `Default` (tableaux de plus de 32 éléments) : [`Taste::neutral`] en tient lieu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Taste {
    pub q8: [[u16; TASTE_CLASSES]; 3],
}

impl Taste {
    pub fn neutral() -> Self {
        Self { q8: [[256; TASTE_CLASSES]; 3] }
    }

    /// 256 partout, les trois types.
    pub fn is_neutral(&self) -> bool {
        self.q8.iter().all(|t| t.iter().all(|&q| q == 256))
    }
}

/// La table commune (lot 4, 3b, §14.3) est-elle servie normalisée ? Posé par la règle du §14.12.4 de la note : `false`
/// (brute) tant que le banc n'a pas montré mieux. Le profil d'une équipe (`ord-graph-match`, `profile`) apprend avec la
/// même forme : changer cette constante change `Common::hash` et rend tous les profils écrits incompatibles.
pub const COMMON_USE_NORMALIZED: bool = false;

impl Taste {
    /// La table commune brute : la table recalée commune à trois pas, 72 cases par type (constantes générées).
    pub fn common_raw() -> Self {
        Self { q8: crate::taste_common::COMMON_Q8 }
    }

    /// La table commune normalisée (moyenne pondérée de 1 par les décimètres générés par classe).
    pub fn common_normalized() -> Self {
        Self { q8: crate::taste_common::COMMON_Q8_NORMALIZED }
    }

    /// La table commune servie : l'une ou l'autre selon [`COMMON_USE_NORMALIZED`]. Poids de base du moteur
    /// (`Params::base_taste`) et point de départ de l'apprentissage d'un profil d'équipe.
    pub fn common() -> Self {
        if COMMON_USE_NORMALIZED {
            Self::common_normalized()
        } else {
            Self::common_raw()
        }
    }
}

/// Décimètres générés par boucle sous la table commune brute (les deux curseurs), par type et par classe : le `R` du
/// mélange d'un profil d'équipe (§14.5.2).
pub fn common_gen_dm_per_loop() -> &'static [[u32; TASTE_CLASSES]; 3] {
    &crate::taste_common::COMMON_GEN_DM_PER_LOOP
}

/// Boucles comptées par type pour [`common_gen_dm_per_loop`].
pub fn common_loops() -> [u32; 3] {
    crate::taste_common::COMMON_LOOPS
}

/// Mètres du réel, en décimètres, que joue chacun des trois pas de la chaîne commune, par pas, type et classe, toutes
/// équipes réunies : la cible du mélange « sonde » d'un profil d'équipe (`ord-graph-match`, `profile::learn::Mix`).
pub fn common_step_real_dm() -> &'static [[[u32; TASTE_CLASSES]; 3]; 3] {
    &crate::taste_common::COMMON_STEP_REAL_DM
}

/// Mètres générés, comme [`common_step_real_dm`].
pub fn common_step_gen_dm() -> &'static [[[u32; TASTE_CLASSES]; 3]; 3] {
    &crate::taste_common::COMMON_STEP_GEN_DM
}

/// Boucles comptées par pas et par type pour [`common_step_real_dm`].
pub fn common_step_loops() -> [[u32; 3]; 3] {
    crate::taste_common::COMMON_STEP_LOOPS
}

/// D'où vient la table commune (provenance, sans donnée privée).
pub fn common_source() -> &'static str {
    crate::taste_common::COMMON_SOURCE
}

/// Poids d'une arête de base au goût : `⌈len_dm × K × q8[classe] / (64 × 256)⌉`, borné à `u32::MAX / 8` comme
/// [`edge_weight`]. Avec 256 partout, rend exactement `edge_weight` (`⌈256 x / 16 384⌉ = ⌈x / 64⌉`). Le produit tient
/// en `u64` : `len_dm` < 2³², `K` ≤ 1 000, `q8` < 2¹⁶.
pub fn taste_weight(ride: RideType, a: Attrs, len_dm: u32, q8: &[u16; TASTE_CLASSES]) -> u32 {
    let k = u64::from(k64(ride, a));
    let q = u64::from(q8[taste_class(a)]);
    (u64::from(len_dm) * k * q).div_ceil(64 * 256).min(u64::from(u32::MAX / 8)) as u32
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

    use ord_graph::attrs::{Fields, ROUTE_LOCAL};

    fn attrs(class: HighwayClass, f: impl FnOnce(&mut Fields)) -> Attrs {
        let mut fields = Fields { class: class as u8, fwd_ok: true, bwd_ok: true, ..Fields::default() };
        f(&mut fields);
        Attrs::pack(&fields)
    }

    /// Code de `surface` pour une valeur nommée (code = position + 1).
    fn surface(name: &str) -> u8 {
        ord_graph::attrs::SURFACES.iter().position(|s| *s == name).unwrap() as u8 + 1
    }

    #[test]
    fn taste_class_reaches_the_72_distinct_classes_and_stays_below() {
        use HighwayClass::*;
        // Une représentante par voie, avec ses revêtements, balisage et accès.
        let ways: [Attrs; 6] = [
            attrs(Primary, |_| {}),
            attrs(Tertiary, |_| {}),
            attrs(Unclassified, |_| {}),
            attrs(Residential, |_| {}),
            attrs(Cycleway, |_| {}),
            attrs(Track, |_| {}),
        ];
        let mut seen = std::collections::BTreeSet::new();
        for w in ways {
            for surf in [surface("asphalt"), 0, surface("gravel")] {
                for routes in [0, ROUTE_LOCAL] {
                    for access in [Access::Yes as u8, Access::Destination as u8] {
                        let mut f = w.unpack();
                        (f.surface, f.routes, f.access) = (surf, routes, access);
                        let c = taste_class(Attrs::pack(&f));
                        assert!(c < TASTE_CLASSES);
                        seen.insert(c);
                    }
                }
            }
        }
        assert_eq!(seen.len(), TASTE_CLASSES);
    }

    #[test]
    fn taste_class_known_cases() {
        use HighwayClass::*;
        let paved = surface("asphalt");
        let primary = attrs(Primary, |f| f.surface = paved);
        assert_eq!((taste_way(primary), taste_surface(primary), taste_class(primary)), (0, 0, 0));
        assert_eq!(taste_way(attrs(Cycleway, |_| {})), 4);
        // Voie verte : `path` désigné aux vélos (is_greenway) ; sans désignation, un chemin.
        assert_eq!(taste_way(attrs(Path, |f| f.bicycle_tag = 2)), 4);
        assert_eq!(taste_way(attrs(Path, |_| {})), 5);
        // `track` sans `surface` : chemin, revêtement présumé.
        let track = attrs(Track, |_| {});
        assert_eq!((taste_way(track), taste_surface(track)), (5, 1));
        // `access=destination` : bit de destination (le plus faible) ; balisé : le suivant.
        let dest = attrs(Residential, |f| (f.surface, f.access) = (paved, Access::Destination as u8));
        assert_eq!(taste_class(dest) & 1, 1);
        let marked = attrs(Residential, |f| (f.surface, f.routes) = (paved, ROUTE_LOCAL));
        assert_eq!(taste_class(marked) & 3, 2);
        // Une `_link` suit sa classe.
        let link = attrs(Primary, |f| (f.surface, f.link) = (paved, true));
        assert_eq!(taste_class(link), taste_class(primary));
        assert_eq!(taste_surface(attrs(Residential, |f| f.surface = surface("gravel"))), 2);
    }

    /// Un échantillon d'attributs : toutes les classes, avec et sans surface, accès `yes` / `destination`.
    fn sample_attrs() -> Vec<Attrs> {
        let mut out = Vec::new();
        for c in HighwayClass::ALL {
            for surf in [0, surface("asphalt"), surface("gravel"), surface("sand")] {
                for access in [Access::Yes as u8, Access::Destination as u8] {
                    for (bike, routes) in [(0, 0), (2, ROUTE_LOCAL)] {
                        out.push(attrs(c, |f| {
                            (f.surface, f.access, f.bicycle_tag, f.routes) = (surf, access, bike, routes);
                        }));
                    }
                }
            }
        }
        out
    }

    /// Empreinte des `q8` d'une table, petit-boutiste, dans l'ordre (type, classe).
    fn q8_hash(q: &[[u16; TASTE_CLASSES]; 3]) -> u64 {
        let bytes: Vec<u8> = q.iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
        fnv1a64(&bytes)
    }

    /// FNV-1a 64 bits : sans dépendance, suffisant pour figer une table dans un test (ce n'est pas le `taste_xxh3` du banc).
    fn fnv1a64(b: &[u8]) -> u64 {
        b.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &x| (h ^ u64::from(x)).wrapping_mul(0x0100_0000_01b3))
    }

    #[test]
    fn the_common_table_is_bounded_neutral_for_mtb_and_frozen() {
        let (raw, norm) = (Taste::common_raw(), Taste::common_normalized());
        for t in [&raw, &norm] {
            assert!(t.q8.iter().flatten().all(|&q| (64..=1_024).contains(&q)));
        }
        // MTB : apprise sur 3 boucles, sous le seuil de 50 : neutre, brute comme normalisée.
        assert!(raw.q8[2].iter().all(|&q| q == 256) && norm.q8[2].iter().all(|&q| q == 256));
        assert!(!raw.is_neutral() && !norm.is_neutral() && raw != norm);
        // La table commune servie suit la constante.
        assert_eq!(Taste::common(), if COMMON_USE_NORMALIZED { norm.clone() } else { raw.clone() });
        // Empreintes figées : changer une constante générée sans le dire casse ce test (re-bénir ici, et le dire).
        assert_eq!(q8_hash(&raw.q8), RAW_HASH, "{:#x}", q8_hash(&raw.q8));
        assert_eq!(q8_hash(&norm.q8), NORMALIZED_HASH, "{:#x}", q8_hash(&norm.q8));
        // Les poids du mélange : des décimètres par boucle, jamais tous nuls pour un type qui a des boucles.
        let r = common_gen_dm_per_loop();
        assert!(r[0].iter().any(|&x| x > 0) && r[1].iter().any(|&x| x > 0));
        let loops = common_loops();
        assert!(loops[0] >= 50 && loops[1] >= 50 && !common_source().is_empty());
        // Les pas de la chaîne commune : le pas 3 joue la table du pas 2, le pas 1 la neutre ; mêmes boucles à peu près.
        let (real, gen, sl) = (common_step_real_dm(), common_step_gen_dm(), common_step_loops());
        for k in 0..3 {
            assert!(sl[k][0] >= 50 && sl[k][1] >= 50, "{sl:?}");
            for t in 0..2 {
                assert!(real[k][t].iter().any(|&x| x > 0) && gen[k][t].iter().any(|&x| x > 0));
            }
        }
        // `R` de la table commune = les mètres générés par boucle du dernier pas joué (sous `C`), de même ordre de grandeur
        // que ceux du pas 3 (sous la table du pas 2) : pas le même nombre, mais jamais deux ordres d'écart.
        let (rr, g3): (f64, f64) = (
            r[0].iter().map(|&x| f64::from(x) / 10.0).sum::<f64>(),
            gen[2][0].iter().map(|&x| f64::from(x) / 10.0).sum::<f64>() / f64::from(sl[2][0]),
        );
        assert!(rr / g3 > 0.5 && rr / g3 < 2.0, "{rr} {g3}");
    }

    const RAW_HASH: u64 = 0x8f43a55dab78eda4;
    const NORMALIZED_HASH: u64 = 0xdad60f58ca7c0dea;

    const LENS: [u32; 7] = [1, 7, 999, 1_000_000, u32::MAX / 8, u32::MAX / 2, u32::MAX];

    #[test]
    fn neutral_taste_weight_is_exactly_edge_weight() {
        let q = Taste::neutral();
        for ride in RideType::ALL {
            for a in sample_attrs() {
                for len in LENS {
                    assert_eq!(
                        taste_weight(ride, a, len, &q.q8[ride.index()]),
                        edge_weight(ride, a, len),
                        "{ride:?} {a:?} {len}"
                    );
                }
            }
        }
        assert!(q.is_neutral());
    }

    #[test]
    fn taste_weight_scales_and_never_vanishes() {
        for ride in RideType::ALL {
            for a in sample_attrs().into_iter().filter(|a| k64(ride, *a) > 0) {
                for len in [1u32, 7, 999, 1_000_000] {
                    let k = u64::from(k64(ride, a));
                    let double = taste_weight(ride, a, len, &[512; TASTE_CLASSES]);
                    assert_eq!(u64::from(double), (2 * u64::from(len) * k).div_ceil(64));
                    assert!(taste_weight(ride, a, len, &[85; TASTE_CLASSES]) > 0);
                }
            }
        }
        let mut q = Taste::neutral();
        assert!(q.is_neutral());
        q.q8[1][3] = 255;
        assert!(!q.is_neutral());
    }
}
