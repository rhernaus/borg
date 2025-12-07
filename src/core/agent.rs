use anyhow::{anyhow, Context, Result};
use log::{debug, error, info, warn};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::code_generation::generator::CodeGenerator;
use crate::code_generation::llm_generator::LlmCodeGenerator;
use crate::core::authentication::{AccessRole, AuthenticationManager};
use crate::core::config::{Config, LlmConfig};
use crate::core::ethics::EthicsManager;
use crate::core::optimization::{
    GoalStatus, OptimizationCategory, OptimizationGoal, OptimizationManager, PriorityLevel,
};
use crate::core::persistence::PersistenceManager;
use crate::core::planning::{StrategicObjective, StrategicPlan, StrategicPlanningManager};
use crate::core::strategies::CodeImprovementStrategy;
use crate::core::strategy::{ActionType, Plan, StrategyManager};
use crate::database::DatabaseManager;
use crate::resource_monitor::monitor::{ResourceLimits, ResourceMonitor, SystemResourceMonitor};
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

    /// Code generator for producing improvements
    code_generator: Arc<dyn CodeGenerator>,

    /// Test runner for validating changes
    test_runner: Arc<dyn TestRunner>,

    /// Git manager for version control
    git_manager: Arc<Mutex<dyn GitManager>>,

    /// Resource monitor for tracking system usage
    resource_monitor: Arc<Mutex<dyn ResourceMonitor>>,

    /// Ethics manager for ensuring ethical compliance
    ethics_manager: Arc<Mutex<EthicsManager>>,

    /// Optimization manager for tracking improvement goals
    optimization_manager: Arc<Mutex<OptimizationManager>>,

    /// Authentication manager for user permissions
    authentication_manager: Arc<Mutex<AuthenticationManager>>,

    /// Persistence manager for saving/loading goals
    persistence_manager: PersistenceManager,

    /// Database manager for persistent storage
    database_manager: Option<Arc<DatabaseManager>>,

    /// Strategic planning manager
    strategic_planning_manager: Arc<Mutex<StrategicPlanningManager>>,

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
        if config.llm_logging.enabled {
            let log_dir = PathBuf::from(&config.llm_logging.log_dir);
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

        let code_generator: Arc<dyn CodeGenerator> = {
            // Get LLM configuration - first try specific config for code generation, then default
            let llm_config = config
                .llm
                .get("code_generation")
                .or_else(|| config.llm.get("default"))
                .ok_or_else(|| anyhow!("No suitable LLM configuration found"))?
                .clone();

            Arc::new(LlmCodeGenerator::new(
                llm_config,
                config.code_generation.clone(),
                config.llm_logging.clone(),
                Arc::clone(&git_manager),
                working_dir.clone(),
            )?)
        };

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

        let optimization_manager = Arc::new(Mutex::new(OptimizationManager::new(Arc::clone(
            &ethics_manager,
        ))));

        let authentication_manager = Arc::new(Mutex::new(AuthenticationManager::new()));

        let persistence_manager =
            PersistenceManager::new(&data_dir).context("Failed to create persistence manager")?;

        // Initialize the database manager
        let database_manager = match DatabaseManager::new(&data_dir, &config).await {
            Ok(manager) => {
                info!("Database manager initialized successfully");
                Some(Arc::new(manager))
            }
            Err(e) => {
                warn!("Failed to initialize database manager: {}", e);
                warn!("Proceeding without database manager");
                None
            }
        };

        let strategic_planning_manager = Arc::new(Mutex::new(StrategicPlanningManager::new(
            Arc::clone(&optimization_manager),
            Arc::clone(&ethics_manager),
            &data_dir.to_string_lossy(),
        )));

        let strategy_manager = Arc::new(Mutex::new(StrategyManager::new(
            Arc::clone(&authentication_manager),
            Arc::clone(&ethics_manager),
        )));

        let agent = Self {
            config,
            working_dir,
            code_generator,
            test_runner,
            git_manager,
            resource_monitor,
            ethics_manager,
            optimization_manager,
            authentication_manager,
            persistence_manager,
            database_manager,
            strategic_planning_manager,
            strategy_manager,
        };

        // Initialize the repository if needed
        agent.initialize_git_repository().await?;

        // Register built-in strategies
        agent.register_strategies().await?;

        // Load goals from disk
        agent.load_goals_from_disk().await?;

        Ok(agent)
    }

    /// Main loop for the agent
    pub async fn run(&mut self) -> Result<()> {
        info!("Agent starting main improvement loop");

        // Initialize the Git repository
        self.initialize_git_repository().await?;

        // Load goals from disk
        self.load_goals_from_disk().await?;

        // Check if we have any existing goals and create defaults if needed
        self.initialize_optimization_goals().await?;

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

    /// Initialize default optimization goals if needed
    async fn initialize_optimization_goals(&self) -> Result<()> {
        let mut optimization_manager = self.optimization_manager.lock().await;

        // Check if we already have goals
        if optimization_manager.get_all_goals().is_empty() {
            info!("No optimization goals found, creating initial goals");

            // Create some initial goals
            let goals = vec![
                {
                    let mut goal = OptimizationGoal::new(
                        "persist-001",
                        "Implement Goal Persistence to Disk",
                        "Design and implement a persistence mechanism to save optimization goals to disk in a structured format (JSON/TOML) and load them on startup. This ensures goal progress isn't lost between agent restarts and enables long-term tracking of improvement history."
                    );
                    goal.category = OptimizationCategory::General;
                    goal.priority = u8::from(PriorityLevel::Critical);
                    goal.tags.push("file:src/core/persistence.rs".to_string());
                    goal.tags.push("file:src/core/optimization.rs".to_string());
                    goal.success_metrics = vec![
                        "Goals persist between agent restarts".to_string(),
                        "Loading time < 100ms for up to 1000 goals".to_string(),
                        "Saves changes within 50ms of goal modification".to_string(),
                        "File format is human-readable for manual inspection".to_string(),
                    ];
                    goal.implementation_notes = Some("Implement using serde with the existing Serialize/Deserialize traits on OptimizationGoal. Consider using an atomic file write pattern to prevent data corruption during saves.".to_string());
                    goal
                },
                {
                    let mut goal = OptimizationGoal::new(
                        "codegen-001",
                        "Enhance Prompt Templates for Code Generation",
                        "Improve the LLM prompt templates for code generation to provide more context, constraints, and examples of desired output. Include Rust best practices, memory safety guidelines, and error handling patterns in the prompts to generate higher quality and more secure code."
                    );
                    goal.priority = u8::from(PriorityLevel::High);
                    goal.tags
                        .push("file:src/code_generation/prompt.rs".to_string());
                    goal.tags
                        .push("file:src/code_generation/llm_generator.rs".to_string());
                    goal.success_metrics = vec![
                        "Reduction in rejected code changes by 50%".to_string(),
                        "Increase in test pass rate of generated code by 30%".to_string(),
                        "Higher quality error handling in generated code".to_string(),
                        "Better adherence to Rust idioms in generated code".to_string(),
                    ];
                    goal.implementation_notes = Some("Research effective prompt engineering techniques for code generation. Include system message that emphasizes Rust safety and idioms. Create specialized templates for different code modification tasks.".to_string());
                    goal
                },
                {
                    let mut goal = OptimizationGoal::new(
                        "testing-001",
                        "Implement Comprehensive Testing Framework",
                        "Enhance the testing framework to include code linting, compilation validation, unit tests, integration tests, and performance benchmarks. The framework should provide detailed feedback on why tests failed to guide future improvement attempts."
                    );
                    goal.category = OptimizationCategory::TestCoverage;
                    goal.priority = u8::from(PriorityLevel::High);
                    goal.tags
                        .push("file:src/testing/test_runner.rs".to_string());
                    goal.tags.push("file:src/testing/simple.rs".to_string());
                    goal.success_metrics = vec![
                        "Complete test pipeline with 5+ validation stages".to_string(),
                        "Detailed error reports for failed tests".to_string(),
                        "Test coverage reporting for generated code".to_string(),
                        "Performance comparison against baseline for changes".to_string(),
                    ];
                    goal.implementation_notes = Some("Integrate with rustfmt, clippy, and cargo for validation. Store test results in structured format for trend analysis. Implement timeouts for each test phase.".to_string());
                    goal
                },
                {
                    let mut goal = OptimizationGoal::new(
                        "multi-llm-001",
                        "Implement Multi-LLM Architecture",
                        "Refactor the LLM integration to support multiple specialized models for different tasks: code generation, ethics assessment, test validation, and planning. Each model should be optimized for its specific task with appropriate context windows and parameters."
                    );
                    goal.category = OptimizationCategory::Performance;
                    goal.priority = u8::from(PriorityLevel::Medium);
                    goal.tags
                        .push("file:src/code_generation/llm_generator.rs".to_string());
                    goal.tags.push("file:src/core/ethics.rs".to_string());
                    goal.success_metrics = vec![
                        "Successful integration of 4+ specialized LLMs".to_string(),
                        "30%+ improvement in code quality via specialized models".to_string(),
                        "More nuanced ethical assessments".to_string(),
                        "Fallback mechanisms for API unavailability".to_string(),
                    ];
                    goal.implementation_notes = Some("Create an LLM manager that can route requests to the appropriate model based on task. Implement caching to reduce API costs. Add performance tracking to identify which models perform best for which tasks.".to_string());
                    goal
                },
                {
                    let mut goal = OptimizationGoal::new(
                        "resource-001",
                        "Implement Resource Usage Forecasting",
                        "Create a sophisticated resource monitoring system that not only tracks current usage but predicts future resource needs based on planned operations. This forecasting should help prevent resource exhaustion and optimize scheduling of intensive tasks."
                    );
                    goal.category = OptimizationCategory::Performance;
                    goal.priority = u8::from(PriorityLevel::Medium);
                    goal.tags
                        .push("file:src/resource_monitor/forecasting.rs".to_string());
                    goal.tags
                        .push("file:src/resource_monitor/monitor.rs".to_string());
                    goal.success_metrics = vec![
                        "Predict memory usage with 90%+ accuracy".to_string(),
                        "Predict CPU usage spikes 30+ seconds in advance".to_string(),
                        "Automatic throttling when resources are constrained".to_string(),
                        "Resource usage visualization and trend analysis".to_string(),
                    ];
                    goal.implementation_notes = Some("Implement time-series analysis of resource metrics. Use simple linear regression for initial forecasting. Consider adding a small machine learning model for more accurate predictions of complex patterns.".to_string());
                    goal
                },
                {
                    let mut goal = OptimizationGoal::new(
                        "ethics-001",
                        "Enhanced Ethical Decision Framework",
                        "Implement a more sophisticated ethical assessment system that can evaluate potential improvements across multiple ethical dimensions. The framework should consider safety, privacy, fairness, transparency, and alignment with human values."
                    );
                    goal.category = OptimizationCategory::Security;
                    goal.priority = u8::from(PriorityLevel::High);
                    goal.tags.push("file:src/core/ethics.rs".to_string());
                    goal.success_metrics = vec![
                        "Multi-dimensional ethical scoring system".to_string(),
                        "Detailed reasoning for ethical decisions".to_string(),
                        "Integration with specialized ethics LLM".to_string(),
                        "Audit trail of ethical assessments".to_string(),
                    ];
                    goal.implementation_notes = Some("Research ethical frameworks for AI systems. Implement structured reasoning about potential consequences of code changes. Create detailed logging of decision-making process.".to_string());
                    goal
                },
            ];

            // Add the goals to the optimization manager
            for goal in goals {
                optimization_manager.add_goal(goal.clone());
                info!("Added optimization goal: {} ({})", goal.title, goal.id);
            }

            info!(
                "Initialized {} default optimization goals",
                optimization_manager.get_all_goals().len()
            );
        } else {
            info!(
                "Found {} existing optimization goals",
                optimization_manager.get_all_goals().len()
            );
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

    /// Identify the next optimization goal to work on
    async fn identify_next_goal(&self) -> Result<Option<OptimizationGoal>> {
        let optimization_manager = self.optimization_manager.lock().await;
        let next_goal = optimization_manager.get_next_goal();

        match next_goal {
            Some(goal) => {
                info!(
                    "Selected next optimization goal: {} ({})",
                    goal.title, goal.id
                );
                Ok(Some(goal.clone()))
            }
            None => {
                info!("No optimization goals available to work on");
                Ok(None)
            }
        }
    }

    /// Perform ethical assessment on a proposed goal
    async fn assess_goal_ethics(&self, goal: &mut OptimizationGoal) -> Result<bool> {
        info!("Assessing ethics for goal: {}", goal.id);

        // Get the ethics manager
        let mut ethics_manager = self.ethics_manager.lock().await;

        // Perform ethical assessment
        goal.assess_ethics(&mut ethics_manager);

        // Check if the goal is ethically sound
        let is_ethical = goal.is_ethically_sound();

        if !is_ethical {
            info!("Goal {} failed ethical assessment", goal.id);
        } else {
            info!("Goal {} passed ethical assessment", goal.id);
        }

        Ok(is_ethical)
    }

    /// Load optimization goals from disk
    pub async fn load_goals_from_disk(&self) -> Result<()> {
        // First try to load from database if available
        if let Some(db_manager) = &self.database_manager {
            match db_manager.goals().get_all().await {
                Ok(records) if !records.is_empty() => {
                    let goals: Vec<OptimizationGoal> = records
                        .into_iter()
                        .map(|record| record.entity().clone())
                        .collect();

                    let mut optimization_manager = self.optimization_manager.lock().await;
                    for goal in goals {
                        optimization_manager.add_goal(goal);
                    }

                    info!(
                        "Loaded {} optimization goals from database",
                        optimization_manager.get_all_goals().len()
                    );
                    return Ok(());
                }
                Ok(_) => {
                    debug!("No optimization goals found in database");
                }
                Err(e) => {
                    warn!("Failed to load optimization goals from database: {}", e);
                }
            }
        }

        // Fall back to persistence manager
        match self.persistence_manager.load_optimization_goals() {
            Ok(goals) => {
                let mut optimization_manager = self.optimization_manager.lock().await;
                for goal in goals {
                    optimization_manager.add_goal(goal);
                }

                info!(
                    "Loaded {} optimization goals from persistence manager",
                    optimization_manager.get_all_goals().len()
                );
                Ok(())
            }
            Err(e) => {
                warn!("Failed to load optimization goals from disk: {}", e);
                Ok(()) // Return Ok to continue even if loading fails
            }
        }
    }

    /// Save optimization goals to disk
    pub async fn save_goals_to_disk(&self) -> Result<()> {
        // First, use the persistence manager as before
        let goals = {
            let optimization_manager = self.optimization_manager.lock().await;
            optimization_manager.get_all_goals().to_vec()
        };

        self.persistence_manager
            .save_optimization_goals(&goals)
            .context("Failed to save optimization goals with persistence manager")?;

        // If database manager is available, also save to database
        if let Some(db_manager) = &self.database_manager {
            for goal in &goals {
                match db_manager.goals().update(goal.clone(), None).await {
                    Ok(_) => debug!("Updated goal {} in database", goal.id),
                    Err(e) => {
                        // If update fails, try to insert
                        match db_manager.goals().insert(goal.clone()).await {
                            Ok(_) => debug!("Inserted goal {} in database", goal.id),
                            Err(e2) => {
                                error!("Failed to save goal {} to database: update error: {}, insert error: {}",
                                    goal.id, e, e2);
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Get a reference to the strategic planning manager
    pub fn get_strategic_planning_manager(&self) -> &Arc<Mutex<StrategicPlanningManager>> {
        &self.strategic_planning_manager
    }

    /// Get a reference to the optimization manager
    pub fn get_optimization_manager(&self) -> &Arc<Mutex<OptimizationManager>> {
        &self.optimization_manager
    }

    /// Get a reference to the agent's configuration
    pub fn get_config(&self) -> &Config {
        &self.config
    }

    /// Register all available strategies with the strategy manager
    async fn register_strategies(&self) -> Result<()> {
        let mut strategy_manager = self.strategy_manager.lock().await;

        // Register the code improvement strategy
        let code_improvement_strategy = CodeImprovementStrategy::new(
            self.working_dir.clone(),
            self.code_generator.clone(),
            self.test_runner.clone(),
            self.git_manager.clone(),
            self.authentication_manager.clone(),
            self.optimization_manager.clone(),
        );

        strategy_manager.register_strategy(code_improvement_strategy);

        info!(
            "Registered {} strategies",
            strategy_manager.get_strategies().len()
        );

        Ok(())
    }

    /// Process an optimization goal using the appropriate strategy
    async fn process_goal(&self, goal: OptimizationGoal) -> Result<()> {
        info!("Processing optimization goal: {}", goal.id);

        // Update goal status to in-progress
        {
            let mut optimization_manager = self.optimization_manager.lock().await;
            if let Some(goal_mut) = optimization_manager.get_goal_mut(&goal.id) {
                goal_mut.update_status(GoalStatus::InProgress);
            }
        }

        // Check ethics first
        let ethical = self.assess_goal_ethics(&mut goal.clone()).await?;
        if !ethical {
            warn!("Goal {} failed ethical assessment, skipping", goal.id);

            // Update goal status to failed
            self.update_goal_status(&goal.id, GoalStatus::Failed).await;
            return Ok(());
        }

        // Use the strategy manager to create a plan
        let plan = {
            let mut strategy_manager = self.strategy_manager.lock().await;
            match strategy_manager.create_plan(&goal).await {
                Ok(plan) => plan,
                Err(e) => {
                    warn!("Failed to create plan for goal {}: {}", goal.id, e);
                    self.update_goal_status(&goal.id, GoalStatus::Failed).await;
                    return Ok(());
                }
            }
        };

        // Execute the plan
        let result = {
            let strategy_manager = self.strategy_manager.lock().await;
            match strategy_manager.execute_plan(&plan).await {
                Ok(result) => result,
                Err(e) => {
                    warn!("Failed to execute plan for goal {}: {}", goal.id, e);
                    self.update_goal_status(&goal.id, GoalStatus::Failed).await;
                    return Ok(());
                }
            }
        };

        // Update goal status based on the result
        if result.success {
            info!("Successfully completed goal {}", goal.id);
            self.update_goal_status(&goal.id, GoalStatus::Completed)
                .await;
        } else {
            warn!("Failed to complete goal {}: {}", goal.id, result.message);
            self.update_goal_status(&goal.id, GoalStatus::Failed).await;
        }

        // Save goals to disk
        self.save_goals_to_disk().await?;

        Ok(())
    }

    /// Update the status of a goal
    async fn update_goal_status(&self, goal_id: &str, status: GoalStatus) {
        let mut optimization_manager = self.optimization_manager.lock().await;
        if let Some(goal) = optimization_manager.get_goal_mut(goal_id) {
            goal.update_status(status);
        }
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

    /// Get the number of active goals
    pub async fn get_active_goal_count(&self) -> Result<usize> {
        let optimization_manager = self.optimization_manager.lock().await;
        Ok(optimization_manager.get_all_goals().len())
    }

    /// List all strategic objectives
    pub async fn list_strategic_objectives(&self) -> Vec<StrategicObjective> {
        // Prefer the in-memory plan, which contains the most up-to-date objective state
        let planning_manager = self.strategic_planning_manager.lock().await;
        let mut objectives = planning_manager.get_plan().objectives.clone();
        drop(planning_manager);

        // If no objectives are present in-memory (e.g., fresh start), fall back to database
        if objectives.is_empty() {
            if let Some(db_manager) = &self.database_manager {
                match futures::executor::block_on(db_manager.objectives().get_all()) {
                    Ok(records) => {
                        objectives = records
                            .into_iter()
                            .map(|record| record.entity().clone())
                            .collect();
                    }
                    Err(e) => {
                        error!("Error retrieving strategic objectives from database: {}", e);
                    }
                }
            }
        }

        objectives
    }

    /// Generate a strategic plan
    pub async fn generate_strategic_plan(&self) -> Result<()> {
        info!("Generating strategic plan using LLM integration");
        let mut planning_manager = self.strategic_planning_manager.lock().await;

        // Run the planning cycle with LLM integration
        planning_manager.run_planning_cycle().await?;

        // Log the successful generation
        info!("Strategic plan generated successfully with LLM integration");

        // Save to the database if available
        if let Some(db_manager) = &self.database_manager {
            match db_manager
                .save_plan(planning_manager.get_plan().clone())
                .await
            {
                Ok(_) => {
                    info!("Saved strategic plan to MongoDB database");
                }
                Err(e) => {
                    warn!("Failed to save strategic plan to database: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Get the current strategic plan from the database
    pub async fn get_current_strategic_plan(&self) -> Option<StrategicPlan> {
        if let Some(db_manager) = &self.database_manager {
            match db_manager.get_current_plan().await {
                Ok(Some(plan)) => Some(plan),
                Ok(None) => {
                    debug!("No current strategic plan found in database");
                    None
                }
                Err(e) => {
                    error!("Error retrieving strategic plan from database: {}", e);
                    None
                }
            }
        } else {
            // Fall back to the in-memory plan from the strategic planning manager
            let planning_manager = self.strategic_planning_manager.lock().await;
            Some(planning_manager.get_plan().clone())
        }
    }

    /// Save a strategic plan to the database
    pub async fn save_strategic_plan(&self, plan: StrategicPlan) -> Result<()> {
        if let Some(db_manager) = &self.database_manager {
            db_manager
                .save_full_plan(&plan)
                .await
                .context("Failed to save strategic plan to database")?;
            Ok(())
        } else {
            // Fall back to the in-memory plan in the strategic planning manager
            let mut planning_manager = self.strategic_planning_manager.lock().await;
            planning_manager.set_plan(plan);
            planning_manager
                .save_to_disk()
                .await
                .context("Failed to save strategic plan to disk")?;
            Ok(())
        }
    }

    /// Generate a progress report
    pub async fn generate_progress_report(&self) -> Result<String> {
        self.generate_strategic_plan_report().await
    }

    /// Get the LLM config for planning
    pub fn get_planning_llm_config(&self) -> Option<LlmConfig> {
        // Try to get the planning-specific LLM config first
        self.config
            .llm
            .get("planning")
            .or_else(|| self.config.llm.get("default"))
            .cloned()
    }
}

// Implementation of the main agent loop that orchestrates the optimization process
impl Agent {
    /// The core improvement loop that drives the agent's self-improvement process
    async fn improvement_loop(&mut self) -> Result<()> {
        info!("Starting improvement loop");

        // Initialize Git and optimization goals
        self.initialize_git_repository().await?;
        self.initialize_optimization_goals().await?;

        // Process each available goal
        loop {
            // Check system resources before proceeding
            let resources_ok = self.check_resources().await?;
            if !resources_ok {
                warn!("System resources are low, pausing improvement loop");
                // Sleep briefly and check again
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                continue;
            }

            // Get the next goal to work on
            let next_goal = self.identify_next_goal().await?;

            let working_goal = match next_goal {
                Some(goal) => goal,
                None => {
                    info!("No optimization goals available to work on");
                    break;
                }
            };

            // Process the goal
            if let Err(e) = self.process_goal(working_goal).await {
                error!("Error processing goal: {}", e);
            }
        }

        info!("No goals to work on, waiting for new goals");
        info!("Improvement loop completed");

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

        // Initialize optimization goals from disk or create defaults
        self.initialize_optimization_goals().await?;

        // Initialize authentication manager and authenticate the agent
        self.authenticate_agent().await?;

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

        // Set up the planning manager with the right LLM config
        let planning_llm_config = self.get_planning_llm_config();
        let mut planning_manager = self.strategic_planning_manager.lock().await;

        // Set the LLM config if available
        if let Some(config) = planning_llm_config {
            planning_manager.set_llm_config(config);
        }

        // Set the LLM logging config
        planning_manager.set_llm_logging_config(self.config.llm_logging.clone());

        // Set planning LLM timeouts from config (fallback to defaults if zero)
        planning_manager.set_timeouts(
            self.config.planning.llm_timeout_seconds,
            self.config.planning.milestone_llm_timeout_seconds,
        );

        // Release the lock before potentially long operations
        drop(planning_manager);

        // Load strategic planning data - if using MongoDB, load from there first
        if let Some(db_manager) = &self.database_manager {
            match db_manager.get_current_plan().await {
                Ok(Some(plan)) => {
                    info!("Loaded strategic plan from MongoDB");
                    let mut planning_manager = self.strategic_planning_manager.lock().await;
                    planning_manager.set_plan(plan);
                }
                Ok(None) => {
                    info!("No strategic plan found in MongoDB, checking file system");
                    let mut planning_manager = self.strategic_planning_manager.lock().await;
                    planning_manager.load_from_disk().await?;
                }
                Err(e) => {
                    warn!("Failed to load strategic plan from MongoDB: {} - falling back to file system", e);
                    let mut planning_manager = self.strategic_planning_manager.lock().await;
                    planning_manager.load_from_disk().await?;
                }
            }
        } else {
            // If not using MongoDB, load from disk
            let mut planning_manager = self.strategic_planning_manager.lock().await;
            planning_manager.load_from_disk().await?;
        }

        info!("Agent initialized successfully");

        Ok(())
    }

    /// Authenticate the agent to enable autonomous operations
    async fn authenticate_agent(&self) -> Result<()> {
        info!("Authenticating agent for autonomous operation");

        let mut auth_manager = self.authentication_manager.lock().await;

        // Grant Developer access automatically without password verification
        match auth_manager.grant_access("agent", AccessRole::Developer) {
            Ok(role) => {
                info!("Agent authenticated successfully with role: {}", role);
                Ok(())
            }
            Err(e) => {
                warn!("Failed to authenticate agent: {}", e);
                // Continue without authentication - operations requiring it will fail
                // This is a graceful fallback rather than a hard error
                Ok(())
            }
        }
    }

    /// Run a single improvement iteration
    pub async fn run_improvement_iteration(&self) -> Result<()> {
        info!("Starting improvement iteration");

        // Find an optimization goal to work on
        let goal = match self.identify_next_goal().await? {
            Some(g) => g,
            None => {
                info!("No suitable optimization goals available");
                return Ok(());
            }
        };

        // Process this goal
        match self.process_goal(goal).await {
            Ok(_) => {
                info!("Successfully processed goal");
            }
            Err(e) => {
                error!("Failed to process goal: {}", e);
            }
        }

        // Save updated goals to disk
        self.save_goals_to_disk().await?;

        info!("Completed improvement iteration");

        Ok(())
    }

    /// Add a strategic objective
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::cognitive_complexity)]
    pub async fn add_strategic_objective(
        &self,
        id: &str,
        title: &str,
        description: &str,
        timeframe: u32,
        creator: &str,
        key_results: Vec<String>,
        constraints: Vec<String>,
    ) -> Result<()> {
        let mut planning_manager = self.strategic_planning_manager.lock().await;

        let objective = StrategicObjective::new(id, title, description, timeframe, creator)
            .with_key_results(key_results)
            .with_constraints(constraints);

        planning_manager.add_objective(objective.clone());

        // Attempt to persist to database if available (file-based or MongoDB) before releasing the lock
        if let Some(db_manager) = &self.database_manager {
            match tokio::time::timeout(
                std::time::Duration::from_secs(3),
                db_manager.objectives().insert(objective.clone()),
            )
            .await
            {
                Ok(Ok(_)) => {
                    info!("Inserted strategic objective {} into database", id);
                }
                Ok(Err(e)) => {
                    warn!(
                        "Failed to insert strategic objective {} into database: {} - continuing",
                        id, e
                    );
                }
                Err(_) => {
                    warn!(
                        "Timed out inserting strategic objective {} into database - continuing",
                        id
                    );
                }
            }
        }

        // Clone the objective to avoid borrowing issues
        drop(planning_manager);

        // Generate milestones for this objective in a separate transaction
        let mut planning_manager = self.strategic_planning_manager.lock().await;

        // Add timeout to milestone generation
        let milestone_result = tokio::time::timeout(
            std::time::Duration::from_secs(5), // 5 second timeout
            planning_manager.generate_milestones_for_objective(&objective),
        )
        .await;

        let milestones = match milestone_result {
            Ok(Ok(milestones)) => milestones,
            Ok(Err(e)) => {
                warn!(
                    "Error generating milestones: {} - continuing without milestones",
                    e
                );
                Vec::new()
            }
            Err(_) => {
                warn!("Milestone generation timed out after 5 seconds - continuing without milestones");
                Vec::new()
            }
        };

        for milestone in milestones {
            planning_manager.add_milestone(milestone);
        }

        // Get the current plan to save it
        let plan = planning_manager.get_plan().clone();

        // Release the lock before potentially long-running operations
        drop(planning_manager);

        // Save the strategic plan - prioritize database if available
        if let Some(db_manager) = &self.database_manager {
            match tokio::time::timeout(
                std::time::Duration::from_secs(3), // 3 second timeout
                db_manager.save_plan(plan),
            )
            .await
            {
                Ok(Ok(_)) => {
                    info!("Saved strategic plan to MongoDB database");
                }
                Ok(Err(e)) => {
                    warn!("Failed to save strategic plan to MongoDB: {} - falling back to file system", e);
                    // Fall back to file system
                    let planning_manager = self.strategic_planning_manager.lock().await;
                    if let Err(e) = tokio::time::timeout(
                        std::time::Duration::from_secs(3), // 3 second timeout
                        planning_manager.save_to_disk(),
                    )
                    .await
                    {
                        warn!("Saving plan to disk timed out: {} - continuing anyway", e);
                    }
                }
                Err(_) => {
                    warn!("Save to MongoDB timed out - falling back to file system");
                    // Fall back to file system
                    let planning_manager = self.strategic_planning_manager.lock().await;
                    if let Err(e) = tokio::time::timeout(
                        std::time::Duration::from_secs(3), // 3 second timeout
                        planning_manager.save_to_disk(),
                    )
                    .await
                    {
                        warn!("Saving plan to disk timed out: {} - continuing anyway", e);
                    }
                }
            }
        } else {
            // If not using MongoDB, save to disk
            let planning_manager = self.strategic_planning_manager.lock().await;
            if let Err(e) = tokio::time::timeout(
                std::time::Duration::from_secs(3), // 3 second timeout
                planning_manager.save_to_disk(),
            )
            .await
            {
                warn!("Saving plan to disk timed out: {} - continuing anyway", e);
            }
        }

        Ok(())
    }

    /// Generate a report on the strategic plan
    pub async fn generate_strategic_plan_report(&self) -> Result<String> {
        let planning_manager = self.strategic_planning_manager.lock().await;
        planning_manager.generate_progress_report().await
    }

    /// Visualize the strategic plan
    pub async fn visualize_strategic_plan(&self) -> Result<String> {
        let planning_manager = self.strategic_planning_manager.lock().await;
        planning_manager.generate_planning_visualization()
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
