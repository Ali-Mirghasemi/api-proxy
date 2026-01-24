# API Proxy (Rust)

A high-performance, configurable API proxy written in Rust, built on top of Actix Web.
It allows you to expose, secure, validate, and forward APIs with optional TLS, payload inspection, and request filtering.

This project can be used as:

* A standalone proxy server
* A Rust library embedded into other services

---

## Features

* HTTP and HTTPS proxying
* Optional TLS using rustls
* Automatic self-signed certificate generation
* Multiple servers in one process
* Per-API routing and proxy rules
* HTTP → HTTPS redirection
* Payload forwarding and validation
* Header and cookie forwarding/injection
* Payload size limiting
* HTML link rewriting support
* Feature-gated dependencies (JSON, TOML, TLS)

---

## Architecture Overview

```
Client
  │
  │ HTTP / HTTPS
  ▼
Actix Web Server (server.rs)
  │
  │ Route match (per API)
  ▼
Proxy Engine (proxy.rs)
  │
  │ Forward request
  ▼
Upstream API
```

---

## Project Structure

```
.
├── Cargo.toml
├── src/
│   ├── main.rs        # Application entry point
│   ├── server.rs      # HTTP/HTTPS server logic
│   └── cert.rs        # TLS certificate handling
├── lib/
│   ├── mod.rs
│   ├── proxy.rs       # Core proxy & forwarding logic
│   ├── config.rs      # Configuration models & loaders
│   └── errors.rs      # Unified error handling
└── config.toml        # Example configuration file
```

---

## How It Works

1. Server startup

   * Loads configuration from a TOML or JSON file
   * Spawns one or more HTTP/HTTPS servers

2. Request handling

   * Incoming requests are matched against configured API paths
   * Optional HTTP → HTTPS redirect
   * Requests are forwarded to upstream targets

3. Proxy logic

   * Headers and cookies are forwarded or injected
   * Payload size and content type can be enforced
   * Optional rule-based JSON validation
   * Responses are streamed back to the client

4. TLS

   * If enabled, TLS certificates are loaded or automatically generated
   * Uses rustls for modern TLS support

---

## Configuration File

Example `config.toml`:

```toml
[[servers]]
name = "example"
listen = "0.0.0.0:8080"
https = true
cert_file = "certs/server.crt"
key_file  = "certs/server.key"
redirect_http_to_https = true

[[servers.apis]]
path = "/api/"
target = "https://backend.internal"
include_tail = true
keep_proxy_path = false
payload_limit = 1048576
method = "POST"
content_type = "application/json"
```

---

## Running the Proxy

### Build

```
cargo build --release
```

### Run

```
cargo run --release -- config.toml
```

If no configuration file is provided, the proxy defaults to `config.toml`.

---

## TLS Support

TLS is optional and controlled by features.

### Enable TLS

```
cargo build --features tls
```

If certificate or key files do not exist, a self-signed certificate will be generated automatically.

---

## Feature Flags

| Feature | Description                 |
| ------- | --------------------------- |
| full    | Enables JSON, TOML, and TLS |
| json    | JSON parsing & validation   |
| toml    | TOML configuration support  |
| tls     | HTTPS via rustls            |
| clap    | Optional CLI support        |

Default features:

```toml
default = ["full"]
```

---

## Library Usage

```rust
use api_proxy_lib::server::Server;
use api_proxy_lib::config::Config;

let config = Config::load("config.toml")?;
Server::run_servers(config.servers).await?;
```

---

## Error Handling

All proxy and server errors are unified under a custom error system and translated into proper HTTP responses:

* 400 → Validation errors
* 413 → Payload too large
* 502 → Upstream failure
* 500 → Internal errors

---

## Logging

Logging is powered by `env_logger`.

Enable debug logs:

```
RUST_LOG=debug cargo run
```

---

## Security Notes

* Headers and cookies are forwarded intentionally — review configuration carefully
* Self-signed certificates are for development or internal usage
* Always use trusted certificates in production

---

## Roadmap

* Request/response body transformation
* Rate limiting
* Auth middleware (JWT / API keys)
* Metrics (Prometheus)
* OpenAPI-aware validation

---

## License

MIT License

---

## Author

Built with ❤️ in Rust. Designed for performance, security, and control.
