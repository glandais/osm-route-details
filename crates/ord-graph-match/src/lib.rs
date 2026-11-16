//! Appariement des arêtes d'équipe (documents `network_edges`) au graphe vélo OSM, agrégation en surcouche, et dépôt
//! en flux à mémoire bornée. Lots 1 (outil `ord-graph-overlay`) et 3a (service) : un seul chemin d'appariement et de
//! comptage pour les deux.
//!
//! Notes de conception : `docs/design/technique/ord-graph-lot1.md` et `ord-loops-lot3a.md` §4.
//! Les données lues sont **privées** : rien d'ici n'est journalisé.

pub mod agg;
pub mod deposit;
pub mod doc;
pub mod geo;
pub mod index;
pub mod job;
pub mod matcher;

pub use deposit::{Deposit, DepositStats};
pub use index::{CellIndex, Near, SegIndex};
