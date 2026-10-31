//! Modèle neutre des rues (T03 §3), indépendant du schéma de tuiles.

use std::sync::Arc;

use crate::tiles::{DecodeError, TileKey};

/// Chaîne internée par tuile décodée.
pub type Sym = Arc<str>;

#[derive(Clone, Debug, PartialEq)]
pub struct WayAttrs {
    /// `highway` brut (Shortbread `kind`).
    pub highway: Sym,
    pub surface: SurfaceTag,
    pub tracktype: Option<TrackType>,
    pub bicycle: Option<Sym>,
    pub service: Option<Sym>,
    pub link: bool,
    pub rail: bool,
    pub structure: StructureTag,
    pub layer: Option<i8>,
    pub smoothness: Option<Sym>,
    pub osm_id: Option<u64>,
    /// Attributs bruts, seulement si `debug`.
    pub raw: Option<Box<[(Sym, Sym)]>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SurfaceTag {
    Absent,
    Grouped(PavedGroup),
    Raw(Sym),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PavedGroup {
    Paved,
    Unpaved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructureTag {
    None,
    Bridge,
    Tunnel,
    Covered,
    BuildingPassage,
    Ford,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackType {
    Grade1,
    Grade2,
    Grade3,
    Grade4,
    Grade5,
}

/// Ligne en unités globales ; `cut` : extrémité coupée au bord de la tuile.
#[derive(Clone, Debug, PartialEq)]
pub struct StreetLine {
    pub attr: u32,
    pub points: Vec<[f64; 2]>,
    pub cut: [bool; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointKind {
    Ford,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PointFeature {
    pub kind: PointKind,
    pub p: [f64; 2],
    pub osm_id: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StreetTile {
    pub key: TileKey,
    pub attrs: Vec<WayAttrs>,
    pub lines: Vec<StreetLine>,
    pub points: Vec<PointFeature>,
}

pub trait StreetSchemaAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn zoom(&self) -> u8;
    fn capabilities(&self) -> SchemaCapabilities;
    fn decode(&self, key: TileKey, bytes: &[u8]) -> Result<StreetTile, DecodeError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceFidelity {
    /// paved / unpaved (VersaTiles, OpenFreeMap).
    Grouped,
    /// `surface` brut (tuiles maison, V2).
    Raw,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SchemaCapabilities {
    pub surface: SurfaceFidelity,
    pub tracktype: bool,
    pub layer: bool,
    pub ford_ways: bool,
    pub ford_nodes: bool,
    pub covered_distinct: bool,
    pub osm_id: bool,
    pub smoothness: bool,
    pub lines_merged: bool,
}
