//! HTTP and HTTPS server orchestration for the API proxy.
//!
//! This module is responsible for:
//! - Spawning one or more Actix-Web servers
//! - Binding configured API routes
//! - Handling HTTP → HTTPS redirection
//! - Initializing TLS (via rustls) when enabled
//! - Dispatching incoming requests to the [`Proxy`] layer
//!
//! Each server instance is configured using [`ServerConfig`]
//! and may expose multiple API proxy routes.

use actix_web::dev::WebService;
use actix_web::{App, HttpServer, web, HttpRequest, HttpResponse};
use actix_web::http::header;
use std::sync::Arc;
use std::io::{BufReader};
use std::fs::File;

#[cfg(feature = "rustls")]
use rustls::ServerConfig as RustlsConfig;
#[cfg(feature = "rustls")]
// use rustls::pki_types::{CertificateDer, PrivateKeyDer};
#[cfg(feature = "rustls")]
use rustls_pemfile::Item;

#[cfg(feature = "rustls-0_23")]
use rustls_0_23::ServerConfig as RustlsConfig;
#[cfg(feature = "rustls-0_23")]
use rustls_0_23::pki_types::{CertificateDer, PrivateKeyDer};
#[cfg(feature = "rustls-0_23")]
use rustls_pemfile::Item;

#[cfg(feature = "__tls")]
use crate::cert::ensure_cert;

use crate::config::{ServerConfig, ApiConfig};
use crate::errors::ProxyHttpResponse;
use crate::proxy::Proxy;

use log::{debug, error, info};

/// HTTP/HTTPS server instance.
///
/// A `Server` owns a single [`ServerConfig`] and is responsible
/// for binding listeners, registering routes, and forwarding
/// requests to the proxy layer.
pub struct Server {
    /// Server configuration.
    config:     ServerConfig,
}

impl Server {
    /// Create a new server instance from configuration.
    pub fn new(config: ServerConfig) -> Self {
        Self {
            config,
        }
    }
    
    /// Run a single HTTP/HTTPS server instance.
    ///
    /// This method:
    /// - Builds the Actix application
    /// - Registers all configured API routes
    /// - Starts an HTTP listener
    /// - Optionally starts an HTTPS listener (TLS feature)
    ///
    /// If HTTPS is enabled, both HTTP and HTTPS servers
    /// are run concurrently.
    pub async fn run(self) -> std::io::Result<()> {
        debug!("{:#?}", self.config);

        let server = Arc::new(self.config);

        let build_app = |server: Arc<ServerConfig>, is_https| {
            let mut app = App::new();

            for api in &server.apis {
                let api = api.clone();
                let server = server.clone();
                let path = if api.include_tail {
                    api.path.clone() + "{tail:.*}"
                }
                else {
                    api.path.clone()
                };

                app = app.route(
                    &path,
                    web::to(move |req: HttpRequest, payload: web::Payload| {
                        Self::handle_request(req, payload, server.clone(), api.clone(), is_https)
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

        let https_server: Option<actix_web::dev::Server> = if server.https {
            #[cfg(feature = "__tls")]
            {
                let cert = server.cert_file.as_ref().expect("cert_file required for HTTPS");
                let key  = server.key_file.as_ref().expect("key_file required for HTTPS");

                ensure_cert(cert, key, &server.name)
                    .expect("Failed to ensure TLS cert");

                let tls_config = Self::load_rustls_config(cert, key);

                let _server = server.clone();

                let mut x = HttpServer::new(move || {
                        build_app(_server.clone(), true) // ✅ HTTPS
                    });

                #[cfg(feature = "rustls")]
                {
                    x = x.bind_rustls(&server.listen, tls_config)?;
                }

                #[cfg(feature = "rustls-0_23")]
                {
                    x = x.bind_rustls_0_23(&server.listen, tls_config)?;
                }

                Some(
                    x.run()
                )
            }

            #[cfg(not(feature = "__tls"))]
            {
                None
            }
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

    /// Launch all configured servers concurrently.
    ///
    /// Each server runs in its own asynchronous task.
    /// A failure in one server does not stop others.
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

    /// Handle an incoming HTTP request.
    ///
    /// This function:
    /// - Applies HTTP → HTTPS redirection (if enabled)
    /// - Dispatches the request to the [`Proxy`] layer
    ///
    /// Redirection behavior can be configured globally
    /// (server-level) or overridden per API.
    pub async fn handle_request(
        req: HttpRequest,
        payload: web::Payload,
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
        Proxy::new(api).forward(req, payload, server).await
    }

    /// Build a `301 Moved Permanently` redirect to HTTPS.
    ///
    /// The original request URI is preserved and the
    /// scheme is rewritten to `https`.
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

    /// Load a rustls server configuration from PEM files.
    ///
    /// This function:
    /// - Loads X.509 certificates from a PEM file
    /// - Loads a PKCS#8 private key
    /// - Builds a rustls [`ServerConfig`]
    ///
    /// # Panics
    /// Panics if:
    /// - No certificates are found
    /// - No private keys are found
    /// - The TLS configuration is invalid
    #[cfg(feature = "rustls")]
    pub fn load_rustls_config(cert_path: &str, key_path: &str) -> RustlsConfig {
        use rustls::{Certificate, PrivateKey};

        let cert_file = File::open(cert_path).expect("Cannot open cert file");
        let key_file  = File::open(key_path).expect("Cannot open key file");

        let mut cert_reader = BufReader::new(cert_file);
        let mut key_reader  = BufReader::new(key_file);

        let certs: Vec<Certificate> =
            rustls_pemfile::read_all(&mut cert_reader)
                .map(|x| {
                    x.into_iter().filter_map(|item| {
                        match item {
                            Item::X509Certificate(cert) => {
                                Some(Certificate(cert))
                            }
                            _ => None,
                        }
                    })
                    .collect::<Vec<Certificate>>()
                })
                .unwrap();

        assert!(!certs.is_empty(), "No certificates found");

        let mut keys: Vec<PrivateKey> =
            rustls_pemfile::read_all(&mut key_reader)
                .map(|x| {
                    x.into_iter().filter_map(|item| {
                        match item {
                            Item::PKCS8Key(key) => Some(PrivateKey(key)),
                            _ => None,
                        }
                    }).collect()
                })
                .unwrap();

        assert!(!keys.is_empty(), "No private keys found");

        RustlsConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(certs, keys.remove(0))
            .expect("Invalid TLS config")
    }

    #[cfg(feature = "rustls-0_23")]
    pub fn load_rustls_config(cert_path: &str, key_path: &str) -> RustlsConfig {

        let cert_file = File::open(cert_path).expect("Cannot open cert file");
        let key_file  = File::open(key_path).expect("Cannot open key file");

        let mut cert_reader = BufReader::new(cert_file);
        let mut key_reader  = BufReader::new(key_file);

        let certs: Vec<CertificateDer> =
            rustls_pemfile::read_all(&mut cert_reader)
                .map(|x| {
                    x.into_iter().filter_map(|item| {
                        match item {
                            Item::X509Certificate(cert) => {
                                Some(CertificateDer::from(cert))
                            }
                            _ => None,
                        }
                    })
                    .collect::<Vec<CertificateDer>>()
                })
                .unwrap();

        assert!(!certs.is_empty(), "No certificates found");

        let mut keys: Vec<PrivateKeyDer> =
            rustls_pemfile::read_all(&mut key_reader)
                .map(|x| {
                    x.into_iter().filter_map(|item| {
                        match item {
                            Item::PKCS8Key(key) => Some(PrivateKeyDer::try_from(key).unwrap()),
                            _ => None,
                        }
                    }).collect()
                })
                .unwrap();

        assert!(!keys.is_empty(), "No private keys found");

        RustlsConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, keys.remove(0))
            .expect("Invalid TLS config")
    }
}

