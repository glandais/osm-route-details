//! Service HTTP osm-route-details : DTO v1, validation, problèmes RFC 9457, configuration,
//! observabilité, OpenAPI, câblage (T01 §4, §7–§10).

pub mod app;
pub mod capabilities;
pub mod config;
pub mod dto;
pub mod observability;
pub mod openapi;
pub mod problem;
pub mod strict_json;
pub mod validate;
pub mod wiring;

/// Version du contrat, montée à la main quand il change de façon observable (T01 §9).
pub const API_VERSION: &str = "1.3";

pub use app::{internal_router, router, AppState};
pub use openapi::{openapi, render_contract};
pub use wiring::{StartupError, Wiring};
