use std::collections::HashMap;
use crate::code_generation::generator::CodeContext;

/// A prompt template manager for generating effective prompts
pub struct PromptManager {
    /// Template cache
    templates: HashMap<String, String>,
}

impl PromptManager {
    /// Create a new prompt manager with default templates
    pub fn new() -> Self {
        let mut templates = HashMap::new();

        // Default improvement prompt
        templates.insert(
            "improvement".to_string(),
            r#"
            Your task is to improve the provided Rust code.

            ## TASK DESCRIPTION:
            {{task}}

            {{#if requirements}}
            ## REQUIREMENTS:
            {{requirements}}
            {{/if}}

            ## FILES TO MODIFY:
            {{file_paths}}

            ## CURRENT CODE:
            {{current_code}}

            {{#if previous_attempts}}
            ## PREVIOUS ATTEMPTS THAT FAILED:
            {{#each previous_attempts}}
            ### ATTEMPT:
            ```rust
            {{this.code}}
            ```

            ### FAILURE REASON:
            {{this.failure_reason}}
            {{/each}}
            {{/if}}

            ## INSTRUCTIONS:
            1. Analyze the current code and understand its purpose
            2. Identify ways to improve the code based on the task description
            3. Create a modified version that addresses the requested improvements
            4. Provide a clear explanation of what you changed and why
            5. Ensure your solution follows Rust best practices

            ## EXPECTED OUTPUT FORMAT:
            ```rust
            // Modified code here
            ```

            ## EXPLANATION:
            Provide a clear explanation of your changes.
            "#.to_string(),
        );

        // Bug fix prompt
        templates.insert(
            "bugfix".to_string(),
            r#"
            Your task is to fix a bug in the provided Rust code.

            ## BUG DESCRIPTION:
            {{task}}

            {{#if requirements}}
            ## REQUIREMENTS:
            {{requirements}}
            {{/if}}

            ## FILES TO MODIFY:
            {{file_paths}}

            ## CURRENT CODE:
            {{current_code}}

            {{#if previous_attempts}}
            ## PREVIOUS ATTEMPTS THAT FAILED:
            {{#each previous_attempts}}
            ### ATTEMPT:
            ```rust
            {{this.code}}
            ```

            ### FAILURE REASON:
            {{this.failure_reason}}
            {{/each}}
            {{/if}}

            ## INSTRUCTIONS:
            1. Analyze the current code and identify the bug
            2. Fix the bug while minimizing changes to the code
            3. Provide a clear explanation of what was wrong and how you fixed it
            4. Ensure your solution follows Rust best practices

            ## EXPECTED OUTPUT FORMAT:
            ```rust
            // Fixed code here
            ```

            ## EXPLANATION:
            Provide a clear explanation of the bug and your fix.
            "#.to_string(),
        );

        Self { templates }
    }

    /// Create a prompt for code improvement
    pub fn create_improvement_prompt(&self, context: &CodeContext, current_code: &str) -> String {
        let template = self.templates.get("improvement").unwrap();

        // Simple template substitution for now, would use a proper templating engine in a real implementation
        let mut prompt = template.replace("{{task}}", &context.task);

        if let Some(requirements) = &context.requirements {
            prompt = prompt.replace("{{#if requirements}}", "");
            prompt = prompt.replace("{{requirements}}", requirements);
            prompt = prompt.replace("{{/if}}", "");
        } else {
            prompt = prompt.replace("{{#if requirements}}\n## REQUIREMENTS:\n{{requirements}}\n{{/if}}", "");
        }

        let file_paths = context.file_paths.join("\n");
        prompt = prompt.replace("{{file_paths}}", &file_paths);

        prompt = prompt.replace("{{current_code}}", current_code);

        if !context.previous_attempts.is_empty() {
            prompt = prompt.replace("{{#if previous_attempts}}", "");
            prompt = prompt.replace("{{/if}}", "");

            let mut attempts_text = String::new();

            for attempt in &context.previous_attempts {
                let attempt_template = "### ATTEMPT:\n```rust\n{{code}}\n```\n\n### FAILURE REASON:\n{{failure_reason}}";
                let attempt_text = attempt_template
                    .replace("{{code}}", &attempt.code)
                    .replace("{{failure_reason}}", &attempt.failure_reason);

                attempts_text.push_str(&attempt_text);
                attempts_text.push_str("\n\n");
            }

            prompt = prompt.replace("{{#each previous_attempts}}\n### ATTEMPT:\n```rust\n{{this.code}}\n```\n\n### FAILURE REASON:\n{{this.failure_reason}}\n{{/each}}", &attempts_text);
        } else {
            prompt = prompt.replace("{{#if previous_attempts}}\n## PREVIOUS ATTEMPTS THAT FAILED:\n{{#each previous_attempts}}\n### ATTEMPT:\n```rust\n{{this.code}}\n```\n\n### FAILURE REASON:\n{{this.failure_reason}}\n{{/each}}\n{{/if}}", "");
        }

        prompt
    }

    /// Create a prompt for bug fixing
    pub fn create_bugfix_prompt(&self, context: &CodeContext, current_code: &str) -> String {
        let template = self.templates.get("bugfix").unwrap();

        // Simple template substitution similar to the improvement prompt
        let mut prompt = template.replace("{{task}}", &context.task);

        // Same substitution pattern as in create_improvement_prompt
        if let Some(requirements) = &context.requirements {
            prompt = prompt.replace("{{#if requirements}}", "");
            prompt = prompt.replace("{{requirements}}", requirements);
            prompt = prompt.replace("{{/if}}", "");
        } else {
            prompt = prompt.replace("{{#if requirements}}\n## REQUIREMENTS:\n{{requirements}}\n{{/if}}", "");
        }

        let file_paths = context.file_paths.join("\n");
        prompt = prompt.replace("{{file_paths}}", &file_paths);

        prompt = prompt.replace("{{current_code}}", current_code);

        if !context.previous_attempts.is_empty() {
            prompt = prompt.replace("{{#if previous_attempts}}", "");
            prompt = prompt.replace("{{/if}}", "");

            let mut attempts_text = String::new();

            for attempt in &context.previous_attempts {
                let attempt_template = "### ATTEMPT:\n```rust\n{{code}}\n```\n\n### FAILURE REASON:\n{{failure_reason}}";
                let attempt_text = attempt_template
                    .replace("{{code}}", &attempt.code)
                    .replace("{{failure_reason}}", &attempt.failure_reason);

                attempts_text.push_str(&attempt_text);
                attempts_text.push_str("\n\n");
            }

            prompt = prompt.replace("{{#each previous_attempts}}\n### ATTEMPT:\n```rust\n{{this.code}}\n```\n\n### FAILURE REASON:\n{{this.failure_reason}}\n{{/each}}", &attempts_text);
        } else {
            prompt = prompt.replace("{{#if previous_attempts}}\n## PREVIOUS ATTEMPTS THAT FAILED:\n{{#each previous_attempts}}\n### ATTEMPT:\n```rust\n{{this.code}}\n```\n\n### FAILURE REASON:\n{{this.failure_reason}}\n{{/each}}\n{{/if}}", "");
        }

        prompt
    }

    /// Add a custom template
    pub fn add_template(&mut self, name: &str, template: &str) {
        self.templates.insert(name.to_string(), template.to_string());
    }

    /// Get a template by name
    pub fn get_template(&self, name: &str) -> Option<&String> {
        self.templates.get(name)
    }
}