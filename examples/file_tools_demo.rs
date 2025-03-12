use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::{Result, Context};

use crate::code_generation::llm_tool::{
    CreateFileTool, ModifyFileTool, LlmTool, ToolCall, ToolResult
};
use crate::version_control::git::GitManager;
use crate::version_control::git_implementation::GitImplManager;

/// This example demonstrates how the LLM can create and modify files
/// without explicitly being told which files to change
#[tokio::main]
async fn main() -> Result<()> {
    // Setup workspace
    let workspace = std::env::current_dir()?;
    println!("Working directory: {:?}", workspace);

    // Create a temp directory for this demo
    let temp_dir = workspace.join("temp_demo");
    std::fs::create_dir_all(&temp_dir)
        .context("Failed to create temp directory")?;

    // Create git manager
    let git_manager = Arc::new(Mutex::new(
        GitImplManager::new(workspace.clone())
            .context("Failed to create git manager")?
    ));

    // Create tool instances
    let create_file_tool = CreateFileTool::new(temp_dir.clone());
    let modify_file_tool = ModifyFileTool::new(temp_dir.clone());

    println!("=== LLM File Tools Demo ===\n");

    // Demonstrate creating a new file
    println!("Creating a new file...");
    let create_result = create_file_tool.execute(&[
        "example.rs",
        "fn main() {\n    println!(\"Hello from example!\");\n}"
    ]).await?;
    println!("Result: {}\n", create_result);

    // Read the file to confirm
    println!("Contents of the file:");
    let file_path = temp_dir.join("example.rs");
    let content = std::fs::read_to_string(&file_path)
        .context("Failed to read created file")?;
    println!("{}\n", content);

    // Demonstrate modifying the entire file
    println!("Modifying the entire file...");
    let modify_result = modify_file_tool.execute(&[
        "example.rs",
        "fn main() {\n    println!(\"Hello from modified example!\");\n    println!(\"LLM changed this file!\");\n}"
    ]).await?;
    println!("Result: {}\n", modify_result);

    // Read the modified file
    println!("Contents after full modification:");
    let content = std::fs::read_to_string(&file_path)
        .context("Failed to read modified file")?;
    println!("{}\n", content);

    // Demonstrate modifying specific lines
    println!("Modifying specific lines...");
    let partial_modify_result = modify_file_tool.execute(&[
        "example.rs",
        "    // This line was inserted at line 2",
        "2",
        "2"  // Replace just line 2
    ]).await?;
    println!("Result: {}\n", partial_modify_result);

    // Read the file after line modification
    println!("Contents after line modification:");
    let content = std::fs::read_to_string(&file_path)
        .context("Failed to read line-modified file")?;
    println!("{}\n", content);

    // Demonstrate multiple tool calls in one response
    println!("\n=== Multiple Tool Calls Demonstration ===\n");
    println!("The LLM can now make multiple tool calls in a single response:");
    println!("Example LLM response format:");
    println!(r#"
I need to understand the codebase better:

{"tool": "file_contents", "args": ["src/main.rs"]}
{"tool": "code_search", "args": ["important_function"]}

After examining these files, I'll create a new module and modify the existing one:

{"tool": "create_file", "args": ["src/utils.rs", "// New utility module\npub fn helper() -> String {\n    \"Helper function\".to_string()\n}\n"]}
{"tool": "modify_file", "args": ["src/main.rs", "// Modified main file\nuse crate::utils::helper;\n\nfn main() {\n    println!(\"{}\", helper());\n}\n"]}
"#);

    println!("\nThis allows the LLM to perform multiple operations in sequence without waiting for each response,");
    println!("making the code generation process more efficient.");

    // Clean up
    std::fs::remove_dir_all(temp_dir)
        .context("Failed to remove temp directory")?;

    println!("Demo complete and temp files cleaned up!");

    Ok(())
}