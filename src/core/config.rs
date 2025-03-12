use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::collections::HashMap;

/// Top-level configuration structure
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    /// LLM configurations
    pub llm: HashMap<String, LlmConfig>,

    /// Agent configuration
    pub agent: AgentConfig,

    /// Git configuration
    pub git: Option<GitConfig>,

    /// Testing configuration
    pub testing: TestingConfig,

    /// Code generation configuration
    #[serde(default)]
    pub code_generation: CodeGenerationConfig,

    /// LLM logging configuration
    #[serde(default)]
    pub llm_logging: LlmLoggingConfig,
}

/// LLM provider configuration
#[derive(Debug, Deserialize, Clone)]
pub struct LlmConfig {
    /// Provider name (e.g., "openai", "anthropic", "local")
    pub provider: String,

    /// API key for the provider
    pub api_key: String,

    /// Model name to use
    pub model: String,

    /// Maximum tokens to generate
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,

    /// Temperature setting for generation
    #[serde(default = "default_temperature")]
    pub temperature: f32,
}

/// Agent configuration
#[derive(Debug, Deserialize, Clone)]
pub struct AgentConfig {
    /// Maximum memory usage in MB
    pub max_memory_usage_mb: usize,

    /// Maximum CPU usage percentage
    pub max_cpu_usage_percent: u8,

    /// Working directory for the agent
    #[serde(default = "default_working_dir")]
    pub working_dir: String,

    /// Timeout for operations in seconds
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,

    /// Maximum runtime in seconds for autonomous mode
    /// If set, the agent will exit after this many seconds
    #[serde(default)]
    pub max_runtime_seconds: Option<u32>,

    /// Whether to disable process forking
    #[serde(default)]
    pub no_fork: bool,
}

/// Git configuration
#[derive(Debug, Deserialize, Clone)]
pub struct GitConfig {
    /// Repository URL
    pub repo_url: Option<String>,

    /// Username for authentication
    pub username: Option<String>,

    /// Password or token for authentication
    pub token: Option<String>,

    /// Branch naming convention
    #[serde(default = "default_branch_prefix")]
    pub branch_prefix: String,
}

/// Testing configuration
#[derive(Debug, Deserialize, Clone)]
pub struct TestingConfig {
    /// Whether to run linting checks
    #[serde(default = "default_linting_enabled")]
    pub linting_enabled: bool,

    /// Whether to check that code compiles
    #[serde(default = "default_compilation_check")]
    pub compilation_check: bool,

    /// Whether to run unit tests
    #[serde(default = "default_run_unit_tests")]
    pub run_unit_tests: bool,

    /// Whether to run integration tests
    #[serde(default = "default_run_integration_tests")]
    pub run_integration_tests: bool,

    /// Whether to run performance benchmarks
    #[serde(default = "default_performance_benchmarks")]
    pub performance_benchmarks: bool,

    /// Timeout for test execution in seconds
    #[serde(default = "default_test_timeout")]
    pub timeout_seconds: u64,

    /// Whether the agent is running in test mode
    #[serde(default)]
    pub test_mode: bool,

    /// Whether to exit early in autonomous mode
    #[serde(default)]
    pub early_exit: bool,

    /// Whether to run formatting checks (rustfmt)
    #[serde(default = "default_run_formatting")]
    pub run_formatting: bool,

    /// Whether to run linting (clippy)
    #[serde(default = "default_run_linting")]
    pub run_linting: bool,

    /// Whether to verify compilation
    #[serde(default = "default_run_compilation")]
    pub run_compilation: bool,

    /// Whether to run doc tests
    #[serde(default = "default_run_doc_tests")]
    pub run_doc_tests: bool,

    /// Whether to run benchmarks
    #[serde(default = "default_run_benchmarks")]
    pub run_benchmarks: bool,
}

/// Code generation configuration
#[derive(Debug, Deserialize, Clone)]
pub struct CodeGenerationConfig {
    /// Maximum number of tool iterations for code generation
    #[serde(default = "default_max_tool_iterations")]
    pub max_tool_iterations: usize,

    /// Whether to use tools for code generation
    #[serde(default = "default_use_tools")]
    pub use_tools: bool,
}

/// LLM logging configuration
#[derive(Debug, Deserialize, Clone, Default)]
pub struct LlmLoggingConfig {
    /// Whether logging is enabled
    #[serde(default = "default_logging_enabled")]
    pub enabled: bool,

    /// Directory for log files
    #[serde(default = "default_log_dir")]
    pub log_dir: String,

    /// Whether to log to console as well
    #[serde(default = "default_console_logging")]
    pub console_logging: bool,

    /// Whether to include full prompts in logs (could be very large)
    #[serde(default = "default_include_full_prompts")]
    pub include_full_prompts: bool,

    /// Whether to include full responses in logs (could be very large)
    #[serde(default = "default_include_full_responses")]
    pub include_full_responses: bool,

    /// Maximum log file size in MB before rotation
    #[serde(default = "default_max_log_size_mb")]
    pub max_log_size_mb: u64,

    /// Number of log files to keep
    #[serde(default = "default_log_files_to_keep")]
    pub log_files_to_keep: u64,
}

// Default values for optional configuration
fn default_max_tokens() -> usize {
    1024
}

fn default_temperature() -> f32 {
    0.7
}

fn default_working_dir() -> String {
    "./workspace".to_string()
}

fn default_timeout() -> u64 {
    60
}

fn default_branch_prefix() -> String {
    "borg/improvement/".to_string()
}

// Default values for testing configuration
fn default_linting_enabled() -> bool {
    true
}

fn default_compilation_check() -> bool {
    true
}

fn default_run_unit_tests() -> bool {
    true
}

fn default_run_integration_tests() -> bool {
    false
}

fn default_performance_benchmarks() -> bool {
    false
}

fn default_test_timeout() -> u64 {
    300
}

fn default_run_formatting() -> bool {
    true
}

fn default_run_linting() -> bool {
    true
}

fn default_run_compilation() -> bool {
    true
}

fn default_run_doc_tests() -> bool {
    false
}

fn default_run_benchmarks() -> bool {
    false
}

/// Default value for max tool iterations
fn default_max_tool_iterations() -> usize {
    25
}

/// Default value for use tools
fn default_use_tools() -> bool {
    true
}

// Default values for LLM logging configuration
fn default_logging_enabled() -> bool {
    true
}

fn default_log_dir() -> String {
    "./logs/llm".to_string()
}

fn default_console_logging() -> bool {
    true
}

fn default_include_full_prompts() -> bool {
    true
}

fn default_include_full_responses() -> bool {
    true
}

fn default_max_log_size_mb() -> u64 {
    100
}

fn default_log_files_to_keep() -> u64 {
    10
}

impl Config {
    /// Load configuration from a file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let config_text = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {:?}", path.as_ref()))?;

        let config: Config = toml::from_str(&config_text)
            .with_context(|| format!("Failed to parse config file: {:?}", path.as_ref()))?;

        Ok(config)
    }

    /// Create a new config with default values for testing
    #[cfg(test)]
    pub fn for_testing() -> Self {
        Self {
            llm: HashMap::from([
                ("mock".to_string(), LlmConfig {
                    provider: "mock".to_string(),
                    api_key: "test-key".to_string(),
                    model: "test-model".to_string(),
                    max_tokens: default_max_tokens(),
                    temperature: default_temperature(),
                }),
            ]),
            agent: AgentConfig {
                max_memory_usage_mb: 1024,
                max_cpu_usage_percent: 50,
                working_dir: default_working_dir(),
                timeout_seconds: default_timeout(),
                max_runtime_seconds: None,
                no_fork: false,
            },
            git: Some(GitConfig {
                repo_url: None,
                username: None,
                token: None,
                branch_prefix: default_branch_prefix(),
            }),
            testing: TestingConfig {
                linting_enabled: default_linting_enabled(),
                compilation_check: default_compilation_check(),
                run_unit_tests: default_run_unit_tests(),
                run_integration_tests: default_run_integration_tests(),
                performance_benchmarks: default_performance_benchmarks(),
                timeout_seconds: default_test_timeout(),
                test_mode: false,
                early_exit: false,
                run_formatting: default_run_formatting(),
                run_linting: default_run_linting(),
                run_compilation: default_run_compilation(),
                run_doc_tests: default_run_doc_tests(),
                run_benchmarks: default_run_benchmarks(),
            },
            code_generation: CodeGenerationConfig {
                max_tool_iterations: default_max_tool_iterations(),
                use_tools: default_use_tools(),
            },
            llm_logging: LlmLoggingConfig {
                enabled: default_logging_enabled(),
                log_dir: default_log_dir(),
                console_logging: default_console_logging(),
                include_full_prompts: default_include_full_prompts(),
                include_full_responses: default_include_full_responses(),
                max_log_size_mb: default_max_log_size_mb(),
                log_files_to_keep: default_log_files_to_keep(),
            },
        }
    }
}

impl Default for TestingConfig {
    fn default() -> Self {
        Self {
            linting_enabled: default_linting_enabled(),
            compilation_check: default_compilation_check(),
            run_unit_tests: default_run_unit_tests(),
            run_integration_tests: default_run_integration_tests(),
            performance_benchmarks: default_performance_benchmarks(),
            timeout_seconds: default_test_timeout(),
            test_mode: false,
            early_exit: false,
            run_formatting: default_run_formatting(),
            run_linting: default_run_linting(),
            run_compilation: default_run_compilation(),
            run_doc_tests: default_run_doc_tests(),
            run_benchmarks: default_run_benchmarks(),
        }
    }
}

impl Default for CodeGenerationConfig {
    fn default() -> Self {
        Self {
            max_tool_iterations: default_max_tool_iterations(),
            use_tools: default_use_tools(),
        }
    }
}