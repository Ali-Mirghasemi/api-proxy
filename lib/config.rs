//! Configuration models and payload validation logic for the API proxy.
//!
//! This module defines all configuration structures required to:
//! - Configure one or more proxy servers
//! - Define API routing rules
//! - Validate JSON payloads
//! - Apply header, method, and content-type restrictions
//! - Enforce rate limits and payload size limits
//!
//! Configuration can be loaded from **TOML** or **JSON** depending on enabled
//! Cargo features.
//!
//! # Features
//! - `toml`  — Enable TOML config loading
//! - `json`  — Enable JSON config loading
//! - `serde` — Enable deserialization via Serde

use std::fmt::Display;
use std::fs;
use std::path::Path;
use std::collections::HashMap;
use actix_http::Payload;
use actix_http::encoding::Decoder;
use actix_web::HttpRequest;
use awc::ClientResponse;
use futures::future::BoxFuture;
use serde_json::Value;

use crate::errors::Error;

/// Root configuration object.
///
/// A configuration contains one or more proxy servers, each with
/// independent listeners, TLS settings, and API routing rules.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct Config {
    /// List of configured proxy servers.
    pub servers:                Vec<ServerConfig>,
}

/// Configuration for a single proxy server.
///
/// Each server listens on a socket address and may expose
/// multiple API routes.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct ServerConfig {
    /// Human-readable server name.
    pub name:                   String,
    /// Address to bind to, e.g. `0.0.0.0:8080`.
    pub listen:                 String,
    /// Enable HTTPS listener.
    #[cfg_attr(feature = "serde", serde(default))]
    pub https:                  bool,
    /// Redirect incoming HTTP traffic to HTTPS.
    #[cfg_attr(feature = "serde", serde(default))]
    pub redirect_http_to_https: bool,
    /// TLS certificate file path.
    pub cert_file:              Option<String>,
    /// TLS private key file path.
    pub key_file:               Option<String>,
    /// Maximum allowed request payload size (bytes).
    #[cfg_attr(feature = "serde", serde(default))]
    pub payload_limit:          Option<usize>,
    /// Optional rate limiting configuration.
    #[cfg_attr(feature = "serde", serde(default))]
    pub rate_limit:             Option<RateLimitConfig>,
    /// API route definitions handled by this server.
    #[cfg_attr(feature = "serde", serde(default))]
    pub apis:                   Vec<ApiConfig>,
}

/// Configuration for a single proxied API endpoint.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct ApiConfig {
    /// Incoming request path to match.
    pub path:                   String,
    /// Target upstream URL.
    pub target:                 String,
    /// Optional override for forwarded path.
    #[cfg_attr(feature = "serde", serde(default))]
    pub target_path:            Option<String>,
    /// Optional path prefix stripping or rewriting.
    #[cfg_attr(feature = "serde", serde(default))]
    pub path_prefix:     Option<String>,
    /// API operation mode.
    #[cfg_attr(feature = "serde", serde(default))]
    pub mode:                   Mode,
    /// Required `Content-Type` for incoming requests.
    #[cfg_attr(feature = "serde", serde(default))]
    pub content_type:           Option<String>,
    /// Required HTTP method (e.g. `POST`, `GET`).
    #[cfg_attr(feature = "serde", serde(default))]
    pub method:                 Option<String>,
    /// Header filtering rules.
    #[cfg_attr(feature = "serde", serde(default))]
    pub header_rules:           Vec<HeaderRule>,
    /// Headers injected into forwarded requests.
    #[cfg_attr(feature = "serde", serde(default))]
    pub inject_headers:         HashMap<String, String>,
    /// Cookies injected into forwarded requests.
    #[cfg_attr(feature = "serde", serde(default))]
    pub inject_cookies:         HashMap<String, String>,
    /// Payload validation rules.
    #[cfg_attr(feature = "serde", serde(default))]
    pub rules:                  Vec<Rule>,
    /// Per-API payload size limit.
    #[cfg_attr(feature = "serde", serde(default))]
    pub payload_limit:          Option<usize>,
    /// Override server-level HTTP→HTTPS redirect behavior.
    #[cfg_attr(feature = "serde", serde(default))]
    pub redirect_http_to_https: Option<bool>,
    /// Per-API rate limiting configuration.
    #[cfg_attr(feature = "serde", serde(default))]
    pub rate_limit:             Option<RateLimitConfig>,
    /// Default action when validation fails.
    #[cfg_attr(feature = "serde", serde(default))]
    pub policy:                 ApiPolicy,
    /// Disable response decompression.
    #[cfg_attr(feature = "serde", serde(default))]
    pub no_decompress:          bool,
    /// Do not forward client headers upstream.
    #[cfg_attr(feature = "serde", serde(default))]
    pub no_forward_headers:     bool,
    /// Do not forward client cookies upstream.
    #[cfg_attr(feature = "serde", serde(default))]
    pub no_forward_cookies:     bool,
    /// Rewrite absolute links in HTML responses.
    #[cfg_attr(feature = "serde", serde(default))]
    pub replace_html_links:     bool,
    /// Include unmatched request tail in forwarded path.
    #[cfg_attr(feature = "serde", serde(default))]
    pub include_tail:           bool,
    /// Preserve original proxy path when forwarding.
    #[cfg_attr(feature = "serde", serde(default))]
    pub keep_proxy_path:        bool,
    /// Hook function called before processing each API request.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub hook_request:           Option<ApiHookRequestFn>,
    /// Hook function called before sending each API response.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub hook_response:          Option<ApiHookResponseFn>,
}

/// Type alias for API hook functions.
pub type ApiHookRequestFn = fn(&ApiConfig, &mut HttpRequest, &mut actix_web::web::Payload) -> BoxFuture<'static, Result<(), Error>>;

/// Type alias for API hook functions.
pub type ApiHookResponseFn = fn(&ApiConfig, &mut ClientResponse<Decoder<Payload>>) -> BoxFuture<'static, Result<(), Error>>;

/// API processing mode.
#[derive(Debug, Copy, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Mode {
    /// Raw proxying with minimal processing.
    Raw,
    /// Apply validation rules and policies.
    Rule,
}

impl Default for Mode {
    fn default() -> Self {
        Self::Raw
    }
}

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct HeaderRule {
    pub name:                   String,
    #[cfg_attr(feature = "serde", serde(rename = "action"))]
    pub action:                 HeaderAction, // block, drop, allow
}

#[derive(Debug, Copy, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum HeaderAction {
    Block,
    Drop,
    Allow,
}

impl Default for HeaderAction {
    fn default() -> Self {
        Self::Allow
    }
}

/// Payload validation rule.
///
/// Rules are applied to JSON fields and support nested
/// paths such as `sensor.readings[0].value`.
#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct Rule {
    /// Field name or JSON path.
    #[cfg_attr(feature = "serde", serde(alias = "name"))]
    pub field:                  String,                 // supports nested fields, e.g., sensor.readings[0].value
    /// Expected field type.
    #[cfg_attr(feature = "serde", serde(default))]
    #[cfg_attr(feature = "serde", serde(alias = "type"))]
    pub field_type:             Option<FieldType>,      // supports nested fields, e.g., sensor.readings[0].value
    /// Numeric minimum value.
    #[cfg_attr(feature = "serde", serde(default))]
    pub min:                    Option<f64>,            // numeric min
    /// Numeric maximum value.
    #[cfg_attr(feature = "serde", serde(default))]
    pub max:                    Option<f64>,            // numeric max
    /// Exact allowed values.
    #[cfg_attr(feature = "serde", serde(default))]
    pub exact:                  Option<Vec<Value>>,    // exact match for strings
    /// Minimum string length.
    #[cfg_attr(feature = "serde", serde(default))]
    pub min_string_length:      Option<usize>,          // string min length
    /// Maximum string length.
    #[cfg_attr(feature = "serde", serde(default))]
    pub max_string_length:      Option<usize>,          // string max length
    /// Minimum array length.
    #[cfg_attr(feature = "serde", serde(default))]
    pub min_array_length:       Option<usize>,          // array min length
    /// Maximum array length.
    #[cfg_attr(feature = "serde", serde(default))]
    pub max_array_length:       Option<usize>,          // array max length
    /// Allow field to be missing.
    #[cfg_attr(feature = "serde", serde(default))]
    pub optional:               bool,                   // optional
}

/// Default action applied when an API validation fails.
#[derive(Debug, Copy, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub enum ApiPolicy {
    /// Allow the request despite validation errors.
    #[cfg_attr(feature = "serde", serde(alias = "allow"))]
    Allow,
    /// Reject the request with an error response.
    #[cfg_attr(feature = "serde", serde(alias = "deny"))]
    Block,
    /// Silently drop the request.
    #[cfg_attr(feature = "serde", serde(alias = "drop"))]
    Drop,
}

/// Rate limiting configuration.
#[derive(Debug, Default, Copy, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct RateLimitConfig {
    /// Allowed requests per second.
    #[cfg_attr(feature = "serde", serde(default))]
    pub requests_per_sec:       Option<u64>,
    /// Maximum burst size.
    #[cfg_attr(feature = "serde", serde(default))]
    pub burst:                  Option<u64>,
}

#[derive(Debug, Copy, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub enum FieldType {
    Null,
    Bool,
    Number,
    String,
    Array,
    Object,
}

impl Default for FieldType {
    fn default() -> Self {
        Self::String
    }
}

impl Config {
    /// Load configuration from a file.
    ///
    /// The format is automatically detected based on file extension
    /// (`.toml` or `.json`). If no extension is present, the loader will
    /// attempt to parse TOML first and then JSON (depending on enabled features).
    ///
    /// # Errors
    /// Returns an error if:
    /// - The file cannot be read
    /// - The format is not supported
    /// - Deserialization fails
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let path = path.as_ref();
        let data = fs::read_to_string(path)?;
        let ext = path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        let config = match ext.as_str() {
            #[cfg(feature = "toml")]
            "toml" => toml::from_str::<Config>(&data)?,
            #[cfg(feature = "json")]
            "json" => serde_json::from_str::<Config>(&data)?,
            _ => {
                // Try auto-detect: first TOML, then JSON
                #[cfg(all(feature = "toml", feature = "json"))]
                {
                    toml::from_str::<Config>(&data)
                        .or_else(|_| serde_json::from_str::<Config>(&data))?
                }
                
                #[cfg(all(feature = "toml", not(feature = "json")))]
                {
                    toml::from_str::<Config>(&data)?
                }

                #[cfg(all(feature = "json", not(feature = "toml")))]
                {
                    serde_json::from_str::<Config>(&data)?
                }
                
                #[cfg(all(not(feature = "toml"), not(feature = "json")))]
                {
                    Err(Error::NotSupported)?
                }
            }
        };

        Ok(config)
    }

}

impl Rule {
    /// Validate a JSON value against this rule.
    pub fn validate(&self, v: &Value) -> Result<(), Error> {
        self.validate_rule(v, false)
    }

    /// Internal validation entry point.
    pub fn validate_rule(&self, v: &Value, is_inner: bool) -> Result<(), Error> {
        // Check field type
        if !is_inner {
            if let Some(x) = self.field_type {
                self.validate_type(v, x)?;
            }
        }
        // Check exact values
        if let Some(exact) = &self.exact {
           self.validate_exact(exact, v)?;
        }
        // Check field as number
        if let Some(x) = v.as_f64() {
            self.validate_number(x)?;
        }
        
        // Check field as string
        if let Some(x) = v.as_str() {
            self.validate_string(x)?;
        }

        // Check array length
        if let Some(x) = v.as_array() {
            self.validate_array(x)?;
        }

        Ok(())
    }

    pub fn validate_type(&self, v: &Value, x: FieldType) -> Result<(), Error> {
        let field_name = self.field.clone();

        if !match x {
            FieldType::Null => v.is_null(),
            FieldType::Bool => v.is_boolean(),
            FieldType::Number => v.is_number(),
            FieldType::String => v.is_string(),
            FieldType::Array => v.is_array(),
            FieldType::Object => v.is_object(),
        } {
            return Err(Error::FieldTypeIncorrect(field_name, x));
        }

        Ok(())
    }

    pub fn validate_string(&self, v: &str) -> Result<(), Error> {
        let field_name = self.field.clone();
        let str_len = v.chars().count();
        // Check minimum
        if let Some(min) = self.min_string_length {
            if str_len < min {
                return Err(Error::FieldLengthLowerThanMinimum(field_name, str_len, min));
            }   
        }
        // Check maximum
        if let Some(max) = self.max_string_length {
            if str_len > max {
                return Err(Error::FieldLengthHigherThanMaximum(field_name, str_len, max));
            }
        }

        Ok(())
    }

    pub fn validate_number(&self, x: f64) -> Result<(), Error> {
        let field_name = self.field.clone();
        // Check minimum
        if let Some(min) = self.min {
            if x < min {
                return Err(Error::FieldLowerThanMinimum(field_name, x, min));
            }   
        }
        // Check maximum
        if let Some(max) = self.max {
            if x > max {
                return Err(Error::FieldHigherThanMaximum(field_name, x, max));
            }
        }

        Ok(())
    }

    pub fn validate_exact(&self, exact: &Vec<Value>, v: &Value) -> Result<(), Error> {
        let field_name = self.field.clone();
        for e in exact {
            if v != e {
                return Err(Error::FieldNotInExactList(field_name, v.clone(), e.clone()));
            }
        }

        Ok(())
    }

    pub fn validate_array(&self, v: &Vec<Value>) -> Result<(), Error> {
        let field_name = self.field.clone();
        // Check minimum length
        if let Some(min) = self.min_array_length {
            if v.len() < min {
                return Err(Error::FieldLengthLowerThanMinimum(field_name, v.len(), min));
            }   
        }
        // Check maximum length
        if let Some(max) = self.max_array_length {
            if v.len() > max {
                return Err(Error::FieldLengthHigherThanMaximum(field_name, v.len(), max));
            }
        }
        // Check array values
        for x in v {
            self.validate(x)?;
        }

        Ok(())
    }
}

impl Default for ApiPolicy {
    fn default() -> Self {
        Self::Allow
    }
}

impl Display for FieldType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "toml")]
    fn load_toml_config() {
        let toml_data = r#"
        [[servers]]
        name = "test"
        listen = "0.0.0.0:8080"
        https = true
        [[servers.apis]]
        path = "/api/v1"
        target = "http://example.com"
        "#;

        let config: Config = toml::from_str(toml_data).unwrap();
        assert_eq!(config.servers.len(), 1);
        assert_eq!(config.servers[0].apis.len(), 1);
    }

    #[test]
    #[cfg(feature = "json")]
    fn load_json_config() {
        let json_data = r#"
        {
            "servers": [
                {
                    "name": "test",
                    "listen": "0.0.0.0:8080",
                    "https": true,
                    "apis": [
                        {"path": "/api/v1", "target": "http://example.com"}
                    ]
                }
            ]
        }
        "#;

        let config: Config = serde_json::from_str(json_data).unwrap();
        assert_eq!(config.servers.len(), 1);
        assert_eq!(config.servers[0].apis.len(), 1);
    }
}
