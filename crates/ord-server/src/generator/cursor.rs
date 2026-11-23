//! Le curseur « routes de l'équipe » (0) ↔ « découverte » (1) et ce qu'il devient dans le moteur (note lot 3a §5).
//!
//! `cursor` ∈ [0, 1], arrondi au centième. **0 : strict** (familiarité 1). **1 : découverte** (coût OSM seul).
//! Entre les deux, **équipe d'abord** avec une familiarité fixée à 0,5 (neutre) et un coût hors équipe
//! `λ(c) = 1 + 2 (1 − c)`, de 3 (le λ du lot 2) à 1. Le moteur reçoit λ en 256es (entier : même poids en natif et en
//! WASM). Mesuré sur 323 requêtes : la part d'équipe médiane du premier parcours décroît strictement de 0,811
//! (c = 0,10) à 0,303 (c = 1) ; par requête, pas de monotonie.

use ord_loops::Mode;

/// Familiarité en équipe d'abord (le réglage « rarement ↔ souvent » n'existe plus) et en découverte.
pub const FAMILIARITY: f32 = 0.5;
/// λ aux deux bouts de l'équipe d'abord.
pub const LAMBDA_MAX: f64 = 3.0;
pub const LAMBDA_MIN: f64 = 1.0;
/// Pas du curseur.
pub const STEP: f64 = 0.01;

/// Ce que le moteur reçoit pour une valeur de curseur.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Applied {
    pub mode: Mode,
    /// λ appliqué ; `None` en strict (le coût hors équipe est infini).
    pub lambda: Option<f64>,
    pub familiarity: f32,
    /// λ en 256es.
    pub off_team_cost_q8: u32,
}

/// Valide et arrondit au centième : en centièmes entiers, `None` hors de [0, 1] ou non fini.
pub fn hundredths(raw: f64) -> Option<u32> {
    (raw.is_finite() && (0.0..=1.0).contains(&raw)).then(|| (raw * 100.0).round() as u32)
}

/// `h` : centièmes, de 0 à 100.
pub fn apply(h: u32) -> Applied {
    let h = h.min(100);
    match h {
        0 => Applied { mode: Mode::Strict, lambda: None, familiarity: 1.0, off_team_cost_q8: 768 },
        100 => Applied {
            mode: Mode::Discovery,
            lambda: Some(LAMBDA_MIN),
            familiarity: FAMILIARITY,
            off_team_cost_q8: 256,
        },
        _ => {
            // 256 × (1 + 2 (100 − h) / 100), arrondi à l'entier.
            let q8 = 256 + (512 * (100 - h) + 50) / 100;
            Applied {
                mode: Mode::TeamFirst,
                lambda: Some(1.0 + 2.0 * f64::from(100 - h) / 100.0),
                familiarity: FAMILIARITY,
                off_team_cost_q8: q8,
            }
        }
    }
}

/// Ce que reçoit le moteur pour un **itinéraire** (api 1.15) : la génération, sauf que le mode strict n'est pas offert.
/// Un curseur qui donnerait le strict (0) route en équipe d'abord au λ le plus haut (3), à la familiarité neutre : un
/// itinéraire qui doit relier deux points posés sur la carte ne peut pas refuser toute arête hors de l'équipe.
pub fn apply_route(h: u32) -> Applied {
    match apply(h) {
        Applied { mode: Mode::Strict, .. } => Applied {
            mode: Mode::TeamFirst,
            lambda: Some(LAMBDA_MAX),
            familiarity: FAMILIARITY,
            off_team_cost_q8: 768,
        },
        a => a,
    }
}

/// Ce que reçoit le moteur pour un itinéraire sans équipe, ou dont l'équipe n'a pas de surcouche servable : la découverte.
pub fn discovery() -> Applied {
    apply(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_route_is_never_strict_and_otherwise_follows_the_generation() {
        let a = apply_route(0);
        assert_eq!(a.mode, Mode::TeamFirst);
        assert_eq!((a.lambda, a.familiarity, a.off_team_cost_q8), (Some(3.0), FAMILIARITY, 768));
        // Le pas suivant ne coûte pas plus que ce plancher : λ ne croît jamais avec le curseur.
        assert!(apply_route(1).off_team_cost_q8 <= a.off_team_cost_q8);
        for h in 1..=100 {
            assert_eq!(apply_route(h), apply(h), "{h}");
        }
        assert_eq!(discovery().mode, Mode::Discovery);
    }

    #[test]
    fn ends_are_strict_and_discovery_and_lambda_decreases_by_the_hundredth() {
        assert_eq!(apply(0).mode, Mode::Strict);
        assert_eq!(apply(0).lambda, None);
        assert_eq!(apply(100).mode, Mode::Discovery);
        let mut last = u32::MAX;
        for h in 1..=99 {
            let a = apply(h);
            assert_eq!(a.mode, Mode::TeamFirst, "{h}");
            assert!(a.off_team_cost_q8 <= last, "λ ne croît jamais avec le curseur ({h})");
            assert!(a.off_team_cost_q8 < 768 && a.off_team_cost_q8 > 256);
            last = a.off_team_cost_q8;
            assert_eq!(a.familiarity, FAMILIARITY);
        }
        assert_eq!(apply(10).lambda, Some(2.8));
        assert_eq!(apply(50).off_team_cost_q8, 512);
        assert_eq!(apply(25).off_team_cost_q8, 640);
        assert_eq!(apply(90).off_team_cost_q8, 307);
    }

    #[test]
    fn range_is_closed_and_rounded() {
        assert_eq!(hundredths(0.0), Some(0));
        assert_eq!(hundredths(1.0), Some(100));
        assert_eq!(hundredths(0.2549), Some(25));
        assert_eq!(hundredths(0.2551), Some(26));
        for bad in [1.01, -0.01, f64::NAN, f64::INFINITY] {
            assert_eq!(hundredths(bad), None, "{bad}");
        }
    }
}
