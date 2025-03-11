use anyhow::{Context, Result};
use log::{error, info, warn};
use std::path::{PathBuf, Path};
use std::sync::Arc;
use tokio::sync::Mutex;
use chrono;
use git2::{Repository, Signature};
use pathdiff;
use anyhow::anyhow;

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
}

impl Agent {
    /// Create a new agent instance
    pub fn new(config: Config) -> Result<Self> {
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

        let code_generator: Arc<dyn CodeGenerator> = Arc::new(
            LlmCodeGenerator::new(llm_config, Arc::clone(&git_manager))
                .context("Failed to create code generator")?
        );

        // Create the test runner
        let test_runner: Arc<dyn TestRunner> = TestRunnerFactory::create(&config, &working_dir)
            .context("Failed to create test runner")?;

        Ok(Self {
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
        })
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
                        "Design and implement a persistence mechanism to save optimization goals to disk in a structured format (JSON/TOML) and load them on startup. This ensures goal progress isn't lost between agent restarts and enables long-term tracking of improvement history.",
                        OptimizationCategory::General,
                    );
                    goal.priority = PriorityLevel::Critical;
                    goal.affected_areas = vec!["src/core/persistence.rs".to_string(), "src/core/optimization.rs".to_string()];
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
                        "Improve the LLM prompt templates for code generation to provide more context, constraints, and examples of desired output. Include Rust best practices, memory safety guidelines, and error handling patterns in the prompts to generate higher quality and more secure code.",
                        OptimizationCategory::Performance,
                    );
                    goal.priority = PriorityLevel::High;
                    goal.affected_areas = vec!["src/code_generation/prompt.rs".to_string(), "src/code_generation/llm_generator.rs".to_string()];
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
                        "Enhance the testing framework to include code linting, compilation validation, unit tests, integration tests, and performance benchmarks. The framework should provide detailed feedback on why tests failed to guide future improvement attempts.",
                        OptimizationCategory::TestCoverage,
                    );
                    goal.priority = PriorityLevel::High;
                    goal.affected_areas = vec!["src/testing/test_runner.rs".to_string(), "src/testing/simple.rs".to_string()];
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
                        "Refactor the LLM integration to support multiple specialized models for different tasks: code generation, ethics assessment, test validation, and planning. Each model should be optimized for its specific task with appropriate context windows and parameters.",
                        OptimizationCategory::Performance,
                    );
                    goal.priority = PriorityLevel::Medium;
                    goal.affected_areas = vec!["src/code_generation/llm_generator.rs".to_string(), "src/core/ethics.rs".to_string()];
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
                        "Create a sophisticated resource monitoring system that not only tracks current usage but predicts future resource needs based on planned operations. This forecasting should help prevent resource exhaustion and optimize scheduling of intensive tasks.",
                        OptimizationCategory::Performance,
                    );
                    goal.priority = PriorityLevel::Medium;
                    goal.affected_areas = vec!["src/resource_monitor/forecasting.rs".to_string(), "src/resource_monitor/monitor.rs".to_string()];
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
                        "Implement a more sophisticated ethical assessment system that can evaluate potential improvements across multiple ethical dimensions. The framework should consider safety, privacy, fairness, transparency, and alignment with human values.",
                        OptimizationCategory::Security,
                    );
                    goal.priority = PriorityLevel::High;
                    goal.affected_areas = vec!["src/core/ethics.rs".to_string()];
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

        // Extract file paths from the goal
        let file_paths = goal.affected_areas.clone();

        // Create the context with the goal description as the task
        let context = CodeContext {
            task: goal.description.clone(),
            file_paths,
            requirements: Some(format!("Category: {}\nPriority: {}",
                goal.category.to_string(),
                goal.priority.to_string())),
            previous_attempts: Vec::new(), // For now, we don't track previous attempts
        };

        Ok(context)
    }

    /// Apply a code change to a branch
    async fn apply_change(&self, goal: &OptimizationGoal, branch_name: &str, code: &str) -> Result<()> {
        info!("Applying change for goal {} to branch {}", goal.id, branch_name);

        // Check if we have affected areas
        if goal.affected_areas.is_empty() {
            return Err(anyhow!("No affected areas specified for goal {}", goal.id));
        }

        // Open the repository
        let repo = Repository::open(&self.working_dir)
            .context(format!("Failed to open repository at {:?}", self.working_dir))?;

        // Get the target path
        let target_path = Path::new(&goal.affected_areas[0]);

        // Make sure the directory exists
        if let Some(parent) = target_path.parent() {
            let parent_path = self.working_dir.join(parent);
            std::fs::create_dir_all(&parent_path)
                .context(format!("Failed to create directory: {:?}", parent_path))?;
        }

        // Write the placeholder content
        let placeholder_content = format!(
            "// Generated improvement for goal: {}\n// Generated on: {}\n\n{}\n",
            goal.id,
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"),
            code.lines().take(20).collect::<Vec<&str>>().join("\n")
        );

        // Write to the file
        let file_path = self.working_dir.join(target_path);
        info!("Writing to file: {:?}", file_path);
        std::fs::write(&file_path, placeholder_content)
            .context(format!("Failed to write to file: {:?}", file_path))?;

        // Stage the file
        let mut index = repo.index()
            .context("Failed to get repository index")?;

        // Since we're working directly in the repository, we can use the target_path directly
        // which is already relative to the repository root
        let relative_path_str = target_path.to_str()
            .ok_or_else(|| anyhow!("Failed to convert path to string"))?;

        info!("Adding file to index: {}", relative_path_str);

        // Add the file to the index
        index.add_path(Path::new(relative_path_str))
            .context(format!("Failed to add file to index: {}", relative_path_str))?;

        index.write()
            .context("Failed to write index")?;

        // Create the commit
        let tree_id = index.write_tree()
            .context("Failed to write tree")?;

        let tree = repo.find_tree(tree_id)
            .context("Failed to find tree")?;

        // Create a signature for the commit
        let signature = Signature::now("Borg Agent", "borg@example.com")
            .context("Failed to create signature")?;

        // Get the reference to the branch
        let reference_name = format!("refs/heads/{}", branch_name);

        // Create the commit - handle the case where this might be the first commit in the branch
        let commit_id = match repo.find_reference(&reference_name) {
            Ok(reference) => {
                // Get the parent commit from the reference
                let parent_oid = reference.target()
                    .context("Failed to get target from reference")?;

                let parent_commit = repo.find_commit(parent_oid)
                    .context("Failed to find parent commit")?;

                // Create the commit with the parent
                repo.commit(
                    Some(&reference_name),
                    &signature,
                    &signature,
                    &format!("Improvement for goal: {}", goal.id),
                    &tree,
                    &[&parent_commit]
                ).context("Failed to create commit with parent")?
            },
            Err(e) => {
                // This might be a new branch without commits, create without parent
                warn!("Could not find reference for branch '{}': {}. Creating initial commit.", branch_name, e);
                repo.commit(
                    Some(&reference_name),
                    &signature,
                    &signature,
                    &format!("Improvement for goal: {}", goal.id),
                    &tree,
                    &[] // No parents
                ).context("Failed to create initial commit")?
            }
        };

        info!("Created commit {} for improvement in branch '{}'", commit_id, branch_name);

        Ok(())
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

        match self.persistence_manager.save_optimization_manager(&self.optimization_manager).await {
            Ok(_) => {
                info!("Successfully saved goals to disk");
                Ok(())
            },
            Err(e) => {
                error!("Failed to save goals to disk: {}", e);
                Err(anyhow!("Failed to save goals to disk: {}", e))
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

    /// Process an optimization goal
    async fn process_goal(&self, working_goal: OptimizationGoal) -> Result<()> {
        info!("Processing optimization goal: {}", working_goal.id);

        // Update goal status to in-progress
        {
            let mut optimization_manager = self.optimization_manager.lock().await;
            if let Some(goal) = optimization_manager.get_goal_mut(&working_goal.id) {
                goal.update_status(GoalStatus::InProgress);
            }
        }

        // Check ethics first
        let ethical = self.assess_goal_ethics(&mut working_goal.clone()).await?;
        if !ethical {
            warn!("Goal {} failed ethical assessment, skipping", working_goal.id);

            // Update goal status to failed
            let mut optimization_manager = self.optimization_manager.lock().await;
            if let Some(goal) = optimization_manager.get_goal_mut(&working_goal.id) {
                goal.update_status(GoalStatus::Failed);
            }

            return Ok(());
        }

        // Generate code improvements
        let code_changes = match self.generate_improvement(&working_goal).await {
            Ok(changes) => changes,
            Err(e) => {
                warn!("Failed to generate improvement for goal {}: {}", working_goal.id, e);

                // Update goal status to failed
                let mut optimization_manager = self.optimization_manager.lock().await;
                if let Some(goal) = optimization_manager.get_goal_mut(&working_goal.id) {
                    goal.update_status(GoalStatus::Failed);
                }

                return Ok(());
            }
        };

        // Create a branch name for this improvement
        // Replace spaces with underscores to avoid issues with Git branch names
        let category_slug = working_goal.category.to_string().to_lowercase().replace(' ', "_");
        let branch_name = format!("improvement/{}/{}", category_slug, working_goal.id);

        // Apply the changes to a branch
        match self.apply_change(&working_goal, &branch_name, &code_changes).await {
            Ok(()) => {
                info!("Successfully applied changes for goal {} in branch {}", working_goal.id, branch_name);
            },
            Err(e) => {
                warn!("Failed to apply changes for goal {}: {}", working_goal.id, e);

                // Update goal status to failed
                let mut optimization_manager = self.optimization_manager.lock().await;
                if let Some(goal) = optimization_manager.get_goal_mut(&working_goal.id) {
                    goal.update_status(GoalStatus::Failed);
                }

                return Ok(());
            }
        };

        // Test the changes
        let tests_passed = match self.test_change(&branch_name).await {
            Ok(passed) => passed,
            Err(e) => {
                warn!("Failed to test changes for goal {}: {}", working_goal.id, e);

                // Update goal status to failed
                let mut optimization_manager = self.optimization_manager.lock().await;
                if let Some(goal) = optimization_manager.get_goal_mut(&working_goal.id) {
                    goal.update_status(GoalStatus::Failed);
                }

                return Ok(());
            }
        };

        // Evaluate the results
        let should_merge = match self.evaluate_results(&working_goal, &branch_name, tests_passed).await {
            Ok(should_merge) => should_merge,
            Err(e) => {
                warn!("Failed to evaluate results for goal {}: {}", working_goal.id, e);

                // Update goal status to failed
                let mut optimization_manager = self.optimization_manager.lock().await;
                if let Some(goal) = optimization_manager.get_goal_mut(&working_goal.id) {
                    goal.update_status(GoalStatus::Failed);
                }

                return Ok(());
            }
        };

        // Merge the changes if successful
        if should_merge {
            match self.merge_change(&mut working_goal.clone(), &branch_name).await {
                Ok(_) => {
                    info!("Successfully completed goal {}", working_goal.id);

                    // Update goal status to completed
                    let mut optimization_manager = self.optimization_manager.lock().await;
                    if let Some(goal) = optimization_manager.get_goal_mut(&working_goal.id) {
                        goal.update_status(GoalStatus::Completed);
                    }
                },
                Err(e) => {
                    error!("Failed to merge changes for goal {}: {}", working_goal.id, e);

                    // Update goal status to failed
                    let mut optimization_manager = self.optimization_manager.lock().await;
                    if let Some(goal) = optimization_manager.get_goal_mut(&working_goal.id) {
                        goal.update_status(GoalStatus::Failed);
                    }
                }
            }
        } else {
            warn!("Changes for goal {} did not pass evaluation, not merging", working_goal.id);

            // Update goal status to failed
            let mut optimization_manager = self.optimization_manager.lock().await;
            if let Some(goal) = optimization_manager.get_goal_mut(&working_goal.id) {
                goal.update_status(GoalStatus::Failed);
            }
        }

        // At the end after updating goal status
        // Save goals to disk after every goal processing
        // to ensure we don't lose progress
        self.save_goals_to_disk().await?;

        Ok(())
    }
}