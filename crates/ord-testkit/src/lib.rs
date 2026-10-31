//! Outillage de test (T05 §8) : parcours synthétiques, faux serveur de tuiles scripté
//! (`ScriptedServer`), terrains synthétiques servis en tuiles Terrarium, réseaux de rues
//! synthétiques servis en tuiles MVT Shortbread.
//!
//! Aucun test ne sort de la machine : le serveur écoute sur `127.0.0.1:0`.

pub mod routes;
pub mod server;
pub mod streets;
pub mod terrain;

pub use server::{ScriptedRequest, ScriptedResponse, ScriptedServer};
pub use streets::{SyntheticStreet, SyntheticStreets};
pub use terrain::SyntheticTerrain;
