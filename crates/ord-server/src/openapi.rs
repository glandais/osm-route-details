//! Contrat OpenAPI dérivé du code (T01 §10), écrit dans `contract/openapi.json`.

use utoipa::openapi::OpenApi as OpenApiDoc;
use utoipa::OpenApi;

use crate::app::paths::*;
use crate::API_VERSION;

#[derive(OpenApi)]
#[openapi(
    paths(route_details, capabilities, attribution, health_live, health_ready),
    tags(
        (name = "route", description = "Elevation and typology along a route"),
        (name = "service", description = "Capabilities and attribution"),
        (name = "health", description = "Liveness and readiness probes"),
    )
)]
struct ApiDoc;

pub fn openapi() -> OpenApiDoc {
    let mut doc = ApiDoc::openapi();
    // Version du contrat, jamais celle du paquet (T01 §9).
    doc.info = utoipa::openapi::InfoBuilder::new()
        .title("osm-route-details")
        .version(API_VERSION)
        .description(Some("Elevation and typology (structure, surface) along a route."))
        .build();
    doc
}

/// Rendu canonique du contrat : JSON indenté, terminé par un saut de ligne.
pub fn render_contract() -> String {
    let mut out = serde_json::to_string_pretty(&openapi()).unwrap_or_default();
    out.push('\n');
    out
}
