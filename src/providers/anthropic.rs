// File: src/providers/anthropic.rs
use async_trait::async_trait;
use futures_util::StreamExt;
use log::{debug, warn};
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

/// Anthropic Messages API adapter implementing the canonical Provider contracts
pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    model: String,
    api_base: String, // e.g., https://api.anthropic.com/v1
    first_token_timeout_ms: u64,
    stall_timeout_ms: u64,
    static_headers: Option<HashMap<String, String>>,
}

impl AnthropicProvider {
    /// Create an Anthropic provider from LlmConfig (reads ANTHROPIC_API_KEY when api_key is empty)
    pub fn from_config(cfg: &LlmConfig) -> Result<Self, ProviderError> {
        let api_key = if !cfg.api_key.is_empty() {
            cfg.api_key.clone()
        } else {
            std::env::var("ANTHROPIC_API_KEY").map_err(|_| ProviderError::Auth {
                details: None,
                code: None,
                message: "Missing ANTHROPIC_API_KEY".to_string(),
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
            model: cfg.model.clone(),
            api_base: cfg
                .api_base
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com/v1".to_string()),
            first_token_timeout_ms: cfg.first_token_timeout_ms.unwrap_or(30_000),
            stall_timeout_ms: cfg.stall_timeout_ms.unwrap_or(10_000),
            static_headers: cfg.headers.clone(),
        })
    }

    fn build_messages(&self, req: &GenerateRequest) -> Vec<JsonValue> {
        let mut out = Vec::new();
        for m in &req.messages {
            // Anthropic supports "user" and "assistant" roles
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::System => {
                    // We'll rely on top-level "system" for system content
                    // If a message arrives with System role, downgrade to 'user' to preserve content
                    "user"
                }
                Role::Tool => {
                    // No native "tool" role in Anthropic content; treat as assistant echo
                    "assistant"
                }
            };
            let mut content: Vec<JsonValue> = Vec::new();
            for part in &m.content {
                match part {
                    ContentPart::Text { text } => {
                        content.push(json!({"type":"text","text": text}));
                    }
                    ContentPart::ImageUrl { url, mime } => {
                        // Anthropic multi-modal: image via URL source
                        let media_type = mime.clone().unwrap_or_else(|| "image/*".to_string());
                        content.push(json!({
                            "type": "image",
                            "source": {
                                "type": "url",
                                "url": url,
                                "media_type": media_type
                            }
                        }));
                    }
                }
            }
            // If no parts provided, still send empty text node to be safe
            if content.is_empty() {
                content.push(json!({"type":"text","text": ""}));
            }
            out.push(json!({"role": role, "content": content}));
        }
        out
    }

    fn map_tools(tools: &Option<Vec<ToolSpec>>) -> Option<Vec<JsonValue>> {
        tools.as_ref().map(|v| {
            v.iter()
                .map(|t| {
                    let schema = t
                        .json_schema
                        .clone()
                        .unwrap_or_else(|| json!({"type":"object","properties":{}}));
                    json!({
                        "name": t.name,
                        "description": t.description.clone().unwrap_or_default(),
                        "input_schema": schema
                    })
                })
                .collect()
        })
    }

    fn map_tool_choice(choice: &Option<ToolChoice>) -> Option<JsonValue> {
        match choice {
            Some(ToolChoice::Auto) => Some(json!("auto")),
            Some(ToolChoice::Required) => {
                // Anthropic "any" forces a tool call
                Some(json!("any"))
            }
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
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json");
        if sse {
            rb = rb.header("Accept", "text/event-stream");
        }
        if let Some(h) = &self.static_headers {
            for (k, v) in h {
                rb = rb.header(k, v);
            }
        }
        if let Some(meta) = &req.metadata {
            for (k, v) in meta {
                // do not clobber auth/version headers
                if k.eq_ignore_ascii_case("x-api-key")
                    || k.eq_ignore_ascii_case("anthropic-version")
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

    fn map_http_error(status: u16, body: String) -> ProviderError {
        let lower = body.to_lowercase();
        if status == 401 || status == 403 {
            ProviderError::Auth {
                details: Some(body),
                code: None,
                message: "Authentication failed for Anthropic".to_string(),
                status: Some(status),
            }
        } else if status == 429 {
            // Extract retry-after if present is non-trivial without headers here; keep None
            ProviderError::RateLimited {
                details: Some(body),
                code: None,
                message: "Rate limited by Anthropic".to_string(),
                status: Some(status),
                retry_after_ms: None,
            }
        } else if status >= 500 {
            ProviderError::ServerError {
                details: Some(body),
                code: None,
                message: "Anthropic server error".to_string(),
                status: Some(status),
            }
        } else if lower.contains("unsupported parameter") || lower.contains("invalid") {
            ProviderError::InvalidParams {
                details: Some(body),
                code: None,
                message: "Invalid parameters for Anthropic".to_string(),
                status: Some(status),
            }
        } else {
            ProviderError::ProviderOutage {
                details: Some(body),
                code: None,
                message: "Anthropic error".to_string(),
                status: Some(status),
            }
        }
    }

    fn parse_usage(v: &JsonValue) -> Option<Usage> {
        let prompt_tokens = v
            .get("input_tokens")
            .and_then(|x| x.as_u64())
            .map(|x| x as u32);
        let completion_tokens = v
            .get("output_tokens")
            .and_then(|x| x.as_u64())
            .map(|x| x as u32);
        let total_tokens = match (prompt_tokens, completion_tokens) {
            (Some(p), Some(c)) => Some(p + c),
            _ => None,
        };
        Some(Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        })
    }
}

#[async_trait]
impl crate::providers::Provider for AnthropicProvider {
    async fn generate(&self, req: GenerateRequest) -> Result<GenerateResponse, ProviderError> {
        let url = format!("{}/messages", self.api_base.trim_end_matches('/'));

        // Build payload
        let mut payload = json!({
            "model": self.model,
            "messages": self.build_messages(&req),
            "max_tokens": req.max_output_tokens.unwrap_or(1024),
        });

        // Optional "system"
        if let Some(sys) = &req.system {
            if !sys.is_empty() {
                if let Some(obj) = payload.as_object_mut() {
                    obj.insert("system".to_string(), JsonValue::String(sys.clone()));
                }
            }
        }

        // Sampling
        if let Some(t) = req.temperature {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("temperature".to_string(), json!(t));
            }
        }
        if let Some(tp) = req.top_p {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("top_p".to_string(), json!(tp));
            }
        }
        if let Some(stops) = &req.stop {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("stop_sequences".to_string(), json!(stops));
            }
        }

        // Tools
        if let Some(t) = Self::map_tools(&req.tools) {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("tools".to_string(), json!(t));
            }
        }
        if let Some(tc) = Self::map_tool_choice(&req.tool_choice) {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("tool_choice".to_string(), tc);
            }
        }

        // Send
        let rb = self.client.post(&url);
        let rb = self.apply_headers(rb, &req, false);
        let resp = rb
            .json(&payload)
            .send()
            .await
            .map_err(|e| ProviderError::Network {
                message: format!("Anthropic network error: {}", e),
            })?;

        let status = resp.status().as_u16();
        let text = resp.text().await.map_err(|e| ProviderError::Network {
            message: format!("Failed reading Anthropic response: {}", e),
        })?;

        if !(200..300).contains(&status) {
            return Err(Self::map_http_error(status, text));
        }

        // Parse JSON
        let v: JsonValue = serde_json::from_str(&text).map_err(|e| ProviderError::Network {
            message: format!("Invalid JSON from Anthropic: {}", e),
        })?;

        // Extract content text and tool calls
        let mut out_text = String::new();
        let mut tool_calls: Vec<ToolCallNormalized> = Vec::new();

        if let Some(arr) = v.get("content").and_then(|x| x.as_array()) {
            for block in arr {
                if let Some(t) = block.get("type").and_then(|x| x.as_str()) {
                    match t {
                        "text" => {
                            if let Some(s) = block.get("text").and_then(|x| x.as_str()) {
                                out_text.push_str(s);
                            }
                        }
                        "tool_use" => {
                            let id = block
                                .get("id")
                                .and_then(|x| x.as_str())
                                .map(|s| s.to_string());
                            let name = block
                                .get("name")
                                .and_then(|x| x.as_str())
                                .unwrap_or("tool")
                                .to_string();
                            let args = block.get("input").cloned().unwrap_or_else(|| json!({}));
                            tool_calls.push(ToolCallNormalized {
                                id,
                                name,
                                arguments_json: args,
                            });
                        }
                        _ => {}
                    }
                }
            }
        }

        let usage = v.get("usage").and_then(Self::parse_usage).or_else(|| {
            // Some variants place usage under "message.usage"
            v.get("message")
                .and_then(|m| m.get("usage"))
                .and_then(Self::parse_usage)
        });

        Ok(GenerateResponse {
            text: out_text,
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
        let url = format!("{}/messages", self.api_base.trim_end_matches('/'));

        // Build payload with stream flag
        let mut payload = json!({
            "model": self.model,
            "messages": self.build_messages(&req),
            "max_tokens": req.max_output_tokens.unwrap_or(1024),
            "stream": true,
        });

        if let Some(sys) = &req.system {
            if !sys.is_empty() {
                if let Some(obj) = payload.as_object_mut() {
                    obj.insert("system".to_string(), JsonValue::String(sys.clone()));
                }
            }
        }
        if let Some(t) = req.temperature {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("temperature".to_string(), json!(t));
            }
        }
        if let Some(tp) = req.top_p {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("top_p".to_string(), json!(tp));
            }
        }
        if let Some(stops) = &req.stop {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("stop_sequences".to_string(), json!(stops));
            }
        }
        if let Some(t) = Self::map_tools(&req.tools) {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("tools".to_string(), json!(t));
            }
        }
        if let Some(tc) = Self::map_tool_choice(&req.tool_choice) {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("tool_choice".to_string(), tc);
            }
        }

        // Send request
        let rb = self.client.post(&url);
        let rb = self.apply_headers(rb, &req, true);
        let resp = rb
            .json(&payload)
            .send()
            .await
            .map_err(|e| ProviderError::Network {
                message: format!("Anthropic network error: {}", e),
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
        let mut decoder = SseDecoder::new();
        let mut tool_calls: Vec<ToolCallNormalized> = Vec::new();

        let first_timeout = self.first_token_timeout_ms;
        let stall_timeout = self.stall_timeout_ms;
        let mut got_first = false;

        loop {
            let cur = if got_first {
                stall_timeout
            } else {
                first_timeout
            };
            let next = timeout(Duration::from_millis(cur), stream.next()).await;
            match next {
                Ok(Some(Ok(bytes))) => {
                    let s = String::from_utf8_lossy(&bytes);
                    for data_line in decoder.push_chunk(&s) {
                        // Each data_line is a JSON object per Anthropic SSE
                        if let Ok(v) = serde_json::from_str::<JsonValue>(&data_line) {
                            if let Some(t) = v.get("type").and_then(|x| x.as_str()) {
                                match t {
                                    "content_block_delta" => {
                                        // text delta appears as delta.text (type can be "text_delta")
                                        if let Some(delta) = v.get("delta") {
                                            if let Some(txt) =
                                                delta.get("text").and_then(|x| x.as_str())
                                            {
                                                content.push_str(txt);
                                                on_event(StreamEvent::TextDelta(txt.to_string()));
                                                got_first = true;
                                            }
                                        }
                                    }
                                    "content_block_start" => {
                                        // tool_use start may include id, name, input (possibly empty)
                                        if let Some(cb) = v.get("content_block") {
                                            if cb
                                                .get("type")
                                                .and_then(|x| x.as_str())
                                                .is_some_and(|k| k == "tool_use")
                                            {
                                                let id = cb
                                                    .get("id")
                                                    .and_then(|x| x.as_str())
                                                    .map(|s| s.to_string());
                                                let name = cb
                                                    .get("name")
                                                    .and_then(|x| x.as_str())
                                                    .unwrap_or("tool")
                                                    .to_string();
                                                let args = cb
                                                    .get("input")
                                                    .cloned()
                                                    .unwrap_or_else(|| json!({}));
                                                let tc = ToolCallNormalized {
                                                    id,
                                                    name,
                                                    arguments_json: args,
                                                };
                                                on_event(StreamEvent::ToolCall(tc.clone()));
                                                tool_calls.push(tc);
                                            }
                                        }
                                    }
                                    "message_stop" => {
                                        on_event(StreamEvent::Finished);
                                    }
                                    "message_delta" => {
                                        if let Some(u) = v.get("usage") {
                                            if let Some(usage) = Self::parse_usage(u) {
                                                on_event(StreamEvent::Usage(usage));
                                            }
                                        }
                                    }
                                    _ => {
                                        debug!("Unhandled Anthropic SSE event type: {}", t);
                                    }
                                }
                            }
                        } else {
                            warn!("Failed to parse Anthropic SSE data line");
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
                    // Timeout
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
            tool_calls,
            usage: None,
            raw: None,
        })
    }
}
