use crate::code_generation::generator::CodeContext;
use std::collections::HashMap;

/// A prompt template manager for generating effective prompts
pub struct PromptManager {
    /// Template cache
    templates: HashMap<String, String>,
}

impl Default for PromptManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PromptManager {
    /// Create a new prompt manager with default templates
    pub fn new() -> Self {
        let mut templates = HashMap::new();

        // System message for Rust code generation
        templates.insert(
            "system_message".to_string(),
            r#"You are an expert Rust developer specializing in high-performance, memory-safe, and reliable code.
Your code follows these principles:
1. Memory safety - You leverage Rust's ownership system correctly, avoiding unsafe blocks unless absolutely necessary.
2. Error handling - You use Result and Option types properly, with appropriate error propagation and handling.
3. Performance - You understand zero-cost abstractions and write efficient code without unnecessary allocations.
4. Readability - Your code is idiomatic Rust with clear naming conventions and appropriate documentation.
5. Testability - You write code that is easy to test and include test examples where appropriate.

When improving code, ensure that:
- You maintain or improve thread safety where applicable
- You use Result instead of panicking for recoverable errors
- You leverage the type system to prevent errors at compile time
- You follow the Rust API guidelines for public interfaces
- You use appropriate lifetime annotations where needed
- You handle all error cases explicitly

Whenever possible, use Rust's standard library and well-established crates rather than reinventing functionality.
"#.to_string(),
        );

        // Enhanced improvement prompt
        templates.insert(
            "improvement".to_string(),
            r#"
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

## RUST BEST PRACTICES TO APPLY:
- Leverage Rust's ownership model for memory safety
- Use Result<T, E> for recoverable errors, not unwrap() or expect() in production code
- Implement appropriate traits (Debug, Clone, etc.) when needed
- Use iterators and functional programming patterns when appropriate
- Structure code in modules for proper organization
- Use meaningful variable and function names that follow Rust conventions
- Add appropriate documentation comments (///) for public APIs
- Add unit tests for new functionality
- Consider performance implications, especially for hot paths
- Use appropriate lifetime annotations where needed
- Ensure thread safety with proper use of Arc, Mutex, etc. where appropriate

## EXPECTED OUTPUT FORMAT:
For each file you modify, include the complete modified file in this format:

```rust
// File: path/to/file.rs
// Modified file content here
```

## EXPLANATION:
After the code blocks, provide a detailed explanation of:
1. What you changed
2. Why you made these changes
3. How your changes improve the code
4. Any trade-offs or considerations for your implementation

Be specific about memory safety, error handling, and performance implications.
"#
            .to_string(),
        );

        // Enhanced bug fix prompt
        templates.insert(
            "bugfix".to_string(),
            r#"
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

## COMMON RUST BUGS TO CHECK FOR:
- Ownership/borrowing issues (e.g., use of moved values, reference lifetimes)
- Concurrency bugs (e.g., data races, deadlocks)
- Improper error handling (e.g., swallowed errors, unwrapped Results/Options that can fail)
- Type conversion issues (e.g., as casts that might panic)
- Logic errors in dealing with Option/Result types
- Resource leaks (e.g., unclosed files, connections)
- Integer overflow/underflow
- Off-by-one errors in ranges or indexing
- Missing error propagation with `?` operator
- Improper use of unsafe code
- Infinite loops or recursion

## EXPECTED OUTPUT FORMAT:
For each file you modify, include the complete modified file in this format:

```rust
// File: path/to/file.rs
// Modified file content here
```

## EXPLANATION:
After the code blocks, provide a detailed explanation of:
1. What the bug was
2. Root cause analysis
3. How your changes fix the issue
4. How to prevent similar bugs in the future
"#
            .to_string(),
        );

        // New feature implementation prompt
        templates.insert(
            "feature".to_string(),
            r#"
## FEATURE DESCRIPTION:
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
1. Analyze the current codebase to understand the architecture
2. Design and implement the new feature according to the description
3. Ensure the implementation follows Rust best practices
4. Maintain compatibility with the existing codebase
5. Add appropriate error handling, documentation, and tests

## IMPLEMENTATION GUIDELINES:
- Follow existing patterns and coding style for consistency
- Use traits for abstraction when appropriate
- Implement proper error handling with custom error types if needed
- Ensure thread safety if the feature might be used in concurrent contexts
- Add appropriate logging at key points
- Keep functions focused and modular
- Consider performance implications, especially for operations that might scale
- Add unit tests that cover happy path and error cases

## EXPECTED OUTPUT FORMAT:
For each file you modify, include the complete modified file in this format:

```rust
// File: path/to/file.rs
// Modified file content here
```

For new files, include the complete file content in this format:

```rust
// File: path/to/new_file.rs
// New file content here
```

## EXPLANATION:
After the code blocks, provide a detailed explanation of:
1. Your implementation approach
2. Key design decisions and alternatives considered
3. How your implementation satisfies the requirements
4. Any areas that might need further refinement
"#
            .to_string(),
        );

        // Refactoring prompt
        templates.insert(
            "refactor".to_string(),
            r#"
## REFACTORING TASK:
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
1. Analyze the current code to understand its functionality
2. Refactor the code while preserving its behavior
3. Apply Rust best practices and improve code quality
4. Ensure the refactored code is more maintainable, efficient, or readable

## REFACTORING PRINCIPLES:
- Extract reusable logic into functions or traits
- Remove code duplication
- Improve variable and function naming
- Use appropriate Rust patterns (builder, visitor, etc.) when applicable
- Replace imperative code with functional/iterator patterns where appropriate
- Simplify complex logic
- Improve error handling
- Enhance documentation
- Consider adding unit tests to verify behavior preservation

## EXPECTED OUTPUT FORMAT:
For each file you modify, include the complete modified file in this format:

```rust
// File: path/to/file.rs
// Modified file content here
```

## EXPLANATION:
After the code blocks, provide a detailed explanation of:
1. What you refactored and why
2. How your changes improve the code
3. What code quality aspects have been enhanced
4. Any performance or safety improvements
"#
            .to_string(),
        );

        // Add template for Git operations
        templates.insert(
            "git_operations".to_string(),
            r#"
## GIT OPERATIONS GUIDE

You have access to a GitCommandTool that allows you to execute Git commands directly. This tool provides you with flexibility to handle complex Git scenarios that may be difficult to express programmatically.

### AVAILABLE GIT COMMANDS:

You can execute standard Git commands such as:
- git status
- git add <files>
- git commit -m "message"
- git branch <branch-name>
- git checkout <branch-name>
- git merge <branch-name>
- git log
- git diff
- git pull
- git push (if configured)

### HOW TO CALL THE TOOL:

To execute a Git command, use this exact JSON format:
```
{"tool": "git_command", "args": ["your_git_command_here"]}
```

For example:
```
{"tool": "git_command", "args": ["status"]}
```

Or:
```
{"tool": "git_command", "args": ["log", "-n", "5"]}
```

### SAFETY CONSTRAINTS:

For safety reasons, certain destructive Git commands are restricted:
- Commands involving `--force` or `-f` flags
- `git clean` commands
- Hard resets (`git reset --hard`)
- Any command with shell escape characters or pipes

### BEST PRACTICES:

1. **Check State First**: Always check the repository state before making changes (use `git status`)
2. **Handle Errors**: Check command output for errors and handle them appropriately
3. **Atomic Operations**: Keep Git operations small and focused
4. **Clear Commit Messages**: Use descriptive commit messages that explain the "why" not just the "what"
5. **Branch Management**: Create feature branches for new work
6. **Conflict Resolution**: When conflicts occur, analyze the conflict and resolve appropriately

### EXAMPLE WORKFLOW:

1. Check current status: `{"tool": "git_command", "args": ["status"]}`
2. Create a new branch: `{"tool": "git_command", "args": ["branch", "feature-x"]}`
3. Switch to the branch: `{"tool": "git_command", "args": ["checkout", "feature-x"]}`
4. Make code changes (using other tools)
5. Check changes: `{"tool": "git_command", "args": ["status"]}`
6. Stage changes: `{"tool": "git_command", "args": ["add", "src/modified_file.rs"]}`
7. Commit changes: `{"tool": "git_command", "args": ["commit", "-m", "Implement feature X"]}`
8. Check log: `{"tool": "git_command", "args": ["log", "-n", "1"]}`
9. Switch back to main: `{"tool": "git_command", "args": ["checkout", "main"]}`
10. Merge changes: `{"tool": "git_command", "args": ["merge", "feature-x"]}`

### HANDLING MERGE CONFLICTS:

If a merge conflict occurs:
1. Identify conflicted files from command output
2. Use FileContentsTool to read the conflicted files
3. Analyze the conflicts (marked with <<<<<<< HEAD, =======, and >>>>>>> branch)
4. Use ModifyFileTool to resolve conflicts
5. Stage resolved files: `{"tool": "git_command", "args": ["add", "<resolved-files>"]}`
6. Complete the merge: `{"tool": "git_command", "args": ["commit", "-m", "Resolve merge conflicts"]}`

Remember to approach Git operations with care and maintain the integrity of the repository.
"#.to_string(),
        );

        Self { templates }
    }

    /// Get the system message template
    pub fn create_system_message(&self) -> String {
        self.templates
            .get("system_message")
            .expect("System message template not found")
            .clone()
    }

    /// Create a prompt for code improvement
    pub fn create_improvement_prompt(&self, context: &CodeContext, current_code: &str) -> String {
        let template = self.templates.get("improvement").unwrap();
        let system_message = self.templates.get("system_message").unwrap();

        // Combine system message and improvement template
        let full_template = format!("{}\n\n{}", system_message, template);

        // Simple template substitution for now, would use a proper templating engine in a real implementation
        let mut prompt = full_template.replace("{{task}}", &context.task);

        if let Some(requirements) = &context.requirements {
            prompt = prompt.replace("{{#if requirements}}", "");
            prompt = prompt.replace("{{requirements}}", requirements);
            prompt = prompt.replace("{{/if}}", "");
        } else {
            prompt = prompt.replace(
                "{{#if requirements}}\n## REQUIREMENTS:\n{{requirements}}\n{{/if}}",
                "",
            );
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
        let system_message = self.templates.get("system_message").unwrap();

        // Combine system message and bugfix template
        let full_template = format!("{}\n\n{}", system_message, template);

        // Simple template substitution similar to the improvement prompt
        let mut prompt = full_template.replace("{{task}}", &context.task);

        // Same substitution pattern as in create_improvement_prompt
        if let Some(requirements) = &context.requirements {
            prompt = prompt.replace("{{#if requirements}}", "");
            prompt = prompt.replace("{{requirements}}", requirements);
            prompt = prompt.replace("{{/if}}", "");
        } else {
            prompt = prompt.replace(
                "{{#if requirements}}\n## REQUIREMENTS:\n{{requirements}}\n{{/if}}",
                "",
            );
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

    /// Create a prompt for new feature implementation
    pub fn create_feature_prompt(&self, context: &CodeContext, current_code: &str) -> String {
        let template = self.templates.get("feature").unwrap();
        let system_message = self.templates.get("system_message").unwrap();

        // Combine system message and feature template
        let full_template = format!("{}\n\n{}", system_message, template);

        // Template substitution (similar pattern to other methods)
        let mut prompt = full_template.replace("{{task}}", &context.task);

        if let Some(requirements) = &context.requirements {
            prompt = prompt.replace("{{#if requirements}}", "");
            prompt = prompt.replace("{{requirements}}", requirements);
            prompt = prompt.replace("{{/if}}", "");
        } else {
            prompt = prompt.replace(
                "{{#if requirements}}\n## REQUIREMENTS:\n{{requirements}}\n{{/if}}",
                "",
            );
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

    /// Create a prompt for code refactoring
    pub fn create_refactor_prompt(&self, context: &CodeContext, current_code: &str) -> String {
        let template = self.templates.get("refactor").unwrap();
        let system_message = self.templates.get("system_message").unwrap();

        // Combine system message and refactor template
        let full_template = format!("{}\n\n{}", system_message, template);

        // Template substitution (similar pattern to other methods)
        let mut prompt = full_template.replace("{{task}}", &context.task);

        if let Some(requirements) = &context.requirements {
            prompt = prompt.replace("{{#if requirements}}", "");
            prompt = prompt.replace("{{requirements}}", requirements);
            prompt = prompt.replace("{{/if}}", "");
        } else {
            prompt = prompt.replace(
                "{{#if requirements}}\n## REQUIREMENTS:\n{{requirements}}\n{{/if}}",
                "",
            );
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

    /// Create a prompt for Git operations
    pub fn create_git_operations_prompt(&self) -> String {
        let git_template = self.templates.get("git_operations").unwrap();
        let system_message = self.templates.get("system_message").unwrap();

        // Combine system message and git operations template
        format!("{}\n\n{}", system_message, git_template)
    }

    /// Add a custom template
    pub fn add_template(&mut self, name: &str, template: &str) {
        self.templates
            .insert(name.to_string(), template.to_string());
    }

    /// Get a template by name
    pub fn get_template(&self, name: &str) -> Option<&String> {
        self.templates.get(name)
    }
}
