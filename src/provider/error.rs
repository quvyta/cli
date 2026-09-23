//! Why a reply did not come, told in words a person can act on.

use std::fmt;

/// How much of a server's answer is ever repeated back. Enough to recognise the complaint, short
/// enough that the screen is not filled with someone's HTML error page.
const QUOTED: usize = 240;

/// Why a request to a provider failed.
///
/// None of these carries a key: a key is only ever a header value, never part of an address,
/// and what is quoted back is the server's own words, not the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    /// The address could not be reached at all.
    Unreachable {
        /// Where the request was going.
        url: String,
        /// What the network said.
        reason: String,
    },
    /// The server answered, and refused.
    Refused {
        /// Where the request went.
        url: String,
        /// The status it refused with.
        status: u16,
        /// The beginning of what it said.
        said: String,
    },
    /// The server stopped the reply part way with an error of its own.
    Stopped {
        /// The beginning of what it said.
        said: String,
    },
    /// What came back is not the shape the Messages API promises.
    Unreadable {
        /// What was wrong with it.
        what: String,
    },
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable { url, reason } => write!(formatter, "{url}: {reason}"),
            Self::Refused { url, status, said } => write!(formatter, "{url}: {status} {said}"),
            Self::Stopped { said } => formatter.write_str(said),
            Self::Unreadable { what } => formatter.write_str(what),
        }
    }
}

/// The words of a refusal a person can act on, out of a body that may wrap them in JSON.
///
/// Providers put the sentence that says what happened in different places: `error.message` in
/// the Anthropic shape, a bare `message` or `error` string in others, and some keep the useful
/// part under `metadata.raw`. The most specific one is taken when the body is one of these
/// shapes, and the body itself otherwise.
pub(crate) fn complaint(body: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else { return shorten(body) };
    let error = value.get("error");
    let said = [
        value.get("metadata").and_then(|metadata| metadata.get("raw")),
        error.and_then(|error| error.get("metadata")).and_then(|metadata| metadata.get("raw")),
        error.and_then(|error| error.get("message")),
        error.filter(|error| error.is_string()),
        value.get("message"),
    ]
    .into_iter()
    .flatten()
    .find_map(|said| said.as_str().filter(|said| !said.trim().is_empty()))
    .unwrap_or(body);
    shorten(said)
}

/// The beginning of `said`, so that the screen is never filled with an error page.
fn shorten(said: &str) -> String {
    let trimmed = said.trim();
    match trimmed.char_indices().nth(QUOTED) {
        Some((end, _)) => format!("{}…", &trimmed[..end]),
        None => trimmed.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sentence_is_taken_out_of_the_shapes_providers_use() {
        let anthropic = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        assert_eq!(complaint(anthropic), "invalid x-api-key");
        assert_eq!(complaint(r#"{"error":"model not found"}"#), "model not found");
        assert_eq!(complaint(r#"{"message":"rate limited"}"#), "rate limited");
        assert_eq!(complaint("  Bad Gateway \n"), "Bad Gateway");
    }

    #[test]
    fn a_long_page_is_cut_with_an_ellipsis() {
        let page = "x".repeat(1000);
        let said = complaint(&page);
        assert_eq!(said.chars().count(), QUOTED + 1);
        assert!(said.ends_with('…'));
    }
}
