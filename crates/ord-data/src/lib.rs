//! Accès données et caches (T02).
//!
//! Port `TileSource`, transports HTTP XYZ et WMS `GetMap` sous un budget par hôte, cache disque
//! autoréparé, LRU mémoire et cache négatif (`TileReader`), lecteur de terrain IGN LiDAR HD avec
//! repli Mapterhorn, référentiel de couverture (contour, zone France, GLO-30), catalogue des
//! sources et lectures de démarrage (version Mapterhorn, index de couverture).

pub mod budget;
pub mod catalog;
pub mod coverage;
pub mod disk;
pub mod elevation;
pub mod http;
pub mod pmtiles;
pub mod port;
mod reader;
pub mod startup;
