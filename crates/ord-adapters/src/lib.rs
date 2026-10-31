//! Codecs purs de tuiles (`TileCodec`) : streets (MVT + Shortbread), BIL, Terrarium, couverture.
//!
//! Incrément 1 : Terrarium (WebP de Mapterhorn). Incrément 2 : BIL32 de l'IGN et polygones de
//! couverture de Mapterhorn (MVT). Streets à l'incrément 3.

pub mod bil;
pub mod coverage;
pub mod terrarium;

pub use bil::BilCodec;
pub use coverage::CoverageCodec;
pub use terrarium::TerrariumCodec;
