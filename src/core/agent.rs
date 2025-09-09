use anyhow::{anyhow, Context, Result};
use git2::{Repository, Signature};
use log::{debug, error, info, warn};
use serde_json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use walkdir;

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
use crate::resource_monitor::monitor::ResourceMonitor;
use crate::resource_monitor::monitor::SystemResourceMonitor;
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

        let resource_monitor: Arc<Mutex<dyn ResourceMonitor>> =
            Arc::new(Mutex::new(SystemResourceMonitor::new()));

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
        // Initialize or open the repository directly
        let repo_path = &self.working_dir;
        let repo = match Repository::open(repo_path) {
            Ok(repo) => {
                info!("Git repository already opened at {:?}", repo_path);
                repo
            }
            Err(_) => {
                info!("Creating new Git repository at {:?}", repo_path);
                Repository::init(repo_path).context(format!(
                    "Failed to initialize Git repository at {:?}",
                    repo_path
                ))?
            }
        };

        // Check if we need to create an initial commit
        if repo.head().is_err() {
            info!("Creating initial commit");

            // Create a README file
            let readme_path = repo_path.join("README.md");
            let readme_content = "# Borg Agent Workspace\n\nThis workspace contains files generated and modified by the Borg self-improving AI agent.\n";

            std::fs::write(&readme_path, readme_content)
                .context("Failed to create README.md file")?;

            // Add the file to the index
            let mut index = repo.index().context("Failed to get repository index")?;

            index
                .add_path(Path::new("README.md"))
                .context("Failed to add README.md to index")?;

            index.write().context("Failed to write index")?;

            // Create a tree from the index
            let tree_id = index.write_tree().context("Failed to write tree")?;

            let tree = repo.find_tree(tree_id).context("Failed to find tree")?;

            // Create a signature for the commit
            let signature = Signature::now("Borg Agent", "borg@example.com")
                .context("Failed to create signature")?;

            // Create the initial commit
            repo.commit(
                Some("HEAD"),     // Reference to update
                &signature,       // Author
                &signature,       // Committer
                "Initial commit", // Message
                &tree,            // Tree
                &[],              // Parents (empty for initial commit)
            )
            .context("Failed to create initial commit")?;

            info!("Created initial commit with README.md");
        } else {
            info!("Repository already has commits");
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

    /// Generate a code improvement for a specific goal
    async fn generate_improvement(&self, goal: &OptimizationGoal) -> Result<String> {
        info!("Generating improvement for goal: {}", goal.id);

        // Create a code context from the optimization goal
        let context = self.create_code_context(goal).await?;

        // Use the code generator to generate an improvement
        let improvement = self
            .code_generator
            .generate_improvement(&context)
            .await
            .context("Failed to generate code improvement")?;

        // Log success
        info!(
            "Successfully generated improvement for goal: {} with ID: {}",
            goal.id, improvement.id
        );

        // Return the raw code content
        Ok(improvement.code)
    }

    /// Create a code context from an optimization goal
    async fn create_code_context(
        &self,
        goal: &OptimizationGoal,
    ) -> Result<crate::code_generation::generator::CodeContext> {
        use crate::code_generation::generator::CodeContext;

        // Get the file paths for the goal
        let file_paths: Vec<String> = goal
            .tags
            .iter()
            .filter(|tag| tag.starts_with("file:"))
            .map(|tag| tag.trim_start_matches("file:").to_string())
            .collect();

        // Create the context with the goal description as the task
        let context = CodeContext {
            task: goal.description.clone(),
            file_paths,
            requirements: Some(format!(
                "Category: {}\nPriority: {}",
                goal.category, goal.priority
            )),
            previous_attempts: Vec::new(), // For now, we don't track previous attempts
            file_contents: None,
            test_files: None,
            test_contents: None,
            dependencies: None,
            code_structure: None,
            max_attempts: Some(3),
            current_attempt: Some(1),
        };

        Ok(context)
    }

    /// Apply a code change to a branch
    async fn apply_change(
        &self,
        goal: &OptimizationGoal,
        branch_name: &str,
        code: &str,
    ) -> Result<()> {
        info!(
            "Applying change for goal {} to branch {}",
            goal.id, branch_name
        );

        // Open the repository
        let repo = Repository::open(&self.working_dir).context(format!(
            "Failed to open repository at {:?}",
            self.working_dir
        ))?;

        // Extract file changes from the LLM code response
        let improvement_result = self.parse_code_changes(code)?;

        if improvement_result.target_files.is_empty() {
            return Err(anyhow!("No files to modify were identified in the code"));
        }

        info!(
            "Found {} file(s) to modify",
            improvement_result.target_files.len()
        );

        // Apply each file change
        for file_change in &improvement_result.target_files {
            let target_path = Path::new(&file_change.file_path);
            let full_path = self.working_dir.join(target_path);

            // Make sure the directory exists
            if let Some(parent) = target_path.parent() {
                let parent_path = self.working_dir.join(parent);
                std::fs::create_dir_all(&parent_path)
                    .context(format!("Failed to create directory: {:?}", parent_path))?;
            }

            // Write the content to the file
            info!("Writing to file: {:?}", full_path);
            std::fs::write(&full_path, &file_change.new_content)
                .context(format!("Failed to write to file: {:?}", full_path))?;

            // Stage the file
            let mut index = repo.index().context("Failed to get repository index")?;

            // Get relative path
            let relative_path_str = target_path
                .to_str()
                .ok_or_else(|| anyhow!("Failed to convert path to string"))?;

            info!("Adding file to index: {}", relative_path_str);
            index
                .add_path(Path::new(relative_path_str))
                .context(format!(
                    "Failed to add file to index: {}",
                    relative_path_str
                ))?;

            index.write().context("Failed to write index")?;
        }

        // Create a commit
        let tree_id = repo
            .index()
            .unwrap()
            .write_tree()
            .context("Failed to write tree")?;
        let tree = repo.find_tree(tree_id).context("Failed to find tree")?;

        // Create signature
        let signature = Signature::now("Borg Agent", "borg@example.com")
            .context("Failed to create signature")?;

        // Create commit
        let message = format!("Code improvement for goal: {}", goal.id);
        let parent_commit = self.find_branch_commit(&repo, branch_name)?;

        // Create the commit with or without parent
        let commit_id = if let Some(parent) = parent_commit {
            repo.commit(
                Some(&format!("refs/heads/{}", branch_name)),
                &signature,
                &signature,
                &message,
                &tree,
                &[&parent],
            )
            .context("Failed to create commit")?
        } else {
            repo.commit(
                Some(&format!("refs/heads/{}", branch_name)),
                &signature,
                &signature,
                &message,
                &tree,
                &[],
            )
            .context("Failed to create commit")?
        };

        info!("Successfully applied changes for goal {}", goal.id);
        Ok(())
    }

    /// Parse code changes from LLM response
    fn parse_code_changes(
        &self,
        code: &str,
    ) -> Result<crate::code_generation::generator::CodeImprovement> {
        // Create a code generator instance
        let code_gen = self.code_generator.clone();

        // Create a dummy context with the LLM code
        let context = crate::code_generation::generator::CodeContext {
            task: "Apply changes".to_string(),
            requirements: None,
            file_paths: vec![],
            file_contents: None,
            test_files: None,
            test_contents: None,
            code_structure: None,
            previous_attempts: vec![],
            max_attempts: None,
            current_attempt: None,
            dependencies: None,
        };

        // Extract file changes from the code
        let improvement = crate::code_generation::generator::CodeImprovement {
            id: uuid::Uuid::new_v4().to_string(),
            task: "Apply changes".to_string(),
            code: code.to_string(),
            target_files: self.extract_file_changes(code)?,
            explanation: "Changes applied from LLM response".to_string(),
        };

        Ok(improvement)
    }

    /// Extract file changes from code string
    fn extract_file_changes(
        &self,
        code: &str,
    ) -> Result<Vec<crate::code_generation::generator::FileChange>> {
        let re = regex::Regex::new(r"```(?:rust|rs)?\s*(?:\n|\r\n)([\s\S]*?)```").unwrap();
        let mut changes = Vec::new();

        // Let's start by looking for specific files called out with path comments
        let file_re = regex::Regex::new(r#"(?i)for\s+file\s+(?:"|`)?([\w./\\-]+)(?:"|`)?|file:\s*(?:"|`)?([\w./\\-]+)(?:"|`)?|filename:\s*(?:"|`)?([\w./\\-]+)(?:"|`)?"#).unwrap();

        for cap in re.captures_iter(code) {
            let code_block = cap[1].to_string();
            let mut file_path = String::new();

            // Look for a file path in close proximity to this code block
            // First check lines right before the code block
            let code_start_index = code.find(&code_block).unwrap_or(0);
            let pre_code = &code[..code_start_index];

            // Look for the last file path mention before this code block
            if let Some(file_cap) = file_re.captures_iter(pre_code).last() {
                file_path = file_cap
                    .get(1)
                    .or(file_cap.get(2))
                    .or(file_cap.get(3))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
            }

            // If no file path found, use a default
            if file_path.is_empty() {
                file_path = "src/main.rs".to_string();
            }

            changes.push(crate::code_generation::generator::FileChange {
                file_path,
                start_line: None,
                end_line: None,
                new_content: code_block,
            });
        }

        if changes.is_empty() {
            // If no code blocks found, try to look for file path mentions anyway
            for cap in file_re.captures_iter(code) {
                let file_path = cap
                    .get(1)
                    .or(cap.get(2))
                    .or(cap.get(3))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
                if !file_path.is_empty() {
                    changes.push(crate::code_generation::generator::FileChange {
                        file_path,
                        start_line: None,
                        end_line: None,
                        new_content: "// File mentioned but no code provided".to_string(),
                    });
                }
            }
        }

        // If still no changes found, create a default one
        if changes.is_empty() {
            changes.push(crate::code_generation::generator::FileChange {
                file_path: "src/main.rs".to_string(),
                start_line: None,
                end_line: None,
                new_content: "// No specific file identified, adding placeholder comment"
                    .to_string(),
            });
        }

        Ok(changes)
    }

    /// Test a change in a branch
    async fn test_change(&self, branch: &str) -> Result<bool> {
        let test_start = std::time::Instant::now();
        info!("Testing changes in branch {}", branch);

        let result = self.test_runner.run_tests(branch, None).await?;

        // The TestResult.success field now correctly indicates if tests passed
        let passed = result.success;
        let duration = test_start.elapsed();

        // Log the result appropriately
        if passed {
            info!("Tests passed for branch {} in {:?}", branch, duration);

            // Check specific metrics if available
            if let Some(metrics) = &result.metrics {
                info!(
                    "Test metrics: {} tests run, {} passed, {} failed",
                    metrics.tests_run, metrics.tests_passed, metrics.tests_failed
                );
            }

            Ok(true)
        } else {
            error!("Tests failed for branch {} in {:?}", branch, duration);

            // Log any test output regardless of type
            for line in result.output.lines() {
                if line.contains("test result:") || line.contains("error") {
                    error!("Test failure: {}", line);
                }
            }

            Ok(false)
        }
    }

    /// Evaluate the results of a code change
    async fn evaluate_results(
        &self,
        goal: &OptimizationGoal,
        branch: &str,
        test_passed: bool,
    ) -> Result<bool> {
        info!("Evaluating results for goal: {}", goal.id);

        // If tests didn't pass, the change is rejected
        if !test_passed {
            warn!("Tests failed for goal: {}", goal.id);
            return Ok(false);
        }

        // Implement more sophisticated metrics validation

        // Validate against goal success metrics
        let metrics_passed = self.validate_goal_metrics(goal, branch).await?;

        // Get benchmarks if they exist
        if goal.category == OptimizationCategory::Performance {
            info!("Running performance benchmarks for goal: {}", goal.id);
            let benchmark_result = self.test_runner.run_benchmarks(branch).await?;

            if !benchmark_result.success {
                warn!("Benchmark execution failed: {}", benchmark_result.output);
                // Continue with evaluation despite benchmark execution failure
            } else {
                // Parse and validate benchmark results
                let benchmark_improvements =
                    self.analyze_benchmark_results(&benchmark_result.output)?;

                // Check if performance goals were met
                let target = goal.improvement_target;
                if target > 0 && !benchmark_improvements.is_empty() {
                    let avg_improvement = benchmark_improvements.iter().sum::<f64>()
                        / benchmark_improvements.len() as f64;

                    info!("Average performance improvement: {:.2}%", avg_improvement);

                    if avg_improvement < target as f64 {
                        warn!(
                            "Performance improvement of {:.2}% below target of {}%",
                            avg_improvement, target
                        );

                        // Fail if the improvement is significantly below target (less than 70% of target)
                        if avg_improvement < (target as f64 * 0.7) {
                            warn!("Rejecting change: Performance improvement significantly below target");
                            return Ok(false);
                        }

                        // Allow if it's close to target
                        warn!("Accepting change despite being below target as it's within 70% of goal");
                    } else {
                        info!(
                            "Performance improvement met or exceeded target: {:.2}% vs {}%",
                            avg_improvement, target
                        );
                    }
                } else if benchmark_improvements.is_empty() {
                    // We expected performance improvements but couldn't measure any
                    warn!("Could not measure performance improvements");
                    // We'll still return true since the tests passed, but log a warning
                    info!("Accepting change despite unclear performance impact since tests pass");
                }
            }
        }

        // Add category-specific validations
        let category_validation_passed = match goal.category {
            OptimizationCategory::Security => self.validate_security_goal(goal, branch).await?,
            OptimizationCategory::Readability => {
                self.validate_readability_goal(goal, branch).await?
            }
            OptimizationCategory::TestCoverage => {
                self.validate_test_coverage_goal(goal, branch).await?
            }
            OptimizationCategory::ErrorHandling => {
                self.validate_error_handling_goal(goal, branch).await?
            }
            OptimizationCategory::Financial => self.validate_financial_goal(goal, branch).await?,
            _ => true, // Default validation passes for other categories
        };

        if !category_validation_passed {
            warn!("Category-specific validation failed for goal: {}", goal.id);
            return Ok(false);
        }

        if !metrics_passed {
            warn!("Goal metrics validation failed for goal: {}", goal.id);
            return Ok(false);
        }

        // All validations passed
        info!("All validations passed for goal '{}'", goal.id);
        Ok(true)
    }

    /// Validate goal metrics
    async fn validate_goal_metrics(&self, goal: &OptimizationGoal, branch: &str) -> Result<bool> {
        // If no explicit success metrics, default to true
        if goal.success_metrics.is_empty() {
            return Ok(true);
        }

        info!(
            "Validating {} success metrics for goal: {}",
            goal.success_metrics.len(),
            goal.id
        );

        let mut all_metrics_passed = true;

        for (i, metric) in goal.success_metrics.iter().enumerate() {
            info!("Evaluating metric {}: {}", i + 1, metric);

            // Parse and evaluate the metric
            let metric_passed = match self.evaluate_metric(metric, goal, branch).await {
                Ok(passed) => passed,
                Err(e) => {
                    warn!("Error evaluating metric: {}", e);
                    false
                }
            };

            if !metric_passed {
                warn!("Metric failed: {}", metric);
                all_metrics_passed = false;
            } else {
                info!("Metric passed: {}", metric);
            }
        }

        Ok(all_metrics_passed)
    }

    /// Evaluate a specific metric
    async fn evaluate_metric(
        &self,
        metric: &str,
        goal: &OptimizationGoal,
        branch: &str,
    ) -> Result<bool> {
        // This implementation would parse the metric string and evaluate it
        // For example, metrics might be in format "coverage > 80%" or "error rate < 0.1%"

        // For now, we'll implement a simple parsing and evaluation
        if metric.contains("coverage") {
            // Test coverage metric
            return self.evaluate_coverage_metric(metric, branch).await;
        } else if metric.contains("complexity") || metric.contains("cognitive") {
            // Code complexity metric
            return self.evaluate_complexity_metric(metric, goal, branch).await;
        } else if metric.contains("performance")
            || metric.contains("speed")
            || metric.contains("time")
        {
            // Performance metric
            return self.evaluate_performance_metric(metric, branch).await;
        } else if metric.contains("error") || metric.contains("exception") {
            // Error handling metric
            return self.evaluate_error_handling_metric(metric, branch).await;
        }

        // Default for unrecognized metrics
        warn!("Unrecognized metric format: {}", metric);
        Ok(true)
    }

    /// Parse and evaluate a coverage metric
    async fn evaluate_coverage_metric(&self, metric: &str, branch: &str) -> Result<bool> {
        // Extract the target value
        let target = extract_numeric_target(metric)?;

        // Run coverage analysis
        let coverage_result = self.test_runner.run_coverage_analysis(branch).await?;

        if !coverage_result.success {
            warn!("Coverage analysis failed: {}", coverage_result.output);
            return Ok(false);
        }

        // Parse the coverage percentage from output
        let coverage = parse_coverage_percentage(&coverage_result.output)?;

        info!("Coverage: {:.2}%, Target: {:.2}%", coverage, target);

        // Check if we met the target
        Ok(coverage >= target)
    }

    /// Evaluate a complexity metric for a branch
    async fn evaluate_complexity_metric(
        &self,
        metric: &str,
        goal: &OptimizationGoal,
        branch: &str,
    ) -> Result<bool> {
        info!(
            "Evaluating complexity metric '{}' for branch '{}'",
            metric, branch
        );

        // Extract the target value from the metric
        let target = extract_numeric_target(metric)?;

        // Run complexity analysis tool for the branch
        let complexity_cmd = format!("cargo complexity --branch {}", branch);
        let complexity_result = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&complexity_cmd)
            .output()
            .await?;

        if !complexity_result.status.success() {
            warn!(
                "Failed to run complexity analysis: {}",
                String::from_utf8_lossy(&complexity_result.stderr)
            );

            // Fall back to a basic complexity estimation based on file stats
            let complexity = Agent::fallback_complexity_analysis(metric)?;
            // Extract target from metric name if possible, otherwise use a default threshold of 10.0
            let target = if let Some(target_str) = metric.split('_').next_back() {
                target_str.parse::<f64>().unwrap_or(10.0)
            } else {
                10.0 // Default threshold
            };

            info!(
                "Complexity analysis: Score={:.2}, Target={:.2}",
                complexity, target
            );
            return Ok(complexity <= target);
        }

        // Parse the output to get complexity metrics
        let output = String::from_utf8_lossy(&complexity_result.stdout).to_string();
        let complexity = Agent::parse_complexity_output(&output)?;

        info!(
            "Complexity analysis: Score={:.2}, Target={:.2}",
            complexity, target
        );

        // Check if the complexity is below the target
        Ok(complexity <= target)
    }

    /// Fallback method to extract complexity from text output if JSON parsing fails
    fn fallback_complexity_analysis(output: &str) -> Result<f64> {
        // Look for patterns like "Cyclomatic Complexity: 12.5" or "Average complexity: 8.3"
        for line in output.lines() {
            if line.to_lowercase().contains("complexity") {
                // Extract numeric values from the line
                let numbers: Vec<f64> = line
                    .split_whitespace()
                    .filter_map(|word| {
                        word.trim_matches(|c: char| !c.is_ascii_digit() && c != '.')
                            .parse::<f64>()
                            .ok()
                    })
                    .collect();

                if !numbers.is_empty() {
                    // Return the first numeric value found
                    return Ok(numbers[0]);
                }
            }
        }

        // If no complexity metric is found, return an error
        Err(anyhow!("Could not extract complexity metric from output"))
    }

    /// Parse the output from a code complexity tool
    fn parse_complexity_output(output: &str) -> Result<f64> {
        // Attempt to parse JSON output from a tool like cargo-complexity
        // This is a simplified implementation and would need to be adapted
        // for the specific output format of the tool you use

        if let Ok(json) = serde_json::from_str::<serde_json::Value>(output) {
            // Extract complexity metrics - this structure would depend on the tool used

            // For cargo-complexity, the structure might be like:
            if let Some(metrics) = json.get("metrics") {
                if let Some(cyclomatic) = metrics.get("cyclomatic") {
                    if let Some(value) = cyclomatic.get("average") {
                        if let Some(avg) = value.as_f64() {
                            return Ok(avg);
                        }
                    }
                }
            }

            // If we can't find the expected structure, try a more general approach
            // Look for any numeric values that might represent complexity
            if let Some(avg_complexity) = Agent::find_complexity_value(&json) {
                return Ok(avg_complexity);
            }
        }

        // If JSON parsing fails, try to extract complexity from plain text output
        Agent::fallback_complexity_analysis(output)
    }

    /// Recursively search for a complexity value in a JSON structure
    fn find_complexity_value(json: &serde_json::Value) -> Option<f64> {
        match json {
            serde_json::Value::Object(map) => {
                // Check for keys that might indicate complexity metrics
                for (key, value) in map {
                    if key.contains("complex")
                        || key.contains("cyclomatic")
                        || key.contains("cognitive")
                    {
                        if let Some(num) = value.as_f64() {
                            return Some(num);
                        } else if let Some(num) = value.as_i64() {
                            return Some(num as f64);
                        } else if let Some(avg) = Agent::find_complexity_value(value) {
                            return Some(avg);
                        }
                    }
                }

                // If no direct match, recursively check all object values
                for value in map.values() {
                    if let Some(avg) = Agent::find_complexity_value(value) {
                        return Some(avg);
                    }
                }
                None
            }
            serde_json::Value::Array(arr) => {
                // For arrays, calculate the average of any complexity values
                let mut sum = 0.0;
                let mut count = 0;
                for value in arr {
                    if let Some(avg) = Agent::find_complexity_value(value) {
                        sum += avg;
                        count += 1;
                    }
                }
                if count > 0 {
                    Some(sum / count as f64)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Parse and evaluate a performance metric
    async fn evaluate_performance_metric(&self, metric: &str, branch: &str) -> Result<bool> {
        // Extract the target value
        let target = extract_numeric_target(metric)?;

        // Get benchmark results
        let benchmark_result = self.test_runner.run_benchmarks(branch).await?;

        if !benchmark_result.success {
            warn!("Benchmark execution failed: {}", benchmark_result.output);
            return Ok(false);
        }

        // Parse benchmark results
        let improvements = self.analyze_benchmark_results(&benchmark_result.output)?;

        if improvements.is_empty() {
            warn!("No performance improvements measured");
            return Ok(false);
        }

        let avg_improvement = improvements.iter().sum::<f64>() / improvements.len() as f64;

        info!(
            "Performance improvement: {:.2}%, Target: {:.2}%",
            avg_improvement, target
        );

        // Check if we met the target
        Ok(avg_improvement >= target)
    }

    /// Parse and evaluate an error handling metric
    async fn evaluate_error_handling_metric(&self, metric: &str, branch: &str) -> Result<bool> {
        // Extract the target value
        let target = extract_numeric_target(metric)?;

        // Run error handling tests
        info!("Running error handling tests for branch: {}", branch);
        let error_test_result = self
            .test_runner
            .run_tests_with_tag(branch, "error-handling")
            .await?;

        if !error_test_result.success {
            warn!("Error handling tests failed: {}", error_test_result.output);
            return Ok(false);
        }

        // Analyze code for error handling patterns
        info!("Analyzing error handling patterns in code");

        // Get all Rust files in the repository
        let git_manager = self.git_manager.lock().await;
        git_manager.checkout_branch(branch).await?;

        let rust_files = walkdir::WalkDir::new(&self.working_dir)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().and_then(|e| e.to_str()) == Some("rs"))
            .collect::<Vec<_>>();

        if rust_files.is_empty() {
            warn!("No Rust files found for error handling analysis");
            return Ok(false);
        }

        // Count error handling patterns
        let mut total_functions = 0;
        let mut functions_with_error_handling = 0;
        let mut total_lines = 0;
        let mut error_handling_lines = 0;

        // Compile regex patterns for error handling
        let fn_regex = regex::Regex::new(r"fn\s+([a-zA-Z0-9_]+)\s*\(")?;
        let result_regex = regex::Regex::new(r"Result<")?;
        let option_regex = regex::Regex::new(r"Option<")?;
        let error_handling_patterns = [
            regex::Regex::new(r"match|if\s+let\s+(?:Some|Ok|Err|None)")?,
            regex::Regex::new(r"(?:try!|[?]|\.unwrap_or|\.unwrap_or_else|\.unwrap_or_default)")?,
            regex::Regex::new(r"(?:\.map_err|\.and_then|\.or_else|\.map|\.unwrap|\.expect)")?,
            regex::Regex::new(r"catch_unwind|panic!|assert!|unreachable!")?,
            regex::Regex::new(r"Error::|anyhow!|bail!")?,
        ];

        for file in &rust_files {
            let content = fs::read_to_string(file.path())?;
            let lines: Vec<&str> = content.lines().collect();

            total_lines += lines.len();

            let mut in_function = false;
            let mut current_function_has_error_handling = false;
            let mut current_function_returns_result = false;
            let mut current_function_returns_option = false;

            for line in &lines {
                // Detect function declarations
                if let Some(captures) = fn_regex.captures(line) {
                    if in_function
                        && (current_function_returns_result || current_function_returns_option)
                    {
                        total_functions += 1;
                        if current_function_has_error_handling {
                            functions_with_error_handling += 1;
                        }
                    }

                    // Reset for new function
                    in_function = true;
                    current_function_has_error_handling = false;
                    current_function_returns_result = line.contains("-> Result<");
                    current_function_returns_option = line.contains("-> Option<");
                }

                // Check if function returns Result or Option
                if in_function && !current_function_returns_result {
                    current_function_returns_result = result_regex.is_match(line);
                }
                if in_function && !current_function_returns_option {
                    current_function_returns_option = option_regex.is_match(line);
                }

                // Check for error handling patterns
                let has_error_handling = error_handling_patterns
                    .iter()
                    .any(|pattern| pattern.is_match(line));

                if has_error_handling {
                    error_handling_lines += 1;
                    if in_function {
                        current_function_has_error_handling = true;
                    }
                }

                // Detect end of function
                if in_function && line.trim() == "}" && line.trim_start() == "}" {
                    if current_function_returns_result || current_function_returns_option {
                        total_functions += 1;
                        if current_function_has_error_handling {
                            functions_with_error_handling += 1;
                        }
                    }
                    in_function = false;
                }
            }

            // Handle last function in file
            if in_function && (current_function_returns_result || current_function_returns_option) {
                total_functions += 1;
                if current_function_has_error_handling {
                    functions_with_error_handling += 1;
                }
            }
        }

        // Calculate error handling metrics
        let error_handling_ratio = if total_functions > 0 {
            functions_with_error_handling as f64 / total_functions as f64 * 100.0
        } else {
            0.0
        };

        let error_code_ratio = if total_lines > 0 {
            error_handling_lines as f64 / total_lines as f64 * 100.0
        } else {
            0.0
        };

        info!("Error handling analysis: Functions with error handling: {}/{} ({:.1}%), Error handling code: {}/{} lines ({:.1}%)",
            functions_with_error_handling, total_functions, error_handling_ratio,
            error_handling_lines, total_lines, error_code_ratio);

        // Check if the metrics meet the target
        // The interpretation depends on the metric's wording
        if metric.contains("coverage") || metric.contains("ratio") {
            Ok(error_handling_ratio >= target)
        } else if metric.contains("functions") {
            Ok(functions_with_error_handling as f64 >= target)
        } else {
            // Default to using the ratio of functions with error handling
            Ok(error_handling_ratio >= target)
        }
    }

    /// Analyze benchmark results to extract improvement percentages
    fn analyze_benchmark_results(&self, output: &str) -> Result<Vec<f64>> {
        // This would parse benchmark output and extract improvement percentages
        // For example, parsing output like "Task A: 120ms -> 90ms (25% improvement)"

        let mut improvements = Vec::new();

        // Simple parsing logic (would be more sophisticated in real implementation)
        for line in output.lines() {
            if line.contains("improvement") {
                // Try to extract the percentage
                if let Some(percentage) = extract_percentage(line) {
                    improvements.push(percentage);
                }
            }
        }

        Ok(improvements)
    }

    /// Validate a security optimization goal
    async fn validate_security_goal(&self, goal: &OptimizationGoal, branch: &str) -> Result<bool> {
        // Run security-specific tests and analysis
        let security_test_result = self
            .test_runner
            .run_tests_with_tag(branch, "security")
            .await?;

        if !security_test_result.success {
            warn!("Security tests failed: {}", security_test_result.output);
            return Ok(false);
        }

        // In a real implementation, we'd run security analysis tools
        // and check for specific vulnerabilities

        Ok(true)
    }

    /// Validate a readability optimization goal
    async fn validate_readability_goal(
        &self,
        _goal: &OptimizationGoal,
        branch: &str,
    ) -> Result<bool> {
        // Run linting and style checks
        let lint_result = self.test_runner.run_linting(branch).await?;

        if !lint_result.success {
            warn!("Linting failed: {}", lint_result.output);
            return Ok(false);
        }

        // In a real implementation, we'd analyze metrics like:
        // - Comment ratio
        // - Function length
        // - Variable naming consistency

        Ok(true)
    }

    /// Validate a test coverage optimization goal
    async fn validate_test_coverage_goal(
        &self,
        goal: &OptimizationGoal,
        branch: &str,
    ) -> Result<bool> {
        // Run coverage analysis
        let coverage_result = self.test_runner.run_coverage_analysis(branch).await?;

        if !coverage_result.success {
            warn!("Coverage analysis failed: {}", coverage_result.output);
            return Ok(false);
        }

        // Parse the coverage percentage
        let coverage = parse_coverage_percentage(&coverage_result.output)?;

        // Get the target coverage from the goal
        let target_coverage = goal.improvement_target as f64;

        info!(
            "Test coverage: {:.2}%, Target: {:.2}%",
            coverage, target_coverage
        );

        Ok(coverage >= target_coverage)
    }

    /// Validate an error handling optimization goal
    async fn validate_error_handling_goal(
        &self,
        _goal: &OptimizationGoal,
        branch: &str,
    ) -> Result<bool> {
        // Run error scenario tests
        let error_test_result = self
            .test_runner
            .run_tests_with_tag(branch, "error-handling")
            .await?;

        if !error_test_result.success {
            warn!("Error handling tests failed: {}", error_test_result.output);
            return Ok(false);
        }

        // In a real implementation, we'd analyze:
        // - Exception/error handling coverage
        // - Recovery mechanisms
        // - User-facing error messages

        Ok(true)
    }

    /// Validate a financial optimization goal
    async fn validate_financial_goal(
        &self,
        goal: &OptimizationGoal,
        _branch: &str,
    ) -> Result<bool> {
        info!("Validating financial goal in permissive mode: {}", goal.id);

        // In a real implementation, we'd run:
        // - Financial calculation tests
        // - Audit logs verification
        // - Compliance checks

        // For now, we'll accept if the goal has been properly reviewed
        let has_review = goal
            .implementation_notes
            .as_ref()
            .map(|notes| notes.contains("reviewed"))
            .unwrap_or(false);

        if !has_review {
            warn!("Financial optimization lacks review notes, but accepting in permissive mode");
        } else {
            info!("Financial goal has been reviewed");
        }

        // In permissive mode, we allow all financial goals
        Ok(true)
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

    /// Find the commit pointed to by a branch
    fn find_branch_commit<'a>(
        &self,
        repo: &'a Repository,
        branch_name: &str,
    ) -> Result<Option<git2::Commit<'a>>> {
        let reference_name = format!("refs/heads/{}", branch_name);

        match repo.find_reference(&reference_name) {
            Ok(reference) => {
                // Get the target commit from the reference
                let target_oid = reference
                    .target()
                    .context("Failed to get target from reference")?;

                let commit = repo
                    .find_commit(target_oid)
                    .context("Failed to find commit")?;

                Ok(Some(commit))
            }
            Err(_) => {
                // Branch might not exist yet
                warn!(
                    "Could not find reference for branch '{}'. May be creating a new branch.",
                    branch_name
                );
                Ok(None)
            }
        }
    }

    /// Get the number of active goals
    pub async fn get_active_goal_count(&self) -> Result<usize> {
        let optimization_manager = self.optimization_manager.lock().await;
        Ok(optimization_manager.get_all_goals().len())
    }

    /// List all strategic objectives
    pub async fn list_strategic_objectives(&self) -> Vec<StrategicObjective> {
        if let Some(db_manager) = &self.database_manager {
            match futures::executor::block_on(db_manager.objectives().get_all()) {
                Ok(records) => records
                    .into_iter()
                    .map(|record| record.entity().clone())
                    .collect(),
                Err(e) => {
                    error!("Error retrieving strategic objectives from database: {}", e);
                    Vec::new()
                }
            }
        } else {
            // Fall back to the in-memory objectives from the strategic planning manager
            let planning_manager = self.strategic_planning_manager.lock().await;
            planning_manager.get_plan().objectives.clone()
        }
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

/// Extract a numeric target from a metric string
fn extract_numeric_target(metric: &str) -> Result<f64> {
    // Look for patterns like "X > 80%" or "Y < 5"
    for part in metric.split_whitespace() {
        if let Ok(value) = part
            .trim_end_matches(['%', ',', '.', ':', ';'])
            .parse::<f64>()
        {
            return Ok(value);
        }
    }

    // Default if we can't find a specific target
    warn!("Could not extract numeric target from metric: {}", metric);
    Ok(0.0)
}

/// Extract a percentage from a string
fn extract_percentage(text: &str) -> Option<f64> {
    // Look for a pattern like "25% improvement" or "improved by 30%"
    for part in text.split_whitespace() {
        if part.ends_with('%') {
            if let Ok(value) = part.trim_end_matches('%').parse::<f64>() {
                return Some(value);
            }
        }
    }
    None
}

/// Parse coverage percentage from test output
fn parse_coverage_percentage(output: &str) -> Result<f64> {
    // Look for lines containing "coverage" and a percentage
    for line in output.lines() {
        if line.to_lowercase().contains("coverage") {
            for part in line.split_whitespace() {
                if part.ends_with('%') {
                    if let Ok(value) = part.trim_end_matches('%').parse::<f64>() {
                        return Ok(value);
                    }
                }
            }
        }
    }

    // Default if we can't find coverage information
    warn!("Could not parse coverage percentage from output");
    Ok(0.0)
}
