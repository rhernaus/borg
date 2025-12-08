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

/// A tool that searches code using ripgrep
pub struct GrepTool {
    workspace: PathBuf,
    #[allow(dead_code)]
    git_manager: Arc<Mutex<dyn GitManager>>,
}

impl GrepTool {
    /// Create a new grep tool
    pub fn new(workspace: PathBuf, git_manager: Arc<Mutex<dyn GitManager>>) -> Self {
        Self {
            workspace,
            git_manager,
        }
    }
}

#[async_trait]
impl LlmTool for GrepTool {
    fn name(&self) -> &str {
        "Grep"
    }

    fn description(&self) -> &str {
        "Search for code patterns or symbols in the codebase using ripgrep."
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
pub struct ReadTool {
    workspace: PathBuf,
}

impl ReadTool {
    /// Create a new file contents tool
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl LlmTool for ReadTool {
    fn name(&self) -> &str {
        "Read"
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
/// A tool that finds files matching glob patterns
pub struct GlobTool {
    workspace: PathBuf,
}

impl GlobTool {
    /// Create a new glob tool
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl LlmTool for GlobTool {
    fn name(&self) -> &str {
        "Glob"
    }

    fn description(&self) -> &str {
        "Find files matching glob patterns (e.g., '**/*.rs', 'src/**/*.toml')"
    }

    fn parameters(&self) -> Vec<ToolParameter> {
        vec![
            ToolParameter {
                name: "pattern".to_string(),
                description: "Glob pattern to match files (e.g., '**/*.rs')".to_string(),
                required: true,
                default_value: None,
                param_type: Some(ToolParameterType::String),
            },
            ToolParameter {
                name: "path".to_string(),
                description: "Optional directory to search in (defaults to workspace root)"
                    .to_string(),
                required: false,
                default_value: None,
                param_type: Some(ToolParameterType::String),
            },
        ]
    }

    async fn execute(&self, args: &[&str]) -> Result<String> {
        if args.is_empty() {
            return Err(anyhow::anyhow!("No glob pattern provided"));
        }

        let pattern = args[0];

        // Determine the search base directory
        let search_dir = if args.len() > 1 && !args[1].is_empty() {
            self.workspace.join(args[1])
        } else {
            self.workspace.clone()
        };

        if !search_dir.exists() {
            return Err(anyhow::anyhow!(
                "Search directory not found: {}",
                search_dir.display()
            ));
        }

        // Build the full glob pattern including the base directory
        let glob_pattern = search_dir.join(pattern);
        let pattern_str = glob_pattern.to_string_lossy();

        // Use glob crate to find matching files
        let mut matches = Vec::new();
        match glob::glob(&pattern_str) {
            Ok(paths) => {
                for entry in paths {
                    match entry {
                        Ok(path) => {
                            // Get path relative to workspace
                            if let Ok(rel_path) = path.strip_prefix(&self.workspace) {
                                matches.push(rel_path.to_string_lossy().to_string());
                            } else {
                                matches.push(path.to_string_lossy().to_string());
                            }
                        }
                        Err(e) => {
                            warn!("Error reading glob entry: {}", e);
                        }
                    }
                }
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Invalid glob pattern '{}': {}", pattern, e));
            }
        }

        if matches.is_empty() {
            Ok(format!("No files found matching pattern: {}", pattern))
        } else {
            // Sort matches for consistent output
            matches.sort();
            Ok(format!(
                "Files matching '{}' ({} files):
{}",
                pattern,
                matches.len(),
                matches.join(
                    "
"
                )
            ))
        }
    }
}

/// A tool that executes shell commands
pub struct BashTool {
    workspace: PathBuf,
}

impl BashTool {
    /// Create a new bash tool
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    /// Safety blocklist for dangerous commands
    fn is_safe_command(&self, command: &str) -> Result<()> {
        let dangerous_patterns = [
            "rm -rf /",
            "rm -rf /*",
            "rm -rf ~",
            "rm -rf $HOME",
            "mkfs",
            "dd if=",
            "> /dev/sda",
            "> /dev/sd",
            "wipefs",
            "shred",
            ":(){:|:&};:", // Fork bomb
            "chmod -R 777 /",
            "chown -R",
        ];

        let cmd_lower = command.to_lowercase();
        for pattern in &dangerous_patterns {
            if cmd_lower.contains(&pattern.to_lowercase()) {
                return Err(anyhow::anyhow!(
                    "Dangerous command blocked: contains pattern '{}'",
                    pattern
                ));
            }
        }

        // Block commands trying to escape workspace
        if command.contains("../") || command.contains("/..") {
            return Err(anyhow::anyhow!(
                "Command blocked: attempting to access parent directories"
            ));
        }

        Ok(())
    }
}

#[async_trait]
impl LlmTool for BashTool {
    fn name(&self) -> &str {
        "Bash"
    }

    fn description(&self) -> &str {
        "Execute shell commands in the workspace. Has safety blocklist for dangerous commands."
    }

    fn parameters(&self) -> Vec<ToolParameter> {
        vec![
            ToolParameter {
                name: "command".to_string(),
                description: "Shell command to execute".to_string(),
                required: true,
                default_value: None,
                param_type: Some(ToolParameterType::String),
            },
            ToolParameter {
                name: "description".to_string(),
                description: "Optional description of what this command does".to_string(),
                required: false,
                default_value: None,
                param_type: Some(ToolParameterType::String),
            },
            ToolParameter {
                name: "timeout".to_string(),
                description: "Optional timeout in milliseconds (max 600000ms / 10 minutes)"
                    .to_string(),
                required: false,
                default_value: Some("120000".to_string()),
                param_type: Some(ToolParameterType::Integer),
            },
            ToolParameter {
                name: "run_in_background".to_string(),
                description: "Set to true to run command in background".to_string(),
                required: false,
                default_value: Some("false".to_string()),
                param_type: Some(ToolParameterType::Boolean),
            },
        ]
    }

    async fn execute(&self, args: &[&str]) -> Result<String> {
        if args.is_empty() {
            return Err(anyhow::anyhow!("No command provided"));
        }

        let command = args[0];
        let _description = if args.len() > 1 && !args[1].is_empty() {
            Some(args[1])
        } else {
            None
        };

        let timeout_ms = if args.len() > 2 {
            args[2].parse::<u64>().unwrap_or(120000).min(600000)
        } else {
            120000
        };

        let run_in_background = if args.len() > 3 {
            args[3].parse::<bool>().unwrap_or(false)
        } else {
            false
        };

        // Safety check
        self.is_safe_command(command)?;

        info!("Executing command: {}", command);

        if run_in_background {
            // For background execution, we'd need a process manager
            // For now, just return a message
            return Ok(format!(
                "Background execution not yet implemented. Command would run: {}",
                command
            ));
        }

        // Execute command with timeout
        let output = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            tokio::process::Command::new("sh")
                .current_dir(&self.workspace)
                .arg("-c")
                .arg(command)
                .output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Command timed out after {}ms", timeout_ms))?
        .context("Failed to execute command")?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        let result = if output.status.success() {
            if stdout.is_empty() && stderr.is_empty() {
                "Command completed successfully (no output)".to_string()
            } else if stdout.is_empty() {
                format!("Command completed successfully:\n{}", stderr)
            } else {
                format!("Command completed successfully:\n{}", stdout)
            }
        } else {
            let error_output = if stderr.is_empty() { &stdout } else { &stderr };
            format!(
                "Command failed with exit code {}:\n{}",
                exit_code, error_output
            )
        };

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
pub struct WriteTool {
    workspace: PathBuf,
}

impl WriteTool {
    /// Create a new file creation tool
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl LlmTool for WriteTool {
    fn name(&self) -> &str {
        "Write"
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
pub struct EditTool {
    workspace: PathBuf,
}

impl EditTool {
    /// Create a new file modification tool
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl LlmTool for EditTool {
    fn name(&self) -> &str {
        "Edit"
    }

    fn description(&self) -> &str {
        "Edit an existing file by replacing exact string matches. Fails if old_string is not found or not unique (unless replace_all is true)."
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
                name: "old_string".to_string(),
                description: "Exact string to find and replace in the file".to_string(),
                required: true,
                default_value: None,
                param_type: Some(ToolParameterType::Code),
            },
            ToolParameter {
                name: "new_string".to_string(),
                description: "String to replace old_string with".to_string(),
                required: true,
                default_value: None,
                param_type: Some(ToolParameterType::Code),
            },
            ToolParameter {
                name: "replace_all".to_string(),
                description: "If true, replace all occurrences. If false (default), fail if old_string appears more than once"
                    .to_string(),
                required: false,
                default_value: Some("false".to_string()),
                param_type: Some(ToolParameterType::Boolean),
            },
        ]
    }

    async fn execute(&self, args: &[&str]) -> Result<String> {
        if args.len() < 3 {
            return Err(anyhow::anyhow!(
                "file_path, old_string, and new_string are all required"
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

        let old_string = args[1];
        let new_string = args[2];
        let replace_all = if args.len() > 3 {
            args[3].parse::<bool>().unwrap_or(false)
        } else {
            false
        };

        // Check if old_string exists in the file
        if !current_content.contains(old_string) {
            return Err(anyhow::anyhow!(
                "String not found in file: {}",
                file_path.display()
            ));
        }

        // Count occurrences of old_string
        let occurrences = current_content.matches(old_string).count();

        // If replace_all is false and there are multiple occurrences, fail
        if !replace_all && occurrences > 1 {
            return Err(anyhow::anyhow!(
                "String appears {} times in file (not unique). Use replace_all=true to replace all occurrences, or provide a more specific old_string.",
                occurrences
            ));
        }

        // Perform the replacement
        let result = if replace_all {
            current_content.replace(old_string, new_string)
        } else {
            // Replace only the first (and only) occurrence
            current_content.replacen(old_string, new_string, 1)
        };

        // Write the modified content back to the file
        std::fs::write(&full_path, result)
            .context(format!("Failed to write to file: {:?}", full_path))?;

        let message = if replace_all {
            format!(
                "Successfully modified file: {} ({} occurrence(s) replaced)",
                file_path.display(),
                occurrences
            )
        } else {
            format!("Successfully modified file: {}", file_path.display())
        };

        Ok(message)
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

/// A tool that runs tests and returns structured feedback
pub struct TestRunnerTool {
    workspace: PathBuf,
}

impl TestRunnerTool {
    /// Create a new test runner tool
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl LlmTool for TestRunnerTool {
    fn name(&self) -> &str {
        "run_tests"
    }

    fn description(&self) -> &str {
        "Run the project's test suite and return results. Use this to verify code changes work correctly."
    }

    fn parameters(&self) -> Vec<ToolParameter> {
        vec![ToolParameter {
            name: "test_filter".to_string(),
            description: "Optional filter to run specific tests (e.g., 'test_name' or 'module::')"
                .to_string(),
            required: false,
            default_value: None,
            param_type: Some(ToolParameterType::String),
        }]
    }

    async fn execute(&self, args: &[&str]) -> Result<String> {
        let test_filter = if !args.is_empty() && !args[0].is_empty() {
            Some(args[0])
        } else {
            None
        };

        info!("Running tests in workspace: {:?}", self.workspace);

        // Build the cargo test command
        let mut cmd = Command::new("cargo");
        cmd.current_dir(&self.workspace)
            .arg("test")
            .arg("--")
            .arg("--color=never"); // Disable color for easier parsing

        if let Some(filter) = test_filter {
            cmd.arg(filter);
        }

        match cmd.output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let success = output.status.success();

                // Parse test results
                let mut result = String::new();

                if success {
                    result.push_str("✅ All tests passed!\n\n");
                } else {
                    result.push_str("❌ Some tests failed!\n\n");
                }

                // Extract test summary line
                if let Some(summary_line) = stdout.lines().find(|l| l.contains("test result:")) {
                    result.push_str(&format!("Summary: {}\n\n", summary_line));
                }

                // Extract failed test names
                let failed_tests: Vec<&str> =
                    stdout.lines().filter(|l| l.contains("FAILED")).collect();

                if !failed_tests.is_empty() {
                    result.push_str("Failed tests:\n");
                    for test in failed_tests {
                        result.push_str(&format!("  - {}\n", test));
                    }
                    result.push('\n');
                }

                // Include compilation errors if any
                if !stderr.is_empty() && stderr.contains("error") {
                    result.push_str("Compilation/Runtime Errors:\n");
                    // Limit error output to avoid overwhelming the context
                    let error_lines: Vec<&str> = stderr
                        .lines()
                        .filter(|l| l.contains("error") || l.contains("-->"))
                        .take(20)
                        .collect();
                    for line in error_lines {
                        result.push_str(&format!("{}\n", line));
                    }
                }

                Ok(result)
            }
            Err(e) => Err(anyhow::anyhow!("Failed to run tests: {}", e)),
        }
    }
}

/// Tool for fetching web content from URLs
pub struct WebFetchTool {
    client: reqwest::Client,
}

impl WebFetchTool {
    /// Create a new web fetch tool
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent("Borg/1.0 (Autonomous Agent)")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { client }
    }

    /// Extract text content from HTML (basic extraction)
    fn extract_text_from_html(html: &str) -> String {
        // Remove script and style tags
        let re_script = regex::Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap();
        let re_style = regex::Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap();
        let re_tags = regex::Regex::new(r"<[^>]+>").unwrap();
        let re_whitespace = regex::Regex::new(r"\s+").unwrap();

        let mut text = html.to_string();
        text = re_script.replace_all(&text, " ").to_string();
        text = re_style.replace_all(&text, " ").to_string();
        text = re_tags.replace_all(&text, " ").to_string();
        text = re_whitespace.replace_all(&text, " ").to_string();

        // Decode common HTML entities
        text = text
            .replace("&nbsp;", " ")
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'");

        text.trim().to_string()
    }
}

/// A tool that tracks and displays todo progress
pub struct TodoWriteTool;

impl TodoWriteTool {
    /// Create a new TodoWrite tool
    pub fn new() -> Self {
        Self
    }
}

impl Default for TodoWriteTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LlmTool for TodoWriteTool {
    fn name(&self) -> &str {
        "TodoWrite"
    }

    fn description(&self) -> &str {
        "Track and display todo list progress. Updates the current todo list with new items or status changes."
    }

    fn parameters(&self) -> Vec<ToolParameter> {
        vec![ToolParameter {
            name: "todos".to_string(),
            description: "JSON string containing array of todo items. Each item has: content (String), status (String: pending/in_progress/completed), activeForm (String)".to_string(),
            required: true,
            default_value: None,
            param_type: Some(ToolParameterType::String),
        }]
    }

    async fn execute(&self, args: &[&str]) -> Result<String> {
        if args.is_empty() {
            return Err(anyhow::anyhow!("todos parameter is required"));
        }

        let todos_json = args[0];

        // Parse the JSON input
        let todos: Vec<serde_json::Value> = serde_json::from_str(todos_json)
            .map_err(|e| anyhow::anyhow!("Failed to parse todos JSON: {}", e))?;

        if todos.is_empty() {
            return Ok("Todo list is empty".to_string());
        }

        // Format and return a status message showing the todo list
        let mut result = String::from("Todo List:\n");

        for (idx, todo) in todos.iter().enumerate() {
            let content = todo["content"].as_str().unwrap_or("(no content)");
            let status = todo["status"].as_str().unwrap_or("pending");
            let _active_form = todo["activeForm"].as_str().unwrap_or(content);

            let status_icon = match status {
                "completed" => "✓",
                "in_progress" => "→",
                _ => "○",
            };

            result.push_str(&format!("{}. {} {}\n", idx + 1, status_icon, content));
        }

        Ok(result)
    }
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LlmTool for WebFetchTool {
    fn name(&self) -> &str {
        "WebFetch"
    }

    fn description(&self) -> &str {
        "Fetch content from a URL to get factual information from the web. \
         Useful for looking up documentation, APIs, or current information."
    }

    fn parameters(&self) -> Vec<ToolParameter> {
        vec![ToolParameter {
            name: "url".to_string(),
            description: "The URL to fetch content from".to_string(),
            required: true,
            default_value: None,
            param_type: Some(ToolParameterType::String),
        }]
    }

    async fn execute(&self, args: &[&str]) -> Result<String> {
        if args.is_empty() {
            return Err(anyhow::anyhow!("URL is required"));
        }

        let url = args[0].trim();
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(anyhow::anyhow!(
                "Invalid URL: must start with http:// or https://"
            ));
        }

        // Fetch the URL
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch URL: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "HTTP error: {} {}",
                response.status().as_u16(),
                response.status().canonical_reason().unwrap_or("Unknown")
            ));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();

        let body = response
            .text()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read response: {}", e))?;

        // Extract text from HTML or return raw for other content types
        let text = if content_type.contains("text/html") {
            Self::extract_text_from_html(&body)
        } else {
            body
        };

        // Limit output size
        let max_chars = 8000;
        if text.len() > max_chars {
            Ok(format!(
                "{}\n\n[Content truncated, showing first {} characters]",
                &text[..max_chars],
                max_chars
            ))
        } else {
            Ok(text)
        }
    }
}

/// Tool for searching the web using DuckDuckGo
pub struct WebSearchTool {
    client: reqwest::Client,
}

impl WebSearchTool {
    /// Create a new web search tool
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:120.0) Gecko/20100101 Firefox/120.0")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { client }
    }

    /// Parse DuckDuckGo HTML search results
    fn parse_search_results(html: &str) -> Vec<(String, String, String)> {
        let mut results = Vec::new();

        // Parse search results from DuckDuckGo HTML
        // Each result is in a div with class "result" or "web-result"
        let result_re = regex::Regex::new(
            r#"(?is)<div[^>]*class="[^"]*(?:result|web-result)[^"]*"[^>]*>.*?</div>"#,
        )
        .unwrap_or_else(|_| regex::Regex::new(r"").unwrap());

        // Extract title from result__a or result__title
        let title_re =
            regex::Regex::new(r#"(?is)<a[^>]*class="[^"]*result__a[^"]*"[^>]*>(.*?)</a>"#)
                .unwrap_or_else(|_| regex::Regex::new(r"").unwrap());

        // Extract URL from href
        let url_re = regex::Regex::new(r#"(?is)href="//duckduckgo\.com/l/\?uddg=([^"&]+)"#)
            .unwrap_or_else(|_| regex::Regex::new(r"").unwrap());

        // Alternative URL extraction
        let url_direct_re = regex::Regex::new(r#"(?is)href="(https?://[^"]+)""#)
            .unwrap_or_else(|_| regex::Regex::new(r"").unwrap());

        // Extract snippet from result__snippet
        let snippet_re =
            regex::Regex::new(r#"(?is)<a[^>]*class="[^"]*result__snippet[^"]*"[^>]*>(.*?)</a>"#)
                .unwrap_or_else(|_| regex::Regex::new(r"").unwrap());

        // Pre-compile regex for stripping HTML tags (used in loop)
        let tag_stripper = regex::Regex::new(r"<[^>]+>").unwrap();

        for result_match in result_re.find_iter(html).take(10) {
            let result_html = result_match.as_str();

            // Extract title
            let title = title_re
                .captures(result_html)
                .and_then(|cap| cap.get(1))
                .map(|m| {
                    let raw = m.as_str();
                    // Remove HTML tags and decode entities
                    let clean = tag_stripper.replace_all(raw, "");
                    clean
                        .replace("&nbsp;", " ")
                        .replace("&amp;", "&")
                        .replace("&lt;", "<")
                        .replace("&gt;", ">")
                        .replace("&quot;", "\"")
                        .replace("&#39;", "'")
                        .trim()
                        .to_string()
                })
                .unwrap_or_default();

            // Extract URL
            let url = url_re
                .captures(result_html)
                .and_then(|cap| cap.get(1))
                .map(|m| {
                    // URL decode
                    urlencoding::decode(m.as_str())
                        .unwrap_or_default()
                        .to_string()
                })
                .or_else(|| {
                    url_direct_re
                        .captures(result_html)
                        .and_then(|cap| cap.get(1))
                        .map(|m| m.as_str().to_string())
                })
                .unwrap_or_default();

            // Extract snippet
            let snippet = snippet_re
                .captures(result_html)
                .and_then(|cap| cap.get(1))
                .map(|m| {
                    let raw = m.as_str();
                    // Remove HTML tags and decode entities
                    let clean = tag_stripper.replace_all(raw, "");
                    clean
                        .replace("&nbsp;", " ")
                        .replace("&amp;", "&")
                        .replace("&lt;", "<")
                        .replace("&gt;", ">")
                        .replace("&quot;", "\"")
                        .replace("&#39;", "'")
                        .trim()
                        .to_string()
                })
                .unwrap_or_default();

            if !title.is_empty() && !url.is_empty() {
                results.push((title, url, snippet));
            }
        }

        results
    }
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LlmTool for WebSearchTool {
    fn name(&self) -> &str {
        "WebSearch"
    }

    fn description(&self) -> &str {
        "Search the web using DuckDuckGo to find relevant information. \
         Returns a list of search results with titles, URLs, and snippets."
    }

    fn parameters(&self) -> Vec<ToolParameter> {
        vec![
            ToolParameter {
                name: "query".to_string(),
                description: "The search query to execute".to_string(),
                required: true,
                default_value: None,
                param_type: Some(ToolParameterType::String),
            },
            ToolParameter {
                name: "allowed_domains".to_string(),
                description: "Optional comma-separated list of domains to restrict search to (e.g., 'github.com,docs.rs')".to_string(),
                required: false,
                default_value: None,
                param_type: Some(ToolParameterType::String),
            },
            ToolParameter {
                name: "blocked_domains".to_string(),
                description: "Optional comma-separated list of domains to exclude from search (e.g., 'example.com,spam.com')".to_string(),
                required: false,
                default_value: None,
                param_type: Some(ToolParameterType::String),
            },
        ]
    }

    async fn execute(&self, args: &[&str]) -> Result<String> {
        if args.is_empty() {
            return Err(anyhow::anyhow!("Search query is required"));
        }

        let query = args[0].trim();
        if query.is_empty() {
            return Err(anyhow::anyhow!("Search query cannot be empty"));
        }

        // Parse optional domain filters
        let allowed_domains: Option<Vec<String>> = if args.len() > 1 && !args[1].is_empty() {
            Some(
                args[1]
                    .split(',')
                    .map(|s| s.trim().to_lowercase())
                    .collect(),
            )
        } else {
            None
        };

        let blocked_domains: Option<Vec<String>> = if args.len() > 2 && !args[2].is_empty() {
            Some(
                args[2]
                    .split(',')
                    .map(|s| s.trim().to_lowercase())
                    .collect(),
            )
        } else {
            None
        };

        // Build DuckDuckGo search URL
        let search_url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(query)
        );

        info!("Searching DuckDuckGo: {}", query);

        // Fetch search results
        let response = self
            .client
            .get(&search_url)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to execute search: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Search failed with HTTP error: {} {}",
                response.status().as_u16(),
                response.status().canonical_reason().unwrap_or("Unknown")
            ));
        }

        let html = response
            .text()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read search results: {}", e))?;

        // Parse search results
        let mut results = Self::parse_search_results(&html);

        // Apply domain filters
        if let Some(ref allowed) = allowed_domains {
            results.retain(|(_, url, _)| {
                allowed
                    .iter()
                    .any(|domain| url.to_lowercase().contains(domain))
            });
        }

        if let Some(ref blocked) = blocked_domains {
            results.retain(|(_, url, _)| {
                !blocked
                    .iter()
                    .any(|domain| url.to_lowercase().contains(domain))
            });
        }

        // Format results
        if results.is_empty() {
            Ok(format!("No search results found for query: {}", query))
        } else {
            let mut output = format!("Search results for '{}':\n\n", query);

            for (idx, (title, url, snippet)) in results.iter().enumerate().take(10) {
                output.push_str(&format!("{}. {}\n", idx + 1, title));
                output.push_str(&format!("   URL: {}\n", url));
                if !snippet.is_empty() {
                    output.push_str(&format!("   {}\n", snippet));
                }
                output.push('\n');
            }

            Ok(output)
        }
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
