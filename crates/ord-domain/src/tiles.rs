//! Tuiles : clés, issues de lecture, codecs et port objet (T02 §2.2, §6.1).

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;

use crate::catalog::DataVersion;
use crate::ctx::{RequestCtx, TileFailure};

/// Clé de tuile XYZ. Une clé est une coordonnée (≈ 2,4 km à z14) : `Debug` masqué.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileKey {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

impl fmt::Debug for TileKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TileKey(z{}, ..)", self.z)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum SourceId {
    Streets,
    IgnDtm,
    IgnDsm,
    Mapterhorn,
    MapterhornCoverage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TtlClass {
    Immutable,
    Expires(Duration),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("unexpected tile size: {0} bytes")]
    Size(usize),
    #[error("invalid tile format: {0}")]
    Format(String),
}

/// Tuile d'altitude décodée (BIL ou Terrarium).
#[derive(Clone, Debug, PartialEq)]
pub enum ElevationTile {
    NoData,
    Grid { size: u16, values: Box<[f32]>, all_valid: bool },
}

/// Décodeur pur d'une source (implémenté dans `ord-adapters`).
pub trait TileCodec: Send + Sync + 'static {
    type Tile: Send + Sync + 'static;
    fn decode(&self, id: TileKey, bytes: &[u8]) -> Result<Self::Tile, DecodeError>;
    fn empty(&self, id: TileKey) -> Self::Tile;
    /// Poids en octets, pour le LRU.
    fn weight(&self, tile: &Self::Tile) -> u32;
    fn ttl_class(&self, tile: &Self::Tile) -> TtlClass;
}

/// Issue d'une lecture : quatre états distincts, `Failed` n'est jamais « pas de rue ».
#[derive(Debug)]
pub enum TileOutcome<T> {
    Data { tile: Arc<T>, stale: bool },
    Empty(Arc<T>),
    Absent,
    Failed(TileFailure),
}

/// Vue objet d'un lecteur de tuiles, consommée par le pipeline.
#[async_trait::async_trait]
pub trait TileStore<T>: Send + Sync {
    async fn get_many(&self, keys: &[TileKey], ctx: &RequestCtx) -> Vec<TileOutcome<T>>;
    fn data_version(&self) -> DataVersion;
}
