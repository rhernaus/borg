use anyhow::{Context, Result};
use log::{info, warn};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::core::config::Config;
use crate::core::ethics::EthicsManager;
use crate::core::strategy::{ActionType, Plan, StrategyManager};
use crate::resource_monitor::monitor::{ResourceLimits, ResourceMonitor, SystemResourceMonitor};
use crate::swarm::{SwarmCoordinator, SwarmCycleResult};
use crate::testing::simple::SimpleTestRunner;
use crate::testing::test_runner::TestRunner;
use crate::version_control::git::GitManager;
use crate::version_control::git_implementation::GitImplementation;

/// The main agent structure that coordinates the self-improvement process
pub struct Agent {
    /// Configuration parameters
    config: Config,

    /// Working directory
    working_dir: PathBuf,

    /// Test runner for validating changes
    test_runner: Arc<dyn TestRunner>,

    /// Git manager for version control
    git_manager: Arc<Mutex<dyn GitManager>>,

    /// Resource monitor for tracking system usage
    resource_monitor: Arc<Mutex<dyn ResourceMonitor>>,

    /// Ethics manager for ensuring ethical compliance
    ethics_manager: Arc<Mutex<EthicsManager>>,

    /// Strategy manager for coordinating different action strategies
    strategy_manager: Arc<Mutex<StrategyManager>>,
}

#[allow(dead_code)]
impl Agent {
    /// Create a new agent with the given configuration
    pub async fn new(config: Config) -> Result<Self> {
        info!(
            "Initializing agent with working directory: {}",
            config.agent.working_dir
        );

        // Create the working directory if it doesn't exist
        let working_dir = PathBuf::from(&config.agent.working_dir);
        std::fs::create_dir_all(&working_dir).context(format!(
            "Failed to create working directory: {:?}",
            working_dir
        ))?;

        // Create logs directory for LLM if enabled
        if config.logging.enabled {
            let log_dir = PathBuf::from(&config.logging.llm_log_dir);
            std::fs::create_dir_all(&log_dir)
                .context(format!("Failed to create log directory: {:?}", log_dir))?;
        }

        // Create data directory for persistence
        let data_dir = working_dir.join("data");
        std::fs::create_dir_all(&data_dir)
            .context(format!("Failed to create data directory: {:?}", data_dir))?;

        // Initialize components
        let git_manager: Arc<Mutex<dyn GitManager>> = Arc::new(Mutex::new(
            GitImplementation::new(&working_dir).context("Failed to create GitImplementation")?,
        ));

        let test_runner: Arc<dyn TestRunner> = Arc::new(SimpleTestRunner::new(&working_dir)?);

        let resource_limits = ResourceLimits {
            max_memory_mb: config.agent.max_memory_usage_mb as f64,
            max_cpu_percent: config.agent.max_cpu_usage_percent as f64,
            max_disk_mb: Some(1000.0),
        };

        let resource_monitor: Arc<Mutex<dyn ResourceMonitor>> = Arc::new(Mutex::new(
            SystemResourceMonitor::with_limits(resource_limits),
        ));

        let ethics_manager = Arc::new(Mutex::new(EthicsManager::new()));

        let strategy_manager = Arc::new(Mutex::new(StrategyManager::new(Arc::clone(
            &ethics_manager,
        ))));

        let agent = Self {
            config,
            working_dir,
            test_runner,
            git_manager,
            resource_monitor,
            ethics_manager,
            strategy_manager,
        };

        // Initialize the repository if needed
        agent.initialize_git_repository().await?;

        Ok(agent)
    }

    /// Main loop for the agent
    pub async fn run(&mut self) -> Result<()> {
        info!("Agent starting main improvement loop");

        // Initialize the Git repository
        self.initialize_git_repository().await?;

        // Run the improvement loop
        self.improvement_loop().await?;

        info!("Improvement loop completed");

        Ok(())
    }

    /// Initialize the Git repository
    async fn initialize_git_repository(&self) -> Result<()> {
        let repo_path = &self.working_dir;

        // Initialize repository via GitManager abstraction
        {
            let git_manager = self.git_manager.lock().await;
            git_manager.init_repository(repo_path.as_path()).await?;
        }

        // Create an initial README commit only if README.md doesn't exist yet
        let readme_path = repo_path.join("README.md");
        if !readme_path.exists() {
            info!("Creating initial README and committing via GitManager");
            let readme_content = "# Borg Agent Workspace\n\nThis workspace contains files generated and modified by the Borg self-improving AI agent.\n";
            std::fs::write(&readme_path, readme_content)
                .context("Failed to create README.md file")?;

            let git_manager = self.git_manager.lock().await;
            let path_ref: &Path = readme_path.as_path();
            git_manager.add_files(&[path_ref]).await?;
            git_manager.commit("Initial commit").await?;
            info!("Created initial commit with README.md");
        } else {
            info!("Repository already initialized; skipping initial commit");
        }

        Ok(())
    }

    /// Helper function to check system resources
    async fn check_resources(&self) -> Result<bool> {
        info!("Checking system resources before proceeding");

        // Get the resource monitor
        let resource_monitor = self.resource_monitor.lock().await;

        // Get current resource usage
        let usage = resource_monitor.get_resource_usage().await?;
        let memory_usage = usage.memory_usage_mb;
        let cpu_usage = usage.cpu_usage_percent;

        // Get disk space via proper interface
        let within_limits = resource_monitor
            .is_within_limits(&crate::resource_monitor::monitor::ResourceLimits {
                max_memory_mb: self.config.agent.max_memory_usage_mb as f64,
                max_cpu_percent: self.config.agent.max_cpu_usage_percent as f64,
                max_disk_mb: Some(1000.0), // Minimum 1GB of disk space
            })
            .await?;

        // Log current resource usage
        info!(
            "Current resource usage - Memory: {:.1} MB, CPU: {:.1}%, Available disk: {}",
            memory_usage,
            cpu_usage,
            if within_limits {
                "sufficient"
            } else {
                "insufficient"
            }
        );

        // Check if we're exceeding limits
        if memory_usage > (self.config.agent.max_memory_usage_mb as f64) * 0.9 {
            warn!(
                "Memory usage is approaching limit: {:.1} MB / {} MB",
                memory_usage, self.config.agent.max_memory_usage_mb
            );

            if memory_usage > self.config.agent.max_memory_usage_mb as f64 {
                warn!(
                    "Insufficient memory available: {:.1} MB / {} MB",
                    memory_usage, self.config.agent.max_memory_usage_mb
                );
                return Ok(false);
            }
        }

        if cpu_usage > (self.config.agent.max_cpu_usage_percent as f64) * 0.9 {
            warn!(
                "CPU usage is approaching limit: {:.1}% / {}%",
                cpu_usage, self.config.agent.max_cpu_usage_percent
            );

            if cpu_usage > self.config.agent.max_cpu_usage_percent as f64 {
                warn!(
                    "Insufficient CPU available: {:.1}% / {}%",
                    cpu_usage, self.config.agent.max_cpu_usage_percent
                );
                return Ok(false);
            }
        }

        if !within_limits {
            warn!("Insufficient disk space available");
            return Ok(false);
        }

        // Check if we can actually write to the disk
        let test_file = self.working_dir.join(".resource_check_test");
        match fs::write(&test_file, b"Resource check test") {
            Ok(_) => {
                // Clean up test file
                let _ = fs::remove_file(&test_file);
            }
            Err(e) => {
                warn!("Failed to write to working directory: {}", e);
                return Ok(false);
            }
        }

        // All resources are available
        info!("Resource check passed");
        Ok(true)
    }

    /// Get a reference to the agent's configuration
    pub fn get_config(&self) -> &Config {
        &self.config
    }

    /// Build codebase context for swarm agents
    async fn build_codebase_context(&self) -> Result<String> {
        // Get list of source files
        let _git = self.git_manager.lock().await;

        // Build a context string describing the codebase
        let context = format!(
            r#"Codebase: {}
Working directory: {}
This is a Rust project (Cargo-based).

Key modules:
- src/core/ - Core agent logic
- src/swarm/ - Multi-agent swarm coordination
- src/code_generation/ - LLM-based code generation
- src/providers/ - LLM provider implementations
- src/testing/ - Test infrastructure

The swarm should identify improvements that:
1. Enhance system reliability
2. Improve code quality
3. Add valuable features
4. Fix potential issues"#,
            env!("CARGO_PKG_NAME"),
            self.config.agent.working_dir,
        );

        Ok(context)
    }

    /// Get the available action types from all registered strategies
    pub async fn get_available_action_types(&self) -> Vec<ActionType> {
        let strategy_manager = self.strategy_manager.lock().await;
        strategy_manager.get_available_action_types()
    }

    /// Get a list of all registered strategies
    pub async fn get_registered_strategies(&self) -> Vec<String> {
        let strategy_manager = self.strategy_manager.lock().await;
        strategy_manager
            .get_strategies()
            .into_iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// Execute a specific plan
    pub async fn execute_plan(&self, plan: &Plan) -> Result<bool> {
        let strategy_manager = self.strategy_manager.lock().await;
        let result = strategy_manager.execute_plan(plan).await?;
        Ok(result.success)
    }

    /// Execute a specific step of a plan
    pub async fn execute_step(&self, plan: &Plan, step_id: &str) -> Result<bool> {
        let strategy_manager = self.strategy_manager.lock().await;
        let result = strategy_manager.execute_step(plan, step_id).await?;
        Ok(result.success)
    }
}

// Implementation of the main agent loop that orchestrates the optimization process
impl Agent {
    /// The core improvement loop that drives the agent's self-improvement process
    async fn improvement_loop(&mut self) -> Result<()> {
        info!("Starting swarm-based improvement loop");

        // Build codebase context
        let codebase_context = self.build_codebase_context().await?;

        // Create swarm coordinator with the config
        let coordinator = SwarmCoordinator::new(
            self.config.clone(),
            self.git_manager.clone(),
            self.test_runner.clone(),
        )
        .await?;

        // Run swarm cycle
        let results = coordinator.run(&codebase_context, Some(1)).await?;

        for result in results {
            match result {
                SwarmCycleResult::Success {
                    proposal,
                    changes_applied,
                    tests_passed,
                } => {
                    info!(
                        "Swarm successfully executed: {} (changes: {}, tests: {})",
                        proposal.title, changes_applied, tests_passed
                    );
                }
                SwarmCycleResult::NoConsensus {
                    proposals_count,
                    rejection_reasons,
                } => {
                    info!(
                        "Swarm reached no consensus - {} proposals considered",
                        proposals_count
                    );
                    for reason in rejection_reasons {
                        info!("  Rejection reason: {}", reason);
                    }
                }
                SwarmCycleResult::ExecutionFailed { proposal, error } => {
                    warn!("Swarm execution failed for '{}': {}", proposal.title, error);
                }
                SwarmCycleResult::NoImprovementsFound => {
                    info!("Swarm found no improvements - system is optimal");
                }
            }
        }

        Ok(())
    }
}

/// Initialize the agent's components
impl Agent {
    /// Initialize the agent's components
    pub async fn initialize(&self) -> Result<()> {
        // Create data directories
        let data_dir = self.working_dir.join("data");
        fs::create_dir_all(&data_dir).context("Failed to create data directory")?;

        // Initialize ethics manager
        {
            let _ethics = self.ethics_manager.lock().await;
            // No initialize method, nothing to do
        }

        // Initialize version control
        {
            let _vc = self.git_manager.lock().await;
            // No initialize method, nothing to do
        }

        info!("Agent initialized successfully");

        Ok(())
    }
}

// Helper functions for metric evaluation

/// Optional accessor for Modes v2 dispatcher (feature-gated via config).
/// NOTE: Modes v2 has been removed for MVP. This always returns None.
impl Agent {
    pub fn maybe_mode_dispatcher(&self) -> Option<()> {
        // Modes v2 removed - always return None
        None
    }
}
