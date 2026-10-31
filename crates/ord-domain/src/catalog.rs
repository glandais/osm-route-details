//! Attributions, versions des données et disponibilité (T01 §9, T02 §9).

use serde::Serialize;

use crate::ctx::DataLog;
use crate::tiles::SourceId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum SourceRole {
    Primary,
    Fallback,
    Coverage,
}

/// Version d'une source de données.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DataVersion {
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zoom: Option<u8>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub products: Vec<&'static str>,
    pub role: SourceRole,
    /// Source lue en ligne : version seulement indicative (doc 01 §8.3).
    pub indicative: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DataVersions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typology: Option<DataVersion>,
    pub elevation: Vec<DataVersion>,
}

/// Les trois versions exposées dans `meta` (doc 02 §6).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Versions {
    pub api_version: &'static str,
    pub rules_version: &'static str,
    pub data_versions: DataVersions,
}

/// Mention d'une source configurée.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Attribution {
    pub source: SourceId,
    /// Mention courte à relayer.
    pub short: String,
    pub licence: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// État observé des données locales, pour `/health/ready` ; jamais d'URL.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Readiness {
    pub ready: bool,
    pub checks: Vec<ReadinessCheck>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ReadinessCheck {
    pub name: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

pub trait DataCatalog: Send + Sync {
    /// Sources configurées seulement.
    fn attributions(&self) -> Vec<Attribution>;
    /// Sources utilisées par la requête ; `read_at` IGN tiré du journal.
    fn data_versions(&self, log: &DataLog) -> DataVersions;
    fn readiness(&self) -> Readiness;
}
