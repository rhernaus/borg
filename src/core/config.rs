use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;

/// Top-level configuration structure
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    /// LLM configuration
    pub llm: LlmConfig,

    /// Agent configuration
    pub agent: AgentConfig,

    /// Git configuration
    pub git: Option<GitConfig>,
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
            llm: LlmConfig {
                provider: "mock".to_string(),
                api_key: "test-key".to_string(),
                model: "test-model".to_string(),
                max_tokens: default_max_tokens(),
                temperature: default_temperature(),
            },
            agent: AgentConfig {
                max_memory_usage_mb: 1024,
                max_cpu_usage_percent: 50,
                working_dir: default_working_dir(),
                timeout_seconds: default_timeout(),
            },
            git: Some(GitConfig {
                repo_url: None,
                username: None,
                token: None,
                branch_prefix: default_branch_prefix(),
            }),
        }
    }
}