use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Top-level configuration structure
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Model configurations
    pub models: Vec<ModelConfig>,

    /// Phase configurations for TDD workflow
    pub phases: PhasesConfig,

    /// Agent configuration
    pub agent: AgentConfig,

    /// Database configuration
    pub database: DatabaseConfig,

    /// Git configuration
    pub git: GitConfig,

    /// Logging configuration
    pub logging: LoggingConfig,
}

/// Model configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    /// Unique name for this model configuration
    pub name: String,

    /// Provider name (anthropic | openai | openrouter | google | ollama)
    pub provider: String,

    /// API key for the provider (optional for ollama)
    pub api_key: Option<String>,

    /// Model name to use
    pub model: String,

    /// Maximum tokens to generate
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,

    /// Temperature setting for generation
    #[serde(default = "default_temperature")]
    pub temperature: f32,

    /// Optional base URL override for provider
    #[serde(default)]
    pub api_base: Option<String>,

    /// Enable provider/model-specific "thinking/reasoning" traces where supported
    #[serde(default)]
    pub enable_thinking: Option<bool>,

    /// Hint for provider-specific reasoning effort levels
    #[serde(default)]
    pub reasoning_effort: Option<ReasoningEffort>,

    /// Provider-specific budget for thinking/reasoning tokens where supported
    #[serde(default)]
    pub reasoning_budget_tokens: Option<usize>,
}

/// Phase configuration for TDD workflow
#[derive(Debug, Clone, Deserialize)]
pub struct PhaseConfig {
    /// References to ModelConfig.name (models to use for this phase)
    pub models: Vec<String>,

    /// Tools available to agents in this phase (e.g., ["Read", "Grep"])
    /// Empty list means no tools (pure LLM reasoning)
    #[serde(default)]
    pub tools: Vec<String>,

    /// Prompt to use for this phase
    pub prompt: String,
}

/// Phases configuration
#[derive(Debug, Clone, Deserialize)]
pub struct PhasesConfig {
    /// Research phase configuration
    pub research: PhaseConfig,

    /// Deliberation phase configuration
    pub deliberation: PhaseConfig,

    /// TDD phase configuration
    pub tdd: PhaseConfig,
}

/// Agent configuration
#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    /// Working directory for the agent
    pub working_dir: String,

    /// Timeout for operations in seconds
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,

    /// Maximum memory usage in MB
    #[serde(default = "default_max_memory_mb")]
    pub max_memory_usage_mb: u64,

    /// Maximum CPU usage percent
    #[serde(default = "default_max_cpu_percent")]
    pub max_cpu_usage_percent: u64,
}

fn default_timeout_seconds() -> u64 {
    120
}

fn default_max_memory_mb() -> u64 {
    4096
}

fn default_max_cpu_percent() -> u64 {
    80
}

/// Database configuration
#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    /// Path to the database file
    pub path: String,
}

/// Git configuration
#[derive(Debug, Clone, Deserialize)]
pub struct GitConfig {
    /// Branch naming convention prefix
    pub branch_prefix: String,
}

/// Logging configuration
#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    /// Whether LLM logging is enabled
    #[serde(default = "default_logging_enabled")]
    pub enabled: bool,

    /// Directory for LLM log files
    pub llm_log_dir: String,
}

fn default_logging_enabled() -> bool {
    true
}

/// Reasoning effort levels for provider-specific reasoning (OpenRouter unified interface)
/// See: https://openrouter.ai/docs/guides/best-practices/reasoning-tokens
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    /// No reasoning (disables reasoning entirely)
    None,
    /// Minimal reasoning effort (~10% of max_tokens)
    Minimal,
    /// Low reasoning effort (~20% of max_tokens)
    Low,
    /// Medium reasoning effort (~50% of max_tokens)
    Medium,
    /// High reasoning effort (~80% of max_tokens)
    High,
}

// =====================
// Legacy compatibility types for existing code
// =====================

/// Code generation configuration (legacy compatibility)
#[derive(Debug, Deserialize, Clone, Default)]
pub struct CodeGenerationConfig {
    /// Maximum number of tool iterations
    #[serde(default = "default_max_tool_iterations")]
    pub max_tool_iterations: usize,

    /// Whether to use tools for code generation
    #[serde(default = "default_use_tools")]
    pub use_tools: bool,
}

fn default_max_tool_iterations() -> usize {
    25
}

fn default_use_tools() -> bool {
    true
}

/// LLM provider configuration (legacy compatibility)
/// Used by existing code that hasn't been migrated to ModelConfig
#[derive(Debug, Deserialize, Clone)]
pub struct LlmConfig {
    /// Provider name (e.g., "openai", "anthropic", "local", "openrouter")
    pub provider: String,

    /// API key for the provider
    pub api_key: String,

    /// Model name to use
    pub model: String,

    /// Maximum tokens to generate
    pub max_tokens: usize,

    /// Temperature setting for generation
    pub temperature: f32,

    /// Optional base URL override for provider
    pub api_base: Option<String>,

    /// Optional static HTTP headers to send on every request
    pub headers: Option<std::collections::HashMap<String, String>>,

    /// Enable streaming responses when supported
    pub enable_streaming: Option<bool>,

    /// Enable provider/model-specific "thinking/reasoning" traces where supported
    pub enable_thinking: Option<bool>,

    /// Hint for provider-specific reasoning effort levels
    pub reasoning_effort: Option<ReasoningEffort>,

    /// Provider-specific budget for thinking/reasoning tokens where supported
    pub reasoning_budget_tokens: Option<usize>,

    /// Max wait time for the first streaming token before timing out
    pub first_token_timeout_ms: Option<u64>,

    /// Max idle gap between streaming tokens before timing out
    pub stall_timeout_ms: Option<u64>,
}

/// LLM logging configuration (legacy compatibility)
#[derive(Debug, Deserialize, Clone, Default)]
pub struct LlmLoggingConfig {
    /// Whether logging is enabled
    pub enabled: bool,

    /// Directory for log files
    pub log_dir: String,

    /// Whether to log to console as well
    pub console_logging: bool,

    /// Whether to include full prompts in logs
    pub include_full_prompts: bool,

    /// Whether to include full responses in logs
    pub include_full_responses: bool,

    /// Maximum log file size in MB before rotation
    pub max_log_size_mb: u64,

    /// Number of log files to keep
    pub log_files_to_keep: u64,
}

// Default values for optional configuration
fn default_max_tokens() -> usize {
    16384
}

fn default_temperature() -> f32 {
    0.7
}

impl Config {
    /// Load configuration from a YAML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let config_text = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {:?}", path.as_ref()))?;

        // Expand environment variables
        let expanded_text = expand_env_vars(&config_text)?;

        let config: Config = serde_yaml::from_str(&expanded_text)
            .with_context(|| format!("Failed to parse YAML config file: {:?}", path.as_ref()))?;

        // Validate the configuration
        config.validate()?;

        Ok(config)
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        // Check that at least one model is configured
        if self.models.is_empty() {
            bail!("Configuration must have at least one model defined");
        }

        // Build a set of all model names for quick lookup
        let model_names: HashSet<String> = self.models.iter().map(|m| m.name.clone()).collect();

        // Validate phase model references
        self.validate_phase_models("research", &self.phases.research.models, &model_names)?;
        self.validate_phase_models(
            "deliberation",
            &self.phases.deliberation.models,
            &model_names,
        )?;
        self.validate_phase_models("tdd", &self.phases.tdd.models, &model_names)?;

        // Validate phase tool references
        self.validate_phase_tools("research", &self.phases.research.tools)?;
        self.validate_phase_tools("deliberation", &self.phases.deliberation.tools)?;
        self.validate_phase_tools("tdd", &self.phases.tdd.tools)?;

        // Validate that model names are unique
        let mut seen_names = HashSet::new();
        for model in &self.models {
            if !seen_names.insert(&model.name) {
                bail!("Duplicate model name found: '{}'", model.name);
            }
        }

        // Validate provider names
        for model in &self.models {
            match model.provider.as_str() {
                "anthropic" | "openai" | "openrouter" | "google" | "ollama" => {},
                _ => bail!("Invalid provider '{}' for model '{}'. Valid providers: anthropic, openai, openrouter, google, ollama",
                          model.provider, model.name),
            }
        }

        // Validate that non-ollama models have API keys
        for model in &self.models {
            if model.provider != "ollama" && model.api_key.is_none() {
                bail!(
                    "Model '{}' with provider '{}' must have an api_key configured",
                    model.name,
                    model.provider
                );
            }
        }

        Ok(())
    }

    /// Validate that all phase model references exist
    fn validate_phase_models(
        &self,
        phase_name: &str,
        models: &[String],
        valid_names: &HashSet<String>,
    ) -> Result<()> {
        if models.is_empty() {
            bail!(
                "Phase '{}' must have at least one model configured",
                phase_name
            );
        }

        for model_name in models {
            if !valid_names.contains(model_name) {
                bail!(
                    "Phase '{}' references unknown model '{}'. Available models: {}",
                    phase_name,
                    model_name,
                    valid_names.iter().cloned().collect::<Vec<_>>().join(", ")
                );
            }
        }

        Ok(())
    }

    /// Valid tool names that can be used in phase configurations
    pub const VALID_TOOLS: &'static [&'static str] = &[
        // File operations
        "Read",
        "Write",
        "Edit",
        // Execution
        "Bash",
        // Search
        "Grep",
        "Glob",
        // Web
        "WebSearch",
        "WebFetch",
        // Agent (main agent only)
        "Task",
        // Task management
        "TodoWrite",
    ];

    /// Validate that all phase tool references are valid
    fn validate_phase_tools(&self, phase_name: &str, tools: &[String]) -> Result<()> {
        for tool_name in tools {
            if !Self::VALID_TOOLS.contains(&tool_name.as_str()) {
                bail!(
                    "Phase '{}' references unknown tool '{}'. Available tools: {}",
                    phase_name,
                    tool_name,
                    Self::VALID_TOOLS.join(", ")
                );
            }
        }
        Ok(())
    }

    /// Get a model configuration by name
    pub fn get_model(&self, name: &str) -> Option<&ModelConfig> {
        self.models.iter().find(|m| m.name == name)
    }

    /// Create a new config with default values for testing
    #[cfg(test)]
    pub fn for_testing() -> Self {
        Self {
            models: vec![ModelConfig {
                name: "test-model".to_string(),
                provider: "anthropic".to_string(),
                api_key: Some("test-key".to_string()),
                model: "claude-3-5-sonnet-20241022".to_string(),
                max_tokens: default_max_tokens(),
                temperature: default_temperature(),
                api_base: None,
                enable_thinking: None,
                reasoning_effort: None,
                reasoning_budget_tokens: None,
            }],
            phases: PhasesConfig {
                research: PhaseConfig {
                    models: vec!["test-model".to_string()],
                    tools: vec!["Read".to_string(), "Grep".to_string()],
                    prompt: "Research phase prompt".to_string(),
                },
                deliberation: PhaseConfig {
                    models: vec!["test-model".to_string()],
                    tools: vec![], // Pure reasoning
                    prompt: "Deliberation phase prompt".to_string(),
                },
                tdd: PhaseConfig {
                    models: vec!["test-model".to_string()],
                    tools: vec!["Read".to_string(), "Write".to_string(), "Edit".to_string()],
                    prompt: "TDD phase prompt".to_string(),
                },
            },
            agent: AgentConfig {
                working_dir: "./workspace".to_string(),
                timeout_seconds: 60,
                max_memory_usage_mb: 4096,
                max_cpu_usage_percent: 80,
            },
            database: DatabaseConfig {
                path: "./data/borg.db".to_string(),
            },
            git: GitConfig {
                branch_prefix: "borg/improvement/".to_string(),
            },
            logging: LoggingConfig {
                enabled: true,
                llm_log_dir: "./logs/llm".to_string(),
            },
        }
    }
}

/// Expand environment variables in the configuration text
/// Supports ${VAR} and ${VAR:-default} syntax
fn expand_env_vars(text: &str) -> Result<String> {
    use regex::Regex;

    // Match ${VAR} or ${VAR:-default}
    let re = Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)(:-([^}]*))?\}").unwrap();

    let mut result = text.to_string();
    let mut errors = Vec::new();

    // Process all matches
    for cap in re.captures_iter(text) {
        let full_match = cap.get(0).unwrap().as_str();
        let var_name = cap.get(1).unwrap().as_str();
        let default_value = cap.get(3).map(|m| m.as_str());

        match std::env::var(var_name) {
            Ok(value) => {
                result = result.replace(full_match, &value);
            }
            Err(_) => {
                if let Some(default) = default_value {
                    result = result.replace(full_match, default);
                } else {
                    errors.push(format!(
                        "Environment variable '{}' not found and no default provided",
                        var_name
                    ));
                }
            }
        }
    }

    if !errors.is_empty() {
        bail!(
            "Failed to expand environment variables:\n{}",
            errors.join("\n")
        );
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_env_vars_with_value() {
        std::env::set_var("TEST_VAR", "test_value");
        let result = expand_env_vars("key: ${TEST_VAR}").unwrap();
        assert_eq!(result, "key: test_value");
        std::env::remove_var("TEST_VAR");
    }

    #[test]
    fn test_expand_env_vars_with_default() {
        std::env::remove_var("MISSING_VAR");
        let result = expand_env_vars("key: ${MISSING_VAR:-default_value}").unwrap();
        assert_eq!(result, "key: default_value");
    }

    #[test]
    fn test_expand_env_vars_missing_no_default() {
        std::env::remove_var("MISSING_VAR");
        let result = expand_env_vars("key: ${MISSING_VAR}");
        assert!(result.is_err());
    }

    #[test]
    fn test_config_validation_no_models() {
        let config = Config {
            models: vec![],
            phases: PhasesConfig {
                research: PhaseConfig {
                    models: vec![],
                    tools: vec![],
                    prompt: "test".to_string(),
                },
                deliberation: PhaseConfig {
                    models: vec![],
                    tools: vec![],
                    prompt: "test".to_string(),
                },
                tdd: PhaseConfig {
                    models: vec![],
                    tools: vec![],
                    prompt: "test".to_string(),
                },
            },
            agent: AgentConfig {
                working_dir: "./workspace".to_string(),
                timeout_seconds: 60,
                max_memory_usage_mb: 4096,
                max_cpu_usage_percent: 80,
            },
            database: DatabaseConfig {
                path: "./data/borg.db".to_string(),
            },
            git: GitConfig {
                branch_prefix: "borg/".to_string(),
            },
            logging: LoggingConfig {
                enabled: true,
                llm_log_dir: "./logs".to_string(),
            },
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_invalid_model_reference() {
        let config = Config {
            models: vec![ModelConfig {
                name: "model1".to_string(),
                provider: "anthropic".to_string(),
                api_key: Some("key".to_string()),
                model: "claude-3-5-sonnet-20241022".to_string(),
                max_tokens: 1000,
                temperature: 0.7,
                api_base: None,
                enable_thinking: None,
                reasoning_effort: None,
                reasoning_budget_tokens: None,
            }],
            phases: PhasesConfig {
                research: PhaseConfig {
                    models: vec!["nonexistent".to_string()],
                    tools: vec![],
                    prompt: "test".to_string(),
                },
                deliberation: PhaseConfig {
                    models: vec!["model1".to_string()],
                    tools: vec![],
                    prompt: "test".to_string(),
                },
                tdd: PhaseConfig {
                    models: vec!["model1".to_string()],
                    tools: vec![],
                    prompt: "test".to_string(),
                },
            },
            agent: AgentConfig {
                working_dir: "./workspace".to_string(),
                timeout_seconds: 60,
                max_memory_usage_mb: 4096,
                max_cpu_usage_percent: 80,
            },
            database: DatabaseConfig {
                path: "./data/borg.db".to_string(),
            },
            git: GitConfig {
                branch_prefix: "borg/".to_string(),
            },
            logging: LoggingConfig {
                enabled: true,
                llm_log_dir: "./logs".to_string(),
            },
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_for_testing() {
        let config = Config::for_testing();
        assert!(config.validate().is_ok());
        assert_eq!(config.models.len(), 1);
        assert_eq!(config.models[0].name, "test-model");
    }
}
