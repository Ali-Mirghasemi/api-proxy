use actix_web::{HttpRequest, HttpResponse};
use actix_web::http::{Method, StatusCode, header};
use awc::Client;
use actix_web::web::Bytes;
use awc::http::Uri;
use percent_encoding::{CONTROLS, utf8_percent_encode};
use serde_json::Value;
use std::sync::Arc;
use std::usize;

use log::{debug, error};

use crate::config::{ServerConfig, ApiConfig, Mode};
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
            .finish()
    }

    /// Entry point called from server.rs
    pub async fn forward(
        &self,
        req: HttpRequest,
        body: Bytes,
        server: Arc<ServerConfig>,
    ) -> ProxyHttpResponse {
        match self.config.mode.unwrap_or_default() {
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

        let mut client_req = client
            .request(req.method().clone(), target_url);

        // Forward headers
        for (name, value) in req.headers().iter() {
            if name == header::HOST {
                continue;
            }
            client_req = client_req.insert_header((name.clone(), value.clone()));
        }

        // Header injection
        for (k, v) in &self.config.inject_headers {
            client_req = client_req.insert_header((k.as_str(), v.as_str()));
        }

        let res = match client_req.send_body(body).await {
            Ok(r) => r,
            Err(e) => {
                error!("Upstream request failed: {}", e);
                return Ok(HttpResponse::BadGateway().finish());
            }
        };

        Self::build_response(res).await
    }

    pub async fn forward_rule(
        &self,
        req: HttpRequest,
        body: Bytes,
        server: Arc<ServerConfig>,
    ) -> ProxyHttpResponse {
        // 🔒 Placeholder: JSON / header validation will live here
        debug!("RULE proxy validation passed");
        // Check payload limit
        if let Some(payload_limit) = self.config.payload_limit {
            if body.len() >= payload_limit {
                return Err(Error::PayloadLimit);
            }
        }

        let value: Value = serde_json::from_slice(&body)?;
        // Check Rules per fields
        for field in &self.config.json_rules {
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
            // Check exact values
            if let Some(exact) = &field.exact {
                for e in exact {
                    if v != e {
                        return Err(Error::FieldNotInExactList(field_name, v.clone(), e.clone()));
                    }
                }
            }
            // Check field as number
            if let Some(x) = v.as_f64() {
                // Check minimum
                if let Some(min) = field.min {
                    if x < min {
                        return Err(Error::FieldLowerThanMinimum(field_name, x, min));
                    }   
                }
                // Check maximum
                if let Some(max) = field.max {
                    if x > max {
                        return Err(Error::FieldHigherThanMaximum(field_name, x, max));
                    }
                }
            }
            // Check string length
            if let Some(x) = v.as_str() {
                // Check minimum
                if let Some(min) = field.min_length {
                    if x.chars().count() < min {
                        return Err(Error::FieldLengthLowerThanMinimum(field_name, x.chars().count(), min));
                    }   
                }
                // Check maximum
                if let Some(max) = field.max_length {
                    if x.chars().count() > max {
                        return Err(Error::FieldLengthHigherThanMaximum(field_name, x.chars().count(), max));
                    }
                }
            }
            // Check array length
            if let Some(x) = v.as_array() {
                debug!("{field_name} It's array");
                // Check minimum
                if let Some(min) = field.min_length {
                    if x.len() < min {
                        return Err(Error::FieldLengthLowerThanMinimum(field_name, x.len(), min));
                    }   
                }
                // Check maximum
                if let Some(max) = field.max_length {
                    if x.len() > max {
                        return Err(Error::FieldLengthHigherThanMaximum(field_name, x.len(), max));
                    }
                }
            }
        }


        self.forward_raw(req, body, server).await
    }

    pub fn build_target_url(
        &self,
        req: &HttpRequest,
        server: &ServerConfig,
    ) -> String {
        let path = self.config.target_path.as_ref().map(|x| x.as_str()).unwrap_or(req.uri().path());
        let mut url = self.config.target.clone();

        url.push_str(&path);

        if let Some(q) = req.uri().query() {
            url.push_str("?");
            url.push_str(q);
        }

        utf8_percent_encode(&url, CONTROLS).to_string()
    }

    pub async fn build_response(
        mut res: awc::ClientResponse,
    ) -> ProxyHttpResponse {
        let status = res.status();

        let mut client_resp = HttpResponse::build(status);

        for (name, value) in res.headers().iter() {
            client_resp.append_header((name.clone(), value.clone()));
        }

        match res.body().await {
            Ok(body) => Ok(client_resp.body(body)),
            Err(_) => Ok(HttpResponse::BadGateway().finish()),
        }
    }
}

