use thiserror::Error;

/// Custom error types for the Borg agent
#[derive(Error, Debug)]
pub enum BorgError {
    /// Configuration errors
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// LLM API errors
    #[error("LLM API error: {0}")]
    LlmApiError(String),

    /// Git errors
    #[error("Git error: {0}")]
    GitError(String),

    /// Testing errors
    #[error("Testing error: {0}")]
    TestingError(String),

    /// Code generation errors
    #[error("Code generation error: {0}")]
    CodeGenError(String),

    /// Resource limit errors
    #[error("Resource limit exceeded: {0}")]
    ResourceLimitError(String),

    /// Timeout errors
    #[error("Operation timed out: {0}")]
    TimeoutError(String),

    /// Validation errors
    #[error("Validation failed: {0}")]
    ValidationError(String),

    /// I/O errors
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Serialization errors
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// External command execution errors
    #[error("Command execution failed: {0}")]
    CommandError(String),
}

/// Normalized provider-layer errors
#[derive(Error, Debug, Clone)]
pub enum ProviderError {
    #[error("Invalid parameters: {message}")]
    InvalidParams {
        details: Option<String>,
        #[cfg_attr(not(debug_assertions), allow(dead_code))]
        code: Option<String>,
        message: String,
        status: Option<u16>,
    },

    #[error("Rate limited: {message}")]
    RateLimited {
        details: Option<String>,
        code: Option<String>,
        message: String,
        status: Option<u16>,
        retry_after_ms: Option<u64>,
    },

    #[error("Authentication error: {message}")]
    Auth {
        details: Option<String>,
        code: Option<String>,
        message: String,
        status: Option<u16>,
    },

    #[error("Model unavailable: {message}")]
    ModelUnavailable {
        details: Option<String>,
        code: Option<String>,
        message: String,
        status: Option<u16>,
    },

    #[error("Provider outage: {message}")]
    ProviderOutage {
        details: Option<String>,
        code: Option<String>,
        message: String,
        status: Option<u16>,
    },

    #[error("Server error: {message}")]
    ServerError {
        details: Option<String>,
        code: Option<String>,
        message: String,
        status: Option<u16>,
    },

    #[error("Streaming timeout waiting for first token after {timeout_ms} ms")]
    TimeoutFirstToken { timeout_ms: u64 },

    #[error("Streaming stalled for {timeout_ms} ms")]
    TimeoutStall { timeout_ms: u64 },

    #[error("Network error: {message}")]
    Network { message: String },
}

impl ProviderError {
    /// Convenience to construct an InvalidParams error
    pub fn invalid_params(message: impl Into<String>, status: Option<u16>) -> Self {
        ProviderError::InvalidParams {
            details: None,
            code: None,
            message: message.into(),
            status,
        }
    }
}
