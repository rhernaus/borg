use anyhow::{Context, Result};
use log::warn;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{OnceLock, RwLock};

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

    /// Strategic planning configuration
    #[serde(default)]
    pub planning: PlanningConfig,

    /// LLM logging configuration
    #[serde(default)]
    pub llm_logging: LlmLoggingConfig,

    /// Provider-level configuration (api_base, headers, global streaming defaults)
    #[serde(default)]
    pub providers: ProvidersConfig,

    /// MongoDB configuration
    #[serde(default)]
    pub mongodb: MongoDbConfig,

    /// Model selection configuration
    #[serde(default)]
    pub model_selection: ModelSelectionConfig,

    /// Modes v2 configuration (feature-flagged; default off)
    #[serde(default)]
    pub modes: ModesConfig,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "lowercase")]
/// Hint for provider-specific reasoning effort levels (safe to ignore if unsupported).
pub enum ReasoningEffort {
    /// Low reasoning effort
    Low,
    /// Medium reasoning effort
    Medium,
    /// High reasoning effort
    High,
}

/// LLM provider configuration
#[derive(Debug, Deserialize, Clone)]
pub struct LlmConfig {
    /// Provider name (e.g., "openai", "anthropic", "local", "openrouter")
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

    /// Optional base URL override for provider. For OpenRouter default is https://openrouter.ai/api/v1
    /// when provider="openrouter" unless explicitly set in config.
    #[serde(default)]
    pub api_base: Option<String>,

    /// Optional static HTTP headers to send on every request (e.g., "HTTP-Referer", "X-Title" for OpenRouter).
    #[serde(default)]
    pub headers: Option<std::collections::HashMap<String, String>>,

    /// Enable streaming responses when supported. If None, Ask CLI will treat as enabled by default;
    /// other flows default to non-streaming unless configured.
    #[serde(default)]
    pub enable_streaming: Option<bool>,

    /// Enable provider/model-specific "thinking/reasoning" traces where supported; ignored for unsupported models/providers.
    #[serde(default)]
    pub enable_thinking: Option<bool>,

    /// Hint for provider-specific reasoning effort levels (safe to ignore if unsupported).
    #[serde(default)]
    pub reasoning_effort: Option<ReasoningEffort>,

    /// Provider-specific budget for thinking/reasoning tokens where supported; ignored if unsupported.
    #[serde(default)]
    pub reasoning_budget_tokens: Option<usize>,

    /// Max wait time for the first streaming token before timing out (idle-timeout).
    /// RFC default: 30000 ms when unspecified by provider usage.
    #[serde(default)]
    pub first_token_timeout_ms: Option<u64>,

    /// Max idle gap between streaming tokens before timing out (idle-timeout).
    /// RFC default: 10000 ms when unspecified by provider usage.
    #[serde(default)]
    pub stall_timeout_ms: Option<u64>,
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

    /// Number of parallel candidates (1 for MVP)
    #[serde(default = "default_candidate_count")]
    pub candidate_count: Option<usize>,

    /// Retry attempts if all candidates fail
    #[serde(default = "default_max_retries")]
    pub max_retries: Option<usize>,

    /// Use git worktrees for isolation
    #[serde(default = "default_use_worktrees")]
    pub use_worktrees: Option<bool>,

    /// Enable multi-LLM rating (false for MVP)
    #[serde(default = "default_rating_enabled")]
    pub rating_enabled: Option<bool>,

    /// Enable TDD flow: spec → tests → implement
    #[serde(default = "default_tdd_enabled")]
    pub tdd_enabled: Option<bool>,

    /// Retries for implementation (tests stay fixed)
    #[serde(default = "default_max_implementation_retries")]
    pub max_implementation_retries: Option<usize>,
}

/// Strategic planning configuration
#[derive(Debug, Deserialize, Clone, Default)]
pub struct PlanningConfig {
    /// Timeout in seconds for strategic plan LLM calls
    pub llm_timeout_seconds: u64,

    /// Timeout in seconds for milestone generation LLM calls
    pub milestone_llm_timeout_seconds: u64,
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

/// MongoDB configuration
#[derive(Debug, Deserialize, Clone, Default)]
pub struct MongoDbConfig {
    /// MongoDB connection string
    #[serde(default = "default_mongodb_connection_string")]
    pub connection_string: String,

    /// MongoDB database name
    #[serde(default = "default_mongodb_database")]
    pub database: String,

    /// Whether to use MongoDB instead of file-based storage
    #[serde(default = "default_use_mongodb")]
    pub enabled: bool,
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

/// Default value for candidate count
fn default_candidate_count() -> Option<usize> {
    Some(1)
}

/// Default value for max retries
fn default_max_retries() -> Option<usize> {
    Some(3)
}

/// Default value for use worktrees
fn default_use_worktrees() -> Option<bool> {
    Some(true)
}

/// Default value for rating enabled
fn default_rating_enabled() -> Option<bool> {
    Some(false)
}

/// Default value for TDD enabled
fn default_tdd_enabled() -> Option<bool> {
    Some(true)
}

/// Default value for max implementation retries
fn default_max_implementation_retries() -> Option<usize> {
    Some(3)
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

// Default values for MongoDB configuration
fn default_mongodb_connection_string() -> String {
    "mongodb://localhost:27017".to_string()
}

fn default_mongodb_database() -> String {
    "borg".to_string()
}

fn default_use_mongodb() -> bool {
    false
}

impl Config {
    /// Load configuration from a file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let config_text = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {:?}", path.as_ref()))?;

        let mut config: Config = toml::from_str(&config_text)
            .with_context(|| format!("Failed to parse config file: {:?}", path.as_ref()))?;

        // Backward-compat streaming defaults if unset in providers.streaming
        if config.providers.streaming.first_token_timeout_ms.is_none() {
            config.providers.streaming.first_token_timeout_ms = Some(30_000);
        }
        if config.providers.streaming.stall_timeout_ms.is_none() {
            config.providers.streaming.stall_timeout_ms = Some(10_000);
        }

        // Backward-compat: derive providers.* sections from llm.* entries if missing
        let mut emitted_warning = false;

        // OpenRouter mapping
        if config.providers.openrouter.is_none() {
            if let Some((_, llm_cfg)) = config
                .llm
                .iter()
                .find(|(_, v)| v.provider.eq_ignore_ascii_case("openrouter"))
            {
                config.providers.openrouter = Some(OpenRouterProviderSection {
                    api_base: llm_cfg.api_base.clone(),
                    headers: llm_cfg.headers.clone(),
                    timeouts: Some(StreamingTimeouts {
                        first_token_timeout_ms: llm_cfg.first_token_timeout_ms,
                        stall_timeout_ms: llm_cfg.stall_timeout_ms,
                    }),
                });
                emitted_warning = true;
            }
        }

        // OpenAI mapping
        if config.providers.openai.is_none() {
            if let Some((_, llm_cfg)) = config
                .llm
                .iter()
                .find(|(_, v)| v.provider.eq_ignore_ascii_case("openai"))
            {
                config.providers.openai = Some(OpenAiProviderSection {
                    api_base: llm_cfg.api_base.clone(),
                    use_responses_api: false,
                    timeouts: Some(StreamingTimeouts {
                        first_token_timeout_ms: llm_cfg.first_token_timeout_ms,
                        stall_timeout_ms: llm_cfg.stall_timeout_ms,
                    }),
                });
                emitted_warning = true;
            }
        }

        // Anthropic mapping
        if config.providers.anthropic.is_none() {
            if let Some((_, llm_cfg)) = config
                .llm
                .iter()
                .find(|(_, v)| v.provider.eq_ignore_ascii_case("anthropic"))
            {
                config.providers.anthropic = Some(AnthropicProviderSection {
                    api_base: llm_cfg.api_base.clone(),
                    timeouts: Some(StreamingTimeouts {
                        first_token_timeout_ms: llm_cfg.first_token_timeout_ms,
                        stall_timeout_ms: llm_cfg.stall_timeout_ms,
                    }),
                });
                emitted_warning = true;
            }
        }

        if emitted_warning {
            warn!("Deprecated: provider-specific settings under [llm.*] were mapped to new [providers.*] sections during load. Please migrate to [providers.openrouter]/[providers.openai]/[providers.anthropic] and [providers.streaming].");
        }

        // One-time deprecation warning: if model_selection is enabled and an OpenRouter llm entry pins a specific model,
        // inform that auto-selection will be used unless overridden.
        if config.model_selection.enabled {
            if let Some((_k, pinned)) = config.llm.iter().find(|(_k, v)| {
                v.provider.eq_ignore_ascii_case("openrouter")
                    && !v.model.is_empty()
                    && v.model != "openrouter/auto"
            }) {
                warn!(
                    "model_selection.enabled=true: configured OpenRouter model '{}' will be superseded by auto-selection unless you pin explicitly. Disable via [model_selection].enabled=false.",
                    pinned.model
                );
            }
        }

        // Deprecation warnings for legacy llm.* role keys mapping to modes.*
        {
            let mapping = [
                ("code_generation", "modes.code"),
                ("planning", "modes.architect"),
                ("code_review", "modes.review"),
                ("ethics", "modes.ethical"),
            ];
            for (legacy, new_path) in mapping {
                if config.llm.contains_key(legacy) {
                    warn!(
                        "Deprecated config: [llm.{}] detected. This maps to [{}] in Modes v2. Behavior unchanged unless [modes.v2_enabled]=true.",
                        legacy, new_path
                    );
                }
            }
        }

        Ok(config)
    }

    /// Create a new config with default values for testing
    #[cfg(test)]
    pub fn for_testing() -> Self {
        Self {
            llm: HashMap::from([(
                "mock".to_string(),
                LlmConfig {
                    provider: "mock".to_string(),
                    api_key: "test-key".to_string(),
                    model: "test-model".to_string(),
                    max_tokens: default_max_tokens(),
                    temperature: default_temperature(),
                    api_base: None,
                    headers: None,
                    enable_streaming: None,
                    enable_thinking: None,
                    reasoning_effort: None,
                    reasoning_budget_tokens: None,
                    first_token_timeout_ms: None,
                    stall_timeout_ms: None,
                },
            )]),
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
                candidate_count: default_candidate_count(),
                max_retries: default_max_retries(),
                use_worktrees: default_use_worktrees(),
                rating_enabled: default_rating_enabled(),
                tdd_enabled: default_tdd_enabled(),
                max_implementation_retries: default_max_implementation_retries(),
            },
            planning: PlanningConfig::default(),
            llm_logging: LlmLoggingConfig {
                enabled: default_logging_enabled(),
                log_dir: default_log_dir(),
                console_logging: default_console_logging(),
                include_full_prompts: default_include_full_prompts(),
                include_full_responses: default_include_full_responses(),
                max_log_size_mb: default_max_log_size_mb(),
                log_files_to_keep: default_log_files_to_keep(),
            },
            providers: ProvidersConfig::default(),
            mongodb: MongoDbConfig {
                connection_string: default_mongodb_connection_string(),
                database: default_mongodb_database(),
                enabled: default_use_mongodb(),
            },
            model_selection: ModelSelectionConfig::default(),
            modes: ModesConfig::default(),
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
            candidate_count: default_candidate_count(),
            max_retries: default_max_retries(),
            use_worktrees: default_use_worktrees(),
            rating_enabled: default_rating_enabled(),
            tdd_enabled: default_tdd_enabled(),
            max_implementation_retries: default_max_implementation_retries(),
        }
    }
}

/// Provider-wide configuration sections and streaming defaults
#[derive(Debug, Deserialize, Clone)]
pub struct ProvidersConfig {
    #[serde(default)]
    pub openrouter: Option<OpenRouterProviderSection>,
    #[serde(default)]
    pub openai: Option<OpenAiProviderSection>,
    #[serde(default)]
    pub anthropic: Option<AnthropicProviderSection>,
    /// Global streaming defaults (used when llm.* entries don't specify)
    #[serde(default)]
    pub streaming: StreamingTimeouts,
}

impl Default for ProvidersConfig {
    fn default() -> Self {
        Self {
            openrouter: None,
            openai: None,
            anthropic: None,
            streaming: StreamingTimeouts {
                first_token_timeout_ms: Some(30_000),
                stall_timeout_ms: Some(10_000),
            },
        }
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct OpenRouterProviderSection {
    #[serde(default)]
    pub api_base: Option<String>,
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub timeouts: Option<StreamingTimeouts>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct OpenAiProviderSection {
    #[serde(default)]
    pub api_base: Option<String>,
    #[serde(default)]
    pub use_responses_api: bool,
    #[serde(default)]
    pub timeouts: Option<StreamingTimeouts>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct AnthropicProviderSection {
    #[serde(default)]
    pub api_base: Option<String>,
    #[serde(default)]
    pub timeouts: Option<StreamingTimeouts>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct StreamingTimeouts {
    #[serde(default)]
    pub first_token_timeout_ms: Option<u64>,
    #[serde(default)]
    pub stall_timeout_ms: Option<u64>,
}

// =====================
// Model selection config (feature-gated integration)
// =====================

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelSelectionConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_ttl_hours")]
    pub ttl_hours: u64,
    #[serde(default = "default_sticky_days")]
    pub sticky_days: u64,
    /// Which provider catalog to use. For now only "openrouter" is supported.
    #[serde(default = "default_selection_provider")]
    pub provider: String,
    #[serde(default)]
    pub ranking: Option<RankingConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RankingConfig {
    #[serde(default = "default_ranking_provider")]
    pub provider: String, // "internal" | "artificialanalysis"
}

fn default_ttl_hours() -> u64 {
    24
}
fn default_sticky_days() -> u64 {
    7
}
fn default_selection_provider() -> String {
    "openrouter".to_string()
}
fn default_ranking_provider() -> String {
    "internal".to_string()
}

impl Default for ModelSelectionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ttl_hours: default_ttl_hours(),
            sticky_days: default_sticky_days(),
            provider: default_selection_provider(),
            ranking: Some(RankingConfig {
                provider: default_ranking_provider(),
            }),
        }
    }
}

/// Runtime snapshot for model selection (small subset of Config)
#[derive(Debug, Clone, Default)]
pub struct ModelSelectionRuntime {
    pub config: ModelSelectionConfig,
    pub openrouter: Option<OpenRouterProviderSection>,
}

static RUNTIME_MODEL_SELECTION: OnceLock<RwLock<ModelSelectionRuntime>> = OnceLock::new();

/// Set or update the runtime snapshot used by model selection.
/// Safe to call multiple times; last call wins.
pub fn set_runtime_model_selection_from(cfg: &Config) {
    let lock =
        RUNTIME_MODEL_SELECTION.get_or_init(|| RwLock::new(ModelSelectionRuntime::default()));
    let mut guard = lock.write().expect("poisoned runtime model selection lock");
    *guard = ModelSelectionRuntime {
        config: cfg.model_selection.clone(),
        openrouter: cfg.providers.openrouter.clone(),
    };
}

/// Read a clone of the runtime model selection snapshot (if set).
pub fn get_runtime_model_selection() -> Option<ModelSelectionRuntime> {
    RUNTIME_MODEL_SELECTION.get().map(|rw| {
        rw.read()
            .expect("poisoned runtime model selection lock")
            .clone()
    })
}

// =====================
// Modes v2 config (feature-gated scaffolding)
// =====================

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ModesConfig {
    /// Master feature flag: when false (default), legacy paths remain in effect.
    #[serde(default)]
    pub v2_enabled: bool,

    /// Optional per-mode placeholders (parsed but currently unused)
    #[serde(default)]
    pub orchestrate: Option<ModeOrchestrateConfig>,
    #[serde(default)]
    pub architect: Option<ModeArchitectConfig>,
    #[serde(default)]
    pub code: Option<ModeCodeConfig>,
    #[serde(default)]
    pub review: Option<ModeReviewConfig>,
    #[serde(default)]
    pub debug: Option<ModeDebugConfig>,
    #[serde(default)]
    pub ethical: Option<ModeEthicalConfig>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ModeOrchestrateConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ModeArchitectConfig {
    #[serde(default)]
    pub model_policy: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ModeCodeConfig {
    #[serde(default)]
    pub model_policy: Option<String>,
    #[serde(default)]
    pub tools_required: Option<bool>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ModeReviewConfig {
    #[serde(default)]
    pub model_policy: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ModeDebugConfig {
    #[serde(default)]
    pub model_policy: Option<String>,
    #[serde(default)]
    pub reasoning_preferred: Option<bool>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ModeEthicalConfig {
    #[serde(default)]
    pub model_policy: Option<String>,
    #[serde(default)]
    pub strict: Option<bool>,
}
