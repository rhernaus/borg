use borg::code_generation::llm::LlmFactory;
use borg::core::config::{LlmConfig, LlmLoggingConfig};
use std::env;
use std::sync::{Mutex as StdMutex, OnceLock as StdOnceLock};

fn env_lock() -> &'static StdMutex<()> {
    static LOCK: StdOnceLock<StdMutex<()>> = StdOnceLock::new();
    LOCK.get_or_init(|| StdMutex::new(()))
}

fn disabled_logger() -> LlmLoggingConfig {
    LlmLoggingConfig {
        enabled: false,
        ..Default::default()
    }
}

fn make_openrouter_config() -> LlmConfig {
    LlmConfig {
        provider: "openrouter".to_string(),
        api_key: "".to_string(), // Provider ignores this; reads from env
        model: "openrouter/auto".to_string(),
        max_tokens: 1024,
        temperature: 0.7,
        api_base: None,
        headers: None,
        enable_streaming: Some(false),
        enable_thinking: None,
        reasoning_effort: None,
        reasoning_budget_tokens: None,
        first_token_timeout_ms: None,
        stall_timeout_ms: None,
    }
}

// Unpinned provider (empty string) should default to OpenRouter when OPENROUTER_API_KEY is present
fn make_unpinned_default_config() -> LlmConfig {
    LlmConfig {
        provider: "".to_string(), // unpinned/default marker
        api_key: "".to_string(),
        model: "openrouter/auto".to_string(), // model name is forwarded to adapter
        max_tokens: 1024,
        temperature: 0.7,
        api_base: None,
        headers: None,
        enable_streaming: Some(false),
        enable_thinking: None,
        reasoning_effort: None,
        reasoning_budget_tokens: None,
        first_token_timeout_ms: None,
        stall_timeout_ms: None,
    }
}

#[test]
fn test_openrouter_factory_missing_api_key() {
    let _g = env_lock().lock().unwrap();
    // Ensure env var is unset
    env::remove_var("OPENROUTER_API_KEY");

    let config = make_openrouter_config();
    let logging = disabled_logger();

    let res = LlmFactory::create(config, logging);

    assert!(
        res.is_err(),
        "Expected factory to fail without OPENROUTER_API_KEY"
    );
    let msg = res.err().unwrap().to_string();
    assert!(
        msg.contains("OPENROUTER_API_KEY"),
        "Error message should mention missing OPENROUTER_API_KEY, got: {}",
        msg
    );
}

#[test]
fn test_default_provider_is_openrouter_when_env_present() {
    let _g = env_lock().lock().unwrap();
    // Ensure env var is set so adapter construction succeeds without network calls
    env::set_var("OPENROUTER_API_KEY", "test-key");

    let config = make_unpinned_default_config();
    let logging = disabled_logger();

    let res = LlmFactory::create(config, logging);
    assert!(
        res.is_ok(),
        "Expected default provider selection to succeed with OPENROUTER_API_KEY set"
    );

    // cleanup
    env::remove_var("OPENROUTER_API_KEY");
}

#[test]
fn test_default_provider_errors_without_env() {
    let _g = env_lock().lock().unwrap();
    env::remove_var("OPENROUTER_API_KEY");

    let config = make_unpinned_default_config();
    let logging = disabled_logger();

    let res = LlmFactory::create(config, logging);
    assert!(
        res.is_err(),
        "Expected default provider selection to fail without OPENROUTER_API_KEY"
    );
    let msg = res.err().unwrap().to_string();
    assert!(
        msg.contains("OPENROUTER_API_KEY"),
        "Error should mention missing OPENROUTER_API_KEY, got: {}",
        msg
    );
}
