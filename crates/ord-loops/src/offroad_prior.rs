//! Le prior de la consigne de part (lot 5, §4.3 point 1) : de la part `offroad` visée au multiplicateur μ du premier pas.
//!
//! **Provenance** : la table `part(μ)` du relevé du 5a (`docs/design/technique/ord-loops-lot5-mesures.md`, §6.3), sonde
//! complète GRAVEL, 780 requêtes, surcouche complète, sept valeurs de μ ; pour chaque cible `t`, le μ médian auquel la
//! courbe du premier parcours croise `t` (interpolé en log₂ μ entre les deux points de grille qui encadrent `t`), **moyenné
//! sur les deux curseurs** de familiarité (0,25 et 1). Aucune donnée d'équipe : c'est le prior général d'une équipe
//! dominante et de quelques autres (§12), pas un prior par équipe (§14, défaut 5).
//!
//! Aux cibles basses (≤ 0,20), la médiane est **censurée** par le haut de la grille (μ = 2) : la valeur est une borne
//! basse, la table y monte un peu plus loin que le relevé, et la recherche corrige. Au-delà de 0,65 la table prolonge
//! la courbe sans mesure (une requête sur cent y croise la cible) jusqu'au bas de la grille.
//!
//! Le prior ne sert qu'à choisir le **point de grille du pas 0** : le plus proche du μ lu ici, en log₂.

/// Grille des μ (§4.2 et décisions du propriétaire du 23 novembre) : 0,25 · 0,35 · 0,5 · 0,7 · 1 · 1,4 · 2, en 256es
/// (arrondis : 64, 89,6, 128, 179,2, 256, 358,4, 512). Du moins cher au plus cher pour le non-revêtu.
pub(crate) const GRID_Q8: [u32; 7] = [64, 90, 128, 179, 256, 358, 512];

/// Indice du μ neutre (256) dans [`GRID_Q8`].
pub(crate) const NEUTRAL: usize = 4;

/// `(cible en millièmes, μ médian en millièmes)`, par cible croissante (relevé du 5a, §6.3 ; voir le haut du fichier).
const MU_BY_TARGET_MILLI: [(u32, u32); 14] = [
    (0, 2_000),
    (100, 1_620),
    (150, 1_455),
    (200, 1_385),
    (250, 1_350),
    (300, 1_340),
    (350, 1_250),
    (400, 1_100),
    (450, 890),
    (470, 755),
    (500, 600),
    (550, 460),
    (600, 355),
    (650, 305),
];

/// Au-delà de 0,65, la table s'achève au bas de la grille : le moteur n'y croise presque jamais la cible (relevé, §6.3).
const MU_AT_MAX_MILLI: u32 = 250;
const MAX_TARGET_MILLI: u32 = 800;

/// Le μ du prior pour une cible (millièmes), en millièmes de μ : interpolation linéaire en log₂ μ.
pub(crate) fn mu_milli(target_milli: u32) -> u32 {
    let t = target_milli.min(MAX_TARGET_MILLI);
    let mut table: Vec<(u32, u32)> = MU_BY_TARGET_MILLI.to_vec();
    table.push((MAX_TARGET_MILLI, MU_AT_MAX_MILLI));
    for w in table.windows(2) {
        let ((t0, m0), (t1, m1)) = (w[0], w[1]);
        if t <= t1 {
            let a = f64::from(t - t0) / f64::from(t1 - t0);
            let (l0, l1) = (f64::from(m0).log2(), f64::from(m1).log2());
            return (2f64.powf(l0 + a * (l1 - l0))).round() as u32;
        }
    }
    MU_AT_MAX_MILLI
}

/// Indice dans [`GRID_Q8`] du point le plus proche (en log₂) du μ du prior pour cette cible.
pub(crate) fn grid_index(target_milli: u32) -> usize {
    let x = (f64::from(mu_milli(target_milli)) / 1000.0).log2();
    (0..GRID_Q8.len())
        .min_by(|&a, &b| {
            let da = (f64::from(GRID_Q8[a]) / 256.0).log2() - x;
            let db = (f64::from(GRID_Q8[b]) / 256.0).log2() - x;
            da.abs().partial_cmp(&db.abs()).expect("fini").then(a.cmp(&b))
        })
        .expect("grille non vide")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prior_falls_as_the_target_rises_and_hits_the_surveyed_points() {
        let mut last = u32::MAX;
        for t in (0..=800).step_by(10) {
            let m = mu_milli(t);
            assert!(m <= last, "décroissant : t = {t}, μ = {m} après {last}");
            assert!((250..=2_000).contains(&m));
            last = m;
        }
        assert_eq!(mu_milli(470), 755);
        assert_eq!(mu_milli(650), 305);
        assert_eq!(mu_milli(5_000), 250);
    }

    #[test]
    fn the_grid_point_of_the_prior_follows_the_survey() {
        // Les points de grille du relevé (§6.3, « point de grille » moyen des deux curseurs).
        assert_eq!(GRID_Q8[grid_index(250)], 358);
        assert_eq!(GRID_Q8[grid_index(470)], 179);
        assert_eq!(GRID_Q8[grid_index(600)], 90);
        assert_eq!(GRID_Q8[grid_index(0)], 512);
        assert_eq!(GRID_Q8[grid_index(800)], 64);
        assert_eq!(GRID_Q8[NEUTRAL], 256);
    }
}
