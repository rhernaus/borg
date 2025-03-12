use async_trait::async_trait;
use anyhow::Result;
use std::time::Duration;

/// A trait representing a code generator that can propose code improvements
#[async_trait]
pub trait CodeGenerator: Send + Sync {
    /// Generate a code improvement based on the current codebase
    ///
    /// # Arguments
    /// * `context` - Information about the current codebase and task
    ///
    /// # Returns
    /// A result containing the generated code as a string or an error
    async fn generate_improvement(&self, context: &CodeContext) -> Result<CodeImprovement>;

    /// Provide feedback to the generator about a previous generation
    ///
    /// # Arguments
    /// * `improvement` - The previous code improvement
    /// * `success` - Whether the improvement was successful
    /// * `feedback` - Detailed feedback about the improvement
    ///
    /// # Returns
    /// A result indicating success or failure
    async fn provide_feedback(&self, improvement: &CodeImprovement, success: bool, feedback: &str) -> Result<()>;

    /// Generate a response for git operations
    async fn generate_git_response(&self, query: &str) -> Result<String>;
}

/// The context for code generation
#[derive(Debug, Clone)]
pub struct CodeContext {
    /// The task or goal for the improvement
    pub task: String,

    /// The relevant file paths to consider
    pub file_paths: Vec<String>,

    /// Additional context or requirements
    pub requirements: Option<String>,

    /// Previous attempts that didn't work (if any)
    pub previous_attempts: Vec<PreviousAttempt>,

    /// File contents keyed by file path
    pub file_contents: Option<std::collections::HashMap<String, String>>,

    /// Related test files if any
    pub test_files: Option<Vec<String>>,

    /// Test file contents keyed by file path
    pub test_contents: Option<std::collections::HashMap<String, String>>,

    /// Dependency information
    pub dependencies: Option<String>,

    /// Code structure information
    pub code_structure: Option<String>,

    /// Maximum number of attempts to try
    pub max_attempts: Option<u32>,

    /// Current attempt number (1-indexed)
    pub current_attempt: Option<u32>,
}

/// A previous code generation attempt
#[derive(Debug, Clone)]
pub struct PreviousAttempt {
    /// The code that was generated
    pub code: String,

    /// The reason it failed or was rejected
    pub failure_reason: String,

    /// The time the attempt was made
    pub timestamp: chrono::DateTime<chrono::Utc>,

    /// Test results if any
    pub test_results: Option<String>,

    /// Specific error messages
    pub error_messages: Option<Vec<String>>,

    /// Whether this attempt was compiled successfully
    pub compiled: Option<bool>,

    /// Whether this attempt passed tests
    pub tests_passed: Option<bool>,

    /// Any notes or observations about this attempt
    pub notes: Option<String>,
}

/// A code improvement proposal
#[derive(Debug, Clone)]
pub struct CodeImprovement {
    /// A unique identifier for this improvement
    pub id: String,

    /// The task or goal that was targeted
    pub task: String,

    /// The generated code
    pub code: String,

    /// The files that should be modified
    pub target_files: Vec<FileChange>,

    /// Explanation of the changes
    pub explanation: String,
}

/// A change to be applied to a file
#[derive(Debug, Clone)]
pub struct FileChange {
    /// The path to the file
    pub file_path: String,

    /// The starting line number (1-indexed)
    pub start_line: Option<usize>,

    /// The ending line number (1-indexed)
    pub end_line: Option<usize>,

    /// The new content
    pub new_content: String,
}