//! Terrains analytiques servis en tuiles Terrarium WebP (T05 §2.2, `SyntheticTerrain`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ord_adapters::bil::encode_bil;
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

    /// Réponse BIL32 d'un `GetMap` WMS 1.3.0 en EPSG:4326 (`BBOX=s,w,n,e`, `WIDTH`) : chaque
    /// pixel vaut le terrain en son centre, lignes régulières en latitude ; `None` si la requête
    /// n'est pas lisible.
    pub fn bil(&self, req: &ScriptedRequest) -> Option<Vec<u8>> {
        let b: Vec<f64> = req.param("BBOX")?.split(',').map(str::parse).collect::<Result<_, _>>().ok()?;
        let size: u16 = req.param("WIDTH")?.parse().ok()?;
        let [s, w, n, e] = b[..] else { return None };
        let k = f64::from(size);
        Some(encode_bil(size, |px, py| {
            let lon = w + (f64::from(px) + 0.5) / k * (e - w);
            let lat = n - (f64::from(py) + 0.5) / k * (n - s);
            (self.f)(LatLon::new(lat, lon))
        }))
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
            if req.query.is_some() {
                return match self.bil(req) {
                    Some(body) => ScriptedResponse::ok(body).header("content-type", "image/x-bil;bits=32"),
                    None => ScriptedResponse::status(400),
                };
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
