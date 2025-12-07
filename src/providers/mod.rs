// File: src/providers/mod.rs
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;

use crate::core::error::ProviderError;

pub mod anthropic;
pub mod openrouter;
/// Common metadata map for provider hints/headers
pub type Metadata = HashMap<String, String>;

/// Roles supported for canonical messages
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// Multi-part content (text or images)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text {
        text: String,
    },
    ImageUrl {
        url: String,
        #[serde(default)]
        mime: Option<String>,
    },
}

/// Canonical message (role + parts)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    /// Content parts for this message
    #[serde(default)]
    pub content: Vec<ContentPart>,
}

/// Tool choice semantics
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolChoice {
    Auto,
    Required,
    None,
}

/// Unified tool schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Optional JSON schema describing the tool arguments
    #[serde(default)]
    pub json_schema: Option<JsonValue>,
}

/// Normalized tool call emitted by providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallNormalized {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    /// Parsed, validated JSON for tool arguments (object)
    pub arguments_json: JsonValue,
}

/// Canonical usage counters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: Option<u32>,
    #[serde(default)]
    pub completion_tokens: Option<u32>,
    #[serde(default)]
    pub total_tokens: Option<u32>,
}

/// Canonical generate request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateRequest {
    #[serde(default)]
    pub system: Option<String>,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub tools: Option<Vec<ToolSpec>>,
    #[serde(default)]
    pub tool_choice: Option<ToolChoice>,

    // Sampling and controls
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub stop: Option<Vec<String>>,
    #[serde(default)]
    pub seed: Option<u64>,
    #[serde(default)]
    pub logit_bias: Option<HashMap<String, f32>>,
    #[serde(default)]
    pub response_format: Option<String>,

    /// Canonical field to cap output tokens; providers map to their own keys
    #[serde(default)]
    pub max_output_tokens: Option<usize>,

    /// Optional provider metadata (headers, extra flags)
    #[serde(default)]
    pub metadata: Option<Metadata>,
}

/// Canonical generate response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateResponse {
    /// Final text (provider-joined) for convenience; callers may also parse raw
    pub text: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallNormalized>,
    #[serde(default)]
    pub usage: Option<Usage>,
    /// Provider raw JSON for diagnostics
    #[serde(default)]
    pub raw: Option<JsonValue>,
}

/// Unified streaming event model
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum StreamEvent {
    TextDelta(String),
    ToolDelta(String),
    ToolCall(ToolCallNormalized),
    Usage(Usage),
    Finished,
    Error(String),
}

/// Provider trait for canonical generate APIs
#[async_trait]
pub trait Provider: Send + Sync {
    async fn generate(&self, req: GenerateRequest) -> Result<GenerateResponse, ProviderError>;

    /// Streaming generation. Implementations should:
    /// - invoke `on_event` for each StreamEvent
    /// - return final GenerateResponse when done (with full text)
    async fn generate_streaming(
        &self,
        req: GenerateRequest,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<GenerateResponse, ProviderError>;
}

/// Mapping helpers (RFC: canonical -> provider requests)
pub fn map_internal_to_openai_chat(req: &GenerateRequest) -> JsonValue {
    // Messages mapping: flatten text; images skipped for now
    let mut messages: Vec<JsonValue> = Vec::new();
    if let Some(sys) = &req.system {
        messages.push(json!({"role":"system","content": sys}));
    }
    for m in &req.messages {
        let text = m
            .content
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                ContentPart::ImageUrl { .. } => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        let role = match m.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };
        messages.push(json!({"role": role, "content": text}));
    }

    json!({
        "messages": messages,
        "max_tokens": req.max_output_tokens.unwrap_or(1024),
        "temperature": req.temperature.unwrap_or(0.7),
    })
}

pub fn map_internal_to_openai_responses(req: &GenerateRequest) -> JsonValue {
    let user_text = {
        let mut buf = String::new();
        if let Some(sys) = &req.system {
            buf.push_str(sys);
            buf.push('\n');
        }
        for m in &req.messages {
            let prefix = match m.role {
                Role::System => "[system] ",
                Role::User => "[user] ",
                Role::Assistant => "[assistant] ",
                Role::Tool => "[tool] ",
            };
            let t = m
                .content
                .iter()
                .filter_map(|p| match p {
                    ContentPart::Text { text } => Some(text.as_str()),
                    ContentPart::ImageUrl { .. } => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            buf.push_str(prefix);
            buf.push_str(&t);
            buf.push('\n');
        }
        buf
    };

    json!({
        "input": user_text,
        "max_output_tokens": req.max_output_tokens.unwrap_or(1024),
        "temperature": req.temperature.unwrap_or(0.7),
    })
}

pub fn map_internal_to_anthropic(req: &GenerateRequest) -> JsonValue {
    let mut messages: Vec<JsonValue> = Vec::new();
    // Anthropic treats system special; newer API can include it separately
    if let Some(sys) = &req.system {
        messages.push(json!({"role": "system", "content": sys}));
    }
    for m in &req.messages {
        let text = m
            .content
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                ContentPart::ImageUrl { .. } => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        let role = match m.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };
        messages.push(json!({"role": role, "content": text}));
    }

    json!({
        "messages": messages,
        "max_tokens": req.max_output_tokens.unwrap_or(1024),
        "temperature": req.temperature.unwrap_or(0.7),
    })
}

pub fn map_internal_to_openrouter(req: &GenerateRequest) -> JsonValue {
    // OpenRouter is primarily OpenAI-chat compatible
    map_internal_to_openai_chat(req)
}

/// Backpressure-safe SSE decoder (very simple)
pub struct SseDecoder {
    buffer: String,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }

    /// Push raw chunk; returns complete "data: ..." lines (without prefix)
    pub fn push_chunk(&mut self, chunk: &str) -> Vec<String> {
        self.buffer.push_str(chunk);
        let mut out = Vec::new();
        let mut start = 0usize;

        while let Some(idx) = self.buffer[start..].find('\n') {
            let line_end = start + idx;
            let line = &self.buffer[start..line_end];
            let line = line.trim_end_matches('\r');
            if let Some(json_str) = line.strip_prefix("data: ") {
                if json_str != "[DONE]" {
                    out.push(json_str.to_string());
                }
            }
            start = line_end + 1;
        }

        // retain leftover (partial line)
        let leftover = self.buffer[start..].to_string();
        self.buffer.clear();
        self.buffer.push_str(&leftover);
        out
    }
}

impl Default for SseDecoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a single OpenAI-Chat SSE JSON line into a StreamEvent::TextDelta if present
pub fn parse_openai_chat_sse(json_line: &str) -> Option<StreamEvent> {
    if let Ok(v) = serde_json::from_str::<JsonValue>(json_line) {
        if let Some(delta) = v
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("delta"))
            .and_then(|d| d.get("content"))
            .and_then(|c| c.as_str())
        {
            return Some(StreamEvent::TextDelta(delta.to_string()));
        }
    }
    None
}

/// Parse a single OpenAI-Responses SSE line (output_text.delta etc.)
pub fn parse_openai_responses_sse(json_line: &str) -> Option<StreamEvent> {
    if let Ok(v) = serde_json::from_str::<JsonValue>(json_line) {
        if let Some(t) = v.get("type").and_then(|t| t.as_str()) {
            if t.contains("output_text.delta") {
                if let Some(delta) = v.get("delta").and_then(|d| d.as_str()) {
                    return Some(StreamEvent::TextDelta(delta.to_string()));
                }
            }
        }
    }
    None
}
