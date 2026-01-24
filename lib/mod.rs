//! Core library modules for the API Proxy application.
//!
//! This crate provides the building blocks required to run a configurable
//! API proxy with request filtering, payload validation, rate limiting,
//! and optional TLS support.
//!
//! # Modules
//!
//! - [`config`]  — Configuration models and validation rules loaded from TOML/JSON.
//! - [`errors`]  — Central error type used across the proxy and Actix integration.
//! - [`server`]  — HTTP/HTTPS server initialization and listener logic.
//! - [`proxy`]   — Request forwarding, filtering, validation, and response handling.
//! - [`cert`]    — TLS certificate utilities (only available with the `tls` feature).
//!
//! Most applications will interact with this crate by:
//! 1. Loading a [`config::Config`] from disk
//! 2. Spawning one or more servers using [`server`]
//! 3. Forwarding requests through [`proxy`] with validation and policy enforcement

pub mod errors;
pub mod config;
pub mod server;
pub mod proxy;

#[cfg(feature = "tls")]
pub mod cert;
