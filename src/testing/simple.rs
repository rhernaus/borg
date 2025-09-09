use anyhow::{Context, Result};
use async_trait::async_trait;
use log::{info, warn};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use crate::core::error::BorgError;
use crate::testing::test_runner::{TestMetrics, TestResult, TestRunner};

/// A simple test runner for Rust code
pub struct SimpleTestRunner {
    /// Path to the workspace
    workspace: PathBuf,

    /// Timeout for tests in seconds
    timeout_seconds: u64,
}

impl SimpleTestRunner {
    /// Create a new simple test runner
    pub fn new<P: AsRef<Path>>(workspace: P) -> Result<Self> {
        Ok(Self {
            workspace: workspace.as_ref().to_path_buf(),
            timeout_seconds: 120, // Default timeout of 2 minutes
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
                if let Some(passed_str) = line
                    .split_whitespace()
                    .skip_while(|&s| !s.ends_with("passed;"))
                    .next()
                {
                    if let Ok(passed) = passed_str
                        .trim_end_matches("passed;")
                        .trim()
                        .parse::<usize>()
                    {
                        tests_passed = passed;
                    }
                }

                if let Some(failed_str) = line
                    .split_whitespace()
                    .skip_while(|&s| !s.ends_with("failed;"))
                    .next()
                {
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
                memory_usage_mb: None, // We don't track this in the simple implementation
                cpu_usage_percent: None, // We don't track this in the simple implementation
            })
        } else {
            None
        }
    }
}

#[async_trait]
impl TestRunner for SimpleTestRunner {
    async fn run_tests(&self, branch: &str, target_path: Option<&Path>) -> Result<TestResult> {
        info!("Running tests on branch {} with SimpleTestRunner", branch);

        let start_time = Instant::now();

        // Determine the target directory
        let target_dir = match target_path {
            Some(path) => path.to_path_buf(),
            None => self.workspace.clone(),
        };

        // Build the command
        let mut cmd = Command::new("cargo");
        cmd.current_dir(&target_dir)
            .arg("test")
            .arg("--color=always");

        // Run the command
        let output = match cmd.output() {
            Ok(output) => output,
            Err(e) => {
                return Err(anyhow::anyhow!(BorgError::TestingError(format!(
                    "Failed to run cargo test: {}",
                    e
                ))));
            }
        };

        let duration = start_time.elapsed();

        // Convert output to string
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let combined_output = format!("{}\n{}", stdout, stderr);

        // Determine if tests passed based on exit status
        let success = output.status.success();

        // Parse metrics from output
        let metrics = self.parse_test_output(&combined_output);

        // Log test summary
        if let Some(metrics) = &metrics {
            info!(
                "Test results: {} passed, {} failed, {} total",
                metrics.tests_passed, metrics.tests_failed, metrics.tests_run
            );
        } else {
            warn!("Could not parse test metrics from output");
        }

        Ok(TestResult {
            success,
            output: combined_output,
            duration,
            metrics,
            report: None,
            failures: None,
            compilation_errors: None,
            exit_code: Some(output.status.code().unwrap_or(-1)),
            branch: Some(branch.to_string()),
            test_stage: Some("unit".to_string()),
        })
    }

    async fn run_benchmark(&self, branch: &str, target_path: Option<&Path>) -> Result<TestResult> {
        info!(
            "Running benchmarks on branch {} with SimpleTestRunner",
            branch
        );

        let target_dir = target_path
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.workspace.clone());
        let start_time = Instant::now();

        let output = Command::new("cargo")
            .current_dir(&target_dir)
            .args(&["bench"])
            .output()
            .context("Failed to run benchmarks")?;

        let duration = start_time.elapsed();
        let success = output.status.success();
        let exit_code = output.status.code().unwrap_or(-1);

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let combined_output = format!("STDOUT:\n{}\n\nSTDERR:\n{}", stdout, stderr);

        Ok(TestResult {
            success,
            output: combined_output,
            duration,
            metrics: None,
            report: None,
            failures: None,
            compilation_errors: None,
            exit_code: Some(exit_code),
            branch: Some(branch.to_string()),
            test_stage: Some("benchmark".to_string()),
        })
    }
}
