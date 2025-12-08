use anyhow::{Context, Result};
use async_trait::async_trait;
use log::{info, warn};
use regex::Regex;
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::code_generation::generator::{CodeContext, CodeGenerator, CodeImprovement, FileChange};
use crate::code_generation::llm::{LlmFactory, LlmProvider};
use crate::code_generation::llm_tool::{
    BashTool, CompilationFeedbackTool, EditTool, FindTestsTool, GitCommandTool, GitHistoryTool,
    GrepTool, LlmTool, ReadTool, TestRunnerTool, ToolCall, ToolRegistry, ToolResult, WriteTool,
};
use crate::code_generation::prompt::PromptManager;
use crate::core::config::{CodeGenerationConfig, LlmConfig, LlmLoggingConfig};
use crate::core::error::ProviderError;
use crate::providers::{
    ContentPart as UnifiedContentPart, GenerateRequest as UnifiedGenerateRequest,
    Message as UnifiedMessage, Provider as UnifiedProvider, Role as UnifiedRole, StreamEvent,
    ToolCallNormalized, ToolChoice as UnifiedToolChoice, ToolSpec as UnifiedToolSpec, Usage,
};
use crate::version_control::git::GitManager;

/// A code generator that uses LLM to generate code improvements
pub struct LlmCodeGenerator {
    /// The LLM provider
    llm: Box<dyn LlmProvider>,

    /// Unified provider adapter (when available; used for streaming with events/tool-calls)
    provider_adapter: Option<Box<dyn UnifiedProvider>>,

    /// The prompt manager
    prompt_manager: PromptManager,

    /// The git manager for retrieving code
    git_manager: Arc<Mutex<dyn GitManager>>,

    /// The workspace path
    workspace: PathBuf,

    /// Maximum number of iterations for tool usage
    max_tool_iterations: usize,

    /// Whether to use tools for code generation
    use_tools: bool,

    /// Registry of available tools
    tool_registry: ToolRegistry,
}

impl LlmCodeGenerator {
    /// Create a new LLM code generator
    pub fn new(
        llm_config: LlmConfig,
        code_gen_config: CodeGenerationConfig,
        llm_logging_config: LlmLoggingConfig,
        git_manager: Arc<Mutex<dyn GitManager>>,
        workspace: PathBuf,
    ) -> Result<Self> {
        let llm_cfg_clone = llm_config.clone();
        let llm = LlmFactory::create(llm_config, llm_logging_config)
            .context("Failed to create LLM provider")?;

        // Create unified provider adapter when available (Anthropic/OpenRouter)
        let provider_adapter: Option<Box<dyn UnifiedProvider>> =
            match llm_cfg_clone.provider.as_str() {
                "anthropic" => Some(Box::new(
                    crate::providers::anthropic::AnthropicProvider::from_config(&llm_cfg_clone)
                        .map_err(|e| anyhow::anyhow!(e.to_string()))?,
                )),
                "openrouter" => Some(Box::new(
                    crate::providers::openrouter::OpenRouterProvider::from_config(&llm_cfg_clone)
                        .map_err(|e| anyhow::anyhow!(e.to_string()))?,
                )),
                _ => None,
            };

        let prompt_manager = PromptManager::new();

        // Use configuration values or defaults
        let max_tool_iterations = code_gen_config.max_tool_iterations;
        let use_tools = code_gen_config.use_tools;

        // Initialize tool registry
        let mut tool_registry = ToolRegistry::new();

        // Register available tools
        tool_registry.register(GrepTool::new(workspace.clone(), Arc::clone(&git_manager)));
        tool_registry.register(ReadTool::new(workspace.clone()));
        tool_registry.register(FindTestsTool::new(workspace.clone()));
        tool_registry.register(BashTool::new(workspace.clone()));
        tool_registry.register(BashTool::new(workspace.clone()));
        tool_registry.register(GitHistoryTool::new(
            workspace.clone(),
            Arc::clone(&git_manager),
        ));
        tool_registry.register(CompilationFeedbackTool::new(workspace.clone()));
        tool_registry.register(WriteTool::new(workspace.clone()));
        tool_registry.register(EditTool::new(workspace.clone()));
        tool_registry.register(GitCommandTool::new(workspace.clone()));
        tool_registry.register(TestRunnerTool::new(workspace.clone()));

        Ok(Self {
            llm,
            provider_adapter,
            prompt_manager,
            git_manager,
            workspace,
            max_tool_iterations,
            use_tools,
            tool_registry,
        })
    }

    /// Extract code from LLM response
    fn extract_code_from_response(&self, response: &str) -> Result<Vec<FileChange>> {
        let re = Regex::new(r"```(?:rust|rs)?\s*(?:\n|\r\n)([\s\S]*?)```").unwrap();
        let mut changes = Vec::new();

        // Let's start by looking for specific files called out with path comments
        let file_re = Regex::new(r#"(?i)for\s+file\s+(?:"|`)?([\w./\\-]+)(?:"|`)?|file:\s*(?:"|`)?([\w./\\-]+)(?:"|`)?|filename:\s*(?:"|`)?([\w./\\-]+)(?:"|`)?"#).unwrap();

        for cap in re.captures_iter(response) {
            let code_block = cap[1].to_string();
            let mut file_path = String::new();

            // Look for a file path in close proximity to this code block
            // First check lines right before the code block
            let code_start_index = response.find(&code_block).unwrap_or(0);
            let pre_code = &response[..code_start_index];

            // Look for the last file path mention before this code block
            if let Some(file_cap) = file_re.captures_iter(pre_code).last() {
                file_path = file_cap
                    .get(1)
                    .or(file_cap.get(2))
                    .or(file_cap.get(3))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
            }

            // If no file path found, use a default
            if file_path.is_empty() {
                file_path = "src/main.rs".to_string();
            }

            changes.push(FileChange {
                file_path,
                start_line: None,
                end_line: None,
                new_content: code_block,
            });
        }

        if changes.is_empty() {
            // If no code blocks found, try to look for file path mentions anyway
            for cap in file_re.captures_iter(response) {
                let file_path = cap
                    .get(1)
                    .or(cap.get(2))
                    .or(cap.get(3))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
                if !file_path.is_empty() {
                    changes.push(FileChange {
                        file_path,
                        start_line: None,
                        end_line: None,
                        new_content: "// File mentioned but no code provided".to_string(),
                    });
                }
            }
        }

        Ok(changes)
    }

    /// Fetch code content from Git
    async fn fetch_code_content(&self, file_path: &str) -> Result<String> {
        let git_manager = self.git_manager.lock().await;
        match git_manager.read_file(file_path).await {
            Ok(content) => Ok(content),
            Err(e) => {
                info!("Failed to read file {}: {}", file_path, e);
                Ok(format!(
                    "// File {} does not exist or cannot be read",
                    file_path
                ))
            }
        }
    }

    /// Build provider tool specifications based on registered tools (JSON schema)
    fn build_unified_tool_specs(&self) -> Vec<UnifiedToolSpec> {
        let mut out = Vec::new();
        for (name, description, params) in self.tool_registry.get_tool_specifications() {
            // Build a simple JSON schema for parameters in declared order
            let mut schema_obj = serde_json::Map::new();
            schema_obj.insert("type".into(), JsonValue::String("object".into()));

            let mut properties = serde_json::Map::new();
            let mut required: Vec<String> = Vec::new();
            for p in params {
                // Best-effort type mapping
                let typ = "string"; // keep string; tools parse internally
                let mut prop = serde_json::Map::new();
                prop.insert("type".into(), JsonValue::String(typ.to_string()));
                if let Some(def) = p.default_value {
                    prop.insert("default".into(), JsonValue::String(def));
                }
                properties.insert(p.name.clone(), JsonValue::Object(prop));
                if p.required {
                    required.push(p.name.clone());
                }
            }
            schema_obj.insert("properties".into(), JsonValue::Object(properties));
            if !required.is_empty() {
                schema_obj.insert(
                    "required".into(),
                    JsonValue::Array(required.into_iter().map(JsonValue::String).collect()),
                );
            }

            out.push(UnifiedToolSpec {
                name,
                description: Some(description),
                json_schema: Some(JsonValue::Object(schema_obj)),
            });
        }
        out
    }

    /// Map a normalized tool call to ToolRegistry argument list, using parameter specs
    fn map_normalized_tool_to_args(&self, tc: &ToolCallNormalized) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();
        let mut params_for_tool: Option<Vec<crate::code_generation::llm_tool::ToolParameter>> =
            None;
        for (n, _desc, params) in self.tool_registry.get_tool_specifications() {
            if n == tc.name {
                params_for_tool = Some(params);
                break;
            }
        }
        if let Some(params) = params_for_tool {
            for p in params {
                if let Some(v) = tc.arguments_json.get(&p.name) {
                    if let Some(s) = v.as_str() {
                        args.push(s.to_string());
                    } else if let Some(b) = v.as_bool() {
                        args.push(b.to_string());
                    } else if let Some(n) = v.as_i64() {
                        args.push(n.to_string());
                    } else if let Some(n) = v.as_u64() {
                        args.push(n.to_string());
                    } else if let Some(n) = v.as_f64() {
                        args.push(n.to_string());
                    } else {
                        args.push(v.to_string());
                    }
                } else if let Some(def) = p.default_value {
                    args.push(def);
                } else {
                    // Missing required; push empty to preserve arity (tool may validate)
                    args.push(String::new());
                }
            }
        } else {
            // Unknown tool; pass entire JSON as single arg for debugging
            args.push(tc.arguments_json.to_string());
        }
        args
    }

    /// Execute a normalized tool call via ToolRegistry (public for tests)
    pub async fn execute_normalized_tool_call(&self, tc: &ToolCallNormalized) -> ToolResult {
        let args = self.map_normalized_tool_to_args(tc);
        let call = ToolCall {
            tool: tc.name.clone(),
            args,
        };
        self.tool_registry.execute(&call).await
    }

    /// Stream via unified provider adapter and collect events, text, and usage
    async fn stream_with_unified_provider(
        &self,
        prompt: &str,
        max_tokens: Option<usize>,
        temperature: Option<f32>,
        print_tokens: bool,
        include_tools: bool,
    ) -> Result<(String, Vec<ToolCallNormalized>, Option<Usage>)> {
        if self.provider_adapter.is_none() {
            // Fallback: legacy LLM interface (no events)
            let text = self
                .llm
                .generate_streaming(prompt, max_tokens, temperature, print_tokens)
                .await?;
            return Ok((text, Vec::new(), None));
        }

        info!(
            "Starting unified streaming (include_tools={}, print_tokens={})",
            include_tools, print_tokens
        );

        let mut content = String::new();
        let mut tool_calls: Vec<ToolCallNormalized> = Vec::new();
        let mut usage: Option<Usage> = None;

        // Prepare request
        let mut req = UnifiedGenerateRequest {
            system: Some(
                "You are an AI assistant that helps with coding in Rust. You provide clear, concise, and correct code."
                    .to_string(),
            ),
            messages: vec![UnifiedMessage {
                role: UnifiedRole::User,
                content: vec![UnifiedContentPart::Text {
                    text: prompt.to_string(),
                }],
            }],
            tools: None,
            tool_choice: None,
            temperature,
            top_p: None,
            stop: None,
            seed: None,
            logit_bias: None,
            response_format: None,
            max_output_tokens: max_tokens.or(Some(1024)),
            metadata: None,
        };

        if include_tools {
            let specs = self.build_unified_tool_specs();
            if !specs.is_empty() {
                req.tools = Some(specs);
                req.tool_choice = Some(UnifiedToolChoice::Auto);
            }
        }

        let mut stdout = io::stdout();
        let mut tool_delta_buf = String::new();

        let mut on_event = |ev: StreamEvent| match ev {
            StreamEvent::TextDelta(d) => {
                content.push_str(&d);
                if print_tokens {
                    print!("{}", d);
                    let _ = stdout.flush();
                }
            }
            StreamEvent::ToolDelta(s) => {
                tool_delta_buf.push_str(&s);
                // Attempt to parse partial buffer
                if let Ok(v) = serde_json::from_str::<JsonValue>(&tool_delta_buf) {
                    if v.get("name").is_some() {
                        let name = v
                            .get("name")
                            .and_then(|x| x.as_str())
                            .unwrap_or("tool")
                            .to_string();
                        let id = v.get("id").and_then(|x| x.as_str()).map(|s| s.to_string());
                        let args = v
                            .get("arguments")
                            .or_else(|| v.get("input"))
                            .cloned()
                            .unwrap_or_else(|| json!({}));
                        tool_calls.push(ToolCallNormalized {
                            id,
                            name,
                            arguments_json: args,
                        });
                        tool_delta_buf.clear();
                    }
                }
            }
            StreamEvent::ToolCall(tc) => {
                tool_calls.push(tc);
            }
            StreamEvent::Usage(u) => {
                usage = Some(u);
            }
            StreamEvent::Finished => {
                // no-op
            }
            StreamEvent::Error(msg) => {
                warn!("Streaming error event: {}", msg);
            }
        };

        let res = match self
            .provider_adapter
            .as_ref()
            .unwrap()
            .generate_streaming(req, &mut on_event)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                match e.clone() {
                    ProviderError::TimeoutFirstToken { timeout_ms } => {
                        warn!("Streaming first token timeout after {} ms", timeout_ms);
                    }
                    ProviderError::TimeoutStall { timeout_ms } => {
                        warn!("Streaming stalled for {} ms", timeout_ms);
                    }
                    _ => {}
                }
                return Err(anyhow::anyhow!(e.to_string()));
            }
        };

        if print_tokens {
            println!();
        }

        let final_text = if res.text.is_empty() {
            content
        } else {
            res.text
        };

        info!(
            "Unified streaming finished (chars={}, tool_calls={})",
            final_text.len(),
            tool_calls.len()
        );

        Ok((final_text, tool_calls, usage))
    }

    /// Extract tool calls from LLM response
    #[allow(dead_code)]
    fn extract_tool_calls(&self, response: &str) -> Vec<ToolCall> {
        let mut tool_calls = Vec::new();

        // Look for JSON-formatted tool calls
        let re = Regex::new(r#"\{(?:\s*)"tool"(?:\s*):(?:\s*)"([^"]+)"(?:\s*),(?:\s*)"args"(?:\s*):(?:\s*)\[(.*?)\](?:\s*)\}"#).unwrap();

        for cap in re.captures_iter(response) {
            let tool_name = cap[1].to_string();
            let args_json = format!("[{}]", &cap[2]);

            if let Ok(args) = serde_json::from_str::<Vec<String>>(&args_json) {
                tool_calls.push(ToolCall {
                    tool: tool_name,
                    args,
                });
            }
        }

        // Look for more relaxed format (non-JSON)
        let alt_re = Regex::new(r"(?i)use\s+tool\s+([a-z_]+)(?:\s*):(?:\s*)(.+?)(?:\n|$)").unwrap();
        for cap in alt_re.captures_iter(response) {
            let tool_name = cap[1].to_string();
            let args_text = cap[2].trim();

            // Simple parsing of comma-separated arguments
            let args = args_text
                .split(',')
                .map(|s| s.trim().trim_matches(|c| c == '\'' || c == '"').to_string())
                .collect();

            tool_calls.push(ToolCall {
                tool: tool_name,
                args,
            });
        }

        tool_calls
    }

    /// Generate with tools in a conversational format
    #[allow(clippy::cognitive_complexity)]
    async fn generate_with_tools(&self, context: &CodeContext) -> Result<String> {
        // Create initial prompt with tool instructions
        let mut conversation = String::new();

        // System message with tool instructions
        let mut tool_descriptions = String::new();
        // Use detailed tool specifications instead of simple descriptions
        for (name, description, params) in self.tool_registry.get_tool_specifications() {
            tool_descriptions.push_str(&format!("- {}: {}\n", name, description));

            // Add parameter details if there are any
            if !params.is_empty() {
                tool_descriptions.push_str("  Parameters:\n");
                for param in params {
                    let required_str = if param.required {
                        "required"
                    } else {
                        "optional"
                    };
                    let default_str = if let Some(default) = &param.default_value {
                        format!(" (default: {})", default)
                    } else {
                        String::new()
                    };

                    tool_descriptions.push_str(&format!(
                        "    - {}: {} [{}]{}\n",
                        param.name, param.description, required_str, default_str
                    ));
                }
            }
        }

        let system_message = format!(
            "You are a skilled Rust programmer tasked with implementing code improvements. \
            You have access to the following tools to explore and understand the codebase:\n\n{}\n\n\
            When you need to use a tool, make a tool call using the appropriate function from the tools provided.\n\n\
            IMPORTANT: Use a variety of tools to thoroughly explore and understand the codebase BEFORE making changes:\n\
            1. Use 'explore_dir' to understand the project structure and file organization\n\
            2. Use 'file_contents' to read and analyze specific files\n\
            3. Use 'code_search' to find specific functions, patterns, or implementations\n\
            4. Use 'find_tests' to locate and understand existing test coverage\n\
            5. Use 'git_history' to understand how code has evolved\n\
            6. Use 'compile_check' to validate code snippets before implementing\n\
            7. Use 'git_command' for version control operations when needed\n\
            8. Finally, use 'create_file' or 'modify_file' to implement your improvements\n\n\
            CRITICAL: Do NOT rely on a single tool. Use multiple different tools to gather comprehensive information before making any changes. \
            This ensures you understand the full context and can make informed improvements.",
            tool_descriptions
        );

        // Task description
        let task = if let Some(requirements) = &context.requirements {
            format!(
                "## Task:\n{}\n\n## Requirements:\n{}\n\n",
                context.task, requirements
            )
        } else {
            format!("## Task:\n{}\n\n", context.task)
        };

        // Add file paths
        let files_section = if !context.file_paths.is_empty() {
            format!("## Files to modify:\n{}\n\n", context.file_paths.join("\n"))
        } else {
            String::new()
        };

        // Add previous attempts if any
        let attempts_section = if !context.previous_attempts.is_empty() {
            let mut s = String::from("## Previous Attempts:\n\n");
            for (i, attempt) in context.previous_attempts.iter().enumerate() {
                s.push_str(&format!("### Attempt {}:\n", i + 1));
                s.push_str(&format!("```rust\n{}\n```\n", attempt.code));
                s.push_str(&format!("Failure reason: {}\n\n", attempt.failure_reason));

                if let Some(test_results) = &attempt.test_results {
                    s.push_str(&format!("Test results:\n{}\n\n", test_results));
                }
            }
            s
        } else {
            String::new()
        };

        // Initialize conversation
        conversation.push_str(&system_message);
        conversation.push_str("\n\n");
        conversation.push_str(&task);
        conversation.push_str(&files_section);
        conversation.push_str(&attempts_section);

        let mut final_response = String::new();

        // Iterative conversation with tool usage
        for iteration in 0..self.max_tool_iterations {
            info!(
                "Tool iteration {}/{}",
                iteration + 1,
                self.max_tool_iterations
            );

            // Generate a response (prefer unified streaming to collect ToolCall events)
            let (response, normalized_tool_calls, usage) = if self.provider_adapter.is_some() {
                self.stream_with_unified_provider(&conversation, Some(2048), Some(0.4), false, true)
                    .await?
            } else {
                let text = self
                    .llm
                    .generate_streaming(&conversation, Some(2048), Some(0.4), false)
                    .await?;
                (text, Vec::new(), None)
            };

            if let Some(u) = usage {
                info!(
                    "Streaming usage: prompt_tokens={:?} completion_tokens={:?} total_tokens={:?}",
                    u.prompt_tokens, u.completion_tokens, u.total_tokens
                );
            }

            // Back-compat: also scan response text for legacy inline tool-call JSON when no normalized events present
            let legacy_tool_calls = if normalized_tool_calls.is_empty() {
                self.tool_registry.extract_tool_calls(&response)
            } else {
                Vec::new()
            };

            if normalized_tool_calls.is_empty() && legacy_tool_calls.is_empty() {
                // No tool calls, so this is the final response
                final_response = response;
                break;
            }

            // Add the response to the conversation
            conversation.push_str(&format!("\n\nYou: {}\n\n", response));

            // Prefer normalized tool calls when available
            if !normalized_tool_calls.is_empty() {
                for tc in normalized_tool_calls {
                    info!("Normalized tool call detected: {}", tc.name);
                    let result = self.execute_normalized_tool_call(&tc).await;
                    if result.success {
                        conversation.push_str(&format!(
                            "Tool result for {}:\n{}\n\n",
                            tc.name, result.result
                        ));
                    } else {
                        conversation.push_str(&format!(
                            "Tool error for {}: {}\n\n",
                            tc.name,
                            result.error.unwrap_or_else(|| "Unknown error".to_string())
                        ));
                    }
                }
            } else {
                // Legacy path
                for tool_call in legacy_tool_calls {
                    info!(
                        "Tool call detected: {} with {} args",
                        tool_call.tool,
                        tool_call.args.len()
                    );

                    // Execute the tool
                    let tool_result = self.tool_registry.execute(&tool_call).await;

                    // Add the result to the conversation
                    if tool_result.success {
                        conversation.push_str(&format!(
                            "Tool result for {}:\n{}\n\n",
                            tool_call.tool, tool_result.result
                        ));
                    } else {
                        conversation.push_str(&format!(
                            "Tool error for {}: {}\n\n",
                            tool_call.tool,
                            tool_result
                                .error
                                .unwrap_or_else(|| "Unknown error".to_string())
                        ));
                    }
                }
            }
        }

        if final_response.is_empty() {
            final_response = format!(
                "After {} tool iterations, no final response was generated. \
                 Please provide your code implementation now based on the information gathered.",
                self.max_tool_iterations
            );

            // Generate one more time to get a final response
            final_response = self
                .llm
                .generate(&(conversation + &final_response), Some(4096), Some(0.4))
                .await?;
        }

        Ok(final_response)
    }

    /// Enhance the context with additional information
    async fn enhance_context(&self, context: &mut CodeContext) -> Result<()> {
        // Add file contents if not already present
        if context.file_contents.is_none() && !context.file_paths.is_empty() {
            let mut file_contents = HashMap::new();
            let git_manager = self.git_manager.lock().await;

            for file_path in &context.file_paths {
                if let Ok(content) = git_manager.read_file(file_path).await {
                    file_contents.insert(file_path.clone(), content);
                }
            }

            context.file_contents = Some(file_contents);
        }

        // Find related test files if not already present
        if context.test_files.is_none() && !context.file_paths.is_empty() {
            let mut test_files = Vec::new();
            let find_tests_tool = FindTestsTool::new(self.workspace.clone());
            // Compile regex once outside the loop to avoid clippy::regex_creation_in_loops
            let re = Regex::new(r"- (tests/[^\n]+|src/[^\n]+)").unwrap();

            for file_path in &context.file_paths {
                let result = find_tests_tool.execute(&[file_path]).await;
                if let Ok(result) = result {
                    if !result.contains("No test files found") {
                        // Extract test file names from the result (regex compiled above)
                        for cap in re.captures_iter(&result) {
                            test_files.push(cap[1].to_string());
                        }
                    }
                }
            }

            context.test_files = Some(test_files);
        }

        // Add test contents if not already present
        if context.test_contents.is_none() && context.test_files.is_some() {
            let mut test_contents = HashMap::new();
            let git_manager = self.git_manager.lock().await;

            if let Some(test_files) = &context.test_files {
                for file_path in test_files {
                    if let Ok(content) = git_manager.read_file(file_path).await {
                        test_contents.insert(file_path.clone(), content);
                    }
                }
            }

            context.test_contents = Some(test_contents);
        }

        Ok(())
    }

    /// Process a prompt with tools
    async fn process_with_tools(&self, prompt: &str) -> Result<String> {
        info!(
            "Processing prompt with tools, prompt length: {}",
            prompt.len()
        );

        let mut conversation = vec![("system", prompt.to_string())];

        let mut iterations = 0;

        while iterations < self.max_tool_iterations {
            iterations += 1;

            info!("Tool iteration {}/{}", iterations, self.max_tool_iterations);

            // Generate the next message from the LLM
            let llm_response = self
                .llm
                .generate_streaming(&conversation[0].1, Some(4096), Some(0.5), false)
                .await?;

            // Look for tool calls in the response
            let tool_calls = self.tool_registry.extract_tool_calls(&llm_response);

            if tool_calls.is_empty() {
                info!("No tool calls found, returning response");
                return Ok(llm_response);
            }

            info!("Found {} tool calls", tool_calls.len());

            // Add the LLM response to the conversation
            conversation.push(("assistant", llm_response));

            // Execute each tool call
            let mut all_results = String::new();

            for tool_call in tool_calls {
                info!("Executing tool: {}", tool_call.tool);

                // Execute the tool
                let result = match self.tool_registry.execute_tool(&tool_call).await {
                    Ok(result) => result,
                    Err(e) => {
                        let error_message =
                            format!("Error executing tool {}: {}", tool_call.tool, e);
                        info!("{}", error_message);
                        ToolResult {
                            success: false,
                            result: String::new(),
                            error: Some(error_message),
                        }
                    }
                };

                let result_message = if result.success {
                    format!(
                        "Tool '{}' execution succeeded:\n{}",
                        tool_call.tool, result.result
                    )
                } else {
                    format!(
                        "Tool '{}' execution failed:\n{}",
                        tool_call.tool,
                        result.error.unwrap_or_else(|| "Unknown error".to_string())
                    )
                };

                all_results.push_str(&result_message);
                all_results.push_str("\n\n");
            }

            // Add the tool results to the conversation
            conversation.push(("user", all_results));
        }

        info!(
            "Reached maximum tool iterations ({}), returning final response",
            self.max_tool_iterations
        );

        // Get the last assistant message as the final response
        for (role, message) in conversation.iter().rev() {
            if *role == "assistant" {
                return Ok(message.clone());
            }
        }

        // If no assistant message found, generate a final response
        let final_prompt = format!(
            "{}\n\nPlease provide your final response based on the conversation above.",
            prompt
        );
        let final_response = self
            .llm
            .generate_streaming(&final_prompt, Some(4096), Some(0.5), false)
            .await?;

        Ok(final_response)
    }

    /// Generate a response using the git operations prompt
    pub async fn generate_git_operations_response(&self, query: &str) -> Result<String> {
        let prompt = self.prompt_manager.create_git_operations_prompt();

        info!("Generating git operations response for query: {}", query);

        // Combine the prompt with the user's query
        let full_prompt = format!("{}\n\n## QUERY:\n{}", prompt, query);

        if self.use_tools {
            // Process with tools
            let response = self.process_with_tools(&full_prompt).await?;
            Ok(response)
        } else {
            // Direct LLM call without tools
            let response = self
                .llm
                .generate_streaming(&full_prompt, Some(4096), Some(0.5), false)
                .await?;
            Ok(response)
        }
    }

    /// Generate a commit message based on code changes
    pub async fn create_commit_message(
        &self,
        improvement: &CodeImprovement,
        goal_id: &str,
        branch_name: &str,
    ) -> Result<String> {
        let prompt = self.prompt_manager.create_system_message();

        // Build a list of changed files
        let mut changed_files_desc = String::new();
        for file in &improvement.target_files {
            changed_files_desc.push_str(&format!("- {}\n", file.file_path));
        }

        // Create a description of the changes
        let query = format!(
            "I need a Git commit message for the following changes:\n\n\
            Goal ID: {}\n\
            Branch: {}\n\
            Task: {}\n\n\
            Changes made to these files:\n{}\n\n\
            Explanation of changes:\n{}\n\n\
            Please write a clear, concise, and informative commit message that follows Git best practices. \
            The message should have a brief summary (50-72 chars) as the first line, followed by a blank line and \
            a more detailed explanation if needed. Focus on WHY the change was made, not just WHAT was changed. \
            Do not include the word 'commit' in the message.",
            goal_id,
            branch_name,
            improvement.task,
            changed_files_desc,
            improvement.explanation
        );

        // Combine the prompt with the user's query
        let full_prompt = format!("{}\n\n{}", prompt, query);

        // Direct LLM call for commit message
        let response = self
            .llm
            .generate_streaming(&full_prompt, Some(1024), Some(0.4), false)
            .await?;

        // Extract just the commit message (removing any explanations the LLM might add)
        let commit_message = if response.contains("```") {
            // If the response includes code blocks, extract the content
            let re = Regex::new(r"```(?:commit|git)?\s*\n([\s\S]*?)\n```").unwrap();
            if let Some(cap) = re.captures(&response) {
                cap[1].trim().to_string()
            } else {
                response.trim().to_string()
            }
        } else {
            response.trim().to_string()
        };

        Ok(commit_message)
    }

    /// Handle git merge operations
    pub async fn process_merge_operation(
        &self,
        branch_name: &str,
        target_branch: &str,
        summary: &str,
    ) -> Result<String> {
        let _prompt = self.prompt_manager.create_system_message();

        // Create a description for the merge operation
        let query = format!(
            "I need assistance with a Git merge operation:\n\n\
            Merging branch '{}' into branch '{}'\n\n\
            Summary of changes being merged:\n{}\n\n\
            Please provide guidance on performing this merge. Consider the following:\n\
            1. Is this merge safe to proceed with?\n\
            2. What conflicts might arise and how should they be handled?\n\
            3. What should the merge commit message be?\n\
            4. Are there any post-merge steps that should be taken?\n\
            5. Should the source branch be deleted after merging?",
            branch_name, target_branch, summary
        );

        // Combine the prompt with the query (not used; removed to satisfy clippy)

        // Use the git operations prompt approach
        let response = self.generate_git_operations_response(&query).await?;

        Ok(response)
    }
}

#[async_trait]
impl CodeGenerator for LlmCodeGenerator {
    async fn generate_improvement(&self, context: &CodeContext) -> Result<CodeImprovement> {
        // Enhancement: Use stored flag instead of hard-coded value
        let use_tools = self.use_tools && context.current_attempt.unwrap_or(1) > 0;

        // Create a mutable copy of the context that we can enhance
        let mut enhanced_context = context.clone();

        // Enhance the context with additional information
        self.enhance_context(&mut enhanced_context).await?;

        let response = if use_tools {
            info!("Using interactive tool-based approach for code generation");
            self.generate_with_tools(&enhanced_context).await?
        } else {
            // Standard approach for first attempt
            info!("Using standard approach for code generation");

            // Fetch content of all relevant files
            let current_code = self
                .fetch_code_content(context.file_paths.first().unwrap())
                .await?;

            // Determine the appropriate prompt type based on the task description
            let prompt = if context.task.to_lowercase().contains("bug")
                || context.task.to_lowercase().contains("fix")
            {
                info!("Using bugfix prompt for task: {}", context.task);
                self.prompt_manager
                    .create_bugfix_prompt(context, &current_code)
            } else if context.task.to_lowercase().contains("feature")
                || context.task.to_lowercase().contains("implement")
                || context.task.to_lowercase().contains("add")
            {
                info!("Using feature prompt for task: {}", context.task);
                self.prompt_manager
                    .create_feature_prompt(context, &current_code)
            } else if context.task.to_lowercase().contains("refactor")
                || context.task.to_lowercase().contains("restructure")
                || context.task.to_lowercase().contains("simplify")
            {
                info!("Using refactor prompt for task: {}", context.task);
                self.prompt_manager
                    .create_refactor_prompt(context, &current_code)
            } else {
                info!(
                    "Using general improvement prompt for task: {}",
                    context.task
                );
                self.prompt_manager
                    .create_improvement_prompt(context, &current_code)
            };

            info!("Generated prompt with length: {} characters", prompt.len());

            // Ask the LLM with appropriate parameters based on the task
            let max_tokens = Some(4096); // Increased token limit for more detailed responses
            let temperature = if context.task.to_lowercase().contains("bug")
                || context.task.to_lowercase().contains("fix")
            {
                // Lower temperature for bug fixes to get more deterministic outputs
                Some(0.2)
            } else if context.task.to_lowercase().contains("feature")
                || context.task.to_lowercase().contains("innovative")
            {
                // Higher temperature for features to encourage creativity
                Some(0.7)
            } else {
                // Balanced temperature for most improvements
                Some(0.4)
            };

            self.llm
                .generate_streaming(&prompt, max_tokens, temperature, false)
                .await?
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

    async fn provide_feedback(
        &self,
        improvement: &CodeImprovement,
        success: bool,
        feedback: &str,
    ) -> Result<()> {
        // This is a simple implementation
        // In a real system, we might store this feedback for future reference
        // or use it to fine-tune the LLM

        info!(
            "Feedback for improvement {}: Success={}, Feedback={}",
            improvement.id, success, feedback
        );

        // We could also log this to a database or send it to a feedback API

        Ok(())
    }

    /// Generate a response for git operations
    async fn generate_git_response(&self, query: &str) -> Result<String> {
        self.generate_git_operations_response(query).await
    }

    /// Generate a git commit message based on code changes
    async fn generate_commit_message(
        &self,
        improvement: &CodeImprovement,
        goal_id: &str,
        branch_name: &str,
    ) -> Result<String> {
        self.create_commit_message(improvement, goal_id, branch_name)
            .await
    }

    /// Handle git merge operations
    async fn handle_merge_operation(
        &self,
        branch_name: &str,
        target_branch: &str,
        summary: &str,
    ) -> Result<String> {
        self.process_merge_operation(branch_name, target_branch, summary)
            .await
    }
}
