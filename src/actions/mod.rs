//! The five things the model may do inside the working folder: read a file, list a folder,
//! search for text, edit a file and run a command.
//!
//! The model asks for an action in tool-use form (a tool name and a JSON input). [`Action`]
//! parses that request, [`Workspace::preview`] shows what it would do without changing anything,
//! and [`Workspace::perform`] does it. Every path is confined to the workspace folder, and every
//! failure becomes an [`Outcome`] with a sentence the model can act on. This module knows
//! nothing about the screen: the application maps these types to its widgets.

mod diff;
mod edit;
mod list;
mod read;
mod run;
mod search;
mod workspace;

#[cfg(test)]
mod tests;

use serde_json::{Value, json};

pub use workspace::Workspace;

/// One action the model asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Read a text file.
    Read {
        /// The file, relative to the workspace root.
        path: String,
    },
    /// List a folder; an empty path or `.` is the root.
    List {
        /// The folder, relative to the workspace root.
        path: String,
    },
    /// Find a piece of text in the files under a folder.
    Search {
        /// The exact, case-sensitive text to find.
        pattern: String,
        /// Where to start; an empty path is the root.
        path: String,
    },
    /// Replace one exact piece of text in a file, or create a new file.
    Edit {
        /// The file, relative to the workspace root.
        path: String,
        /// The text to replace; it must occur exactly once. Empty to create a new file.
        old_text: String,
        /// The text that takes its place.
        new_text: String,
    },
    /// Run a shell command in the workspace root.
    Run {
        /// The command, given to `sh -c`.
        command: String,
    },
}

/// Which of the five actions an [`Action`] is, without its details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionKind {
    /// [`Action::Read`].
    Read,
    /// [`Action::List`].
    List,
    /// [`Action::Search`].
    Search,
    /// [`Action::Edit`].
    Edit,
    /// [`Action::Run`].
    Run,
}

impl ActionKind {
    /// The tool name the model uses for this action, such as `read_file`.
    #[must_use]
    pub fn tool_name(self) -> &'static str {
        match self {
            Self::Read => "read_file",
            Self::List => "list_dir",
            Self::Search => "search",
            Self::Edit => "edit_file",
            Self::Run => "run_command",
        }
    }

    /// The short English verb used in [`Action::summary`], such as `read`.
    #[must_use]
    pub fn verb(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::List => "list",
            Self::Search => "search",
            Self::Edit => "edit",
            Self::Run => "run",
        }
    }
}

impl Action {
    /// The five tools in Anthropic tool-use form: the JSON array for the request's `tools` field,
    /// each with a `name`, a `description` and an `input_schema`.
    #[must_use]
    pub fn definitions() -> Value {
        json!([
            {
                "name": "read_file",
                "description": "Read a UTF-8 text file in the working folder. Long files are cut; the reply says how many lines the file has.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path relative to the working folder." }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "list_dir",
                "description": "List a folder in the working folder, one entry per line, folders first with a trailing slash.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Folder path relative to the working folder; empty or \".\" for the working folder itself." }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "search",
                "description": "Find an exact, case-sensitive piece of text in the text files under a folder. Skips .git, target, node_modules and binary files. Replies with path:line: text.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string", "description": "The exact text to find." },
                        "path": { "type": "string", "description": "Folder or file to search, relative to the working folder; the whole working folder when left out." }
                    },
                    "required": ["pattern"]
                }
            },
            {
                "name": "edit_file",
                "description": "Replace one exact piece of text in a file with new text. old_text must occur exactly once; include surrounding lines to make it unique. With an empty old_text, create a new file containing new_text. The person approves every edit first.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path relative to the working folder." },
                        "old_text": { "type": "string", "description": "The exact text to replace, including whitespace; empty to create a new file." },
                        "new_text": { "type": "string", "description": "The text that takes its place." }
                    },
                    "required": ["path", "old_text", "new_text"]
                }
            },
            {
                "name": "run_command",
                "description": "Run a shell command with sh -c in the working folder, without input, for at most 120 seconds. Replies with the combined output (the end of it when long) and the exit code. The person approves every command first.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "The shell command to run." }
                    },
                    "required": ["command"]
                }
            }
        ])
    }

    /// Reads a tool call. An unknown tool or a bad input gives a sentence for the model.
    ///
    /// # Errors
    ///
    /// When `name` is not one of the five tools, or `input` is not an object with the fields the
    /// tool needs as strings.
    pub fn from_call(name: &str, input: &Value) -> Result<Self, String> {
        let field = |key: &str| text_field(name, input, key, true);
        match name {
            "read_file" => Ok(Self::Read { path: field("path")? }),
            "list_dir" => Ok(Self::List { path: text_field(name, input, "path", false)? }),
            "search" => Ok(Self::Search { pattern: field("pattern")?, path: text_field(name, input, "path", false)? }),
            "edit_file" => {
                Ok(Self::Edit { path: field("path")?, old_text: field("old_text")?, new_text: field("new_text")? })
            }
            "run_command" => Ok(Self::Run { command: field("command")? }),
            other => Err(format!(
                "There is no tool named `{other}`; use read_file, list_dir, search, edit_file or run_command."
            )),
        }
    }

    /// Whether the person must approve the action first: editing and running do, reading does not.
    #[must_use]
    pub fn needs_approval(&self) -> bool {
        matches!(self.kind(), ActionKind::Edit | ActionKind::Run)
    }

    /// Which action this is.
    #[must_use]
    pub fn kind(&self) -> ActionKind {
        match self {
            Self::Read { .. } => ActionKind::Read,
            Self::List { .. } => ActionKind::List,
            Self::Search { .. } => ActionKind::Search,
            Self::Edit { .. } => ActionKind::Edit,
            Self::Run { .. } => ActionKind::Run,
        }
    }

    /// What the action is about: the path for reading, listing and editing, the pattern for a
    /// search and the command for running.
    #[must_use]
    pub fn subject(&self) -> &str {
        match self {
            Self::Read { path } | Self::List { path } | Self::Edit { path, .. } => path,
            Self::Search { pattern, .. } => pattern,
            Self::Run { command } => command,
        }
    }

    /// One short untranslated line for the transcript, such as `read src/main.rs` or
    /// `run cargo test`. An application that translates builds its own line from [`Self::kind`]
    /// and [`Self::subject`].
    #[must_use]
    pub fn summary(&self) -> String {
        let subject = match self {
            Self::List { path } if path.is_empty() => ".",
            _ => self.subject(),
        };
        let verb = self.kind().verb();
        match self {
            Self::Search { path, .. } if !path.is_empty() && path != "." => {
                format!("{verb} {subject} in {path}")
            }
            _ => format!("{verb} {subject}"),
        }
    }
}

/// Reads one string field of a tool input; a missing optional field is empty.
fn text_field(tool: &str, input: &Value, key: &str, required: bool) -> Result<String, String> {
    let Some(object) = input.as_object() else {
        return Err(format!("The input of `{tool}` must be a JSON object."));
    };
    match object.get(key) {
        Some(Value::String(text)) => Ok(text.clone()),
        None | Some(Value::Null) if !required => Ok(String::new()),
        None | Some(Value::Null) => Err(format!("`{tool}` needs the field `{key}`.")),
        Some(_) => Err(format!("The field `{key}` of `{tool}` must be a string.")),
    }
}

/// What an action would do, shown on the approval card before anything changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preview {
    /// Nothing to show beyond the action line.
    Plain,
    /// The edit as a line diff of the whole affected region with up to three lines of context
    /// on each side.
    Diff {
        /// The file, relative to the workspace root.
        path: String,
        /// The lines of the region, in order, each tagged with its change.
        lines: Vec<DiffLine>,
    },
    /// The command as it will be run.
    Command {
        /// The command, given to `sh -c`.
        command: String,
    },
}

/// One line of a [`Preview::Diff`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    /// Whether the line stays, comes in or goes away.
    pub change: Change,
    /// The line, without its line ending.
    pub text: String,
}

/// How a [`DiffLine`] changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Change {
    /// The line is there before and after: context.
    Same,
    /// The line is new.
    Added,
    /// The line goes away.
    Removed,
}

/// What an action did, as the tool result for the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The result, or a sentence saying why the action was not done.
    pub text: String,
    /// Whether the action failed; a command that ran and exited with a non-zero code did not.
    pub is_error: bool,
}

impl Outcome {
    fn done(text: String) -> Self {
        Self { text, is_error: false }
    }

    fn failed(text: String) -> Self {
        Self { text, is_error: true }
    }
}
