//! What the model is told before the conversation: who it is working for, where, and with
//! which tools, followed by the folder's own `AGENTS.md` when it has one.

use std::path::Path;

use serde_json::{Value, json};

use crate::actions::Action;
use crate::conversation::Conversation;

/// The name of the file a folder keeps its instructions for coding agents in.
pub const AGENTS_FILE: &str = "AGENTS.md";

/// The most of `AGENTS.md` that is sent. A file larger than this is most likely not meant as
/// instructions, and sending it whole with every request would cost the person on every turn.
const AGENTS_LIMIT: usize = 64 * 1024;

/// How many tokens a reply may use, thinking included.
const MAX_TOKENS: u32 = 16_384;

/// The system prompt for work in `root`.
#[must_use]
pub fn system(root: &Path) -> String {
    let mut text = format!(
        "You are qcli, a coding agent in a terminal. You work in the folder {}. \
         Use the tools to look at the files before you answer questions about them. \
         Paths are relative to that folder, and nothing outside it can be reached. \
         Every edit and every command is shown to the person first, and they may decline it; \
         when they do, do not try the same thing another way, ask what they want instead. \
         Keep answers short and plain.",
        root.display()
    );
    if let Some(agents) = agents(root) {
        text.push_str("\n\nThe folder's AGENTS.md says:\n\n");
        text.push_str(&agents);
    }
    text
}

/// The folder's `AGENTS.md`, when it has one that can be read as text.
#[must_use]
pub fn agents(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join(AGENTS_FILE)).ok()?;
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some(match text.char_indices().nth(AGENTS_LIMIT) {
        Some((end, _)) => format!("{}\n\n(The rest of the file was left out: it is too long.)", &text[..end]),
        None => text.to_owned(),
    })
}

/// The body of the next request: the conversation so far, the system prompt and the tools.
#[must_use]
pub fn body(conversation: &Conversation, system: &str) -> Value {
    json!({
        "max_tokens": MAX_TOKENS,
        "system": system,
        "messages": conversation.messages(),
        "tools": Action::definitions(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folders_agents_file_joins_the_system_prompt() {
        let root = std::env::temp_dir().join(format!("qcli-instructions-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("folder");
        assert!(!system(&root).contains("AGENTS.md says"), "no file, no section");
        std::fs::write(root.join(AGENTS_FILE), "Always answer in haiku.\n").expect("write");
        let prompt = system(&root);
        assert!(prompt.contains(&root.display().to_string()));
        assert!(prompt.ends_with("Always answer in haiku."), "{prompt}");
        std::fs::write(root.join(AGENTS_FILE), "x".repeat(AGENTS_LIMIT + 10)).expect("write");
        assert!(agents(&root).expect("text").ends_with("it is too long.)"));
        std::fs::remove_dir_all(&root).expect("clean up");
    }

    #[test]
    fn the_body_carries_the_conversation_and_the_five_tools() {
        let mut conversation = Conversation::default();
        conversation.say("hi");
        let body = body(&conversation, "be brief");
        assert_eq!(body["system"], "be brief");
        assert_eq!(body["messages"][0]["content"][0]["text"], "hi");
        let names: Vec<_> = body["tools"].as_array().expect("tools").iter().map(|tool| tool["name"].clone()).collect();
        assert_eq!(names, ["read_file", "list_dir", "search", "edit_file", "run_command"]);
    }
}
