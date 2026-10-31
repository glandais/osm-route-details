//! Dérivés du profil (T04 §5.3–5.4) : lissage métrique, D+/D− à hystérésis, pentes.

pub mod gain;
pub mod smooth;

pub use gain::{accumulate, gain_on_runs, AdaptedRange, GainParams, GainResult};
pub use smooth::smooth_profile;
