use matcher::{Config, Engine};
use std::path::PathBuf;
use structopt::StructOpt;
use tracing::info;

#[derive(Debug, StructOpt)]
#[structopt(name = "matcher", about = "High-performance trading order matching engine")]
struct Args {
    /// Configuration file path
    #[structopt(short, long, parse(from_os_str), default_value = "config.toml")]
    config: PathBuf,
    
    /// Product ID to trade (overrides config)
    #[structopt(long)]
    product_id: Option<String>,
    
    /// Listen port (overrides config)
    #[structopt(long)]
    listen_port: Option<u16>,
    
    /// Multicast address (overrides config)
    #[structopt(long)]
    multicast_addr: Option<String>,
    
    /// Log level (overrides config)
    #[structopt(long)]
    log_level: Option<String>,
    
    /// Generate default configuration file
    #[structopt(long)]
    generate_config: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::from_args();
    
    // Generate default config if requested
    if args.generate_config {
        let default_config = Config::default();
        default_config.save_to_file(&args.config)?;
        println!("Generated default configuration at: {}", args.config.display());
        return Ok(());
    }
    
    // Load configuration
    let mut config = if args.config.exists() {
        Config::from_file(&args.config)?
    } else {
        println!("Configuration file not found, using defaults");
        Config::default()
    };
    
    // Apply command line overrides
    if let Some(product_id) = args.product_id {
        config.engine.product_ids = vec![product_id];
    }
    
    if let Some(listen_port) = args.listen_port {
        config.network.listen_port = listen_port;
    }
    
    if let Some(multicast_addr) = args.multicast_addr {
        config.network.multicast_addr = multicast_addr;
    }
    
    if let Some(log_level) = args.log_level {
        config.monitoring.log_level = log_level;
    }
    
    // Create and start the engine
    let mut engine = Engine::new(config).await?;
    
    info!("Starting Matcher - High-Performance Trading Engine");
    info!("Version: {}", matcher::VERSION);
    
    engine.start().await?;
    
    // Keep the engine running
    info!("Engine is running. Press Ctrl+C to stop.");
    
    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    
    info!("Shutdown signal received, stopping engine...");
    
    Ok(())
}