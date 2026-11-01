//! Heuristiques pures sur profils échantillonnés au pas fixe, portées de vcyclist : tabliers de
//! pont sur MNS/MNT LiDAR (`deck_detector`) et tunnels sur MNT (`tunnel_detector`) ; galeries
//! couvertes sur MNT et MNS (`gallery_detector`, propre au service).

pub mod deck_detector;
pub mod gallery_detector;
pub mod tunnel_detector;

pub use deck_detector::{
    detect_decks, DeckCandidate, DeckDetection, DeckDetectorOptions, DeckMetrics, DeckRejection,
};
pub use gallery_detector::{detect_galleries, GalleryDetectorOptions, GallerySpan};
pub use tunnel_detector::{detect_tunnels, TunnelDetectorOptions, TunnelSpan};
