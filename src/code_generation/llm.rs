use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::io::{self, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::code_generation::llm_logging::LlmLogger;
use crate::core::config::{LlmConfig, LlmLoggingConfig};
use crate::core::error::BorgError;

/// LLM provider trait
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Generate a text completion for the given prompt
    async fn generate(
        &self,
        prompt: &str,
        max_tokens: Option<usize>,
        temperature: Option<f32>,
    ) -> Result<String>;

    /// Generate a text completion for the given prompt and stream the response to stdout
    async fn generate_streaming(
        &self,
        prompt: &str,
        max_tokens: Option<usize>,
        temperature: Option<f32>,
    ) -> Result<String>;
}

/// Factory for creating the appropriate LLM provider
pub struct LlmFactory;

impl LlmFactory {
    /// Create a new LLM provider based on configuration
    pub fn create(
        config: LlmConfig,
        logging_config: LlmLoggingConfig,
    ) -> Result<Box<dyn LlmProvider>> {
        match config.provider.as_str() {
            "openai" => Ok(Box::new(OpenAiProvider::new(config, logging_config)?)),
            "anthropic" => Ok(Box::new(AnthropicProvider::new(config, logging_config)?)),
            "mock" => Ok(Box::new(MockLlmProvider::new(config, logging_config)?)),
            // Add more providers as needed
            _ => Err(anyhow::anyhow!(BorgError::ConfigError(format!(
                "Unsupported LLM provider: {}",
                config.provider
            )))),
        }
    }
}

/// OpenAI API provider
pub struct OpenAiProvider {
    api_key: String,
    model: String,
    client: Client,
    logger: Arc<LlmLogger>,
}

impl OpenAiProvider {
    /// Create a new OpenAI provider
    pub fn new(config: LlmConfig, logging_config: LlmLoggingConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .context("Failed to create HTTP client")?;

        let logger = Arc::new(LlmLogger::new(logging_config)?);

        Ok(Self {
            api_key: config.api_key,
            model: config.model,
            client,
            logger,
        })
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn generate(
        &self,
        prompt: &str,
        max_tokens: Option<usize>,
        temperature: Option<f32>,
    ) -> Result<String> {
        let url = "https://api.openai.com/v1/chat/completions";

        let payload = json!({
            "model": self.model,
            "messages": [
                {
                    "role": "system",
                    "content": "You are an AI assistant that helps with coding in Rust. You provide clear, concise, and correct code."
                },
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "max_tokens": max_tokens.unwrap_or(1024),
            "temperature": temperature.unwrap_or(0.7),
        });

        log::debug!("Sending request to OpenAI API with model: {}", self.model);

        // Log the request
        self.logger.log_request("OpenAI", &self.model, prompt)?;

        // Track time for request duration
        let start_time = Instant::now();

        let response = match self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                log::error!("Network error when contacting OpenAI API: {}", e);
                return Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                    "Failed to send request to OpenAI API: {}",
                    e
                ))));
            }
        };

        let status = response.status();
        if !status.is_success() {
            let error_text = match response.text().await {
                Ok(text) => text,
                Err(e) => format!("Could not read error response: {}", e),
            };

            log::error!("OpenAI API error ({}): {}", status, error_text);

            return Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                "OpenAI API returned error ({}): {}",
                status, error_text
            ))));
        }

        #[derive(Deserialize)]
        struct ChatResponse {
            choices: Vec<ChatChoice>,
        }

        #[derive(Deserialize)]
        struct ChatChoice {
            message: ChatMessage,
        }

        #[derive(Deserialize)]
        struct ChatMessage {
            content: String,
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .context("Failed to parse OpenAI API response")?;

        // Calculate request duration
        let duration = start_time.elapsed().as_millis() as u64;

        if let Some(choice) = chat_response.choices.first() {
            let content = choice.message.content.clone();

            // Log the response
            self.logger
                .log_response("OpenAI", &self.model, &content, duration)?;

            Ok(content)
        } else {
            Err(anyhow::anyhow!(BorgError::LlmApiError(
                "OpenAI API returned no choices".to_string()
            )))
        }
    }

    async fn generate_streaming(
        &self,
        prompt: &str,
        max_tokens: Option<usize>,
        temperature: Option<f32>,
    ) -> Result<String> {
        let url = "https://api.openai.com/v1/chat/completions";

        let payload = json!({
            "model": self.model,
            "messages": [
                {
                    "role": "system",
                    "content": "You are an AI assistant that helps with coding in Rust. You provide clear, concise, and correct code."
                },
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "max_tokens": max_tokens.unwrap_or(1024),
            "temperature": temperature.unwrap_or(0.7),
            "stream": true  // Enable streaming
        });

        log::debug!(
            "Sending streaming request to OpenAI API with model: {}",
            self.model
        );

        // Log the request
        self.logger.log_request("OpenAI", &self.model, prompt)?;

        // Track time for request duration
        let start_time = Instant::now();

        let response = match self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                log::error!("Network error when contacting OpenAI API: {}", e);
                return Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                    "Failed to send request to OpenAI API: {}",
                    e
                ))));
            }
        };

        let status = response.status();
        if !status.is_success() {
            let error_text = match response.text().await {
                Ok(text) => text,
                Err(e) => format!("Could not read error response: {}", e),
            };

            log::error!("OpenAI API error ({}): {}", status, error_text);

            return Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                "OpenAI API returned error ({}): {}",
                status, error_text
            ))));
        }

        // Get the streaming body
        let mut stream = response.bytes_stream();
        let mut content = String::new();
        let mut stdout = io::stdout();

        // Process the stream chunks
        while let Some(item) = stream.next().await {
            let chunk = match item {
                Ok(chunk) => chunk,
                Err(e) => {
                    return Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                        "Error reading streaming response: {}",
                        e
                    ))));
                }
            };

            // Parse the chunk
            let chunk_str = String::from_utf8_lossy(&chunk);

            // OpenAI streams data as "data: {...}\n\n" for each chunk
            for line in chunk_str.lines() {
                if line.starts_with("data: ") {
                    if line == "data: [DONE]" {
                        continue;
                    }

                    let json_str = &line["data: ".len()..];
                    match serde_json::from_str::<serde_json::Value>(json_str) {
                        Ok(json) => {
                            if let Some(delta) = json
                                .get("choices")
                                .and_then(|choices| choices.get(0))
                                .and_then(|choice| choice.get("delta"))
                                .and_then(|delta| delta.get("content"))
                            {
                                if let Some(text) = delta.as_str() {
                                    content.push_str(text);
                                    print!("{}", text);
                                    stdout.flush().unwrap();
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!("Failed to parse JSON from OpenAI stream: {}", e);
                        }
                    }
                }
            }
        }

        println!(); // Ensure a newline at the end

        // Calculate request duration
        let duration = start_time.elapsed().as_millis() as u64;

        // Log the response
        self.logger
            .log_response("OpenAI", &self.model, &content, duration)?;

        Ok(content)
    }
}

/// Anthropic API provider
pub struct AnthropicProvider {
    api_key: String,
    model: String,
    client: Client,
    logger: Arc<LlmLogger>,
}

impl AnthropicProvider {
    /// Create a new Anthropic provider
    pub fn new(config: LlmConfig, logging_config: LlmLoggingConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .context("Failed to create HTTP client")?;

        let logger = Arc::new(LlmLogger::new(logging_config)?);

        Ok(Self {
            api_key: config.api_key,
            model: config.model,
            client,
            logger,
        })
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn generate(
        &self,
        prompt: &str,
        max_tokens: Option<usize>,
        temperature: Option<f32>,
    ) -> Result<String> {
        let url = "https://api.anthropic.com/v1/messages";

        let payload = json!({
            "model": self.model,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "max_tokens": max_tokens.unwrap_or(1024),
            "temperature": temperature.unwrap_or(0.7),
        });

        // Log the request
        self.logger.log_request("Anthropic", &self.model, prompt)?;

        // Track time for request duration
        let start_time = Instant::now();

        let response = self
            .client
            .post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .context("Failed to send request to Anthropic API")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .context("Failed to read error response from Anthropic API")?;
            return Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                "Anthropic API returned error ({}): {}",
                status, error_text
            ))));
        }

        #[derive(Deserialize)]
        struct AnthropicResponse {
            content: Vec<ContentBlock>,
        }

        #[derive(Deserialize)]
        struct ContentBlock {
            text: String,
        }

        let anthropic_response: AnthropicResponse = response
            .json()
            .await
            .context("Failed to parse Anthropic API response")?;

        // Calculate request duration
        let duration = start_time.elapsed().as_millis() as u64;

        if let Some(block) = anthropic_response.content.first() {
            let content = block.text.clone();

            // Log the response
            self.logger
                .log_response("Anthropic", &self.model, &content, duration)?;

            Ok(content)
        } else {
            Err(anyhow::anyhow!(BorgError::LlmApiError(
                "Anthropic API returned no content".to_string()
            )))
        }
    }

    async fn generate_streaming(
        &self,
        prompt: &str,
        max_tokens: Option<usize>,
        temperature: Option<f32>,
    ) -> Result<String> {
        let url = "https://api.anthropic.com/v1/messages";

        let payload = json!({
            "model": self.model,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "max_tokens": max_tokens.unwrap_or(1024),
            "temperature": temperature.unwrap_or(0.7),
            "stream": true  // Enable streaming
        });

        log::debug!(
            "Sending streaming request to Anthropic API with model: {}",
            self.model
        );

        // Log the request
        self.logger.log_request("Anthropic", &self.model, prompt)?;

        // Track time for request duration
        let start_time = Instant::now();

        let response = self
            .client
            .post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .context("Failed to send request to Anthropic API")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .context("Failed to read error response from Anthropic API")?;
            return Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                "Anthropic API returned error ({}): {}",
                status, error_text
            ))));
        }

        // Get the streaming body
        let mut stream = response.bytes_stream();
        let mut content = String::new();
        let mut stdout = io::stdout();

        // Process the stream chunks
        while let Some(item) = stream.next().await {
            let chunk = match item {
                Ok(chunk) => chunk,
                Err(e) => {
                    return Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                        "Error reading streaming response: {}",
                        e
                    ))));
                }
            };

            // Parse the chunk
            let chunk_str = String::from_utf8_lossy(&chunk);

            // Anthropic streams data as "event: content_block_start\ndata: {...}\n\n" etc.
            for line in chunk_str.lines() {
                if line.starts_with("data: ") {
                    let json_str = &line["data: ".len()..];
                    match serde_json::from_str::<serde_json::Value>(json_str) {
                        Ok(json) => {
                            if let Some(delta) = json
                                .get("delta")
                                .and_then(|delta| delta.get("text"))
                                .and_then(|text| text.as_str())
                            {
                                content.push_str(delta);
                                print!("{}", delta);
                                stdout.flush().unwrap();
                            }
                        }
                        Err(e) => {
                            log::warn!("Failed to parse JSON from Anthropic stream: {}", e);
                        }
                    }
                }
            }
        }

        println!(); // Ensure a newline at the end

        // Calculate request duration
        let duration = start_time.elapsed().as_millis() as u64;

        // Log the response
        self.logger
            .log_response("Anthropic", &self.model, &content, duration)?;

        Ok(content)
    }
}

/// A mock LLM provider for testing without API keys
pub struct MockLlmProvider {
    model: String,
    logger: Arc<LlmLogger>,
}

impl MockLlmProvider {
    /// Create a new mock provider
    pub fn new(config: LlmConfig, logging_config: LlmLoggingConfig) -> Result<Self> {
        log::info!("Creating mock LLM provider with model: {}", config.model);

        let logger = Arc::new(LlmLogger::new(logging_config)?);

        Ok(Self {
            model: config.model,
            logger,
        })
    }

    /// Generate a mock response based on the code improvement task
    fn generate_mock_response(&self, prompt: &str) -> String {
        // Find the task description in the prompt
        let task = if let Some(start) = prompt.find("## TASK DESCRIPTION:") {
            if let Some(end_idx) = prompt[start..].find("\n##") {
                prompt[start + 21..start + end_idx].trim()
            } else {
                "Improve code efficiency"
            }
        } else {
            "Improve code efficiency"
        };

        let file_path = if let Some(start) = prompt.find("## FILES TO MODIFY:") {
            if let Some(end_idx) = prompt[start..].find("\n##") {
                let files_section = &prompt[start + 20..start + end_idx];
                if let Some(file) = files_section.lines().next() {
                    file.trim()
                } else {
                    "src/main.rs"
                }
            } else {
                "src/main.rs"
            }
        } else {
            "src/main.rs"
        };

        // Generate a placeholder response with a template improvement
        format!(
            "I've analyzed the task to '{task}' for file {file_path}.\n\n\
            ## CODE IMPROVEMENT:\n\
            ```rust\n\
            // Improved implementation for {task}\n\
            pub fn improved_function() {{\n\
                // More efficient algorithm\n\
                let result = compute_faster();\n\
                println!(\"Improved result: {{}}\", result);\n\
            }}\n\
            \n\
            fn compute_faster() -> u64 {{\n\
                // Use memoization for better performance\n\
                let cached_result = 42;\n\
                cached_result\n\
            }}\n\
            ```\n\n\
            ## EXPLANATION:\n\
            This improvement addresses '{task}' by implementing a more efficient algorithm with memoization, \
            which reduces redundant calculations and improves overall performance. \
            The new implementation maintains the same functionality while being more maintainable and faster.\n"
        )
    }
}

#[async_trait]
impl LlmProvider for MockLlmProvider {
    async fn generate(
        &self,
        prompt: &str,
        _max_tokens: Option<usize>,
        _temperature: Option<f32>,
    ) -> Result<String> {
        // Log the request
        self.logger.log_request("Mock", &self.model, prompt)?;

        let start_time = Instant::now();

        // Generate mock response
        let response = self.generate_mock_response(prompt);

        // Calculate duration (simulate some delay)
        let duration = start_time.elapsed().as_millis() as u64;

        // Log the response
        self.logger
            .log_response("Mock", &self.model, &response, duration)?;

        Ok(response)
    }

    async fn generate_streaming(
        &self,
        prompt: &str,
        _max_tokens: Option<usize>,
        _temperature: Option<f32>,
    ) -> Result<String> {
        log::info!("Generating streaming mock response for prompt");

        // Log the request
        self.logger.log_request("Mock", &self.model, prompt)?;

        // Track time for request duration
        let start_time = Instant::now();

        // Get the mock response
        let response = self.generate_mock_response(prompt);

        // Mock streaming by printing each word with a small delay
        let mut stdout = io::stdout();
        let mut content = String::new();

        for word in response.split_whitespace() {
            // Add a space if this isn't the first word
            if !content.is_empty() {
                print!(" ");
                content.push(' ');
            }

            // Print the word
            print!("{}", word);
            content.push_str(word);
            stdout.flush().unwrap();

            // Sleep for a short time to simulate streaming
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        println!(); // Ensure a newline at the end

        // Calculate request duration
        let duration = start_time.elapsed().as_millis() as u64;

        // Log the response
        self.logger
            .log_response("Mock", &self.model, &content, duration)?;

        Ok(content)
    }
}
