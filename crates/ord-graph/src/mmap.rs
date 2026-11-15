//! Ouverture d'un fichier de graphe par projection mémoire.
//!
//! **Le seul `unsafe` du crate** : un appel à `memmap2::Mmap::map`. Le crate déclare
//! `unsafe_code = "deny"` (le `forbid` du workspace ne se relâche pas par module) et seul ce module porte
//! `#[allow(unsafe_code)]`, dans `lib.rs`.

use std::fs::File;
use std::path::Path;

use memmap2::Mmap;

use crate::format::{FormatError, GraphFile};

/// Projette un fichier en lecture seule.
pub fn map_readonly(file: &File) -> std::io::Result<Mmap> {
    // SAFETY: le fichier est ouvert en lecture seule et n'est jamais modifié sur place : `write_graph`
    // écrit à côté, passe en 0444 puis renomme. Une troncature par un tiers pendant la projection donnerait
    // un SIGBUS ; elle est hors contrat et documentée pour l'exploitation (note de conception §4).
    unsafe { Mmap::map(file) }
}

/// Un graphe lu par projection mémoire.
pub type MappedGraph = GraphFile<Mmap>;

impl GraphFile<Mmap> {
    /// Ouvre un fichier de graphe projeté en mémoire.
    pub fn open_mmap(path: &Path) -> Result<Self, FormatError> {
        let file = File::open(path)?;
        let map = map_readonly(&file)?;
        GraphFile::open(map)
    }
}
