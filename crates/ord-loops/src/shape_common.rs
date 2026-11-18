//! Répartition générale des figures par type (lot 4, 4c, §15.3 de `docs/design/technique/ord-loops-lot4.md`) :
//! **généré** par `ord-loops-bench shape-common`, ne pas modifier à la main. Comptes des boucles réelles retenues
//! par nombre de sommets à 10 % de D, part de triangles et quantiles du r / D des triangles, toutes équipes
//! réunies : des agrégats, aucune donnée d'équipe.
//!
//! Provenance : rapport `base` du banc de vraisemblance du 2026-11-18 06:55:13 UTC (commit 2a4e446 (arbre modifié)), boucles retenues ROAD 2071, GRAVEL 780, MTB 4.

use crate::{FigureMix, RideType};

/// Boucles réelles retenues par type (`RideType::index()`), `[k ≤ 1, k = 2, k ≥ 3]` sommets à 10 % de D.
#[allow(dead_code)] // la provenance des deux constantes suivantes ; un test la relit
pub(crate) const SHAPE_COUNTS: [[u32; 3]; 3] = [[838, 1098, 135], [432, 335, 13], [3, 1, 0]];

/// Part des triangles `round(256 · n₂ / (n₁ + n₂))` (la part du quadrilatère est ôtée, parts renormalisées) ; le MTB, 4
/// boucles, est forcé à 0.
pub(crate) const SHAPE_TRI_Q8: [u16; 3] = [145, 112, 0];

/// P10, P25, P50, P75, P90 (type 7) du r / D des boucles réelles à deux sommets, en millièmes.
pub(crate) const SHAPE_RD_TRI_MILLI: [[u16; 5]; 3] =
    [[252, 286, 318, 345, 363], [207, 236, 267, 294, 316], [0, 0, 0, 0, 0]];

/// La répartition générale d'un type.
pub(crate) fn general(ride: RideType) -> FigureMix {
    let i = ride.index();
    FigureMix { tri_q8: SHAPE_TRI_Q8[i], rd_tri_milli: SHAPE_RD_TRI_MILLI[i] }
}
