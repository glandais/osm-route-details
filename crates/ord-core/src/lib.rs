//! Cœur pur du service : préparation, zonage, typologie, classification, élévation, assemblage.
//!
//! Incrément 1 : préparation, profil d'élévation sans ouvrages (MNT seul), D+ des presets, pentes,
//! points de sortie des modes `input` et `distance`, assembleur partiel. Les autres algorithmes
//! arrivent avec les incréments suivants (plan du MVP, `docs/design/technique/README.md` §3).
//! Aucun I/O ici (T01 §2.1).

pub mod assemble;
pub mod elevation;
pub mod prepare;
pub(crate) mod round;
pub mod rules;

pub use rules::{Rules, RULES_VERSION};
