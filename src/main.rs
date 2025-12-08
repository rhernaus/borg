use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use log::{info, LevelFilter};
use std::path::Path;

use borg::core::agent::Agent;
use borg::core::config::Config;

#[derive(Parser)]
#[clap(author, version, about = "Borg - Autonomous Self-Improving AI Agent")]
struct Cli {
    /// Path to config file (YAML format)
    #[clap(short, long, default_value = "config.yaml")]
    config: String,

    /// Debug mode
    #[clap(short, long)]
    debug: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a single improvement iteration
    Improve,

    /// Display information about the agent
    Info,
}

fn main() -> Result<()> {
    // Parse command line arguments
    let cli = Cli::parse();

    // Initialize logger
    let log_level = if cli.debug {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };
    env_logger::Builder::new().filter_level(log_level).init();

    // Load configuration (YAML format)
    let config_path = determine_config_path(&cli.config)?;
    info!("Using configuration file: {}", config_path.display());
    let config = Config::from_file(&config_path)?;

    // Ensure logs directory exists
    if config.logging.enabled {
        let log_dir = std::path::Path::new(&config.logging.llm_log_dir);
        std::fs::create_dir_all(log_dir)
            .with_context(|| format!("Failed to create log directory: {:?}", log_dir))?;
        info!("Ensured log directory exists: {:?}", log_dir);
    }

    // Initialize and run the agent
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let agent = Agent::new(config).await?;
            agent.initialize().await?;
            handle_commands(cli.command, agent).await
        })
}

/// Determine which configuration file to use
fn determine_config_path(cli_config: &str) -> Result<std::path::PathBuf> {
    // Return the config file path (defaults to config.yaml)
    // Copy config.sample.yaml to config.yaml and add your API keys
    Ok(Path::new(cli_config).to_path_buf())
}

/// Print a banner with information about the agent
fn print_banner() {
    println!("\n====================================================");
    println!("  BORG - Autonomous Self-Improving AI Agent v0.1.0");
    println!("====================================================");
    println!("  ✅ Core Agent            ✅ Ethics Framework");
    println!("  ✅ Swarm Architecture    ✅ Multi-Model Support");
    println!("  ✅ Version Control       ✅ Code Generation");
    println!("  ✅ Testing Framework     ✅ Resource Monitoring");
    println!("====================================================\n");
}

/// Handle the commands passed to the agent
async fn handle_commands(command: Option<Commands>, mut agent: Agent) -> Result<()> {
    match command {
        None => {
            println!("Running in default mode...");
            agent.run().await
        }
        Some(Commands::Improve) => {
            println!("Running improvement cycle...");
            agent.run().await
        }
        Some(Commands::Info) => {
            print_banner();
            println!("Agent Information:");
            let version = env!("CARGO_PKG_VERSION");
            println!("Version: {}", version);
            println!(
                "Working Directory: {}",
                agent.get_config().agent.working_dir
            );
            println!("Mode: Swarm-based improvements");
            println!("Models configured: {}", agent.get_config().models.len());
            Ok(())
        }
    }
}
