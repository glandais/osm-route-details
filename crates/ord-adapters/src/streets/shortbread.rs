//! Schéma Shortbread 1.0 de VersaTiles, couche `streets` (T03 §3.2, exploration 02 §4).
//!
//! Clés lues comme osm-surface (`src/mvt.rs`, Apache-2.0, © Gabriel Landais) : `kind`, `surface`,
//! `tracktype`, `service`, `bicycle`, `link`, `rail`, `bridge`, `tunnel`. `oneway` et `horse` ne
//! sont plus lus.

use std::collections::HashMap;
use std::sync::Arc;

use ord_domain::geo::Grid;
use ord_domain::street::{
    PavedGroup, SchemaCapabilities, StreetLine, StreetSchemaAdapter, StreetTile, StructureTag,
    SurfaceFidelity, SurfaceTag, Sym, TrackType, WayAttrs,
};
use ord_domain::tiles::{DecodeError, TileKey};

use super::mvt::{decode_line_layer, MvtValue};

pub const STREETS_LAYER: &str = "streets";

/// Adaptateur Shortbread 1.0 (VersaTiles), zoom 14, extent 4096.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shortbread;

impl Shortbread {
    pub const ID: &'static str = "versatiles-shortbread-1.0";
    pub const ZOOM: u8 = 14;
    pub const EXTENT: u32 = 4096;
    pub const GRID: Grid = Grid::new(Self::ZOOM, Self::EXTENT);
}

/// Table d'internement des chaînes d'une tuile.
#[derive(Default)]
struct Interner(HashMap<String, Sym>);

impl Interner {
    fn sym(&mut self, s: &str) -> Sym {
        if let Some(v) = self.0.get(s) {
            return Arc::clone(v);
        }
        let v: Sym = Arc::from(s);
        self.0.insert(s.to_owned(), Arc::clone(&v));
        v
    }
}

fn track_type(v: &str) -> Option<TrackType> {
    match v {
        "grade1" => Some(TrackType::Grade1),
        "grade2" => Some(TrackType::Grade2),
        "grade3" => Some(TrackType::Grade3),
        "grade4" => Some(TrackType::Grade4),
        "grade5" => Some(TrackType::Grade5),
        _ => None,
    }
}

fn attrs_of(properties: &[(String, MvtValue)], interner: &mut Interner) -> WayAttrs {
    let mut a = WayAttrs {
        highway: interner.sym(""),
        surface: SurfaceTag::Absent,
        tracktype: None,
        bicycle: None,
        service: None,
        link: false,
        rail: false,
        structure: StructureTag::None,
        layer: None,
        smoothness: None,
        osm_id: None,
        raw: None,
    };
    let (mut bridge, mut tunnel) = (false, false);
    for (k, v) in properties {
        match k.as_str() {
            "kind" => a.highway = interner.sym(&v.as_string()),
            // VersaTiles ne sert que `paved` / `unpaved` ; toute autre valeur est ignorée.
            "surface" => {
                a.surface = match v.as_string().as_str() {
                    "paved" => SurfaceTag::Grouped(PavedGroup::Paved),
                    "unpaved" => SurfaceTag::Grouped(PavedGroup::Unpaved),
                    _ => SurfaceTag::Absent,
                }
            }
            "tracktype" => a.tracktype = track_type(&v.as_string()),
            "service" => a.service = Some(interner.sym(&v.as_string())),
            "bicycle" => a.bicycle = Some(interner.sym(&v.as_string())),
            "link" => a.link = v.as_bool(),
            "rail" => a.rail = v.as_bool(),
            "bridge" => bridge = v.as_bool(),
            "tunnel" => tunnel = v.as_bool(),
            _ => {}
        }
    }
    // `bridge` prime : un pont dans un tunnel n'existe pas dans OSM ; `tunnel` couvre aussi
    // `covered` et `building_passage`, que Shortbread ne distingue pas.
    a.structure = if bridge {
        StructureTag::Bridge
    } else if tunnel {
        StructureTag::Tunnel
    } else {
        StructureTag::None
    };
    a
}

impl StreetSchemaAdapter for Shortbread {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn zoom(&self) -> u8 {
        Self::ZOOM
    }

    fn capabilities(&self) -> SchemaCapabilities {
        SchemaCapabilities {
            surface: SurfaceFidelity::Grouped,
            tracktype: true,
            layer: false,
            ford_ways: false,
            ford_nodes: false,
            covered_distinct: false,
            osm_id: false,
            smoothness: false,
            lines_merged: true,
        }
    }

    fn decode(&self, key: TileKey, bytes: &[u8]) -> Result<StreetTile, DecodeError> {
        let features = decode_line_layer(bytes, STREETS_LAYER, key, Self::GRID)?;
        let mut tile = StreetTile::empty(key);
        let mut interner = Interner::default();
        let mut ids: HashMap<WayAttrs, u32> = HashMap::new();
        for f in features {
            let attrs = attrs_of(&f.properties, &mut interner);
            if attrs.highway.is_empty() {
                continue;
            }
            let attr = *ids.entry(attrs.clone()).or_insert_with(|| {
                tile.attrs.push(attrs);
                (tile.attrs.len() - 1) as u32
            });
            tile.lines.extend(f.lines.into_iter().map(|l| StreetLine { attr, points: l.points, cut: l.cut }));
        }
        Ok(tile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streets::mvt::{encode_line_layer, EncodeFeature};

    fn s(v: &str) -> MvtValue {
        MvtValue::String(v.into())
    }

    #[test]
    fn shortbread_properties_become_neutral_attributes() {
        let key = TileKey { z: 14, x: 10, y: 20 };
        let features = vec![
            EncodeFeature {
                properties: vec![
                    ("kind".into(), s("track")),
                    ("surface".into(), s("unpaved")),
                    ("tracktype".into(), s("grade2")),
                    ("bicycle".into(), s("designated")),
                ],
                lines: vec![vec![[10, 10], [100, 10]], vec![[200, 200], [300, 300]]],
            },
            EncodeFeature {
                properties: vec![
                    ("kind".into(), s("primary")),
                    ("bridge".into(), MvtValue::Bool(true)),
                    ("link".into(), s("true")),
                ],
                lines: vec![vec![[0, 50], [50, 50]]],
            },
            EncodeFeature {
                properties: vec![
                    ("kind".into(), s("rail")),
                    ("rail".into(), MvtValue::Bool(true)),
                    ("tunnel".into(), MvtValue::Int(1)),
                ],
                lines: vec![vec![[0, 60], [50, 60]]],
            },
            // Sans `kind` : ignorée.
            EncodeFeature {
                properties: vec![("bridge".into(), MvtValue::Bool(true))],
                lines: vec![vec![[0, 70], [9, 70]]],
            },
        ];
        let tile = Shortbread.decode(key, &encode_line_layer("streets", 4096, &features)).unwrap();
        assert_eq!(tile.attrs.len(), 3);
        assert_eq!(tile.lines.len(), 4);
        let track = &tile.attrs[0];
        assert_eq!(&*track.highway, "track");
        assert_eq!(track.surface, SurfaceTag::Grouped(PavedGroup::Unpaved));
        assert_eq!(track.tracktype, Some(TrackType::Grade2));
        assert_eq!(track.bicycle.as_deref(), Some("designated"));
        assert_eq!(track.structure, StructureTag::None);
        let primary = &tile.attrs[1];
        assert!(
            primary.link
                && primary.structure == StructureTag::Bridge
                && primary.surface == SurfaceTag::Absent
        );
        assert!(tile.attrs[2].rail && tile.attrs[2].structure == StructureTag::Tunnel);
        assert_eq!(tile.lines[0].attr, 0);
        assert_eq!(tile.lines[1].attr, 0, "même entité, même classe d'attributs");
        let [x0, y0] = Shortbread::GRID.tile_origin(key);
        for l in &tile.lines {
            for p in &l.points {
                assert!(p[0] >= x0 && p[0] <= x0 + 4096.0 && p[1] >= y0 && p[1] <= y0 + 4096.0);
            }
        }
    }

    #[test]
    fn empty_and_invalid_bodies() {
        let key = TileKey { z: 14, x: 0, y: 0 };
        assert!(Shortbread.decode(key, &[]).unwrap().lines.is_empty());
        assert!(Shortbread.decode(key, b"<html>oops</html>").is_err());
    }
}
