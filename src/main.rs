use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use log::{info, LevelFilter, debug};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;
use std::fs;
use std::env;
use std::collections::HashMap;

use borg::core::agent::Agent;
use borg::core::config::Config;
use borg::version_control::git::GitManager;
use borg::version_control::git_implementation::GitImplementation;
use borg::code_generation::llm_generator::LlmCodeGenerator;
use serde::Deserialize;

#[derive(Parser)]
#[clap(author, version, about = "Borg - Autonomous Self-Improving AI Agent")]
struct Cli {
    /// Path to config file
    #[clap(short, long, default_value = "config.production.toml")]
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

    /// Manage strategic objectives
    #[command(subcommand)]
    Objective(ObjectiveCommands),

    /// Manage planning
    #[command(subcommand)]
    Plan(PlanCommands),

    /// Load strategic objectives from a TOML file
    LoadObjectives {
        /// Path to the strategic objectives TOML file
        #[arg(value_name = "OBJECTIVES_FILE")]
        objectives_file: PathBuf,
    },

    /// Execute Git operations with LLM assistance
    #[command(name = "git")]
    GitOperation {
        /// The Git operation query
        #[arg(name = "query")]
        query: String,
    },

    /// Ask LLM a question with streaming response
    Ask {
        /// The question or prompt to send to the LLM
        #[arg(name = "prompt")]
        prompt: String,

        /// Whether to stream the response as it's generated (default: true)
        #[arg(short, long, default_value = "true")]
        stream: bool,
    },
}

#[derive(Subcommand)]
enum ObjectiveCommands {
    /// Add a new strategic objective
    Add {
        /// Unique identifier for the objective
        id: String,

        /// Title of the objective
        title: String,

        /// Description of the objective
        description: String,

        /// Timeframe in months for achievement
        timeframe: u32,

        /// Creator name
        creator: String,

        /// File containing key results (one per line)
        #[arg(short, long)]
        key_results_file: Option<PathBuf>,

        /// File containing constraints (one per line)
        #[arg(short, long)]
        constraints_file: Option<PathBuf>,
    },

    /// List all strategic objectives
    List,
}

#[derive(Subcommand)]
enum PlanCommands {
    /// Run a planning cycle
    Generate,

    /// Show the current strategic plan
    Show,

    /// Generate a progress report
    Report,
}

#[derive(Debug, Deserialize)]
struct TomlObjective {
    id: String,
    title: String,
    description: String,
    timeframe: u32,
    creator: String,
    key_results: Vec<String>,
    constraints: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ObjectivesConfig {
    objectives: Vec<TomlObjective>,
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
    env_logger::Builder::new()
        .filter_level(log_level)
        .init();

    // Load configuration
    let config_path = determine_config_path(&cli.config)?;
    info!("Using configuration file: {}", config_path.display());
    let mut config = Config::from_file(config_path)?;

    // Ensure logs directory exists
    if config.llm_logging.enabled {
        let log_dir = std::path::Path::new(&config.llm_logging.log_dir);
        std::fs::create_dir_all(log_dir)
            .with_context(|| format!("Failed to create log directory: {:?}", log_dir))?;
        info!("Ensured log directory exists: {:?}", log_dir);
    }

    // Check for mock LLM environment variable
    let use_mock_llm = env::var("BORG_USE_MOCK_LLM").unwrap_or_else(|_| String::new()) == "true";
    if use_mock_llm {
        info!("Test mode: Using mock LLM provider");
        // Override LLM configuration with mock provider
        config.llm = HashMap::from([
            ("default".to_string(), borg::core::config::LlmConfig {
                provider: "mock".to_string(),
                api_key: "test-key".to_string(),
                model: "test-model".to_string(),
                max_tokens: 1024,
                temperature: 0.7,
            }),
        ]);
    }

    // For the git operation command, we don't fully initialize the agent
    // We just need the config to create a code generator
    // For all other commands, we initialize the agent normally
    match &cli.command {
        Some(Commands::GitOperation { .. }) => {
            // Handle git operation separately without moving config
        },
        _ => {
            // Initialize the agent for other commands
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?
                .block_on(async {
                    let agent = Agent::new(config).await?;
                    handle_commands(cli.command, agent).await
                })?;
            return Ok(());
        }
    }

    // Handle the git operation command
    if let Some(Commands::GitOperation { query }) = cli.command {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?
            .block_on(async {
                info!("Executing Git operation with query: {}", query);

                // Get the LLM config
                let llm_config = config.llm.get("code_generation")
                    .or_else(|| config.llm.get("default"))
                    .ok_or_else(|| anyhow!("No suitable LLM configuration found"))?
                    .clone();

                // Create the git manager and code generator
                let git_manager: Arc<TokioMutex<dyn GitManager>> = Arc::new(TokioMutex::new(
                    GitImplementation::new(&PathBuf::from(&config.agent.working_dir))
                        .context("Failed to create git manager")?
                ));

                let code_generator = LlmCodeGenerator::new(
                    llm_config,
                    config.code_generation.clone(),
                    config.llm_logging.clone(),
                    Arc::clone(&git_manager),
                    PathBuf::from(&config.agent.working_dir)
                )?;

                // Generate response
                let response = code_generator.generate_git_operations_response(&query).await?;

                println!("Git Operation Result:\n{}", response);

                Ok::<_, anyhow::Error>(())
            })?;
    }

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

/// Load lines from a file, trimming whitespace
fn load_lines_from_file(path: &PathBuf) -> Result<Vec<String>> {
    let content = fs::read_to_string(path)
        .context(format!("Failed to read file: {:?}", path))?;

    Ok(content.lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect())
}

/// Print a banner with information about the agent
fn print_banner() {
    println!("\n====================================================");
    println!("  BORG - Autonomous Self-Improving AI Agent v0.1.0");
    println!("====================================================");
    println!("  ✅ Core Agent            ✅ Ethics Framework");
    println!("  ✅ Optimization Goals    ✅ Authentication");
    println!("  ✅ Version Control       ✅ Code Generation");
    println!("  ✅ Testing Framework     ✅ Resource Monitoring");
    println!("  ✅ Strategic Planning    ✅ CLI Interface");
    println!("====================================================\n");
}

/// Handle the Ask command with streaming option
async fn handle_ask_command(prompt: String, stream: bool, agent: &Agent) -> Result<()> {
    info!("Processing Ask command with prompt: {}, streaming: {}", prompt, stream);

    // Get access to the config
    let config = agent.get_config();

    // Get an LLM configuration - first try default, then code_generation, then any available one
    let llm_config = config.llm.get("default")
        .or_else(|| config.llm.get("code_generation"))
        .or_else(|| config.llm.values().next())
        .ok_or_else(|| anyhow!("No LLM configuration found"))?
        .clone();

    info!("Using LLM provider: {} with model: {}", llm_config.provider, llm_config.model);

    // Create an LLM provider using the configuration
    let llm_provider = borg::code_generation::llm::LlmFactory::create(
        llm_config,
        config.llm_logging.clone(),
    )?;

    // Generate a response based on the user's choice
    if stream {
        info!("Using streaming mode for response");
        let response = llm_provider.generate_streaming(&prompt, None, Some(0.7)).await?;

        // The streaming implementation already prints to stdout as it receives tokens,
        // so we don't need to print anything else here
        info!("Streaming response completed with {} characters", response.len());
    } else {
        info!("Using non-streaming mode for response");
        let response = llm_provider.generate(&prompt, None, Some(0.7)).await?;
        println!("\n{}\n", response);
        info!("Response completed with {} characters", response.len());
    }

    Ok(())
}

/// Handle the commands passed to the agent
async fn handle_commands(command: Option<Commands>, mut agent: Agent) -> Result<()> {
    match command {
        None => {
            println!("Running in default mode...");
            agent.run().await
        },
        Some(Commands::Improve) => {
            println!("Running a single improvement iteration...");
            agent.run_improvement_iteration().await
        },
        Some(Commands::Info) => {
            print_banner();
            let version = env!("CARGO_PKG_VERSION");
            println!("Version: {}", version);
            println!("Working directory: {}", agent.get_config().agent.working_dir);

            // Display active goals
            let goal_count = agent.get_active_goal_count().await?;
            println!("Active goals: {}", goal_count);

            Ok(())
        },
        Some(Commands::Objective(ObjectiveCommands::Add { id, title, description, timeframe, creator, key_results_file, constraints_file })) => {
            println!("Adding strategic objective: {} - {}", id, title);

            // Load key results if file is provided
            let key_results = if let Some(path) = key_results_file {
                load_lines_from_file(&path)?
            } else {
                Vec::new()
            };

            // Load constraints if file is provided
            let constraints = if let Some(path) = constraints_file {
                load_lines_from_file(&path)?
            } else {
                Vec::new()
            };

            // Add the strategic objective with individual parameters
            agent.add_strategic_objective(
                &id,
                &title,
                &description,
                timeframe,
                &creator,
                key_results,
                constraints
            ).await?;

            println!("Strategic objective added successfully");

            Ok(())
        },
        Some(Commands::Objective(ObjectiveCommands::List)) => {
            let objectives = agent.list_strategic_objectives().await;

            if objectives.is_empty() {
                println!("No strategic objectives found.");
            } else {
                println!("Strategic Objectives:");
                for objective in objectives {
                    println!("- {} (ID: {})", objective.title, objective.id);
                    println!("  Description: {}", objective.description);
                    println!("  Timeframe: {} months", objective.timeframe);
                    println!("  Created by: {}", objective.created_by);
                    println!("  Progress: {}%", objective.progress);
                    println!();
                }
            }

            Ok(())
        },
        Some(Commands::Plan(PlanCommands::Generate)) => {
            println!("Generating strategic plan...");
            agent.generate_strategic_plan().await?;
            println!("Strategic plan generated successfully");

            Ok(())
        },
        Some(Commands::Plan(PlanCommands::Show)) => {
            let plan_option = agent.get_current_strategic_plan().await;

            match plan_option {
                Some(plan) => {
                    println!("Strategic Plan:");
                    println!("Created: {}", plan.created_at.format("%Y-%m-%d %H:%M:%S"));
                    println!("Last Updated: {}", plan.updated_at.format("%Y-%m-%d %H:%M:%S"));

                    println!("\nObjectives:");
                    for objective in &plan.objectives {
                        println!("- {} ({}% complete)", objective.title, objective.progress);
                    }

                    println!("\nMilestones:");
                    for milestone in &plan.milestones {
                        println!("- {} ({}% complete)", milestone.title, milestone.progress);
                    }
                },
                None => {
                    println!("No active strategic plan found.");
                }
            }

            Ok(())
        },
        Some(Commands::Plan(PlanCommands::Report)) => {
            println!("Generating progress report...");
            let report = agent.generate_progress_report().await?;
            println!("{}", report);

            Ok(())
        },
        Some(Commands::LoadObjectives { objectives_file }) => {
            println!("Loading strategic objectives from file: {:?}", objectives_file);

            let file_content = fs::read_to_string(&objectives_file)
                .context(format!("Failed to read objectives file: {:?}", objectives_file))?;

            let config: ObjectivesConfig = toml::from_str(&file_content)
                .context(format!("Failed to parse objectives file: {:?}", objectives_file))?;

            println!("Found {} objectives in file", config.objectives.len());

            for obj in config.objectives {
                // Add each strategic objective with individual parameters
                agent.add_strategic_objective(
                    &obj.id,
                    &obj.title,
                    &obj.description,
                    obj.timeframe,
                    &obj.creator,
                    obj.key_results,
                    obj.constraints
                ).await?;

                println!("Added objective: {}", obj.id);
            }

            println!("All objectives loaded successfully");

            Ok(())
        },
        Some(Commands::GitOperation { .. }) => {
            // This is handled separately in main.rs
            unreachable!("Git operation should be handled separately");
        },
        Some(Commands::Ask { prompt, stream }) => {
            handle_ask_command(prompt, stream, &agent).await
        },
    }
}