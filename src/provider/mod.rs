//! Talking to a model provider in the Anthropic Messages shape.
//!
//! This module depends on nothing else in the crate: it will move into a crate shared with
//! qcode, so it speaks in its own terms (a key, an endpoint, the pieces of a streamed reply) and
//! leaves what a conversation means to the application.
//!
//! Two rules run through it. **The key lives apart:** it is read from the person's key file into
//! a [`Key`], which cannot be printed, and it only ever travels in a header, so no address, error
//! or log can carry it. **No test reaches a real provider:** tests talk to a server of their own
//! on the loopback, and live tries are separate and ignored unless asked for.

mod endpoint;
mod error;
#[cfg(test)]
pub(crate) mod fake;
mod key;
#[cfg(test)]
mod live;
mod reply;
#[cfg(test)]
mod tests;

pub use endpoint::Endpoint;
pub use error::ProviderError;
pub use key::{Key, KeyError};
pub use reply::{Delta, Reply, Stop};
