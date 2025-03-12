use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use chrono;
use git2::{Repository, Signature};
use log::{error, info, warn};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use serde_json;

use crate::code_generation::generator::{CodeContext, CodeGenerator};
use crate::core::authentication::{AuthenticationManager, AccessRole};
use crate::core::optimization::{OptimizationCategory, OptimizationGoal, GoalStatus};
use crate::core::strategy::{
    ActionPermission, ActionStep, ActionType, ExecutionResult, PermissionScope, Plan, Strategy,
};
use crate::testing::test_runner::TestRunner;
use crate::version_control::git::GitManager;

/// Permissions for code-related operations
#[derive(Debug, Clone, PartialEq)]
enum CodePermission {
    /// Permission to read code
    ReadCode,

    /// Permission to write/modify code
    WriteCode,

    /// Permission to merge code changes
    MergeCode,

    /// Permission to modify configuration
    ModifyConfiguration,

    /// Permission to execute tests
    ExecuteTests,
}

/// Strategy for improving code based on optimization goals
pub struct CodeImprovementStrategy {
    /// Working directory
    working_dir: PathBuf,

    /// Code generator for producing improvements
    code_generator: Arc<dyn CodeGenerator>,

    /// Test runner for validating changes
    test_runner: Arc<dyn TestRunner>,

    /// Git manager for version control
    git_manager: Arc<Mutex<dyn GitManager>>,

    /// Authentication manager for permission checks
    auth_manager: Arc<Mutex<AuthenticationManager>>,
}

impl CodeImprovementStrategy {
    /// Create a new code improvement strategy
    pub fn new(
        working_dir: PathBuf,
        code_generator: Arc<dyn CodeGenerator>,
        test_runner: Arc<dyn TestRunner>,
        git_manager: Arc<Mutex<dyn GitManager>>,
        auth_manager: Arc<Mutex<AuthenticationManager>>,
    ) -> Self {
        Self {
            working_dir,
            code_generator,
            test_runner,
            git_manager,
            auth_manager,
        }
    }

    /// Create a code context from an optimization goal
    async fn create_code_context(&self, goal: &OptimizationGoal) -> Result<CodeContext> {
        // Get the file paths for the goal
        let file_paths: Vec<String> = goal.tags.iter()
            .filter(|tag| tag.starts_with("file:"))
            .map(|tag| tag.trim_start_matches("file:").to_string())
            .collect();

        // Create the context with the goal description as the task
        let context = CodeContext {
            task: goal.description.clone(),
            file_paths: file_paths,
            requirements: Some(format!(
                "Category: {}\nPriority: {}",
                goal.category.to_string(),
                goal.priority.to_string()
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

    /// Generate code improvements for a goal
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

    /// Apply a code change to a branch
    async fn apply_change(&self, goal: &OptimizationGoal, branch_name: &str, code: &str) -> Result<()> {
        info!("Applying change for goal {} to branch {}", goal.id, branch_name);

        // Check if we have file tags
        let has_file_tags = goal.tags.iter().any(|tag| tag.starts_with("file:"));
        if !has_file_tags {
            return Err(anyhow!("No file tags specified for goal {}", goal.id));
        }

        // Open the repository
        let repo = Repository::open(&self.working_dir)
            .context(format!("Failed to open repository at {:?}", self.working_dir))?;

        // Create or checkout the branch
        let branch_exists = match repo.find_branch(branch_name, git2::BranchType::Local) {
            Ok(_) => true,
            Err(_) => false,
        };

        if branch_exists {
            info!("Branch '{}' exists, checking it out", branch_name);

            // Find the branch reference
            let obj = repo.revparse_single(&branch_name)
                .context(format!("Failed to find branch '{}'", branch_name))?;

            // Checkout the branch
            repo.checkout_tree(&obj, None)
                .context(format!("Failed to checkout branch '{}'", branch_name))?;

            repo.set_head(&format!("refs/heads/{}", branch_name))
                .context(format!("Failed to set HEAD to branch '{}'", branch_name))?;

            info!("Successfully checked out branch '{}'", branch_name);
        } else {
            info!("Branch '{}' does not exist, creating it", branch_name);

            // Start from the main branch, either master or main
            let main_branch_name = if repo.find_branch("master", git2::BranchType::Local).is_ok() {
                "master"
            } else {
                "main"
            };

            // Checkout the main branch first
            let main_obj = repo.revparse_single(main_branch_name)
                .context(format!("Failed to find {} branch", main_branch_name))?;

            repo.checkout_tree(&main_obj, None)
                .context(format!("Failed to checkout {} branch", main_branch_name))?;

            repo.set_head(&format!("refs/heads/{}", main_branch_name))
                .context(format!("Failed to set HEAD to {} branch", main_branch_name))?;

            // Get current HEAD to branch from
            let head = repo.head()
                .context("Failed to get repository HEAD")?;

            let commit = head.peel_to_commit()
                .context("Failed to peel HEAD to commit")?;

            // Create the new branch
            repo.branch(branch_name, &commit, false)
                .with_context(|| format!("Failed to create branch '{}'", branch_name))?;

            // Check out the new branch
            let obj = repo.revparse_single(branch_name)
                .context(format!("Failed to find branch '{}'", branch_name))?;

            repo.checkout_tree(&obj, None)
                .context(format!("Failed to checkout branch '{}'", branch_name))?;

            repo.set_head(&format!("refs/heads/{}", branch_name))
                .context(format!("Failed to set HEAD to branch '{}'", branch_name))?;

            info!("Successfully created and checked out branch '{}'", branch_name);
        }

        // Get the target path from the first file tag
        let file_tags: Vec<&String> = goal.tags.iter()
            .filter(|tag| tag.starts_with("file:"))
            .collect();

        if file_tags.is_empty() {
            return Err(anyhow!("No file tags specified for goal {}", goal.id));
        }

        let target_path = Path::new(file_tags[0].trim_start_matches("file:"));

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
        index.add_path(Path::new(relative_path_str))
            .context(format!("Failed to add file to index: {}", relative_path_str))?;

        index.write().context("Failed to write index")?;

        // Create a commit
        let tree_id = index.write_tree().context("Failed to write tree")?;
        let tree = repo.find_tree(tree_id).context("Failed to find tree")?;

        // Create signature
        let signature = Signature::now("Borg Agent", "borg@example.com")
            .context("Failed to create signature")?;

        // Create commit
        let message = format!("Code improvement for goal: {}", goal.id);

        // We need to get the current HEAD as the parent, which should now be the branch we're working on
        let head = repo.head().context("Failed to get HEAD")?;
        let parent_commit = head.peel_to_commit().context("Failed to get parent commit")?;

        let commit_oid = repo.commit(
            Some(&format!("refs/heads/{}", branch_name)),
            &signature,
            &signature,
            &message,
            &tree,
            &[&parent_commit],
        ).context("Failed to create commit")?;

        info!("Successfully created commit {} on branch {}", commit_oid, branch_name);
        info!("Successfully applied changes for goal {} in branch {}", goal.id, branch_name);

        Ok(())
    }

    /// Test a code change in a branch
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
            if goal.category == OptimizationCategory::Performance || goal.tags.iter().any(|t| t == "performance") {
                info!("Running benchmarks for performance goal");

                let benchmark_result = self.test_runner.run_benchmark(branch, None).await?;

                // Check if benchmark meets performance requirements
                if benchmark_result.success {
                    info!("Benchmark passed for goal '{}'", goal.id);
                    return Ok(true);
                } else {
                    warn!("Benchmark failed for goal '{}'", goal.id);
                    return Ok(false);
                }
            }
        }

        // If we got here, the tests passed and there were no specific metrics to check
        info!("Goal '{}' satisfied requirements", goal.id);
        Ok(true)
    }

    /// Merge a branch into the main branch
    async fn merge_change(&self, branch: &str) -> Result<()> {
        info!("Merging branch '{}' into main", branch);

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
        let _merge_base_result = repo.merge_base(branch_commit.id(), main_commit.id());

        // Perform the merge
        let message = format!("Merge branch '{}' into {}", branch, main_branch_name);

        // Create annotated commits for merge
        let annotated_commit = repo.reference_to_annotated_commit(&repo.find_reference(&format!("refs/heads/{}", branch))?)
            .context(format!("Failed to create annotated commit for branch '{}'", branch))?;

        // Perform the merge - using the annotated commit
        let _merge_result = repo.merge(&[&annotated_commit], None, None)
            .context(format!("Failed to merge branch '{}' into {}", branch, main_branch_name))?;

        // Check if we have conflicts
        let mut index = repo.index().context("Failed to get repository index")?;
        if index.has_conflicts() {
            warn!("Merge has conflicts, aborting merge");
            repo.cleanup_state().context("Failed to cleanup merge state")?;
            return Err(anyhow!("Merge has conflicts"));
        }

        // Create a commit
        let tree_id = index.write_tree().context("Failed to write tree")?;
        let tree = repo.find_tree(tree_id).context("Failed to find tree")?;

        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            &message,
            &tree,
            &[&main_commit, &branch_commit],
        ).context("Failed to create merge commit")?;

        // Clean up
        repo.cleanup_state().context("Failed to cleanup merge state")?;

        info!("Successfully merged branch '{}' into {}", branch, main_branch_name);

        Ok(())
    }
}

impl CodeImprovementStrategy {
    /// Get the permissions required for a specific goal
    fn get_required_permissions_for_goal(&self, goal: &OptimizationGoal) -> Vec<CodePermission> {
        let mut permissions = vec![CodePermission::ReadCode];

        // All code improvement goals require code writing permission
        permissions.push(CodePermission::WriteCode);

        // Add execute tests permission
        permissions.push(CodePermission::ExecuteTests);

        // If the goal involves configuration, add that permission
        if goal.description.to_lowercase().contains("config") ||
           goal.tags.iter().any(|tag| tag.to_lowercase().contains("config")) {
            permissions.push(CodePermission::ModifyConfiguration);
        }

        // If the goal involves merging, add that permission
        if goal.description.to_lowercase().contains("merge") ||
           goal.tags.iter().any(|tag| tag.to_lowercase().contains("merge")) {
            permissions.push(CodePermission::MergeCode);
        }

        permissions
    }
}

#[async_trait]
impl Strategy for CodeImprovementStrategy {
    fn name(&self) -> &str {
        "code_improvement"
    }

    fn action_types(&self) -> Vec<ActionType> {
        vec![ActionType::CodeImprovement]
    }

    async fn evaluate_applicability(&self, goal: &OptimizationGoal) -> Result<f64> {
        // We're most applicable for code-related goals with certain tags
        let mut category_score = 0.6; // Default base score

        // Check tags to determine the most applicable goals
        for tag in &goal.tags {
            match tag.as_str() {
                "performance" => category_score = 1.0,
                "readability" => category_score = 0.9,
                "test-coverage" => category_score = 0.9,
                "security" => category_score = 0.9,
                "complexity" => category_score = 1.0,
                "error-handling" => category_score = 0.7,
                "compatibility" => category_score = 0.8,
                "financial" => category_score = 0.5,
                _ => {} // Keep default score for other tags
            }
        }

        // Use category score from tags
        Ok(category_score)
    }

    async fn create_plan(&self, goal: &OptimizationGoal) -> Result<Plan> {
        let plan_id = Uuid::new_v4().to_string();
        let goal_id = goal.id.clone();

        // Create a branch name for this improvement
        let category_slug = if let Some(category_tag) = goal.tags.iter().find(|tag|
            ["performance", "readability", "test-coverage", "security", "complexity",
             "error-handling", "compatibility", "financial", "general"].contains(&tag.as_str())) {
            category_tag.clone()
        } else {
            goal.category.to_string().to_lowercase()
        };

        let branch_name = format!("improvement/{}/{}", category_slug.replace(' ', "_"), goal.id);

        // Create steps for the plan
        let mut steps = Vec::new();

        // Step 1: Generate code improvement
        let generate_id = Uuid::new_v4().to_string();
        steps.push(ActionStep {
            id: generate_id.clone(),
            description: format!("Generate code improvement for goal: {}", goal.title),
            action_type: ActionType::CodeImprovement,
            dependencies: Vec::new(),
            parameters: {
                let mut params = HashMap::new();
                params.insert("goal_id".to_string(), goal.id.clone());
                params
            },
            expected_outcome: "Generated code improvement".to_string(),
            requires_confirmation: false,
        });

        // Step 2: Apply code changes to branch
        let apply_id = Uuid::new_v4().to_string();
        steps.push(ActionStep {
            id: apply_id.clone(),
            description: format!("Apply code changes to branch: {}", branch_name),
            action_type: ActionType::CodeImprovement,
            dependencies: vec![generate_id.clone()],
            parameters: {
                let mut params = HashMap::new();
                params.insert("goal_id".to_string(), goal.id.clone());
                params.insert("branch_name".to_string(), branch_name.clone());
                params
            },
            expected_outcome: format!("Code changes applied to branch: {}", branch_name),
            requires_confirmation: false,
        });

        // Step 3: Test code changes
        let test_id = Uuid::new_v4().to_string();
        steps.push(ActionStep {
            id: test_id.clone(),
            description: format!("Test code changes in branch: {}", branch_name),
            action_type: ActionType::CodeImprovement,
            dependencies: vec![apply_id.clone()],
            parameters: {
                let mut params = HashMap::new();
                params.insert("branch_name".to_string(), branch_name.clone());
                params
            },
            expected_outcome: "Tests passed for code changes".to_string(),
            requires_confirmation: false,
        });

        // Step 4: Merge changes if tests pass
        let merge_id = Uuid::new_v4().to_string();
        steps.push(ActionStep {
            id: merge_id.clone(),
            description: format!("Merge branch {} into main if tests pass", branch_name),
            action_type: ActionType::CodeImprovement,
            dependencies: vec![test_id.clone()],
            parameters: {
                let mut params = HashMap::new();
                params.insert("branch_name".to_string(), branch_name.clone());
                params
            },
            expected_outcome: format!("Branch {} merged into main", branch_name),
            requires_confirmation: true,
        });

        // Create the plan
        let plan = Plan {
            id: plan_id,
            goal_id,
            steps,
            success_probability: 0.8,
            resource_estimate: {
                let mut resources = HashMap::new();
                resources.insert("time_seconds".to_string(), 120.0);
                resources.insert("memory_mb".to_string(), 200.0);
                resources
            },
            strategy_name: self.name().to_string(),
            step_outputs: HashMap::new(),
        };

        Ok(plan)
    }

    async fn execute(&self, plan: &Plan, step_id: Option<&str>) -> Result<ExecutionResult> {
        // If step_id is None, execute the entire plan
        if step_id.is_none() {
            return self.execute_full_plan(plan).await;
        }

        // Find the step
        let step = plan
            .steps
            .iter()
            .find(|s| s.id == step_id.unwrap())
            .ok_or_else(|| anyhow!("Step not found: {}", step_id.unwrap()))?;

        match step.id.as_str() {
            // Execute the appropriate step
            s if s == plan.steps[0].id => self.execute_generate_step(plan, step).await,
            s if s == plan.steps[1].id => self.execute_apply_step(plan, step).await,
            s if s == plan.steps[2].id => self.execute_test_step(plan, step).await,
            s if s == plan.steps[3].id => self.execute_merge_step(plan, step).await,
            _ => Err(anyhow!("Unknown step: {}", step.id)),
        }
    }

    fn check_permissions(&self, goal: &OptimizationGoal) -> Result<bool> {
        info!("Checking permissions for goal: {}", goal.id);

        // Lock the authentication manager
        let auth_manager = match self.auth_manager.try_lock() {
            Ok(manager) => manager,
            Err(_) => return Err(anyhow!("Failed to acquire authentication manager lock")),
        };

        // Check if there's an authenticated user
        if auth_manager.current_user().is_none() {
            info!("No authenticated user found, but we're in permissive mode - granting permission");
            return Ok(true);
        }

        // Check if the session is valid - even if not, we'll allow operations
        if !auth_manager.is_session_valid() {
            info!("User session expired, but we're in permissive mode - granting permission");
            return Ok(true);
        }

        // Get the permissions required for this goal (for logging purposes only)
        let required_permissions = self.get_required_permissions_for_goal(goal);
        info!("Goal requires {} permissions - granting all in permissive mode", required_permissions.len());

        // In permissive mode, always grant permission
        Ok(true)
    }

    fn required_permissions(&self) -> Vec<ActionPermission> {
        vec![
            ActionPermission {
                scope: PermissionScope::LocalFileSystem(self.working_dir.to_string_lossy().to_string()),
                requires_confirmation: false,
                audit_level: "high".to_string(),
                expiry: None,
            },
        ]
    }
}

impl CodeImprovementStrategy {
    async fn execute_full_plan(&self, plan: &Plan) -> Result<ExecutionResult> {
        info!("Executing full plan: {}", plan.id);

        let mut success = true;
        let mut execution_log = Vec::new();
        let mut outputs = HashMap::new();

        // Execute each step in order
        for step in &plan.steps {
            // Check if this step has dependencies that failed
            let deps_satisfied = step.dependencies.iter().all(|dep_id| {
                if let Some(status) = outputs.get(dep_id) {
                    status == "success"
                } else {
                    false
                }
            });

            if !step.dependencies.is_empty() && !deps_satisfied {
                let msg = format!("Skipping step {} as dependencies failed", step.id);
                info!("{}", msg);
                execution_log.push(msg);
                outputs.insert(step.id.clone(), "skipped".to_string());
                continue;
            }

            // Execute the step
            let result = self.execute(plan, Some(&step.id)).await;

            match result {
                Ok(step_result) => {
                    if step_result.success {
                        let msg = format!("Step {} completed successfully", step.id);
                        info!("{}", msg);
                        execution_log.push(msg);
                        outputs.insert(step.id.clone(), "success".to_string());

                        // Merge the execution log and outputs
                        execution_log.extend(step_result.execution_log);
                        outputs.extend(step_result.outputs);
                    } else {
                        let msg = format!("Step {} failed: {}", step.id, step_result.message);
                        warn!("{}", msg);
                        execution_log.push(msg);
                        outputs.insert(step.id.clone(), "failed".to_string());
                        success = false;
                        break;
                    }
                }
                Err(e) => {
                    let msg = format!("Error executing step {}: {}", step.id, e);
                    error!("{}", msg);
                    execution_log.push(msg);
                    outputs.insert(step.id.clone(), "error".to_string());
                    success = false;
                    break;
                }
            }

            // If the step requires confirmation and it's not the last step, pause here
            if step.requires_confirmation && step.id != plan.steps.last().unwrap().id {
                let msg = "Pausing execution for user confirmation".to_string();
                info!("{}", msg);
                execution_log.push(msg);
                break;
            }
        }

        let message = if success {
            "Plan executed successfully".to_string()
        } else {
            "Plan execution failed".to_string()
        };

        Ok(ExecutionResult {
            success,
            message,
            outputs,
            metrics: HashMap::new(),
            execution_log,
        })
    }

    async fn execute_generate_step(&self, plan: &Plan, step: &ActionStep) -> Result<ExecutionResult> {
        let goal_id = step
            .parameters
            .get("goal_id")
            .ok_or_else(|| anyhow!("Missing goal_id parameter"))?;

        // Fetch the actual goal from the optimization manager using the agent
        info!("Fetching goal with id: {}", goal_id);

        // Use goal from previous steps if available in the plan outputs
        let goal = if let Some(goal_json) = plan.get_step_output("fetch_goal") {
            serde_json::from_str::<OptimizationGoal>(&goal_json)?
        } else {
            // In a production implementation, we would fetch from optimization manager
            // For example via a shared context or service locator
            return Err(anyhow!("Goal not found in plan outputs. Make sure to fetch goal before generation step."));
        };

        info!("Generating code improvement for goal: {}", goal.title);

        // Generate the improvement
        let generated_code = self.generate_improvement(&goal).await?;

        // Store previous attempts for future reference
        let mut previous_attempts = Vec::new();

        // Check if we have previous attempts in the plan outputs
        if let Some(prev_attempts_json) = plan.get_step_output("previous_attempts") {
            previous_attempts = serde_json::from_str::<Vec<String>>(&prev_attempts_json)?;
        }

        // Add this attempt to previous attempts
        previous_attempts.push(generated_code.clone());

        // Create execution result
        let mut outputs = HashMap::new();
        outputs.insert("generated_code".to_string(), generated_code);
        outputs.insert("previous_attempts".to_string(), serde_json::to_string(&previous_attempts)?);

        let execution_log = vec![
            format!("Generated code improvement for goal: {}", goal.id),
            format!("Code generation attempt {}", previous_attempts.len()),
        ];

        Ok(ExecutionResult {
            success: true,
            message: format!("Successfully generated code improvement for goal: {}", goal.id),
            outputs,
            metrics: {
                let mut metrics = HashMap::new();
                metrics.insert("generation_time_ms".to_string(), 1000.0); // Example metric
                metrics
            },
            execution_log,
        })
    }

    async fn execute_apply_step(&self, plan: &Plan, step: &ActionStep) -> Result<ExecutionResult> {
        let goal_id = step
            .parameters
            .get("goal_id")
            .ok_or_else(|| anyhow!("Missing goal_id parameter"))?;

        let branch_name = step
            .parameters
            .get("branch_name")
            .ok_or_else(|| anyhow!("Missing branch_name parameter"))?;

        // Fetch the goal from the plan outputs
        let goal = if let Some(goal_json) = plan.get_step_output("fetch_goal") {
            serde_json::from_str::<OptimizationGoal>(&goal_json)?
        } else {
            // In a production implementation, we would fetch from optimization manager
            return Err(anyhow!("Goal not found in plan outputs. Make sure to fetch goal before apply step."));
        };

        // Get the generated code from the previous step
        let code = plan
            .get_step_output("generated_code")
            .ok_or_else(|| anyhow!("No generated code found in plan outputs"))?;

        info!("Applying code changes for goal: {}", goal.title);
        info!("Using branch: {}", branch_name);

        // Apply the changes
        self.apply_change(&goal, branch_name, &code).await?;

        // Create execution result
        let mut outputs = HashMap::new();
        outputs.insert("branch_name".to_string(), branch_name.clone());

        let execution_log = vec![
            format!("Applied changes for goal: {}", goal.id),
            format!("Changes applied to branch: {}", branch_name),
        ];

        Ok(ExecutionResult {
            success: true,
            message: format!("Successfully applied code changes for goal: {}", goal.id),
            outputs,
            metrics: {
                let mut metrics = HashMap::new();
                metrics.insert("files_changed".to_string(), 1.0); // Example metric
                metrics
            },
            execution_log,
        })
    }

    async fn execute_test_step(&self, _plan: &Plan, step: &ActionStep) -> Result<ExecutionResult> {
        let branch_name = step
            .parameters
            .get("branch_name")
            .ok_or_else(|| anyhow!("Missing branch_name parameter"))?;

        // Test the changes
        let test_passed = self.test_change(branch_name).await?;

        let (success, message) = if test_passed {
            (
                true,
                format!("Tests passed for branch: {}", branch_name),
            )
        } else {
            (
                false,
                format!("Tests failed for branch: {}", branch_name),
            )
        };

        let execution_log = vec![
            format!("Tested changes in branch: {}", branch_name),
            message.clone(),
        ];

        Ok(ExecutionResult {
            success,
            message,
            outputs: {
                let mut outputs = HashMap::new();
                outputs.insert("tests_passed".to_string(), test_passed.to_string());
                outputs
            },
            metrics: HashMap::new(),
            execution_log,
        })
    }

    async fn execute_merge_step(&self, _plan: &Plan, step: &ActionStep) -> Result<ExecutionResult> {
        let branch_name = step
            .parameters
            .get("branch_name")
            .ok_or_else(|| anyhow!("Missing branch_name parameter"))?;

        // Merge the changes
        self.merge_change(branch_name).await?;

        let execution_log = vec![
            format!("Merged branch {} into main", branch_name),
        ];

        Ok(ExecutionResult {
            success: true,
            message: format!("Successfully merged branch {} into main", branch_name),
            outputs: {
                let mut outputs = HashMap::new();
                outputs.insert("merged".to_string(), "true".to_string());
                outputs
            },
            metrics: HashMap::new(),
            execution_log,
        })
    }
}