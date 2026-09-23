//! Tries against a real provider. Ignored unless asked for by name, since they need the person's
//! key file and the network:
//!
//! ```text
//! cargo test live_ -- --ignored --nocapture
//! ```
//!
//! The address, model and key header come from `QCLI_LIVE_ADDRESS`, `QCLI_LIVE_MODEL` and
//! `QCLI_LIVE_HEADER`, the key from the file in `QCLI_LIVE_KEY_FILE`. Nothing prints the key.

use std::path::PathBuf;

use serde_json::json;

use super::{Delta, Endpoint, Key};

/// The endpoint the environment describes.
pub(crate) fn endpoint() -> Endpoint {
    let variable = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set"));
    let key_file = PathBuf::from(variable("QCLI_LIVE_KEY_FILE"));
    Endpoint {
        address: variable("QCLI_LIVE_ADDRESS"),
        model: variable("QCLI_LIVE_MODEL"),
        key_header: std::env::var("QCLI_LIVE_HEADER").unwrap_or_else(|_| "x-api-key".into()),
        key: Key::from_file(&key_file).unwrap_or_else(|error| panic!("no key: {error}")),
    }
}

#[test]
#[ignore = "talks to a real provider; run by hand"]
fn live_a_short_chat_streams_an_answer() {
    let endpoint = endpoint();
    let mut pieces = 0;
    let reply = endpoint
        .stream(
            &json!({"max_tokens": 400, "messages": [{"role": "user", "content": "Reply with exactly: ready"}]}),
            &mut |delta| {
                if let Delta::Text(text) = delta {
                    pieces += 1;
                    print!("{text}");
                }
            },
            &|| false,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    println!("\n{} pieces, stop {:?}, tokens {:?}/{:?}", pieces, reply.stop, reply.input_tokens, reply.output_tokens);
    assert!(pieces > 0, "the answer streamed in");
}
