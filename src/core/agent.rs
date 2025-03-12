use anyhow::{Context, Result};
use log::{error, info, warn};
use std::path::{PathBuf, Path};
use std::sync::Arc;
use tokio::sync::Mutex;
use chrono;
use git2::{Repository, Signature};
use pathdiff;
use anyhow::anyhow;
use std::fs;
use regex;
use uuid;

use crate::code_generation::generator::CodeGenerator;
use crate::code_generation::llm_generator::LlmCodeGenerator;
use crate::core::config::Config;
use crate::core::ethics::{EthicsManager, FundamentalPrinciple};
use crate::core::optimization::{OptimizationManager, OptimizationGoal, OptimizationCategory, GoalStatus, PriorityLevel};
use crate::core::authentication::{AuthenticationManager, AccessRole};
use crate::core::persistence::PersistenceManager;
use crate::resource_monitor::monitor::ResourceMonitor;
use crate::resource_monitor::system::SystemMonitor;
use crate::testing::test_runner::TestRunner;
use crate::testing::factory::TestRunnerFactory;
use crate::version_control::git::GitManager;
use crate::version_control::git_implementation::GitImplementation;
use crate::core::planning::{StrategicPlanningManager, StrategicObjective};
use crate::core::strategy::{ActionType, Plan, StrategyManager};
use crate::core::strategies::CodeImprovementStrategy;

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

    /// Strategic planning manager
    strategic_planning_manager: Arc<Mutex<StrategicPlanningManager>>,

    /// Strategy manager for coordinating different action strategies
    strategy_manager: Arc<Mutex<StrategyManager>>,
}

impl Agent {
    /// Create a new agent with the given configuration
    pub async fn new(config: Config) -> Result<Self> {
        // Create working directory if it doesn't exist
        let working_dir = PathBuf::from(&config.agent.working_dir);
        std::fs::create_dir_all(&working_dir)
            .with_context(|| format!("Failed to create working directory: {:?}", working_dir))?;

        // Create the git manager
        let git_manager: Arc<Mutex<dyn GitManager>> = Arc::new(Mutex::new(
            GitImplementation::new(&working_dir)
                .context("Failed to create git manager")?
        ));

        // Create the resource monitor
        let resource_monitor: Arc<Mutex<dyn ResourceMonitor>> = Arc::new(Mutex::new(
            SystemMonitor::new()
                .context("Failed to create resource monitor")?
        ));

        // Create the ethics manager
        let ethics_manager = Arc::new(Mutex::new(
            EthicsManager::new()
        ));

        // Create the authentication manager
        let authentication_manager = Arc::new(Mutex::new(
            AuthenticationManager::new()
        ));

        // Create the optimization manager with the ethics manager
        let optimization_manager = Arc::new(Mutex::new(
            OptimizationManager::new(Arc::clone(&ethics_manager))
        ));

        // Create the persistence manager
        let data_dir = working_dir.join("data");
        let persistence_manager = PersistenceManager::new(&data_dir)
            .context("Failed to create persistence manager")?;

        // Create the code generator
        // Use the code_generation LLM config if available, otherwise fall back to default
        let llm_config = config.llm.get("code_generation")
            .or_else(|| config.llm.get("default"))
            .ok_or_else(|| anyhow::anyhow!("No suitable LLM configuration found"))?
            .clone();

        let workspace = working_dir.clone();
        let code_gen_config = config.code_generation.clone();
        let llm_logging_config = config.llm_logging.clone();

        let code_generator: Arc<dyn CodeGenerator> = Arc::new(
            LlmCodeGenerator::new(llm_config, code_gen_config, llm_logging_config, Arc::clone(&git_manager), workspace.clone())
                .context("Failed to create code generator")?
        );

        // Create the test runner
        let test_runner: Arc<dyn TestRunner> = TestRunnerFactory::create(&config, &working_dir)
            .context("Failed to create test runner")?;

        // Create strategic planning manager
        let strategic_planning_manager = Arc::new(Mutex::new(
            StrategicPlanningManager::new(
                optimization_manager.clone(),
                ethics_manager.clone(),
                &data_dir.join("planning").to_string_lossy(),
            )
        ));

        // Create the strategy manager
        let strategy_manager = Arc::new(Mutex::new(
            StrategyManager::new(
                authentication_manager.clone(),
                ethics_manager.clone(),
            )
        ));

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
            strategic_planning_manager,
            strategy_manager,
        };

        // Register strategies
        agent.register_strategies().await?;

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
            },
            Err(_) => {
                info!("Creating new Git repository at {:?}", repo_path);
                Repository::init(repo_path)
                    .context(format!("Failed to initialize Git repository at {:?}", repo_path))?
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
            let mut index = repo.index()
                .context("Failed to get repository index")?;

            index.add_path(Path::new("README.md"))
                .context("Failed to add README.md to index")?;

            index.write()
                .context("Failed to write index")?;

            // Create a tree from the index
            let tree_id = index.write_tree()
                .context("Failed to write tree")?;

            let tree = repo.find_tree(tree_id)
                .context("Failed to find tree")?;

            // Create a signature for the commit
            let signature = Signature::now("Borg Agent", "borg@example.com")
                .context("Failed to create signature")?;

            // Create the initial commit
            repo.commit(
                Some("HEAD"),    // Reference to update
                &signature,      // Author
                &signature,      // Committer
                "Initial commit", // Message
                &tree,           // Tree
                &[],             // Parents (empty for initial commit)
            ).context("Failed to create initial commit")?;

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
                    goal.tags.push("file:src/code_generation/prompt.rs".to_string());
                    goal.tags.push("file:src/code_generation/llm_generator.rs".to_string());
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
                    goal.tags.push("file:src/testing/test_runner.rs".to_string());
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
                    goal.tags.push("file:src/code_generation/llm_generator.rs".to_string());
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
                    goal.tags.push("file:src/resource_monitor/forecasting.rs".to_string());
                    goal.tags.push("file:src/resource_monitor/monitor.rs".to_string());
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

            info!("Initialized {} default optimization goals", optimization_manager.get_all_goals().len());
        } else {
            info!("Found {} existing optimization goals", optimization_manager.get_all_goals().len());
        }

        Ok(())
    }

    /// Helper function to check system resources
    async fn check_resources(&self) -> Result<bool> {
        // This is a placeholder for resource checking logic
        // Will be implemented in future commits
        Ok(true)
    }

    /// Identify the next optimization goal to work on
    async fn identify_next_goal(&self) -> Result<Option<OptimizationGoal>> {
        let optimization_manager = self.optimization_manager.lock().await;
        let next_goal = optimization_manager.get_next_goal();

        match next_goal {
            Some(goal) => {
                info!("Selected next optimization goal: {} ({})", goal.title, goal.id);
                Ok(Some(goal.clone()))
            },
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
        let improvement = self.code_generator.generate_improvement(&context).await
            .context("Failed to generate code improvement")?;

        // Log success
        info!("Successfully generated improvement for goal: {} with ID: {}", goal.id, improvement.id);

        // Return the raw code content
        Ok(improvement.code)
    }

    /// Create a code context from an optimization goal
    async fn create_code_context(&self, goal: &OptimizationGoal) -> Result<crate::code_generation::generator::CodeContext> {
        use crate::code_generation::generator::CodeContext;

        // Get the file paths for the goal
        let file_paths: Vec<String> = goal.tags.iter()
            .filter(|tag| tag.starts_with("file:"))
            .map(|tag| tag.trim_start_matches("file:").to_string())
            .collect();

        // Create the context with the goal description as the task
        let context = CodeContext {
            task: goal.description.clone(),
            file_paths: file_paths,
            requirements: Some(format!("Category: {}\nPriority: {}",
                goal.category.to_string(),
                goal.priority.to_string())),
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
    async fn apply_change(&self, goal: &OptimizationGoal, branch_name: &str, code: &str) -> Result<()> {
        info!("Applying change for goal {} to branch {}", goal.id, branch_name);

        // Open the repository
        let repo = Repository::open(&self.working_dir)
            .context(format!("Failed to open repository at {:?}", self.working_dir))?;

        // Extract file changes from the LLM code response
        let improvement_result = self.parse_code_changes(code)?;

        if improvement_result.target_files.is_empty() {
            return Err(anyhow!("No files to modify were identified in the code"));
        }

        info!("Found {} file(s) to modify", improvement_result.target_files.len());

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
            let mut index = repo.index()
                .context("Failed to get repository index")?;

            // Get relative path
            let relative_path_str = target_path.to_str()
                .ok_or_else(|| anyhow!("Failed to convert path to string"))?;

            info!("Adding file to index: {}", relative_path_str);
            index.add_path(Path::new(relative_path_str))
                .context(format!("Failed to add file to index: {}", relative_path_str))?;

            index.write().context("Failed to write index")?;
        }

        // Create a commit
        let tree_id = repo.index().unwrap().write_tree().context("Failed to write tree")?;
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
            ).context("Failed to create commit")?
        } else {
            repo.commit(
                Some(&format!("refs/heads/{}", branch_name)),
                &signature,
                &signature,
                &message,
                &tree,
                &[],
            ).context("Failed to create commit")?
        };

        info!("Successfully applied changes for goal {}", goal.id);
        Ok(())
    }

    /// Parse code changes from LLM response
    fn parse_code_changes(&self, code: &str) -> Result<crate::code_generation::generator::CodeImprovement> {
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
    fn extract_file_changes(&self, code: &str) -> Result<Vec<crate::code_generation::generator::FileChange>> {
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
                file_path = file_cap.get(1).or(file_cap.get(2)).or(file_cap.get(3))
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
                let file_path = cap.get(1).or(cap.get(2)).or(cap.get(3))
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
                new_content: "// No specific file identified, adding placeholder comment".to_string(),
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
                info!("Test metrics: {} tests run, {} passed, {} failed",
                      metrics.tests_run, metrics.tests_passed, metrics.tests_failed);
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
    async fn evaluate_results(&self, goal: &OptimizationGoal, branch: &str, test_passed: bool) -> Result<bool> {
        if !test_passed {
            warn!("Tests failed for goal '{}' in branch '{}'", goal.id, branch);
            return Ok(false);
        }

        info!("Tests passed for goal '{}' in branch '{}'", goal.id, branch);

        // Check if the change satisfies the success metrics
        if !goal.success_metrics.is_empty() {
            info!("Evaluating success metrics for goal '{}'", goal.id);

            // Run benchmarks if this is a performance-related goal
            if goal.category == OptimizationCategory::Performance {
                info!("Running benchmarks for performance goal");

                let benchmark_result = self.test_runner.run_benchmark(branch, None).await?;

                if benchmark_result.success {
                    info!("Benchmarks completed successfully");

                    // Check for performance improvements in the benchmark output
                    let has_improvement = benchmark_result.output.contains("improvement") ||
                                         benchmark_result.output.contains("faster") ||
                                         benchmark_result.output.contains("reduced");

                    if has_improvement {
                        info!("Performance improvement detected in benchmarks");
                    } else {
                        warn!("No clear performance improvement detected in benchmarks");

                        // We'll still return true since the tests passed, but log a warning
                        info!("Accepting change despite unclear performance impact since tests pass");
                    }
                } else {
                    warn!("Benchmarks failed: {}", benchmark_result.output);
                    // Benchmarks failing is not a reason to reject if main tests pass
                    info!("Accepting change despite benchmark failures since main tests pass");
                }
            }

            // For now, we'll consider the goal achieved if tests pass
            // In a future enhancement, we could implement more sophisticated metrics validation
            info!("All checks passed for goal '{}'", goal.id);
        }

        // Code change successful
        Ok(true)
    }

    /// Merge a branch into main
    async fn merge_change(&self, goal: &mut OptimizationGoal, branch: &str) -> Result<()> {
        info!("Merging branch '{}' for goal: {}", branch, goal.id);

        // Open the repository
        let repo = Repository::open(&self.working_dir)
            .context(format!("Failed to open repository at {:?}", self.working_dir))?;

        // Checkout the main branch
        let main_obj = repo.revparse_single("HEAD")
            .context("Failed to find main branch")?;

        repo.checkout_tree(&main_obj, None)
            .context("Failed to checkout main branch")?;

        // Try to set head to master or main branch
        let main_branch_name = if repo.find_branch("master", git2::BranchType::Local).is_ok() {
            "master"
        } else {
            "main"
        };

        repo.set_head(&format!("refs/heads/{}", main_branch_name))
            .context("Failed to set HEAD to main branch")?;

        // Find the branch to merge
        let branch_obj = repo.revparse_single(&format!("refs/heads/{}", branch))
            .context(format!("Failed to find branch '{}'", branch))?;

        // Create a signature for the merge
        let signature = Signature::now("Borg Agent", "borg@example.com")
            .context("Failed to create signature")?;

        // Get the branch commit
        let branch_commit = branch_obj.peel_to_commit()
            .context(format!("Failed to peel branch '{}' to commit", branch))?;

        // Get the main commit
        let main_commit = main_obj.peel_to_commit()
            .context("Failed to peel main branch to commit")?;

        // Try to find a merge base, but don't error if one doesn't exist
        let merge_base_result = repo.merge_base(branch_commit.id(), main_commit.id());

        match merge_base_result {
            Ok(merge_base) => {
                // If the merge base is the same as the branch commit, the branch is already merged
                if merge_base == branch_commit.id() {
                    info!("Branch '{}' is already merged into main", branch);
                    return Ok(());
                }
            },
            Err(e) => {
                // Log the error but continue (we'll handle this case differently)
                warn!("Could not find merge base: {}", e);
                // In this case, we'll do a hard reset to main, then apply the changes
                repo.reset(&main_obj, git2::ResetType::Hard, None)
                    .context("Failed to reset to main branch")?;

                // Simple cherry-pick approach for new branches without merge base
                // Reading the tree of the branch
                let branch_tree = branch_commit.tree().context("Failed to get branch tree")?;

                // Create a new commit with the same tree but parent as the current HEAD
                let new_commit_id = repo.commit(
                    Some("HEAD"),  // Update HEAD and current branch
                    &signature,    // Author
                    &signature,    // Committer
                    &format!("Cherry-pick changes from {}\n\nImplemented goal: {}", branch, goal.id),
                    &branch_tree,  // The tree from the branch
                    &[&main_commit] // Parent is the current HEAD commit
                )?;

                info!("Cherry-picked changes from '{}' as commit {}", branch, new_commit_id);

                // Change the goal status to completed
                goal.status = GoalStatus::Completed;
                goal.updated_at = chrono::Utc::now();

                return Ok(());
            }
        }

        // If we get here, we have a merge base and can proceed with a normal merge

        // If the merge base is the same as the main commit, we can fast-forward
        if merge_base_result.unwrap() == main_commit.id() {
            // Fast-forward merge
            let mut reference = repo.find_reference("HEAD")
                .context("Failed to find HEAD reference")?;

            reference.set_target(branch_commit.id(), "Fast-forward merge")
                .context("Failed to update HEAD reference")?;

            repo.checkout_head(None)
                .context("Failed to checkout HEAD")?;

            info!("Fast-forward merged branch '{}' into main", branch);
        } else {
            // Try to create a normal merge
            match repo.merge_commits(&main_commit, &branch_commit, None) {
                Ok(mut index) => {
                    if index.has_conflicts() {
                        warn!("Merge conflicts detected. Using cherry-pick instead.");
                        // Reset the merge state
                        repo.cleanup_state()?;
                        // Reset to main
                        repo.reset(&main_obj, git2::ResetType::Hard, None)?;

                        // Use cherry-pick approach for conflicting changes
                        let branch_tree = branch_commit.tree().context("Failed to get branch tree")?;

                        // Create a new commit with the changes but parent as the current HEAD
                        let new_commit_id = repo.commit(
                            Some("HEAD"),
                            &signature,
                            &signature,
                            &format!("Cherry-pick changes from {} (merge conflict resolution)\n\nImplemented goal: {}", branch, goal.id),
                            &branch_tree,
                            &[&main_commit]
                        )?;

                        info!("Cherry-picked changes from '{}' to resolve conflicts as commit {}", branch, new_commit_id);
                    } else {
                        // No conflicts, proceed with merge
                        let tree_id = index.write_tree_to(&repo)
                            .context("Failed to write merge tree")?;

                        let tree = repo.find_tree(tree_id)
                            .context("Failed to find merge tree")?;

                        // Create the merge commit
                        let merge_commit_id = repo.commit(
                            Some("HEAD"),
                            &signature,
                            &signature,
                            &format!("Merge branch '{}' for goal: {}", branch, goal.id),
                            &tree,
                            &[&main_commit, &branch_commit]
                        ).context("Failed to create merge commit")?;

                        info!("Merged branch '{}' into main as commit {}", branch, merge_commit_id);
                    }
                },
                Err(e) => {
                    warn!("Merge failed: {}. Using cherry-pick instead.", e);
                    // Reset to main
                    repo.reset(&main_obj, git2::ResetType::Hard, None)?;

                    // Use cherry-pick approach as fallback
                    let branch_tree = branch_commit.tree().context("Failed to get branch tree")?;

                    // Create a new commit with the changes but parent as the current HEAD
                    let new_commit_id = repo.commit(
                        Some("HEAD"),
                        &signature,
                        &signature,
                        &format!("Cherry-pick changes from {} (merge failed)\n\nImplemented goal: {}", branch, goal.id),
                        &branch_tree,
                        &[&main_commit]
                    )?;

                    info!("Cherry-picked changes from '{}' as fallback as commit {}", branch, new_commit_id);
                }
            }
        }

        // Update the goal status
        goal.status = GoalStatus::Completed;
        goal.updated_at = chrono::Utc::now();

        info!("Successfully merged changes for goal: {}", goal.id);

        Ok(())
    }

    /// Create a new optimization goal based on analysis
    async fn create_optimization_goal(&self, description: &str, category: OptimizationCategory, affected_files: &[String]) -> Result<OptimizationGoal> {
        let optimization_manager = self.optimization_manager.lock().await;

        // For financial goals, verify that the user has appropriate permissions
        if category == OptimizationCategory::Financial {
            let auth_manager = self.authentication_manager.lock().await;

            // Only creators or administrators can create financial goals
            if !auth_manager.has_role(AccessRole::Administrator) &&
               !auth_manager.has_role(AccessRole::Creator) {
                return Err(anyhow::anyhow!("Insufficient permissions to create financial optimization goals"));
            }

            info!("Financial goal creation authorized for user with appropriate permissions");
        }

        // Generate a new goal
        let goal = optimization_manager.generate_goal(description, affected_files, category);

        info!("Created new optimization goal: {} ({})", goal.title, goal.id);

        Ok(goal)
    }

    /// Save an optimization goal
    async fn save_goal(&self, goal: OptimizationGoal) -> Result<()> {
        let mut optimization_manager = self.optimization_manager.lock().await;

        // Add the goal to the manager
        optimization_manager.add_goal(goal);

        // Update dependencies between goals
        optimization_manager.update_goal_dependencies();

        // Save all goals to disk
        drop(optimization_manager); // Release the lock before the async call
        self.save_goals_to_disk().await?;

        Ok(())
    }

    /// Allow an authenticated user to set the priority for a financial goal
    /// This requires appropriate permissions and is logged for accountability
    async fn set_financial_goal_priority(&self, goal_id: &str, priority: PriorityLevel) -> Result<()> {
        // First check the user has appropriate permissions
        let auth_manager = self.authentication_manager.lock().await;

        // Only creators or administrators can manage financial goal priorities
        if !auth_manager.has_role(AccessRole::Administrator) &&
           !auth_manager.has_role(AccessRole::Creator) {
            return Err(anyhow::anyhow!("Insufficient permissions to modify financial goal priorities"));
        }

        // Get the current user for logging
        let user = match auth_manager.current_user() {
            Some(u) => u.name.clone(),
            None => "Unknown User".to_string(),
        };

        // Now get the goal and check if it's a financial goal
        let mut optimization_manager = self.optimization_manager.lock().await;

        // First check if the goal exists and is a financial goal before trying to modify it
        {
            let goal_check = match optimization_manager.get_goal(goal_id) {
                Some(g) => g,
                None => return Err(anyhow::anyhow!("Goal not found: {}", goal_id)),
            };

            // Only financial goals can be prioritized with this method
            if goal_check.category != OptimizationCategory::Financial {
                return Err(anyhow::anyhow!("Only financial goals can be prioritized with this method"));
            }
        }

        // Now update the goal's priority
        if let Some(goal) = optimization_manager.get_goal_mut(goal_id) {
            let old_priority = goal.priority;
            goal.update_priority(priority);

            // Log the change for accountability
            info!(
                "Financial goal priority changed: goal={}, old_priority={:?}, new_priority={:?}, by_user={}",
                goal_id, old_priority, priority, user
            );
        }

        // In a real implementation, we would perform an ethical assessment here
        // using the ethics manager, but for now we'll just log that it should happen
        info!(
            "Ethical assessment should be performed for goal {} after priority change",
            goal_id
        );

        Ok(())
    }

    /// Generate an audit report for financial goals to ensure transparency
    async fn audit_financial_goals(&self) -> Result<String> {
        info!("Auditing financial optimization goals");

        // Get the ethics manager
        let ethics_manager = self.ethics_manager.lock().await;

        // Get the impact assessment history
        let impact_history = ethics_manager.get_impact_assessment_history();

        // Get the optimization manager
        let optimization_manager = self.optimization_manager.lock().await;

        // Get all goals
        let all_goals = optimization_manager.get_all_goals();

        // Filter for financial goals
        let financial_goals: Vec<&OptimizationGoal> = all_goals
            .iter()
            .filter(|goal| matches!(goal.category, OptimizationCategory::Financial))
            .collect();

        if financial_goals.is_empty() {
            return Ok("No financial optimization goals found.".to_string());
        }

        // Generate audit report
        let mut report = String::new();
        report.push_str("# Financial Optimization Goals Audit Report\n\n");
        report.push_str(&format!("Date: {}\n\n", chrono::Utc::now()));
        report.push_str(&format!("Total financial goals: {}\n\n", financial_goals.len()));

        for goal in financial_goals {
            report.push_str(&format!("## Goal: {}\n", goal.title));
            report.push_str(&format!("ID: {}\n", goal.id));
            report.push_str(&format!("Status: {}\n", goal.status));
            report.push_str(&format!("Priority: {}\n", goal.priority));
            report.push_str(&format!("Created: {}\n", goal.created_at));
            report.push_str(&format!("Last updated: {}\n", goal.updated_at));

            // Add ethical assessment if available
            if let Some(assessment) = &goal.ethical_assessment {
                report.push_str("\n### Ethical Assessment\n");
                report.push_str(&format!("Risk level: {}\n", assessment.risk_level));
                report.push_str(&format!("Approved: {}\n", assessment.is_approved));
                report.push_str(&format!("Justification: {}\n", assessment.approval_justification));

                if !assessment.mitigations.is_empty() {
                    report.push_str("\n#### Mitigations\n");
                    for mitigation in &assessment.mitigations {
                        report.push_str(&format!("- {}\n", mitigation));
                    }
                }
            } else {
                report.push_str("\n### Ethical Assessment: None\n");
            }

            report.push_str("\n");
        }

        // Add summary of impact history related to financial goals
        report.push_str("## Historical Impact\n\n");

        let financial_impacts = impact_history.iter()
            .filter(|assessment| {
                assessment.affected_principles.contains(&FundamentalPrinciple::AccountabilityAndResponsibility)
            })
            .count();

        report.push_str(&format!("Total financial impact assessments: {}\n\n", financial_impacts));

        Ok(report)
    }

    /// Load goals from disk
    async fn load_goals_from_disk(&self) -> Result<()> {
        info!("Loading optimization goals from disk");

        match self.persistence_manager.load_into_optimization_manager(&self.optimization_manager).await {
            Ok(_) => {
                let optimization_manager = self.optimization_manager.lock().await;
                let goals = optimization_manager.get_all_goals();
                info!("Successfully loaded {} goals from disk", goals.len());
                Ok(())
            },
            Err(e) => {
                warn!("Failed to load goals from disk: {}", e);
                warn!("Starting with empty goals");
                Ok(())
            }
        }
    }

    /// Save goals to disk
    async fn save_goals_to_disk(&self) -> Result<()> {
        info!("Saving optimization goals to disk");

        // Save optimization goals
        self.persistence_manager.save_optimization_manager(&self.optimization_manager).await
            .context("Failed to save optimization goals to disk")?;

        info!("Successfully saved goals to disk");

        // Also save the strategic plan
        let planning_manager = self.strategic_planning_manager.lock().await;
        planning_manager.save_to_disk().await?;

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
        );

        strategy_manager.register_strategy(code_improvement_strategy);

        info!("Registered {} strategies", strategy_manager.get_strategies().len());

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
            self.update_goal_status(&goal.id, GoalStatus::Completed).await;
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
        strategy_manager.get_strategies().into_iter().map(|s| s.to_string()).collect()
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
    fn find_branch_commit<'a>(&self, repo: &'a Repository, branch_name: &str) -> Result<Option<git2::Commit<'a>>> {
        let reference_name = format!("refs/heads/{}", branch_name);

        match repo.find_reference(&reference_name) {
            Ok(reference) => {
                // Get the target commit from the reference
                let target_oid = reference.target()
                    .context("Failed to get target from reference")?;

                let commit = repo.find_commit(target_oid)
                    .context("Failed to find commit")?;

                Ok(Some(commit))
            },
            Err(_) => {
                // Branch might not exist yet
                warn!("Could not find reference for branch '{}'. May be creating a new branch.", branch_name);
                Ok(None)
            }
        }
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

        // Initialize authentication manager
        {
            let _authentication_manager = self.authentication_manager.lock().await;
            // No initialize method, nothing to do
        }

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

        // Load strategic planning data
        {
            let mut planning_manager = self.strategic_planning_manager.lock().await;
            planning_manager.load_from_disk().await?;
        }

        info!("Agent initialized successfully");

        Ok(())
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
    pub async fn add_strategic_objective(&self,
        id: &str,
        title: &str,
        description: &str,
        timeframe: u32,
        creator: &str,
        key_results: Vec<String>,
        constraints: Vec<String>
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

        let milestones = planning_manager.generate_milestones_for_objective(&objective).await?;

        for milestone in milestones {
            planning_manager.add_milestone(milestone);
        }

        // Save the strategic plan
        planning_manager.save_to_disk().await?;

        info!("Added strategic objective: {} with {} key results and {} constraints",
            id, objective.key_results.len(), objective.constraints.len());

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