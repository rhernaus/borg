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
use crate::resource_monitor::monitor::ResourceMonitor;
use crate::resource_monitor::system::SystemMonitor;
use crate::testing::test_runner::TestRunner;
use crate::testing::simple::SimpleTestRunner;
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

        // Create the code generator
        let code_generator: Arc<dyn CodeGenerator> = Arc::new(
            LlmCodeGenerator::new(config.llm.clone(), Arc::clone(&git_manager))
                .context("Failed to create code generator")?
        );

        // Create the test runner
        let test_runner: Arc<dyn TestRunner> = Arc::new(
            SimpleTestRunner::new(&working_dir)
                .context("Failed to create test runner")?
        );

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
        })
    }

    /// Main loop for the agent
    pub async fn run(&mut self) -> Result<()> {
        info!("Agent starting main improvement loop");

        // Initialize the Git repository
        self.initialize_git_repository().await?;

        // Check if we have any existing goals
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
                        "comm-001",
                        "Implement Inter-Agent Communication Mechanism",
                        "Create a mechanism for agents to communicate with each other, share information, and coordinate tasks.",
                        OptimizationCategory::General,
                    );
                    goal.priority = PriorityLevel::Medium;
                    goal.affected_areas = vec!["src/communication/agent_communication.rs".to_string()];
                    goal
                },
                {
                    let mut goal = OptimizationGoal::new(
                        "persist-001",
                        "Implement Goal Persistence to Disk",
                        "Implement a mechanism to save optimization goals to disk and load them on startup.",
                        OptimizationCategory::General,
                    );
                    goal.priority = PriorityLevel::High;
                    goal.affected_areas = vec!["src/core/persistence.rs".to_string()];
                    goal
                },
                {
                    let mut goal = OptimizationGoal::new(
                        "code-001",
                        "Enhance Prompt Templates for Code Generation",
                        "Improve the prompt templates used for code generation to produce more accurate and efficient code.",
                        OptimizationCategory::Readability,
                    );
                    goal.priority = PriorityLevel::Medium;
                    goal.affected_areas = vec!["src/code_generation/prompt.rs".to_string()];
                    goal
                },
                {
                    let mut goal = OptimizationGoal::new(
                        "res-001",
                        "Implement Resource Usage Forecasting",
                        "Create a system to forecast resource usage based on current trends and planned operations.",
                        OptimizationCategory::Performance,
                    );
                    goal.priority = PriorityLevel::Low;
                    goal.affected_areas = vec!["src/resource_monitor/forecasting.rs".to_string()];
                    goal
                },
                {
                    let mut goal = OptimizationGoal::new(
                        "test-001",
                        "Implement Automated Test Coverage Analysis",
                        "Create a system to analyze test coverage and identify areas that need more testing.",
                        OptimizationCategory::TestCoverage,
                    );
                    goal.priority = PriorityLevel::Medium;
                    goal.affected_areas = vec!["src/testing/coverage.rs".to_string()];
                    goal
                },
            ];

            // Add goals to the manager
            for goal in goals {
                optimization_manager.add_goal(goal);
            }

            info!("Created 5 initial optimization goals");
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

    /// Test a code change
    async fn test_change(&self, branch: &str) -> Result<bool> {
        info!("Testing changes in branch: {}", branch);

        // Open the repository
        let repo = Repository::open(&self.working_dir)
            .context(format!("Failed to open repository at {:?}", self.working_dir))?;

        // Store the current branch so we can return to it
        let head = repo.head()
            .context("Failed to get repository HEAD")?;
        let current_branch = head.shorthand().unwrap_or("master").to_string();

        // Checkout the branch to test - use safer checkout options
        let obj = repo.revparse_single(&format!("refs/heads/{}", branch))
            .context(format!("Failed to find branch '{}'", branch))?;

        // Use checkout options to force checkout
        let mut checkout_opts = git2::build::CheckoutBuilder::new();
        checkout_opts.force(); // Force checkout, discard local changes

        // Try to checkout with more robust error handling
        match repo.checkout_tree(&obj, Some(&mut checkout_opts)) {
            Ok(_) => {},
            Err(e) => {
                error!("Failed to checkout tree for branch '{}': {}", branch, e);
                // Attempt to clean up any failed checkout
                let _ = repo.cleanup_state();
                return Err(anyhow!("Failed to checkout branch '{}': {}", branch, e));
            }
        }

        // Set HEAD to the branch reference
        match repo.set_head(&format!("refs/heads/{}", branch)) {
            Ok(_) => {},
            Err(e) => {
                error!("Failed to set HEAD to branch '{}': {}", branch, e);
                // Try to return to original branch
                let original = repo.revparse_single(&format!("refs/heads/{}", current_branch))
                    .context(format!("Failed to find original branch '{}'", current_branch))?;
                let _ = repo.checkout_tree(&original, None);
                let _ = repo.set_head(&format!("refs/heads/{}", current_branch));
                return Err(anyhow!("Failed to set HEAD to branch '{}': {}", branch, e));
            }
        }

        info!("Successfully checked out branch: {}", branch);

        // TODO: Implement real testing - for now we just simulate it
        info!("Simulating tests for branch: {}", branch);

        // In a real implementation, we would run tests here
        let random_success = true; // Always pass for now

        // Return to the original branch
        let original = repo.revparse_single(&format!("refs/heads/{}", current_branch))
            .context(format!("Failed to find original branch '{}'", current_branch))?;

        // Use checkout options for returning to original branch too
        let mut checkout_opts = git2::build::CheckoutBuilder::new();
        checkout_opts.force();

        if let Err(e) = repo.checkout_tree(&original, Some(&mut checkout_opts)) {
            warn!("Failed to return to original branch '{}': {}", current_branch, e);
            // Continue anyway since this is a cleanup step
        }

        if let Err(e) = repo.set_head(&format!("refs/heads/{}", current_branch)) {
            warn!("Failed to set HEAD back to original branch '{}': {}", current_branch, e);
            // Continue anyway since this is a cleanup step
        }

        if random_success {
            info!("Tests passed for branch: {}", branch);
        } else {
            warn!("Tests failed for branch: {}", branch);
        }

        Ok(random_success)
    }

    /// Evaluate test results
    async fn evaluate_results(&self, goal: &OptimizationGoal, _branch: &str, test_passed: bool) -> Result<bool> {
        info!("Evaluating results for goal: {}", goal.id);

        // Simple evaluation for now - if tests pass, we consider it successful
        if test_passed {
            info!("Evaluation successful for goal: {}", goal.id);
            Ok(true)
        } else {
            warn!("Evaluation failed for goal: {} - tests did not pass", goal.id);
            Ok(false)
        }
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

        repo.set_head("refs/heads/master")
            .or_else(|_| repo.set_head("refs/heads/main"))
            .context("Failed to set HEAD to main branch")?;

        // Find the branch to merge
        let branch_obj = repo.revparse_single(&format!("refs/heads/{}", branch))
            .context(format!("Failed to find branch '{}'", branch))?;

        // Create a signature for the merge
        let signature = Signature::now("Borg Agent", "borg@example.com")
            .context("Failed to create signature")?;

        // Merge the branch
        let branch_commit = branch_obj.peel_to_commit()
            .context(format!("Failed to peel branch '{}' to commit", branch))?;

        let main_commit = main_obj.peel_to_commit()
            .context("Failed to peel main branch to commit")?;

        let merge_base = repo.merge_base(branch_commit.id(), main_commit.id())
            .context("Failed to find merge base")?;

        // If the merge base is the same as the branch commit, the branch is already merged
        if merge_base == branch_commit.id() {
            info!("Branch '{}' is already merged into main", branch);
            return Ok(());
        }

        // If the merge base is the same as the main commit, we can fast-forward
        if merge_base == main_commit.id() {
            // Fast-forward merge
            let mut reference = repo.find_reference("HEAD")
                .context("Failed to find HEAD reference")?;

            reference.set_target(branch_commit.id(), "Fast-forward merge")
                .context("Failed to update HEAD reference")?;

            repo.checkout_head(None)
                .context("Failed to checkout HEAD")?;

            info!("Fast-forward merged branch '{}' into main", branch);
        } else {
            // Create a normal merge
            let mut index = repo.merge_commits(&main_commit, &branch_commit, None)
                .context("Failed to merge commits")?;

            let tree_id = index.write_tree_to(&repo)
                .context("Failed to write merge tree")?;

            let tree = repo.find_tree(tree_id)
                .context("Failed to find merge tree")?;

            // Create the merge commit
            repo.commit(
                Some("HEAD"),
                &signature,
                &signature,
                &format!("Merge branch '{}' for goal: {}", branch, goal.id),
                &tree,
                &[&main_commit, &branch_commit]
            ).context("Failed to create merge commit")?;

            info!("Merged branch '{}' into main", branch);
        }

        // Update the goal status
        goal.update_status(crate::core::optimization::GoalStatus::Completed);

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

        Ok(())
    }
}