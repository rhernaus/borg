use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Helper function to initialize a git repository in a directory
fn init_git_repo(dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    // Initialize git repo
    let init_output = Command::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()?;

    if !init_output.status.success() {
        return Err(format!(
            "Failed to initialize git repo: {}",
            String::from_utf8_lossy(&init_output.stderr)
        )
        .into());
    }

    // Configure git user for the test repo
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(dir)
        .output()?;

    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(dir)
        .output()?;

    // Create initial commit to ensure HEAD exists
    fs::write(
        dir.join("README.md"),
        "# Test Project\n\nThis is a test project for the self-improvement loop.\n",
    )?;

    Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .output()?;

    let commit_output = Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(dir)
        .output()?;

    if !commit_output.status.success() {
        return Err(format!(
            "Failed to create initial commit: {}",
            String::from_utf8_lossy(&commit_output.stderr)
        )
        .into());
    }

    Ok(())
}

/// Helper function to create a test config file
fn create_test_config(
    workspace_dir: &Path,
    config_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let config_content = format!(
        r#"
[llm.default]
provider = "mock"
api_key = "test-api-key"
model = "mock-model"
max_tokens = 1024
temperature = 0.7

[llm.code_generation]
provider = "mock"
api_key = "test-api-key"
model = "mock-model"
max_tokens = 2048
temperature = 0.5

[agent]
name = "test-agent"
max_memory_usage_mb = 1024
max_cpu_usage_percent = 50
working_dir = "{}"
timeout_seconds = 60
no_fork = true

[git]
branch_prefix = "test-"
username = "test-user"
email = "test@example.com"

[code_generation]
max_tool_iterations = 5
enable_diff_preview = false

[testing]
timeout_seconds = 30
linting_enabled = false
compilation_check = false
run_unit_tests = false
run_integration_tests = false
performance_benchmarks = false
test_mode = true
early_exit = true

[llm_logging]
enabled = false
"#,
        workspace_dir.to_str().unwrap().replace('\\', "\\\\")
    );

    fs::write(config_path, config_content)?;
    Ok(())
}

#[test]
fn test_self_improvement_loop_with_mock_llm() {
    // Create temporary workspace directory
    let temp_dir = TempDir::new().expect("Failed to create temp workspace directory");
    let workspace_path = temp_dir.path();

    // Initialize git repository in workspace
    init_git_repo(workspace_path).expect("Failed to initialize git repo");

    // Create a simple source file to potentially improve
    let src_dir = workspace_path.join("src");
    fs::create_dir_all(&src_dir).expect("Failed to create src directory");

    fs::write(
        src_dir.join("main.rs"),
        r#"fn main() {
    println!("Hello, world!");
}
"#,
    )
    .expect("Failed to write main.rs");

    // Commit the initial source file
    Command::new("git")
        .args(["add", "src/main.rs"])
        .current_dir(workspace_path)
        .output()
        .expect("Failed to git add");

    Command::new("git")
        .args(["commit", "-m", "Add initial source file"])
        .current_dir(workspace_path)
        .output()
        .expect("Failed to commit source file");

    // Create config file
    let config_dir = TempDir::new().expect("Failed to create temp config directory");
    let config_path = config_dir.path().join("test-config.toml");
    create_test_config(workspace_path, &config_path).expect("Failed to create config file");

    // Run borg improve command with mock LLM
    let output = Command::new(env!("CARGO_BIN_EXE_borg"))
        .env("BORG_USE_MOCK_LLM", "true")
        .env("BORG_TEST_MODE", "true")
        .args(["-c", config_path.to_str().unwrap(), "improve"])
        .output()
        .expect("Failed to execute borg command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let output_text = format!("{}\n{}", stdout, stderr);

    // Verify the command completed (may or may not be successful depending on mock LLM behavior)
    // The important thing is that it ran without crashing
    println!("Output from improve command:\n{}", output_text);

    // Check for expected output indicating the improvement iteration ran
    let has_improvement_output = output_text.contains("improvement iteration")
        || output_text.contains("Starting improvement")
        || output_text.contains("optimization")
        || output_text.contains("goal")
        || output.status.success();

    assert!(
        has_improvement_output,
        "Expected improvement iteration output, got: {}",
        output_text
    );
}

#[test]
fn test_self_improvement_with_optimization_goal() {
    // Create temporary workspace directory
    let temp_dir = TempDir::new().expect("Failed to create temp workspace directory");
    let workspace_path = temp_dir.path();

    // Initialize git repository in workspace
    init_git_repo(workspace_path).expect("Failed to initialize git repo");

    // Create a simple source file with obvious optimization opportunities
    let src_dir = workspace_path.join("src");
    fs::create_dir_all(&src_dir).expect("Failed to create src directory");

    fs::write(
        src_dir.join("lib.rs"),
        r#"// This is a simple library with optimization opportunities

pub fn inefficient_function() -> Vec<i32> {
    let mut result = Vec::new();
    for i in 0..100 {
        result.push(i);
    }
    result
}

pub fn unused_function() {
    // This function is never used
}
"#,
    )
    .expect("Failed to write lib.rs");

    // Commit the source file
    Command::new("git")
        .args(["add", "src/lib.rs"])
        .current_dir(workspace_path)
        .output()
        .expect("Failed to git add");

    Command::new("git")
        .args([
            "commit",
            "-m",
            "Add library with optimization opportunities",
        ])
        .current_dir(workspace_path)
        .output()
        .expect("Failed to commit source file");

    // Create config file
    let config_dir = TempDir::new().expect("Failed to create temp config directory");
    let config_path = config_dir.path().join("test-config.toml");
    create_test_config(workspace_path, &config_path).expect("Failed to create config file");

    // Create a goals file to seed the optimization
    let goals_dir = workspace_path.join(".borg");
    fs::create_dir_all(&goals_dir).expect("Failed to create .borg directory");

    let goals_content = r#"{
  "goals": [
    {
      "id": "test-optimization-1",
      "description": "Improve code efficiency",
      "category": "Performance",
      "priority": "High",
      "status": "Pending",
      "created_at": "2024-01-01T00:00:00Z",
      "updated_at": "2024-01-01T00:00:00Z"
    }
  ]
}"#;

    fs::write(goals_dir.join("optimization_goals.json"), goals_content)
        .expect("Failed to write goals file");

    // Run borg improve command with mock LLM
    let output = Command::new(env!("CARGO_BIN_EXE_borg"))
        .env("BORG_USE_MOCK_LLM", "true")
        .env("BORG_TEST_MODE", "true")
        .args(["-c", config_path.to_str().unwrap(), "improve"])
        .output()
        .expect("Failed to execute borg command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let output_text = format!("{}\n{}", stdout, stderr);

    println!("Output from improve command with goal:\n{}", output_text);

    // Verify the improvement iteration attempted to process the goal
    let has_goal_processing = output_text.contains("goal")
        || output_text.contains("optimization")
        || output_text.contains("improvement")
        || output.status.success();

    assert!(
        has_goal_processing,
        "Expected goal processing output, got: {}",
        output_text
    );

    // Check if goals file was updated (it should exist even if not modified)
    let goals_file_exists = goals_dir.join("optimization_goals.json").exists();
    assert!(
        goals_file_exists,
        "Goals file should exist after improvement iteration"
    );
}

#[test]
fn test_self_improvement_initializes_workspace() {
    // Create temporary workspace directory
    let temp_dir = TempDir::new().expect("Failed to create temp workspace directory");
    let workspace_path = temp_dir.path();

    // Don't initialize git - let borg do it
    // This tests the workspace bootstrap process

    // Create config file
    let config_dir = TempDir::new().expect("Failed to create temp config directory");
    let config_path = config_dir.path().join("test-config.toml");
    create_test_config(workspace_path, &config_path).expect("Failed to create config file");

    // Run borg improve command with mock LLM
    let output = Command::new(env!("CARGO_BIN_EXE_borg"))
        .env("BORG_USE_MOCK_LLM", "true")
        .env("BORG_TEST_MODE", "true")
        .args(["-c", config_path.to_str().unwrap(), "improve"])
        .output()
        .expect("Failed to execute borg command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let output_text = format!("{}\n{}", stdout, stderr);

    println!(
        "Output from workspace initialization test:\n{}",
        output_text
    );

    // Verify that git repository was initialized
    let git_dir = workspace_path.join(".git");
    assert!(
        git_dir.exists(),
        "Git repository should be initialized in workspace"
    );

    // Verify HEAD exists (initial commit was made)
    let head_file = git_dir.join("HEAD");
    assert!(
        head_file.exists(),
        "Git HEAD should exist after initialization"
    );

    // The command may or may not succeed depending on the state,
    // but it should have at least attempted initialization
    let has_initialization = output_text.contains("initializ")
        || output_text.contains("Starting")
        || output_text.contains("Borg")
        || git_dir.exists();

    assert!(
        has_initialization,
        "Expected workspace initialization, got: {}",
        output_text
    );
}

#[test]
fn test_self_improvement_info_command() {
    // Create temporary workspace directory
    let temp_dir = TempDir::new().expect("Failed to create temp workspace directory");
    let workspace_path = temp_dir.path();

    // Initialize git repository
    init_git_repo(workspace_path).expect("Failed to initialize git repo");

    // Create config file
    let config_dir = TempDir::new().expect("Failed to create temp config directory");
    let config_path = config_dir.path().join("test-config.toml");
    create_test_config(workspace_path, &config_path).expect("Failed to create config file");

    // Run borg info command
    let output = Command::new(env!("CARGO_BIN_EXE_borg"))
        .env("BORG_USE_MOCK_LLM", "true")
        .env("BORG_TEST_MODE", "true")
        .args(["-c", config_path.to_str().unwrap(), "info"])
        .output()
        .expect("Failed to execute borg command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let output_text = format!("{}\n{}", stdout, stderr);

    // Info command should succeed
    assert!(
        output.status.success(),
        "Info command should succeed, got: {}",
        output_text
    );

    // Verify expected info output
    assert!(
        output_text.contains("Agent Information") || output_text.contains("Version"),
        "Info output should contain agent information"
    );

    // Should show the working directory
    assert!(
        output_text.contains("Working Directory")
            || output_text.contains(workspace_path.to_str().unwrap()),
        "Info output should contain working directory path"
    );
}

#[test]
fn test_self_improvement_with_early_termination() {
    // Create temporary workspace directory
    let temp_dir = TempDir::new().expect("Failed to create temp workspace directory");
    let workspace_path = temp_dir.path();

    // Initialize git repository
    init_git_repo(workspace_path).expect("Failed to initialize git repo");

    // Create config file with max_runtime_seconds to ensure quick termination
    let config_dir = TempDir::new().expect("Failed to create temp config directory");
    let config_path = config_dir.path().join("test-config.toml");

    let config_content = format!(
        r#"
[llm.default]
provider = "mock"
api_key = "test-api-key"
model = "mock-model"

[agent]
name = "test-agent"
max_memory_usage_mb = 1024
max_cpu_usage_percent = 50
working_dir = "{}"
timeout_seconds = 5
max_runtime_seconds = 1
no_fork = true

[git]
branch_prefix = "test-"

[code_generation]
max_tool_iterations = 2

[testing]
test_mode = true
early_exit = true
timeout_seconds = 5
linting_enabled = false
compilation_check = false
run_unit_tests = false
run_integration_tests = false
performance_benchmarks = false

[llm_logging]
enabled = false
"#,
        workspace_path.to_str().unwrap().replace('\\', "\\\\")
    );

    fs::write(&config_path, config_content).expect("Failed to write config file");

    // Run borg improve command - should terminate quickly
    let start = std::time::Instant::now();

    let output = Command::new(env!("CARGO_BIN_EXE_borg"))
        .env("BORG_USE_MOCK_LLM", "true")
        .env("BORG_TEST_MODE", "true")
        .env("BORG_DISABLE_LONG_RUNNING", "true")
        .args(["-c", config_path.to_str().unwrap(), "improve"])
        .output()
        .expect("Failed to execute borg command");

    let duration = start.elapsed();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let output_text = format!("{}\n{}", stdout, stderr);

    println!("Output from early termination test:\n{}", output_text);

    // Should complete relatively quickly (within 60 seconds)
    // Note: Mock LLM has delays and retry logic, so this can take some time
    assert!(
        duration.as_secs() < 60,
        "Command should complete within reasonable time with early termination settings, took {} seconds",
        duration.as_secs()
    );

    // The command should have at least started
    let has_started = output_text.contains("improvement")
        || output_text.contains("Starting")
        || output_text.contains("goal")
        || output.status.success();

    assert!(
        has_started,
        "Expected command to start, got: {}",
        output_text
    );
}
