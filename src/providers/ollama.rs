// File: src/providers/ollama.rs
use async_trait::async_trait;
use futures_util::StreamExt;
use log::warn;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::timeout;

use crate::core::config::ModelConfig;
use crate::core::error::ProviderError;
use crate::providers::{ContentPart, GenerateRequest, GenerateResponse, Role, StreamEvent, Usage};

/// Ollama provider for local LLM inference
/// Supports the Ollama /api/generate and /api/chat endpoints
pub struct OllamaProvider {
    client: Client,
    base_url: String,
    model: String,
    first_token_timeout_ms: u64,
    stall_timeout_ms: u64,
    static_headers: Option<HashMap<String, String>>,
}

impl OllamaProvider {
    /// Create an Ollama provider from ModelConfig
    pub fn from_config(cfg: &ModelConfig) -> Result<Self, ProviderError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(300)) // Ollama can be slow on first load
            .build()
            .map_err(|e| ProviderError::Network {
                message: format!("Failed to create HTTP client: {}", e),
            })?;

        Ok(Self {
            client,
            base_url: cfg
                .api_base
                .clone()
                .unwrap_or_else(|| "http://localhost:11434".to_string()),
            model: cfg.model.clone(),
            first_token_timeout_ms: 60_000, // Longer for local models
            stall_timeout_ms: 30_000,
            static_headers: None, // ModelConfig doesn't have headers field yet
        })
    }

    fn build_messages(&self, req: &GenerateRequest) -> Vec<JsonValue> {
        let mut out = Vec::new();

        // Add system message if present
        if let Some(sys) = &req.system {
            if !sys.is_empty() {
                out.push(json!({
                    "role": "system",
                    "content": sys
                }));
            }
        }

        // Add user and assistant messages
        for m in &req.messages {
            let role = match m.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "assistant", // Ollama doesn't have a dedicated tool role
            };

            // Concatenate text parts (Ollama doesn't support multi-part content in the same way)
            let text = m
                .content
                .iter()
                .filter_map(|p| match p {
                    ContentPart::Text { text } => Some(text.as_str()),
                    ContentPart::ImageUrl { .. } => None, // TODO: Add image support later
                })
                .collect::<Vec<_>>()
                .join(" ");

            out.push(json!({
                "role": role,
                "content": text
            }));
        }

        out
    }

    fn apply_headers(
        &self,
        mut rb: reqwest::RequestBuilder,
        req: &GenerateRequest,
    ) -> reqwest::RequestBuilder {
        rb = rb.header("Content-Type", "application/json");

        if let Some(h) = &self.static_headers {
            for (k, v) in h {
                rb = rb.header(k, v);
            }
        }

        if let Some(meta) = &req.metadata {
            for (k, v) in meta {
                if k.eq_ignore_ascii_case("content-type") {
                    continue;
                }
                rb = rb.header(k, v);
            }
        }

        rb
    }

    fn map_http_error(status: u16, body: String) -> ProviderError {
        let lower = body.to_lowercase();
        if status == 404 {
            ProviderError::InvalidParams {
                details: Some(body),
                code: None,
                message: "Model not found in Ollama. Try running 'ollama pull <model>'".to_string(),
                status: Some(status),
            }
        } else if status >= 500 {
            ProviderError::ServerError {
                details: Some(body),
                code: None,
                message: "Ollama server error".to_string(),
                status: Some(status),
            }
        } else if lower.contains("invalid") || lower.contains("error") {
            ProviderError::InvalidParams {
                details: Some(body),
                code: None,
                message: "Invalid parameters for Ollama".to_string(),
                status: Some(status),
            }
        } else {
            ProviderError::ProviderOutage {
                details: Some(body),
                code: None,
                message: "Ollama error".to_string(),
                status: Some(status),
            }
        }
    }
}

#[derive(Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<JsonValue>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
}

#[derive(Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct OllamaChatResponse {
    message: OllamaMessage,
    done: bool,
    #[serde(default)]
    prompt_eval_count: Option<u64>,
    #[serde(default)]
    eval_count: Option<u64>,
}

#[derive(Deserialize)]
struct OllamaMessage {
    content: String,
}

#[derive(Deserialize)]
struct OllamaStreamChunk {
    message: Option<OllamaMessage>,
    done: bool,
    #[serde(default)]
    prompt_eval_count: Option<u64>,
    #[serde(default)]
    eval_count: Option<u64>,
}

#[async_trait]
impl crate::providers::Provider for OllamaProvider {
    async fn generate(&self, req: GenerateRequest) -> Result<GenerateResponse, ProviderError> {
        let url = format!("{}/api/chat", self.base_url.trim_end_matches('/'));

        // Build options
        let options = OllamaOptions {
            num_predict: req.max_output_tokens.map(|t| t as i32),
            temperature: req.temperature,
            top_p: req.top_p,
            stop: req.stop.clone(),
        };

        // Only include options if at least one field is set
        let options_to_send = if options.num_predict.is_some()
            || options.temperature.is_some()
            || options.top_p.is_some()
            || options.stop.is_some()
        {
            Some(options)
        } else {
            None
        };

        let payload = OllamaChatRequest {
            model: self.model.clone(),
            messages: self.build_messages(&req),
            stream: false,
            options: options_to_send,
        };

        // Send request
        let rb = self.client.post(&url);
        let rb = self.apply_headers(rb, &req);
        let resp = rb
            .json(&payload)
            .send()
            .await
            .map_err(|e| ProviderError::Network {
                message: format!("Ollama network error: {}", e),
            })?;

        let status = resp.status().as_u16();
        let text = resp.text().await.map_err(|e| ProviderError::Network {
            message: format!("Failed reading Ollama response: {}", e),
        })?;

        if !(200..300).contains(&status) {
            return Err(Self::map_http_error(status, text));
        }

        // Parse JSON
        let ollama_response: OllamaChatResponse =
            serde_json::from_str(&text).map_err(|e| ProviderError::Network {
                message: format!("Invalid JSON from Ollama: {}", e),
            })?;

        // Calculate usage if available
        let usage = if ollama_response.prompt_eval_count.is_some()
            || ollama_response.eval_count.is_some()
        {
            Some(Usage {
                prompt_tokens: ollama_response.prompt_eval_count.map(|c| c as u32),
                completion_tokens: ollama_response.eval_count.map(|c| c as u32),
                total_tokens: match (
                    ollama_response.prompt_eval_count,
                    ollama_response.eval_count,
                ) {
                    (Some(p), Some(c)) => Some((p + c) as u32),
                    _ => None,
                },
            })
        } else {
            None
        };

        Ok(GenerateResponse {
            text: ollama_response.message.content,
            tool_calls: vec![], // Ollama doesn't support tool calling in the same way
            usage,
            raw: serde_json::from_str(&text).ok(),
        })
    }

    async fn generate_streaming(
        &self,
        req: GenerateRequest,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<GenerateResponse, ProviderError> {
        let url = format!("{}/api/chat", self.base_url.trim_end_matches('/'));

        // Build options
        let options = OllamaOptions {
            num_predict: req.max_output_tokens.map(|t| t as i32),
            temperature: req.temperature,
            top_p: req.top_p,
            stop: req.stop.clone(),
        };

        let options_to_send = if options.num_predict.is_some()
            || options.temperature.is_some()
            || options.top_p.is_some()
            || options.stop.is_some()
        {
            Some(options)
        } else {
            None
        };

        let payload = OllamaChatRequest {
            model: self.model.clone(),
            messages: self.build_messages(&req),
            stream: true,
            options: options_to_send,
        };

        // Send request
        let rb = self.client.post(&url);
        let rb = self.apply_headers(rb, &req);
        let resp = rb
            .json(&payload)
            .send()
            .await
            .map_err(|e| ProviderError::Network {
                message: format!("Ollama network error: {}", e),
            })?;

        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let body = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("Could not read error body: {}", e));
            return Err(Self::map_http_error(status, body));
        }

        let mut stream = resp.bytes_stream();
        let mut content = String::new();
        let mut usage_info: Option<Usage> = None;

        let first_timeout = self.first_token_timeout_ms;
        let stall_timeout = self.stall_timeout_ms;
        let mut got_first = false;

        loop {
            let cur = if got_first {
                stall_timeout
            } else {
                first_timeout
            };

            match timeout(Duration::from_millis(cur), stream.next()).await {
                Ok(Some(Ok(bytes))) => {
                    let s = String::from_utf8_lossy(&bytes);

                    // Ollama streams newline-delimited JSON (not SSE)
                    for line in s.lines() {
                        let line = line.trim();
                        if line.is_empty() {
                            continue;
                        }

                        match serde_json::from_str::<OllamaStreamChunk>(line) {
                            Ok(chunk) => {
                                if let Some(msg) = &chunk.message {
                                    if !msg.content.is_empty() {
                                        content.push_str(&msg.content);
                                        on_event(StreamEvent::TextDelta(msg.content.clone()));
                                        got_first = true;
                                    }
                                }

                                if chunk.done {
                                    // Extract usage info from final chunk
                                    if chunk.prompt_eval_count.is_some()
                                        || chunk.eval_count.is_some()
                                    {
                                        usage_info = Some(Usage {
                                            prompt_tokens: chunk
                                                .prompt_eval_count
                                                .map(|c| c as u32),
                                            completion_tokens: chunk.eval_count.map(|c| c as u32),
                                            total_tokens: match (
                                                chunk.prompt_eval_count,
                                                chunk.eval_count,
                                            ) {
                                                (Some(p), Some(c)) => Some((p + c) as u32),
                                                _ => None,
                                            },
                                        });
                                        if let Some(u) = &usage_info {
                                            on_event(StreamEvent::Usage(u.clone()));
                                        }
                                    }
                                    on_event(StreamEvent::Finished);
                                }
                            }
                            Err(e) => {
                                warn!(
                                    "Failed to parse Ollama stream chunk: {} - line: {}",
                                    e, line
                                );
                            }
                        }
                    }
                }
                Ok(Some(Err(e))) => {
                    return Err(ProviderError::Network {
                        message: format!("Error reading streaming response: {}", e),
                    });
                }
                Ok(None) => break,
                Err(_) => {
                    if !got_first {
                        return Err(ProviderError::TimeoutFirstToken { timeout_ms: cur });
                    } else {
                        return Err(ProviderError::TimeoutStall { timeout_ms: cur });
                    }
                }
            }
        }

        Ok(GenerateResponse {
            text: content,
            tool_calls: vec![],
            usage: usage_info,
            raw: None,
        })
    }
}
