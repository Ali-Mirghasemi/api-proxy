use actix_http::{BoxedPayloadStream, Payload};
use actix_http::encoding::Decoder;
use actix_web::{HttpRequest, HttpResponse, HttpResponseBuilder};
use actix_web::http::{Method, StatusCode, header};
use awc::body::MessageBody;
use awc::{Client, ClientResponse, Connector};
use actix_web::web::Bytes;
use awc::http::Uri;
use percent_encoding::{CONTROLS, utf8_percent_encode};
use serde_json::Value;
use std::str::FromStr;
use std::sync::Arc;
use std::usize;

use log::{debug, error};

use crate::config::{ApiConfig, FieldType, JsonRule, Mode, ServerConfig};
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

        let host = target_url.host().unwrap_or_default().to_string();
        let mut client_req = client
            .request(req.method().clone(), target_url)
            .no_decompress();

        // Forward headers
        for (name, value) in req.headers().iter() {
            if name == header::HOST {
                client_req = client_req.insert_header((header::HOST, host.clone()));
            }
            else {
                client_req = client_req.insert_header((name.clone(), value.clone()));
            }
        }

        // Header injection
        for (k, v) in &self.config.inject_headers {
            client_req = client_req.insert_header((k.as_str(), v.as_str()));
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

        Self::build_response(req, res).await
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
            
            field.validate(v)?;
        }


        self.forward_raw(req, body, server).await
    }

    pub fn build_target_url(
        &self,
        req: &HttpRequest,
        server: &ServerConfig,
    ) -> Uri {
        let path = self.config.target_path.as_ref().map(|x| x.as_str()).unwrap_or(req.uri().path());
        let mut url = self.config.target.clone();

        if !url.ends_with("/") {
            url += "/";
        }

        if let Some(x) = &self.config.target_path_prefix {
            url.push_str(&x);
        }

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
        req: HttpRequest,
        mut res: ClientResponse<Decoder<Payload>>,
    ) -> ProxyHttpResponse {
        let status = res.status();

        let mut client_resp = HttpResponseBuilder::new(status);

        debug!("Headers count: {}", res.headers().len());
        for (name, value) in res.headers() {
            client_resp.insert_header((name.clone(), value.clone()));
        }

        match res.body().await {
            Ok(body) => {
                debug!("Payload Len: {}, {}", body.len(), String::from_utf8_lossy(&body[0..20]));
                let response = client_resp.body(body);
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
}
