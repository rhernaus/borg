use anyhow::{Context, Result};
use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

use crate::code_generation::generator::{CodeContext, CodeGenerator, CodeImprovement, FileChange, PreviousAttempt};
use crate::code_generation::llm::{LlmFactory, LlmProvider};
use crate::code_generation::prompt::PromptManager;
use crate::core::config::LlmConfig;
use crate::version_control::git::GitManager;

/// A code generator that uses LLM to generate code improvements
pub struct LlmCodeGenerator {
    /// The LLM provider
    llm: Box<dyn LlmProvider>,

    /// The prompt manager
    prompt_manager: PromptManager,

    /// The git manager for retrieving code
    git_manager: Arc<tokio::sync::Mutex<dyn GitManager>>,
}

impl LlmCodeGenerator {
    /// Create a new LLM code generator
    pub fn new(llm_config: LlmConfig, git_manager: Arc<tokio::sync::Mutex<dyn GitManager>>) -> Result<Self> {
        let llm = LlmFactory::create(llm_config)
            .context("Failed to create LLM provider")?;

        let prompt_manager = PromptManager::new();

        Ok(Self {
            llm,
            prompt_manager,
            git_manager,
        })
    }

    /// Extract code from LLM response
    fn extract_code_from_response(&self, response: &str) -> Result<Vec<FileChange>> {
        // Basic implementation - in a real system this would be more robust
        let mut file_changes = Vec::new();

        // Split the response by file markers and code blocks
        let mut current_file = None;
        let mut in_code_block = false;
        let mut code = String::new();

        for line in response.lines() {
            let trimmed = line.trim();

            // Look for file path indicators (common patterns in LLM responses)
            if trimmed.starts_with("File:") || trimmed.starts_with("```") && trimmed.contains(".rs") {
                // If we were already processing a file, save it
                if in_code_block && current_file.is_some() {
                    file_changes.push(FileChange {
                        file_path: current_file.take().unwrap(),
                        start_line: None, // We don't have line information from the LLM
                        end_line: None,
                        new_content: code.clone(),
                    });
                    code.clear();
                }

                // Extract file path
                if trimmed.starts_with("File:") {
                    current_file = Some(trimmed.trim_start_matches("File:").trim().to_string());
                } else if trimmed.starts_with("```") && trimmed.contains(".rs") {
                    // Extract from code block markers with filename
                    let parts: Vec<&str> = trimmed.split_whitespace().collect();
                    if parts.len() >= 2 && parts[1].ends_with(".rs") {
                        current_file = Some(parts[1].to_string());
                    }
                }

                in_code_block = true;
                continue;
            }

            // End of code block
            if trimmed == "```" && in_code_block {
                if let Some(file) = current_file.take() {
                    file_changes.push(FileChange {
                        file_path: file,
                        start_line: None,
                        end_line: None,
                        new_content: code.clone(),
                    });
                    code.clear();
                }
                in_code_block = false;
                continue;
            }

            // Collect code content
            if in_code_block && current_file.is_some() {
                code.push_str(line);
                code.push('\n');
            }
        }

        // Handle any remaining code
        if in_code_block && current_file.is_some() {
            file_changes.push(FileChange {
                file_path: current_file.take().unwrap(),
                start_line: None,
                end_line: None,
                new_content: code,
            });
        }

        Ok(file_changes)
    }

    /// Fetch code content for the given file paths
    async fn fetch_code_content(&self, file_paths: &[String]) -> Result<String> {
        let mut content = String::new();

        let git_manager = self.git_manager.lock().await;

        for file_path in file_paths {
            let file_content = match git_manager.read_file(file_path).await {
                Ok(content) => content,
                Err(e) => {
                    log::warn!("Failed to read file {}: {}. Assuming it's a new file.", file_path, e);
                    format!("// This is a new file that needs to be created: {}", file_path)
                }
            };

            content.push_str(&format!("### FILE: {}\n", file_path));
            content.push_str("```rust\n");
            content.push_str(&file_content);
            content.push_str("\n```\n\n");
        }

        Ok(content)
    }
}

#[async_trait]
impl CodeGenerator for LlmCodeGenerator {
    async fn generate_improvement(&self, context: &CodeContext) -> Result<CodeImprovement> {
        // Fetch content of all relevant files
        let current_code = self.fetch_code_content(&context.file_paths).await?;

        // Determine the appropriate prompt type based on the task description
        let prompt = if context.task.to_lowercase().contains("bug") || context.task.to_lowercase().contains("fix") {
            log::info!("Using bugfix prompt for task: {}", context.task);
            self.prompt_manager.create_bugfix_prompt(context, &current_code)
        } else if context.task.to_lowercase().contains("feature") || context.task.to_lowercase().contains("implement") || context.task.to_lowercase().contains("add") {
            log::info!("Using feature prompt for task: {}", context.task);
            self.prompt_manager.create_feature_prompt(context, &current_code)
        } else if context.task.to_lowercase().contains("refactor") || context.task.to_lowercase().contains("restructure") || context.task.to_lowercase().contains("simplify") {
            log::info!("Using refactor prompt for task: {}", context.task);
            self.prompt_manager.create_refactor_prompt(context, &current_code)
        } else {
            log::info!("Using general improvement prompt for task: {}", context.task);
            self.prompt_manager.create_improvement_prompt(context, &current_code)
        };

        log::debug!("Generated prompt for LLM: \n{}", prompt);

        // Ask the LLM with appropriate parameters based on the task
        let max_tokens = Some(4096); // Increased token limit for more detailed responses
        let temperature = if context.task.to_lowercase().contains("bug") || context.task.to_lowercase().contains("fix") {
            // Lower temperature for bug fixes to get more deterministic outputs
            Some(0.2)
        } else if context.task.to_lowercase().contains("feature") || context.task.to_lowercase().contains("innovative") {
            // Higher temperature for features to encourage creativity
            Some(0.7)
        } else {
            // Balanced temperature for most improvements
            Some(0.4)
        };

        let response = match self.llm.generate(&prompt, max_tokens, temperature).await {
            Ok(resp) => resp,
            Err(e) => {
                log::error!("LLM API error: {}", e);
                return Err(anyhow::anyhow!("Failed to generate code improvement: {}", e));
            }
        };

        // Extract code changes from the response
        let target_files = self.extract_code_from_response(&response)?;

        // Generate a unique ID
        let id = Uuid::new_v4().to_string();

        // Extract explanation (everything after ## EXPLANATION: if it exists)
        let explanation = if let Some(idx) = response.find("## EXPLANATION:") {
            response[idx..].to_string()
        } else {
            "No explicit explanation provided by LLM.".to_string()
        };

        // Create the improvement
        let improvement = CodeImprovement {
            id,
            task: context.task.clone(),
            code: response.clone(),
            target_files,
            explanation,
        };

        Ok(improvement)
    }

    async fn provide_feedback(&self, improvement: &CodeImprovement, success: bool, feedback: &str) -> Result<()> {
        // This is a simple implementation
        // In a real system, we might store this feedback for future reference
        // or use it to fine-tune the LLM

        log::info!(
            "Feedback for improvement {}: Success={}, Feedback={}",
            improvement.id,
            success,
            feedback
        );

        // We could also log this to a database or send it to a feedback API

        Ok(())
    }
}