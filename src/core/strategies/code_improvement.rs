use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use git2::{MergeOptions, Repository, Signature};
use log::{error, info, warn};
use regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::code_generation::generator::{CodeContext, CodeGenerator, CodeImprovement, FileChange};
use crate::code_generation::spec_generator::SpecGenerator;
use crate::code_generation::test_generator::{parse_test_failures, GeneratedTests, TestGenerator};
use crate::core::authentication::AuthenticationManager;
use crate::core::optimization::{OptimizationCategory, OptimizationGoal, OptimizationManager};
use crate::core::strategy::{
    ActionPermission, ActionStep, ActionType, ExecutionResult, PermissionScope, Plan, Strategy,
};
use crate::testing::test_runner::TestRunner;
use crate::version_control::git::GitManager;

/// Permissions for code-related operations
#[allow(dead_code)]
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
    #[allow(dead_code)]
    test_runner: Arc<dyn TestRunner>,

    /// Git manager for version control
    #[allow(dead_code)]
    git_manager: Arc<Mutex<dyn GitManager>>,

    /// Authentication manager for permission checks
    auth_manager: Arc<Mutex<AuthenticationManager>>,

    /// Optimization manager for retrieving goals
    optimization_manager: Arc<Mutex<OptimizationManager>>,

    // TDD components (optional, for TDD flow)

    /// Spec generator for creating specifications from goals
    #[allow(dead_code)]
    spec_generator: Option<Arc<SpecGenerator>>,

    /// Test generator for creating tests from specifications
    #[allow(dead_code)]
    test_generator: Option<Arc<TestGenerator>>,

    /// Whether TDD mode is enabled
    #[allow(dead_code)]
    tdd_enabled: bool,

    /// Maximum implementation retries in TDD mode
    max_implementation_retries: usize,
}

impl CodeImprovementStrategy {
    /// Create a new code improvement strategy
    pub fn new(
        working_dir: PathBuf,
        code_generator: Arc<dyn CodeGenerator>,
        test_runner: Arc<dyn TestRunner>,
        git_manager: Arc<Mutex<dyn GitManager>>,
        auth_manager: Arc<Mutex<AuthenticationManager>>,
        optimization_manager: Arc<Mutex<OptimizationManager>>,
    ) -> Self {
        Self {
            working_dir,
            code_generator,
            test_runner,
            git_manager,
            auth_manager,
            optimization_manager,
            spec_generator: None,
            test_generator: None,
            tdd_enabled: false,
            max_implementation_retries: 3,
        }
    }

    /// Create a new code improvement strategy with TDD support
    #[allow(dead_code)]
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_tdd(
        working_dir: PathBuf,
        code_generator: Arc<dyn CodeGenerator>,
        test_runner: Arc<dyn TestRunner>,
        git_manager: Arc<Mutex<dyn GitManager>>,
        auth_manager: Arc<Mutex<AuthenticationManager>>,
        optimization_manager: Arc<Mutex<OptimizationManager>>,
        spec_generator: Arc<SpecGenerator>,
        test_generator: Arc<TestGenerator>,
        max_implementation_retries: usize,
    ) -> Self {
        Self {
            working_dir,
            code_generator,
            test_runner,
            git_manager,
            auth_manager,
            optimization_manager,
            spec_generator: Some(spec_generator),
            test_generator: Some(test_generator),
            tdd_enabled: true,
            max_implementation_retries,
        }
    }

    /// Create a code context from an optimization goal
    #[allow(dead_code)]
    async fn create_code_context(&self, goal: &OptimizationGoal) -> Result<CodeContext> {
        self.create_code_context_with_attempts(goal, Vec::new())
            .await
    }

    /// Create a code context from an optimization goal with previous attempts
    async fn create_code_context_with_attempts(
        &self,
        goal: &OptimizationGoal,
        previous_attempts: Vec<crate::code_generation::generator::PreviousAttempt>,
    ) -> Result<CodeContext> {
        // Get the file paths for the goal
        let file_paths: Vec<String> = goal
            .tags
            .iter()
            .filter(|tag| tag.starts_with("file:"))
            .map(|tag| tag.trim_start_matches("file:").to_string())
            .collect();

        // Create the context with the goal description as the task
        let current_attempt = (previous_attempts.len() + 1) as u32;
        let context = CodeContext {
            task: goal.description.clone(),
            file_paths,
            requirements: Some(format!(
                "Category: {}\nPriority: {}",
                goal.category, goal.priority
            )),
            previous_attempts,
            file_contents: None,
            test_files: None,
            test_contents: None,
            dependencies: None,
            code_structure: None,
            max_attempts: Some(3),
            current_attempt: Some(current_attempt),
            // TDD fields
            specification: None,
            generated_tests: None,
            failing_tests: None,
        };

        Ok(context)
    }

    /// Generate code improvements for a goal
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    async fn apply_change(
        &self,
        goal: &OptimizationGoal,
        branch_name: &str,
        code: &str,
    ) -> Result<()> {
        // Parse code changes
        let code_improvement = self.parse_code_changes(code)?;
        info!(
            "Parsed {} file changes to apply",
            code_improvement.target_files.len()
        );

        // Phase 1: All git operations before the await (in a block so repo is dropped)
        {
            let repo = Repository::open(&self.working_dir).context(format!(
                "Failed to open repository at {:?}",
                self.working_dir
            ))?;

            // Create or checkout the branch
            let branch_exists = repo
                .find_branch(branch_name, git2::BranchType::Local)
                .is_ok();
            info!("Branch {} exists: {}", branch_name, branch_exists);

            if branch_exists {
                // Checkout the existing branch
                info!("Checking out existing branch: {}", branch_name);
                let branch_ref = format!("refs/heads/{}", branch_name);
                let obj = repo
                    .revparse_single(&branch_ref)
                    .context(format!("Failed to find branch: {}", branch_name))?;

                repo.checkout_tree(&obj, None).context(format!(
                    "Failed to checkout tree for branch: {}",
                    branch_name
                ))?;

                repo.set_head(&branch_ref)
                    .context(format!("Failed to set HEAD to branch: {}", branch_name))?;
            } else {
                // Create and checkout a new branch from HEAD
                info!("Creating new branch: {}", branch_name);
                let head = repo.head().context("Failed to get HEAD reference")?;

                let head_commit = head
                    .peel_to_commit()
                    .context("Failed to peel HEAD to commit")?;

                repo.branch(branch_name, &head_commit, false)
                    .context(format!("Failed to create branch: {}", branch_name))?;

                let branch_ref = format!("refs/heads/{}", branch_name);
                let obj = repo
                    .revparse_single(&branch_ref)
                    .context(format!("Failed to find branch: {}", branch_name))?;

                repo.checkout_tree(&obj, None).context(format!(
                    "Failed to checkout tree for branch: {}",
                    branch_name
                ))?;

                repo.set_head(&branch_ref)
                    .context(format!("Failed to set HEAD to branch: {}", branch_name))?;
            }

            // Apply each file change
            for file_change in &code_improvement.target_files {
                info!("Applying changes to file: {}", file_change.file_path);

                let file_path = Path::new(&file_change.file_path);
                let full_path = self.working_dir.join(file_path);

                // Make sure the directory exists
                if let Some(parent) = full_path.parent() {
                    if !parent.exists() {
                        std::fs::create_dir_all(parent)
                            .context(format!("Failed to create directory: {:?}", parent))?;
                    }
                }

                // Write the new content to file
                std::fs::write(&full_path, &file_change.new_content)
                    .context(format!("Failed to write to file: {:?}", full_path))?;

                // Add the file to the staging area
                let mut index = repo.index().context("Failed to get repository index")?;

                // Convert the file path to a relative path if needed
                let repo_relative_path = if file_path.is_absolute() {
                    let repo_path = repo.path().parent().unwrap();
                    file_path.strip_prefix(repo_path).unwrap_or(file_path)
                } else {
                    file_path
                };

                index.add_path(repo_relative_path).context(format!(
                    "Failed to add file to index: {:?}",
                    repo_relative_path
                ))?;

                index.write().context("Failed to write index")?;
            }
        } // repo is dropped here

        // Phase 2: Async operation - generate commit message
        // (git2 objects are not Send and can't be held across await points)
        let commit_message = self
            .code_generator
            .generate_commit_message(&code_improvement, &goal.id, branch_name)
            .await
            .context("Failed to generate commit message")?;

        info!("LLM generated commit message: {}", commit_message);

        // Phase 3: Re-open repo and create commit (no awaits after this point)
        {
            let repo = Repository::open(&self.working_dir).context(format!(
                "Failed to reopen repository at {:?}",
                self.working_dir
            ))?;

            // Create a tree from the index
            let mut index = repo.index().context("Failed to get repository index")?;

            let tree_id = index.write_tree().context("Failed to write tree")?;

            let tree = repo.find_tree(tree_id).context("Failed to find tree")?;

            let signature = Signature::now("Borg Agent", "borg@example.com")
                .context("Failed to create signature")?;

            // We need to get the current HEAD as the parent, which should now be the branch we're working on
            let head = repo.head().context("Failed to get HEAD")?;
            let parent_commit = head
                .peel_to_commit()
                .context("Failed to get parent commit")?;

            let commit_oid = repo
                .commit(
                    Some(&format!("refs/heads/{}", branch_name)),
                    &signature,
                    &signature,
                    &commit_message,
                    &tree,
                    &[&parent_commit],
                )
                .context("Failed to create commit")?;

            info!(
                "Successfully created commit {} on branch {}",
                commit_oid, branch_name
            );
        }

        info!(
            "Successfully applied changes for goal {} in branch {}",
            goal.id, branch_name
        );

        Ok(())
    }

    /// Test a code change in a branch
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    async fn evaluate_results(
        &self,
        goal: &OptimizationGoal,
        branch: &str,
        test_passed: bool,
    ) -> Result<bool> {
        if !test_passed {
            warn!("Tests failed for goal '{}' in branch '{}'", goal.id, branch);
            return Ok(false);
        }

        info!("Tests passed for goal '{}' in branch '{}'", goal.id, branch);

        // Check if the change satisfies the success metrics
        if !goal.success_metrics.is_empty() {
            info!("Evaluating success metrics for goal '{}'", goal.id);

            // Run benchmarks if this is a performance-related goal
            if goal.category == OptimizationCategory::Performance
                || goal.tags.iter().any(|t| t == "performance")
            {
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

    /// Execute a specific step of the plan - private implementation
    async fn execute_step_internal(&self, plan: &Plan, step_id: &str) -> Result<ExecutionResult> {
        let step = plan
            .steps
            .iter()
            .find(|s| s.id == step_id)
            .ok_or_else(|| anyhow!("Step with ID {} not found", step_id))?;

        info!("Executing step {} - {}", step.id, step.description);

        let mut execution_log = vec![format!("Started step: {}", step.description)];
        let mut outputs = HashMap::new();

        // Get the optimization goal
        let goal = {
            let manager = self
                .optimization_manager
                .try_lock()
                .map_err(|_| anyhow!("Failed to acquire optimization manager lock"))?;

            manager
                .get_goal(&plan.goal_id)
                .ok_or_else(|| anyhow!("Goal not found: {}", plan.goal_id))?
                .clone()
        };

        // Create branch name
        let branch_name = format!("improvement/{}", goal.id);
        outputs.insert("branch_name".to_string(), branch_name.clone());
        execution_log.push(format!("Target branch: {}", branch_name));

        // Step 1: Create code context
        execution_log.push("Creating code context".to_string());
        let _context = self
            .create_code_context(&goal)
            .await
            .context("Failed to create code context")?;
        execution_log.push("Code context created".to_string());

        // Step 2: Generate improvement
        execution_log.push("Generating code improvement from LLM".to_string());
        let code = self
            .generate_improvement(&goal)
            .await
            .context("Failed to generate improvement")?;
        outputs.insert("code_length".to_string(), code.len().to_string());
        execution_log.push(format!("Generated {} bytes of code", code.len()));

        // Step 3: Apply change to branch
        execution_log.push(format!("Applying changes to branch {}", branch_name));
        self.apply_change(&goal, &branch_name, &code)
            .await
            .context("Failed to apply change")?;
        execution_log.push("Changes applied successfully".to_string());

        // Step 4: Test change
        execution_log.push("Running tests".to_string());
        let test_passed = self
            .test_change(&branch_name)
            .await
            .context("Failed to run tests")?;
        outputs.insert("test_passed".to_string(), test_passed.to_string());
        execution_log.push(format!(
            "Tests {}",
            if test_passed { "passed" } else { "failed" }
        ));

        // Step 5: Evaluate results
        execution_log.push("Evaluating results".to_string());
        let goal_satisfied = self
            .evaluate_results(&goal, &branch_name, test_passed)
            .await
            .context("Failed to evaluate results")?;
        outputs.insert("goal_satisfied".to_string(), goal_satisfied.to_string());
        execution_log.push(format!(
            "Goal {}",
            if goal_satisfied {
                "satisfied"
            } else {
                "not satisfied"
            }
        ));

        // Create execution result
        let success = test_passed && goal_satisfied;
        let message = if success {
            format!("Successfully completed improvement for goal {}", goal.id)
        } else if !test_passed {
            format!("Tests failed for goal {}", goal.id)
        } else {
            format!("Goal {} not satisfied", goal.id)
        };

        Ok(ExecutionResult {
            success,
            message,
            outputs,
            metrics: HashMap::new(),
            execution_log,
        })
    }

    /// Execute a step using TDD flow: spec → tests → implement until pass
    #[allow(dead_code)]
    async fn execute_step_tdd(
        &self,
        plan: &Plan,
        step_id: &str,
    ) -> Result<ExecutionResult> {
        let step = plan
            .steps
            .iter()
            .find(|s| s.id == step_id)
            .ok_or_else(|| anyhow!("Step with ID {} not found", step_id))?;

        info!("Executing TDD step {} - {}", step.id, step.description);

        let mut execution_log = vec![format!("Started TDD step: {}", step.description)];
        let mut outputs = HashMap::new();

        // Get the optimization goal
        let goal = {
            let manager = self
                .optimization_manager
                .try_lock()
                .map_err(|_| anyhow!("Failed to acquire optimization manager lock"))?;

            manager
                .get_goal(&plan.goal_id)
                .ok_or_else(|| anyhow!("Goal not found: {}", plan.goal_id))?
                .clone()
        };

        let branch_name = format!("improvement/{}", goal.id);
        outputs.insert("branch_name".to_string(), branch_name.clone());
        execution_log.push(format!("Target branch: {}", branch_name));

        // Get spec and test generators
        let spec_gen = self
            .spec_generator
            .as_ref()
            .ok_or_else(|| anyhow!("TDD mode enabled but spec_generator is None"))?;
        let test_gen = self
            .test_generator
            .as_ref()
            .ok_or_else(|| anyhow!("TDD mode enabled but test_generator is None"))?;

        // Step 1: Create code context
        execution_log.push("Creating code context".to_string());
        let mut context = self
            .create_code_context(&goal)
            .await
            .context("Failed to create code context")?;

        // Step 2: Generate specification
        execution_log.push("Generating specification from goal".to_string());
        let spec = spec_gen
            .generate_spec(&goal, &context)
            .await
            .context("Failed to generate specification")?;
        execution_log.push(format!(
            "Specification generated: {} file changes, {} acceptance criteria",
            spec.file_changes.len(),
            spec.acceptance_criteria.len()
        ));
        context.specification = Some(spec.clone());

        // Step 3: Generate tests from specification
        execution_log.push("Generating tests from specification".to_string());
        let generated_tests = test_gen
            .generate_tests(&spec, &context)
            .await
            .context("Failed to generate tests")?;
        execution_log.push(format!(
            "Generated {} tests at {}",
            generated_tests.test_names.len(),
            generated_tests.test_file_path
        ));
        context.generated_tests = Some(generated_tests.clone());
        outputs.insert("test_count".to_string(), generated_tests.test_names.len().to_string());

        // Step 4: Write tests to workspace
        execution_log.push("Writing tests to workspace".to_string());
        self.write_tests_to_workspace(&generated_tests)
            .await
            .context("Failed to write tests to workspace")?;
        execution_log.push("Tests written to workspace".to_string());

        // Step 5: Verify tests fail (red phase)
        execution_log.push("Running tests - expecting failure (red phase)".to_string());
        let red_phase_result = self.test_change(&branch_name).await;
        if red_phase_result.unwrap_or(true) {
            warn!("Tests passed in red phase - spec may not need implementation");
            execution_log.push("Warning: Tests passed before implementation".to_string());
        } else {
            execution_log.push("Tests failed as expected (red phase confirmed)".to_string());
        }

        // Step 6-9: Implementation with retries
        let mut implementation_attempt = 0;
        let mut test_passed = false;

        while implementation_attempt < self.max_implementation_retries && !test_passed {
            implementation_attempt += 1;
            execution_log.push(format!(
                "Implementation attempt {} of {}",
                implementation_attempt, self.max_implementation_retries
            ));

            // Generate implementation
            let code = self
                .generate_improvement(&goal)
                .await
                .context("Failed to generate implementation")?;
            outputs.insert("code_length".to_string(), code.len().to_string());

            // Apply changes
            execution_log.push(format!("Applying implementation to branch {}", branch_name));
            self.apply_change(&goal, &branch_name, &code)
                .await
                .context("Failed to apply implementation")?;

            // Run tests
            execution_log.push("Running tests against implementation".to_string());
            test_passed = self
                .test_change(&branch_name)
                .await
                .context("Failed to run tests")?;

            if test_passed {
                execution_log.push("All tests passed (green phase)".to_string());
            } else {
                // Collect failing test info for next attempt
                let test_output = self.get_test_output(&branch_name).await.unwrap_or_default();
                let failing_tests = parse_test_failures(&test_output);
                context.failing_tests = Some(failing_tests.clone());
                execution_log.push(format!(
                    "Tests failed: {} failing tests",
                    failing_tests.len()
                ));
            }
        }

        outputs.insert("test_passed".to_string(), test_passed.to_string());
        outputs.insert(
            "implementation_attempts".to_string(),
            implementation_attempt.to_string(),
        );

        // Step 10: Evaluate results
        execution_log.push("Evaluating results".to_string());
        let goal_satisfied = if test_passed {
            self.evaluate_results(&goal, &branch_name, test_passed)
                .await
                .context("Failed to evaluate results")?
        } else {
            false
        };
        outputs.insert("goal_satisfied".to_string(), goal_satisfied.to_string());

        let success = test_passed && goal_satisfied;
        let message = if success {
            format!(
                "TDD: Successfully implemented goal {} after {} attempts",
                goal.id, implementation_attempt
            )
        } else if !test_passed {
            format!(
                "TDD: Tests failed for goal {} after {} attempts",
                goal.id, implementation_attempt
            )
        } else {
            format!("TDD: Goal {} not satisfied", goal.id)
        };

        Ok(ExecutionResult {
            success,
            message,
            outputs,
            metrics: HashMap::new(),
            execution_log,
        })
    }

    /// Write generated tests to the workspace
    #[allow(dead_code)]
    async fn write_tests_to_workspace(&self, tests: &GeneratedTests) -> Result<()> {
        let test_path = self.working_dir.join(&tests.test_file_path);

        // Create parent directories if needed
        if let Some(parent) = test_path.parent() {
            std::fs::create_dir_all(parent)
                .context(format!("Failed to create test directory: {:?}", parent))?;
        }

        // Write the test file
        std::fs::write(&test_path, &tests.test_code)
            .context(format!("Failed to write test file: {:?}", test_path))?;

        info!("Wrote tests to {:?}", test_path);
        Ok(())
    }

    /// Get test output for failure analysis
    #[allow(dead_code)]
    async fn get_test_output(&self, _branch_name: &str) -> Result<String> {
        // Run cargo test and capture output
        let output = std::process::Command::new("cargo")
            .args(["test", "--", "--nocapture"])
            .current_dir(&self.working_dir)
            .output()
            .context("Failed to run cargo test")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Ok(format!("{}\n{}", stdout, stderr))
    }

    /// Execute a step with retry logic
    async fn execute_with_retry(
        &self,
        plan: &Plan,
        step_id: &str,
        max_retries: usize,
    ) -> Result<ExecutionResult> {
        use crate::code_generation::generator::PreviousAttempt;

        let mut attempts = 0;
        let mut _previous_attempts: Vec<PreviousAttempt> = Vec::new();

        loop {
            attempts += 1;
            info!(
                "Attempt {} of {} for step {}",
                attempts, max_retries, step_id
            );

            // Execute the step
            match self.execute_step_internal(plan, step_id).await {
                Ok(result) => {
                    if result.success {
                        info!("Step {} succeeded on attempt {}", step_id, attempts);
                        return Ok(result);
                    } else {
                        // Step failed, record the attempt
                        warn!(
                            "Step {} failed on attempt {}: {}",
                            step_id, attempts, result.message
                        );

                        if attempts >= max_retries {
                            error!("Step {} failed after {} attempts", step_id, max_retries);
                            return Ok(result); // Return the last failed result
                        }

                        // Record this attempt for context enrichment
                        let code = result.outputs.get("code").cloned().unwrap_or_default();
                        let test_passed = result
                            .outputs
                            .get("test_passed")
                            .and_then(|s| s.parse::<bool>().ok());
                        _previous_attempts.push(PreviousAttempt {
                            code,
                            failure_reason: result.message.clone(),
                            timestamp: chrono::Utc::now(),
                            test_results: None,
                            error_messages: None,
                            compiled: Some(true), // If we got this far, code compiled
                            tests_passed: test_passed,
                            notes: None,
                        });

                        info!(
                            "Retrying step {} (attempt {} of {})",
                            step_id,
                            attempts + 1,
                            max_retries
                        );
                    }
                }
                Err(e) => {
                    error!("Step {} error on attempt {}: {}", step_id, attempts, e);

                    if attempts >= max_retries {
                        error!("Step {} failed after {} attempts", step_id, max_retries);
                        return Err(e);
                    }

                    // Record this attempt
                    _previous_attempts.push(PreviousAttempt {
                        code: String::new(),
                        failure_reason: format!("Error: {}", e),
                        timestamp: chrono::Utc::now(),
                        test_results: None,
                        error_messages: Some(vec![e.to_string()]),
                        compiled: None,
                        tests_passed: None,
                        notes: None,
                    });

                    info!(
                        "Retrying step {} after error (attempt {} of {})",
                        step_id,
                        attempts + 1,
                        max_retries
                    );
                }
            }

            // Note: Future enhancement could pass previous_attempts to create_code_context_with_attempts
            // to enrich the LLM context with failure history for better retry attempts
            info!(
                "Enriching context with {} previous attempt(s)",
                _previous_attempts.len()
            );
        }
    }

    /// Execute the entire plan - private implementation
    async fn execute_full_plan_internal(&self, plan: &Plan) -> Result<ExecutionResult> {
        info!(
            "Executing full code improvement plan with {} steps",
            plan.steps.len()
        );

        let mut execution_log = Vec::new();
        let mut successes = 0;
        let mut failures = 0;
        let mut outputs = HashMap::new();

        // Get the optimization goals
        let goal = {
            let manager = self
                .optimization_manager
                .try_lock()
                .map_err(|_| anyhow!("Failed to acquire optimization manager lock"))?;

            manager
                .get_goal(&plan.goal_id)
                .ok_or_else(|| anyhow!("Goal not found: {}", plan.goal_id))?
                .clone()
        };

        // Create a branch for our improvements
        let branch_name = format!("improvement/{}", plan.goal_id);
        outputs.insert("branch_name".to_string(), branch_name.clone());

        let repo_path = self.working_dir.clone();

        // Open repository and create branch in a non-async scope
        {
            let repo = Repository::open(&repo_path)
                .context(format!("Failed to open repository at {:?}", repo_path))?;

            // Check if branch already exists
            let branch_exists = repo
                .find_branch(&branch_name, git2::BranchType::Local)
                .is_ok();

            if !branch_exists {
                info!("Creating branch {}", branch_name);
                execution_log.push(format!("Created branch {}", branch_name));

                // Create a new branch
                let head = repo.head().context("Failed to get HEAD reference")?;
                let head_commit = head
                    .peel_to_commit()
                    .context("Failed to peel HEAD to commit")?;

                repo.branch(&branch_name, &head_commit, false)
                    .context(format!("Failed to create branch '{}'", branch_name))?;
            }

            // Checkout the branch
            let obj = repo
                .revparse_single(&format!("refs/heads/{}", branch_name))
                .context(format!("Failed to find branch '{}'", branch_name))?;

            repo.checkout_tree(&obj, None)
                .context(format!("Failed to checkout branch '{}'", branch_name))?;

            repo.set_head(&format!("refs/heads/{}", branch_name))
                .context(format!("Failed to set HEAD to branch '{}'", branch_name))?;

            info!("Checked out branch {}", branch_name);
            execution_log.push(format!("Checked out branch {}", branch_name));
        }

        for step in &plan.steps {
            let result = self.execute(plan, Some(&step.id)).await;

            match result {
                Ok(exec_result) => {
                    info!("Step {} executed successfully", step.id);
                    execution_log.extend(exec_result.execution_log);
                    // Merge outputs
                    for (key, value) in exec_result.outputs {
                        outputs.insert(format!("{}.{}", step.id, key), value);
                    }
                    successes += 1;
                }
                Err(e) => {
                    let err_msg = format!("Step {} failed: {}", step.id, e);
                    error!("{}", err_msg);
                    execution_log.push(err_msg);
                    failures += 1;
                }
            }
        }

        // Only merge if all steps were successful
        if failures == 0 && successes > 0 {
            // Create a commit with all our changes
            let code_improvement = CodeImprovement {
                id: format!("improvement-{}", uuid::Uuid::new_v4()),
                task: plan.goal_id.clone(),
                code: "".to_string(), // We don't have the actual code here
                target_files: vec![
                    // In a real implementation, we would track the actual files changed
                    FileChange {
                        file_path: "example.rs".to_string(),
                        start_line: None,
                        end_line: None,
                        new_content: "".to_string(),
                    },
                ],
                explanation: "Automated code improvement".to_string(),
            };

            if let Err(e) = self
                .create_commit(&repo_path, &branch_name, &goal, &code_improvement)
                .await
            {
                let err_msg = format!("Failed to create commit: {}", e);
                error!("{}", err_msg);
                execution_log.push(err_msg);
                failures += 1;
            } else {
                execution_log.push("Created commit with code improvements".to_string());

                // Merge the changes
                if let Err(e) = self.handle_merge(&repo_path, &branch_name).await {
                    let err_msg = format!("Failed to merge changes: {}", e);
                    error!("{}", err_msg);
                    execution_log.push(err_msg);
                    failures += 1;
                } else {
                    execution_log.push(format!("Merged branch {} into main", branch_name));
                }
            }
        }

        let success = failures == 0 && successes > 0;
        let message = if success {
            format!("Successfully executed plan with {} steps", plan.steps.len())
        } else {
            format!(
                "Plan execution partially failed: {} successes, {} failures",
                successes, failures
            )
        };

        Ok(ExecutionResult {
            success,
            message,
            outputs,
            metrics: HashMap::new(),
            execution_log,
        })
    }

    /// Extract file changes from LLM response
    #[allow(dead_code)]
    fn parse_code_changes(
        &self,
        code: &str,
    ) -> Result<crate::code_generation::generator::CodeImprovement> {
        info!("Parsing code changes from LLM response");

        // Create a dummy context with the LLM code
        let _context = crate::code_generation::generator::CodeContext {
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
            // TDD fields
            specification: None,
            generated_tests: None,
            failing_tests: None,
        };

        // Extract file changes
        let mut target_files = Vec::new();

        // Use regex to find code blocks with file path comments
        let re = regex::Regex::new(r"```(?:rust|rs)?\s*(?:// File:|// file:|// Filename:|// filename:)\s*([^\n]+)\n([\s\S]*?)```").unwrap();

        // Find all matches
        for cap in re.captures_iter(code) {
            let file_path = cap[1].trim().to_string();
            let code_content = cap[2].to_string();

            info!("Found file: {}", file_path);

            target_files.push(crate::code_generation::generator::FileChange {
                file_path,
                start_line: None,
                end_line: None,
                new_content: code_content,
            });
        }

        // If no matches with file path comments, try to find any code blocks
        if target_files.is_empty() {
            let simple_re = regex::Regex::new(r"```(?:rust|rs)?\n([\s\S]*?)```").unwrap();

            if let Some(cap) = simple_re.captures(code) {
                let code_content = cap[1].to_string();

                // Use a default file path or extract from context
                target_files.push(crate::code_generation::generator::FileChange {
                    file_path: "src/main.rs".to_string(), // Default path
                    start_line: None,
                    end_line: None,
                    new_content: code_content,
                });
            }
        }

        if target_files.is_empty() {
            warn!("No code blocks found in LLM response");
            return Err(anyhow!("No code blocks found in LLM response"));
        }

        // Create the improvement
        let improvement = crate::code_generation::generator::CodeImprovement {
            id: uuid::Uuid::new_v4().to_string(),
            task: "Apply changes".to_string(),
            code: code.to_string(),
            target_files,
            explanation: "Changes applied from LLM response".to_string(),
        };

        info!("Parsed {} file changes", improvement.target_files.len());

        Ok(improvement)
    }

    /// Create a commit with the improvements in the given branch
    async fn create_commit(
        &self,
        repo_path: &Path,
        branch_name: &str,
        goal: &OptimizationGoal,
        code_improvement: &CodeImprovement,
    ) -> Result<String> {
        info!(
            "Creating commit in branch {} for goal {}",
            branch_name, goal.id
        );

        // First, generate the commit message using LLM, before opening any Git repository
        let commit_message = self
            .code_generator
            .generate_commit_message(code_improvement, &goal.id, branch_name)
            .await
            .context("Failed to generate commit message")?;

        info!("LLM generated commit message: {}", commit_message);

        // Now, open the repository and perform Git operations
        let repo = Repository::open(repo_path).context("Failed to open repository")?;

        let mut index = repo.index().context("Failed to get repository index")?;

        let tree_id = index.write_tree().context("Failed to write tree")?;

        let tree = repo.find_tree(tree_id).context("Failed to find tree")?;

        let signature = Signature::now("Borg Agent", "borg@example.com")
            .context("Failed to create signature")?;

        // We need to get the current HEAD as the parent, which should now be the branch we're working on
        let head = repo.head().context("Failed to get HEAD")?;
        let parent_commit = head
            .peel_to_commit()
            .context("Failed to get parent commit")?;

        // Create the commit with the message generated by the LLM
        let commit_id = repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                &commit_message,
                &tree,
                &[&parent_commit],
            )
            .context("Failed to create commit")?;

        info!("Created commit: {}", commit_id);

        Ok(commit_id.to_string())
    }

    /// Handle merging a branch into the main branch
    async fn handle_merge(&self, repo_path: &Path, branch: &str) -> Result<()> {
        info!("Handling merge of branch {} into main", branch);

        // Extract branch and commit information before calling async function
        let mut summary = String::new();
        let main_branch_name: String;

        {
            // Open the repository and collect information
            let repo = Repository::open(repo_path).context("Failed to open repository")?;

            // Try to find main branch name
            main_branch_name = if repo.find_branch("master", git2::BranchType::Local).is_ok() {
                "master".to_string()
            } else {
                "main".to_string()
            };

            // Get list of commits in branch that aren't in main
            let branch_obj = repo
                .revparse_single(&format!("refs/heads/{}", branch))
                .context(format!("Failed to find branch '{}'", branch))?;

            let branch_commit = branch_obj
                .peel_to_commit()
                .context(format!("Failed to peel branch '{}' to commit", branch))?;

            let main_obj = repo
                .revparse_single(&format!("refs/heads/{}", main_branch_name))
                .context(format!("Failed to find {} branch", main_branch_name))?;

            let main_commit = main_obj.peel_to_commit().context(format!(
                "Failed to peel {} branch to commit",
                main_branch_name
            ))?;

            // Create a simple summary of commits
            let mut revwalk = repo.revwalk().context("Failed to create revwalk")?;
            revwalk
                .push(branch_commit.id())
                .context("Failed to push branch commit to revwalk")?;
            revwalk
                .hide(main_commit.id())
                .context("Failed to hide main commit in revwalk")?;

            for oid in revwalk.flatten() {
                if let Ok(commit) = repo.find_commit(oid) {
                    let message = commit.message().unwrap_or("No message");
                    let summary_line = message.lines().next().unwrap_or("No message");
                    summary.push_str(&format!("- {}\n", summary_line));
                }
            }

            if summary.is_empty() {
                summary = format!(
                    "Branch '{}' has changes that need to be merged into {}",
                    branch, main_branch_name
                );
            }
        } // End of Git object scope

        // Use LLM to get guidance on merge
        let merge_guidance = self
            .code_generator
            .handle_merge_operation(branch, &main_branch_name, &summary)
            .await
            .context("Failed to get merge guidance from LLM")?;

        info!("LLM merge guidance: {}", merge_guidance);

        // Extract merge commit message from the guidance
        let mut merge_message = format!("Merge branch '{}' into {}", branch, main_branch_name);
        if let Some(msg_start) = merge_guidance.find("MERGE COMMIT MESSAGE:") {
            if let Some(msg_end) = merge_guidance[msg_start..].find("\n\n") {
                merge_message = merge_guidance
                    [msg_start + "MERGE COMMIT MESSAGE:".len()..msg_start + msg_end]
                    .trim()
                    .to_string();
            }
        }

        // Perform the actual merge in a new scope to avoid async boundary issues
        {
            let repo = Repository::open(repo_path).context("Failed to open repository")?;

            // Checkout the main branch
            let obj = repo
                .revparse_single(&format!("refs/heads/{}", main_branch_name))
                .context(format!("Failed to find {} branch", main_branch_name))?;

            repo.checkout_tree(&obj, None)
                .context(format!("Failed to checkout {} branch", main_branch_name))?;

            repo.set_head(&format!("refs/heads/{}", main_branch_name))
                .context(format!("Failed to set HEAD to {} branch", main_branch_name))?;

            info!("Checked out {} branch", main_branch_name);

            // Find the annotated commit for our branch
            let branch_ref = repo
                .find_reference(&format!("refs/heads/{}", branch))
                .context(format!("Failed to find branch reference '{}'", branch))?;

            let annotated_commit = repo
                .reference_to_annotated_commit(&branch_ref)
                .context("Failed to convert reference to annotated commit")?;

            let (merge_analysis, _) = repo
                .merge_analysis(&[&annotated_commit])
                .context("Failed to analyze merge")?;

            if merge_analysis.is_up_to_date() {
                info!(
                    "Branch {} is already merged into {}",
                    branch, main_branch_name
                );
                return Ok(());
            }

            if merge_analysis.is_fast_forward() {
                info!("Fast-forward merge possible, but performing normal merge instead");
            }

            // Perform the merge
            let mut merge_opts = MergeOptions::new();
            merge_opts.fail_on_conflict(false);

            repo.merge(&[&annotated_commit], Some(&mut merge_opts), None)
                .context("Failed to merge branches")?;

            // Check for conflicts
            let statuses = repo
                .statuses(None)
                .context("Failed to get repository status")?;

            let mut has_conflicts = false;
            for entry in statuses.iter() {
                if entry.status().is_conflicted() {
                    has_conflicts = true;
                    info!("Conflict in file: {:?}", entry.path());
                }
            }

            if has_conflicts {
                info!("Conflicts detected. LLM guidance: {}", merge_guidance);
                return Err(anyhow!(
                    "Merge conflicts detected. Manual resolution required."
                ));
            }

            // Create the merge commit
            let mut index = repo.index().context("Failed to get repository index")?;

            let tree_id = index.write_tree().context("Failed to write tree")?;

            let tree = repo.find_tree(tree_id).context("Failed to find tree")?;

            let signature = Signature::now("Borg Agent", "borg@example.com")
                .context("Failed to create signature")?;

            let head = repo.head().context("Failed to get HEAD")?;
            let head_commit = head
                .peel_to_commit()
                .context("Failed to peel HEAD to commit")?;

            let branch_ref = repo
                .find_reference(&format!("refs/heads/{}", branch))
                .context(format!("Failed to find branch reference '{}'", branch))?;

            let branch_commit = branch_ref
                .peel_to_commit()
                .context("Failed to peel branch reference to commit")?;

            repo.commit(
                Some("HEAD"),
                &signature,
                &signature,
                &merge_message,
                &tree,
                &[&head_commit, &branch_commit],
            )
            .context("Failed to create merge commit")?;

            // Clean up the merge state
            repo.cleanup_state()
                .context("Failed to cleanup merge state")?;

            info!(
                "Successfully merged branch {} into {}",
                branch, main_branch_name
            );
        } // End of Git operations scope

        Ok(())
    }
}

impl CodeImprovementStrategy {
    /// Get the permissions required for a specific goal
    #[allow(dead_code)]
    fn get_required_permissions_for_goal(&self, goal: &OptimizationGoal) -> Vec<CodePermission> {
        let mut permissions = vec![CodePermission::ReadCode];

        // All code improvement goals require code writing permission
        permissions.push(CodePermission::WriteCode);

        // Add execute tests permission
        permissions.push(CodePermission::ExecuteTests);

        // If the goal involves configuration, add that permission
        if goal.description.to_lowercase().contains("config")
            || goal
                .tags
                .iter()
                .any(|tag| tag.to_lowercase().contains("config"))
        {
            permissions.push(CodePermission::ModifyConfiguration);
        }

        // If the goal involves merging, add that permission
        if goal.description.to_lowercase().contains("merge")
            || goal
                .tags
                .iter()
                .any(|tag| tag.to_lowercase().contains("merge"))
        {
            permissions.push(CodePermission::MergeCode);
        }

        permissions
    }

    /// Get the required permissions for a goal - private implementation
    fn get_required_permissions_for_goal_internal(
        &self,
        _goal: &OptimizationGoal,
    ) -> Vec<ActionPermission> {
        // For code improvement, we need permissions to:
        // 1. Read code files
        // 2. Write code files
        // 3. Run tests
        // 4. Create git commits
        // 5. Create git branches

        vec![
            ActionPermission {
                scope: PermissionScope::LocalFileSystem(
                    self.working_dir.to_string_lossy().to_string(),
                ),
                requires_confirmation: false,
                audit_level: "high".to_string(),
                expiry: None,
            },
            ActionPermission {
                scope: PermissionScope::SystemCommand(vec!["git".to_string()]),
                requires_confirmation: true,
                audit_level: "high".to_string(),
                expiry: None,
            },
        ]
    }

    #[allow(dead_code)]
    fn get_permissions(&self) -> Vec<ActionPermission> {
        vec![
            ActionPermission {
                scope: PermissionScope::LocalFileSystem(
                    self.working_dir.to_string_lossy().to_string(),
                ),
                requires_confirmation: false,
                audit_level: "high".to_string(),
                expiry: None,
            },
            ActionPermission {
                scope: PermissionScope::SystemCommand(vec!["git".to_string()]),
                requires_confirmation: true,
                audit_level: "high".to_string(),
                expiry: None,
            },
        ]
    }
}

#[async_trait]
impl Strategy for CodeImprovementStrategy {
    /// Get the name of this strategy
    fn name(&self) -> &str {
        "Code Improvement"
    }

    /// Get the types of actions this strategy can perform
    fn action_types(&self) -> Vec<ActionType> {
        vec![ActionType::CodeImprovement]
    }

    /// Evaluate how applicable this strategy is for a given goal
    async fn evaluate_applicability(&self, _goal: &OptimizationGoal) -> Result<f64> {
        // For now, we'll just return a high score for all goals
        // In a real implementation, we would analyze the goal and return a score
        Ok(0.9)
    }

    /// Create a plan to achieve the given goal using this strategy
    async fn create_plan(&self, goal: &OptimizationGoal) -> Result<Plan> {
        let plan_id = Uuid::new_v4().to_string();
        let goal_id = goal.id.clone();

        // Create a branch name for this improvement
        let category_slug = if let Some(category_tag) = goal.tags.iter().find(|tag| {
            [
                "performance",
                "readability",
                "test-coverage",
                "security",
                "complexity",
                "error-handling",
                "compatibility",
                "financial",
                "general",
            ]
            .contains(&tag.as_str())
        }) {
            category_tag.clone()
        } else {
            goal.category.to_string().to_lowercase()
        };

        let branch_name = format!(
            "improvement/{}/{}",
            category_slug.replace(' ', "_"),
            goal.id
        );

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

    /// Execute a plan or a specific step of a plan
    async fn execute(&self, plan: &Plan, step_id: Option<&str>) -> Result<ExecutionResult> {
        // If step_id is None, execute the entire plan
        if step_id.is_none() {
            return self.execute_full_plan_internal(plan).await;
        }

        // Otherwise, execute a specific step with retry logic
        self.execute_with_retry(plan, step_id.unwrap(), 3).await
    }

    /// Check if this strategy has the required permissions
    fn check_permissions(&self, goal: &OptimizationGoal) -> Result<bool> {
        info!("Checking permissions for goal: {}", goal.id);

        // Lock the authentication manager
        let auth_manager = match self.auth_manager.try_lock() {
            Ok(manager) => manager,
            Err(_) => return Err(anyhow!("Failed to acquire authentication manager lock")),
        };

        // Check if there's an authenticated user
        if auth_manager.current_user().is_none() {
            info!(
                "No authenticated user found, but we're in permissive mode - granting permission"
            );
            return Ok(true);
        }

        // Check if the session is valid - even if not, we'll allow operations
        if !auth_manager.is_session_valid() {
            info!("User session expired, but we're in permissive mode - granting permission");
            return Ok(true);
        }

        // Get the permissions required for this goal (for logging purposes only)
        let required_permissions = self.get_required_permissions_for_goal_internal(goal);
        info!(
            "Goal requires {} permissions - granting all in permissive mode",
            required_permissions.len()
        );

        // In permissive mode, always grant permission
        Ok(true)
    }

    /// Get the required permissions for this strategy
    fn required_permissions(&self) -> Vec<ActionPermission> {
        vec![
            ActionPermission {
                scope: PermissionScope::LocalFileSystem(
                    self.working_dir.to_string_lossy().to_string(),
                ),
                requires_confirmation: false,
                audit_level: "high".to_string(),
                expiry: None,
            },
            ActionPermission {
                scope: PermissionScope::SystemCommand(vec!["git".to_string()]),
                requires_confirmation: true,
                audit_level: "high".to_string(),
                expiry: None,
            },
        ]
    }
}
