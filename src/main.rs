use std::env;
use std::process;
use std::io::Write;

use log::{info, error};
use api_proxy::config::Config;
use api_proxy::server::Server;

#[actix_web::main]
async fn main() {
    // Initialize logger
    env_logger::Builder::from_default_env()
            .format(|buf, record| {
                let mut file = record.file().unwrap_or("Unknown");

                file = if file.starts_with(env!("CARGO_MANIFEST_DIR")) {
                    file = file.trim_start_matches(env!("CARGO_MANIFEST_DIR"));

                    #[cfg(target_os = "windows")]
                    {
                        file = file.trim_start_matches("\\");
                    }
                    
                    #[cfg(target_os = "linux")]
                    {
                        file = file.trim_start_matches("/");
                    }

                    file
                }
                else {
                    file = file.split(".cargo")
                        .last()
                        .unwrap_or(file)
                        .trim_start_matches("/registry/src/")
                        .trim_start_matches("\\registry\\src\\");
                
                    #[cfg(target_os = "windows")]
                    {
                        file = file.split_once("\\").unwrap_or(("", file)).1;
                    }
                    
                    #[cfg(target_os = "linux")]
                    {
                        file = file.split_once("/").unwrap_or(("", file)).1;
                    }

                    file
                };

                writeln!(
                    buf,
                    "[{} {}:{}] {}",
                    record.level(),
                    file,
                    record.line().unwrap_or(0),
                    record.args()
                )
            })
            .init();

    // Read configuration path from env or default
    let config_path = env::args().nth(1).unwrap_or_else(|| "config.toml".to_string());

    // Load configuration
    let config: Config = match Config::load(&config_path) {
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
    if let Err(e) = Server::run_servers(config.servers).await {
        error!("Server error: {:?}", e);
        process::exit(1);
    }

    info!("All servers stopped gracefully");
}
