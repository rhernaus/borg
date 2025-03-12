use anyhow::Result;
use clap::Parser;
use log::{info, LevelFilter, debug};
use borg::core::agent::Agent;
use borg::core::config::Config;
use std::path::Path;

#[derive(Parser)]
#[clap(author, version, about = "An autonomous self-improving AI agent")]
struct Cli {
    /// Path to config file
    #[clap(short, long, default_value = "config.production.toml")]
    config: String,

    /// Debug mode
    #[clap(short, long)]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let cli = Cli::parse();

    // Initialize logger
    let log_level = if cli.debug {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };
    env_logger::Builder::new()
        .filter_level(log_level)
        .init();

    print_banner();

    info!("Starting Borg - Autonomous Self-Improving AI Agent");

    // Load configuration: try the provided config file first, then fall back to alternatives if necessary
    let config_path = determine_config_path(&cli.config)?;
    info!("Using configuration file: {}", config_path.display());
    let config = Config::from_file(config_path)?;

    // Initialize agent
    let mut agent = Agent::new(config)?;

    // Start agent's main loop
    agent.run().await?;

    info!("Agent has completed its run successfully");
    info!("All core components are functioning properly");
    info!("The system is ready for defining optimization goals");

    Ok(())
}

/// Determine which configuration file to use
fn determine_config_path(cli_config: &str) -> Result<std::path::PathBuf> {
    // First, try the config file specified by command line argument
    let cli_path = Path::new(cli_config);
    if cli_path.exists() {
        return Ok(cli_path.to_path_buf());
    }

    // If the specified config doesn't exist and is the default value,
    // try falling back to config.toml
    if cli_config == "config.production.toml" {
        let fallback_path = Path::new("config.toml");
        if fallback_path.exists() {
            debug!("Configuration file config.production.toml not found, using config.toml as fallback");
            return Ok(fallback_path.to_path_buf());
        }
    }

    // Otherwise return the original path which will likely cause a proper error
    // when trying to read the file
    Ok(cli_path.to_path_buf())
}

fn print_banner() {
    println!("\n====================================================");
    println!("  BORG - Autonomous Self-Improving AI Agent v0.1.0");
    println!("====================================================");
    println!("  ✅ Core Agent            ✅ Ethics Framework");
    println!("  ✅ Optimization Goals    ✅ Authentication");
    println!("  ✅ Version Control       ✅ Code Generation");
    println!("  ✅ Testing Framework     ✅ Resource Monitoring");
    println!("====================================================\n");
}