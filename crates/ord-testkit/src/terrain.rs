//! Terrains analytiques servis en tuiles Terrarium WebP (T05 §2.2, `SyntheticTerrain`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ord_adapters::terrarium::encode_terrarium_webp;
use ord_data::elevation::pixel_center;
use ord_domain::LatLon;

use crate::server::{ScriptedRequest, ScriptedResponse, ScriptedServer};

/// Terrain défini par une fonction de la position ; chaque pixel vaut la fonction en son centre.
#[derive(Clone)]
pub struct SyntheticTerrain {
    f: Arc<dyn Fn(LatLon) -> f64 + Send + Sync>,
    pub tile_size: u16,
}

impl std::fmt::Debug for SyntheticTerrain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyntheticTerrain").field("tile_size", &self.tile_size).finish_non_exhaustive()
    }
}

impl SyntheticTerrain {
    pub fn new(tile_size: u16, f: impl Fn(LatLon) -> f64 + Send + Sync + 'static) -> Self {
        Self { f: Arc::new(f), tile_size }
    }

    /// Altitude du terrain en un point.
    pub fn at(&self, p: LatLon) -> f64 {
        (self.f)(p)
    }

    /// Tuile Terrarium `z/x/y` encodée en WebP sans perte.
    pub fn tile(&self, z: u8, x: u32, y: u32) -> Vec<u8> {
        let size = self.tile_size;
        let s = u64::from(size);
        encode_terrarium_webp(size, |px, py| {
            let p = pixel_center(
                u64::from(x) * s + u64::from(px),
                u64::from(y) * s + u64::from(py),
                z,
                u32::from(size),
            );
            (self.f)(p)
        })
    }

    /// Serveur scripté qui sert ce terrain (tuiles mémoïsées), avec un script facultatif qui
    /// remplace la réponse de certaines requêtes.
    pub async fn serve(
        self,
        script: impl Fn(usize, &ScriptedRequest) -> Option<ScriptedResponse> + Send + Sync + 'static,
    ) -> ScriptedServer {
        let memo: Mutex<HashMap<(u8, u32, u32), Vec<u8>>> = Mutex::new(HashMap::new());
        ScriptedServer::start(move |n, req| {
            if let Some(reply) = script(n, req) {
                return reply;
            }
            let mut memo = memo.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let body =
                memo.entry((req.z, req.x, req.y)).or_insert_with(|| self.tile(req.z, req.x, req.y)).clone();
            ScriptedResponse::ok(body)
                .header("content-type", "image/webp")
                .header("cache-control", "public, max-age=604800")
        })
        .await
    }
}
