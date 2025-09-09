use log::{debug, info, warn};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::testing::test_runner::TestResult;

/// Analysis of a test result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestAnalysis {
    /// Overall success status
    pub success: bool,

    /// Summarized feedback for the code improvement
    pub feedback: String,

    /// Extracted errors if any
    pub errors: Vec<TestError>,

    /// Was the test run complete?
    pub complete: bool,

    /// Performance difference compared to baseline
    pub performance_change: Option<PerformanceChange>,
}

/// Structure representing a performance change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceChange {
    /// Percentage improvement (positive) or regression (negative)
    pub percentage: f64,

    /// Absolute difference in milliseconds
    pub absolute_ms: f64,

    /// Description of the change
    pub description: String,
}

/// Structure representing a test error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestError {
    /// Error type
    pub error_type: ErrorType,

    /// Error message
    pub message: String,

    /// File where the error occurred
    pub file: Option<String>,

    /// Line number where the error occurred
    pub line: Option<usize>,

    /// Column number where the error occurred
    pub column: Option<usize>,
}

/// Type of error
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ErrorType {
    /// Compilation error
    CompileError,

    /// Runtime error
    RuntimeError,

    /// Test failure
    TestFailure,

    /// Panic
    Panic,

    /// Other error
    Other,
}

impl std::fmt::Display for ErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorType::CompileError => write!(f, "Compilation Error"),
            ErrorType::RuntimeError => write!(f, "Runtime Error"),
            ErrorType::TestFailure => write!(f, "Test Failure"),
            ErrorType::Panic => write!(f, "Panic"),
            ErrorType::Other => write!(f, "Other Error"),
        }
    }
}

/// Test result analyzer
pub struct TestResultAnalyzer {
    /// Regular expressions for parsing errors
    error_regex: Regex,

    /// Regular expression for parsing panics
    panic_regex: Regex,

    /// Regular expression for parsing test failures
    test_failure_regex: Regex,
}

impl TestResultAnalyzer {
    /// Create a new test result analyzer
    pub fn new() -> Self {
        // These regex patterns are simplified and would need to be enhanced for a real implementation
        let error_regex = Regex::new(r"(?m)error(\[E\d+\])?: (.+?)\n-->(.+?):(\d+):(\d+)").unwrap();
        let panic_regex =
            Regex::new(r"(?m)thread '.+?' panicked at '(.+?)',(.+?):(\d+):(\d+)").unwrap();
        let test_failure_regex = Regex::new(r"(?m)test (.+?) \.\.\. FAILED").unwrap();

        Self {
            error_regex,
            panic_regex,
            test_failure_regex,
        }
    }

    /// Analyze a test result
    pub fn analyze(&self, result: &TestResult, baseline: Option<&TestResult>) -> TestAnalysis {
        // Start with the success value from the test result
        let success = result.success;

        let mut analysis = TestAnalysis {
            success,
            feedback: if success {
                "All tests passed successfully.".to_string()
            } else {
                "Tests failed.".to_string()
            },
            errors: Vec::new(),
            complete: !result.output.contains("execution timed out"),
            performance_change: None,
        };

        // Only look for errors if the result indicates failure
        if !success {
            // Extract compilation errors
            for cap in self.error_regex.captures_iter(&result.output) {
                let error = TestError {
                    error_type: ErrorType::CompileError,
                    message: cap[2].to_string(),
                    file: Some(cap[3].trim().to_string()),
                    line: cap[4].parse().ok(),
                    column: cap[5].parse().ok(),
                };

                analysis.errors.push(error);
            }

            // Extract panics
            for cap in self.panic_regex.captures_iter(&result.output) {
                let error = TestError {
                    error_type: ErrorType::Panic,
                    message: cap[1].to_string(),
                    file: Some(cap[2].trim().to_string()),
                    line: cap[3].parse().ok(),
                    column: cap[4].parse().ok(),
                };

                analysis.errors.push(error);
            }

            // Extract test failures
            for cap in self.test_failure_regex.captures_iter(&result.output) {
                let error = TestError {
                    error_type: ErrorType::TestFailure,
                    message: format!("Test '{}' failed", &cap[1]),
                    file: None,
                    line: None,
                    column: None,
                };

                analysis.errors.push(error);
            }
        }

        // Compare performance if we have a baseline
        if let Some(baseline) = baseline {
            if let (Some(result_metrics), Some(baseline_metrics)) =
                (&result.metrics, &baseline.metrics)
            {
                if result_metrics.tests_run > 0 && baseline_metrics.tests_run > 0 {
                    // Calculate performance change based on test duration
                    let baseline_duration = baseline.duration.as_millis() as f64;
                    let result_duration = result.duration.as_millis() as f64;

                    if baseline_duration > 0.0 {
                        let absolute_diff = baseline_duration - result_duration;
                        let percentage = (absolute_diff / baseline_duration) * 100.0;

                        let description = if percentage > 0.0 {
                            format!("Performance improved by {:.2}%", percentage)
                        } else {
                            format!("Performance degraded by {:.2}%", -percentage)
                        };

                        analysis.performance_change = Some(PerformanceChange {
                            percentage,
                            absolute_ms: absolute_diff,
                            description,
                        });
                    }
                }
            }
        }

        // Generate feedback based on analysis
        analysis.feedback = if analysis.success {
            if analysis.errors.is_empty() {
                "All tests passed successfully.".to_string()
            } else {
                "Tests passed, but there were some warnings or non-fatal issues.".to_string()
            }
        } else if !analysis.complete {
            "Test execution timed out.".to_string()
        } else if !analysis.errors.is_empty() {
            let mut feedback = "Tests failed with the following issues:\n".to_string();

            for (i, error) in analysis.errors.iter().enumerate() {
                let location = match (&error.file, &error.line) {
                    (Some(file), Some(line)) => format!("{} line {}", file, line),
                    (Some(file), None) => file.clone(),
                    _ => "unknown location".to_string(),
                };

                feedback.push_str(&format!(
                    "{}. {} at {}: {}\n",
                    i + 1,
                    match error.error_type {
                        ErrorType::CompileError => "Compilation error",
                        ErrorType::RuntimeError => "Runtime error",
                        ErrorType::TestFailure => "Test failure",
                        ErrorType::Panic => "Panic",
                        ErrorType::Other => "Error",
                    },
                    location,
                    error.message
                ));
            }

            feedback
        } else {
            "Tests failed but no specific errors were identified.".to_string()
        };

        // Log analysis
        if !analysis.success {
            warn!("Test analysis found failures: {}", analysis.feedback);
            for error in &analysis.errors {
                debug!("Error: {:?}", error);
            }
        } else {
            info!("Test analysis: {}", analysis.feedback);
            if let Some(perf) = &analysis.performance_change {
                info!("Performance change: {}", perf.description);
            }
        }

        analysis
    }
}
