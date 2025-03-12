use anyhow::{Context, Result};
use chrono::prelude::*;
use log::info;
use std::fs::{self, File, create_dir_all};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

use crate::core::config::LlmLoggingConfig;

/// LLM Logger to record communications between the agent and LLMs
pub struct LlmLogger {
    /// Configuration for logging
    config: LlmLoggingConfig,

    /// Current log file path
    log_file_path: Option<PathBuf>,

    /// Writer for the log file
    log_file: Option<Arc<Mutex<File>>>,
}

impl LlmLogger {
    /// Create a new LLM logger with the given configuration
    pub fn new(config: LlmLoggingConfig) -> Result<Self> {
        let mut logger = Self {
            config,
            log_file_path: None,
            log_file: None,
        };

        // If logging is enabled, initialize the log directory and file
        if logger.config.enabled {
            logger.initialize_logging()?;
        }

        Ok(logger)
    }

    /// Initialize logging by creating the log directory and file
    fn initialize_logging(&mut self) -> Result<()> {
        // Create the log directory if it doesn't exist
        let log_dir = Path::new(&self.config.log_dir);
        create_dir_all(log_dir)
            .with_context(|| format!("Failed to create log directory: {:?}", log_dir))?;

        // Create a new log file with a timestamp
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let log_file_name = format!("llm_log_{}.txt", timestamp);
        let log_file_path = log_dir.join(log_file_name);

        // Open the log file
        let file = File::create(&log_file_path)
            .with_context(|| format!("Failed to create log file: {:?}", log_file_path))?;

        self.log_file_path = Some(log_file_path.clone());
        self.log_file = Some(Arc::new(Mutex::new(file)));

        info!("LLM logging initialized. Log file: {:?}", log_file_path);

        // Clean up old log files if there are too many
        self.clean_old_logs()?;

        Ok(())
    }

    /// Log a request to an LLM
    pub fn log_request(&self, provider: &str, model: &str, prompt: &str) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        // Format timestamp
        let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string();

        // Create log entry
        let mut log_entry = format!("\n===== REQUEST: {} {} =====\n", provider, model);
        log_entry.push_str(&format!("TIMESTAMP: {}\n", timestamp));

        if self.config.include_full_prompts {
            log_entry.push_str(&format!("PROMPT:\n{}\n", prompt));
        } else {
            // Only include a summary of the prompt
            let summary = if prompt.len() > 100 {
                format!("{}...", &prompt[0..100])
            } else {
                prompt.to_string()
            };
            log_entry.push_str(&format!("PROMPT SUMMARY: {}\n", summary));
        }

        // Write to log file
        self.write_to_log(&log_entry)?;

        // Write to console if enabled
        if self.config.console_logging {
            println!("{}", log_entry);
        }

        Ok(())
    }

    /// Log a response from an LLM
    pub fn log_response(&self, provider: &str, model: &str, response: &str, duration_ms: u64) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        // Format timestamp
        let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string();

        // Create log entry
        let mut log_entry = format!("\n===== RESPONSE: {} {} =====\n", provider, model);
        log_entry.push_str(&format!("TIMESTAMP: {}\n", timestamp));
        log_entry.push_str(&format!("DURATION: {}ms\n", duration_ms));

        if self.config.include_full_responses {
            log_entry.push_str(&format!("RESPONSE:\n{}\n", response));
        } else {
            // Only include a summary of the response
            let summary = if response.len() > 100 {
                format!("{}...", &response[0..100])
            } else {
                response.to_string()
            };
            log_entry.push_str(&format!("RESPONSE SUMMARY: {}\n", summary));
        }

        // Write to log file
        self.write_to_log(&log_entry)?;

        // Write to console if enabled
        if self.config.console_logging {
            println!("{}", log_entry);
        }

        Ok(())
    }

    /// Write text to the log file
    fn write_to_log(&self, text: &str) -> Result<()> {
        if let Some(file) = &self.log_file {
            let mut file_guard = file.lock().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "Failed to acquire lock on log file")
            })?;

            file_guard.write_all(text.as_bytes())
                .with_context(|| "Failed to write to log file")?;

            file_guard.flush()
                .with_context(|| "Failed to flush log file")?;
        }

        Ok(())
    }

    /// Clean up old log files if there are too many
    fn clean_old_logs(&self) -> Result<()> {
        let log_dir = Path::new(&self.config.log_dir);

        // Get all log files
        let entries = fs::read_dir(log_dir)
            .with_context(|| format!("Failed to read log directory: {:?}", log_dir))?;

        // Filter log files and sort by modification time
        let mut log_files = Vec::new();
        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() &&
               path.extension().map_or(false, |ext| ext == "txt") &&
               path.file_name().map_or(false, |name| name.to_string_lossy().starts_with("llm_log_")) {

                let metadata = fs::metadata(&path)?;
                log_files.push((path, metadata.modified()?));
            }
        }

        // Sort by modification time (newest first)
        log_files.sort_by(|a, b| b.1.cmp(&a.1));

        // If there are more than the configured number of files, delete the oldest ones
        if log_files.len() > self.config.log_files_to_keep as usize {
            for (path, _) in log_files.iter().skip(self.config.log_files_to_keep as usize) {
                fs::remove_file(path)
                    .with_context(|| format!("Failed to delete old log file: {:?}", path))?;

                info!("Deleted old log file: {:?}", path);
            }
        }

        Ok(())
    }
}