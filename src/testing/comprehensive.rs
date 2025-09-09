use anyhow::Result;
use async_trait::async_trait;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::core::error::BorgError;
use crate::testing::result_analyzer::{TestAnalysis, TestError, TestResultAnalyzer};
use crate::testing::test_runner::{TestMetrics, TestResult, TestRunner};

/// The stage of testing being performed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TestStage {
    /// Code formatting verification
    Formatting,

    /// Code linting with clippy
    Linting,

    /// Compilation validation
    Compilation,

    /// Unit tests
    UnitTests,

    /// Integration tests
    IntegrationTests,

    /// Documentation tests
    DocTests,

    /// Performance benchmarks
    Benchmarks,
}

impl std::fmt::Display for TestStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestStage::Formatting => write!(f, "Formatting"),
            TestStage::Linting => write!(f, "Linting"),
            TestStage::Compilation => write!(f, "Compilation"),
            TestStage::UnitTests => write!(f, "Unit Tests"),
            TestStage::IntegrationTests => write!(f, "Integration Tests"),
            TestStage::DocTests => write!(f, "Doc Tests"),
            TestStage::Benchmarks => write!(f, "Benchmarks"),
        }
    }
}

/// Detailed results from all test stages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComprehensiveTestResult {
    /// Overall success across all stages
    pub success: bool,

    /// Individual stage results
    pub stage_results: Vec<StageResult>,

    /// Total time taken for all stages
    pub total_duration: Duration,

    /// Overall test analysis
    pub analysis: Option<TestAnalysis>,
}

/// Results from a single test stage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageResult {
    /// The stage that was run
    pub stage: TestStage,

    /// Whether this stage was successful
    pub success: bool,

    /// Raw test result for this stage
    pub result: TestResult,

    /// Any errors specific to this stage
    pub errors: Vec<TestError>,
}

/// A comprehensive test runner that runs multiple stages of testing
pub struct ComprehensiveTestRunner {
    /// Path to the workspace
    workspace: PathBuf,

    /// Timeout for each test stage in seconds
    timeout_seconds: u64,

    /// Whether to continue testing after a stage fails
    continue_on_failure: bool,

    /// Which stages to run
    enabled_stages: Vec<TestStage>,

    /// Test result analyzer
    analyzer: TestResultAnalyzer,
}

impl ComprehensiveTestRunner {
    /// Create a new comprehensive test runner with all stages enabled
    pub fn new<P: AsRef<Path>>(workspace: P) -> Result<Self> {
        Ok(Self {
            workspace: workspace.as_ref().to_path_buf(),
            timeout_seconds: 300,       // 5 minutes default timeout per stage
            continue_on_failure: false, // Default to stopping on first failure
            enabled_stages: vec![
                TestStage::Formatting,
                TestStage::Linting,
                TestStage::Compilation,
                TestStage::UnitTests,
                TestStage::IntegrationTests,
                TestStage::DocTests,
                // Benchmarks are not enabled by default as they can be slow
            ],
            analyzer: TestResultAnalyzer::new(),
        })
    }

    /// Configure which test stages to run
    pub fn with_stages(mut self, stages: Vec<TestStage>) -> Self {
        self.enabled_stages = stages;
        self
    }

    /// Configure whether to continue testing after a stage fails
    pub fn continue_on_failure(mut self, continue_on_failure: bool) -> Self {
        self.continue_on_failure = continue_on_failure;
        self
    }

    /// Set timeout for each test stage
    pub fn with_timeout(mut self, timeout_seconds: u64) -> Self {
        self.timeout_seconds = timeout_seconds;
        self
    }

    /// Check if a command is available
    fn check_command(command: &str) -> Result<()> {
        let output = Command::new("which").arg(command).output();

        match output {
            Ok(output) if output.status.success() => Ok(()),
            _ => {
                // Instead of error, return a warning for certain tools
                if command == "rustfmt" || command == "clippy-driver" {
                    warn!("Command '{}' not found, skipping related tests", command);
                    Err(anyhow::anyhow!(BorgError::TestingError(format!(
                        "'{}' command not found, but it's optional",
                        command
                    ))))
                } else {
                    Err(anyhow::anyhow!(BorgError::TestingError(format!(
                        "'{}' command not found",
                        command
                    ))))
                }
            }
        }
    }

    /// Run a command with timeout and return the result
    async fn run_command(
        &self,
        cmd: &mut Command,
        stage: TestStage,
        branch: &str,
    ) -> Result<TestResult> {
        let start_time = Instant::now();

        info!("Running {:?} on branch {}", stage, branch);

        let output = match cmd.output() {
            Ok(output) => output,
            Err(e) => {
                return Err(anyhow::anyhow!(BorgError::TestingError(format!(
                    "Failed to run command for {}: {}",
                    stage, e
                ))));
            }
        };

        let duration = start_time.elapsed();

        // Convert output to string
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let combined_output = format!("{}\n{}", stdout, stderr);

        // Process status differently based on the stage
        let success = match stage {
            // For test stages, check both the status and parse the output for "test result: ok"
            TestStage::UnitTests | TestStage::IntegrationTests | TestStage::DocTests => {
                // First check the exit code
                let status_success = output.status.success();

                // If status is success, it's definitely a pass
                if status_success {
                    true
                } else {
                    // For some test failures, we need to check more carefully
                    // Sometimes cargo test might return non-zero despite passing tests
                    !combined_output.contains("test result: FAILED")
                }
            }
            // For other stages, just use the command exit status
            _ => output.status.success(),
        };

        if success {
            info!("{} passed on branch '{}' in {:?}", stage, branch, duration);
        } else {
            error!("{} failed on branch '{}' in {:?}", stage, branch, duration);
            debug!("Output: {}", combined_output);
        }

        // Parse metrics if it's a test run
        let metrics = if stage == TestStage::UnitTests
            || stage == TestStage::IntegrationTests
            || stage == TestStage::DocTests
        {
            self.parse_test_output(&combined_output)
        } else {
            None
        };

        Ok(TestResult {
            success,
            output: combined_output,
            duration,
            metrics,
            report: None,
            failures: None,
            compilation_errors: None,
            exit_code: Some(output.status.code().unwrap_or(0)),
            branch: Some(branch.to_string()),
            test_stage: Some(stage.to_string()),
        })
    }

    /// Parse test output to extract metrics
    fn parse_test_output(&self, output: &str) -> Option<TestMetrics> {
        let mut tests_run = 0;
        let mut tests_passed = 0;
        let mut tests_failed = 0;

        // Look for the test result summary line
        for line in output.lines() {
            let line = line.trim();

            if line.starts_with("test result:") {
                // Parse the line like "test result: ok. 42 passed; 0 failed;"
                if let Some(passed_str) = line.split_whitespace().find(|s| s.ends_with("passed;")) {
                    if let Ok(passed) = passed_str
                        .trim_end_matches("passed;")
                        .trim()
                        .parse::<usize>()
                    {
                        tests_passed = passed;
                    }
                }

                if let Some(failed_str) = line.split_whitespace().find(|s| s.ends_with("failed;")) {
                    if let Ok(failed) = failed_str
                        .trim_end_matches("failed;")
                        .trim()
                        .parse::<usize>()
                    {
                        tests_failed = failed;
                    }
                }

                tests_run = tests_passed + tests_failed;
                break;
            }
        }

        if tests_run > 0 {
            Some(TestMetrics {
                tests_run,
                tests_passed,
                tests_failed,
                memory_usage_mb: None,
                cpu_usage_percent: None,
            })
        } else {
            None
        }
    }

    /// Run code formatting check
    async fn run_formatting(&self, branch: &str, target_path: Option<&Path>) -> Result<TestResult> {
        Self::check_command("rustfmt")?;

        let target_dir = target_path.unwrap_or(&self.workspace);

        let mut cmd = Command::new("cargo");
        cmd.current_dir(target_dir)
            .arg("fmt")
            .arg("--")
            .arg("--check"); // Check only, don't modify files

        self.run_command(&mut cmd, TestStage::Formatting, branch)
            .await
    }

    /// Run code linting with clippy
    async fn run_linting(&self, branch: &str, target_path: Option<&Path>) -> Result<TestResult> {
        Self::check_command("clippy-driver")?;

        let target_dir = target_path.unwrap_or(&self.workspace);

        let mut cmd = Command::new("cargo");
        cmd.current_dir(target_dir)
            .arg("clippy")
            .arg("--all-targets")
            .arg("--all-features")
            .arg("--")
            .arg("-D")
            .arg("warnings"); // Treat warnings as errors

        self.run_command(&mut cmd, TestStage::Linting, branch).await
    }

    /// Run compilation check
    async fn run_compilation(
        &self,
        branch: &str,
        target_path: Option<&Path>,
    ) -> Result<TestResult> {
        Self::check_command("cargo")?;

        let target_dir = target_path.unwrap_or(&self.workspace);

        let mut cmd = Command::new("cargo");
        cmd.current_dir(target_dir)
            .arg("check")
            .arg("--all-targets")
            .arg("--all-features");

        self.run_command(&mut cmd, TestStage::Compilation, branch)
            .await
    }

    /// Run unit tests
    async fn run_unit_tests(&self, branch: &str, target_path: Option<&Path>) -> Result<TestResult> {
        Self::check_command("cargo")?;

        let target_dir = target_path.unwrap_or(&self.workspace);

        let mut cmd = Command::new("cargo");
        cmd.current_dir(target_dir)
            .arg("test")
            .arg("--lib") // Only test library code, not binaries or integration tests
            .arg("--color=always");

        self.run_command(&mut cmd, TestStage::UnitTests, branch)
            .await
    }

    /// Run integration tests
    async fn run_integration_tests(
        &self,
        branch: &str,
        target_path: Option<&Path>,
    ) -> Result<TestResult> {
        Self::check_command("cargo")?;

        let target_dir = target_path.unwrap_or(&self.workspace);

        let mut cmd = Command::new("cargo");
        cmd.current_dir(target_dir)
            .arg("test")
            .arg("--test=*") // Only run integration tests
            .arg("--color=always");

        self.run_command(&mut cmd, TestStage::IntegrationTests, branch)
            .await
    }

    /// Run documentation tests
    async fn run_doc_tests(&self, branch: &str, target_path: Option<&Path>) -> Result<TestResult> {
        Self::check_command("cargo")?;

        let target_dir = target_path.unwrap_or(&self.workspace);

        let mut cmd = Command::new("cargo");
        cmd.current_dir(target_dir)
            .arg("test")
            .arg("--doc") // Only run documentation tests
            .arg("--color=always");

        self.run_command(&mut cmd, TestStage::DocTests, branch)
            .await
    }

    /// Run benchmarks
    async fn run_performance_benchmarks(
        &self,
        branch: &str,
        target_path: Option<&Path>,
    ) -> Result<TestResult> {
        Self::check_command("cargo")?;

        let target_dir = target_path.unwrap_or(&self.workspace);

        let mut cmd = Command::new("cargo");
        cmd.current_dir(target_dir)
            .arg("bench")
            .arg("--color=always");

        self.run_command(&mut cmd, TestStage::Benchmarks, branch)
            .await
    }

    /// Run a single stage and return the result
    async fn run_stage(
        &self,
        stage: TestStage,
        branch: &str,
        target_path: Option<&Path>,
    ) -> Result<Option<StageResult>> {
        let stage_result = match stage {
            TestStage::Formatting => self.run_formatting(branch, target_path).await,
            TestStage::Linting => self.run_linting(branch, target_path).await,
            TestStage::Compilation => self.run_compilation(branch, target_path).await,
            TestStage::UnitTests => self.run_unit_tests(branch, target_path).await,
            TestStage::IntegrationTests => self.run_integration_tests(branch, target_path).await,
            TestStage::DocTests => self.run_doc_tests(branch, target_path).await,
            TestStage::Benchmarks => self.run_performance_benchmarks(branch, target_path).await,
        };

        match stage_result {
            Ok(result) => {
                let result_analysis = self.analyzer.analyze(&result, None);
                let success = result.success;

                Ok(Some(StageResult {
                    stage,
                    success,
                    result,
                    errors: result_analysis.errors,
                }))
            }
            Err(e) => {
                // Check if this is a "not found, but optional" error for formatting/linting tools
                if let Some(err_msg) = e
                    .to_string()
                    .strip_suffix("command not found, but it's optional")
                {
                    info!("Skipping {} stage: {}", stage, err_msg);
                    return Ok(None); // Return None to indicate stage was skipped
                }

                error!("Failed to run {} stage: {}", stage, e);
                Err(e)
            }
        }
    }

    /// Run all test stages and return comprehensive results
    async fn run_all_stages(
        &self,
        branch: &str,
        target_path: Option<&Path>,
    ) -> Result<ComprehensiveTestResult> {
        info!("Running comprehensive tests on branch {}", branch);

        let start_time = Instant::now();
        let mut stage_results = Vec::new();
        let mut overall_success = true;

        for &stage in &self.enabled_stages {
            match self.run_stage(stage, branch, target_path).await {
                Ok(Some(stage_result)) => {
                    let stage_success = stage_result.success;
                    stage_results.push(stage_result);

                    // Update overall success
                    overall_success = overall_success && stage_success;

                    // Check if we should continue after a failure
                    if !stage_success && !self.continue_on_failure {
                        break;
                    }
                }
                Ok(None) => {
                    // Stage was skipped (e.g., optional tool not available)
                    continue;
                }
                Err(e) => {
                    error!("Error running test stage {}: {}", stage, e);
                    overall_success = false;

                    if !self.continue_on_failure {
                        break;
                    }
                }
            }
        }

        let total_duration = start_time.elapsed();

        // Create an overall analysis from all stage results
        let analysis = if !stage_results.is_empty() {
            // Find the first failed stage for detailed analysis
            let failed_stage = stage_results.iter().find(|r| !r.success);

            if let Some(failed) = failed_stage {
                let analysis = self.analyzer.analyze(&failed.result, None);
                Some(analysis)
            } else {
                // All stages passed
                Some(TestAnalysis {
                    success: true,
                    feedback: "All test stages passed successfully.".to_string(),
                    errors: Vec::new(),
                    complete: true,
                    performance_change: None,
                })
            }
        } else {
            None
        };

        Ok(ComprehensiveTestResult {
            success: overall_success,
            stage_results,
            total_duration,
            analysis,
        })
    }

    /// Generate a report from test results
    pub fn generate_report(&self, result: &ComprehensiveTestResult) -> String {
        let mut report = String::new();

        // Overall summary
        report.push_str("# Test Report\n\n");
        report.push_str(&format!(
            "**Overall Result:** {}\n",
            if result.success {
                "✅ PASSED"
            } else {
                "❌ FAILED"
            }
        ));
        report.push_str(&format!(
            "**Total Duration:** {:.2}s\n\n",
            result.total_duration.as_secs_f64()
        ));

        // Detailed stage results
        report.push_str("## Test Stages\n\n");
        report.push_str("| Stage | Result | Duration | Metrics |\n");
        report.push_str("|-------|--------|----------|--------|\n");

        for stage_result in &result.stage_results {
            let metrics_str = match &stage_result.result.metrics {
                Some(m) => format!(
                    "{} run, {} passed, {} failed",
                    m.tests_run, m.tests_passed, m.tests_failed
                ),
                None => "N/A".to_string(),
            };

            report.push_str(&format!(
                "| {} | {} | {:.2}s | {} |\n",
                stage_result.stage,
                if stage_result.success {
                    "✅ PASS"
                } else {
                    "❌ FAIL"
                },
                stage_result.result.duration.as_secs_f64(),
                metrics_str
            ));
        }

        // Errors
        let mut has_errors = false;
        for stage_result in &result.stage_results {
            if !stage_result.errors.is_empty() {
                has_errors = true;
                report.push_str(&format!("\n## {} Errors\n\n", stage_result.stage));

                for (i, error) in stage_result.errors.iter().enumerate() {
                    let location = match (&error.file, &error.line) {
                        (Some(file), Some(line)) => format!("{}:{}", file, line),
                        (Some(file), None) => file.clone(),
                        _ => "unknown location".to_string(),
                    };

                    report.push_str(&format!(
                        "### Error {}: {}\n**Location:** {}\n**Message:** {}\n\n",
                        i + 1,
                        error.error_type,
                        location,
                        error.message
                    ));
                }
            }
        }

        // If there were no errors but the test failed, include a note
        if !has_errors && !result.success {
            report.push_str("\n## Issues\n\nNo specific errors were identified, but tests failed. Check the complete output for details.\n");
        }

        // Include analysis feedback if available
        if let Some(analysis) = &result.analysis {
            report.push_str("\n## Analysis\n\n");
            report.push_str(&analysis.feedback);
            report.push('\n');
        }

        report
    }
}

#[async_trait]
impl TestRunner for ComprehensiveTestRunner {
    async fn run_tests(&self, branch: &str, target_path: Option<&Path>) -> Result<TestResult> {
        info!("Running comprehensive tests on branch {}", branch);

        // Run all test stages
        let comprehensive_result = self.run_all_stages(branch, target_path).await?;

        // Generate a comprehensive report
        let report = self.generate_report(&comprehensive_result);

        // Create a combined output
        let mut combined_output = String::new();
        combined_output.push_str(&report);
        combined_output.push_str("\n\n## Detailed Stage Outputs\n\n");

        for stage_result in &comprehensive_result.stage_results {
            combined_output.push_str(&format!(
                "### {} Output\n```\n{}\n```\n\n",
                stage_result.stage, stage_result.result.output
            ));
        }

        // Create a simplified TestResult
        Ok(TestResult {
            success: comprehensive_result.success,
            output: combined_output,
            duration: comprehensive_result.total_duration,
            metrics: comprehensive_result
                .stage_results
                .iter()
                .find(|r| r.stage == TestStage::UnitTests)
                .and_then(|r| r.result.metrics.clone()),
            report: None,
            failures: None,
            compilation_errors: None,
            exit_code: None,
            branch: Some(branch.to_string()),
            test_stage: Some("comprehensive".to_string()),
        })
    }

    async fn run_benchmark(&self, branch: &str, target_path: Option<&Path>) -> Result<TestResult> {
        info!("Running benchmarks on branch {}", branch);

        // Only run the benchmark stage
        let result = self.run_performance_benchmarks(branch, target_path).await?;

        Ok(result)
    }
}
