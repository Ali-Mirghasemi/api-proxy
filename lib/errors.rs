//! Central error definitions for the API proxy.
//!
//! This module defines a unified [`Error`] enum used across
//! configuration loading, request validation, proxying,
//! and Actix-Web integration.
//!
//! Errors implement [`ResponseError`] so they can be
//! returned directly from HTTP handlers.

use actix_web::{HttpResponse, ResponseError};
use serde_json::Value;

use crate::config::FieldType;

/// Unified error type for the API proxy.
///
/// Covers:
/// - IO and parsing errors
/// - Configuration errors
/// - Validation failures
/// - HTTP request violations
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// IO-related error.
    #[error("IO Error {0}")]
    IO(#[from] std::io::Error),

    /// TOML parsing error.
    #[cfg(feature = "toml")]
    #[error("TOML Error {0}")]
    TOML(#[from] toml::de::Error),

    /// JSON parsing error.
    #[cfg(feature = "json")]
    #[error("Json Error {0}")]
    Json(#[from] serde_json::Error),
    
    /// URL parsing error.
    #[error("IpAddr Error {0}")]
    IpAddr(#[from] std::net::AddrParseError),

    /// RcGen error.
    #[cfg(feature = "tls")]
    #[error("RcGen Error {0}")]
    RcGen(#[from] rcgen::Error),

    /// Infallible error wrapper.
    #[error("Infallible Error {0}")]
    Infallible(#[from] std::convert::Infallible),
    
    /// HTTP request cookie error.
    #[error("Cookie Error {0}")]
    Cookie(#[from] actix_web::cookie::ParseError),

    /// HTTP request URI parsing error.
    #[error("ParseUtf8 Error {0}")]
    ParseUtf8(#[from] std::string::FromUtf8Error),

    /// HTTP request ToStr error.
    #[error("ToStr Error {0}")]
    ToStr(#[from] actix_http::header::ToStrError),

    /// Unsupported configuration or feature.
    #[error("Not Supported")]
    NotSupported,

    /// Fallback error.
    #[error("Unknown Error")]
    Unknown,

    /// Request payload exceeds configured size.
    #[error("Payload Limit")]
    PayloadLimit,

    /// Field not found or invalid during validation.
    #[error("Field not found, {0}")]
    FieldNotFound(String),
    
    /// Field not an array during validation.
    #[error("Field not object, {0}")]
    FieldNotObject(String),

    /// Field lower than minimum during validation.
    #[error("Field lower than minimum '{0}', {1} < {2}")]
    FieldLowerThanMinimum(String, f64, f64),

    /// Field higher than maximum during validation.
    #[error("Field higher than maximum '{0}', {1} > {2}")]
    FieldHigherThanMaximum(String, f64, f64),

    /// Field not in exact list during validation.
    #[error("Field not in exact list '{0}', {1} > {2}")]
    FieldNotInExactList(String, Value, Value),

    /// Field length lower than minimum during validation.
    #[error("Field length lower than minimum '{0}', {1} < {2}")]
    FieldLengthLowerThanMinimum(String, usize, usize),

    /// Field length higher than maximum during validation.
    #[error("Field length higher than maximum '{0}', {1} > {2}")]
    FieldLengthHigherThanMaximum(String, usize, usize),

    /// Field type incorrect during validation.
    #[error("Field type incorrect '{0}', {1}")]
    FieldTypeIncorrect(String, FieldType),

    /// HTTP invalid method.
    #[error("API invalid method '{0}' != {1}")]
    InvalidMethod(String, String),

    /// HTTP invalid content type.
    #[error("API invalid content_type '{0}' != {1}")]
    InvalidContentType(String, String),
}

/// Standard HTTP response type used by proxy handlers.
pub type ProxyHttpResponse = Result<HttpResponse, Error>;

impl ResponseError for Error {
    /// Map all proxy errors to HTTP 400 Bad Request.
    fn status_code(&self) -> awc::http::StatusCode {
        awc::http::StatusCode::BAD_REQUEST
    }
}
