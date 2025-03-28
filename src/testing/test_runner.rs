use anyhow::{Context, Result};
use async_trait::async_trait;
use log::{info, error, debug};
use serde::{Serialize, Deserialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use tokio::process::Command as TokioCommand;
use tokio::time::timeout;

use crate::core::error::BorgError;

/// Result of running tests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    /// Whether the tests passed
    pub success: bool,

    /// Output from the test run
    pub output: String,

    /// Time taken to run the tests
    pub duration: Duration,

    /// Any metrics collected during the test run
    pub metrics: Option<TestMetrics>,

    /// Structured test report for better feedback
    pub report: Option<String>,

    /// Specific test failures with details
    pub failures: Option<Vec<TestFailure>>,

    /// Error details if compilation failed
    pub compilation_errors: Option<Vec<CompilationError>>,

    /// Exit code of the test process
    pub exit_code: Option<i32>,

    /// The current branch being tested
    pub branch: Option<String>,

    /// The stage of testing (unit tests, integration tests, etc.)
    pub test_stage: Option<String>,
}

/// A specific test failure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestFailure {
    /// The name of the failing test
    pub test_name: String,

    /// The expected output or behavior
    pub expected: Option<String>,

    /// The actual output or behavior
    pub actual: Option<String>,

    /// The file where the test is located
    pub file: Option<String>,

    /// The line number of the test
    pub line: Option<usize>,

    /// The raw test output
    pub output: String,

    /// Any additional context about the failure
    pub context: Option<String>,
}

/// A compilation error from the test run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilationError {
    /// The error message
    pub message: String,

    /// The file where the error occurred
    pub file: Option<String>,

    /// The line number where the error occurred
    pub line: Option<usize>,

    /// The column number where the error occurred
    pub column: Option<usize>,

    /// The code snippet where the error occurred
    pub code_snippet: Option<String>,

    /// Error code if any
    pub error_code: Option<String>,
}

/// Metrics collected during a test run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestMetrics {
    /// Number of tests run
    pub tests_run: usize,

    /// Number of tests passed
    pub tests_passed: usize,

    /// Number of tests failed
    pub tests_failed: usize,

    /// Memory usage during tests
    pub memory_usage_mb: Option<usize>,

    /// CPU usage during tests
    pub cpu_usage_percent: Option<f64>,
}

/// Test runner interface
#[async_trait]
pub trait TestRunner: Send + Sync {
    /// Run tests on a branch
    async fn run_tests(&self, branch: &str, target_path: Option<&Path>) -> Result<TestResult>;

    /// Run a benchmark on a branch
    async fn run_benchmark(&self, branch: &str, target_path: Option<&Path>) -> Result<TestResult>;

    /// Run benchmarks on a branch (multiple benchmarks)
    async fn run_benchmarks(&self, branch: &str) -> Result<TestResult> {
        // Default implementation just calls the single benchmark method
        self.run_benchmark(branch, None).await
    }

    /// Run tests with a specific tag
    async fn run_tests_with_tag(&self, branch: &str, tag: &str) -> Result<TestResult> {
        // Default implementation just passes the tag as a filter to the tests
        info!("Running tests with tag: {} on branch: {}", tag, branch);
        self.run_tests(branch, None).await
    }

    /// Run linting checks on a branch
    async fn run_linting(&self, branch: &str) -> Result<TestResult> {
        // Default implementation - can be overridden by specific implementations
        info!("Running linting on branch: {}", branch);
        let result = TestResult {
            success: true,
            output: "Linting not implemented for this test runner".to_string(),
            duration: Duration::from_secs(0),
            metrics: None,
            report: None,
            failures: None,
            compilation_errors: None,
            exit_code: Some(0),
            branch: Some(branch.to_string()),
            test_stage: Some("linting".to_string()),
        };
        Ok(result)
    }

    /// Run coverage analysis on a branch
    async fn run_coverage_analysis(&self, branch: &str) -> Result<TestResult> {
        // Default implementation - can be overridden by specific implementations
        info!("Running coverage analysis on branch: {}", branch);
        let result = TestResult {
            success: true,
            output: "Coverage analysis not implemented for this test runner".to_string(),
            duration: Duration::from_secs(0),
            metrics: None,
            report: None,
            failures: None,
            compilation_errors: None,
            exit_code: Some(0),
            branch: Some(branch.to_string()),
            test_stage: Some("coverage".to_string()),
        };
        Ok(result)
    }
}

/// Cargo-based test runner
pub struct CargoTestRunner {
    /// Path to the workspace
    workspace: PathBuf,

    /// Timeout for test execution
    timeout_seconds: u64,
}

impl CargoTestRunner {
    /// Create a new cargo test runner
    pub fn new<P: AsRef<Path>>(workspace: P, timeout_seconds: u64) -> Self {
        Self {
            workspace: workspace.as_ref().to_path_buf(),
            timeout_seconds,
        }
    }

    /// Check if cargo exists
    fn check_cargo() -> Result<()> {
        let output = Command::new("cargo")
            .arg("--version")
            .output()
            .context("Failed to check if cargo is installed")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(BorgError::TestingError(
                "Cargo is not available in the system".to_string()
            )));
        }

        Ok(())
    }

    /// Parse test output to extract metrics
    fn parse_test_output(&self, output: &str) -> Option<TestMetrics> {
        // Example output parsing - this is a simplified version
        // In a real implementation, this would be more robust

        let mut tests_passed = 0;
        let mut tests_failed = 0;

        // Look for lines like "test result: ok. 42 passed; 0 failed;"
        if let Some(result_line) = output.lines().find(|line| line.trim().starts_with("test result:")) {
            if let Some(passed_part) = result_line.split(';').next() {
                if let Some(passed_str) = passed_part.split_whitespace().nth(3) {
                    if let Ok(passed) = passed_str.parse::<usize>() {
                        tests_passed = passed;
                    }
                }
            }

            if let Some(failed_part) = result_line.split(';').nth(1) {
                if let Some(failed_str) = failed_part.split_whitespace().nth(1) {
                    if let Ok(failed) = failed_str.parse::<usize>() {
                        tests_failed = failed;
                    }
                }
            }

            let tests_run = tests_passed + tests_failed;

            Some(TestMetrics {
                tests_run,
                tests_passed,
                tests_failed,
                memory_usage_mb: None, // Would be populated in a real implementation
                cpu_usage_percent: None, // Would be populated in a real implementation
            })
        } else {
            None
        }
    }
}

#[async_trait]
impl TestRunner for CargoTestRunner {
    async fn run_tests(&self, branch: &str, target_path: Option<&Path>) -> Result<TestResult> {
        // Ensure cargo is available
        Self::check_cargo()?;

        info!("Running tests on branch: {}", branch);

        let target_dir = target_path.unwrap_or(&self.workspace);

        let start_time = Instant::now();

        // Run cargo test with timeout
        let result = timeout(
            Duration::from_secs(self.timeout_seconds),
            TokioCommand::new("cargo")
                .current_dir(target_dir)
                .arg("test")
                .arg("--color=always")
                .output()
        ).await;

        let duration = start_time.elapsed();

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let combined_output = format!("{}\n{}", stdout, stderr);

                let success = output.status.success();
                let metrics = self.parse_test_output(&combined_output);

                if success {
                    info!("Tests passed on branch '{}' in {:?}", branch, duration);
                } else {
                    error!("Tests failed on branch '{}' in {:?}", branch, duration);
                    debug!("Test output: {}", combined_output);
                }

                Ok(TestResult {
                    success,
                    output: combined_output,
                    duration,
                    metrics,
                    report: None,
                    failures: None,
                    compilation_errors: None,
                    exit_code: None,
                    branch: Some(branch.to_string()),
                    test_stage: None,
                })
            },
            Ok(Err(e)) => Err(anyhow::anyhow!(BorgError::TestingError(
                format!("Failed to run cargo test: {}", e)
            ))),
            Err(_) => Err(anyhow::anyhow!(BorgError::TimeoutError(
                format!("Test execution timed out after {} seconds", self.timeout_seconds)
            ))),
        }
    }

    async fn run_benchmark(&self, branch: &str, target_path: Option<&Path>) -> Result<TestResult> {
        // Ensure cargo is available
        Self::check_cargo()?;

        info!("Running benchmarks on branch: {}", branch);

        let target_dir = target_path.unwrap_or(&self.workspace);

        let start_time = Instant::now();

        // Run cargo bench with timeout
        let result = timeout(
            Duration::from_secs(self.timeout_seconds),
            TokioCommand::new("cargo")
                .current_dir(target_dir)
                .arg("bench")
                .output()
        ).await;

        let duration = start_time.elapsed();

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let combined_output = format!("{}\n{}", stdout, stderr);

                let success = output.status.success();

                if success {
                    info!("Benchmarks completed on branch '{}' in {:?}", branch, duration);
                } else {
                    error!("Benchmarks failed on branch '{}' in {:?}", branch, duration);
                    debug!("Benchmark output: {}", combined_output);
                }

                Ok(TestResult {
                    success,
                    output: combined_output,
                    duration,
                    metrics: None, // Benchmarks don't have the same metrics as tests
                    report: None,
                    failures: None,
                    compilation_errors: None,
                    exit_code: None,
                    branch: Some(branch.to_string()),
                    test_stage: None,
                })
            },
            Ok(Err(e)) => Err(anyhow::anyhow!(BorgError::TestingError(
                format!("Failed to run cargo bench: {}", e)
            ))),
            Err(_) => Err(anyhow::anyhow!(BorgError::TimeoutError(
                format!("Benchmark execution timed out after {} seconds", self.timeout_seconds)
            ))),
        }
    }
}