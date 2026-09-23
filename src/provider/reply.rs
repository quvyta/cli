//! A streamed reply in the Anthropic Messages shape, put together as it arrives.
//!
//! The server sends server-sent events: `event:` and `data:` lines, a blank line between events.
//! Only the `data` matters, since each JSON object names its own type. Text and thinking are
//! handed on piece by piece as they come, so the screen can show them growing; a tool call is
//! handed on once its name is known, and its input, which arrives as fragments of JSON, is read
//! when its block ends.

use std::io::BufRead;

use serde_json::{Map, Value, json};

use super::ProviderError;

/// Something that arrived while a reply streams in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delta {
    /// More of the answer's text.
    Text(String),
    /// More of the model's thinking, for providers that show it.
    Thinking(String),
    /// The model began a tool call with this name.
    ToolCall(String),
}

/// Why the model stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// It finished its turn.
    EndTurn,
    /// It asked for tools and waits for their results.
    ToolUse,
    /// It ran out of the tokens it was allowed to write.
    MaxTokens,
    /// Anything else the server named, kept as it was said.
    Other(String),
}

/// A whole reply.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    /// The content blocks in the Messages shape, ready to go back into the conversation as the
    /// assistant's message: `text`, `thinking` and `tool_use` blocks.
    pub content: Vec<Value>,
    /// Why the model stopped.
    pub stop: Stop,
    /// The input tokens the server counted, when it said.
    pub input_tokens: Option<u64>,
    /// The output tokens the server counted, when it said.
    pub output_tokens: Option<u64>,
}

/// One content block while it is being written.
#[derive(Debug)]
struct Block {
    value: Value,
    /// The fragments of a tool call's input, joined when the block ends.
    input: String,
}

/// Reads the event stream in `lines` to its end and puts the reply together, handing each piece
/// to `on_delta` as it comes. `cancelled` is asked between events; when it says yes the reply
/// read so far is returned as it is.
///
/// # Errors
///
/// When the server sends an error event, when the stream breaks, or when it ends without saying
/// the message is complete.
pub fn read(
    lines: impl BufRead,
    on_delta: &mut dyn FnMut(Delta),
    cancelled: &dyn Fn() -> bool,
) -> Result<Reply, ProviderError> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut reply = Reply { content: Vec::new(), stop: Stop::EndTurn, input_tokens: None, output_tokens: None };
    let mut data = String::new();
    let mut complete = false;
    for line in lines.lines() {
        let line = line.map_err(|error| ProviderError::Unreadable { what: error.to_string() })?;
        if let Some(rest) = line.strip_prefix("data:") {
            data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
            continue;
        }
        if !line.is_empty() || data.is_empty() {
            continue;
        }
        let event = std::mem::take(&mut data);
        if event == "[DONE]" {
            complete = true;
            break;
        }
        let event: Value = serde_json::from_str(&event)
            .map_err(|error| ProviderError::Unreadable { what: format!("{error}: {event}") })?;
        if apply(&event, &mut blocks, &mut reply, on_delta)? {
            complete = true;
            break;
        }
        if cancelled() {
            complete = true;
            break;
        }
    }
    if !complete {
        return Err(ProviderError::Unreadable { what: "the reply ended before the message was complete".to_owned() });
    }
    reply.content = blocks.into_iter().map(finish).collect();
    Ok(reply)
}

/// Applies one event and says whether it was the last.
fn apply(
    event: &Value,
    blocks: &mut Vec<Block>,
    reply: &mut Reply,
    on_delta: &mut dyn FnMut(Delta),
) -> Result<bool, ProviderError> {
    let kind = event.get("type").and_then(Value::as_str).unwrap_or_default();
    match kind {
        "message_start" => {
            let usage = event.pointer("/message/usage");
            reply.input_tokens = usage.and_then(|usage| usage.get("input_tokens")).and_then(Value::as_u64);
        }
        "content_block_start" => {
            let block = event.get("content_block").cloned().unwrap_or_else(|| json!({"type": "text", "text": ""}));
            if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                on_delta(Delta::ToolCall(block.get("name").and_then(Value::as_str).unwrap_or_default().to_owned()));
            }
            blocks.push(Block { value: block, input: String::new() });
        }
        "content_block_delta" => {
            let Some(block) = blocks.last_mut() else {
                return Err(ProviderError::Unreadable { what: "a delta arrived before any block".to_owned() });
            };
            let delta = event.get("delta").cloned().unwrap_or_default();
            let text = |field: &str| delta.get(field).and_then(Value::as_str).unwrap_or_default().to_owned();
            match delta.get("type").and_then(Value::as_str).unwrap_or_default() {
                "text_delta" => {
                    let piece = text("text");
                    append(&mut block.value, "text", &piece);
                    on_delta(Delta::Text(piece));
                }
                "thinking_delta" => {
                    let piece = text("thinking");
                    append(&mut block.value, "thinking", &piece);
                    on_delta(Delta::Thinking(piece));
                }
                "signature_delta" => append(&mut block.value, "signature", &text("signature")),
                "input_json_delta" => block.input.push_str(&text("partial_json")),
                _ => {}
            }
        }
        "message_delta" => {
            if let Some(stop) = event.pointer("/delta/stop_reason").and_then(Value::as_str) {
                reply.stop = match stop {
                    "end_turn" | "stop_sequence" => Stop::EndTurn,
                    "tool_use" => Stop::ToolUse,
                    "max_tokens" => Stop::MaxTokens,
                    other => Stop::Other(other.to_owned()),
                };
            }
            if let Some(tokens) = event.pointer("/usage/output_tokens").and_then(Value::as_u64) {
                reply.output_tokens = Some(tokens);
            }
        }
        "message_stop" => return Ok(true),
        "error" => {
            let said = event.pointer("/error/message").and_then(Value::as_str).unwrap_or("error").to_owned();
            return Err(ProviderError::Stopped { said });
        }
        _ => {}
    }
    Ok(false)
}

/// Adds `piece` to the text field `field` of `block`.
fn append(block: &mut Value, field: &str, piece: &str) {
    if let Some(object) = block.as_object_mut() {
        let mut text = object.get(field).and_then(Value::as_str).unwrap_or_default().to_owned();
        text.push_str(piece);
        object.insert(field.to_owned(), Value::String(text));
    }
}

/// A block as it goes back into the conversation: a tool call gets its input read from the
/// fragments; an input that is not JSON is kept as a string under `input` so that the model sees
/// what it sent.
fn finish(block: Block) -> Value {
    let Block { mut value, input } = block;
    if value.get("type").and_then(Value::as_str) == Some("tool_use")
        && let Some(object) = value.as_object_mut()
    {
        let parsed = if input.trim().is_empty() {
            Value::Object(Map::new())
        } else {
            serde_json::from_str(&input).unwrap_or(Value::String(input))
        };
        object.insert("input".to_owned(), parsed);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The events of a reply, written the way servers send them.
    fn stream(events: &[Value]) -> String {
        events
            .iter()
            .map(|event| format!("event: {}\ndata: {event}\n\n", event["type"].as_str().unwrap_or("")))
            .collect()
    }

    fn collect(text: &str) -> (Result<Reply, ProviderError>, Vec<Delta>) {
        let mut deltas = Vec::new();
        let reply = read(text.as_bytes(), &mut |delta| deltas.push(delta), &|| false);
        (reply, deltas)
    }

    #[test]
    fn text_and_thinking_arrive_in_pieces_and_end_up_whole() {
        let text = stream(&[
            json!({"type": "message_start", "message": {"usage": {"input_tokens": 14}}}),
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": "", "signature": ""}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "Short "}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "answer."}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "sig"}}),
            json!({"type": "content_block_stop", "index": 0}),
            json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "Hey"}}),
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": " there"}}),
            json!({"type": "content_block_stop", "index": 1}),
            json!({"type": "ping"}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 9}}),
            json!({"type": "message_stop"}),
        ]);
        let (reply, deltas) = collect(&text);
        let reply = reply.expect("a reply");
        assert_eq!(
            deltas,
            [
                Delta::Thinking("Short ".into()),
                Delta::Thinking("answer.".into()),
                Delta::Text("Hey".into()),
                Delta::Text(" there".into())
            ]
        );
        assert_eq!(
            reply.content,
            [
                json!({"type": "thinking", "thinking": "Short answer.", "signature": "sig"}),
                json!({"type": "text", "text": "Hey there"})
            ]
        );
        assert_eq!(reply.stop, Stop::EndTurn);
        assert_eq!((reply.input_tokens, reply.output_tokens), (Some(14), Some(9)));
    }

    #[test]
    fn a_tool_call_is_announced_by_name_and_its_input_read_from_fragments() {
        let text = stream(&[
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": "t1", "name": "read_file", "input": {}}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": "{\"path\": \"src/"}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": "main.rs\"}"}}),
            json!({"type": "content_block_stop", "index": 0}),
            json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}}),
            json!({"type": "message_stop"}),
        ]);
        let (reply, deltas) = collect(&text);
        let reply = reply.expect("a reply");
        assert_eq!(deltas, [Delta::ToolCall("read_file".into())]);
        assert_eq!(
            reply.content,
            [json!({"type": "tool_use", "id": "t1", "name": "read_file", "input": {"path": "src/main.rs"}})]
        );
        assert_eq!(reply.stop, Stop::ToolUse);
    }

    #[test]
    fn an_error_event_and_a_cut_stream_are_errors() {
        let error = stream(&[json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}})]);
        assert_eq!(collect(&error).0, Err(ProviderError::Stopped { said: "Overloaded".into() }));
        let cut = stream(&[
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        ]);
        assert!(matches!(collect(&cut).0, Err(ProviderError::Unreadable { .. })), "no message_stop, no reply");
    }

    #[test]
    fn a_cancelled_reply_keeps_what_had_arrived() {
        let text = stream(&[
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Half"}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": " more"}}),
        ]);
        let mut seen = 0;
        let reply = read(text.as_bytes(), &mut |_| seen += 1, &|| true).expect("what arrived");
        assert_eq!(reply.content, [json!({"type": "text", "text": ""})], "stopped after the first event");
        assert_eq!(seen, 0);
    }
}
