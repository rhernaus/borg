use borg::code_generation::llm::LlmFactory;
use borg::core::config::{LlmConfig, LlmLoggingConfig};
use regex::Regex;
use std::env;
use std::sync::OnceLock;

fn env_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn disabled_logger() -> LlmLoggingConfig {
    LlmLoggingConfig {
        enabled: false,
        ..Default::default()
    }
}

fn make_mock_config(first: Option<u64>, stall: Option<u64>) -> LlmConfig {
    LlmConfig {
        provider: "mock".to_string(),
        api_key: "test-api-key".to_string(),
        model: "mock-1".to_string(),
        max_tokens: 1024,
        temperature: 0.7,
        api_base: None,
        headers: None,
        enable_streaming: Some(true),
        enable_thinking: None,
        reasoning_effort: None,
        reasoning_budget_tokens: None,
        first_token_timeout_ms: first,
        stall_timeout_ms: stall,
    }
}

fn clear_mock_env() {
    let keys = [
        "BORG_MOCK_STREAM_PROFILE",
        "BORG_MOCK_STALL_AFTER_CHUNKS",
        "BORG_MOCK_THINKING_TOKENS",
        "BORG_MOCK_FIRST_CHUNK_DELAY_MS",
        "BORG_MOCK_INTER_CHUNK_DELAY_MS",
    ];
    for k in keys {
        env::remove_var(k);
    }
}

#[tokio::test]
async fn test_mock_first_token_timeout() {
    let _g = env_lock().lock().await;

    clear_mock_env();
    env::remove_var("OPENROUTER_API_KEY");
    env::set_var("BORG_MOCK_STREAM_PROFILE", "no_first_chunk");

    let config = make_mock_config(Some(50), Some(200));
    let logging = disabled_logger();

    let provider = LlmFactory::create(config, logging).expect("mock provider creation failed");
    let res = provider
        .generate_streaming("Test prompt", None, None, false)
        .await;

    assert!(res.is_err(), "Expected timeout error, got Ok");
    let err = res.err().unwrap().to_string();
    let err_lower = err.to_lowercase();
    assert!(
        err_lower.contains("first token timeout"),
        "Error did not indicate first-token timeout: {}",
        err
    );

    clear_mock_env();
}

#[tokio::test]
async fn test_mock_stall_timeout_after_n() {
    let _g = env_lock().lock().await;
    clear_mock_env();

    env::set_var("BORG_MOCK_STREAM_PROFILE", "stall_after_n");
    env::set_var("BORG_MOCK_STALL_AFTER_CHUNKS", "2");
    env::set_var("BORG_MOCK_INTER_CHUNK_DELAY_MS", "5");

    let config = make_mock_config(Some(500), Some(50));
    let logging = disabled_logger();

    let provider = LlmFactory::create(config, logging).expect("mock provider creation failed");
    let res = provider
        .generate_streaming("Test prompt", None, None, false)
        .await;

    assert!(res.is_err(), "Expected stall timeout error, got Ok");
    let err = res.err().unwrap().to_string();
    let err_lower = err.to_lowercase();
    assert!(
        err_lower.contains("stalled"),
        "Error did not indicate stream stalled: {}",
        err
    );

    let re = Regex::new(r"with\s+(\d+)\s+chars\s+received").unwrap();
    let caps = re
        .captures(&err)
        .expect("Could not parse received chars from error");
    let n: usize = caps.get(1).unwrap().as_str().parse().unwrap_or(0);
    assert!(n > 0, "Expected partial content > 0 chars, got {}", n);

    clear_mock_env();
}

#[tokio::test]
async fn test_mock_thinking_then_answer_content() {
    let _g = env_lock().lock().await;
    clear_mock_env();

    env::set_var("BORG_MOCK_STREAM_PROFILE", "thinking_then_answer");
    env::set_var("BORG_MOCK_THINKING_TOKENS", "3");
    env::set_var("BORG_MOCK_FIRST_CHUNK_DELAY_MS", "0");
    env::set_var("BORG_MOCK_INTER_CHUNK_DELAY_MS", "1");

    let config = make_mock_config(Some(1000), Some(1000));
    let logging = disabled_logger();

    let provider = LlmFactory::create(config, logging).expect("mock provider creation failed");
    let res = provider
        .generate_streaming("Test prompt for thinking", None, None, false)
        .await;

    let content = res.expect("Expected successful streaming content");
    assert!(!content.trim().is_empty(), "Expected non-empty content");
    assert!(
        content.to_lowercase().contains("(thinking"),
        "Expected thinking prelude in content, got: {}",
        content
    );

    clear_mock_env();
}
