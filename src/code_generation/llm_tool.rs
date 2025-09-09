use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono;
use log::{info, warn};
use rand;
use regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::version_control::git::GitManager;

/// New tool parameter type for structured parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolParameterType {
    String,
    Integer,
    Boolean,
    Code,
}

/// Parameter specification for a tool
#[derive(Debug, Clone)]
pub struct ToolParameter {
    /// Name of the parameter
    pub name: String,

    /// Description of the parameter
    pub description: String,

    /// Whether the parameter is required
    pub required: bool,

    /// Default value for optional parameters
    pub default_value: Option<String>,

    /// Type of the parameter (defaults to String if not specified)
    pub param_type: Option<ToolParameterType>,
}

/// A tool that can be used by the LLM during code generation
#[async_trait]
pub trait LlmTool: Send + Sync {
    /// Get the name of the tool
    fn name(&self) -> &str;

    /// Get a description of what the tool does
    fn description(&self) -> &str;

    /// Get the parameter specifications for this tool
    fn parameters(&self) -> Vec<ToolParameter> {
        Vec::new() // Default implementation for backward compatibility
    }

    /// Execute the tool with the given arguments
    async fn execute(&self, args: &[&str]) -> Result<String>;
}

/// Tool call request from the LLM
#[derive(Debug, Serialize, Deserialize)]
pub struct ToolCall {
    /// The name of the tool to call
    pub tool: String,

    /// Arguments to pass to the tool
    pub args: Vec<String>,
}

/// Tool call response to the LLM
#[derive(Debug, Serialize, Deserialize)]
pub struct ToolResult {
    /// Whether the tool call was successful
    pub success: bool,

    /// Result of the tool call
    pub result: String,

    /// Error message if any
    pub error: Option<String>,
}

/// A tool that searches code
pub struct CodeSearchTool {
    workspace: PathBuf,
    #[allow(dead_code)]
    git_manager: Arc<Mutex<dyn GitManager>>,
}

impl CodeSearchTool {
    /// Create a new code search tool
    pub fn new(workspace: PathBuf, git_manager: Arc<Mutex<dyn GitManager>>) -> Self {
        Self {
            workspace,
            git_manager,
        }
    }
}

#[async_trait]
impl LlmTool for CodeSearchTool {
    fn name(&self) -> &str {
        "code_search"
    }

    fn description(&self) -> &str {
        "Search for code patterns or symbols in the codebase."
    }

    fn parameters(&self) -> Vec<ToolParameter> {
        vec![
            ToolParameter {
                name: "pattern".to_string(),
                description: "The pattern to search for".to_string(),
                required: true,
                default_value: None,
                param_type: Some(ToolParameterType::String),
            },
            ToolParameter {
                name: "file_pattern".to_string(),
                description: "Optional glob pattern to limit search to specific files".to_string(),
                required: false,
                default_value: None,
                param_type: Some(ToolParameterType::String),
            },
        ]
    }

    async fn execute(&self, args: &[&str]) -> Result<String> {
        if args.is_empty() {
            return Err(anyhow::anyhow!("No search pattern provided"));
        }

        let pattern = args[0];
        let file_pattern = if args.len() > 1 { Some(args[1]) } else { None };

        // Use ripgrep or grep for search
        let mut cmd = Command::new("rg");
        cmd.current_dir(&self.workspace)
            .arg("--line-number")
            .arg("--with-filename");

        if let Some(file_pattern) = file_pattern {
            cmd.arg("--glob").arg(file_pattern);
        }

        cmd.arg(pattern);

        match cmd.output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let _stderr = String::from_utf8_lossy(&output.stderr).to_string();

                if stdout.is_empty() {
                    Ok(format!(
                        "No matches found for pattern '{}'{}",
                        pattern,
                        if let Some(fp) = file_pattern {
                            format!(" in files matching '{}'", fp)
                        } else {
                            String::new()
                        }
                    ))
                } else {
                    Ok(format!(
                        "Search results for '{}'{}:\n{}",
                        pattern,
                        if let Some(fp) = file_pattern {
                            format!(" in files matching '{}'", fp)
                        } else {
                            String::new()
                        },
                        stdout
                    ))
                }
            }
            Err(_e) => {
                // Fall back to git grep if ripgrep isn't available
                let mut git_cmd = Command::new("git");
                git_cmd
                    .current_dir(&self.workspace)
                    .arg("grep")
                    .arg("-n") // line numbers
                    .arg(pattern);

                match git_cmd.output() {
                    Ok(git_output) => {
                        let git_stdout = String::from_utf8_lossy(&git_output.stdout).to_string();
                        if git_stdout.is_empty() {
                            Ok(format!("No matches found for pattern '{}'", pattern))
                        } else {
                            Ok(format!("Search results for '{}':\n{}", pattern, git_stdout))
                        }
                    }
                    Err(_) => Err(anyhow::anyhow!(
                        "Failed to search code: ripgrep and git grep both unavailable"
                    )),
                }
            }
        }
    }
}

/// A tool that reads file contents
pub struct FileContentsTool {
    workspace: PathBuf,
}

impl FileContentsTool {
    /// Create a new file contents tool
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl LlmTool for FileContentsTool {
    fn name(&self) -> &str {
        "file_contents"
    }

    fn description(&self) -> &str {
        "Read the contents of a file. Usage: file_contents <file_path> [start_line] [end_line]"
    }

    fn parameters(&self) -> Vec<ToolParameter> {
        vec![
            ToolParameter {
                name: "file_path".to_string(),
                description: "Path to the file to read".to_string(),
                required: true,
                default_value: None,
                param_type: Some(ToolParameterType::String),
            },
            ToolParameter {
                name: "start_line".to_string(),
                description: "Optional start line number (1-indexed)".to_string(),
                required: false,
                default_value: Some("1".to_string()),
                param_type: Some(ToolParameterType::Integer),
            },
            ToolParameter {
                name: "end_line".to_string(),
                description: "Optional end line number (1-indexed, inclusive)".to_string(),
                required: false,
                default_value: None,
                param_type: Some(ToolParameterType::Integer),
            },
        ]
    }

    async fn execute(&self, args: &[&str]) -> Result<String> {
        if args.is_empty() {
            return Err(anyhow::anyhow!("No file path provided"));
        }

        let file_path = Path::new(args[0]);
        let full_path = self.workspace.join(file_path);

        if !full_path.exists() {
            return Err(anyhow::anyhow!("File not found: {}", file_path.display()));
        }

        // Parse optional line range
        let start_line = if args.len() > 1 {
            args[1].parse::<usize>().unwrap_or(1)
        } else {
            1
        };

        let end_line = if args.len() > 2 {
            args[2].parse::<usize>().ok()
        } else {
            None
        };

        // Read the file
        let content = std::fs::read_to_string(&full_path)
            .context(format!("Failed to read file: {}", file_path.display()))?;

        // Apply line range if needed
        if start_line > 1 || end_line.is_some() {
            let lines: Vec<&str> = content.lines().collect();
            let start_idx = start_line.saturating_sub(1);
            let end_idx = match end_line {
                Some(end) => std::cmp::min(end, lines.len()),
                None => lines.len(),
            };

            if start_idx >= lines.len() {
                return Err(anyhow::anyhow!(
                    "Start line {} is beyond the end of the file",
                    start_line
                ));
            }

            let slice = &lines[start_idx..end_idx];
            Ok(format!(
                "Contents of {} (lines {} to {}):\n\n{}",
                file_path.display(),
                start_line,
                end_idx,
                slice.join("\n")
            ))
        } else {
            Ok(format!(
                "Contents of {}:\n\n{}",
                file_path.display(),
                content
            ))
        }
    }
}

/// A tool that finds test files related to a source file
pub struct FindTestsTool {
    workspace: PathBuf,
}

impl FindTestsTool {
    /// Create a new find tests tool
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl LlmTool for FindTestsTool {
    fn name(&self) -> &str {
        "find_tests"
    }

    fn description(&self) -> &str {
        "Find tests related to a specific file or functionality. Usage: find_tests <file_path|module_name>"
    }

    fn parameters(&self) -> Vec<ToolParameter> {
        vec![ToolParameter {
            name: "query".to_string(),
            description: "File path or module name to find tests for".to_string(),
            required: true,
            default_value: None,
            param_type: Some(ToolParameterType::String),
        }]
    }

    async fn execute(&self, args: &[&str]) -> Result<String> {
        if args.is_empty() {
            return Err(anyhow::anyhow!("No file path provided"));
        }

        let file_path = Path::new(args[0]);
        let full_path = self.workspace.join(file_path);

        if !full_path.exists() {
            return Err(anyhow::anyhow!("File not found: {}", file_path.display()));
        }

        // Get the file stem (name without extension)
        let _file_name = match file_path.file_name() {
            Some(name) => name.to_string_lossy().to_string(),
            None => return Err(anyhow::anyhow!("Invalid file path")),
        };

        let file_stem = match file_path.file_stem() {
            Some(stem) => stem.to_string_lossy().to_string(),
            None => return Err(anyhow::anyhow!("Could not determine file name")),
        };

        // Look for test files in common test locations
        let mut test_files = Vec::new();

        // Check tests directory
        let tests_dir = self.workspace.join("tests");
        if tests_dir.exists() {
            // Look for test files with the same name or containing the name
            if let Ok(entries) = std::fs::read_dir(tests_dir) {
                for entry in entries.flatten() {
                    let entry_path = entry.path();
                    if let Some(entry_name) = entry_path.file_name() {
                        let entry_name = entry_name.to_string_lossy().to_lowercase();
                        if entry_name.contains(&file_stem.to_lowercase())
                            || entry_name.contains(&format!("test_{}", file_stem.to_lowercase()))
                        {
                            test_files.push(format!(
                                "tests/{}",
                                entry_path.file_name().unwrap().to_string_lossy()
                            ));
                        }
                    }
                }
            }
        }

        // Check for tests in the same directory
        if let Some(parent) = file_path.parent() {
            let parent_path = self.workspace.join(parent);
            if let Ok(entries) = std::fs::read_dir(parent_path) {
                for entry in entries.flatten() {
                    let entry_path = entry.path();
                    if let Some(entry_name) = entry_path.file_name() {
                        let entry_name = entry_name.to_string_lossy().to_lowercase();
                        if (entry_name.contains("test") || entry_name.contains("spec"))
                            && entry_name.contains(&file_stem.to_lowercase())
                        {
                            let entry_rel_path = if parent.to_string_lossy().is_empty() {
                                entry_name.to_string()
                            } else {
                                format!("{}/{}", parent.display(), entry_name)
                            };
                            test_files.push(entry_rel_path);
                        }
                    }
                }
            }
        }

        // Look for test modules in the same file
        let file_content = std::fs::read_to_string(&full_path)
            .context(format!("Failed to read file: {}", file_path.display()))?;

        let has_test_module = file_content.contains("#[cfg(test)]")
            || file_content.contains("mod test")
            || file_content.contains("mod tests");

        if has_test_module {
            test_files.push(format!("{} (contains test module)", file_path.display()));
        }

        if test_files.is_empty() {
            Ok(format!("No test files found for {}", file_path.display()))
        } else {
            Ok(format!(
                "Test files related to {}:\n- {}",
                file_path.display(),
                test_files.join("\n- ")
            ))
        }
    }
}

/// A tool that explores directory contents
pub struct DirectoryExplorationTool {
    workspace: PathBuf,
}

impl DirectoryExplorationTool {
    /// Create a new directory exploration tool
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl LlmTool for DirectoryExplorationTool {
    fn name(&self) -> &str {
        "explore_dir"
    }

    fn description(&self) -> &str {
        "List the contents of a directory. Usage: explore_dir <directory_path> [show_hidden=false] [max_depth=1]"
    }

    fn parameters(&self) -> Vec<ToolParameter> {
        vec![
            ToolParameter {
                name: "directory_path".to_string(),
                description: "Path to the directory to explore".to_string(),
                required: true,
                default_value: None,
                param_type: Some(ToolParameterType::String),
            },
            ToolParameter {
                name: "show_hidden".to_string(),
                description: "Whether to show hidden files and directories".to_string(),
                required: false,
                default_value: Some("false".to_string()),
                param_type: Some(ToolParameterType::Boolean),
            },
            ToolParameter {
                name: "max_depth".to_string(),
                description: "Maximum depth to explore (1 means just the specified directory)"
                    .to_string(),
                required: false,
                default_value: Some("1".to_string()),
                param_type: Some(ToolParameterType::Integer),
            },
        ]
    }

    async fn execute(&self, args: &[&str]) -> Result<String> {
        if args.is_empty() {
            return Err(anyhow::anyhow!("No directory path provided"));
        }

        let dir_path = Path::new(args[0]);
        let full_path = self.workspace.join(dir_path);

        if !full_path.exists() {
            return Err(anyhow::anyhow!(
                "Directory not found: {}",
                dir_path.display()
            ));
        }

        if !full_path.is_dir() {
            return Err(anyhow::anyhow!("Not a directory: {}", dir_path.display()));
        }

        // Parse optional arguments
        let show_hidden = if args.len() > 1 {
            args[1].parse::<bool>().unwrap_or(false)
        } else {
            false
        };

        let max_depth = if args.len() > 2 {
            args[2].parse::<usize>().unwrap_or(1)
        } else {
            1
        };

        let mut result = format!("Contents of directory {}:\n", dir_path.display());

        // Helper function to recursively list directory contents
        fn list_dir_contents(
            path: &Path,
            base_path: &Path,
            prefix: &str,
            show_hidden: bool,
            current_depth: usize,
            max_depth: usize,
            result: &mut String,
        ) -> Result<()> {
            if current_depth > max_depth {
                return Ok(());
            }

            let mut entries: Vec<_> = std::fs::read_dir(path)
                .context(format!("Failed to read directory: {}", path.display()))?
                .filter_map(Result::ok)
                .collect();

            // Sort entries by name, directories first
            entries.sort_by(|a, b| {
                let a_is_dir = a.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                let b_is_dir = b.file_type().map(|ft| ft.is_dir()).unwrap_or(false);

                match (a_is_dir, b_is_dir) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a.file_name().cmp(&b.file_name()),
                }
            });

            for entry in entries {
                let entry_path = entry.path();
                let file_name = entry.file_name().to_string_lossy().to_string();

                // Skip hidden files/directories if not requested
                if !show_hidden && file_name.starts_with('.') {
                    continue;
                }

                let _rel_path = pathdiff::diff_paths(&entry_path, base_path)
                    .unwrap_or_else(|| entry.path().file_name().unwrap().into());
                let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);

                if is_dir {
                    result.push_str(&format!("{}📁 {}/\n", prefix, file_name));

                    // Recursively list subdirectories
                    if current_depth < max_depth {
                        list_dir_contents(
                            &entry_path,
                            base_path,
                            &format!("{}  ", prefix),
                            show_hidden,
                            current_depth + 1,
                            max_depth,
                            result,
                        )?;
                    } else if max_depth > 0 {
                        // Indicate there's more but we're not showing it
                        result.push_str(&format!("{}  ...\n", prefix));
                    }
                } else {
                    // For files, add an icon based on file extension
                    let icon = match entry_path.extension().and_then(|e| e.to_str()) {
                        Some("rs") => "🦀",                             // Rust
                        Some("md" | "txt") => "📄",                     // Documentation
                        Some("toml" | "json" | "yaml" | "yml") => "⚙️", // Config
                        Some("gitignore" | "git") => "🔄",              // Git
                        Some("sh" | "bash") => "⚡",                    // Scripts
                        Some("png" | "jpg" | "jpeg" | "gif") => "🖼️",   // Images
                        _ => "📎",                                      // Other files
                    };

                    result.push_str(&format!("{}{} {}\n", prefix, icon, file_name));
                }
            }

            Ok(())
        }

        // Start the recursive directory listing
        list_dir_contents(
            &full_path,
            &self.workspace,
            "",
            show_hidden,
            1,
            max_depth,
            &mut result,
        )?;

        if max_depth > 1 {
            result.push_str("\nNote: 📁 indicates directories\n");
        }

        Ok(result)
    }
}

/// A tool that shows git history for files
pub struct GitHistoryTool {
    workspace: PathBuf,
    #[allow(dead_code)]
    git_manager: Arc<Mutex<dyn GitManager>>,
}

impl GitHistoryTool {
    /// Create a new git history tool
    pub fn new(workspace: PathBuf, git_manager: Arc<Mutex<dyn GitManager>>) -> Self {
        Self {
            workspace,
            git_manager,
        }
    }
}

#[async_trait]
impl LlmTool for GitHistoryTool {
    fn name(&self) -> &str {
        "git_history"
    }

    fn description(&self) -> &str {
        "Show git commit history for a file or directory. Usage: git_history <file_path> [limit=5]"
    }

    fn parameters(&self) -> Vec<ToolParameter> {
        vec![
            ToolParameter {
                name: "file_path".to_string(),
                description: "Path to the file or directory to show history for".to_string(),
                required: true,
                default_value: None,
                param_type: Some(ToolParameterType::String),
            },
            ToolParameter {
                name: "limit".to_string(),
                description: "Maximum number of commits to show".to_string(),
                required: false,
                default_value: Some("5".to_string()),
                param_type: Some(ToolParameterType::Integer),
            },
        ]
    }

    async fn execute(&self, args: &[&str]) -> Result<String> {
        if args.is_empty() {
            return Err(anyhow::anyhow!("No file path provided"));
        }

        let file_path = args[0];
        let full_path = self.workspace.join(file_path);

        if !full_path.exists() {
            return Err(anyhow::anyhow!(
                "File or directory not found: {}",
                file_path
            ));
        }

        // Get optional commit limit
        let limit = if args.len() > 1 {
            args[1].parse::<usize>().unwrap_or(5)
        } else {
            5
        };

        // Get relative path from workspace
        let rel_path = pathdiff::diff_paths(&full_path, &self.workspace)
            .unwrap_or_else(|| Path::new(file_path).to_path_buf());
        let rel_path_str = rel_path.to_string_lossy();

        // Use git log command to get history
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.workspace)
            .arg("log")
            .arg("--max-count")
            .arg(limit.to_string())
            .arg("--pretty=format:%h|%an|%ad|%s")
            .arg("--date=short")
            .arg("--");

        // Add the file path if specified (otherwise show repo history)
        if !rel_path_str.is_empty() && rel_path_str != "." {
            cmd.arg(rel_path_str.as_ref());
        }

        match cmd.output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();

                if stdout.is_empty() {
                    return Ok(format!("No git history found for {}", file_path));
                }

                let mut result = format!(
                    "Git history for {} (last {} commits):\n\n",
                    file_path, limit
                );
                result.push_str("| Commit | Author | Date | Message |\n");
                result.push_str("|--------|--------|------|--------|\n");

                // Parse the formatted output
                for line in stdout.lines() {
                    let parts: Vec<&str> = line.split('|').collect();
                    if parts.len() >= 4 {
                        let commit = parts[0];
                        let author = parts[1];
                        let date = parts[2];
                        let message = parts[3];

                        result.push_str(&format!(
                            "| {} | {} | {} | {} |\n",
                            commit, author, date, message
                        ));
                    }
                }

                // If we're querying a specific file, also show detailed changes for last commit
                if !rel_path_str.is_empty() && rel_path_str != "." && full_path.is_file() {
                    result.push_str("\n## Most recent changes\n\n");

                    let mut diff_cmd = Command::new("git");
                    diff_cmd
                        .current_dir(&self.workspace)
                        .arg("show")
                        .arg("--pretty=format:")
                        .arg("--patch")
                        .arg("--unified=3")
                        .arg("HEAD")
                        .arg("--")
                        .arg(rel_path_str.as_ref());

                    if let Ok(diff_output) = diff_cmd.output() {
                        let diff = String::from_utf8_lossy(&diff_output.stdout).to_string();

                        if !diff.is_empty() {
                            // Extract the most relevant part of the diff (limit length)
                            let condensed_diff = if diff.lines().count() > 25 {
                                let first_lines: Vec<&str> = diff.lines().take(20).collect();
                                format!(
                                    "{}\n... (diff truncated, showing first 20 lines) ...",
                                    first_lines.join("\n")
                                )
                            } else {
                                diff
                            };

                            result.push_str("```diff\n");
                            result.push_str(&condensed_diff);
                            result.push_str("\n```\n");
                        } else {
                            result.push_str("No changes found in the most recent commit.\n");
                        }
                    } else {
                        result.push_str(
                            "Unable to retrieve detailed changes for the most recent commit.\n",
                        );
                    }
                }

                Ok(result)
            }
            Err(e) => Err(anyhow::anyhow!("Failed to get git history: {}", e)),
        }
    }
}

/// A tool that quickly checks if code will compile
pub struct CompilationFeedbackTool {
    workspace: PathBuf,
}

impl CompilationFeedbackTool {
    /// Create a new compilation feedback tool
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    /// Create a temporary file with the code
    fn create_temp_file(&self, code: &str, extension: &str) -> Result<PathBuf> {
        // Create temp directory if it doesn't exist
        let temp_dir = self.workspace.join("temp");
        std::fs::create_dir_all(&temp_dir).context("Failed to create temp directory")?;

        // Generate a unique filename
        let timestamp = chrono::Utc::now().timestamp();
        let random = rand::random::<u16>();
        let filename = format!("temp_{}_{}.{}", timestamp, random, extension);
        let temp_file = temp_dir.join(filename);

        // Write code to file
        std::fs::write(&temp_file, code).context("Failed to write temporary file")?;

        Ok(temp_file)
    }
}

#[async_trait]
impl LlmTool for CompilationFeedbackTool {
    fn name(&self) -> &str {
        "compile_check"
    }

    fn description(&self) -> &str {
        "Check if code will compile without actually integrating it. Usage: compile_check <code> [file_type=rs]"
    }

    fn parameters(&self) -> Vec<ToolParameter> {
        vec![
            ToolParameter {
                name: "code".to_string(),
                description: "The code to check for compilation".to_string(),
                required: true,
                default_value: None,
                param_type: Some(ToolParameterType::Code),
            },
            ToolParameter {
                name: "language".to_string(),
                description: "The programming language of the code".to_string(),
                required: true,
                default_value: None,
                param_type: Some(ToolParameterType::String),
            },
        ]
    }

    async fn execute(&self, args: &[&str]) -> Result<String> {
        if args.is_empty() {
            return Err(anyhow::anyhow!("No code provided"));
        }

        let code = args[0];
        let file_type = if args.len() > 1 { args[1] } else { "rs" };

        // Create temp file with the code
        let temp_file = self.create_temp_file(code, file_type)?;
        let file_path = temp_file.to_string_lossy();

        let result = match file_type {
            "rs" => {
                // Check Rust code using rustc
                let mut cmd = Command::new("rustc");
                cmd.arg("--color=always")
                    .arg("--emit=metadata") // Don't generate binary, just check
                    .arg("-Z")
                    .arg("no-codegen") // Don't generate code
                    .arg("--crate-type=lib") // Compile as a library
                    .arg(&*file_path); // Dereference the Cow<str>

                match cmd.output() {
                    Ok(output) => {
                        let success = output.status.success();
                        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                        if success {
                            "✅ Code compiles successfully without errors!".to_string()
                        } else {
                            format!("❌ Compilation errors found:\n\n{}", stderr)
                        }
                    }
                    Err(e) => {
                        format!("Error running rustc: {}", e)
                    }
                }
            }
            "toml" => {
                // Validate TOML using Rust's built-in parser
                let mut cmd = Command::new("cargo");
                cmd.arg("script");
                cmd.arg("--");
                cmd.arg(format!(
                    r#"
                    use std::fs;
                    use std::path::Path;

                    fn main() -> Result<(), Box<dyn std::error::Error>> {{
                        let path = Path::new("{}");
                        let content = fs::read_to_string(path)?;

                        match toml::from_str::<toml::Value>(&content) {{
                            Ok(_) => println!("✅ Valid TOML file."),
                            Err(e) => println!("❌ Invalid TOML: {{}}", e),
                        }}

                        Ok(())
                    }}
                "#,
                    file_path.as_ref()
                ));

                match cmd.output() {
                    Ok(output) => {
                        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                        if stdout.contains("✅ Valid TOML file") {
                            "✅ Valid TOML file.".to_string()
                        } else if stderr.contains("Invalid TOML") {
                            format!("❌ Invalid TOML: {}", stderr)
                        } else {
                            format!("Output: {}\nErrors: {}", stdout, stderr)
                        }
                    }
                    Err(_e) => {
                        // Fallback to more basic validation
                        match toml::from_str::<toml::Value>(code) {
                            Ok(_) => "✅ Valid TOML file (basic check).".to_string(),
                            Err(e) => format!("❌ Invalid TOML: {}", e),
                        }
                    }
                }
            }
            other => {
                format!("Compilation check for {} files is not implemented", other)
            }
        };

        // Clean up the temporary file
        if let Err(e) = std::fs::remove_file(&temp_file) {
            warn!(
                "Failed to remove temporary file {}: {}",
                temp_file.display(),
                e
            );
        }

        Ok(result)
    }
}

/// A tool that creates a new file
pub struct CreateFileTool {
    workspace: PathBuf,
}

impl CreateFileTool {
    /// Create a new file creation tool
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl LlmTool for CreateFileTool {
    fn name(&self) -> &str {
        "create_file"
    }

    fn description(&self) -> &str {
        "Create a new file with the specified content. Will fail if the file already exists."
    }

    fn parameters(&self) -> Vec<ToolParameter> {
        vec![
            ToolParameter {
                name: "file_path".to_string(),
                description: "Path to the new file to create, relative to the workspace"
                    .to_string(),
                required: true,
                default_value: None,
                param_type: Some(ToolParameterType::String),
            },
            ToolParameter {
                name: "content".to_string(),
                description: "Content to write to the new file".to_string(),
                required: true,
                default_value: None,
                param_type: Some(ToolParameterType::Code),
            },
        ]
    }

    async fn execute(&self, args: &[&str]) -> Result<String> {
        if args.len() < 2 {
            return Err(anyhow::anyhow!("Both file_path and content are required"));
        }

        let file_path = Path::new(args[0]);
        let full_path = self.workspace.join(file_path);

        // Check if file already exists
        if full_path.exists() {
            return Err(anyhow::anyhow!(
                "File already exists: {}",
                file_path.display()
            ));
        }

        // Create parent directories if needed
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)
                .context(format!("Failed to create directory: {:?}", parent))?;
        }

        // Write content to file
        let content = args[1];
        std::fs::write(&full_path, content)
            .context(format!("Failed to write to file: {:?}", full_path))?;

        Ok(format!(
            "Successfully created file: {}",
            file_path.display()
        ))
    }
}

/// A tool that modifies an existing file
pub struct ModifyFileTool {
    workspace: PathBuf,
}

impl ModifyFileTool {
    /// Create a new file modification tool
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    /// Apply changes to specific lines of a file
    fn apply_line_specific_changes(
        &self,
        content: &str,
        start_line: usize,
        end_line: Option<usize>,
        new_content: &str,
    ) -> Result<String> {
        let lines: Vec<&str> = content.lines().collect();
        let start_idx = start_line.saturating_sub(1);
        let end_idx = match end_line {
            Some(end) => std::cmp::min(end, lines.len()),
            None => start_idx + 1,
        };

        if start_idx >= lines.len() {
            return Err(anyhow::anyhow!(
                "Start line {} is beyond the end of the file",
                start_line
            ));
        }

        let mut result = Vec::new();

        // Add lines before the edit
        result.extend(lines.iter().take(start_idx).map(|s| s.to_string()));

        // Add the new content
        result.extend(new_content.lines().map(|s| s.to_string()));

        // Add lines after the edit
        result.extend(lines.iter().skip(end_idx).map(|s| s.to_string()));

        Ok(result.join("\n"))
    }
}

#[async_trait]
impl LlmTool for ModifyFileTool {
    fn name(&self) -> &str {
        "modify_file"
    }

    fn description(&self) -> &str {
        "Modify an existing file. Can replace the entire content or specific lines."
    }

    fn parameters(&self) -> Vec<ToolParameter> {
        vec![
            ToolParameter {
                name: "file_path".to_string(),
                description: "Path to the file to modify, relative to the workspace".to_string(),
                required: true,
                default_value: None,
                param_type: Some(ToolParameterType::String),
            },
            ToolParameter {
                name: "new_content".to_string(),
                description: "New content or replacement content for the file".to_string(),
                required: true,
                default_value: None,
                param_type: Some(ToolParameterType::Code),
            },
            ToolParameter {
                name: "start_line".to_string(),
                description: "Starting line number for partial modifications (1-indexed)"
                    .to_string(),
                required: false,
                default_value: None,
                param_type: Some(ToolParameterType::Integer),
            },
            ToolParameter {
                name: "end_line".to_string(),
                description: "Ending line number for partial modifications (1-indexed, inclusive)"
                    .to_string(),
                required: false,
                default_value: None,
                param_type: Some(ToolParameterType::Integer),
            },
        ]
    }

    async fn execute(&self, args: &[&str]) -> Result<String> {
        if args.len() < 2 {
            return Err(anyhow::anyhow!(
                "Both file_path and new_content are required"
            ));
        }

        let file_path = Path::new(args[0]);
        let full_path = self.workspace.join(file_path);

        // Check if file exists
        if !full_path.exists() {
            return Err(anyhow::anyhow!(
                "File does not exist: {}",
                file_path.display()
            ));
        }

        // Read the current content
        let current_content = std::fs::read_to_string(&full_path)
            .context(format!("Failed to read file: {}", file_path.display()))?;

        let new_content = args[1];

        // Parse line range if specified
        let result = if args.len() > 2 {
            let start_line = args[2]
                .parse::<usize>()
                .context("Invalid start_line: must be a positive integer")?;

            let end_line = if args.len() > 3 {
                Some(
                    args[3]
                        .parse::<usize>()
                        .context("Invalid end_line: must be a positive integer")?,
                )
            } else {
                None
            };

            self.apply_line_specific_changes(&current_content, start_line, end_line, new_content)?
        } else {
            // Replace entire content
            new_content.to_string()
        };

        // Write the modified content back to the file
        std::fs::write(&full_path, result)
            .context(format!("Failed to write to file: {:?}", full_path))?;

        Ok(format!(
            "Successfully modified file: {}",
            file_path.display()
        ))
    }
}

/// A tool that executes git commands
pub struct GitCommandTool {
    workspace: PathBuf,
}

impl GitCommandTool {
    /// Create a new Git command tool
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    /// Validate if the Git command is safe to execute
    fn is_safe_git_command(&self, command: &str) -> bool {
        // Split the command into parts
        let parts: Vec<&str> = command.split_whitespace().collect();

        // Check if it's a git command
        if parts.is_empty() || parts[0] != "git" {
            return false;
        }

        // Check for unsafe commands that could potentially harm the system
        let unsafe_operations = [
            "clean",
            "reset --hard",
            "push --force",
            "push -f",
            "filter-branch",
            "gc",
            "prune",
            "reflog",
            "rm -r",
            "rm -rf",
            "rm --cached -r",
        ];

        for unsafe_op in unsafe_operations {
            if command.contains(unsafe_op) {
                return false;
            }
        }

        // Disallow arbitrary shell commands through git hooks
        if command.contains("hook")
            || command.contains(";")
            || command.contains("&&")
            || command.contains("||")
            || command.contains("|")
            || command.contains(">")
            || command.contains("<")
        {
            return false;
        }

        true
    }
}

#[async_trait]
impl LlmTool for GitCommandTool {
    fn name(&self) -> &str {
        "git_command"
    }

    fn description(&self) -> &str {
        "Execute Git commands directly in the workspace."
    }

    fn parameters(&self) -> Vec<ToolParameter> {
        vec![ToolParameter {
            name: "command".to_string(),
            description: "The Git command to execute (e.g., 'status', 'log', etc.)".to_string(),
            required: true,
            default_value: None,
            param_type: Some(ToolParameterType::String),
        }]
    }

    async fn execute(&self, args: &[&str]) -> Result<String> {
        if args.is_empty() {
            return Err(anyhow::anyhow!("Git command is required"));
        }

        let command = args[0];

        // Validate that the command is safe
        if !self.is_safe_git_command(command) {
            return Err(anyhow::anyhow!(
                "Unsafe git command rejected. The command must start with 'git' and cannot include potentially destructive operations or shell escape sequences."
            ));
        }

        info!("Executing git command: {}", command);

        // Run the command in the workspace directory
        let output = Command::new("sh")
            .current_dir(&self.workspace)
            .arg("-c")
            .arg(command)
            .output()
            .context("Failed to execute git command")?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        let result = if output.status.success() {
            format!("Command executed successfully:\n{}", stdout)
        } else {
            format!(
                "Command failed with exit code {}:\n{}",
                output.status.code().unwrap_or(-1),
                if stderr.is_empty() { stdout } else { stderr }
            )
        };

        Ok(result)
    }
}

/// Tool registry for managing available tools
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn LlmTool>>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    /// Create a new tool registry
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool
    pub fn register<T: LlmTool + 'static>(&mut self, tool: T) {
        let name = tool.name().to_string();
        self.tools.insert(name, Box::new(tool));
    }

    /// Validate tool call against parameter specifications
    fn validate_tool_call(&self, tool_name: &str, args: &[&str]) -> Result<()> {
        if let Some(tool) = self.tools.get(tool_name) {
            let params = tool.parameters();

            // If no parameters are specified, skip validation (backward compatibility)
            if params.is_empty() {
                return Ok(());
            }

            // Check for required parameters
            let required_params: Vec<&ToolParameter> =
                params.iter().filter(|p| p.required).collect();

            if args.len() < required_params.len() {
                let missing_params: Vec<String> = required_params
                    .iter()
                    .skip(args.len())
                    .map(|p| p.name.clone())
                    .collect();

                return Err(anyhow::anyhow!(
                    "Missing required parameters for tool '{}': {}",
                    tool_name,
                    missing_params.join(", ")
                ));
            }

            Ok(())
        } else {
            Err(anyhow::anyhow!("Tool not found: {}", tool_name))
        }
    }

    /// Execute a tool
    pub async fn execute(&self, tool_call: &ToolCall) -> ToolResult {
        match self.execute_tool(tool_call).await {
            Ok(result) => result,
            Err(e) => ToolResult {
                success: false,
                result: String::new(),
                error: Some(format!("Error executing tool: {}", e)),
            },
        }
    }

    /// Extract tool calls from a string response
    pub fn extract_tool_calls(&self, response: &str) -> Vec<ToolCall> {
        let mut tool_calls = Vec::new();

        // Look for JSON-formatted tool calls
        let re = regex::Regex::new(r#"\{(?:\s*)"tool"(?:\s*):(?:\s*)"([^"]+)"(?:\s*),(?:\s*)"args"(?:\s*):(?:\s*)\[(.*?)\](?:\s*)\}"#).unwrap();

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
        let alt_re =
            regex::Regex::new(r"(?i)use\s+tool\s+([a-z_]+)(?:\s*):(?:\s*)(.+?)(?:\n|$)").unwrap();
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

    /// Execute a specific tool call
    pub async fn execute_tool(&self, tool_call: &ToolCall) -> Result<ToolResult> {
        if let Some(tool) = self.tools.get(&tool_call.tool) {
            // Convert Vec<String> to Vec<&str> for the tool execute method
            let args: Vec<&str> = tool_call.args.iter().map(|s| s.as_str()).collect();

            // Validate the tool call against parameter specifications
            if let Err(e) = self.validate_tool_call(&tool_call.tool, &args) {
                return Ok(ToolResult {
                    success: false,
                    result: String::new(),
                    error: Some(e.to_string()),
                });
            }

            match tool.execute(&args).await {
                Ok(result) => Ok(ToolResult {
                    success: true,
                    result,
                    error: None,
                }),
                Err(e) => Ok(ToolResult {
                    success: false,
                    result: String::new(),
                    error: Some(e.to_string()),
                }),
            }
        } else {
            Ok(ToolResult {
                success: false,
                result: String::new(),
                error: Some(format!("Tool not found: {}", tool_call.tool)),
            })
        }
    }

    /// Get all tool descriptions
    pub fn get_tool_descriptions(&self) -> Vec<(String, String)> {
        self.tools
            .iter()
            .map(|(name, tool)| (name.clone(), tool.description().to_string()))
            .collect()
    }

    /// Get detailed information about tools including parameter specifications
    pub fn get_tool_specifications(&self) -> Vec<(String, String, Vec<ToolParameter>)> {
        self.tools
            .iter()
            .map(|(name, tool)| {
                (
                    name.clone(),
                    tool.description().to_string(),
                    tool.parameters(),
                )
            })
            .collect()
    }
}
