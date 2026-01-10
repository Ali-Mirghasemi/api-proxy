use actix_web::{HttpRequest, HttpResponse};
use actix_web::http::{Method, StatusCode, header};
use awc::Client;
use actix_web::web::Bytes;
use std::sync::Arc;

use log::{debug, error};

use crate::config::{ServerConfig, ApiConfig, Mode};

/// Shared HTTP client
fn http_client() -> Client {
    Client::builder()
        .disable_redirects()
        .finish()
}

/// Entry point called from server.rs
pub async fn forward(
    req: HttpRequest,
    body: Bytes,
    server: Arc<ServerConfig>,
    api: ApiConfig,
) -> HttpResponse {
    match api.mode.unwrap_or_default() {
        Mode::Raw => forward_raw(req, body, server, api).await,
        Mode::Rule => forward_rule(req, body, server, api).await,
    }
}

async fn forward_raw(
    req: HttpRequest,
    body: Bytes,
    server: Arc<ServerConfig>,
    api: ApiConfig,
) -> HttpResponse {
    let client = http_client();

    let target_url = build_target_url(&req, &server, &api);
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
    for (k, v) in &api.inject_headers {
        client_req = client_req.insert_header((k.as_str(), v.as_str()));
    }

    let res = match client_req.send_body(body).await {
        Ok(r) => r,
        Err(e) => {
            error!("Upstream request failed: {}", e);
            return HttpResponse::BadGateway().finish();
        }
    };

    build_response(res).await
}

async fn forward_rule(
    req: HttpRequest,
    body: Bytes,
    server: Arc<ServerConfig>,
    api: ApiConfig,
) -> HttpResponse {
    // 🔒 Placeholder: JSON / header validation will live here
    debug!("RULE proxy validation passed");

    forward_raw(req, body, server, api).await
}

fn build_target_url(
    req: &HttpRequest,
    server: &ServerConfig,
    api: &ApiConfig,
) -> String {
    let base = &api.target; // upstream base URL
    let path = req.uri().path();
    
    match req.uri().query() {
        Some(q) => format!("{}{}?{}", base, path, q),
        None => format!("{}{}", base, path),
    }
}

async fn build_response(
    mut res: awc::ClientResponse,
) -> HttpResponse {
    let status = res.status();

    let mut client_resp = HttpResponse::build(status);

    for (name, value) in res.headers().iter() {
        client_resp.append_header((name.clone(), value.clone()));
    }

    match res.body().await {
        Ok(body) => client_resp.body(body),
        Err(_) => HttpResponse::BadGateway().finish(),
    }
}

