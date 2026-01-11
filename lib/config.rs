use std::fs;
use std::path::Path;
use std::collections::HashMap;
use serde_json::Value;

use crate::errors::Error;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct Config {
    pub servers:                Vec<ServerConfig>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct ServerConfig {
    pub name:                   String,
    pub listen:                 String,
    #[cfg_attr(feature = "serde", serde(default))]
    pub https:                  bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub redirect_http_to_https: bool,
    pub cert_file:              Option<String>,
    pub key_file:               Option<String>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub mode:                   Mode,
    #[cfg_attr(feature = "serde", serde(default))]
    pub payload_limit:          Option<usize>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub rate_limit:             Option<RateLimitConfig>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub apis:                   Vec<ApiConfig>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct ApiConfig {
    pub path:                   String,
    pub target:                 String,
    #[cfg_attr(feature = "serde", serde(default))]
    pub target_path:            Option<String>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub mode:                   Option<Mode>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub header_rules:           Vec<HeaderRule>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub inject_headers:         HashMap<String, String>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub json_rules:             Vec<JsonRule>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub payload_limit:          Option<usize>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub redirect_http_to_https: Option<bool>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub rate_limit:             Option<RateLimitConfig>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub policy:                 ApiPolicy,
}

#[derive(Debug, Copy, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Mode {
    Raw,
    Rule,
}

impl Default for Mode {
    fn default() -> Self {
        Self::Raw
    }
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct JsonRule {
    pub field:                  String,                 // supports nested fields, e.g., sensor.readings[0].value
    #[cfg_attr(feature = "serde", serde(default))]
    pub min:                    Option<f64>,            // numeric min
    #[cfg_attr(feature = "serde", serde(default))]
    pub max:                    Option<f64>,            // numeric max
    #[cfg_attr(feature = "serde", serde(default))]
    pub exact:                  Option<Vec<Value>>,    // exact match for strings
    #[cfg_attr(feature = "serde", serde(default))]
    pub min_length:             Option<usize>,          // string min length
    #[cfg_attr(feature = "serde", serde(default))]
    pub max_length:             Option<usize>,          // string max length
    #[cfg_attr(feature = "serde", serde(default))]
    pub optional:               bool,                   // optional
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub enum ApiPolicy {
    #[cfg_attr(feature = "serde", serde(alias = "allow"))]
    Allow,
    #[cfg_attr(feature = "serde", serde(alias = "deny"))]
    Block,
    #[cfg_attr(feature = "serde", serde(alias = "drop"))]
    Drop,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct RateLimitConfig {
    #[cfg_attr(feature = "serde", serde(default))]
    pub requests_per_sec:       Option<u64>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub burst:                  Option<u64>,
}

impl Config {
    /// Load configuration from a file (TOML or JSON)
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

impl Default for ApiPolicy {
    fn default() -> Self {
        Self::Allow
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
