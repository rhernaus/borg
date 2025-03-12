use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::{Result, Context};
use regex::Regex;

use crate::code_generation::llm_tool::{
    LlmTool, ToolCall, ToolResult, ToolRegistry,
    CreateFileTool, ModifyFileTool, FileContentsTool,
    DirectoryExplorationTool
};
use crate::version_control::git::GitManager;
use crate::version_control::git_implementation::GitImplManager;

/// This example demonstrates processing multiple tool calls from a single LLM response
#[tokio::main]
async fn main() -> Result<()> {
    // Setup workspace
    let workspace = std::env::current_dir()?;
    println!("Working directory: {:?}", workspace);

    // Create a temp directory for this test
    let temp_dir = workspace.join("temp_multi_tools");
    std::fs::create_dir_all(&temp_dir)
        .context("Failed to create temp directory")?;

    // Initialize tool registry
    let mut tool_registry = ToolRegistry::new();

    // Create git manager
    let git_manager = Arc::new(Mutex::new(
        GitImplManager::new(workspace.clone())
            .context("Failed to create git manager")?
    ));

    // Register tools
    tool_registry.register(CreateFileTool::new(temp_dir.clone()));
    tool_registry.register(ModifyFileTool::new(temp_dir.clone()));
    tool_registry.register(FileContentsTool::new(temp_dir.clone()));
    tool_registry.register(DirectoryExplorationTool::new(temp_dir.clone()));

    println!("=== Multiple Tool Calls Test ===\n");

    // Simulate an LLM response with multiple tool calls
    let simulated_llm_response = r#"
I'm going to build a small Rust project with multiple files:

{"tool": "create_file", "args": ["main.rs", "fn main() {\n    println!(\"Hello from multi-tool test!\");\n    let message = greeting::get_greeting();\n    println!(\"{}\", message);\n}"]}
{"tool": "create_file", "args": ["greeting.rs", "pub mod greeting {\n    pub fn get_greeting() -> String {\n        \"Hello from the greeting module!\".to_string()\n    }\n}"]}

Now let's verify these files were created:

{"tool": "explore_dir", "args": [".", "false", "1"]}
{"tool": "file_contents", "args": ["main.rs"]}
{"tool": "file_contents", "args": ["greeting.rs"]}

Now let's modify the greeting:

{"tool": "modify_file", "args": ["greeting.rs", "pub mod greeting {\n    pub fn get_greeting() -> String {\n        \"Greetings from the improved module!\".to_string()\n    }\n\n    pub fn get_farewell() -> String {\n        \"Goodbye from the greeting module!\".to_string()\n    }\n}"]}
{"tool": "file_contents", "args": ["greeting.rs"]}
"#;

    // Extract and execute tool calls
    let re = Regex::new(r#"\{(?:\s*)"tool"(?:\s*):(?:\s*)"([^"]+)"(?:\s*),(?:\s*)"args"(?:\s*):(?:\s*)\[(.*?)\](?:\s*)\}"#).unwrap();

    // Keep track of all results for display
    let mut all_results = Vec::new();

    // Find and process each tool call
    println!("Processing multiple tool calls from a single response...\n");

    for (idx, cap) in re.captures_iter(simulated_llm_response).enumerate() {
        let tool_name = cap[1].to_string();
        let args_json = format!("[{}]", &cap[2]);

        println!("Tool call #{}: {}", idx + 1, tool_name);

        if let Ok(args) = serde_json::from_str::<Vec<String>>(&args_json) {
            // Convert Vec<String> to Vec<&str>
            let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

            // Create a tool call
            let tool_call = ToolCall {
                tool: tool_name.clone(),
                args: args.clone(),
            };

            // Execute the tool
            let result = tool_registry.execute(&tool_call).await;

            println!("Result: {}\n", if result.success {
                result.result.lines().take(3).collect::<Vec<_>>().join("\n") +
                    if result.result.lines().count() > 3 { "\n... (output truncated)" } else { "" }
            } else {
                format!("Error: {}", result.error.unwrap_or_else(|| "Unknown error".to_string()))
            });

            all_results.push((tool_name, result));
        }
    }

    println!("\n=== Summary of All Tool Calls ===");
    for (idx, (tool_name, result)) in all_results.iter().enumerate() {
        println!("{}. {} - {}",
            idx + 1,
            tool_name,
            if result.success { "Success" } else { "Failed" }
        );
    }

    // Clean up
    std::fs::remove_dir_all(temp_dir)
        .context("Failed to remove temp directory")?;

    println!("\nAll test files cleaned up. Test complete!");

    Ok(())
}