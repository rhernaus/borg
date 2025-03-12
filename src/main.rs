use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use log::{info, LevelFilter, debug};
use borg::core::agent::Agent;
use borg::core::config::Config;
use std::path::{Path, PathBuf};
use std::fs;
use serde::Deserialize;
use std::env;
use std::collections::HashMap;

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

    // Initialize agent
    let agent = Agent::new(config).await?;
    agent.initialize().await?;

    // If no command is provided, run in default mode (full agent process)
    if cli.command.is_none() {
        print_banner();
        info!("Starting Borg - Autonomous Self-Improving AI Agent");

        // Check for test mode environment variables
        let is_test_mode = env::var("BORG_TEST_MODE").unwrap_or_else(|_| String::new()) == "true";
        let disable_long_running = env::var("BORG_DISABLE_LONG_RUNNING").unwrap_or_else(|_| String::new()) == "true";

        // In test mode, we only want to verify initialization, not run the full agent
        if is_test_mode || disable_long_running {
            info!("Running in test mode - skipping full agent execution");
            info!("Agent has been initialized successfully");
            return Ok(());
        }

        // Check for max runtime configuration
        if let Some(max_seconds) = agent.get_config().agent.max_runtime_seconds {
            info!("Agent will run for a maximum of {} seconds", max_seconds);
            // Set up a timer to terminate the agent
            let handle = tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(max_seconds as u64)).await;
                info!("Maximum runtime reached, terminating agent");
                std::process::exit(0);
            });

            // Run the agent's main loop
            let mut agent_mut = agent; // Create mutable agent for run method
            agent_mut.run().await?;

            // Cancel the timer
            handle.abort();
        } else {
            // Run the agent's main loop without time limit
            let mut agent_mut = agent; // Create mutable agent for run method
            agent_mut.run().await?;
        }

        info!("Agent has completed its run successfully");
        info!("All core components are functioning properly");
        info!("The system is ready for defining optimization goals");

        return Ok(());
    }

    // Handle commands
    match cli.command.unwrap() {
        Commands::Improve => {
            println!("Running a single improvement iteration...");
            agent.run_improvement_iteration().await
                .context("Failed to run improvement iteration")?;
        },
        Commands::Info => {
            println!("Agent Information:");
            println!("Version: {}", env!("CARGO_PKG_VERSION"));
            println!("Working Directory: {}", std::env::current_dir()?.display());

            // Get and print goal count
            let optimization_manager = agent.get_optimization_manager().lock().await;
            let goal_count = optimization_manager.get_all_goals().len();
            println!("Active Optimization Goals: {}", goal_count);

            // Get and print plan information
            let planning_manager = agent.get_strategic_planning_manager().lock().await;
            let objectives_count = planning_manager.get_all_objectives().len();
            let milestones_count = planning_manager.get_all_milestones().len();

            println!("Strategic Objectives: {}", objectives_count);
            println!("Milestones: {}", milestones_count);
        },
        Commands::Objective(objective_cmd) => {
            match objective_cmd {
                ObjectiveCommands::Add { id, title, description, timeframe, creator, key_results_file, constraints_file } => {
                    println!("Adding strategic objective: {}", title);

                    // Load key results from file if provided
                    let key_results = if let Some(path) = key_results_file {
                        load_lines_from_file(&path)?
                    } else {
                        Vec::new()
                    };

                    // Load constraints from file if provided
                    let constraints = if let Some(path) = constraints_file {
                        load_lines_from_file(&path)?
                    } else {
                        Vec::new()
                    };

                    agent.add_strategic_objective(
                        &id,
                        &title,
                        &description,
                        timeframe,
                        &creator,
                        key_results,
                        constraints
                    ).await.context("Failed to add strategic objective")?;

                    println!("Strategic objective added successfully");
                },
                ObjectiveCommands::List => {
                    println!("Strategic Objectives:");

                    let planning_manager = agent.get_strategic_planning_manager().lock().await;
                    let objectives = planning_manager.get_all_objectives();

                    if objectives.is_empty() {
                        println!("  No strategic objectives defined");
                    } else {
                        for (i, objective) in objectives.iter().enumerate() {
                            println!("{}. {} ({}%)", i+1, objective.title, objective.progress);
                            println!("   ID: {}", objective.id);
                            println!("   Timeframe: {} months", objective.timeframe);
                            println!("   Created by: {} on {}",
                                objective.created_by,
                                objective.created_at.format("%Y-%m-%d")
                            );
                            println!("   Description: {}", objective.description);

                            if !objective.key_results.is_empty() {
                                println!("   Key Results:");
                                for (j, kr) in objective.key_results.iter().enumerate() {
                                    println!("     {}. {}", j+1, kr);
                                }
                            }

                            if !objective.constraints.is_empty() {
                                println!("   Constraints:");
                                for (j, c) in objective.constraints.iter().enumerate() {
                                    println!("     {}. {}", j+1, c);
                                }
                            }

                            println!();
                        }
                    }
                },
            }
        },
        Commands::Plan(plan_cmd) => {
            match plan_cmd {
                PlanCommands::Generate => {
                    println!("Running planning cycle...");

                    let mut planning_manager = agent.get_strategic_planning_manager().lock().await;
                    planning_manager.run_planning_cycle().await
                        .context("Failed to run planning cycle")?;

                    println!("Planning cycle completed successfully");
                },
                PlanCommands::Show => {
                    println!("Strategic Plan:");

                    let visualization = agent.visualize_strategic_plan().await
                        .context("Failed to visualize strategic plan")?;

                    println!("{}", visualization);
                },
                PlanCommands::Report => {
                    println!("Generating strategic plan progress report...");

                    let report = agent.generate_strategic_plan_report().await
                        .context("Failed to generate strategic plan report")?;

                    println!("{}", report);
                },
            }
        },
        Commands::LoadObjectives { objectives_file } => {
            println!("Loading strategic objectives from: {:?}", objectives_file);
            let toml_content = fs::read_to_string(&objectives_file)
                .context(format!("Failed to read objectives file: {:?}", objectives_file))?;

            let objectives_config: ObjectivesConfig = toml::from_str(&toml_content)
                .context("Failed to parse objectives TOML")?;

            // Add each objective to the agent
            for objective in objectives_config.objectives {
                println!("Adding objective: {}", objective.title);

                agent.add_strategic_objective(
                    &objective.id,
                    &objective.title,
                    &objective.description,
                    objective.timeframe,
                    &objective.creator,
                    objective.key_results,
                    objective.constraints
                ).await.context(format!("Failed to add objective: {}", objective.id))?;
            }

            println!("Successfully loaded all strategic objectives");
            println!("Run 'borg plan generate' to create milestones and tactical goals");
        },
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