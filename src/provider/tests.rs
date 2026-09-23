//! The endpoint against a stand-in server: what goes out, and what comes back.

use serde_json::json;

use super::fake::{FakeServer, says};
use super::{Delta, Endpoint, Key, ProviderError, Stop};

/// Not a key of anyone's: the characters spell what it is.
const MADE_UP: &str = "not-a-real-key-0000-wxyz";

fn endpoint(address: &str, header: &str) -> Endpoint {
    Endpoint {
        address: format!("{address}/"),
        model: "test-model".into(),
        key_header: header.into(),
        key: Key::new(MADE_UP).expect("a key"),
    }
}

#[test]
fn the_request_carries_the_model_the_stream_flag_and_the_key_in_its_header() {
    let server = FakeServer::start(vec![(200, says("Hello there"))]);
    let mut pieces = String::new();
    let reply = endpoint(&server.address, "api-key")
        .stream(
            &json!({"max_tokens": 64, "messages": [{"role": "user", "content": "hi"}]}),
            &mut |delta| {
                if let Delta::Text(text) = delta {
                    pieces.push_str(&text);
                }
            },
            &|| false,
        )
        .expect("a reply");
    assert_eq!(pieces, "Hello there");
    assert_eq!(reply.content, [json!({"type": "text", "text": "Hello there"})]);
    assert_eq!(reply.stop, Stop::EndTurn);

    let received = server.received();
    assert_eq!(received.len(), 1);
    let request = &received[0];
    assert_eq!(request.line, "POST /v1/messages HTTP/1.1", "the trailing slash of the address is not doubled");
    assert_eq!(request.header("api-key"), Some(MADE_UP));
    assert_eq!(request.header("x-api-key"), None, "the key goes only in the header that was named");
    assert_eq!(request.header("anthropic-version"), Some("2023-06-01"));
    assert_eq!(request.body["model"], "test-model");
    assert_eq!(request.body["stream"], true);
    assert_eq!(request.body["messages"][0]["content"], "hi");
}

#[test]
fn an_authorization_header_carries_the_key_as_a_bearer_token() {
    let server = FakeServer::start(vec![(200, says("ok"))]);
    endpoint(&server.address, "Authorization")
        .stream(&json!({"messages": []}), &mut |_| {}, &|| false)
        .expect("a reply");
    assert_eq!(server.received()[0].header("authorization"), Some(format!("Bearer {MADE_UP}").as_str()));
}

#[test]
fn a_refusal_says_what_the_server_said_and_never_the_key() {
    let body = json!({"type": "error", "error": {"type": "authentication_error", "message": "invalid api key"}});
    let server = FakeServer::start(vec![(401, body.to_string())]);
    let error = endpoint(&server.address, "x-api-key")
        .stream(&json!({"messages": []}), &mut |_| {}, &|| false)
        .expect_err("refused");
    let ProviderError::Refused { status, said, .. } = &error else { panic!("a refusal, got {error:?}") };
    assert_eq!((*status, said.as_str()), (401, "invalid api key"));
    assert!(!error.to_string().contains(MADE_UP));
    assert!(!format!("{error:?}").contains(MADE_UP));
}

#[test]
fn an_address_nobody_answers_is_unreachable_and_names_only_the_address() {
    // A port that was free a moment ago and is closed again: nothing listens there.
    let port = std::net::TcpListener::bind("127.0.0.1:0").expect("a port").local_addr().expect("addr").port();
    let error = endpoint(&format!("http://127.0.0.1:{port}"), "x-api-key")
        .stream(&json!({"messages": []}), &mut |_| {}, &|| false)
        .expect_err("nobody there");
    assert!(matches!(error, ProviderError::Unreachable { .. }), "{error:?}");
    assert!(!error.to_string().contains(MADE_UP));
}

#[test]
fn a_tool_call_comes_through_the_endpoint_whole() {
    let server = FakeServer::start(vec![(200, super::fake::calls("t1", "list_dir", &json!({"path": "."})))]);
    let mut announced = Vec::new();
    let reply = endpoint(&server.address, "x-api-key")
        .stream(&json!({"messages": []}), &mut |delta| announced.push(delta), &|| false)
        .expect("a reply");
    assert_eq!(announced, [Delta::ToolCall("list_dir".into())]);
    assert_eq!(reply.stop, Stop::ToolUse);
    assert_eq!(reply.content, [json!({"type": "tool_use", "id": "t1", "name": "list_dir", "input": {"path": "."}})]);
}
