use std::fmt::Display;
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
    pub target_path_prefix:     Option<String>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub mode:                   Mode,
    #[cfg_attr(feature = "serde", serde(default))]
    pub content_type:           Option<String>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub method:                 Option<String>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub header_rules:           Vec<HeaderRule>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub inject_headers:         HashMap<String, String>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub inject_cookies:         HashMap<String, String>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub rules:                  Vec<Rule>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub payload_limit:          Option<usize>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub redirect_http_to_https: Option<bool>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub rate_limit:             Option<RateLimitConfig>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub policy:                 ApiPolicy,
    #[cfg_attr(feature = "serde", serde(default))]
    pub no_decompress:          bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub no_forward_headers:     bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub no_forward_cookies:     bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub replace_html_links:     bool,
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
pub struct Rule {
    #[cfg_attr(feature = "serde", serde(alias = "name"))]
    pub field:                  String,                 // supports nested fields, e.g., sensor.readings[0].value
    #[cfg_attr(feature = "serde", serde(default))]
    #[cfg_attr(feature = "serde", serde(alias = "type"))]
    pub field_type:             Option<FieldType>,      // supports nested fields, e.g., sensor.readings[0].value
    #[cfg_attr(feature = "serde", serde(default))]
    pub min:                    Option<f64>,            // numeric min
    #[cfg_attr(feature = "serde", serde(default))]
    pub max:                    Option<f64>,            // numeric max
    #[cfg_attr(feature = "serde", serde(default))]
    pub exact:                  Option<Vec<Value>>,    // exact match for strings
    #[cfg_attr(feature = "serde", serde(default))]
    pub min_string_length:      Option<usize>,          // string min length
    #[cfg_attr(feature = "serde", serde(default))]
    pub max_string_length:      Option<usize>,          // string max length
    #[cfg_attr(feature = "serde", serde(default))]
    pub min_array_length:       Option<usize>,          // array min length
    #[cfg_attr(feature = "serde", serde(default))]
    pub max_array_length:       Option<usize>,          // array max length
    #[cfg_attr(feature = "serde", serde(default))]
    pub optional:               bool,                   // optional
}

#[derive(Debug, Copy, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub enum ApiPolicy {
    #[cfg_attr(feature = "serde", serde(alias = "allow"))]
    Allow,
    #[cfg_attr(feature = "serde", serde(alias = "deny"))]
    Block,
    #[cfg_attr(feature = "serde", serde(alias = "drop"))]
    Drop,
}

#[derive(Debug, Copy, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct RateLimitConfig {
    #[cfg_attr(feature = "serde", serde(default))]
    pub requests_per_sec:       Option<u64>,
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

impl Rule {
    pub fn validate(&self, v: &Value) -> Result<(), Error> {
        self.validate_rule(v, false)
    }

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
