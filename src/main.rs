use std::env;
use std::process;

use log::{info, error};
use api_proxy_lib::config::{load_config, Config};
use api_proxy_lib::server;

#[actix_web::main]
async fn main() {
    // Initialize logger
    env_logger::init();

    // Read configuration path from env or default
    let config_path = env::args().nth(1).unwrap_or_else(|| "config.toml".to_string());

    // Load configuration
    let config: Config = match load_config(&config_path) {
        Ok(cfg) => {
            info!("Configuration loaded from '{}'", config_path);
            cfg
        }
        Err(e) => {
            error!("Failed to load configuration '{}': {:?}", config_path, e);
            process::exit(1);
        }
    };

    // Run servers
    if let Err(e) = server::run_servers(config.servers).await {
        error!("Server error: {:?}", e);
        process::exit(1);
    }

    info!("All servers stopped gracefully");
}
