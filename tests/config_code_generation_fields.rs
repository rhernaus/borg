use borg::core::config::Config;

#[test]
fn test_code_generation_config_parsing() {
    let config_toml = r#"
[llm.default]
provider = "mock"
api_key = "test-key"
model = "test-model"

[agent]
max_memory_usage_mb = 1024
max_cpu_usage_percent = 50

[testing]

[code_generation]
candidate_count = 2
max_retries = 5
use_worktrees = false
rating_enabled = true
max_tool_iterations = 10
use_tools = true
"#;

    let config: Config = toml::from_str(config_toml).expect("Failed to parse config");

    // Verify the new fields are parsed correctly
    assert_eq!(config.code_generation.candidate_count, Some(2));
    assert_eq!(config.code_generation.max_retries, Some(5));
    assert_eq!(config.code_generation.use_worktrees, Some(false));
    assert_eq!(config.code_generation.rating_enabled, Some(true));
    assert_eq!(config.code_generation.max_tool_iterations, 10);
    assert!(config.code_generation.use_tools);
}

#[test]
fn test_code_generation_config_defaults() {
    let config_toml = r#"
[llm.default]
provider = "mock"
api_key = "test-key"
model = "test-model"

[agent]
max_memory_usage_mb = 1024
max_cpu_usage_percent = 50

[testing]
"#;

    let config: Config = toml::from_str(config_toml).expect("Failed to parse config");

    // Verify defaults are applied when fields are omitted
    assert_eq!(config.code_generation.candidate_count, Some(1));
    assert_eq!(config.code_generation.max_retries, Some(3));
    assert_eq!(config.code_generation.use_worktrees, Some(true));
    assert_eq!(config.code_generation.rating_enabled, Some(false));
    assert_eq!(config.code_generation.max_tool_iterations, 25);
    assert!(config.code_generation.use_tools);
}
