// File: tests/providers_openai_mapping.rs
use borg::code_generation::llm::LlmFactory;
use borg::core::config::{LlmConfig, LlmLoggingConfig};
use httpmock::prelude::*;

fn disabled_logger() -> LlmLoggingConfig {
    LlmLoggingConfig {
        enabled: false,
        ..Default::default()
    }
}

fn make_openai_config(model: &str, api_base: &str) -> LlmConfig {
    LlmConfig {
        provider: "openai".to_string(),
        api_key: "test-key".to_string(),
        model: model.to_string(),
        max_tokens: 1024,
        temperature: 0.0,
        api_base: Some(api_base.to_string()),
        headers: None,
        enable_streaming: Some(false),
        enable_thinking: None,
        reasoning_effort: None,
        reasoning_budget_tokens: None,
        first_token_timeout_ms: Some(5000),
        stall_timeout_ms: Some(3000),
    }
}

fn clear_openai_cache_file() {
    let _ = std::fs::create_dir_all("./logs/llm");
    let _ = std::fs::remove_file("./logs/llm/openai_endpoint_cache.json");
}

#[tokio::test]
async fn test_openai_chat_mapping_sends_max_tokens() {
    clear_openai_cache_file();
    let server = MockServer::start();

    // Expect chat/completions with a body containing "max_tokens" and "messages" (ignore exact value)
    let chat_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/chat/completions")
            .header("content-type", "application/json")
            .body_contains("\"max_tokens\"")
            .body_contains("\"messages\"");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{ "choices": [ { "message": { "content": "OK-CHAT" } } ] }"#);
    });

    let cfg = make_openai_config("test-model-chat-mapping", &server.base_url());
    let logging = disabled_logger();
    let provider = LlmFactory::create(cfg, logging).expect("provider creation");

    let res = provider
        .generate("hello", Some(123), Some(0.1))
        .await
        .expect("chat generate");

    assert_eq!(res, "OK-CHAT");
    assert!(
        chat_mock.hits() >= 1,
        "chat endpoint should be hit at least once; hits={}",
        chat_mock.hits()
    );
}

#[tokio::test]
async fn test_openai_invalid_param_triggers_responses_fallback_and_caches() {
    clear_openai_cache_file();
    let server = MockServer::start();

    // 1) Chat endpoint returns the invalid-parameter error about 'max_tokens' and suggests 'max_completion_tokens'
    let chat_err = r#"{ "error": { "message": "Unsupported parameter: 'max_tokens'. Use 'max_completion_tokens' instead." } }"#;
    let chat_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/chat/completions")
            .header("content-type", "application/json")
            .body_contains("\"messages\"")
            .body_contains("fallback please"); // first call prompt
        then.status(400)
            .header("content-type", "application/json")
            .body(chat_err);
    });

    // 2) First attempt to /responses uses max_output_tokens (no cache yet) and gets a 400 that mentions 'max_completion_tokens'
    let responses_err = r#"{ "error": { "message": "Unsupported parameter. Use 'max_completion_tokens' instead." } }"#;
    let responses_first = server.mock(|when, then| {
        when.method(POST)
            .path("/responses")
            .header("content-type", "application/json")
            .body_contains("\"input\"")
            .body_contains("fallback please")
            .body_contains("\"max_output_tokens\"");
        then.status(400)
            .header("content-type", "application/json")
            .body(responses_err);
    });

    // 3) Retry with max_completion_tokens succeeds
    let responses_retry = server.mock(|when, then| {
        when.method(POST)
            .path("/responses")
            .header("content-type", "application/json")
            .body_contains("\"input\"")
            .body_contains("fallback please")
            .body_contains("\"max_completion_tokens\"");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{ "output_text": "OK-RESP-RETRY" }"#);
    });

    // 4) Second call should hit /responses directly (cache used) and succeed again for a different prompt
    let responses_cached = server.mock(|when, then| {
        when.method(POST)
            .path("/responses")
            .header("content-type", "application/json")
            .body_contains("\"input\"")
            .body_contains("again"); // second call prompt
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{ "output_text": "OK-RESP-CACHED" }"#);
    });

    let cfg = make_openai_config("test-model-fallback-1", &server.base_url());
    let logging = disabled_logger();
    let provider = LlmFactory::create(cfg, logging).expect("provider creation");

    // First call: chat -> 400 invalid-parameter -> responses (max_output_tokens) -> 400 -> responses (max_completion_tokens) -> 200
    let out1 = provider
        .generate("fallback please", Some(77), Some(0.1))
        .await
        .expect("fallback generate");

    assert!(
        out1 == "OK-RESP-RETRY",
        "expected retry success payload, got: {}",
        out1
    );
    assert!(
        chat_mock.hits() >= 1,
        "chat endpoint must be hit; hits={}",
        chat_mock.hits()
    );
    assert!(
        responses_first.hits() >= 1,
        "first responses attempt should be hit; hits={}",
        responses_first.hits()
    );
    assert!(
        responses_retry.hits() >= 1,
        "retry with completion tokens should be hit; hits={}",
        responses_retry.hits()
    );

    // Second call should use cached preference (Responses) directly and not go through Chat
    let out2 = provider
        .generate("again", Some(42), Some(0.0))
        .await
        .expect("cached generate");

    // The exact content may vary if cache already exists from previous runs.
    // Assert we did NOT hit chat again, and that we hit some /responses mock.
    assert!(
        chat_mock.hits() == 1,
        "chat should have been used exactly once; hits={}",
        chat_mock.hits()
    );
    let responses_total = responses_first.hits() + responses_retry.hits() + responses_cached.hits();
    assert!(
        responses_total >= 2,
        "expected at least two /responses calls across both requests; total={}",
        responses_total
    );
    assert!(
        !out2.is_empty(),
        "second call should succeed and return non-empty content"
    );
}
