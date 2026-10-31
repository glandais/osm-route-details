//! Réseaux de rues synthétiques servis en tuiles MVT Shortbread z14 (`SyntheticStreets`).

use std::sync::Arc;

use ord_adapters::streets::mvt::{encode_line_layer, EncodeFeature, MvtValue};
use ord_adapters::streets::shortbread::STREETS_LAYER;
use ord_adapters::Shortbread;
use ord_domain::tiles::TileKey;
use ord_domain::LatLon;

use crate::server::{ScriptedRequest, ScriptedResponse};

/// Rue synthétique : propriétés Shortbread (`kind`, `surface`, `bridge`…) et tracé.
#[derive(Clone, Debug)]
pub struct SyntheticStreet {
    pub properties: Vec<(String, String)>,
    pub points: Vec<LatLon>,
}

impl SyntheticStreet {
    pub fn new(kind: &str, points: Vec<LatLon>) -> Self {
        Self { properties: vec![("kind".into(), kind.into())], points }
    }

    pub fn with(mut self, key: &str, value: &str) -> Self {
        self.properties.push((key.into(), value.into()));
        self
    }
}

/// Réseau servi tuile par tuile : chaque tuile porte les rues entières (le décodeur les découpe).
#[derive(Clone, Debug, Default)]
pub struct SyntheticStreets {
    pub streets: Arc<Vec<SyntheticStreet>>,
}

impl SyntheticStreets {
    pub fn new(streets: Vec<SyntheticStreet>) -> Self {
        Self { streets: Arc::new(streets) }
    }

    /// Tuile MVT `z/x/y` (couche `streets`).
    pub fn tile(&self, key: TileKey) -> Vec<u8> {
        let grid = Shortbread::GRID;
        let [x0, y0] = grid.tile_origin(key);
        let features: Vec<EncodeFeature> = self
            .streets
            .iter()
            .map(|s| EncodeFeature {
                properties: s
                    .properties
                    .iter()
                    .map(|(k, v)| {
                        let value = match v.as_str() {
                            "true" => MvtValue::Bool(true),
                            other => MvtValue::String(other.into()),
                        };
                        (k.clone(), value)
                    })
                    .collect(),
                lines: vec![s
                    .points
                    .iter()
                    .map(|p| {
                        let u = grid.to_units(*p);
                        [(u[0] - x0).round() as i64, (u[1] - y0).round() as i64]
                    })
                    .collect()],
            })
            .collect();
        encode_line_layer(STREETS_LAYER, grid.extent, &features)
    }

    /// Script pour `SyntheticTerrain::serve` : répond aux tuiles z14 (hors WMS).
    pub fn script(
        &self,
    ) -> impl Fn(usize, &ScriptedRequest) -> Option<ScriptedResponse> + Send + Sync + 'static {
        let me = self.clone();
        move |_, req| {
            (req.query.is_none() && req.z == Shortbread::ZOOM).then(|| {
                ScriptedResponse::ok(me.tile(TileKey { z: req.z, x: req.x, y: req.y }))
                    .header("content-type", "application/x-protobuf")
            })
        }
    }
}
