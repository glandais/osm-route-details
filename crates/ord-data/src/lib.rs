//! Accès données et caches (T02).
//!
//! Incrément 1 : port `TileSource`, transport HTTP XYZ, cache disque autoréparé, LRU mémoire et
//! cache négatif (`TileReader`), lecteur de terrain Mapterhorn seul, catalogue des sources.

pub mod catalog;
pub mod disk;
pub mod elevation;
pub mod http;
pub mod port;
mod reader;
