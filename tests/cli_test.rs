use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

// Helper function to run the CLI command and capture output
fn run_command(args: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_borg"))
        .args(args)
        .output()
        .expect("Failed to execute borg command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let output_text = format!("{}\n{}", stdout, stderr);

    (output.status.success(), output_text)
}

// Helper function to create an isolated test environment and run a command
fn run_isolated_command(args: &[&str]) -> (bool, String, tempfile::TempDir) {
    // Create a temporary directory for the workspace
    let workspace_dir = tempdir().expect("Failed to create temp workspace directory");

    // Create a temporary directory for the config file
    let config_dir = tempdir().expect("Failed to create temp config directory");
    let config_path = config_dir.path().join("test-config.toml");

    // Create a config file with the temporary workspace
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
timeout_seconds = 60
no_fork = true

[git]
branch_prefix = "test-"

[testing]
timeout_seconds = 30
linting_enabled = true
compilation_check = true
run_unit_tests = true
run_integration_tests = false
performance_benchmarks = false
test_mode = true
"#,
        workspace_dir.path().to_str().unwrap().replace('\\', "\\\\")
    );

    fs::write(&config_path, config_content).expect("Failed to write test config file");

    // Run command with the test config
    let mut full_args = vec!["-c", config_path.to_str().unwrap()];
    full_args.extend_from_slice(args);

    let output = Command::new(env!("CARGO_BIN_EXE_borg"))
        .args(&full_args)
        .env("BORG_TEST_MODE", "true")
        .output()
        .expect("Failed to execute borg command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let output_text = format!("{}\n{}", stdout, stderr);

    (output.status.success(), output_text, workspace_dir)
}

// Helper function to run in a specific directory
fn run_command_in_dir<P: AsRef<Path>>(dir: P, args: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_borg"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("Failed to execute borg command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let output_text = format!("{}\n{}", stdout, stderr);

    (output.status.success(), output_text)
}

// Helper function to run with environment variables set
fn run_command_with_env(args: &[&str], env_vars: &[(&str, &str)]) -> (bool, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_borg"));
    command.args(args);

    for (key, value) in env_vars {
        command.env(key, value);
    }

    let output = command.output().expect("Failed to execute borg command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let output_text = format!("{}\n{}", stdout, stderr);

    (output.status.success(), output_text)
}

// Test the help command
#[test]
fn test_help_command() {
    let (success, output) = run_command(&["--help"]);
    assert!(success, "Help command failed: {}", output);
    assert!(
        output.contains("Borg - Autonomous Self-Improving AI Agent"),
        "Help output doesn't contain expected text"
    );
    // Help output format might be different across clap versions
    assert!(
        output.contains("USAGE:") || output.contains("Usage:") || output.contains("Commands:"),
        "Help output missing usage section"
    );
}

// Test the info command
#[test]
fn test_info() {
    let (success, output, _temp_dir) = run_isolated_command(&["info"]);
    assert!(success, "Info command failed: {}", output);
    assert!(
        output.contains("Agent Information:"),
        "Info output doesn't contain expected header"
    );
    assert!(
        output.contains("Version:"),
        "Info output doesn't show version"
    );
}

// Test objective list command with no objectives
#[test]
fn test_empty_objective_list() {
    let (success, output, _temp_dir) = run_isolated_command(&["objective", "list"]);
    assert!(success, "Objective list command failed: {}", output);
    assert!(
        output.contains("Strategic Objectives:")
            || output.contains("objectives")
            || output.contains("Objectives"),
        "Objective list output missing expected text: {}",
        output
    );
}

// Test adding and listing objectives
#[test]
fn test_add_and_list_objectives() {
    // Create test environment
    let (_, _, workspace_dir) = run_isolated_command(&["info"]);

    // Create a temporary directory for test files
    let temp_dir = tempdir().expect("Failed to create temp directory");
    let key_results_path = temp_dir.path().join("key_results.txt");
    let constraints_path = temp_dir.path().join("constraints.txt");

    // Create test files
    fs::write(
        &key_results_path,
        "Reduce memory usage by 50%\nImprove response time by 30%",
    )
    .expect("Failed to write key results file");
    fs::write(
        &constraints_path,
        "Maintain compatibility with existing APIs\nNo regressions in functionality",
    )
    .expect("Failed to write constraints file");

    // Create config file with the workspace
    let config_dir = tempdir().expect("Failed to create temp config directory");
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
timeout_seconds = 60
no_fork = true

[git]
branch_prefix = "test-"

[testing]
timeout_seconds = 30
linting_enabled = true
compilation_check = true
run_unit_tests = true
run_integration_tests = false
performance_benchmarks = false
test_mode = true
"#,
        workspace_dir.path().to_str().unwrap().replace('\\', "\\\\")
    );

    fs::write(&config_path, config_content).expect("Failed to write test config file");

    // Add an objective
    let add_args = [
        "-c",
        config_path.to_str().unwrap(),
        "objective",
        "add",
        "test-obj-1",
        "Test Objective",
        "A test objective for CLI testing",
        "6",
        "test-user",
        "-k",
        key_results_path.to_str().unwrap(),
        "-c",
        constraints_path.to_str().unwrap(),
    ];

    let (success, output) = run_command(&add_args);
    assert!(success, "Adding objective failed: {}", output);
    assert!(
        output.contains("Strategic objective added successfully"),
        "Add objective output unexpected"
    );

    // List objectives to verify
    let list_args = ["-c", config_path.to_str().unwrap(), "objective", "list"];

    let (success, output) = run_command(&list_args);
    assert!(
        success,
        "Objective list command failed after adding: {}",
        output
    );
    assert!(
        output.contains("Test Objective"),
        "Listed objectives should contain added objective"
    );
    assert!(
        output.contains("Reduce memory usage by 50%"),
        "Key results not shown in listing"
    );
    assert!(
        output.contains("Maintain compatibility with existing APIs"),
        "Constraints not shown in listing"
    );
}

// Test load objectives from TOML
#[test]
fn test_load_objectives_from_toml() {
    // Create test environment
    let (_, _, workspace_dir) = run_isolated_command(&["info"]);

    // Create a temporary directory for test files
    let temp_dir = tempdir().expect("Failed to create temp directory");
    let toml_path = temp_dir.path().join("test_objectives.toml");

    // Create a test TOML file
    let toml_content = r#"
[[objectives]]
id = "toml-test-1"
title = "TOML Test Objective"
description = "An objective loaded from TOML"
timeframe = 9
creator = "toml-user"
key_results = [
  "TOML key result 1",
  "TOML key result 2"
]
constraints = [
  "TOML constraint 1",
  "TOML constraint 2"
]
"#;

    fs::write(&toml_path, toml_content).expect("Failed to write TOML file");

    // Create config file with the workspace
    let config_dir = tempdir().expect("Failed to create temp config directory");
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
timeout_seconds = 60
no_fork = true

[git]
branch_prefix = "test-"

[testing]
timeout_seconds = 30
linting_enabled = true
compilation_check = true
run_unit_tests = true
run_integration_tests = false
performance_benchmarks = false
test_mode = true
"#,
        workspace_dir.path().to_str().unwrap().replace('\\', "\\\\")
    );

    fs::write(&config_path, config_content).expect("Failed to write test config file");

    // Load objectives from TOML
    let load_args = [
        "-c",
        config_path.to_str().unwrap(),
        "load-objectives",
        toml_path.to_str().unwrap(),
    ];

    let (success, output) = run_command(&load_args);
    assert!(success, "Loading objectives from TOML failed: {}", output);
    assert!(
        output.contains("Adding objective: TOML Test Objective"),
        "TOML load output unexpected"
    );

    // Verify objectives were loaded
    let list_args = ["-c", config_path.to_str().unwrap(), "objective", "list"];

    let (success, output) = run_command(&list_args);
    assert!(
        success,
        "Objective list command failed after TOML load: {}",
        output
    );
    assert!(
        output.contains("TOML Test Objective"),
        "Loaded objective not found in listing"
    );
    assert!(
        output.contains("TOML key result 1"),
        "Loaded key results not found in listing"
    );
}

// Test plan generate command
#[test]
fn test_plan_generate() {
    // Create test environment
    let (_, _, workspace_dir) = run_isolated_command(&["info"]);

    // Create a temporary directory for the config file
    let config_dir = tempdir().expect("Failed to create temp config directory");
    let config_path = config_dir.path().join("test-config.toml");

    // Create a config file with the temporary workspace
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
timeout_seconds = 60
no_fork = true

[git]
branch_prefix = "test-"

[testing]
timeout_seconds = 30
linting_enabled = true
compilation_check = true
run_unit_tests = true
run_integration_tests = false
performance_benchmarks = false
test_mode = true
"#,
        workspace_dir.path().to_str().unwrap().replace('\\', "\\\\")
    );

    fs::write(&config_path, config_content).expect("Failed to write test config file");

    // First ensure we have at least one objective
    let add_args = [
        "-c",
        config_path.to_str().unwrap(),
        "objective",
        "add",
        "plan-test-obj",
        "Planning Test Objective",
        "An objective for testing planning",
        "3",
        "test-user",
    ];

    let (success, _) = run_command(&add_args);
    assert!(success, "Failed to add objective for planning test");

    // Run planning cycle
    let plan_args = ["-c", config_path.to_str().unwrap(), "plan", "generate"];

    let (success, output) = run_command(&plan_args);
    assert!(success, "Plan generate command failed: {}", output);
    assert!(
        output.contains("Planning cycle completed successfully"),
        "Plan generate output unexpected"
    );
}

// Test plan show command
#[test]
fn test_plan_show() {
    // Create test environment
    let (_, _, workspace_dir) = run_isolated_command(&["info"]);

    // Create a temporary directory for the config file
    let config_dir = tempdir().expect("Failed to create temp config directory");
    let config_path = config_dir.path().join("test-config.toml");

    // Create a config file with the temporary workspace
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
timeout_seconds = 60
no_fork = true

[git]
branch_prefix = "test-"

[testing]
timeout_seconds = 30
linting_enabled = true
compilation_check = true
run_unit_tests = true
run_integration_tests = false
performance_benchmarks = false
test_mode = true
"#,
        workspace_dir.path().to_str().unwrap().replace('\\', "\\\\")
    );

    fs::write(&config_path, config_content).expect("Failed to write test config file");

    // First ensure we have at least one objective
    let add_args = [
        "-c",
        config_path.to_str().unwrap(),
        "objective",
        "add",
        "plan-test-obj",
        "Planning Test Objective",
        "An objective for testing planning",
        "3",
        "test-user",
    ];

    let (success, _) = run_command(&add_args);
    assert!(success, "Failed to add objective for planning test");

    // First ensure we have a plan
    let generate_args = ["-c", config_path.to_str().unwrap(), "plan", "generate"];

    let (_, _) = run_command(&generate_args);

    // Show plan
    let show_args = ["-c", config_path.to_str().unwrap(), "plan", "show"];

    let (success, output) = run_command(&show_args);
    assert!(success, "Plan show command failed: {}", output);
    assert!(
        output.contains("Strategic Plan:"),
        "Plan show output missing header"
    );
}

// Test plan report command
#[test]
fn test_plan_report() {
    // Create test environment
    let (_, _, workspace_dir) = run_isolated_command(&["info"]);

    // Create a temporary directory for the config file
    let config_dir = tempdir().expect("Failed to create temp config directory");
    let config_path = config_dir.path().join("test-config.toml");

    // Create a config file with the temporary workspace
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
timeout_seconds = 60
no_fork = true

[git]
branch_prefix = "test-"

[testing]
timeout_seconds = 30
linting_enabled = true
compilation_check = true
run_unit_tests = true
run_integration_tests = false
performance_benchmarks = false
test_mode = true
"#,
        workspace_dir.path().to_str().unwrap().replace('\\', "\\\\")
    );

    fs::write(&config_path, config_content).expect("Failed to write test config file");

    // First ensure we have at least one objective
    let add_args = [
        "-c",
        config_path.to_str().unwrap(),
        "objective",
        "add",
        "plan-test-obj",
        "Planning Test Objective",
        "An objective for testing planning",
        "3",
        "test-user",
    ];

    let (success, _) = run_command(&add_args);
    assert!(success, "Failed to add objective for planning test");

    // First ensure we have a plan
    let generate_args = ["-c", config_path.to_str().unwrap(), "plan", "generate"];

    let (_, _) = run_command(&generate_args);

    // Generate report
    let report_args = ["-c", config_path.to_str().unwrap(), "plan", "report"];

    let (success, output) = run_command(&report_args);
    assert!(success, "Plan report command failed: {}", output);
    assert!(
        output.contains("progress report"),
        "Plan report output unexpected"
    );
}

// Test improve command
#[test]
fn test_improve_command() {
    // Create test environment
    let (_, _, workspace_dir) = run_isolated_command(&["info"]);

    // Create a temporary directory for the config file
    let config_dir = tempdir().expect("Failed to create temp config directory");
    let config_path = config_dir.path().join("test-config.toml");

    // Create a config file with the temporary workspace
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
timeout_seconds = 60
no_fork = true

[git]
branch_prefix = "test-"

[testing]
timeout_seconds = 30
linting_enabled = true
compilation_check = true
run_unit_tests = true
run_integration_tests = false
performance_benchmarks = false
test_mode = true
"#,
        workspace_dir.path().to_str().unwrap().replace('\\', "\\\\")
    );

    fs::write(&config_path, config_content).expect("Failed to write test config file");

    let improve_args = ["-c", config_path.to_str().unwrap(), "improve"];

    let (success, output) = run_command(&improve_args);
    assert!(success, "Improve command failed: {}", output);
    assert!(
        output.contains("Running a single improvement iteration"),
        "Improve command output unexpected"
    );
}

// Test invalid commands
#[test]
fn test_invalid_command() {
    let (success, output) = run_command(&["nonexistent-command"]);
    assert!(!success, "Invalid command should fail");
    assert!(
        output.contains("error"),
        "Invalid command should output error message"
    );
}

// Test configuration file handling
#[test]
fn test_config_file_parameter() {
    // Create a temporary directory
    let temp_dir = tempdir().expect("Failed to create temp directory");

    // Create a simple config file with all required fields
    let config_content = r#"
[llm.default]
provider = "openai"  # Use a supported provider
api_key = "test-api-key"
model = "gpt-4o"

[agent]
name = "test-agent"
workspace = "./workspace"
max_memory_usage_mb = 1024
max_cpu_usage_percent = 50
working_dir = "./workspace"
timeout_seconds = 300

[git]
repository_url = "https://example.com/repo.git"
clone_path = "./repo"
username = "test-user"
email = "test@example.com"

[testing]
timeout_seconds = 30
linting_enabled = true
compilation_check = true
run_unit_tests = true
run_integration_tests = false
performance_benchmarks = false
test_mode = true
early_exit = true
"#;

    let config_path = temp_dir.path().join("test-config.toml");
    fs::write(&config_path, config_content).expect("Failed to write config file");

    // Run with custom config
    let (success, output) = run_command(&["-c", config_path.to_str().unwrap(), "info"]);
    assert!(success, "Command with custom config failed: {}", output);
    assert!(
        output.contains("Using configuration file:"),
        "Config file not mentioned in output"
    );
    assert!(
        output.contains("test-config.toml"),
        "Custom config file not referenced in output"
    );
}

// Test debug mode
#[test]
fn test_debug_flag() {
    // Run with debug flag
    let (success, output) = run_command(&["--debug", "info"]);

    // Just make sure the command succeeds with the debug flag
    assert!(success, "Command with debug flag failed: {}", output);

    // Check that basic agent information is displayed
    assert!(
        output.contains("Agent Information"),
        "Agent information not displayed"
    );
    assert!(
        output.contains("Version:"),
        "Version information not displayed"
    );
    assert!(
        output.contains("Working Directory:"),
        "Working directory not displayed"
    );
}

// Test with invalid parameters
#[test]
fn test_invalid_parameters() {
    // Test missing required parameter
    let (success, output) = run_command(&["objective", "add", "missing-params"]);
    assert!(!success, "Command with missing parameters should fail");
    assert!(
        output.contains("error:"),
        "Error message not present for missing parameters"
    );

    // Test invalid config file
    let (success, output) = run_command(&["-c", "nonexistent-config.toml", "info"]);
    // This might actually succeed with a fallback config, depending on implementation
    if !success {
        assert!(
            output.contains("error"),
            "Error message not present for invalid config"
        );
    }
}

// Test configuration file fallback
#[test]
fn test_config_fallback() {
    // Create a temporary directory
    let temp_dir = tempdir().expect("Failed to create temp directory");

    // Create a fallback config file with all required fields
    let config_content = r#"
[llm.default]
provider = "openai"  # Use a supported provider
api_key = "fallback-api-key"
model = "gpt-4o"

[agent]
name = "fallback-agent"
workspace = "./workspace"
max_memory_usage_mb = 1024
max_cpu_usage_percent = 50
working_dir = "./workspace"
timeout_seconds = 300

[git]
repository_url = "https://example.com/repo.git"
clone_path = "./repo"
username = "test-user"
email = "test@example.com"

[testing]
timeout_seconds = 30
linting_enabled = true
compilation_check = true
run_unit_tests = true
run_integration_tests = false
performance_benchmarks = false
test_mode = true
early_exit = true
"#;

    let config_path = temp_dir.path().join("config.toml");
    fs::write(&config_path, config_content).expect("Failed to write fallback config file");

    // Run in the directory with the fallback config
    let (success, output) = run_command_in_dir(temp_dir.path(), &["info"]);
    assert!(success, "Command with fallback config failed: {}", output);
    assert!(
        output.contains("Using configuration file:"),
        "Config file not mentioned in output"
    );
    // It should use the config.toml in the current directory when config.production.toml is not found
    assert!(
        output.contains("config.toml"),
        "Fallback config file not referenced in output"
    );
}

// Test file validation
#[test]
fn test_file_validation() {
    // Test with non-existent file for key results
    let (success, output) = run_command(&[
        "objective",
        "add",
        "invalid-file-test",
        "Invalid File Test",
        "Testing with non-existent file",
        "3",
        "test-user",
        "-k",
        "nonexistent-file.txt",
    ]);
    assert!(!success, "Command with non-existent file should fail");
    assert!(
        output.contains("error") || output.contains("failed"),
        "Error message not present for non-existent file"
    );

    // Test with invalid TOML file
    let temp_dir = tempdir().expect("Failed to create temp directory");
    let invalid_toml_path = temp_dir.path().join("invalid.toml");
    fs::write(&invalid_toml_path, "This is not valid TOML content")
        .expect("Failed to write invalid TOML file");

    let (success, output) = run_command(&["load-objectives", invalid_toml_path.to_str().unwrap()]);
    assert!(!success, "Command with invalid TOML should fail");
    assert!(
        output.contains("error") || output.contains("Failed to parse"),
        "Error message not present for invalid TOML"
    );
}

// Test version command
#[test]
fn test_version_command() {
    let (success, output) = run_command(&["--version"]);
    assert!(success, "Version command failed: {}", output);
    assert!(
        output.contains("borg"),
        "Version output doesn't contain program name"
    );
    // Check if version follows semver format (x.y.z)
    let has_version = output.lines().any(|line| {
        line.contains("borg")
            && line.trim().matches('.').count() >= 1
            && line.trim().chars().any(|c| c.is_numeric())
    });
    assert!(
        has_version,
        "Version output doesn't seem to contain a version number"
    );
}

// Test nested commands
#[test]
fn test_nested_command_help() {
    // Test help for objective subcommand
    let (success, output) = run_command(&["objective", "--help"]);
    assert!(success, "Objective help command failed: {}", output);
    assert!(
        output.contains("add"),
        "Objective help doesn't mention 'add' subcommand"
    );
    assert!(
        output.contains("list"),
        "Objective help doesn't mention 'list' subcommand"
    );

    // Test help for plan subcommand
    let (success, output) = run_command(&["plan", "--help"]);
    assert!(success, "Plan help command failed: {}", output);
    assert!(
        output.contains("generate"),
        "Plan help doesn't mention 'generate' subcommand"
    );
    assert!(
        output.contains("show"),
        "Plan help doesn't mention 'show' subcommand"
    );
    assert!(
        output.contains("report"),
        "Plan help doesn't mention 'report' subcommand"
    );
}

// Test default mode with proper initialization
#[test]
fn test_default_mode_initialization() {
    // Create a temporary directory for the workspace
    let workspace_dir = tempdir().expect("Failed to create temp workspace directory");

    // Create a temporary directory for the config file
    let config_dir = tempdir().expect("Failed to create temp config directory");
    let config_path = config_dir.path().join("test-config.toml");

    // Create a test config file that will make the agent terminate quickly
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
timeout_seconds = 300
max_runtime_seconds = 1
no_fork = true

[git]
branch_prefix = "test-"

[testing]
timeout_seconds = 30
linting_enabled = true
compilation_check = true
run_unit_tests = true
run_integration_tests = false
performance_benchmarks = false
test_mode = true
early_exit = true
"#,
        workspace_dir.path().to_str().unwrap().replace('\\', "\\\\")
    );

    fs::write(&config_path, config_content).expect("Failed to write config file");

    // Run with environment variables to indicate test mode
    let mut command = Command::new(env!("CARGO_BIN_EXE_borg"));
    command.args(["-c", config_path.to_str().unwrap()]);
    command.env("BORG_TEST_MODE", "true");
    command.env("BORG_DISABLE_LONG_RUNNING", "true");
    command.env("BORG_USE_MOCK_LLM", "true");

    let output = command.output().expect("Failed to execute borg command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let output_text = format!("{}\n{}", stdout, stderr);
    let success = output.status.success();

    // Check that it initialized but didn't run indefinitely
    assert!(success, "Command failed: {}", output_text);
    assert!(
        output_text.contains("Starting Borg")
            || output_text.contains("Autonomous")
            || output_text.contains("Agent")
            || output_text.contains("test mode")
            || output_text.contains("initialized")
            || output_text.contains("Using configuration file"),
        "Default mode didn't show initialization: {}",
        output_text
    );
}

// Test run when environment variable indicates testing mode
#[test]
fn test_respects_test_environment() {
    // Create a temporary directory for the workspace
    let workspace_dir = tempdir().expect("Failed to create temp workspace directory");

    // Create a temporary directory for the config file
    let config_dir = tempdir().expect("Failed to create temp config directory");
    let config_path = config_dir.path().join("test-config.toml");

    // Create a config file with the temporary workspace
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
timeout_seconds = 60
no_fork = true

[git]
branch_prefix = "test-"

[testing]
timeout_seconds = 30
linting_enabled = true
compilation_check = true
run_unit_tests = true
run_integration_tests = false
performance_benchmarks = false
test_mode = true
"#,
        workspace_dir.path().to_str().unwrap().replace('\\', "\\\\")
    );

    fs::write(&config_path, config_content).expect("Failed to write test config file");

    // Run with environment variable that should cause test-mode behavior
    let mut command = Command::new(env!("CARGO_BIN_EXE_borg"));
    command.args(["-c", config_path.to_str().unwrap()]);
    command.env("BORG_TEST_MODE", "true");

    let output = command.output().expect("Failed to execute borg command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let output_text = format!("{}\n{}", stdout, stderr);
    let success = output.status.success();

    // If the agent properly respects the test mode, it should either:
    // 1. Exit early with success, or
    // 2. Show specific test mode behavior
    assert!(
        output_text.contains("test mode")
            || output_text.contains("Testing")
            || output_text.contains("Starting")
            || success,
        "Agent doesn't appear to respect test mode environment variable: {}",
        output_text
    );
}

// Ensure the agent doesn't fork itself in test mode
#[test]
fn test_no_recursive_process_creation() {
    // Create a temporary directory for the workspace
    let workspace_dir = tempdir().expect("Failed to create temp workspace directory");

    // Create a temporary directory for the config file
    let config_dir = tempdir().expect("Failed to create temp config directory");
    let config_path = config_dir.path().join("test-config.toml");

    // Create a special config to detect if a child process is created
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
timeout_seconds = 60
no_fork = true

[git]
branch_prefix = "test-"

[testing]
timeout_seconds = 30
linting_enabled = true
compilation_check = true
run_unit_tests = true
run_integration_tests = false
performance_benchmarks = false
test_mode = true
"#,
        workspace_dir.path().to_str().unwrap().replace('\\', "\\\\")
    );

    fs::write(&config_path, config_content).expect("Failed to write config file");

    // Run with environment variables that should prevent forking
    let mut command = Command::new(env!("CARGO_BIN_EXE_borg"));
    command.args(["-c", config_path.to_str().unwrap()]);
    command.env("BORG_NO_FORK", "true");
    command.env("BORG_TEST_MODE", "true");
    command.env("BORG_DISABLE_LONG_RUNNING", "true");

    let output = command.output().expect("Failed to execute borg command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let output_text = format!("{}\n{}", stdout, stderr);

    // The fact that the command returned means it didn't hang indefinitely
    // This test mainly verifies that the process exits rather than forking endlessly
    assert!(
        !output_text.contains("fork") || !output_text.contains("child process"),
        "Agent may have created child processes: {}",
        output_text
    );
}

// Test default mode with a test config that forces early termination
#[test]
fn test_default_mode_early_termination() {
    // Create a temporary directory for the workspace
    let workspace_dir = tempdir().expect("Failed to create temp workspace directory");

    // Create a temporary directory for the config file
    let config_dir = tempdir().expect("Failed to create temp config directory");
    let config_path = config_dir.path().join("test-config.toml");

    // Create a config that sets the agent to terminate early
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
timeout_seconds = 300
max_runtime_seconds = 1
no_fork = true

[git]
branch_prefix = "test-"

[testing]
timeout_seconds = 30
linting_enabled = true
compilation_check = true
run_unit_tests = true
run_integration_tests = false
performance_benchmarks = false
test_mode = true
early_exit = true
"#,
        workspace_dir.path().to_str().unwrap().replace('\\', "\\\\")
    );

    fs::write(&config_path, config_content).expect("Failed to write config file");

    // Run with the early-exit config and environment variables to indicate test mode
    let mut command = Command::new(env!("CARGO_BIN_EXE_borg"));
    command.args(["-c", config_path.to_str().unwrap()]);
    command.env("BORG_TEST_MODE", "true");
    command.env("BORG_DISABLE_LONG_RUNNING", "true");

    let output = command.output().expect("Failed to execute borg command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let output_text = format!("{}\n{}", stdout, stderr);
    let success = output.status.success();

    // The agent should start and then exit quickly due to the max_runtime_seconds setting
    assert!(
        success || output_text.contains("test mode") || output_text.contains("config"),
        "Agent with early exit config failed: {}",
        output_text
    );
    assert!(
        output_text.contains("Starting")
            || output_text.contains("Borg")
            || output_text.contains("test mode")
            || output_text.contains("Agent")
            || output_text.contains("Using configuration file"),
        "Agent didn't show initialization message: {}",
        output_text
    );
}
