// File: src/providers/openrouter.rs
use async_trait::async_trait;
use futures_util::StreamExt;
use log::debug;
use reqwest::Client;
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::timeout;

use crate::core::config::LlmConfig;
use crate::core::error::ProviderError;
use crate::providers::{
    ContentPart, GenerateRequest, GenerateResponse, Role, SseDecoder, StreamEvent,
    ToolCallNormalized, ToolChoice, ToolSpec, Usage,
};

/// OpenRouter adapter using OpenAI-style /chat/completions by default.
/// Includes fallback to Responses-style tokens when an unsupported-parameter
/// error is detected in the response body.
pub struct OpenRouterProvider {
    client: Client,
    api_key: String,
    api_base: String,
    model: String,
    headers: Option<HashMap<String, String>>,
    first_token_timeout_ms: u64,
    stall_timeout_ms: u64,
}

impl OpenRouterProvider {
    pub fn from_config(cfg: &LlmConfig) -> Result<Self, ProviderError> {
        // Prefer explicit api_key if present, else env OPENROUTER_API_KEY
        let api_key = if !cfg.api_key.is_empty() {
            cfg.api_key.clone()
        } else {
            std::env::var("OPENROUTER_API_KEY").map_err(|_| ProviderError::Auth {
                details: None,
                code: None,
                message: "Missing OPENROUTER_API_KEY".to_string(),
                status: None,
            })?
        };

        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| ProviderError::Network {
                message: format!("Failed to create HTTP client: {}", e),
            })?;

        Ok(Self {
            client,
            api_key,
            api_base: cfg
                .api_base
                .clone()
                .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string()),
            model: cfg.model.clone(),
            headers: cfg.headers.clone(),
            first_token_timeout_ms: cfg.first_token_timeout_ms.unwrap_or(30_000),
            stall_timeout_ms: cfg.stall_timeout_ms.unwrap_or(10_000),
        })
    }

    fn build_url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.api_base.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    fn map_messages(req: &GenerateRequest) -> Vec<JsonValue> {
        // OpenAI-style messages: include system as a dedicated message when present
        let mut msgs: Vec<JsonValue> = Vec::new();
        if let Some(sys) = &req.system {
            msgs.push(json!({"role":"system","content": sys}));
        }
        for m in &req.messages {
            let role = match m.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
            };
            // Concatenate text parts; ignore images in chat format
            let text = m
                .content
                .iter()
                .filter_map(|p| match p {
                    ContentPart::Text { text } => Some(text.as_str()),
                    ContentPart::ImageUrl { .. } => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            msgs.push(json!({"role": role, "content": text}));
        }
        msgs
    }

    fn map_tools_openai(tools: &Option<Vec<ToolSpec>>) -> Option<Vec<JsonValue>> {
        tools.as_ref().map(|v| {
            v.iter()
                .map(|t| {
                    let params = t
                        .json_schema
                        .clone()
                        .unwrap_or_else(|| json!({"type":"object","properties":{}}));
                    json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description.clone().unwrap_or_default(),
                            "parameters": params
                        }
                    })
                })
                .collect()
        })
    }

    fn map_tool_choice_openai(choice: &Option<ToolChoice>) -> Option<JsonValue> {
        match choice {
            Some(ToolChoice::Auto) => Some(json!("auto")),
            Some(ToolChoice::Required) => Some(json!("required")),
            Some(ToolChoice::None) => Some(json!("none")),
            None => None,
        }
    }

    fn apply_headers(
        &self,
        mut rb: reqwest::RequestBuilder,
        req: &GenerateRequest,
        sse: bool,
    ) -> reqwest::RequestBuilder {
        rb = rb
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json");
        if sse {
            rb = rb.header("Accept", "text/event-stream");
        }
        // Forward configured headers (HTTP-Referer, X-Title, etc.)
        if let Some(h) = &self.headers {
            for (k, v) in h {
                rb = rb.header(k, v);
            }
        }
        // Forward request metadata headers if present
        if let Some(meta) = &req.metadata {
            for (k, v) in meta {
                if k.eq_ignore_ascii_case("authorization")
                    || k.eq_ignore_ascii_case("content-type")
                    || k.eq_ignore_ascii_case("accept")
                {
                    continue;
                }
                rb = rb.header(k, v);
            }
        }
        rb
    }

    fn parse_usage_openai(v: &JsonValue) -> Option<Usage> {
        let prompt_tokens = v
            .get("prompt_tokens")
            .and_then(|x| x.as_u64())
            .map(|x| x as u32);
        let completion_tokens = v
            .get("completion_tokens")
            .and_then(|x| x.as_u64())
            .map(|x| x as u32);
        let total_tokens = v
            .get("total_tokens")
            .and_then(|x| x.as_u64())
            .map(|x| x as u32)
            .or_else(|| match (prompt_tokens, completion_tokens) {
                (Some(p), Some(c)) => Some(p + c),
                _ => None,
            });
        Some(Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        })
    }

    fn normalize_tool_calls(choice: &JsonValue) -> Vec<ToolCallNormalized> {
        let mut out = Vec::new();
        if let Some(tool_calls) = choice.get("message").and_then(|m| m.get("tool_calls")) {
            if let Some(arr) = tool_calls.as_array() {
                for tc in arr {
                    let id = tc.get("id").and_then(|x| x.as_str()).map(|s| s.to_string());
                    let name = tc
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("function")
                        .to_string();
                    let args_str = tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("{}");
                    let args_json: JsonValue =
                        serde_json::from_str(args_str).unwrap_or_else(|_| json!({}));
                    out.push(ToolCallNormalized {
                        id,
                        name,
                        arguments_json: args_json,
                    });
                }
            }
        }
        out
    }

    fn map_http_error(status: u16, body: String) -> ProviderError {
        let lower = body.to_lowercase();
        if status == 401 || status == 403 {
            ProviderError::Auth {
                details: Some(body),
                code: None,
                message: "Authentication failed for OpenRouter".to_string(),
                status: Some(status),
            }
        } else if status == 429 {
            ProviderError::RateLimited {
                details: Some(body),
                code: None,
                message: "Rate limited by OpenRouter".to_string(),
                status: Some(status),
                retry_after_ms: None,
            }
        } else if status >= 500 {
            ProviderError::ServerError {
                details: Some(body),
                code: None,
                message: "OpenRouter server error".to_string(),
                status: Some(status),
            }
        } else if lower.contains("unsupported parameter") || lower.contains("invalid") {
            ProviderError::InvalidParams {
                details: Some(body),
                code: None,
                message: "Invalid parameters for OpenRouter".to_string(),
                status: Some(status),
            }
        } else {
            ProviderError::ProviderOutage {
                details: Some(body),
                code: None,
                message: "OpenRouter error".to_string(),
                status: Some(status),
            }
        }
    }
}

#[async_trait]
impl crate::providers::Provider for OpenRouterProvider {
    async fn generate(&self, req: GenerateRequest) -> Result<GenerateResponse, ProviderError> {
        let url = self.build_url("chat/completions");

        // Base OpenAI-style payload
        let mut payload = json!({
            "model": self.model,
            "messages": Self::map_messages(&req),
            "max_tokens": req.max_output_tokens.unwrap_or(1024),
            "temperature": req.temperature.unwrap_or(0.7),
        });

        // Optional sampling and stops
        if let Some(tp) = req.top_p {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("top_p".to_string(), json!(tp));
            }
        }
        if let Some(stops) = &req.stop {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("stop".to_string(), json!(stops));
            }
        }

        // Tools and tool_choice
        if let Some(t) = Self::map_tools_openai(&req.tools) {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("tools".to_string(), json!(t));
            }
        }
        if let Some(tc) = Self::map_tool_choice_openai(&req.tool_choice) {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("tool_choice".to_string(), tc);
            }
        }

        // Send request
        let rb = self.client.post(&url);
        let rb = self.apply_headers(rb, &req, false);
        let resp = rb
            .json(&payload)
            .send()
            .await
            .map_err(|e| ProviderError::Network {
                message: format!("OpenRouter network error: {}", e),
            })?;

        let status = resp.status().as_u16();
        let text = resp.text().await.map_err(|e| ProviderError::Network {
            message: format!("Failed reading OpenRouter response: {}", e),
        })?;

        if !(200..300).contains(&status) {
            // Fallback for Responses-style token fields:
            // if the body indicates 'max_output_tokens' or 'max_completion_tokens' required,
            // retry once with the alternate field in the same endpoint.
            let lower = text.to_lowercase();
            let mut retried_payload = payload.clone();
            let mut did_retry = false;

            if status == 400 && lower.contains("max_output_tokens") {
                if let Some(obj) = retried_payload.as_object_mut() {
                    let v = obj
                        .remove("max_tokens")
                        .unwrap_or(json!(req.max_output_tokens.unwrap_or(1024)));
                    obj.insert("max_output_tokens".to_string(), v);
                }
                did_retry = true;
            } else if status == 400 && lower.contains("max_completion_tokens") {
                if let Some(obj) = retried_payload.as_object_mut() {
                    let v = obj
                        .remove("max_tokens")
                        .unwrap_or(json!(req.max_output_tokens.unwrap_or(1024)));
                    obj.insert("max_completion_tokens".to_string(), v);
                }
                did_retry = true;
            }

            if did_retry {
                let rb2 = self.client.post(&url);
                let rb2 = self.apply_headers(rb2, &req, false);
                let resp2 = rb2.json(&retried_payload).send().await.map_err(|e| {
                    ProviderError::Network {
                        message: format!("OpenRouter retry network error: {}", e),
                    }
                })?;
                let status2 = resp2.status().as_u16();
                let text2 = resp2.text().await.map_err(|e| ProviderError::Network {
                    message: format!("Failed reading OpenRouter retry response: {}", e),
                })?;
                if !(200..300).contains(&status2) {
                    return Err(Self::map_http_error(status2, text2));
                } else {
                    // parse success below using text2
                    let v: JsonValue =
                        serde_json::from_str(&text2).map_err(|e| ProviderError::Network {
                            message: format!("Invalid JSON from OpenRouter retry: {}", e),
                        })?;
                    // Extract text and tool calls
                    let content = v
                        .get("choices")
                        .and_then(|c| c.get(0))
                        .and_then(|c| c.get("message"))
                        .and_then(|m| m.get("content"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();

                    let tool_calls = v
                        .get("choices")
                        .and_then(|c| c.get(0))
                        .map(Self::normalize_tool_calls)
                        .unwrap_or_default();

                    let usage = v.get("usage").and_then(Self::parse_usage_openai);

                    return Ok(GenerateResponse {
                        text: content,
                        tool_calls,
                        usage,
                        raw: Some(v),
                    });
                }
            }

            return Err(Self::map_http_error(status, text));
        }

        let v: JsonValue = serde_json::from_str(&text).map_err(|e| ProviderError::Network {
            message: format!("Invalid JSON from OpenRouter: {}", e),
        })?;

        let content = v
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();

        let tool_calls = v
            .get("choices")
            .and_then(|c| c.get(0))
            .map(Self::normalize_tool_calls)
            .unwrap_or_default();

        let usage = v.get("usage").and_then(Self::parse_usage_openai);

        Ok(GenerateResponse {
            text: content,
            tool_calls,
            usage,
            raw: Some(v),
        })
    }

    async fn generate_streaming(
        &self,
        req: GenerateRequest,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<GenerateResponse, ProviderError> {
        let url = self.build_url("chat/completions");

        let mut payload = json!({
            "model": self.model,
            "messages": Self::map_messages(&req),
            "max_tokens": req.max_output_tokens.unwrap_or(1024),
            "temperature": req.temperature.unwrap_or(0.7),
            "stream": true
        });

        if let Some(tp) = req.top_p {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("top_p".to_string(), json!(tp));
            }
        }
        if let Some(stops) = &req.stop {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("stop".to_string(), json!(stops));
            }
        }
        if let Some(t) = Self::map_tools_openai(&req.tools) {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("tools".to_string(), json!(t));
            }
        }
        if let Some(tc) = Self::map_tool_choice_openai(&req.tool_choice) {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("tool_choice".to_string(), tc);
            }
        }

        // Send request
        let rb = self.client.post(&url);
        let rb = self.apply_headers(rb, &req, true);
        let mut resp = rb
            .json(&payload)
            .send()
            .await
            .map_err(|e| ProviderError::Network {
                message: format!("OpenRouter network error: {}", e),
            })?;

        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let body = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("Could not read error body: {}", e));
            // One-time retry with Responses-style tokens if applicable (same endpoint)
            let lower = body.to_lowercase();
            if status == 400
                && (lower.contains("max_output_tokens") || lower.contains("max_completion_tokens"))
            {
                // Rebuild payload accordingly
                let mut retry_payload = payload.clone();
                if let Some(obj) = retry_payload.as_object_mut() {
                    if lower.contains("max_completion_tokens") {
                        let v = obj
                            .remove("max_tokens")
                            .unwrap_or(json!(req.max_output_tokens.unwrap_or(1024)));
                        obj.insert("max_completion_tokens".to_string(), v);
                    } else {
                        let v = obj
                            .remove("max_tokens")
                            .unwrap_or(json!(req.max_output_tokens.unwrap_or(1024)));
                        obj.insert("max_output_tokens".to_string(), v);
                    }
                }

                let rb2 = self.client.post(&url);
                let rb2 = self.apply_headers(rb2, &req, true);
                resp =
                    rb2.json(&retry_payload)
                        .send()
                        .await
                        .map_err(|e| ProviderError::Network {
                            message: format!("OpenRouter retry network error: {}", e),
                        })?;
                let status2 = resp.status().as_u16();
                if !(200..300).contains(&status2) {
                    let body2 = resp
                        .text()
                        .await
                        .unwrap_or_else(|e| format!("Could not read error body: {}", e));
                    return Err(Self::map_http_error(status2, body2));
                }
            } else {
                return Err(Self::map_http_error(status, body));
            }
        }

        let mut stream = resp.bytes_stream();
        let mut content = String::new();
        let mut decoder = SseDecoder::new();

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
                Ok(Some(Ok(chunk))) => {
                    let s = String::from_utf8_lossy(&chunk);
                    for data_line in decoder.push_chunk(&s) {
                        // Parse as OpenAI-chat SSE
                        if let Some(ev) = crate::providers::parse_openai_chat_sse(&data_line) {
                            if let StreamEvent::TextDelta(d) = &ev {
                                content.push_str(d);
                                got_first = true;
                            }
                            on_event(ev);
                        } else {
                            // Not a standard delta; ignore but keep debug
                            debug!("Unhandled OpenRouter SSE line: {}", data_line);
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
            tool_calls: vec![], // tool calls are not emitted in text delta path; final choices parsing not available in SSE
            usage: None,
            raw: None,
        })
    }
}
