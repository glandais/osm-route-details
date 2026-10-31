//! Codecs purs de tuiles (`TileCodec`) : streets (MVT + Shortbread), BIL, Terrarium, couverture.
//!
//! Incrément 1 : Terrarium (WebP de Mapterhorn). Incrément 2 : BIL32 de l'IGN et polygones de
//! couverture de Mapterhorn (MVT). Incrément 3 : tuiles de rues (MVT, schéma Shortbread).

pub mod bil;
pub mod coverage;
pub mod streets;
pub mod terrarium;

pub use bil::BilCodec;
pub use coverage::CoverageCodec;
pub use streets::{Shortbread, StreetsCodec};
pub use terrarium::TerrariumCodec;
