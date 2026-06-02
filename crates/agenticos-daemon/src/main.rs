mod bootstrap;
mod config;
mod service;
#[cfg(test)]
mod tests;

#[tokio::main]
async fn main() {
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "configs/dev.toml".to_owned());

    let config = match config::DaemonConfig::from_file(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("failed to load config {config_path}: {e}");
            std::process::exit(1);
        }
    };

    let ctx = match bootstrap::DaemonContext::from_config(config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("failed to bootstrap daemon: {e}");
            std::process::exit(1);
        }
    };

    let service = service::DaemonService::new(ctx);

    println!("agenticos daemon starting...");

    if let Err(e) = service.run().await {
        eprintln!("daemon exited with error: {e}");
        std::process::exit(1);
    }
}
