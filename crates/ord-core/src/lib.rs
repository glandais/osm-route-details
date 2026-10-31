//! Cœur pur du service : préparation, zonage, typologie, classification, élévation, assemblage.
//!
//! Incréments 1–2 : préparation, zonage (zone France), profil d'élévation sans ouvrages (MNT seul,
//! IGN et repli), D+ des presets et adaptation GLO-30, pentes, points de sortie des modes `input`
//! et `distance`, assembleur partiel. Les autres algorithmes
//! arrivent avec les incréments suivants (plan du MVP, `docs/design/technique/README.md` §3).
//! Aucun I/O ici (T01 §2.1).

pub mod assemble;
pub mod coverage;
pub mod elevation;
pub mod prepare;
pub(crate) mod round;
pub mod rules;

pub use rules::{Rules, RULES_VERSION};
