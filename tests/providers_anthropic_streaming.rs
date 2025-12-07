// File: tests/providers_anthropic_streaming.rs
use borg::providers::{
    ContentPart, GenerateRequest, Message, Provider, Role, StreamEvent, ToolChoice, ToolSpec,
};
use httpmock::prelude::*;
use serde_json::json;

fn make_req_with_tools() -> GenerateRequest {
    GenerateRequest {
        system: Some("sys-prompt".to_string()),
        messages: vec![
            Message {
                role: Role::User,
                content: vec![
                    ContentPart::Text {
                        text: "hello anthropic".to_string(),
                    },
                    ContentPart::ImageUrl {
                        url: "https://example.com/cat.png".to_string(),
                        mime: Some("image/png".to_string()),
                    },
                ],
            },
            Message {
                role: Role::Assistant,
                content: vec![ContentPart::Text {
                    text: "prior message".to_string(),
                }],
            },
        ],
        tools: Some(vec![ToolSpec {
            name: "search".to_string(),
            description: Some("search the codebase".to_string()),
            json_schema: Some(json!({
                "type": "object",
                "properties": { "q": { "type": "string" } },
                "required": ["q"]
            })),
        }]),
        tool_choice: Some(ToolChoice::Required),
        temperature: Some(0.1),
        top_p: Some(0.9),
        stop: Some(vec!["STOPME".to_string()]),
        seed: None,
        logit_bias: None,
        response_format: None,
        max_output_tokens: Some(123),
        metadata: None,
    }
}

fn make_cfg(base: &str) -> borg::core::config::LlmConfig {
    borg::core::config::LlmConfig {
        provider: "anthropic".to_string(),
        api_key: "test-anthropic".to_string(),
        model: "claude-3-7-sonnet".to_string(),
        max_tokens: 1024,
        temperature: 0.0,
        api_base: Some(base.to_string()),
        headers: None,
        enable_streaming: Some(true),
        enable_thinking: None,
        reasoning_effort: None,
        reasoning_budget_tokens: None,
        first_token_timeout_ms: Some(5_000),
        stall_timeout_ms: Some(3_000),
    }
}

#[tokio::test]
async fn test_anthropic_mapping_non_streaming_payload() {
    let server = MockServer::start();

    // Expect POST /messages with canonical mapped fields
    let m = server.mock(|when, then| {
        when.method(POST)
            .path("/messages")
            .header("x-api-key", "test-anthropic")
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            // Payload mapping checks
            .body_contains("\"system\":\"sys-prompt\"")
            .body_contains("\"messages\"")
            .body_contains("\"role\":\"user\"")
            .body_contains("\"type\":\"image\"")
            .body_contains("\"tools\"")
            .body_contains("\"tool_choice\":\"any\"")
            .body_contains("\"max_tokens\":123");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                "content": [
                    { "type": "text", "text": "OK" }
                ],
                "usage": { "input_tokens": 10, "output_tokens": 3 }
            }"#,
            );
    });

    let cfg = make_cfg(&server.base_url());
    let provider =
        borg::providers::anthropic::AnthropicProvider::from_config(&cfg).expect("provider");

    let req = make_req_with_tools();
    let res = provider.generate(req).await.expect("generate ok");

    assert_eq!(res.text, "OK");
    assert!(m.hits() >= 1, "mock should be hit at least once");
}

#[tokio::test]
async fn test_anthropic_streaming_sse_and_tool_call() {
    let server = MockServer::start();

    // Build a simple SSE response with a tool_use start, two text deltas, usage, and stop
    let sse = "\
event: message_start
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_123\"}}

event: content_block_start
data: {\"type\":\"content_block_start\",\"content_block\":{\"type\":\"tool_use\",\"id\":\"t123\",\"name\":\"search\",\"input\":{\"q\":\"rust\"}}}

event: content_block_delta
data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello \"}}

event: content_block_delta
data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"world\"}}

event: message_delta
data: {\"type\":\"message_delta\",\"usage\":{\"input_tokens\":5,\"output_tokens\":2}}

event: message_stop
data: {\"type\":\"message_stop\"}

data: [DONE]
";

    let m = server.mock(|when, then| {
        when.method(POST)
            .path("/messages")
            .header("accept", "text/event-stream")
            .body_contains("\"stream\":true")
            .body_contains("\"model\"");
        then.status(200)
            .header("content-type", "text/event-stream")
            .body(sse);
    });

    let cfg = make_cfg(&server.base_url());
    let provider =
        borg::providers::anthropic::AnthropicProvider::from_config(&cfg).expect("provider");

    let req = make_req_with_tools();

    let mut deltas = Vec::new();
    let mut saw_tool_call = false;
    let mut saw_finished = false;

    let mut on_event = |ev: StreamEvent| match ev {
        StreamEvent::TextDelta(t) => deltas.push(t),
        StreamEvent::ToolCall(tc) => {
            saw_tool_call = true;
            assert_eq!(tc.name, "search");
            assert_eq!(tc.id.as_deref(), Some("t123"));
            assert_eq!(
                tc.arguments_json.get("q").and_then(|v| v.as_str()),
                Some("rust")
            );
        }
        StreamEvent::Finished => saw_finished = true,
        _ => {}
    };

    let res = provider
        .generate_streaming(req, &mut on_event)
        .await
        .expect("stream ok");

    assert!(m.hits() >= 1, "SSE mock should be hit");
    assert_eq!(deltas.join(""), "Hello world");
    assert!(saw_tool_call, "should emit ToolCall");
    assert!(saw_finished, "should emit Finished");
    assert_eq!(res.text, "Hello world");
}
