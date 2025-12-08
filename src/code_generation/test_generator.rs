use anyhow::{Context, Result};
use log::info;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::code_generation::generator::CodeContext;
use crate::code_generation::llm::LlmProvider;
use crate::code_generation::spec_generator::Specification;

/// Generated tests for a specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedTests {
    /// Path where the test file should be written
    pub test_file_path: String,

    /// The generated test code
    pub test_code: String,

    /// Names of the individual tests
    pub test_names: Vec<String>,
}

/// A failing test with detailed information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailingTest {
    /// Name of the failing test
    pub name: String,

    /// Error message from the test
    pub error_message: String,

    /// Expected value (if available)
    pub expected: Option<String>,

    /// Actual value (if available)
    pub actual: Option<String>,
}

/// Generates tests from specifications
pub struct TestGenerator {
    llm: Arc<dyn LlmProvider>,
}

impl TestGenerator {
    /// Create a new test generator with the given LLM provider
    pub fn new(llm: Arc<dyn LlmProvider>) -> Self {
        Self { llm }
    }

    /// Generate tests for the given specification
    pub async fn generate_tests(
        &self,
        spec: &Specification,
        context: &CodeContext,
    ) -> Result<GeneratedTests> {
        info!("Generating tests for specification: {}", spec.description);

        let prompt = self.build_test_prompt(spec, context);

        let response = self
            .llm
            .generate(&prompt, None, Some(0.2))
            .await
            .context("Failed to generate tests from LLM")?;

        self.parse_test_response(&response, context)
    }

    /// Build the prompt for test generation
    fn build_test_prompt(&self, spec: &Specification, context: &CodeContext) -> String {
        let mut prompt = String::new();

        prompt.push_str(
            "You are a test engineer. Generate Rust tests for the following specification.\n",
        );
        prompt
            .push_str("The tests should verify ALL acceptance criteria and expected behaviors.\n");
        prompt.push_str("Tests should FAIL initially (red phase of TDD) since the implementation doesn't exist yet.\n\n");

        prompt.push_str("## Specification\n");
        prompt.push_str(&format!("Description: {}\n\n", spec.description));

        prompt.push_str("### Expected Behaviors\n");
        for behavior in &spec.expected_behaviors {
            prompt.push_str(&format!("- {}\n", behavior));
        }
        prompt.push('\n');

        prompt.push_str("### Acceptance Criteria\n");
        for criterion in &spec.acceptance_criteria {
            prompt.push_str(&format!("- {}\n", criterion));
        }
        prompt.push('\n');

        prompt.push_str("### Files Being Changed\n");
        for file_change in &spec.file_changes {
            prompt.push_str(&format!(
                "- {} ({}): {}\n",
                file_change.path,
                format!("{:?}", file_change.change_type).to_lowercase(),
                file_change.description
            ));
        }
        prompt.push('\n');

        // Include existing test patterns if available
        if let Some(ref test_contents) = context.test_contents {
            if !test_contents.is_empty() {
                prompt.push_str("### Existing Test Patterns (follow these patterns)\n");
                for (path, content) in test_contents.iter().take(2) {
                    // Limit to avoid huge prompts
                    prompt.push_str(&format!("```rust\n// From {}\n{}\n```\n\n", path, content));
                }
            }
        }

        prompt.push_str("## Output Format\n");
        prompt.push_str("Respond with a JSON object containing:\n");
        prompt.push_str("- test_file_path: Path where the test should be written (e.g., \"tests/feature_test.rs\" or \"src/module/tests.rs\")\n");
        prompt.push_str("- test_code: Complete Rust test code\n");
        prompt.push_str("- test_names: Array of test function names\n\n");

        prompt.push_str("Requirements:\n");
        prompt.push_str("- Use #[test] attribute for each test\n");
        prompt.push_str("- Include necessary imports\n");
        prompt.push_str("- Each acceptance criterion should have at least one test\n");
        prompt.push_str("- Tests should be clear and focused\n\n");

        prompt.push_str("```json\n");

        prompt
    }

    /// Parse the LLM response into GeneratedTests
    fn parse_test_response(
        &self,
        response: &str,
        _context: &CodeContext,
    ) -> Result<GeneratedTests> {
        let json_str = extract_json(response);

        serde_json::from_str(&json_str).context("Failed to parse test generation JSON")
    }
}

/// Parse test output to identify failing tests
pub fn parse_test_failures(test_output: &str) -> Vec<FailingTest> {
    let mut failures = Vec::new();
    let mut current_test: Option<String> = None;
    let mut current_error = String::new();

    for line in test_output.lines() {
        // Detect test failure start
        if line.contains("---- ") && line.contains(" ----") {
            // Save previous failure if any
            if let Some(ref name) = current_test {
                if !current_error.is_empty() {
                    failures.push(FailingTest {
                        name: name.clone(),
                        error_message: current_error.trim().to_string(),
                        expected: None,
                        actual: None,
                    });
                }
            }

            // Extract test name
            let test_name = line
                .trim_start_matches("---- ")
                .trim_end_matches(" ----")
                .trim_end_matches(" stdout")
                .to_string();
            current_test = Some(test_name);
            current_error = String::new();
        }
        // Collect error message lines
        else if current_test.is_some() {
            let is_panic = line.starts_with("thread '") && line.contains("panicked at");
            let is_assertion = line.contains("assertion") || line.contains("expected");
            if is_panic || is_assertion {
                current_error.push_str(line);
                current_error.push('\n');
            }
        }
    }

    // Don't forget the last failure
    if let Some(name) = current_test {
        if !current_error.is_empty() {
            failures.push(FailingTest {
                name,
                error_message: current_error.trim().to_string(),
                expected: None,
                actual: None,
            });
        }
    }

    failures
}

/// Extract JSON from a response that might be wrapped in markdown code blocks
fn extract_json(response: &str) -> String {
    // Try to find JSON in code blocks first
    if let Some(start) = response.find("```json") {
        let after_marker = &response[start + 7..];
        if let Some(end) = after_marker.find("```") {
            return after_marker[..end].trim().to_string();
        }
    }

    // Try plain code blocks
    if let Some(start) = response.find("```") {
        let after_marker = &response[start + 3..];
        if let Some(end) = after_marker.find("```") {
            return after_marker[..end].trim().to_string();
        }
    }

    // Try to find raw JSON object
    if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            return response[start..=end].to_string();
        }
    }

    response.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_test_failures() {
        let output = r#"
running 2 tests
test test_add ... ok
test test_subtract ... FAILED

failures:

---- test_subtract stdout ----
thread 'test_subtract' panicked at 'assertion failed: `(left == right)`
  left: `5`,
 right: `3`', src/lib.rs:10:9

failures:
    test_subtract
"#;

        let failures = parse_test_failures(output);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].name, "test_subtract");
        assert!(failures[0].error_message.contains("assertion failed"));
    }

    #[test]
    fn test_generated_tests_serialization() {
        let tests = GeneratedTests {
            test_file_path: "tests/my_test.rs".to_string(),
            test_code: "#[test]\nfn test_something() {}".to_string(),
            test_names: vec!["test_something".to_string()],
        };

        let json = serde_json::to_string(&tests).unwrap();
        let parsed: GeneratedTests = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.test_file_path, "tests/my_test.rs");
    }
}
