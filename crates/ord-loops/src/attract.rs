//! Attracteurs OSM des points lointains organiques (lot 4, 4d, §17.3 et §17.7 de la note) : les poids par catégorie et
//! le masque d'un nœud. Le fichier (`ord_graph::attract::AttrFile`) donne un masque par nœud de vue ; la fenêtre le
//! range par nœud local (`ViewData::attr`), et les trois codes organiques (candidats du triangle, rang des paires,
//! demi-tour de la lentille) le lisent ici.

use crate::RideType;

/// Poids entiers (0 à 3) par catégorie, dans l'ordre des bits du masque du fichier (`ord_graph::attract::CATS` :
/// café, boulangerie, point de vue, bourg, village, col, sommet, réservé).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttrWeights {
    pub w: [u8; 8],
}

impl AttrWeights {
    /// Aucun attracteur : l'organique rend exactement 1.12.
    pub const NONE: Self = Self { w: [0; 8] };

    /// Les poids généraux d'un type (décisions du §9.10 de la note) : ROAD, commerce et point de vue ; GRAVEL, point de
    /// vue ; cols et sommets au poids du point de vue de chaque type ; bourgs écrits à 0 ; MTB, rien. Pas de poids par
    /// équipe.
    pub fn general(ride: RideType) -> Self {
        // café, boulangerie, point de vue, bourg, village, col, sommet, réservé
        let w = match ride {
            RideType::Road => [1, 1, 1, 0, 0, 1, 1, 0],
            RideType::Gravel => [0, 0, 1, 0, 0, 1, 1, 0],
            RideType::Mtb => [0; 8],
        };
        Self { w }
    }

    /// Aucun poids non nul.
    pub fn is_none(&self) -> bool {
        self.w.iter().all(|&x| x == 0)
    }
}

/// Poids d'un nœud : le plus grand poids des catégories de son masque (0 sans catégorie).
#[inline]
pub(crate) fn attr_w(mask: u8, w: &[u8; 8]) -> u8 {
    let mut best = 0u8;
    let mut m = mask;
    while m != 0 {
        let b = m.trailing_zeros() as usize;
        best = best.max(w[b]);
        m &= m - 1;
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_node_weighs_its_heaviest_category() {
        let w = [1, 2, 0, 0, 0, 3, 0, 0];
        assert_eq!(attr_w(0, &w), 0);
        assert_eq!(attr_w(0b1, &w), 1);
        assert_eq!(attr_w(0b11, &w), 2);
        assert_eq!(attr_w(0b100100, &w), 3);
        assert_eq!(attr_w(0b1000, &w), 0);
        assert_eq!(attr_w(0b0000_1000, &AttrWeights::NONE.w), 0);
    }

    #[test]
    fn the_general_weights_follow_the_owner_decisions() {
        let r = AttrWeights::general(RideType::Road);
        // Café, boulangerie, point de vue, col, sommet à 1 ; bourgs à 0.
        assert_eq!(r.w, [1, 1, 1, 0, 0, 1, 1, 0]);
        let g = AttrWeights::general(RideType::Gravel);
        assert_eq!(g.w, [0, 0, 1, 0, 0, 1, 1, 0], "GRAVEL : point de vue, col et sommet seulement");
        assert!(AttrWeights::general(RideType::Mtb).is_none());
        assert!(AttrWeights::NONE.is_none() && !r.is_none());
    }
}
