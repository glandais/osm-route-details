//! Cœur pur du service : préparation, zonage, typologie, classification, élévation, assemblage.
//!
//! Incrément 0 : seule la version des règles existe ; les algorithmes arrivent à partir de
//! l'incrément 1 (plan du MVP, `docs/design/technique/README.md` §3).

pub mod rules {
    /// Version des règles (`AAAA.MM.n`, doc 02 §6), montée à la main à chaque changement de règle.
    pub const RULES_VERSION: &str = "2026.10.0";
}

pub use rules::RULES_VERSION;
