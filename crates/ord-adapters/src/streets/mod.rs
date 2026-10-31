//! Tuiles de rues (T02 §8, T03 §3) : décodage MVT générique et schémas de tuiles.

pub mod mvt;
pub mod shortbread;

use std::time::Duration;

use ord_domain::street::{StreetSchemaAdapter, StreetTile};
use ord_domain::tiles::{DecodeError, TileCodec, TileKey, TtlClass};

pub use shortbread::Shortbread;

/// Codec de tuiles de rues pour un schéma `A` : une tuile est décodée une fois par processus et
/// partagée (`Arc<StreetTile>`) par le cache mémoire du lecteur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreetsCodec<A> {
    pub adapter: A,
    /// TTL des tuiles (VersaTiles : 7 j, T02 §5.3).
    pub ttl: Duration,
}

impl<A: StreetSchemaAdapter> StreetsCodec<A> {
    pub fn new(adapter: A, ttl: Duration) -> Self {
        Self { adapter, ttl }
    }
}

impl<A: StreetSchemaAdapter + 'static> TileCodec for StreetsCodec<A> {
    type Tile = StreetTile;

    fn decode(&self, id: TileKey, bytes: &[u8]) -> Result<StreetTile, DecodeError> {
        self.adapter.decode(id, bytes)
    }

    fn empty(&self, id: TileKey) -> StreetTile {
        StreetTile::empty(id)
    }

    fn weight(&self, tile: &StreetTile) -> u32 {
        let points: usize = tile.lines.iter().map(|l| l.points.len()).sum();
        let bytes = 64 + points * 16 + tile.lines.len() * 48 + tile.attrs.len() * 160;
        u32::try_from(bytes).unwrap_or(u32::MAX)
    }

    fn ttl_class(&self, _tile: &StreetTile) -> TtlClass {
        TtlClass::Expires(self.ttl)
    }
}
