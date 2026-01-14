use actix_web::{HttpResponse, ResponseError};
use serde_json::Value;

use crate::config::FieldType;


#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO Error {0}")]
    IO(#[from] std::io::Error),

    #[cfg(feature = "toml")]
    #[error("TOML Error {0}")]
    TOML(#[from] toml::de::Error),

    #[cfg(feature = "json")]
    #[error("Json Error {0}")]
    Json(#[from] serde_json::Error),

    #[error("IpAddr Error {0}")]
    IpAddr(#[from] std::net::AddrParseError),

    #[cfg(feature = "tls")]
    #[error("RcGen Error {0}")]
    RcGen(#[from] rcgen::Error),

    #[error("Infallible Error {0}")]
    Infallible(#[from] std::convert::Infallible),
    
    #[error("Cookie Error {0}")]
    Cookie(#[from] actix_web::cookie::ParseError),

    #[error("ParseUtf8 Error {0}")]
    ParseUtf8(#[from] std::string::FromUtf8Error),

    #[error("ToStr Error {0}")]
    ToStr(#[from] actix_http::header::ToStrError),

    #[error("Not Supported")]
    NotSupported,

    #[error("Unknown Error")]
    Unknown,

    #[error("Payload Limit")]
    PayloadLimit,

    #[error("Field not found, {0}")]
    FieldNotFound(String),
    
    #[error("Field not object, {0}")]
    FieldNotObject(String),

    #[error("Field lower than minimum '{0}', {1} < {2}")]
    FieldLowerThanMinimum(String, f64, f64),

    #[error("Field higher than maximum '{0}', {1} > {2}")]
    FieldHigherThanMaximum(String, f64, f64),

    #[error("Field not in exact list '{0}', {1} > {2}")]
    FieldNotInExactList(String, Value, Value),

    #[error("Field length lower than minimum '{0}', {1} < {2}")]
    FieldLengthLowerThanMinimum(String, usize, usize),

    #[error("Field length higher than maximum '{0}', {1} > {2}")]
    FieldLengthHigherThanMaximum(String, usize, usize),

    #[error("Field type incorrect '{0}', {1}")]
    FieldTypeIncorrect(String, FieldType),

    #[error("API invalid method '{0}' != {1}")]
    InvalidMethod(String, String),

    #[error("API invalid content_type '{0}' != {1}")]
    InvalidContentType(String, String),
}

pub type ProxyHttpResponse = Result<HttpResponse, Error>;

impl ResponseError for Error {
    fn status_code(&self) -> awc::http::StatusCode {
        awc::http::StatusCode::BAD_REQUEST
    }
}
