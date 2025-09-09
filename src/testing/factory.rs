use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;

use crate::core::config::Config;
use crate::testing::comprehensive::{ComprehensiveTestRunner, TestStage};
use crate::testing::simple::SimpleTestRunner;
use crate::testing::test_runner::TestRunner;

/// Factory for creating test runners based on configuration
pub struct TestRunnerFactory;

/// The type of test runner to create
pub enum TestRunnerType {
    /// Simple test runner (basic unit tests only)
    Simple,

    /// Comprehensive test runner (multiple test stages)
    Comprehensive,
}

impl TestRunnerFactory {
    /// Create a test runner based on the configuration
    pub fn create<P: AsRef<Path>>(config: &Config, workspace: P) -> Result<Arc<dyn TestRunner>> {
        // Determine which test runner to use based on config
        let runner_type = if config.testing.linting_enabled
            || config.testing.compilation_check
            || config.testing.run_integration_tests
            || config.testing.performance_benchmarks
        {
            TestRunnerType::Comprehensive
        } else {
            TestRunnerType::Simple
        };

        Self::create_runner(runner_type, workspace, config)
    }

    /// Create a specific type of test runner
    pub fn create_runner<P: AsRef<Path>>(
        runner_type: TestRunnerType,
        workspace: P,
        config: &Config,
    ) -> Result<Arc<dyn TestRunner>> {
        match runner_type {
            TestRunnerType::Simple => {
                let runner = SimpleTestRunner::new(workspace)
                    .context("Failed to create simple test runner")?;
                Ok(Arc::new(runner))
            }
            TestRunnerType::Comprehensive => {
                // Create a comprehensive test runner with enabled stages based on config
                let mut enabled_stages = Vec::new();

                if config.testing.linting_enabled {
                    enabled_stages.push(TestStage::Formatting);
                    enabled_stages.push(TestStage::Linting);
                }

                if config.testing.compilation_check {
                    enabled_stages.push(TestStage::Compilation);
                }

                // Always include unit tests
                enabled_stages.push(TestStage::UnitTests);
                enabled_stages.push(TestStage::DocTests);

                if config.testing.run_integration_tests {
                    enabled_stages.push(TestStage::IntegrationTests);
                }

                if config.testing.performance_benchmarks {
                    enabled_stages.push(TestStage::Benchmarks);
                }

                let runner = ComprehensiveTestRunner::new(workspace)
                    .context("Failed to create comprehensive test runner")?
                    .with_stages(enabled_stages)
                    .with_timeout(config.testing.timeout_seconds)
                    .continue_on_failure(true); // Allow collecting all errors

                Ok(Arc::new(runner))
            }
        }
    }
}
