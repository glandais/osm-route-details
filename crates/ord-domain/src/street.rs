//! Modèle neutre des rues (T03 §3), indépendant du schéma de tuiles.

use std::sync::Arc;

use crate::tiles::{DecodeError, TileKey};

/// Chaîne internée par tuile décodée.
pub type Sym = Arc<str>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SurfaceTag {
    Absent,
    Grouped(PavedGroup),
    Raw(Sym),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PavedGroup {
    Paved,
    Unpaved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StructureTag {
    None,
    Bridge,
    Tunnel,
    Covered,
    BuildingPassage,
    Ford,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TrackType {
    Grade1,
    Grade2,
    Grade3,
    Grade4,
    Grade5,
}

/// Ligne en unités globales ; `cut` : extrémité coupée au bord de la tuile.
#[derive(Clone, PartialEq)]
pub struct StreetLine {
    pub attr: u32,
    pub points: Vec<[f64; 2]>,
    pub cut: [bool; 2],
}

impl std::fmt::Debug for StreetLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreetLine")
            .field("attr", &self.attr)
            .field("points", &self.points.len())
            .field("cut", &self.cut)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointKind {
    Ford,
}

#[derive(Clone, PartialEq)]
pub struct PointFeature {
    pub kind: PointKind,
    pub p: [f64; 2],
    pub osm_id: Option<u64>,
}

impl std::fmt::Debug for PointFeature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PointFeature").field("kind", &self.kind).finish_non_exhaustive()
    }
}

/// Lignes de la couche des rues d'une tuile, en unités globales (`Debug` masqué : coordonnées).
#[derive(Clone, PartialEq)]
pub struct StreetTile {
    pub key: TileKey,
    pub attrs: Vec<WayAttrs>,
    pub lines: Vec<StreetLine>,
    pub points: Vec<PointFeature>,
}

impl StreetTile {
    pub fn empty(key: TileKey) -> Self {
        Self { key, attrs: Vec::new(), lines: Vec::new(), points: Vec::new() }
    }

    /// Nombre de segments (paires de sommets consécutifs).
    pub fn segment_count(&self) -> usize {
        self.lines.iter().map(|l| l.points.len().saturating_sub(1)).sum()
    }
}

impl std::fmt::Debug for StreetTile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreetTile")
            .field("key", &self.key)
            .field("attrs", &self.attrs.len())
            .field("lines", &self.lines.len())
            .finish_non_exhaustive()
    }
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
