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
