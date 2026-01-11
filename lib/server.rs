use actix_web::{App, HttpServer, web, HttpRequest, HttpResponse};
use actix_web::http::header;
use std::sync::Arc;
use std::io::{BufReader};
use std::fs::File;

use rustls::ServerConfig as RustlsConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls_pemfile::Item;

use crate::config::{ServerConfig, ApiConfig};
use crate::cert::ensure_cert;
use crate::errors::ProxyHttpResponse;
use crate::proxy::Proxy;

use log::{info, error};

pub struct Server {
    config:     ServerConfig,
}

impl Server {
    pub fn new(config: ServerConfig) -> Self {
        Self {
            config,
        }
    }
    /// Run one HTTP/HTTPS server instance
    pub async fn run(self) -> std::io::Result<()> {
        let server = Arc::new(self.config);

        let build_app = |server: Arc<ServerConfig>, is_https| {
            let mut app = App::new();

            for api in &server.apis {
                let api = api.clone();
                let server = server.clone();

                app = app.route(
                    &api.path.clone(),
                    web::to(move |req: HttpRequest, body: web::Bytes| {
                        Self::handle_request(req, body, server.clone(), api.clone(), is_https)
                    }),
                );
            }

            app
        };

        let http_server = {
            let _server = server.clone();
            HttpServer::new(move || {
                    build_app(_server.clone(), false)
                })
                .bind(&server.listen)?
                .run()
        };

        info!("HTTP server '{}' listening on {}", server.name, server.listen);

        let https_server = if server.https {
            let cert = server.cert_file.as_ref().expect("cert_file required for HTTPS");
            let key  = server.key_file.as_ref().expect("key_file required for HTTPS");

            ensure_cert(cert, key, &server.name)
                .expect("Failed to ensure TLS cert");

            let tls_config = Self::load_rustls_config(cert, key);

            let _server = server.clone();
            Some(
                HttpServer::new(move || {
                        build_app(_server.clone(), true) // ✅ HTTPS
                    })
                    .bind_rustls_0_23(&server.listen, tls_config)?
                    .run()
            )
        } else {
            None
        };

        match https_server {
            Some(https) => {
                info!("HTTPS server '{}' enabled", server.name);
                futures::future::try_join(http_server, https).await?;
            }
            None => {
                http_server.await?;
            }
        }

        Ok(())
    }

    /// Launch all configured servers
    pub async fn run_servers(servers: Vec<ServerConfig>) -> std::io::Result<()> {
        let mut handles = Vec::new();

        for server in servers {
            handles.push(actix_rt::spawn(Self::new(server).run()));
        }

        for h in handles {
            if let Err(e) = h.await {
                error!("Server task failed: {:?}", e);
            }
        }

        Ok(())
    }

    /// Handle request (redirect or proxy)
    pub async fn handle_request(
        req: HttpRequest,
        body: web::Bytes,
        server: Arc<ServerConfig>,
        api: ApiConfig,
        is_https: bool,
    ) -> ProxyHttpResponse {
        let redirect_enabled = api.redirect_http_to_https
            .unwrap_or(server.redirect_http_to_https);

        if redirect_enabled && !is_https {
            return Self::redirect_to_https(&req);
        }

        // 🔜 proxy logic will be called here
        Proxy::new(api).forward(req, body, server).await
    }

    /// Build 301 redirect response
    pub fn redirect_to_https(req: &HttpRequest) -> ProxyHttpResponse {
        let host = req
            .headers()
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let uri = req.uri().to_string();
        let location = format!("https://{}{}", host, uri);

        Ok(HttpResponse::MovedPermanently()
            .append_header((header::LOCATION, location))
            .finish())
    }

    /// Load rustls configuration from PEM files
    pub fn load_rustls_config(cert_path: &str, key_path: &str) -> RustlsConfig {
        let cert_file = File::open(cert_path).expect("Cannot open cert file");
        let key_file  = File::open(key_path).expect("Cannot open key file");

        let mut cert_reader = BufReader::new(cert_file);
        let mut key_reader  = BufReader::new(key_file);

        let certs: Vec<CertificateDer<'static>> =
            rustls_pemfile::read_all(&mut cert_reader)
                .filter_map(|item| match item {
                    Ok(Item::X509Certificate(cert)) => {
                        Some(CertificateDer::from(cert))
                    }
                    _ => None,
                })
                .collect();

        assert!(!certs.is_empty(), "No certificates found");

        let mut keys: Vec<PrivateKeyDer<'static>> =
            rustls_pemfile::read_all(&mut key_reader)
                .filter_map(|item| match item {
                    Ok(Item::Pkcs8Key(key)) => Some(PrivateKeyDer::from(key)),
                    Ok(Item::Pkcs1Key(key)) => Some(PrivateKeyDer::from(key)),
                    _ => None,
                })
                .collect();

        assert!(!keys.is_empty(), "No private keys found");

        RustlsConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, keys.remove(0))
            .expect("Invalid TLS config")
    }
}

