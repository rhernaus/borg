use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;

use crate::testing::simple::SimpleTestRunner;
use crate::testing::test_runner::TestRunner;

/// Factory for creating test runners
pub struct TestRunnerFactory;

impl TestRunnerFactory {
    /// Create a test runner for the given workspace
    pub fn create<P: AsRef<Path>>(workspace: P) -> Result<Arc<dyn TestRunner>> {
        let runner =
            SimpleTestRunner::new(workspace).context("Failed to create simple test runner")?;
        Ok(Arc::new(runner))
    }
}
