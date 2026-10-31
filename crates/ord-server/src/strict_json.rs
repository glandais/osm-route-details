//! Extracteur JSON strict (T01 §4.2, étapes 1 à 3) : toute erreur d'entrée est un problème RFC 9457,
//! et **toutes** les clés inconnues sont listées.

use std::error::Error as _;

use axum::extract::{FromRef, FromRequest, Request};
use axum::http::header;
use http_body_util::LengthLimitError;
use serde::de::DeserializeOwned;

use crate::problem::{FieldError, LimitKind, Problem, ProblemCode, ProblemDetails};

/// Taille maximale du corps décompressé.
#[derive(Clone, Copy, Debug)]
pub struct MaxBodyBytes(pub usize);

#[derive(Debug)]
pub struct StrictJson<T>(pub T);

impl<S, T> FromRequest<S> for StrictJson<T>
where
    S: Send + Sync,
    MaxBodyBytes: FromRef<S>,
    T: DeserializeOwned,
{
    type Rejection = Problem;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        check_content_type(req.headers().get(header::CONTENT_TYPE))?;
        let MaxBodyBytes(limit) = MaxBodyBytes::from_ref(state);
        let bytes = axum::body::to_bytes(req.into_body(), limit).await.map_err(|err| {
            let mut source: Option<&(dyn std::error::Error + 'static)> = err.source();
            while let Some(e) = source {
                if e.is::<LengthLimitError>() {
                    return Problem::too_large(
                        LimitKind::BodyBytes,
                        format!("request body exceeds {limit} bytes"),
                    );
                }
                source = e.source();
            }
            Problem::invalid_input("request body could not be read", Vec::new())
        })?;
        parse_strict(&bytes).map(StrictJson)
    }
}

/// `application/json`, paramètre `charset=utf-8` toléré ; sinon 415.
fn check_content_type(value: Option<&header::HeaderValue>) -> Result<(), Problem> {
    let refuse = || Problem::unsupported_media_type("Content-Type must be application/json");
    let value = value.and_then(|v| v.to_str().ok()).ok_or_else(refuse)?;
    let mut parts = value.split(';').map(str::trim);
    if !parts.next().is_some_and(|essence| essence.eq_ignore_ascii_case("application/json")) {
        return Err(refuse());
    }
    for param in parts.filter(|p| !p.is_empty()) {
        let (name, val) = param.split_once('=').ok_or_else(refuse)?;
        let val = val.trim().trim_matches('"');
        if !(name.trim().eq_ignore_ascii_case("charset") && val.eq_ignore_ascii_case("utf-8")) {
            return Err(refuse());
        }
    }
    Ok(())
}

/// Désérialise en collectant toutes les clés inconnues (pointeurs JSON) ; une clé inconnue est
/// refusée même si le reste est valide.
pub fn parse_strict<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Problem> {
    let mut unknown = Vec::new();
    let mut de = serde_json::Deserializer::from_slice(bytes);
    let mut record = |path: serde_ignored::Path<'_>| unknown.push(ignored_pointer(&path));
    let result: Result<T, _> =
        serde_path_to_error::deserialize(serde_ignored::Deserializer::new(&mut de, &mut record));
    // Données après la valeur JSON : erreur de syntaxe à la racine.
    let result = result
        .map_err(|e| (error_pointer(e.path()), e.into_inner()))
        .and_then(|value| de.end().map(|()| value).map_err(|e| (String::new(), e)));
    match result {
        Ok(value) if unknown.is_empty() => Ok(value),
        Ok(_) => {
            let mut p = Problem::new(ProblemCode::InvalidInput, "unknown keys in request");
            p.details = Some(ProblemDetails { unknown_keys: Some(unknown), ..Default::default() });
            Err(p)
        }
        Err((pointer, inner)) => {
            let detail = if inner.is_data() {
                "request body does not match the schema"
            } else {
                "request body is not valid JSON"
            };
            let mut p = Problem::invalid_input(detail, vec![FieldError::new(pointer, sanitize(&inner))]);
            if !unknown.is_empty() {
                if let Some(d) = p.details.as_mut() {
                    d.unknown_keys = Some(unknown);
                }
            }
            Err(p)
        }
    }
}

fn escape(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

fn ignored_pointer(path: &serde_ignored::Path<'_>) -> String {
    use serde_ignored::Path;
    match path {
        Path::Root => String::new(),
        Path::Seq { parent, index } => format!("{}/{index}", ignored_pointer(parent)),
        Path::Map { parent, key } => format!("{}/{}", ignored_pointer(parent), escape(key)),
        Path::Some { parent } | Path::NewtypeStruct { parent } | Path::NewtypeVariant { parent } => {
            ignored_pointer(parent)
        }
    }
}

fn error_pointer(path: &serde_path_to_error::Path) -> String {
    use serde_path_to_error::Segment;
    path.iter()
        .filter_map(|segment| match segment {
            Segment::Seq { index } => Some(index.to_string()),
            Segment::Map { key } => Some(escape(key)),
            Segment::Enum { .. } | Segment::Unknown => None,
        })
        .fold(String::new(), |acc, token| format!("{acc}/{token}"))
}

/// Message serde sans la valeur reçue (qui peut être une coordonnée) ni la position.
fn sanitize(err: &serde_json::Error) -> String {
    let full = err.to_string();
    let msg = full.rfind(" at line ").map_or(full.as_str(), |i| &full[..i]);
    for prefix in ["invalid type", "invalid value"] {
        if msg.starts_with(prefix) {
            return match msg.find(", expected ") {
                Some(i) => format!("{prefix}{}", &msg[i..]),
                None => prefix.to_owned(),
            };
        }
    }
    if err.is_syntax() || err.is_eof() {
        return format!("{msg} (line {}, column {})", err.line(), err.column());
    }
    msg.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[allow(dead_code)]
    #[derive(Debug, Deserialize)]
    struct Body {
        points: Vec<Pt>,
        #[serde(default)]
        options: Option<Opts>,
    }
    #[allow(dead_code)]
    #[derive(Debug, Deserialize)]
    struct Pt {
        lat: f64,
        lon: f64,
    }
    #[allow(dead_code)]
    #[derive(Debug, Deserialize)]
    struct Opts {
        debug: Option<bool>,
    }

    fn details(p: Problem) -> ProblemDetails {
        p.details.expect("details")
    }

    #[test]
    fn lists_every_unknown_key_as_pointer() {
        let body = br#"{"points":[{"lat":1,"lon":2,"x/y":3}],"options":{"foo":1,"bar":{"baz":2}},"top":0}"#;
        let err = parse_strict::<Body>(body).expect_err("clés inconnues");
        assert_eq!(
            details(err).unknown_keys.expect("liste"),
            vec!["/points/0/x~1y", "/options/foo", "/options/bar", "/top"]
        );
    }

    #[test]
    fn type_error_has_pointer_and_no_value() {
        let err = parse_strict::<Body>(br#"{"points":[{"lat":"47.2184","lon":2}]}"#).expect_err("type");
        let errors = details(err).errors.expect("erreurs");
        assert_eq!(errors[0].pointer, "/points/0/lat");
        assert!(!errors[0].message.contains("47.2184"), "{}", errors[0].message);
    }

    #[test]
    fn syntax_error_and_trailing_data() {
        assert!(parse_strict::<Body>(br#"{"points":[}"#).is_err());
        assert!(parse_strict::<Body>(br#"{"points":[]} x"#).is_err());
        assert!(parse_strict::<Body>(br#"{"points":[]}"#).is_ok());
    }

    #[test]
    fn content_type_rules() {
        let ok = |v: &'static str| check_content_type(Some(&header::HeaderValue::from_static(v))).is_ok();
        assert!(ok("application/json"));
        assert!(ok("Application/JSON; charset=UTF-8"));
        assert!(ok("application/json; charset=\"utf-8\""));
        assert!(!ok("application/json; charset=latin1"));
        assert!(!ok("text/plain"));
        assert!(!ok("application/jsonx"));
        assert!(check_content_type(None).is_err());
    }
}
