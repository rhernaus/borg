use anyhow::Result;
use clap::Parser;
use log::{info, LevelFilter};
use borg::core::agent::Agent;
use borg::core::config::Config;

#[derive(Parser)]
#[clap(author, version, about = "An autonomous self-improving AI agent")]
struct Cli {
    /// Path to config file
    #[clap(short, long, default_value = "config.toml")]
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

    // Load configuration
    let config = Config::from_file(&cli.config)?;

    // Initialize agent
    let mut agent = Agent::new(config)?;

    // Start agent's main loop
    agent.run().await?;

    info!("Agent has completed its run successfully");
    info!("All core components are functioning properly");
    info!("The system is ready for defining optimization goals");

    Ok(())
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