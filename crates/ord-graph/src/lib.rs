//! Graphe vélo en cellules (lot 0 du générateur de parcours) : construction depuis une source OSM
//! abstraite, fichier unique découpé en cellules lu par `mmap`, et une recherche bornée **pour la mesure**.
//! Le lot 0 bis y ajoute les vues par type de parcours (`view`) et l'état de recherche local au disque.
//!
//! Note de conception : `docs/design/technique/ord-graph-lot0.md`. Aucune dépendance `ord-*` ; sans les
//! features `pbf` et `mmap`, le crate ne dépend ni d'`osmpbf` ni de `memmap2`.

pub mod attrs;
pub mod build;
pub mod format;
pub mod grid;
#[cfg(feature = "mmap")]
#[allow(unsafe_code)]
pub mod mmap;
#[cfg(feature = "pbf")]
pub mod pbf;
pub mod search;
pub mod stats;
pub mod view;

pub use attrs::{Access, Attrs, HighwayClass};
pub use build::{build_graph, BuildError, BuildOptions, GraphData, MemSource, OsmSource};
pub use format::{write_graph, FormatError, GraphFile};
