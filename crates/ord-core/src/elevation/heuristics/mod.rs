//! Heuristiques pures sur profils échantillonnés au pas fixe, portées de vcyclist : tabliers de
//! pont sur MNS/MNT LiDAR (`deck_detector`) et tunnels sur MNT (`tunnel_detector`).

pub mod deck_detector;
pub mod tunnel_detector;

pub use deck_detector::{
    detect_decks, DeckCandidate, DeckDetection, DeckDetectorOptions, DeckMetrics, DeckRejection,
};
pub use tunnel_detector::{detect_tunnels, TunnelDetectorOptions, TunnelSpan};
