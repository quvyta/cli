//! A conversation with the model, kept in the Messages shape it is sent in.
//!
//! The messages are kept exactly as the API reads them, so sending the conversation is writing
//! it out and nothing is translated on the way. What the screen shows is read from the same
//! messages.

use serde_json::{Value, json};

/// A tool call the model made.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    /// The id the result must name.
    pub id: String,
    /// The tool.
    pub name: String,
    /// What it was given.
    pub input: Value,
}

/// The messages of one conversation.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Conversation {
    messages: Vec<Value>,
}

impl Conversation {
    /// The messages, oldest first.
    #[must_use]
    pub fn messages(&self) -> &[Value] {
        &self.messages
    }

    /// Whether nothing has been said yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Adds what the person wrote. Right after another user message (a reply that was stopped
    /// before it said anything) the text joins that message, since the API wants the roles to
    /// take turns.
    pub fn say(&mut self, text: &str) {
        let block = json!({"type": "text", "text": text});
        self.push_user(vec![block]);
    }

    /// Adds the model's reply. Nothing is added for a reply with no content.
    pub fn reply(&mut self, content: Vec<Value>) {
        if !content.is_empty() {
            self.messages.push(json!({"role": "assistant", "content": Value::Array(content)}));
        }
    }

    /// Adds the results of tool calls, each a `tool_result` block.
    pub fn results(&mut self, results: Vec<Value>) {
        if !results.is_empty() {
            self.push_user(results);
        }
    }

    fn push_user(&mut self, blocks: Vec<Value>) {
        if let Some(last) = self.messages.last_mut()
            && last["role"] == "user"
            && let Some(content) = last.get_mut("content").and_then(Value::as_array_mut)
        {
            content.extend(blocks);
            return;
        }
        self.messages.push(json!({"role": "user", "content": blocks}));
    }

    /// The tool calls of the last message that have no result yet, in the order they were made.
    #[must_use]
    pub fn open_calls(&self) -> Vec<Call> {
        let Some(last) = self.messages.last().filter(|message| message["role"] == "assistant") else {
            return Vec::new();
        };
        blocks(last)
            .filter(|block| block["type"] == "tool_use")
            .map(|block| Call {
                id: block["id"].as_str().unwrap_or_default().to_owned(),
                name: block["name"].as_str().unwrap_or_default().to_owned(),
                input: block.get("input").cloned().unwrap_or(Value::Null),
            })
            .collect()
    }

    /// The conversation as it is saved.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({"version": 1, "messages": self.messages})
    }

    /// A saved conversation. Anything that is not a message is left out, and calls that were
    /// waiting when the program closed are answered as not done, so the conversation can go on.
    #[must_use]
    pub fn from_json(value: &Value) -> Self {
        let messages = value["messages"]
            .as_array()
            .map(|messages| {
                messages
                    .iter()
                    .filter(|message| {
                        matches!(message["role"].as_str(), Some("user" | "assistant")) && message["content"].is_array()
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let mut conversation = Self { messages };
        let open = conversation.open_calls();
        conversation.results(open.iter().map(|call| result(&call.id, CLOSED, true)).collect());
        conversation
    }
}

/// What a call that was waiting when the program closed is answered with.
const CLOSED: &str = "Not done: the program was closed before this ran.";

/// The blocks of `message`.
pub fn blocks(message: &Value) -> impl Iterator<Item = &Value> {
    message["content"].as_array().into_iter().flatten()
}

/// A `tool_result` block for the call `id`.
#[must_use]
pub fn result(id: &str, text: &str, is_error: bool) -> Value {
    json!({"type": "tool_result", "tool_use_id": id, "content": text, "is_error": is_error})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_take_turns_even_after_a_reply_that_said_nothing() {
        let mut conversation = Conversation::default();
        conversation.say("first");
        conversation.reply(Vec::new());
        conversation.say("second");
        assert_eq!(conversation.messages().len(), 1, "the two texts share one user message");
        assert_eq!(conversation.messages()[0]["content"][1]["text"], "second");
        conversation.reply(vec![json!({"type": "text", "text": "answer"})]);
        assert_eq!(conversation.messages()[1]["role"], "assistant");
    }

    #[test]
    fn open_calls_are_the_last_replys_tool_uses() {
        let mut conversation = Conversation::default();
        conversation.say("look");
        conversation.reply(vec![
            json!({"type": "text", "text": "Looking."}),
            json!({"type": "tool_use", "id": "a", "name": "list_dir", "input": {"path": "."}}),
            json!({"type": "tool_use", "id": "b", "name": "read_file", "input": {"path": "x"}}),
        ]);
        let open = conversation.open_calls();
        assert_eq!(open.iter().map(|call| call.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(open[1].input, json!({"path": "x"}));
        conversation.results(vec![result("a", "x/", false), result("b", "text", false)]);
        assert!(conversation.open_calls().is_empty());
    }

    #[test]
    fn a_saved_conversation_comes_back_and_waiting_calls_are_answered() {
        let mut conversation = Conversation::default();
        conversation.say("run it");
        conversation
            .reply(vec![json!({"type": "tool_use", "id": "r", "name": "run_command", "input": {"command": "ls"}})]);
        let saved = conversation.to_json();
        let back = Conversation::from_json(&saved);
        assert!(back.open_calls().is_empty(), "the waiting call is answered");
        let last = back.messages().last().expect("a message");
        assert_eq!(last["content"][0]["tool_use_id"], "r");
        assert_eq!(last["content"][0]["is_error"], true);
        assert_eq!(Conversation::from_json(&json!({"messages": [{"role": "system"}, 3]})), Conversation::default());
    }
}
