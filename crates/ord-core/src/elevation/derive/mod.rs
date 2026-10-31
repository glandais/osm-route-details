//! Dérivés du profil (T04 §5.3–5.4) : lissage métrique, D+/D− à hystérésis, pentes.

pub mod gain;
pub mod smooth;

pub use gain::{
    accumulate, accumulate_turns, accumulate_with, gain_on_runs, gain_on_runs_with, gain_turns_on_runs,
    AdaptedRange, GainParams, GainResult, TurningPoint,
};
pub use smooth::smooth_profile;
