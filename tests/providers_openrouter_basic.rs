// File: tests/providers_openrouter_basic.rs
use borg::providers::{ContentPart, GenerateRequest, Message, Provider, Role, StreamEvent};
use httpmock::prelude::*;

fn make_cfg_with_headers(base: &str) -> borg::core::config::LlmConfig {
    let mut headers = std::collections::HashMap::new();
    headers.insert("HTTP-Referer".to_string(), "https://unit.test".to_string());
    headers.insert("X-Title".to_string(), "Borg Tests".to_string());

    borg::core::config::LlmConfig {
        provider: "openrouter".to_string(),
        api_key: "test-openrouter".to_string(), // provider adapter uses this or env
        model: "openrouter/auto".to_string(),
        max_tokens: 1024,
        temperature: 0.0,
        api_base: Some(base.to_string()),
        headers: Some(headers),
        enable_streaming: Some(true),
        enable_thinking: None,
        reasoning_effort: None,
        reasoning_budget_tokens: None,
        first_token_timeout_ms: Some(5_000),
        stall_timeout_ms: Some(3_000),
    }
}

fn make_req_basic() -> GenerateRequest {
    GenerateRequest {
        system: Some("sys".to_string()),
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentPart::Text {
                text: "hello".to_string(),
            }],
        }],
        tools: None,
        tool_choice: None,
        temperature: Some(0.1),
        top_p: None,
        stop: None,
        seed: None,
        logit_bias: None,
        response_format: None,
        max_output_tokens: Some(55),
        metadata: None,
    }
}

#[tokio::test]
async fn test_openrouter_streaming_headers_forwarded() {
    let server = MockServer::start();

    // SSE compatible with OpenAI-chat delta format
    let sse = "\
data: {\"choices\":[{\"delta\":{\"content\":\"Hi \"}}]}
data: {\"choices\":[{\"delta\":{\"content\":\"there\"}}]}
data: [DONE]
";

    // Expect headers and stream=true
    let m = server.mock(|when, then| {
        when.method(POST)
            .path("/chat/completions")
            .header_exists("authorization")
            .header("accept", "text/event-stream")
            .header("http-referer", "https://unit.test")
            .header("x-title", "Borg Tests")
            .body_contains("\"stream\":true")
            .body_contains("\"messages\"")
            .body_contains("\"max_tokens\""); // default path uses max_tokens
        then.status(200)
            .header("content-type", "text/event-stream")
            .body(sse);
    });

    let cfg = make_cfg_with_headers(&server.base_url());
    let provider =
        borg::providers::openrouter::OpenRouterProvider::from_config(&cfg).expect("provider");

    let req = make_req_basic();

    let mut deltas = Vec::new();
    let mut on_event = |ev: StreamEvent| {
        if let StreamEvent::TextDelta(t) = ev {
            deltas.push(t);
        }
    };

    let res = provider
        .generate_streaming(req, &mut on_event)
        .await
        .expect("stream ok");

    assert!(m.hits() >= 1, "SSE mock should be hit");
    assert_eq!(deltas.join(""), "Hi there");
    assert_eq!(res.text, "Hi there");
}

#[tokio::test]
async fn test_openrouter_retry_to_responses_style_tokens() {
    let server = MockServer::start();

    // First attempt returns 400 complaining about max_output_tokens -> force retry remapping
    let first = server.mock(|when, then| {
        when.method(POST)
            .path("/chat/completions")
            .header("content-type", "application/json")
            .body_contains("\"max_tokens\"");
        then.status(400)
            .header("content-type", "application/json")
            .body(r#"{ "error": { "message": "Unsupported parameter. Use 'max_output_tokens' instead." } }"#);
    });

    // Retry should include max_output_tokens
    let retry = server.mock(|when, then| {
        when.method(POST)
            .path("/chat/completions")
            .header("content-type", "application/json")
            .body_contains("\"max_output_tokens\"");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                    "choices": [
                        { "message": { "content": "OK-RETRY" } }
                    ],
                    "usage": { "prompt_tokens": 7, "completion_tokens": 2, "total_tokens": 9 }
                }"#,
            );
    });

    let mut cfg = make_cfg_with_headers(&server.base_url());
    // No need to stream for this test; non-streaming generate path
    cfg.enable_streaming = Some(false);

    let provider =
        borg::providers::openrouter::OpenRouterProvider::from_config(&cfg).expect("provider");

    let req = make_req_basic();
    let res = provider.generate(req).await.expect("generate ok");
    assert_eq!(res.text, "OK-RETRY");

    assert!(first.hits() >= 1, "first attempt must be hit");
    assert!(retry.hits() >= 1, "retry should be hit");
}
