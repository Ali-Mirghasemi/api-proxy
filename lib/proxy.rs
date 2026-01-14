use actix_http::header::{CONTENT_ENCODING, CONTENT_TYPE};
use actix_http::{BoxedPayloadStream, Payload};
use actix_http::encoding::Decoder;
use actix_web::dev::WebService;
use actix_web::guard::{self, Guard};
use actix_web::{HttpRequest, HttpResponse, HttpResponseBuilder};
use actix_web::http::{Method, StatusCode, header};
use awc::body::MessageBody;
use awc::cookie::Cookie;
use awc::{Client, ClientResponse, Connector};
use actix_web::web::Bytes;
use awc::http::Uri;
use percent_encoding::{CONTROLS, utf8_percent_encode};
use regex::Regex;
use serde_json::Value;
use std::str::FromStr;
use std::sync::Arc;
use std::usize;

use log::{debug, error};

use crate::config::{ApiConfig, FieldType, Rule, Mode, ServerConfig};
use crate::errors::{Error, ProxyHttpResponse};

pub struct Proxy {
    config:             ApiConfig,
}

impl Proxy {

    pub fn new(config: ApiConfig) -> Self {
        Self {
            config,
        }
    }

    /// Shared HTTP client
    pub fn http_client() -> Client {
        Client::builder()
            .disable_redirects()
            .no_default_headers()
            .finish()
    }

    /// Entry point called from server.rs
    pub async fn forward(
        &self,
        req: HttpRequest,
        body: Bytes,
        server: Arc<ServerConfig>,
    ) -> ProxyHttpResponse {
        match self.config.mode {
            Mode::Raw => self.forward_raw(req, body, server).await,
            Mode::Rule => self.forward_rule(req, body, server).await,
        }
    }

    pub async fn forward_raw(
        &self,
        req: HttpRequest,
        body: Bytes,
        server: Arc<ServerConfig>,
    ) -> ProxyHttpResponse {
        let client = Self::http_client();

        let target_url = self.build_target_url(&req, &server);
        debug!("RAW proxy → {}", target_url);

        // Check payload limit
        if let Some(payload_limit) = self.config.payload_limit {
            if body.len() >= payload_limit {
                return Err(Error::PayloadLimit);
            }
        }

        // Check Method
        if let Some(method) = &self.config.method {
            if req.method().as_str() != method.to_uppercase() {
                return Err(Error::InvalidMethod(req.method().to_string(), method.clone()));
            }
        }

        // Check ContentType
        if let Some(content_type) = &self.config.content_type {
            if let Some(x) =  req.headers().get(CONTENT_TYPE) {
                let ct = x.to_str()?.to_string();
                if ct != *content_type {
                    return Err(Error::InvalidContentType(ct, content_type.clone()));
                }
            }
        }

        let host = target_url.host().unwrap_or_default().to_string();
        let mut client_req = client
            .request(req.method().clone(), target_url);

        if self.config.no_decompress {
            client_req = client_req.no_decompress();
        }

        // Forward headers
        if !self.config.no_forward_headers {
            for (name, value) in req.headers().iter() {
                if name == header::HOST {
                    client_req = client_req.insert_header((header::HOST, host.clone()));
                }
                else {
                    client_req = client_req.insert_header((name.clone(), value.clone()));
                }
            }
        }

        // Header injection
        for (k, v) in &self.config.inject_headers {
            client_req = client_req.insert_header((k.as_str(), v.as_str()));
        }

        // Forward cookies
        if !self.config.no_forward_cookies {
            for x in req.cookies()?.iter() {
                client_req = client_req.cookie(x.clone());
            }
        }

        // Cookie injection
        for (k, v) in &self.config.inject_cookies {
            client_req = client_req.cookie(Cookie::new(k.as_str(), v.as_str()));
        }

        debug!("{client_req:?}");
        let res: ClientResponse<Decoder<Payload>> = match client_req.send_body(body).await {
            Ok(r) => r,
            Err(e) => {
                error!("Upstream request failed: {}", e);
                return Ok(HttpResponse::BadGateway().finish());
            }
        };

        debug!("{res:?}");

        self.build_response(req, res).await
    }

    pub async fn forward_rule(
        &self,
        req: HttpRequest,
        body: Bytes,
        server: Arc<ServerConfig>,
    ) -> ProxyHttpResponse {
        let value: Value = serde_json::from_slice(&body)?;
        // Check Rules per fields
        for field in &self.config.rules {
            let field_name = field.field.clone();
            let v: &Value = if value.is_object() {
                let mut v = Some(&value);
                // Handle nested field
                if field.field.contains(".") {
                    for n in field.field.split(".") {
                        v = v.map(|x| x.as_object().map(|x| x.get(n)).flatten()).flatten();

                        if !v.is_some() {
                            break;
                        }
                    }
                }
                else {
                    v = v.map(|x| x.as_object().map(|x| x.get(&field_name)).flatten()).flatten();
                }

                if let Some(x) = v {
                    x
                }
                else {
                    if field.optional {
                        continue;
                    }
                    else {
                        return Err(Error::FieldNotObject(field.field.clone()));
                    }
                }
            }
            else {
                &value
            };
            
            field.validate(v)?;
        }

        // 🔒 Placeholder: JSON / header validation will live here
        debug!("RULE proxy validation passed");

        self.forward_raw(req, body, server).await
    }

    pub fn build_target_url(
        &self,
        req: &HttpRequest,
        server: &ServerConfig,
    ) -> Uri {
        let mut path = req.path();
        let mut url = self.config.target.clone();

        // Remove proxy path
        if !self.config.keep_proxy_path {
            path = path.trim_start_matches(&self.config.path);
        }

        if !url.ends_with("/") {
            url += "/";
        }

        // Add target path
        if let Some(x) = &self.config.target_path {
            url.push_str(&x);
        }

        // Add path prefix
        if let Some(x) = &self.config.path_prefix {
            url.push_str(&x);
        }

        // Add path
        if url.ends_with("/") && path.starts_with("/") {
            url.pop();
        }

        url.push_str(&path);

        if let Some(q) = req.uri().query() {
            url.push_str("?");
            url.push_str(q);
        }

        Uri::from_str(utf8_percent_encode(&url, CONTROLS).to_string().as_str()).unwrap()

    }

    pub async fn build_response(
        &self,
        req: HttpRequest,
        mut res: ClientResponse<Decoder<Payload>>,
    ) -> ProxyHttpResponse {
        let status = res.status();

        let mut client_resp = HttpResponseBuilder::new(status);

        debug!("Headers count: {}", res.headers().len());
        for (name, value) in res.headers() {
            if !self.config.no_decompress && name == CONTENT_ENCODING {
                continue;
            }

            client_resp.insert_header((name.clone(), value.clone()));
        }

        match res.body().await {
            Ok(body) => {
                debug!("Payload Len: {}, {}", body.len(), String::from_utf8_lossy(&body[0..20]));
                 // Check if content type is HTML
                let is_html = res.headers()
                    .get(header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .map(|v| v.contains("text/html"))
                    .unwrap_or(false);

                let final_body = if is_html && self.config.replace_html_links {
                    let body_str = String::from_utf8(body.to_vec())?;
                    Bytes::copy_from_slice(Self::replace_all_refs(&body_str, "").as_bytes())
                } 
                else {
                    body
                };
                let response = client_resp.body(final_body);
                debug!("{response:?}");
                debug!("Headers count: {}", response.headers().len());
                Ok(response)
            },
            Err(e) => {
                error!("{e}");
                Ok(HttpResponse::BadGateway().finish())
            },
        }
    }

    pub fn replace_all_refs(html: &str, prefix: &str) -> String {
        // Matches href, src, or data-href attributes
        // Group 1: Attribute name
        // Group 2: The value inside quotes
        let re = Regex::new(r#"(href|src|data-href)=(["'])([^"']*)\2"#).unwrap();

        re.replace_all(html, |caps: &regex::Captures| {
            let attr_name = &caps[1];
            let quote = &caps[2];
            let value = &caps[3];

            // Logic to decide replacement
            let new_value = if value.starts_with("/") {
                format!("{}{}", prefix, value)
            } else {
                value.to_string()
            };

            format!("{}={}{}{}", attr_name, quote, new_value, quote)
        }).to_string()
    }
}
