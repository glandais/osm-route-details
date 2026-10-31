//! Problèmes RFC 9457 : seul site de traduction des erreurs en HTTP (T01 §4.3).

use std::future::Future;
use std::time::Duration;

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use ord_pipeline::{DataSide, PipelineError};
use serde::Serialize;
use utoipa::ToSchema;

pub const PROBLEM_CONTENT_TYPE: &str = "application/problem+json";
pub const PROBLEM_TYPE_PREFIX: &str = "urn:osm-route-details:problem:";

tokio::task_local! {
    static REQUEST_ID: String;
}

/// Exécute `f` avec l'identifiant de requête lu par chaque problème rendu (`instance`).
pub async fn with_request_id<F: Future>(id: String, f: F) -> F::Output {
    REQUEST_ID.scope(id, f).await
}

fn current_request_id() -> Option<String> {
    REQUEST_ID.try_with(Clone::clone).ok()
}

/// Code machine stable, suffixe du `type` (doc 02 §5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProblemCode {
    InvalidInput,
    RouteTooLarge,
    OutOfCoverage,
    DataUnavailable,
    Overloaded,
    Timeout,
    Internal,
    NotFound,
    MethodNotAllowed,
    UnsupportedMediaType,
}

impl ProblemCode {
    pub const ALL: [Self; 10] = [
        Self::InvalidInput,
        Self::RouteTooLarge,
        Self::OutOfCoverage,
        Self::DataUnavailable,
        Self::Overloaded,
        Self::Timeout,
        Self::Internal,
        Self::NotFound,
        Self::MethodNotAllowed,
        Self::UnsupportedMediaType,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid_input",
            Self::RouteTooLarge => "route_too_large",
            Self::OutOfCoverage => "out_of_coverage",
            Self::DataUnavailable => "data_unavailable",
            Self::Overloaded => "overloaded",
            Self::Timeout => "timeout",
            Self::Internal => "internal",
            Self::NotFound => "not_found",
            Self::MethodNotAllowed => "method_not_allowed",
            Self::UnsupportedMediaType => "unsupported_media_type",
        }
    }

    pub fn status(self) -> StatusCode {
        match self {
            Self::InvalidInput => StatusCode::BAD_REQUEST,
            Self::RouteTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::OutOfCoverage => StatusCode::UNPROCESSABLE_ENTITY,
            Self::DataUnavailable | Self::Overloaded => StatusCode::SERVICE_UNAVAILABLE,
            Self::Timeout => StatusCode::GATEWAY_TIMEOUT,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::InvalidInput => "Invalid input",
            Self::RouteTooLarge => "Route too large",
            Self::OutOfCoverage => "Out of coverage",
            Self::DataUnavailable => "Data unavailable",
            Self::Overloaded => "Overloaded",
            Self::Timeout => "Timeout",
            Self::Internal => "Internal error",
            Self::NotFound => "Not found",
            Self::MethodNotAllowed => "Method not allowed",
            Self::UnsupportedMediaType => "Unsupported media type",
        }
    }
}

/// Erreur sur un champ de l'entrée : un pointeur JSON, jamais la valeur.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct FieldError {
    /// Pointeur JSON (RFC 6901) vers le champ fautif.
    pub pointer: String,
    pub message: String,
}

impl FieldError {
    pub fn new(pointer: impl Into<String>, message: impl Into<String>) -> Self {
        Self { pointer: pointer.into(), message: message.into() }
    }
}

/// Borne d'entrée dépassée (413).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LimitKind {
    Points,
    LengthM,
    BodyBytes,
}

/// Côté en panne d'un 503 `data_unavailable`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProblemSource {
    Typology,
    Elevation,
    Both,
}

impl From<DataSide> for ProblemSource {
    fn from(side: DataSide) -> Self {
        match side {
            DataSide::Typology => Self::Typology,
            DataSide::Elevation => Self::Elevation,
            DataSide::Both => Self::Both,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, ToSchema)]
pub struct ProblemDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<FieldError>>,
    /// Pointeurs JSON de toutes les clés inconnues.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unknown_keys: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<LimitKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<ProblemSource>,
}

/// Problème à rendre ; `instance` est ajouté au rendu.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub code: ProblemCode,
    /// Diagnostic anglais, sans coordonnées.
    pub detail: String,
    pub details: Option<ProblemDetails>,
    pub retry_after: Option<Duration>,
}

impl Problem {
    pub fn new(code: ProblemCode, detail: impl Into<String>) -> Self {
        Self { code, detail: detail.into(), details: None, retry_after: None }
    }

    pub fn invalid_input(detail: impl Into<String>, errors: Vec<FieldError>) -> Self {
        let mut p = Self::new(ProblemCode::InvalidInput, detail);
        p.details = Some(ProblemDetails { errors: Some(errors), ..Default::default() });
        p
    }

    pub fn too_large(limit: LimitKind, detail: impl Into<String>) -> Self {
        let mut p = Self::new(ProblemCode::RouteTooLarge, detail);
        p.details = Some(ProblemDetails { limit: Some(limit), ..Default::default() });
        p
    }

    pub fn internal() -> Self {
        Self::new(ProblemCode::Internal, "unexpected error")
    }

    pub fn not_found() -> Self {
        Self::new(ProblemCode::NotFound, "no such route")
    }

    pub fn method_not_allowed() -> Self {
        Self::new(ProblemCode::MethodNotAllowed, "method not allowed on this route")
    }

    pub fn unsupported_media_type(detail: impl Into<String>) -> Self {
        Self::new(ProblemCode::UnsupportedMediaType, detail)
    }

    pub fn body(&self) -> ProblemBody {
        ProblemBody {
            type_: format!("{PROBLEM_TYPE_PREFIX}{}", self.code.as_str()),
            title: self.code.title().to_owned(),
            status: self.code.status().as_u16(),
            detail: self.detail.clone(),
            code: self.code,
            details: self.details.clone(),
            instance: current_request_id(),
        }
    }
}

/// Corps `application/problem+json` (RFC 9457, doc 02 §5.1).
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct ProblemBody {
    /// `urn:osm-route-details:problem:<code>`.
    #[serde(rename = "type")]
    pub type_: String,
    pub title: String,
    pub status: u16,
    pub detail: String,
    pub code: ProblemCode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<ProblemDetails>,
    /// Identifiant de la requête (`X-Request-Id`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
}

impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        let body = serde_json::to_vec(&self.body()).unwrap_or_default();
        let mut response = (self.code.status(), body).into_response();
        let headers = response.headers_mut();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(PROBLEM_CONTENT_TYPE));
        if let Some(retry_after) = self.retry_after {
            headers.insert(header::RETRY_AFTER, HeaderValue::from(retry_after.as_secs().max(1)));
        }
        response.extensions_mut().insert(self.code);
        response
    }
}

impl From<PipelineError> for Problem {
    fn from(err: PipelineError) -> Self {
        match err {
            PipelineError::Unavailable { source, retry_after } => {
                let mut p = Self::new(ProblemCode::DataUnavailable, err.to_string());
                p.details = Some(ProblemDetails { source: Some(source.into()), ..Default::default() });
                p.retry_after = retry_after;
                p
            }
            PipelineError::OutOfCoverage => Self::new(ProblemCode::OutOfCoverage, err.to_string()),
            PipelineError::Timeout => Self::new(ProblemCode::Timeout, err.to_string()),
            PipelineError::Overloaded => Self::new(ProblemCode::Overloaded, err.to_string()),
            PipelineError::Cancelled | PipelineError::Internal(_) => {
                // La cause va au log, jamais au client.
                tracing::error!(error = %err, "internal error");
                Self::internal()
            }
        }
    }
}

/// Problème déduit du seul statut, pour les réponses d'erreur produites hors des handlers
/// (couches tower, routage) qui ne sont pas déjà au format RFC 9457.
pub fn problem_for_status(status: StatusCode) -> Problem {
    match status {
        StatusCode::NOT_FOUND => Problem::not_found(),
        StatusCode::METHOD_NOT_ALLOWED => Problem::method_not_allowed(),
        StatusCode::PAYLOAD_TOO_LARGE => Problem::too_large(LimitKind::BodyBytes, "request body too large"),
        StatusCode::UNSUPPORTED_MEDIA_TYPE => {
            Problem::unsupported_media_type("unsupported content encoding or media type")
        }
        s if s.is_client_error() => Problem::new(ProblemCode::InvalidInput, "invalid request"),
        _ => Problem::internal(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_consistent_with_their_wire_name() {
        for code in ProblemCode::ALL {
            let json = serde_json::to_value(code).expect("sérialisable");
            assert_eq!(json, code.as_str());
        }
        for code in [ProblemCode::NotFound, ProblemCode::MethodNotAllowed, ProblemCode::UnsupportedMediaType]
        {
            assert_eq!(problem_for_status(code.status()).code, code);
        }
    }

    #[test]
    fn pipeline_errors_map_to_contract_statuses() {
        let unavailable = Problem::from(PipelineError::Unavailable {
            source: DataSide::Both,
            retry_after: Some(Duration::from_secs(30)),
        });
        assert_eq!(unavailable.code, ProblemCode::DataUnavailable);
        assert_eq!(unavailable.details.and_then(|d| d.source), Some(ProblemSource::Both));
        assert_eq!(Problem::from(PipelineError::Timeout).code.status(), StatusCode::GATEWAY_TIMEOUT);
        let internal = Problem::from(PipelineError::Internal("secret cause".into()));
        assert_eq!(internal.code, ProblemCode::Internal);
        assert!(!internal.detail.contains("secret"));
    }
}
