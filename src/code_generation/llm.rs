use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

use crate::core::config::LlmConfig;
use crate::core::error::BorgError;

/// LLM provider trait
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Generate a text completion for the given prompt
    async fn generate(&self, prompt: &str, max_tokens: Option<usize>, temperature: Option<f32>) -> Result<String>;
}

/// Factory for creating the appropriate LLM provider
pub struct LlmFactory;

impl LlmFactory {
    /// Create a new LLM provider based on configuration
    pub fn create(config: LlmConfig) -> Result<Box<dyn LlmProvider>> {
        match config.provider.as_str() {
            "openai" => Ok(Box::new(OpenAiProvider::new(config)?)),
            "anthropic" => Ok(Box::new(AnthropicProvider::new(config)?)),
            "mock" => Ok(Box::new(MockLlmProvider::new(config)?)),
            // Add more providers as needed
            _ => Err(anyhow::anyhow!(BorgError::ConfigError(format!(
                "Unsupported LLM provider: {}",
                config.provider
            ))))
        }
    }
}

/// OpenAI API provider
pub struct OpenAiProvider {
    api_key: String,
    model: String,
    client: Client,
}

impl OpenAiProvider {
    /// Create a new OpenAI provider
    pub fn new(config: LlmConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            api_key: config.api_key,
            model: config.model,
            client,
        })
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn generate(&self, prompt: &str, max_tokens: Option<usize>, temperature: Option<f32>) -> Result<String> {
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

        let response = match self.client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await {
                Ok(resp) => resp,
                Err(e) => {
                    log::error!("Network error when contacting OpenAI API: {}", e);
                    return Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                        "Failed to send request to OpenAI API: {}", e
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

        let chat_response: ChatResponse = response.json().await
            .context("Failed to parse OpenAI API response")?;

        if let Some(choice) = chat_response.choices.first() {
            Ok(choice.message.content.clone())
        } else {
            Err(anyhow::anyhow!(BorgError::LlmApiError("OpenAI API returned no choices".to_string())))
        }
    }
}

/// Anthropic API provider
pub struct AnthropicProvider {
    api_key: String,
    model: String,
    client: Client,
}

impl AnthropicProvider {
    /// Create a new Anthropic provider
    pub fn new(config: LlmConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            api_key: config.api_key,
            model: config.model,
            client,
        })
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn generate(&self, prompt: &str, max_tokens: Option<usize>, temperature: Option<f32>) -> Result<String> {
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

        let response = self.client
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
            let error_text = response.text().await
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

        let anthropic_response: AnthropicResponse = response.json().await
            .context("Failed to parse Anthropic API response")?;

        if let Some(block) = anthropic_response.content.first() {
            Ok(block.text.clone())
        } else {
            Err(anyhow::anyhow!(BorgError::LlmApiError("Anthropic API returned no content".to_string())))
        }
    }
}

/// A mock LLM provider for testing without API keys
pub struct MockLlmProvider {
    model: String,
}

impl MockLlmProvider {
    /// Create a new mock provider
    pub fn new(config: LlmConfig) -> Result<Self> {
        log::info!("Creating mock LLM provider with model: {}", config.model);

        Ok(Self {
            model: config.model,
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
            r#"I've analyzed the code and here's my implementation for the task: "{task}".

```rust
// Improved implementation for {file_path}
use std::sync::Arc;

// This is a mock improvement generated by the mock LLM provider
// In a real scenario, this would be generated based on the actual code
fn improved_function() -> Result<String, Box<dyn std::error::Error>> {{
    println!("Running improved function with better performance");

    // Mock improvement: Added caching for better performance
    let mut cache = std::collections::HashMap::new();
    cache.insert("key", "value");

    Ok("Success".to_string())
}}
```

## EXPLANATION:
This implementation improves the code by:
1. Adding proper error handling with Result type
2. Implementing a caching mechanism to avoid redundant computations
3. Improving code clarity with better variable names and comments
4. Using more efficient data structures for the task

The changes should result in better performance and maintainability.
"#
        )
    }
}

#[async_trait]
impl LlmProvider for MockLlmProvider {
    async fn generate(&self, prompt: &str, _max_tokens: Option<usize>, _temperature: Option<f32>) -> Result<String> {
        log::info!("Mock LLM provider generating response for prompt length: {} characters", prompt.len());

        // Simulate network delay
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Generate a mock response
        Ok(self.generate_mock_response(prompt))
    }
}