//! HTTP request proxying, validation, and response handling.
//!
//! This module implements the core proxy logic:
//! - Forwarding incoming HTTP requests to upstream targets
//! - Optional payload validation (JSON rules)
//! - Header and cookie forwarding / injection
//! - Payload size limits
//! - Method and Content-Type validation
//! - Response rewriting (HTML link replacement)
//!
//! The proxy operates in two modes:
//! - [`Mode::Raw`]  — Simple forwarding with minimal checks
//! - [`Mode::Rule`] — Payload validation before forwarding
//!
//! This module is designed to be used by the HTTP server layer
//! (see [`crate::server`]).

use actix_http::header::{CONTENT_ENCODING, CONTENT_TYPE};
use actix_http::{BoxedPayloadStream, Payload};
use actix_http::encoding::Decoder;
use actix_web::dev::WebService;
use actix_web::guard::{self, Guard};
use actix_web::{HttpMessage, HttpRequest, HttpResponse, HttpResponseBuilder, web};
use actix_web::http::{Method, StatusCode, header};
use actix_ws::Message;
use awc::body::MessageBody;
use awc::cookie::Cookie;
use awc::{Client, ClientResponse, Connector};
use actix_web::web::{Bytes, BytesMut};
use awc::http::Uri;
use bytestring::ByteString;
use futures::{SinkExt, StreamExt};
use percent_encoding::{CONTROLS, utf8_percent_encode};
use regex::Regex;
use scraper::{Html, Selector};
use serde_json::Value;
use url::Url;
use std::str::FromStr;
use std::sync::Arc;
use std::usize;

use log::{debug, error};

use crate::config::{ApiConfig, FieldType, Rule, Mode, ServerConfig};
use crate::errors::{Error, ProxyHttpResponse};

/// API proxy instance.
///
/// A `Proxy` represents a single API endpoint configuration
/// and is responsible for validating and forwarding requests
/// according to its [`ApiConfig`].
#[derive(Debug, Clone)]
pub struct Proxy {
    /// API-specific configuration.
    config:             ApiConfig,
}

impl Proxy {
    /// Create a new proxy instance for an API configuration.
    pub fn new(config: ApiConfig) -> Self {
        Self {
            config,
        }
    }

    /// Create a shared HTTP client for forwarding requests upstream.
    ///
    /// The client:
    /// - Disables automatic redirects
    /// - Does not inject default headers
    ///
    /// This ensures full control over forwarded requests.
    pub fn http_client() -> Client {
        Client::builder()
            .disable_redirects()
            .no_default_headers()
            .finish()
    }

    pub fn is_websocket(req: &HttpRequest) -> bool {
        req.headers()
            .get("upgrade")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.eq_ignore_ascii_case("websocket"))
            .unwrap_or(false)
    }

    /// Entry point for handling an incoming request.
    ///
    /// Dispatches the request to the appropriate forwarding
    /// implementation based on the configured [`Mode`].
    ///
    /// # Modes
    /// - [`Mode::Raw`]  → [`Self::forward_raw`]
    /// - [`Mode::Rule`] → [`Self::forward_rule`]
    pub async fn forward(
        &self,
        mut req: HttpRequest,
        mut payload: web::Payload,
        server: Arc<ServerConfig>,
    ) -> ProxyHttpResponse {
        // Hook api request
        if let Some(hook) = self.config.hook_request {
            if let Err(e) = hook(&self.config, &mut req, &mut payload).await {
                error!("API request hook failed: {}", e);
                return Ok(HttpResponse::InternalServerError().finish());
            }
        }

        if Self::is_websocket(&req) {
            return self.ws_proxy(req, payload).await.map_err(Error::from);
        }

        let body = payload.to_bytes().await?;

        match self.config.mode {
            Mode::Raw => self.forward_raw(req, body, server).await,
            Mode::Rule => self.forward_rule(req, body, server).await,
        }
    }

    pub async fn payload_to_bytes(
       mut payload: web::Payload,
        limit: usize,
    ) -> Result<Bytes, actix_web::Error> {

        let mut body = BytesMut::new();

        while let Some(chunk) = payload.next().await {
            let chunk = chunk?;

            if body.len() + chunk.len() > limit {
                return Err(actix_web::error::ErrorPayloadTooLarge("payload too large"));
            }

            body.extend_from_slice(&chunk);
        }

        Ok(body.freeze())
    }

    /// Forward a request without payload validation.
    ///
    /// This mode performs:
    /// - Payload size checks
    /// - HTTP method validation
    /// - Content-Type validation
    /// - Header and cookie forwarding
    /// - Header and cookie injection
    /// - Response forwarding and optional rewriting
    ///
    /// No JSON validation is applied in this mode.
    pub async fn forward_raw(
        &self,
        req: HttpRequest,
        body: Bytes,
        server: Arc<ServerConfig>,
    ) -> ProxyHttpResponse {
        let client = Self::http_client();

        let target_url = self.build_target_url(&req);
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
            .request(req.method().clone(), target_url)
            .camel_case();

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
        let mut res: ClientResponse<Decoder<Payload>> = match client_req.send_body(body).await {
            Ok(r) => r,
            Err(e) => {
                error!("Upstream request failed: {}", e);
                return Ok(HttpResponse::BadGateway().finish());
            }
        };

        // Hook api response
        if let Some(hook) = self.config.hook_response {
            if let Err(e) = hook(&self.config, &mut res).await {
                error!("API response hook failed: {}", e);
                return Ok(HttpResponse::InternalServerError().finish());
            }
        }

        debug!("{res:?}");

        self.build_response(req, res).await
    }

    /// Forward a request after validating JSON payload rules.
    ///
    /// The request body is parsed as JSON and validated
    /// against all configured [`Rule`] definitions.
    ///
    /// If validation passes, the request is forwarded using
    /// [`Self::forward_raw`].
    ///
    /// # Errors
    /// Returns an error if:
    /// - JSON parsing fails
    /// - A required field is missing
    /// - A validation rule fails
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

    /// Build the upstream target URL for a request.
    ///
    /// This function:
    /// - Optionally removes the proxy path
    /// - Appends configured target paths and prefixes
    /// - Preserves query parameters
    /// - Percent-encodes the final URL
    pub fn build_target_url(
        &self,
        req: &HttpRequest
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

    /// Build the client-facing response from an upstream response.
    ///
    /// This function:
    /// - Copies upstream status code
    /// - Forwards headers (with optional decompression handling)
    /// - Optionally rewrites HTML links
    /// - Returns a streaming-safe response body
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

        match res.body().limit(self.config.payload_limit.unwrap_or(2 * 1024 * 1024)).await {
            Ok(body) => {
                debug!("Payload Len: {}, {}", body.len(), String::from_utf8_lossy(&body[0..body.len().min(20)]));
                 // Check if content type is HTML
                let is_html = res.headers()
                    .get(header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .map(|v| v.contains("text/html"))
                    .unwrap_or(false);

                let final_body = if is_html && self.config.replace_html_links {
                    let body_str = String::from_utf8(body.to_vec())?;
                    Bytes::copy_from_slice(Self::rewrite_response(req.content_type(), &body_str, "", &self.config.target).as_bytes())
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

    /// Replace relative references in HTML content.
    ///
    /// This function rewrites attributes:
    /// - `href`
    /// - `src`
    /// - `data-href`
    ///
    /// Only relative URLs (`/...`) are modified.
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

    /// Rewrites all links inside HTML, JS and CSS content
    pub fn rewrite_response(
        content_type: &str,
        body: &str,
        proxy_prefix: &str,
        base_url: &str,
    ) -> String {
        if content_type.contains("text/html") {
            Self::rewrite_html(body, proxy_prefix, base_url)
        } 
        else if content_type.contains("javascript") {
            Self::rewrite_js(body, proxy_prefix)
        } 
        else if content_type.contains("text/css") {
            Self::rewrite_css(body, proxy_prefix)
        } 
        else {
            body.to_string()
        }
    }

    fn rewrite_html(html: &str, prefix: &str, base: &str) -> String {
        let document = Html::parse_document(html);

        let attrs = [
            "href", "src", "action", "data", "poster"
        ];

        let selector = Selector::parse("*").unwrap();

        let mut output = html.to_string();

        for element in document.select(&selector) {
            for attr in &attrs {
                if let Some(value) = element.value().attr(attr) {
                    if let Some(new_url) = Self::rewrite_url(value, prefix, base) {
                        output = output.replace(value, &new_url);
                    }
                }
            }
        }

        // Rewrite inline CSS and JS blocks
        output = Self::rewrite_css(&output, prefix);
        output = Self::rewrite_js(&output, prefix);

        output
    }

    fn rewrite_url(url: &str, prefix: &str, base: &str) -> Option<String> {
        // Ignore special schemes
        if url.starts_with("data:")
            || url.starts_with("javascript:")
            || url.starts_with("#")
        {
            return None;
        }

        let base = Url::parse(base).ok()?;
        let absolute = base.join(url).ok()?;
        debug!("RewriteURL: {}{}", prefix, absolute);
        Some(format!("{}{}", prefix, absolute))
    }

    fn rewrite_js(js: &str, prefix: &str) -> String {
        let re = Regex::new(
            r#"(fetch|axios|get|post|WebSocket)\s*\(\s*["']([^"']+)["']"#,
        )
        .unwrap();

        re.replace_all(js, |caps: &regex::Captures| {
            let func = &caps[1];
            let url = &caps[2];

            if url.starts_with("http") || url.starts_with("/") {
                format!(r#"{}("{}{}")"#, func, prefix, url)
            } else {
                caps[0].to_string()
            }
        })
        .to_string()
    }

    fn rewrite_css(css: &str, prefix: &str) -> String {
        let re = Regex::new(r#"url\((["']?)([^"')]+)\1\)"#).unwrap();

        re.replace_all(css, |caps: &regex::Captures| {
            let quote = &caps[1];
            let url = &caps[2];

            if url.starts_with("http") || url.starts_with("/") {
                format!("url({}{}{})", quote, prefix.to_string() + url, quote)
            } else {
                caps[0].to_string()
            }
        })
        .to_string()
    }

    pub async fn ws_proxy(
        &self,
        req: HttpRequest,
        payload: web::Payload,
    ) -> actix_web::Result<HttpResponse> {
        let target = self.build_target_url(&req);
        debug!("Websocket proxy → {}", target);

        // Upgrade client connection
        let (response, mut session, mut msg_stream) = actix_ws::handle(&req, payload)?;
        let no_forward_headers = self.config.no_forward_headers;
        let no_forward_cookies = self.config.no_forward_cookies;

        // Spawn proxy task
        actix_rt::spawn(async move {
            let host = target.host().unwrap_or_default().to_string();
            let mut ws = Client::builder().finish().ws(target);
            ws.set_camel_case_headers(true);
            // Insert headers
            if !no_forward_headers {
                for (name, value) in req.headers().iter() {
                    if name == header::HOST {
                        ws = ws.set_header(header::HOST, host.clone());
                    }
                    else if name != header::CONNECTION && 
                            name != header::UPGRADE &&
                            name != header::SEC_WEBSOCKET_KEY &&
                            name != header::SEC_WEBSOCKET_EXTENSIONS &&
                            name != header::SEC_WEBSOCKET_VERSION
                    {
                        if name.as_str().contains("127.0.0.1") {
                            ws = ws.set_header(name.as_str().replace("127.0.0.1", "192.168.1.124").clone(), value.clone());
                        }
                        else {
                            ws = ws.set_header(name.clone(), value.clone());
                        }
                    }
                }
            }

            // // Insert cookies
            if !no_forward_cookies {
                if let Ok(cookies) = req.cookies() {
                    for x in cookies.iter() {
                        ws = ws.cookie(x.clone());
                    }
                }
            }

            // Connect to upstream websocket
            let (_resp, mut upstream) = match ws.connect().await {
                Ok(x) => x,
                Err(e) => {
                    error!("WS upstream connect failed: {}", e);
                    return;
                }
            };

            loop {
                tokio::select! {

                    // Client -> Upstream
                    Some(msg) = msg_stream.next() => {
                        match msg {
                            Ok(Message::Text(txt)) => {
                                if upstream.send(awc::ws::Message::Text(txt)).await.is_err() {
                                    break;
                                }
                            }
                            Ok(Message::Binary(bin)) => {
                                if upstream.send(awc::ws::Message::Binary(bin)).await.is_err() {
                                    break;
                                }
                            }
                            Ok(Message::Ping(v)) => {
                                let _ = upstream.send(awc::ws::Message::Ping(v)).await;
                            }
                            Ok(Message::Pong(v)) => {
                                let _ = upstream.send(awc::ws::Message::Pong(v)).await;
                            }
                            Ok(Message::Close(reason)) => {
                                let _ = upstream.send(awc::ws::Message::Close(reason)).await;
                                break;
                            }
                            _ => {}
                        }
                    }

                    // Upstream -> Client
                    Some(frame) = upstream.next() => {
                        match frame {
                            Ok(awc::ws::Frame::Text(txt)) => {
                                if let Ok(txt) = String::from_utf8(txt.to_vec()) {
                                    if session.text(txt).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            Ok(awc::ws::Frame::Binary(bin)) => {
                                if session.binary(bin).await.is_err() {
                                    break;
                                }
                            }
                            Ok(awc::ws::Frame::Ping(v)) => {
                                let _ = session.ping(&v).await;
                            }
                            Ok(awc::ws::Frame::Pong(v)) => {
                                let _ = session.pong(&v).await;
                            }
                            Ok(awc::ws::Frame::Close(reason)) => {
                                let _ = session.close(reason).await;
                                break;
                            }
                            _ => {}
                        }
                    }

                    else => break,
                }
            }

            debug!("WebSocket proxy closed");
        });

        Ok(response)
    }

}
