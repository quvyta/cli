//! A stand-in provider for tests: a real HTTP server on `127.0.0.1` that answers each request
//! with the next of the answers it was given and remembers what it was asked.
//!
//! It is a real socket rather than a stand-in transport so that the code that goes out over the
//! network is the code under test, and no test can reach a real provider: the only address a
//! test knows is this one.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use serde_json::Value;

/// One request the server received.
#[derive(Debug, Clone)]
pub struct Received {
    /// The request line, such as `POST /v1/messages HTTP/1.1`.
    pub line: String,
    /// The headers, names in lower case.
    pub headers: Vec<(String, String)>,
    /// The body read as JSON.
    pub body: Value,
}

impl Received {
    /// The value of the header `name` (lower case), if it was sent.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }
}

/// A running stand-in provider.
pub struct FakeServer {
    /// The address to put in an endpoint.
    pub address: String,
    received: Arc<Mutex<Vec<Received>>>,
}

impl FakeServer {
    /// Starts a server that answers the requests in order with `answers`, each a status and a
    /// body; a request past the last answer is answered with 500.
    pub fn start(answers: Vec<(u16, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free port on the loopback");
        let address = format!("http://{}", listener.local_addr().expect("the port"));
        let received = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&received);
        thread::spawn(move || {
            let mut answers = answers.into_iter();
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let Some(request) = read_request(&mut stream) else { continue };
                log.lock().unwrap_or_else(PoisonError::into_inner).push(request);
                let (status, body) = answers.next().unwrap_or((500, "no more answers".to_owned()));
                let kind = if status == 200 { "text/event-stream" } else { "application/json" };
                let head = format!("HTTP/1.1 {status} Answer\r\ncontent-type: {kind}\r\nconnection: close\r\n\r\n");
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body.as_bytes());
            }
        });
        Self { address, received }
    }

    /// The requests received so far.
    pub fn received(&self) -> Vec<Received> {
        self.received.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

fn read_request(stream: &mut std::net::TcpStream) -> Option<Received> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).ok()?;
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        let (name, value) = header.split_once(':')?;
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
    }
    let length: usize =
        headers.iter().find(|(name, _)| name == "content-length").and_then(|(_, value)| value.parse().ok())?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    let body = serde_json::from_slice(&body).unwrap_or(Value::Null);
    Some(Received { line: line.trim_end().to_owned(), headers, body })
}

/// A streamed reply that says `text` and ends its turn, in pieces the way servers send them.
pub fn says(text: &str) -> String {
    let mut events = vec![
        serde_json::json!({"type": "message_start", "message": {"usage": {"input_tokens": 10}}}),
        serde_json::json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
    ];
    for piece in text.split_inclusive(' ') {
        events.push(
            serde_json::json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": piece}}),
        );
    }
    events.push(serde_json::json!({"type": "content_block_stop", "index": 0}));
    events.push(serde_json::json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}}));
    events.push(serde_json::json!({"type": "message_stop"}));
    sse(&events)
}

/// A streamed reply that calls tool `name` with `input` and waits for its result.
pub fn calls(id: &str, name: &str, input: &Value) -> String {
    sse(&[
        serde_json::json!({"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": id, "name": name, "input": {}}}),
        serde_json::json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": input.to_string()}}),
        serde_json::json!({"type": "content_block_stop", "index": 0}),
        serde_json::json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}}),
        serde_json::json!({"type": "message_stop"}),
    ])
}

/// `events` as a server-sent event stream.
pub fn sse(events: &[Value]) -> String {
    events.iter().map(|event| format!("event: {}\ndata: {event}\n\n", event["type"].as_str().unwrap_or(""))).collect()
}
