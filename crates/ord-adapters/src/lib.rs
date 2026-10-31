//! Codecs purs de tuiles (`TileCodec`) : streets (MVT + Shortbread), BIL, Terrarium.
//!
//! Incrément 1 : Terrarium (WebP de Mapterhorn). BIL arrive à l'incrément 2, streets à
//! l'incrément 3.

pub mod terrarium;

pub use terrarium::TerrariumCodec;
