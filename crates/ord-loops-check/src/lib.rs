//! Contrôleur **indépendant** des garanties d'un parcours du moteur `ord-loops` (lot 2).
//!
//! Il décide si un parcours respecte les garanties **depuis le parcours seul** : la suite d'arêtes de base, leur
//! géométrie, le graphe de base et, en mode strict, la surcouche de l'équipe. Les définitions sont celles de la note
//! de conception (`docs/design/technique/ord-loops-lot2.md`, §6) et de `RouteGuarantees.java`, réécrites ici :
//! géométrie, caps, réemploi, croisements, couloir. Du moteur, il ne prend que les types publics de la requête et du
//! résultat (`Request`, `Route`, `Position`, `Step`, `Mode`, `RideType`) : aucune fonction (accroche, caps, vues,
//! poids, contrôle interne), pour qu'un défaut commun ne puisse pas se cacher.
//!
//! ```text
//! let checker = Checker::new(&graph, Some(&overlay), Config::default());
//! let report = checker.check(&request, &route);
//! assert!(report.ok(), "{:?}", report.violations);
//! ```

mod bounded;
mod check;
mod geo;
mod rules;
#[cfg(any(test, feature = "reference"))]
mod team_reference;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_bounded;
#[cfg(test)]
mod tests_team_sparse;

pub use bounded::{CheckPlan, GraphIndex, TeamCheck, TeamCheckEscape};
pub use check::{Checker, Config, Report, Rideability, Violation};
pub use geo::{haversine_m, LatLon};
pub use rules::{admitted, exempt, rideable, team_edge, Admission, TypeIndex};
