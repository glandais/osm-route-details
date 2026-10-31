//! Types partagés du service (carte des types, T01 §0), enums publiques et ports.
//!
//! Aucun I/O ici : ni runtime, ni disque, ni réseau, ni horloge murale (T01 §2.1).
//! Les types qui portent une coordonnée ou une clé de tuile ont un `Debug` masqué (T01 §8).

pub mod budget;
pub mod catalog;
pub mod coverage;
pub mod ctx;
pub mod elevation;
pub mod geo;
pub mod report;
pub mod request;
pub mod route;
pub mod step;
pub mod street;
pub mod tiles;

pub use request::LatLon;
