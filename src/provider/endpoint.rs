//! Where requests go and how they are carried there.

use std::io::BufReader;
use std::time::Duration;

use serde_json::Value;

use super::reply::{self, Delta, Reply};
use super::{Key, ProviderError, error};

/// How long connecting may take. A server that does not answer within this is not there.
const CONNECT: Duration = Duration::from_secs(30);

/// How long the server may think before it starts answering. Generous rather than tight: a large
/// conversation on a busy server takes a while before the first byte, and a short limit would
/// call a working server broken. Finite, because a reply that waits forever is a stuck screen.
const FIRST_BYTE: Duration = Duration::from_secs(300);

/// The version of the Messages API this module speaks.
const API_VERSION: &str = "2023-06-01";

/// A provider speaking the Anthropic Messages shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The address the API lives under, such as `https://api.example.com/anthropic`; requests go
    /// to `{address}/v1/messages`.
    pub address: String,
    /// The model asked for.
    pub model: String,
    /// The header the key goes in: `x-api-key` for most, another name for some servers. For
    /// `authorization` the key goes in as a bearer token.
    pub key_header: String,
    /// The key.
    pub key: Key,
}

impl Endpoint {
    /// Where a message request goes. The key is never part of it, so it is safe to show.
    #[must_use]
    pub fn url(&self) -> String {
        format!("{}/v1/messages", self.address.trim().trim_end_matches('/'))
    }

    /// Sends `body` (a Messages request without `model` and `stream`, which are added here) and
    /// reads the streamed reply, handing each piece to `on_delta` as it arrives. `cancelled` is
    /// asked between events.
    ///
    /// # Errors
    ///
    /// When the server cannot be reached, refuses, or answers with something that is not a
    /// Messages reply. No error carries the key.
    pub fn stream(
        &self,
        body: &Value,
        on_delta: &mut dyn FnMut(Delta),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Reply, ProviderError> {
        let url = self.url();
        let mut body = body.clone();
        if let Some(object) = body.as_object_mut() {
            object.insert("model".to_owned(), Value::String(self.model.clone()));
            object.insert("stream".to_owned(), Value::Bool(true));
        }
        let config = ureq::Agent::config_builder()
            .timeout_connect(Some(CONNECT))
            .timeout_recv_response(Some(FIRST_BYTE))
            // A refusal is an answer, and its body says what the server objected to; it would be
            // lost if the status alone became an error.
            .http_status_as_error(false)
            .build();
        let agent = ureq::Agent::new_with_config(config);
        let header = self.key_header.trim();
        let secret = if header.eq_ignore_ascii_case("authorization") {
            format!("Bearer {}", self.key.expose())
        } else {
            self.key.expose().to_owned()
        };
        // What the transport says can name the address; it never names a header value, so it
        // cannot name the key.
        let unreachable =
            |error: ureq::Error| ProviderError::Unreachable { url: url.clone(), reason: error.to_string() };
        let mut answer = agent
            .post(&url)
            .header("anthropic-version", API_VERSION)
            .header("accept", "text/event-stream")
            .header(header, &secret)
            .content_type("application/json")
            .send(body.to_string().as_str())
            .map_err(unreachable)?;
        let status = answer.status().as_u16();
        if !(200..300).contains(&status) {
            let said = answer.body_mut().read_to_string().unwrap_or_default();
            return Err(ProviderError::Refused { url, status, said: error::complaint(&said) });
        }
        let lines = BufReader::new(answer.into_body().into_reader());
        reply::read(lines, on_delta, cancelled)
    }
}
