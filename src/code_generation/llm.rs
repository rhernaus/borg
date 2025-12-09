use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use serde_json::Value as JsonValue;
use std::collections::HashMap as StdHashMap;
use std::fs as StdFs;
use std::io::{self, Write};
use std::path::PathBuf as StdPathBuf;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::code_generation::llm_logging::LlmLogger;
use crate::core::config::{LlmConfig, LlmLoggingConfig, ReasoningEffort};
use crate::core::error::BorgError;
use crate::providers::ResponseFormat;

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

    /// Generate a text completion with structured output format
    /// Providers that support structured outputs (JSON mode, JSON schema) should override this.
    /// Default implementation ignores response_format and calls generate().
    async fn generate_with_format(
        &self,
        prompt: &str,
        max_tokens: Option<usize>,
        temperature: Option<f32>,
        _response_format: Option<ResponseFormat>,
    ) -> Result<String> {
        self.generate(prompt, max_tokens, temperature).await
    }

    /// Generate a text completion for the given prompt and stream the response to stdout
    async fn generate_streaming(
        &self,
        prompt: &str,
        max_tokens: Option<usize>,
        temperature: Option<f32>,
        print_tokens: bool,
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
            // OpenAI stays on legacy path for now (preserves CLI UX and existing behavior)
            "openai" => Ok(Box::new(OpenAiProvider::new(config, logging_config)?)),

            // Route Anthropic through unified providers module by default
            "anthropic" => {
                let inner = crate::providers::anthropic::AnthropicProvider::from_config(&config)
                    .map_err(|e| anyhow::anyhow!(BorgError::LlmApiError(e.to_string())))?;
                let logger = std::sync::Arc::new(LlmLogger::new(logging_config.clone())?);
                Ok(Box::new(UnifiedProvidersAdapter::new(
                    "Anthropic",
                    config,
                    logger,
                    Box::new(inner),
                )))
            }

            // Route OpenRouter through unified providers module by default
            "openrouter" => {
                let inner = crate::providers::openrouter::OpenRouterProvider::from_config(&config)
                    .map_err(|e| anyhow::anyhow!(BorgError::LlmApiError(e.to_string())))?;
                let logger = std::sync::Arc::new(LlmLogger::new(logging_config.clone())?);
                Ok(Box::new(UnifiedProvidersAdapter::new(
                    "OpenRouter",
                    config,
                    logger,
                    Box::new(inner),
                )))
            }

            // Prefer OpenRouter as a safe default when not pinned (empty/default marker)
            other => {
                if other.is_empty() || other == "default" {
                    let inner =
                        crate::providers::openrouter::OpenRouterProvider::from_config(&config)
                            .map_err(|e| anyhow::anyhow!(BorgError::LlmApiError(e.to_string())))?;
                    let logger = std::sync::Arc::new(LlmLogger::new(logging_config.clone())?);
                    Ok(Box::new(UnifiedProvidersAdapter::new(
                        "OpenRouter",
                        config,
                        logger,
                        Box::new(inner),
                    )))
                } else {
                    Err(anyhow::anyhow!(BorgError::ConfigError(format!(
                        "Unsupported LLM provider: {}",
                        other
                    ))))
                }
            }
        }
    }
}
// Adapter that bridges the canonical providers::Provider into the legacy LlmProvider interface.
// This preserves CLI UX while routing Anthropic and OpenRouter through the unified provider layer.
struct UnifiedProvidersAdapter {
    provider_name: &'static str,
    inner: Box<dyn crate::providers::Provider>,
    logger: std::sync::Arc<crate::code_generation::llm_logging::LlmLogger>,
    model: String,
    // Forwardable static headers for diagnostics (also forwarded via providers where applicable)
    static_metadata: Option<std::collections::HashMap<String, String>>,
}

impl UnifiedProvidersAdapter {
    fn new(
        provider_name: &'static str,
        cfg: crate::core::config::LlmConfig,
        logger: std::sync::Arc<crate::code_generation::llm_logging::LlmLogger>,
        inner: Box<dyn crate::providers::Provider>,
    ) -> Self {
        Self {
            provider_name,
            inner,
            logger,
            model: cfg.model.clone(),
            static_metadata: cfg.headers.clone(),
        }
    }

    fn build_request(
        &self,
        prompt: &str,
        max_tokens: Option<usize>,
        temperature: Option<f32>,
    ) -> crate::providers::GenerateRequest {
        self.build_request_with_format(prompt, max_tokens, temperature, None)
    }

    fn build_request_with_format(
        &self,
        prompt: &str,
        max_tokens: Option<usize>,
        temperature: Option<f32>,
        response_format: Option<ResponseFormat>,
    ) -> crate::providers::GenerateRequest {
        use crate::providers::{ContentPart, Message, Role};

        crate::providers::GenerateRequest {
            system: Some(
                "You are an AI assistant that helps with coding in Rust. You provide clear, concise, and correct code."
                    .to_string(),
            ),
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentPart::Text {
                    text: prompt.to_string(),
                }],
            }],
            tools: None,
            tool_choice: None,
            temperature,
            top_p: None,
            stop: None,
            seed: None,
            logit_bias: None,
            response_format,
            max_output_tokens: max_tokens.or(Some(1024)),
            metadata: self.static_metadata.clone(),
        }
    }
}

#[async_trait]
impl LlmProvider for UnifiedProvidersAdapter {
    async fn generate(
        &self,
        prompt: &str,
        max_tokens: Option<usize>,
        temperature: Option<f32>,
    ) -> Result<String> {
        // Structured request logging (redacted)
        self.logger
            .log_request(self.provider_name, &self.model, prompt)?;

        let start_time = std::time::Instant::now();
        let req = self.build_request(prompt, max_tokens, temperature);

        let out = self
            .inner
            .generate(req)
            .await
            .map_err(|e| anyhow::anyhow!(BorgError::LlmApiError(e.to_string())))?;

        let duration = start_time.elapsed().as_millis() as u64;
        self.logger
            .log_response(self.provider_name, &self.model, &out.text, duration)?;

        Ok(out.text)
    }

    async fn generate_streaming(
        &self,
        prompt: &str,
        max_tokens: Option<usize>,
        temperature: Option<f32>,
        print_tokens: bool,
    ) -> Result<String> {
        // Structured request logging (redacted)
        self.logger
            .log_request(self.provider_name, &self.model, prompt)?;

        let start_time = std::time::Instant::now();
        let req = self.build_request(prompt, max_tokens, temperature);

        let mut content = String::new();
        let mut stdout = std::io::stdout();

        // Bridge unified StreamEvent into legacy token printing/buffering
        let mut on_event = |ev: crate::providers::StreamEvent| {
            match ev {
                crate::providers::StreamEvent::TextDelta(delta) => {
                    content.push_str(&delta);
                    if print_tokens {
                        print!("{}", delta);
                        let _ = stdout.flush();
                    }
                }
                crate::providers::StreamEvent::Finished => {
                    // no-op; completion captured in content buffer
                }
                crate::providers::StreamEvent::Usage(_u) => {
                    // Optionally capture usage later
                }
                crate::providers::StreamEvent::ToolCall(_tc) => {
                    // Tool calls are not surfaced in legacy interface; kept internal
                }
                crate::providers::StreamEvent::ToolDelta(_s) => {}
                crate::providers::StreamEvent::Error(msg) => {
                    log::warn!(
                        "[{}:{}] streaming error event: {}",
                        self.provider_name,
                        self.model,
                        msg
                    );
                }
            }
        };

        let res = self
            .inner
            .generate_streaming(req, &mut on_event)
            .await
            .map_err(|e| anyhow::anyhow!(BorgError::LlmApiError(e.to_string())))?;

        if print_tokens {
            println!();
        }

        // Prefer the provider-returned text if present; else use our buffered content
        let final_text = if res.text.is_empty() {
            content
        } else {
            res.text
        };

        let duration = start_time.elapsed().as_millis() as u64;
        self.logger
            .log_response(self.provider_name, &self.model, &final_text, duration)?;

        Ok(final_text)
    }

    async fn generate_with_format(
        &self,
        prompt: &str,
        max_tokens: Option<usize>,
        temperature: Option<f32>,
        response_format: Option<ResponseFormat>,
    ) -> Result<String> {
        // Structured request logging (redacted)
        self.logger
            .log_request(self.provider_name, &self.model, prompt)?;

        let start_time = std::time::Instant::now();
        let req = self.build_request_with_format(prompt, max_tokens, temperature, response_format);

        let out = self
            .inner
            .generate(req)
            .await
            .map_err(|e| anyhow::anyhow!(BorgError::LlmApiError(e.to_string())))?;

        let duration = start_time.elapsed().as_millis() as u64;
        self.logger
            .log_response(self.provider_name, &self.model, &out.text, duration)?;

        Ok(out.text)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpenAiEndpointPref {
    Chat,
    ResponsesMaxOutput,
    ResponsesMaxCompletion,
}

static OPENAI_ENDPOINT_CACHE: OnceLock<Mutex<StdHashMap<String, OpenAiEndpointPref>>> =
    OnceLock::new();
const OPENAI_CACHE_FILE: &str = "./logs/llm/openai_endpoint_cache.json";

fn openai_cache() -> &'static Mutex<StdHashMap<String, OpenAiEndpointPref>> {
    OPENAI_ENDPOINT_CACHE.get_or_init(|| Mutex::new(StdHashMap::new()))
}

fn load_openai_cache_from_disk() {
    let path = StdPathBuf::from(OPENAI_CACHE_FILE);
    if let Ok(bytes) = StdFs::read(&path) {
        if let Ok(map) = serde_json::from_slice::<StdHashMap<String, String>>(&bytes) {
            let mut guard = openai_cache().lock().unwrap();
            for (model, pref) in map {
                let v = match pref.as_str() {
                    "ResponsesMaxCompletion" => OpenAiEndpointPref::ResponsesMaxCompletion,
                    "ResponsesMaxOutput" => OpenAiEndpointPref::ResponsesMaxOutput,
                    _ => OpenAiEndpointPref::Chat,
                };
                guard.insert(model, v);
            }
        }
    }
}

fn persist_openai_cache_to_disk() {
    // Best-effort persistence
    let guard = openai_cache().lock().unwrap();
    let mut map: StdHashMap<String, String> = StdHashMap::new();
    for (k, v) in guard.iter() {
        let s = match v {
            OpenAiEndpointPref::Chat => "Chat",
            OpenAiEndpointPref::ResponsesMaxOutput => "ResponsesMaxOutput",
            OpenAiEndpointPref::ResponsesMaxCompletion => "ResponsesMaxCompletion",
        };
        map.insert(k.clone(), s.to_string());
    }
    if let Ok(json) = serde_json::to_vec_pretty(&map) {
        let _ = StdFs::create_dir_all("./logs/llm");
        let _ = StdFs::write(OPENAI_CACHE_FILE, json);
    }
}

/// OpenAI API provider
pub struct OpenAiProvider {
    api_key: String,
    model: String,
    api_base: String,
    client: Client,
    logger: Arc<LlmLogger>,
    enable_thinking: Option<bool>,
    reasoning_effort: Option<ReasoningEffort>,
    reasoning_budget_tokens: Option<usize>,
    first_token_timeout_ms: Option<u64>,
    stall_timeout_ms: Option<u64>,
}

impl OpenAiProvider {
    /// Create a new OpenAI provider
    pub fn new(config: LlmConfig, logging_config: LlmLoggingConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .context("Failed to create HTTP client")?;

        let logger = Arc::new(LlmLogger::new(logging_config)?);

        // Load endpoint preference cache once
        load_openai_cache_from_disk();

        let api_base = config
            .api_base
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

        Ok(Self {
            api_key: config.api_key,
            model: config.model,
            api_base,
            client,
            logger,
            enable_thinking: config.enable_thinking,
            reasoning_effort: config.reasoning_effort,
            reasoning_budget_tokens: config.reasoning_budget_tokens,
            first_token_timeout_ms: config.first_token_timeout_ms,
            stall_timeout_ms: config.stall_timeout_ms,
        })
    }
}
impl OpenAiProvider {
    #[allow(clippy::cognitive_complexity)]
    async fn generate_with_fallback(
        &self,
        prompt: &str,
        max_tokens: Option<usize>,
        temperature: Option<f32>,
    ) -> Result<String> {
        let chat_url = format!("{}/chat/completions", self.api_base.trim_end_matches('/'));
        let responses_url = format!("{}/responses", self.api_base.trim_end_matches('/'));

        // Build base payloads
        let mut chat_payload = json!({
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

        let mut responses_payload = json!({
            "model": self.model,
            "input": prompt,
            "max_output_tokens": max_tokens.unwrap_or(1024),
            "temperature": temperature.unwrap_or(0.7),
        });

        // OpenRouter unified reasoning interface
        // See: https://openrouter.ai/docs/guides/best-practices/reasoning-tokens
        let model_lower = self.model.to_lowercase();

        // Models that support reasoning tokens via OpenRouter
        // - OpenAI o-series: use effort parameter
        // - Claude: use max_tokens parameter
        // - DeepSeek, Gemini, Qwen: use max_tokens parameter
        let supports_reasoning = model_lower.starts_with("o1")
            || model_lower.starts_with("o3")
            || model_lower.starts_with("o4")
            || model_lower.contains("claude")
            || model_lower.contains("deepseek")
            || model_lower.contains("gemini")
            || model_lower.contains("qwen");

        let should_add_reasoning = self.enable_thinking.unwrap_or(false)
            || self.reasoning_effort.is_some()
            || self.reasoning_budget_tokens.is_some();

        if supports_reasoning && should_add_reasoning {
            let mut reasoning_obj = serde_json::Map::new();

            // effort: for OpenAI/Grok models
            if let Some(effort) = &self.reasoning_effort {
                let effort_str = match effort {
                    ReasoningEffort::None => "none",
                    ReasoningEffort::Minimal => "minimal",
                    ReasoningEffort::Low => "low",
                    ReasoningEffort::Medium => "medium",
                    ReasoningEffort::High => "high",
                };
                reasoning_obj.insert(
                    "effort".to_string(),
                    serde_json::Value::String(effort_str.to_string()),
                );
            }

            // max_tokens: for Anthropic/Gemini/Qwen models (NOT budget_tokens)
            if let Some(budget) = self.reasoning_budget_tokens {
                reasoning_obj.insert(
                    "max_tokens".to_string(),
                    serde_json::Value::Number(budget.into()),
                );
            }

            if let Some(obj) = chat_payload.as_object_mut() {
                obj.insert(
                    "reasoning".to_string(),
                    serde_json::Value::Object(reasoning_obj.clone()),
                );
            }
            if let Some(obj) = responses_payload.as_object_mut() {
                obj.insert(
                    "reasoning".to_string(),
                    serde_json::Value::Object(reasoning_obj),
                );
            }
        }

        // Logging
        self.logger.log_request("OpenAI", &self.model, prompt)?;

        // Cache-guided preference
        let cached_pref = {
            let g = openai_cache().lock().unwrap();
            g.get(&self.model).cloned()
        };

        // Helpers
        async fn parse_chat_text(resp: reqwest::Response) -> Result<String> {
            let status = resp.status();
            if !status.is_success() {
                let error_text = resp
                    .text()
                    .await
                    .unwrap_or_else(|e| format!("Could not read error response: {}", e));
                return Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                    "OpenAI Chat returned error ({}): {}",
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
            let chat_response: ChatResponse = resp
                .json()
                .await
                .context("Failed to parse OpenAI Chat response JSON")?;
            chat_response
                .choices
                .first()
                .map(|c| c.message.content.clone())
                .ok_or_else(|| {
                    anyhow::anyhow!(BorgError::LlmApiError(
                        "OpenAI Chat returned no choices".to_string()
                    ))
                })
        }

        fn extract_text_from_responses_json(v: &JsonValue) -> Option<String> {
            // Prefer output_text
            if let Some(s) = v.get("output_text").and_then(|x| x.as_str()) {
                return Some(s.to_string());
            }
            // Try "output" array -> items[].content[].text
            if let Some(arr) = v.get("output").and_then(|x| x.as_array()) {
                let mut buf = String::new();
                for item in arr {
                    if let Some(content_arr) = item.get("content").and_then(|c| c.as_array()) {
                        for c in content_arr {
                            if let Some(t) = c.get("text").and_then(|t| t.as_str()) {
                                buf.push_str(t);
                            }
                        }
                    }
                }
                if !buf.is_empty() {
                    return Some(buf);
                }
            }
            // Some proxies return "choices" like Chat
            if let Some(choices) = v.get("choices").and_then(|x| x.as_array()) {
                if let Some(first) = choices.first() {
                    if let Some(content) = first
                        .get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_str())
                    {
                        return Some(content.to_string());
                    }
                }
            }
            None
        }

        async fn parse_responses_text(resp: reqwest::Response) -> Result<String> {
            let status = resp.status();
            if !status.is_success() {
                let error_text = resp
                    .text()
                    .await
                    .unwrap_or_else(|e| format!("Could not read error response: {}", e));
                return Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                    "OpenAI Responses returned error ({}): {}",
                    status, error_text
                ))));
            }
            let v: JsonValue = resp
                .json()
                .await
                .context("Failed to parse OpenAI Responses JSON")?;
            if let Some(s) = extract_text_from_responses_json(&v) {
                Ok(s)
            } else {
                Ok(String::new())
            }
        }

        // Try order: cached Responses first if cached says so, otherwise Chat first.
        let mut last_err: Option<anyhow::Error> = None;

        // Choose if we should try Chat first
        let try_chat_first = !matches!(
            cached_pref,
            Some(OpenAiEndpointPref::ResponsesMaxCompletion)
                | Some(OpenAiEndpointPref::ResponsesMaxOutput)
        );

        let start_time = Instant::now();

        if try_chat_first {
            let resp = self
                .client
                .post(chat_url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&chat_payload)
                .send()
                .await;

            match resp {
                Ok(r) => {
                    if r.status().is_success() {
                        let content = parse_chat_text(r).await?;
                        // Cache success as Chat
                        {
                            let mut g = openai_cache().lock().unwrap();
                            g.insert(self.model.clone(), OpenAiEndpointPref::Chat);
                        }
                        persist_openai_cache_to_disk();

                        let duration = start_time.elapsed().as_millis() as u64;
                        self.logger
                            .log_response("OpenAI", &self.model, &content, duration)?;
                        return Ok(content);
                    } else {
                        let status = r.status();
                        let error_text = r.text().await.unwrap_or_default();
                        // Detect invalid parameter regression
                        let must_switch = status.as_u16() == 400
                            && error_text.to_lowercase().contains("unsupported parameter")
                            && error_text.contains("'max_tokens'")
                            && error_text.to_lowercase().contains("max_completion_tokens");
                        if !must_switch {
                            last_err = Some(anyhow::anyhow!(BorgError::LlmApiError(format!(
                                "OpenAI API returned error ({}): {}",
                                status, error_text
                            ))));
                        } else {
                            // We'll switch to Responses with max_completion_tokens below
                        }
                    }
                }
                Err(e) => {
                    last_err = Some(anyhow::anyhow!(BorgError::LlmApiError(format!(
                        "Failed to send request to OpenAI Chat API: {}",
                        e
                    ))));
                }
            }
        }

        // Responses attempt - decide which field to use
        let use_completion_field = match cached_pref {
            Some(OpenAiEndpointPref::ResponsesMaxCompletion) => true,
            Some(OpenAiEndpointPref::ResponsesMaxOutput) => false,
            _ => {
                // If we came here due to a 400 invalid params with 'max_tokens', use completion field
                // We can't directly know must_switch here; but safe default is max_output_tokens,
                // and we retry one-time with completion if 400 complains.
                false
            }
        };

        // Make an effective payload for first attempt
        let mut responses_effective = responses_payload.clone();
        if use_completion_field {
            if let Some(obj) = responses_effective.as_object_mut() {
                let v = obj
                    .remove("max_output_tokens")
                    .unwrap_or(json!(max_tokens.unwrap_or(1024)));
                obj.insert("max_completion_tokens".to_string(), v);
            }
        }

        // First Responses request
        let first_resp = self
            .client
            .post(&responses_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&responses_effective)
            .send()
            .await;

        match first_resp {
            Ok(r) => {
                if r.status().is_success() {
                    let content = parse_responses_text(r).await?;
                    {
                        let mut g = openai_cache().lock().unwrap();
                        g.insert(
                            self.model.clone(),
                            if use_completion_field {
                                OpenAiEndpointPref::ResponsesMaxCompletion
                            } else {
                                OpenAiEndpointPref::ResponsesMaxOutput
                            },
                        );
                    }
                    persist_openai_cache_to_disk();

                    let duration = start_time.elapsed().as_millis() as u64;
                    self.logger
                        .log_response("OpenAI", &self.model, &content, duration)?;
                    Ok(content)
                } else {
                    // If we used max_output_tokens and got the invalid-parameter error, retry once with max_completion_tokens
                    let status = r.status();
                    let error_text = r.text().await.unwrap_or_default();
                    let need_retry_with_completion = status.as_u16() == 400
                        && error_text.to_lowercase().contains("unsupported parameter")
                        && error_text.to_lowercase().contains("max_completion_tokens");

                    if !use_completion_field && need_retry_with_completion {
                        // Build completion-field payload and retry
                        let mut retry_payload = responses_payload.clone();
                        if let Some(obj) = retry_payload.as_object_mut() {
                            let v = obj
                                .remove("max_output_tokens")
                                .unwrap_or(json!(max_tokens.unwrap_or(1024)));
                            obj.insert("max_completion_tokens".to_string(), v);
                        }

                        let r2 = self
                            .client
                            .post(&responses_url)
                            .header("Authorization", format!("Bearer {}", self.api_key))
                            .header("Content-Type", "application/json")
                            .json(&retry_payload)
                            .send()
                            .await;

                        match r2 {
                            Ok(rf) => {
                                if rf.status().is_success() {
                                    let content = parse_responses_text(rf).await?;
                                    {
                                        let mut g = openai_cache().lock().unwrap();
                                        g.insert(
                                            self.model.clone(),
                                            OpenAiEndpointPref::ResponsesMaxCompletion,
                                        );
                                    }
                                    persist_openai_cache_to_disk();

                                    let duration = start_time.elapsed().as_millis() as u64;
                                    self.logger.log_response(
                                        "OpenAI",
                                        &self.model,
                                        &content,
                                        duration,
                                    )?;
                                    Ok(content)
                                } else {
                                    // Final failure
                                    let status2 = rf.status();
                                    let error_text2 = rf.text().await.unwrap_or_default();
                                    if let Some(e0) = last_err {
                                        return Err(e0);
                                    }
                                    Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                                        "OpenAI Responses error ({}): {}",
                                        status2, error_text2
                                    ))))
                                }
                            }
                            Err(e) => {
                                if let Some(e0) = last_err {
                                    return Err(e0);
                                }
                                Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                                    "Failed to send request to OpenAI Responses API: {}",
                                    e
                                ))))
                            }
                        }
                    } else {
                        // No special retry path; return the most relevant error
                        if let Some(e) = last_err {
                            return Err(e);
                        }
                        Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                            "OpenAI Responses error ({}): {}",
                            status, error_text
                        ))))
                    }
                }
            }
            Err(e) => {
                if let Some(e0) = last_err {
                    return Err(e0);
                }
                Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                    "Failed to send request to OpenAI Responses API: {}",
                    e
                ))))
            }
        }
    }

    #[allow(clippy::cognitive_complexity)]
    async fn generate_streaming_with_fallback(
        &self,
        prompt: &str,
        max_tokens: Option<usize>,
        temperature: Option<f32>,
        print_tokens: bool,
    ) -> Result<String> {
        let chat_url = format!("{}/chat/completions", self.api_base.trim_end_matches('/'));
        let responses_url = format!("{}/responses", self.api_base.trim_end_matches('/'));

        // Build base payloads
        let mut chat_payload = json!({
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
            "stream": true
        });

        // OpenRouter unified reasoning interface
        // See: https://openrouter.ai/docs/guides/best-practices/reasoning-tokens
        let model_lower = self.model.to_lowercase();

        // Models that support reasoning tokens via OpenRouter
        let supports_reasoning = model_lower.starts_with("o1")
            || model_lower.starts_with("o3")
            || model_lower.starts_with("o4")
            || model_lower.contains("claude")
            || model_lower.contains("deepseek")
            || model_lower.contains("gemini")
            || model_lower.contains("qwen");

        let should_add_reasoning = self.enable_thinking.unwrap_or(false)
            || self.reasoning_effort.is_some()
            || self.reasoning_budget_tokens.is_some();

        if supports_reasoning && should_add_reasoning {
            let mut reasoning_obj = serde_json::Map::new();

            // effort: for OpenAI/Grok models
            if let Some(effort) = &self.reasoning_effort {
                let effort_str = match effort {
                    ReasoningEffort::None => "none",
                    ReasoningEffort::Minimal => "minimal",
                    ReasoningEffort::Low => "low",
                    ReasoningEffort::Medium => "medium",
                    ReasoningEffort::High => "high",
                };
                reasoning_obj.insert(
                    "effort".to_string(),
                    serde_json::Value::String(effort_str.to_string()),
                );
            }

            // max_tokens: for Anthropic/Gemini/Qwen models
            if let Some(budget) = self.reasoning_budget_tokens {
                reasoning_obj.insert(
                    "max_tokens".to_string(),
                    serde_json::Value::Number(budget.into()),
                );
            }

            if let Some(obj) = chat_payload.as_object_mut() {
                obj.insert(
                    "reasoning".to_string(),
                    serde_json::Value::Object(reasoning_obj),
                );
            }
        }

        // Cache-guided preference
        let prefer_responses = {
            let g = openai_cache().lock().unwrap();
            matches!(
                g.get(&self.model),
                Some(OpenAiEndpointPref::ResponsesMaxCompletion)
                    | Some(OpenAiEndpointPref::ResponsesMaxOutput)
            )
        };

        // Log request
        self.logger.log_request("OpenAI", &self.model, prompt)?;

        // Common SSE handlers
        fn handle_chat_sse_line(
            line: &str,
            content: &mut String,
            print_tokens: bool,
            stdout: &mut io::Stdout,
        ) {
            if let Some(json_str) = line.strip_prefix("data: ") {
                if line == "data: [DONE]" {
                    return;
                }
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
                    if let Some(delta) = json
                        .get("choices")
                        .and_then(|choices| choices.get(0))
                        .and_then(|choice| choice.get("delta"))
                        .and_then(|delta| delta.get("content"))
                        .and_then(|c| c.as_str())
                    {
                        content.push_str(delta);
                        if print_tokens {
                            print!("{}", delta);
                            let _ = stdout.flush();
                        }
                    }
                }
            }
        }

        fn handle_responses_sse_line(
            line: &str,
            content: &mut String,
            print_tokens: bool,
            stdout: &mut io::Stdout,
        ) {
            if let Some(json_str) = line.strip_prefix("data: ") {
                if line == "data: [DONE]" {
                    return;
                }
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
                    if let Some(t) = json.get("type").and_then(|t| t.as_str()) {
                        if t.contains("output_text.delta") {
                            if let Some(delta) = json.get("delta").and_then(|d| d.as_str()) {
                                content.push_str(delta);
                                if print_tokens {
                                    print!("{}", delta);
                                    let _ = stdout.flush();
                                }
                            }
                        }
                    }
                }
            }
        }

        // Try Chat first unless cache says Responses
        use tokio::time::timeout;
        let first_token_timeout_ms = self.first_token_timeout_ms.unwrap_or(30000);
        let stall_timeout_ms = self.stall_timeout_ms.unwrap_or(10000);

        let mut _attempt_responses_after = false;
        if !prefer_responses {
            let resp = self
                .client
                .post(chat_url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .header("Accept", "text/event-stream")
                .json(&chat_payload)
                .send()
                .await;

            match resp {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        let status = resp.status();
                        let error_text = resp.text().await.unwrap_or_default();
                        let must_switch = status.as_u16() == 400
                            && error_text.to_lowercase().contains("unsupported parameter")
                            && error_text.contains("'max_tokens'")
                            && error_text.to_lowercase().contains("max_completion_tokens");
                        if must_switch {
                            _attempt_responses_after = true;
                        } else {
                            return Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                                "OpenAI API returned error ({}): {}",
                                status, error_text
                            ))));
                        }
                    } else {
                        // Stream Chat SSE
                        let mut stream = resp.bytes_stream();
                        let mut content = String::new();
                        let mut stdout = io::stdout();
                        let start_time = Instant::now();
                        let mut got_first_chunk = false;

                        loop {
                            let cur_timeout_ms = if got_first_chunk {
                                stall_timeout_ms
                            } else {
                                first_token_timeout_ms
                            };
                            match timeout(Duration::from_millis(cur_timeout_ms), stream.next())
                                .await
                            {
                                Ok(opt_item) => match opt_item {
                                    Some(item) => {
                                        let chunk = item.map_err(|e| {
                                            anyhow::anyhow!(BorgError::LlmApiError(format!(
                                                "Error reading streaming response: {}",
                                                e
                                            )))
                                        })?;
                                        let chunk_str = String::from_utf8_lossy(&chunk);
                                        for line in chunk_str.lines() {
                                            handle_chat_sse_line(
                                                line,
                                                &mut content,
                                                print_tokens,
                                                &mut stdout,
                                            );
                                            got_first_chunk = true;
                                        }
                                    }
                                    None => break,
                                },
                                Err(_) => {
                                    let msg = if !got_first_chunk {
                                        format!(
                                            "OpenAI streaming first token timeout after {} ms for model {}. Received {} chars so far.",
                                            cur_timeout_ms, self.model, content.len()
                                        )
                                    } else {
                                        format!(
                                            "OpenAI streaming stalled after {} ms for model {} with {} chars received.",
                                            cur_timeout_ms, self.model, content.len()
                                        )
                                    };
                                    return Err(anyhow::anyhow!(BorgError::TimeoutError(msg)));
                                }
                            }
                        }

                        if print_tokens {
                            println!();
                        }

                        let duration = start_time.elapsed().as_millis() as u64;
                        self.logger
                            .log_response("OpenAI", &self.model, &content, duration)?;

                        {
                            let mut g = openai_cache().lock().unwrap();
                            g.insert(self.model.clone(), OpenAiEndpointPref::Chat);
                        }
                        persist_openai_cache_to_disk();

                        return Ok(content);
                    }
                }
                Err(e) => {
                    // Fall back to Responses path
                    log::error!("Network error when contacting OpenAI Chat API: {}", e);
                    _attempt_responses_after = true;
                }
            }
        } else {
            _attempt_responses_after = true;
        }

        // Responses streaming attempt
        let prefer = {
            let g = openai_cache().lock().unwrap();
            g.get(&self.model).cloned()
        };
        let use_completion_field =
            matches!(prefer, Some(OpenAiEndpointPref::ResponsesMaxCompletion));

        let mut responses_stream_payload = json!({
            "model": self.model,
            "input": prompt,
            "temperature": temperature.unwrap_or(0.7),
            "stream": true
        });
        let tokens_value = json!(max_tokens.unwrap_or(1024));
        if let Some(obj) = responses_stream_payload.as_object_mut() {
            if use_completion_field {
                obj.insert("max_completion_tokens".into(), tokens_value);
            } else {
                obj.insert("max_output_tokens".into(), tokens_value);
            }
        }

        let resp = self
            .client
            .post(&responses_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .json(&responses_stream_payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                "OpenAI API returned error ({}): {}",
                status, error_text
            ))));
        }

        let mut stream = resp.bytes_stream();
        let mut content = String::new();
        let mut stdout = io::stdout();
        let start_time = Instant::now();
        let mut got_first_chunk = false;

        loop {
            let cur_timeout_ms = if got_first_chunk {
                stall_timeout_ms
            } else {
                first_token_timeout_ms
            };
            match timeout(Duration::from_millis(cur_timeout_ms), stream.next()).await {
                Ok(opt_item) => match opt_item {
                    Some(item) => {
                        let chunk = item.map_err(|e| {
                            anyhow::anyhow!(BorgError::LlmApiError(format!(
                                "Error reading streaming response: {}",
                                e
                            )))
                        })?;
                        let chunk_str = String::from_utf8_lossy(&chunk);
                        for line in chunk_str.lines() {
                            handle_responses_sse_line(
                                line,
                                &mut content,
                                print_tokens,
                                &mut stdout,
                            );
                            got_first_chunk = true;
                        }
                    }
                    None => break,
                },
                Err(_) => {
                    let msg = if !got_first_chunk {
                        format!(
                            "OpenAI streaming first token timeout after {} ms for model {}. Received {} chars so far.",
                            cur_timeout_ms, self.model, content.len()
                        )
                    } else {
                        format!(
                            "OpenAI streaming stalled after {} ms for model {} with {} chars received.",
                            cur_timeout_ms, self.model, content.len()
                        )
                    };
                    return Err(anyhow::anyhow!(BorgError::TimeoutError(msg)));
                }
            }
        }

        if print_tokens {
            println!();
        }

        let duration = start_time.elapsed().as_millis() as u64;
        self.logger
            .log_response("OpenAI", &self.model, &content, duration)?;

        {
            let mut g = openai_cache().lock().unwrap();
            g.insert(
                self.model.clone(),
                if use_completion_field {
                    OpenAiEndpointPref::ResponsesMaxCompletion
                } else {
                    OpenAiEndpointPref::ResponsesMaxOutput
                },
            );
        }
        persist_openai_cache_to_disk();

        Ok(content)
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
        self.generate_with_fallback(prompt, max_tokens, temperature)
            .await
    }

    async fn generate_streaming(
        &self,
        prompt: &str,
        max_tokens: Option<usize>,
        temperature: Option<f32>,
        print_tokens: bool,
    ) -> Result<String> {
        self.generate_streaming_with_fallback(prompt, max_tokens, temperature, print_tokens)
            .await
    }
}

/// Anthropic API provider
pub struct AnthropicProvider {
    api_key: String,
    model: String,
    client: Client,
    logger: Arc<LlmLogger>,
    enable_thinking: Option<bool>,
    reasoning_effort: Option<ReasoningEffort>,
    reasoning_budget_tokens: Option<usize>,
    first_token_timeout_ms: Option<u64>,
    stall_timeout_ms: Option<u64>,
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
            enable_thinking: config.enable_thinking,
            reasoning_effort: config.reasoning_effort,
            reasoning_budget_tokens: config.reasoning_budget_tokens,
            first_token_timeout_ms: config.first_token_timeout_ms,
            stall_timeout_ms: config.stall_timeout_ms,
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

        let mut payload = json!({
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

        // Conditionally attach Anthropic thinking controls when supported and requested
        let model_lower = self.model.to_lowercase();
        let supports_thinking = model_lower.contains("thinking")
            || model_lower.contains("-thinking")
            || model_lower.contains("sonnet-3.7")
            || model_lower.contains("claude-3.7");

        let should_add_thinking = self.enable_thinking.unwrap_or(false)
            || self.reasoning_budget_tokens.is_some()
            || self.reasoning_effort.is_some();

        if supports_thinking && should_add_thinking {
            let mut thinking_obj = serde_json::Map::new();
            thinking_obj.insert(
                "type".to_string(),
                serde_json::Value::String("enabled".to_string()),
            );

            if let Some(budget) = self.reasoning_budget_tokens {
                thinking_obj.insert(
                    "budget_tokens".to_string(),
                    serde_json::Value::Number(budget.into()),
                );
            }

            if let Some(obj) = payload.as_object_mut() {
                obj.insert(
                    "thinking".to_string(),
                    serde_json::Value::Object(thinking_obj),
                );
            }
        }

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
        print_tokens: bool,
    ) -> Result<String> {
        let url = "https://api.anthropic.com/v1/messages";

        let mut payload = json!({
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

        // Conditionally attach Anthropic thinking controls when supported and requested
        let model_lower = self.model.to_lowercase();
        let supports_thinking = model_lower.contains("thinking")
            || model_lower.contains("-thinking")
            || model_lower.contains("sonnet-3.7")
            || model_lower.contains("claude-3.7");

        let should_add_thinking = self.enable_thinking.unwrap_or(false)
            || self.reasoning_budget_tokens.is_some()
            || self.reasoning_effort.is_some();

        if supports_thinking && should_add_thinking {
            let mut thinking_obj = serde_json::Map::new();
            thinking_obj.insert(
                "type".to_string(),
                serde_json::Value::String("enabled".to_string()),
            );

            if let Some(budget) = self.reasoning_budget_tokens {
                thinking_obj.insert(
                    "budget_tokens".to_string(),
                    serde_json::Value::Number(budget.into()),
                );
            }

            if let Some(obj) = payload.as_object_mut() {
                obj.insert(
                    "thinking".to_string(),
                    serde_json::Value::Object(thinking_obj),
                );
            }
        }

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

        // Adaptive idle-timeout: first token vs stall between tokens
        use tokio::time::timeout;
        let first_token_timeout_ms = self.first_token_timeout_ms.unwrap_or(30000);
        let stall_timeout_ms = self.stall_timeout_ms.unwrap_or(10000);
        let mut got_first_chunk = false;

        loop {
            let cur_timeout_ms = if got_first_chunk {
                stall_timeout_ms
            } else {
                first_token_timeout_ms
            };

            match timeout(Duration::from_millis(cur_timeout_ms), stream.next()).await {
                Ok(opt_item) => {
                    match opt_item {
                        Some(item) => {
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
                                if let Some(json_str) = line.strip_prefix("data: ") {
                                    match serde_json::from_str::<serde_json::Value>(json_str) {
                                        Ok(json) => {
                                            if let Some(delta) = json
                                                .get("delta")
                                                .and_then(|delta| delta.get("text"))
                                                .and_then(|text| text.as_str())
                                            {
                                                content.push_str(delta);
                                                if print_tokens {
                                                    print!("{}", delta);
                                                    stdout.flush().unwrap();
                                                }
                                                got_first_chunk = true;
                                            }
                                        }
                                        Err(e) => {
                                            log::warn!(
                                                "Failed to parse JSON from Anthropic stream: {}",
                                                e
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        None => {
                            break;
                        }
                    }
                }
                Err(_) => {
                    let msg = if !got_first_chunk {
                        format!(
                            "Anthropic streaming first token timeout after {} ms for model {}. Received {} chars so far.",
                            cur_timeout_ms, self.model, content.len()
                        )
                    } else {
                        format!(
                            "Anthropic streaming stalled after {} ms for model {} with {} chars received.",
                            cur_timeout_ms, self.model, content.len()
                        )
                    };
                    return Err(anyhow::anyhow!(BorgError::TimeoutError(msg)));
                }
            }
        }

        if print_tokens {
            println!();
        } // Ensure a newline at the end

        // Calculate request duration
        let duration = start_time.elapsed().as_millis() as u64;

        // Log the response
        self.logger
            .log_response("Anthropic", &self.model, &content, duration)?;

        Ok(content)
    }
}

//
// OpenRouter API provider (OpenAI-compatible endpoints)
//
pub struct OpenRouterProvider {
    api_key: String,
    model: String,
    api_base: String,
    client: Client,
    headers: Option<std::collections::HashMap<String, String>>,
    enable_thinking: Option<bool>,
    reasoning_effort: Option<ReasoningEffort>,
    reasoning_budget_tokens: Option<usize>,
    first_token_timeout_ms: Option<u64>,
    stall_timeout_ms: Option<u64>,
    logger: Arc<LlmLogger>,
}

impl OpenRouterProvider {
    /// Create a new OpenRouter provider
    pub fn new(config: LlmConfig, logging_config: LlmLoggingConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .context("Failed to create HTTP client")?;

        let logger = Arc::new(LlmLogger::new(logging_config)?);

        // Per spec, load API key from environment variable
        let api_key = std::env::var("OPENROUTER_API_KEY").map_err(|_| {
            anyhow::anyhow!(BorgError::ConfigError(
                "Missing OPENROUTER_API_KEY environment variable for OpenRouter".to_string()
            ))
        })?;

        let api_base = config
            .api_base
            .clone()
            .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string());

        Ok(Self {
            api_key,
            model: config.model,
            api_base,
            client,
            headers: config.headers.clone(),
            enable_thinking: config.enable_thinking,
            reasoning_effort: config.reasoning_effort,
            reasoning_budget_tokens: config.reasoning_budget_tokens,
            first_token_timeout_ms: config.first_token_timeout_ms,
            stall_timeout_ms: config.stall_timeout_ms,
            logger,
        })
    }

    fn build_url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.api_base.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    /// Attach OpenRouter unified reasoning parameters to payload
    /// See: https://openrouter.ai/docs/guides/best-practices/reasoning-tokens
    fn maybe_attach_reasoning(&self, mut payload: serde_json::Value) -> serde_json::Value {
        let should_add = self.enable_thinking.unwrap_or(false)
            || self.reasoning_effort.is_some()
            || self.reasoning_budget_tokens.is_some();

        if should_add {
            let mut reasoning_obj = serde_json::Map::new();

            // effort: for OpenAI/Grok models
            if let Some(effort) = &self.reasoning_effort {
                let effort_str = match effort {
                    ReasoningEffort::None => "none",
                    ReasoningEffort::Minimal => "minimal",
                    ReasoningEffort::Low => "low",
                    ReasoningEffort::Medium => "medium",
                    ReasoningEffort::High => "high",
                };
                reasoning_obj.insert(
                    "effort".to_string(),
                    serde_json::Value::String(effort_str.to_string()),
                );
            }

            // max_tokens: for Anthropic/Gemini/Qwen models
            if let Some(budget) = self.reasoning_budget_tokens {
                reasoning_obj.insert(
                    "max_tokens".to_string(),
                    serde_json::Value::Number(budget.into()),
                );
            }

            if let Some(obj) = payload.as_object_mut() {
                obj.insert(
                    "reasoning".to_string(),
                    serde_json::Value::Object(reasoning_obj),
                );
            }
        }
        payload
    }

    fn apply_extra_headers(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut req = req;
        if let Some(hdrs) = &self.headers {
            for (k, v) in hdrs {
                req = req.header(k, v);
            }
        }
        req
    }
}

#[async_trait]
impl LlmProvider for OpenRouterProvider {
    async fn generate(
        &self,
        prompt: &str,
        max_tokens: Option<usize>,
        temperature: Option<f32>,
    ) -> Result<String> {
        let url = self.build_url("chat/completions");

        let base_payload = json!({
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
            "stream": false
        });

        // Attach optional reasoning/thinking object
        let payload = self.maybe_attach_reasoning(base_payload);

        // Log the request
        self.logger.log_request("OpenRouter", &self.model, prompt)?;

        let start_time = Instant::now();

        let mut req = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json");

        req = self.apply_extra_headers(req);

        let response = match req.json(&payload).send().await {
            Ok(resp) => resp,
            Err(e) => {
                log::error!("Network error when contacting OpenRouter API: {}", e);
                return Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                    "Failed to send request to OpenRouter API: {}",
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

            log::error!("OpenRouter API error ({}): {}", status, error_text);

            return Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                "OpenRouter API returned error ({}): {}",
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
            .context("Failed to parse OpenRouter API response")?;

        let duration = start_time.elapsed().as_millis() as u64;

        if let Some(choice) = chat_response.choices.first() {
            let content = choice.message.content.clone();

            // Log the response
            self.logger
                .log_response("OpenRouter", &self.model, &content, duration)?;

            Ok(content)
        } else {
            Err(anyhow::anyhow!(BorgError::LlmApiError(
                "OpenRouter API returned no choices".to_string()
            )))
        }
    }

    async fn generate_streaming(
        &self,
        prompt: &str,
        max_tokens: Option<usize>,
        temperature: Option<f32>,
        print_tokens: bool,
    ) -> Result<String> {
        let url = self.build_url("chat/completions");

        let base_payload = json!({
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
            "stream": true
        });

        // Attach optional reasoning/thinking object
        let payload = self.maybe_attach_reasoning(base_payload);

        log::debug!(
            "Sending streaming request to OpenRouter API with model: {}",
            self.model
        );

        // Log the request
        self.logger.log_request("OpenRouter", &self.model, prompt)?;

        let start_time = Instant::now();

        let mut req = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream");

        req = self.apply_extra_headers(req);

        let response = match req.json(&payload).send().await {
            Ok(resp) => resp,
            Err(e) => {
                log::error!("Network error when contacting OpenRouter API: {}", e);
                return Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                    "Failed to send request to OpenRouter API: {}",
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

            log::error!("OpenRouter API error ({}): {}", status, error_text);

            return Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                "OpenRouter API returned error ({}): {}",
                status, error_text
            ))));
        }

        // Stream the SSE response (OpenAI-compatible)
        let mut stream = response.bytes_stream();
        let mut content = String::new();
        let mut stdout = io::stdout();

        // Adaptive idle-timeout: first token vs stall between tokens
        use tokio::time::timeout;
        let first_token_timeout_ms = self.first_token_timeout_ms.unwrap_or(30000);
        let stall_timeout_ms = self.stall_timeout_ms.unwrap_or(10000);
        let mut got_first_chunk = false;

        loop {
            let cur_timeout_ms = if got_first_chunk {
                stall_timeout_ms
            } else {
                first_token_timeout_ms
            };

            match timeout(Duration::from_millis(cur_timeout_ms), stream.next()).await {
                Ok(opt_item) => match opt_item {
                    Some(item) => {
                        let chunk = match item {
                            Ok(chunk) => chunk,
                            Err(e) => {
                                return Err(anyhow::anyhow!(BorgError::LlmApiError(format!(
                                    "Error reading streaming response: {}",
                                    e
                                ))));
                            }
                        };

                        let chunk_str = String::from_utf8_lossy(&chunk);

                        for line in chunk_str.lines() {
                            if let Some(json_str) = line.strip_prefix("data: ") {
                                if line == "data: [DONE]" {
                                    continue;
                                }

                                match serde_json::from_str::<serde_json::Value>(json_str) {
                                    Ok(json) => {
                                        if let Some(delta) = json
                                            .get("choices")
                                            .and_then(|choices| choices.get(0))
                                            .and_then(|choice| choice.get("delta"))
                                            .and_then(|delta| delta.get("content"))
                                            .and_then(|c| c.as_str())
                                        {
                                            content.push_str(delta);
                                            if print_tokens {
                                                print!("{}", delta);
                                                stdout.flush().unwrap();
                                            }
                                            got_first_chunk = true;
                                        }
                                    }
                                    Err(e) => {
                                        log::warn!(
                                            "Failed to parse JSON from OpenRouter stream: {}",
                                            e
                                        );
                                    }
                                }
                            }
                        }
                    }
                    None => {
                        break;
                    }
                },
                Err(_) => {
                    let msg = if !got_first_chunk {
                        format!(
                            "OpenRouter streaming first token timeout after {} ms for model {}. Received {} chars so far.",
                            cur_timeout_ms, self.model, content.len()
                        )
                    } else {
                        format!(
                            "OpenRouter streaming stalled after {} ms for model {} with {} chars received.",
                            cur_timeout_ms, self.model, content.len()
                        )
                    };
                    return Err(anyhow::anyhow!(BorgError::TimeoutError(msg)));
                }
            }
        }

        if print_tokens {
            println!();
        } // Ensure a newline at the end

        let duration = start_time.elapsed().as_millis() as u64;

        self.logger
            .log_response("OpenRouter", &self.model, &content, duration)?;

        Ok(content)
    }
}
