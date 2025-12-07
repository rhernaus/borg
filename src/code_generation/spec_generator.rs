use anyhow::{Context, Result};
use log::info;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::code_generation::generator::CodeContext;
use crate::code_generation::llm::LlmProvider;
use crate::core::optimization::OptimizationGoal;

/// Represents a specification for a code change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Specification {
    /// High-level description of what the change accomplishes
    pub description: String,

    /// Files to be created, modified, or deleted
    pub file_changes: Vec<SpecFileChange>,

    /// Expected behaviors that should be testable
    pub expected_behaviors: Vec<String>,

    /// Acceptance criteria that tests should verify
    pub acceptance_criteria: Vec<String>,
}

/// Type of file change in a specification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ChangeType {
    /// Create a new file
    Create,
    /// Modify an existing file
    Modify,
    /// Delete a file
    Delete,
}

/// A file change within a specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecFileChange {
    /// Path to the file
    pub path: String,

    /// Type of change
    pub change_type: ChangeType,

    /// Description of what changes in this file
    pub description: String,
}

/// Generates specifications from optimization goals
pub struct SpecGenerator {
    llm: Arc<dyn LlmProvider>,
}

impl SpecGenerator {
    /// Create a new spec generator with the given LLM provider
    pub fn new(llm: Arc<dyn LlmProvider>) -> Self {
        Self { llm }
    }

    /// Generate a specification for the given optimization goal
    pub async fn generate_spec(
        &self,
        goal: &OptimizationGoal,
        context: &CodeContext,
    ) -> Result<Specification> {
        info!("Generating specification for goal: {}", goal.id);

        let prompt = self.build_spec_prompt(goal, context);

        let response = self
            .llm
            .generate(&prompt, None, Some(0.3))
            .await
            .context("Failed to generate specification from LLM")?;

        self.parse_spec_response(&response)
    }

    /// Build the prompt for specification generation
    fn build_spec_prompt(&self, goal: &OptimizationGoal, context: &CodeContext) -> String {
        let mut prompt = String::new();

        prompt.push_str("You are a software architect. Generate a detailed specification for implementing the following goal.\n\n");

        prompt.push_str("## Goal\n");
        prompt.push_str(&format!("ID: {}\n", goal.id));
        prompt.push_str(&format!("Title: {}\n", goal.title));
        prompt.push_str(&format!("Description: {}\n", goal.description));

        // Extract file references from tags (format: "file:path/to/file.rs")
        let file_refs: Vec<&str> = goal
            .tags
            .iter()
            .filter_map(|tag| tag.strip_prefix("file:"))
            .collect();
        if !file_refs.is_empty() {
            prompt.push_str("Target Files:\n");
            for file_ref in file_refs {
                prompt.push_str(&format!("- {}\n", file_ref));
            }
        }
        prompt.push('\n');

        prompt.push_str("## Task Context\n");
        prompt.push_str(&format!("{}\n\n", context.task));

        if !context.file_paths.is_empty() {
            prompt.push_str("## Relevant Files\n");
            for path in &context.file_paths {
                prompt.push_str(&format!("- {}\n", path));
            }
            prompt.push('\n');
        }

        if let Some(ref contents) = context.file_contents {
            prompt.push_str("## File Contents\n");
            for (path, content) in contents {
                prompt.push_str(&format!("### {}\n```\n{}\n```\n\n", path, content));
            }
        }

        prompt.push_str("## Output Format\n");
        prompt.push_str("Respond with a JSON object containing:\n");
        prompt.push_str("- description: A high-level summary of what this change accomplishes\n");
        prompt.push_str("- file_changes: Array of {path, change_type (create/modify/delete), description}\n");
        prompt.push_str("- expected_behaviors: Array of strings describing testable behaviors\n");
        prompt.push_str("- acceptance_criteria: Array of specific criteria that tests should verify\n\n");

        prompt.push_str("Focus on WHAT should be built, not HOW. The specification should be detailed enough to write tests from.\n\n");

        prompt.push_str("```json\n");

        prompt
    }

    /// Parse the LLM response into a Specification
    fn parse_spec_response(&self, response: &str) -> Result<Specification> {
        // Extract JSON from response (it might be wrapped in markdown code blocks)
        let json_str = extract_json(response);

        serde_json::from_str(&json_str).context("Failed to parse specification JSON")
    }
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
    fn test_extract_json_from_code_block() {
        let response = r#"Here's the specification:
```json
{"description": "Test", "file_changes": [], "expected_behaviors": [], "acceptance_criteria": []}
```
"#;
        let json = extract_json(response);
        assert!(json.starts_with('{'));
        assert!(json.contains("description"));
    }

    #[test]
    fn test_extract_json_raw() {
        let response = r#"{"description": "Test", "file_changes": [], "expected_behaviors": [], "acceptance_criteria": []}"#;
        let json = extract_json(response);
        assert!(json.starts_with('{'));
    }

    #[test]
    fn test_parse_spec() {
        let json = r#"{
            "description": "Add logging to main function",
            "file_changes": [
                {"path": "src/main.rs", "change_type": "modify", "description": "Add log statements"}
            ],
            "expected_behaviors": ["Log messages should appear on startup"],
            "acceptance_criteria": ["Main function logs 'Starting application'"]
        }"#;

        let spec: Specification = serde_json::from_str(json).unwrap();
        assert_eq!(spec.description, "Add logging to main function");
        assert_eq!(spec.file_changes.len(), 1);
        assert_eq!(spec.file_changes[0].change_type, ChangeType::Modify);
    }
}
