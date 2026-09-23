//! What the chat looks like: the conversation with what is streaming in and, at its end, the card
//! of an action waiting for an answer; under it the writing field.

use std::collections::HashMap;

use qframe::prelude::*;
use qframe::widgets::{CodeView, Language, LineMark, Markdown, ScrollView, Spinner, TextInput};
use serde_json::Value;

use super::{Live, Msg, Notice, Phase, QCli};
use crate::actions::{Action, ActionKind, Change, Preview};
use crate::conversation;
use crate::instructions::{self, AGENTS_FILE};
use crate::provider::KeyError;

/// The id of the button that allows the action on the card.
pub const ALLOW: &str = "allow";

/// The id of the writing field.
pub const INPUT: &str = "input";

/// How many lines of an action's result are shown under it. The model reads all of it; the
/// person sees enough to follow along.
const RESULT_LINES: usize = 3;

pub(super) fn view(app: &QCli, ui: &mut View<'_, Msg>) {
    AppShell::new()
        .header(|ui| header(app, ui))
        .body(|ui| {
            ui.column(|ui| {
                ui.add_with(ScrollView::new().follow_end(true), |ui| {
                    ui.column(|ui| {
                        transcript(app, ui);
                        live(app, ui);
                        status(app, ui);
                        if let Phase::Asking { action, preview, .. } = &app.phase {
                            card(ui, action, preview);
                        }
                    })
                    .gap(1)
                    .padding(Padding::symmetric(1, 2));
                })
                .id(format!("conversation-{}", app.turn))
                .fill();
                input(app, ui);
            })
            .fill();
        })
        .footer(|ui| {
            ui.add(
                KeyHints::new()
                    .hint("enter", t!("keys.send"))
                    .action(Scope::App, "stop")
                    .action(Scope::App, "new")
                    .action(Scope::App, "provider")
                    .action_right(Scope::Global, "quit"),
            );
        })
        .show(ui);
    if let Some(form) = &app.form {
        form.view(ui);
    }
}

fn header(app: &QCli, ui: &mut View<'_, Msg>) {
    let folder = app
        .workspace
        .root()
        .file_name()
        .map_or_else(|| app.workspace.root().display().to_string(), |name| name.to_string_lossy().into_owned());
    ui.row(|ui| {
        ui.add(Text::new("qcli").bold().color("accent").no_wrap());
        ui.add(Text::new(folder).no_wrap());
        ui.spacer();
        let model = if app.provider.model.is_empty() { t!("header.no-model") } else { app.provider.model.clone() };
        ui.add(Text::new(model).role("secondary").no_wrap());
    })
    .gap(2)
    .padding(Padding::symmetric(0, 2));
}

/// The conversation so far, as it was saved.
fn transcript(app: &QCli, ui: &mut View<'_, Msg>) {
    let messages = app.conversation.messages();
    if messages.is_empty() {
        let mut spans = vec![Span::new(t!("chat.empty"))];
        if instructions::agents(app.workspace.root()).is_some() {
            spans.push(Span::new(" "));
            spans.push(Span::new(t!("chat.agents", file = AGENTS_FILE)));
        }
        ui.add(Text::rich(spans).role("secondary"));
        return;
    }
    let results: HashMap<&str, (&str, bool)> = messages
        .iter()
        .flat_map(conversation::blocks)
        .filter(|block| block["type"] == "tool_result")
        .map(|block| {
            let text = block["content"].as_str().unwrap_or_default();
            (block["tool_use_id"].as_str().unwrap_or_default(), (text, block["is_error"] == true))
        })
        .collect();
    for message in messages {
        let from_person = message["role"] == "user";
        for block in conversation::blocks(message) {
            match block["type"].as_str().unwrap_or_default() {
                "text" if from_person => said(ui, block["text"].as_str().unwrap_or_default()),
                "text" => answer(ui, block["text"].as_str().unwrap_or_default()),
                "thinking" => thinking(ui, block["thinking"].as_str().unwrap_or_default()),
                "tool_use" => {
                    let id = block["id"].as_str().unwrap_or_default();
                    call(app, ui, block, results.get(id).copied());
                }
                _ => {}
            }
        }
    }
}

fn said(ui: &mut View<'_, Msg>, text: &str) {
    ui.add_with(Panel::new(), |ui| {
        ui.add(Text::new(text)).selectable(true);
    });
}

fn answer(ui: &mut View<'_, Msg>, text: &str) {
    if !text.trim().is_empty() {
        ui.add(Markdown::new(text));
    }
}

fn thinking(ui: &mut View<'_, Msg>, text: &str) {
    let text = text.trim();
    if !text.is_empty() {
        ui.add(Text::new(text).role("faint"));
    }
}

/// A tool call: what it did, the diff it showed when it asked, and the start of its result.
fn call(app: &QCli, ui: &mut View<'_, Msg>, block: &Value, result: Option<(&str, bool)>) {
    let name = block["name"].as_str().unwrap_or_default();
    let id = block["id"].as_str().unwrap_or_default();
    ui.column(|ui| {
        match Action::from_call(name, &block["input"]) {
            Ok(action) => ui.add(Text::rich([
                Span::new(verb(action.kind())).color("accent").bold(),
                Span::new(" "),
                Span::new(action.subject().to_owned()),
            ])),
            Err(_) => ui.add(Text::new(name.to_owned()).color("accent").bold()),
        };
        if let Some(Preview::Diff { path, lines }) = app.previews.get(id) {
            diff(ui, path, lines);
        }
        if let Some((text, is_error)) = result {
            outcome(ui, text, is_error);
        }
    });
}

fn verb(kind: ActionKind) -> String {
    match kind {
        ActionKind::Read => t!("action.read"),
        ActionKind::List => t!("action.list"),
        ActionKind::Search => t!("action.search"),
        ActionKind::Edit => t!("action.edit"),
        ActionKind::Run => t!("action.run"),
    }
}

fn diff(ui: &mut View<'_, Msg>, path: &str, lines: &[crate::actions::DiffLine]) {
    let code: Vec<&str> = lines.iter().map(|line| line.text.as_str()).collect();
    let marks = lines.iter().map(|line| match line.change {
        Change::Same => LineMark::Unchanged,
        Change::Added => LineMark::Added,
        Change::Removed => LineMark::Removed,
    });
    ui.add(CodeView::new(code.join("\n"), Language::from_file_name(path)).line_marks(marks).line_numbers(false));
}

fn outcome(ui: &mut View<'_, Msg>, text: &str, is_error: bool) {
    let lines: Vec<&str> = text.lines().collect();
    let mut shown = lines.iter().take(RESULT_LINES).copied().collect::<Vec<_>>().join("\n");
    if lines.len() > RESULT_LINES {
        shown.push('\n');
        shown.push_str(&t!("action.more", n = lines.len() - RESULT_LINES));
    }
    if shown.trim().is_empty() {
        shown = t!("action.empty");
    }
    let text = Text::new(shown);
    ui.add(if is_error { text.color("danger") } else { text.role("faint") }).selectable(true);
}

/// What is streaming in right now.
fn live(app: &QCli, ui: &mut View<'_, Msg>) {
    for piece in &app.live {
        match piece {
            Live::Thinking(text) => thinking(ui, text),
            Live::Text(text) => answer(ui, text),
            Live::Call(name) => {
                ui.add(Text::new(name.clone()).color("accent").bold());
            }
        }
    }
}

/// The spinner while something is going on, and the notice when something went wrong.
fn status(app: &QCli, ui: &mut View<'_, Msg>) {
    match &app.phase {
        Phase::Replying if app.live.is_empty() => {
            ui.add(Spinner::new().label(t!("status.waiting")));
        }
        Phase::Acting { action, .. } => {
            ui.add(Spinner::new().label(t!("status.acting", what = action.subject().to_owned())));
        }
        _ => {}
    }
    let Some(notice) = &app.notice else { return };
    let (text, tone) = match notice {
        Notice::Provider(said) => (t!("notice.provider", said = said.clone()), "danger"),
        Notice::Unset => (t!("notice.unset"), "warning"),
        Notice::Key(KeyError::Unreadable { path, reason }) => {
            (t!("provider.key-unreadable", path = path.display().to_string(), reason = reason.clone()), "danger")
        }
        Notice::Key(KeyError::NotAKey { path }) => {
            (t!("provider.not-a-key", path = path.display().to_string()), "danger")
        }
        Notice::Stopped => (t!("notice.stopped"), "warning"),
        Notice::Cut => (t!("notice.cut"), "warning"),
        Notice::Rounds => (t!("notice.rounds", n = super::MAX_ROUNDS), "warning"),
        Notice::Unsaved(reason) => (t!("notice.unsaved", reason = reason.clone()), "danger"),
    };
    ui.add(Text::new(text).color(tone)).selectable(true);
}

/// The writing field under the conversation. It steps aside while a card waits for an answer,
/// so the letters that answer the card cannot be typed into it by mistake.
fn input(app: &QCli, ui: &mut View<'_, Msg>) {
    if matches!(app.phase, Phase::Asking { .. }) {
        return;
    }
    ui.add(
        TextInput::new(app.draft.clone())
            .placeholder(t!("chat.placeholder"))
            .on_change(Msg::Draft)
            .on_submit(|_| Msg::Send),
    )
    .id(INPUT)
    .fill_width()
    .padding(Padding::symmetric(0, 2));
}

fn card(ui: &mut View<'_, Msg>, action: &Action, preview: &Preview) {
    let title = match action.kind() {
        ActionKind::Edit => t!("ask.edit", path = action.subject().to_owned()),
        _ => t!("ask.run"),
    };
    ui.add_with(Panel::new().title(title).selected(true), |ui| {
        match preview {
            Preview::Diff { path, lines } => diff(ui, path, lines),
            Preview::Command { command } => {
                ui.add(CodeView::new(command.clone(), Language::Shell).line_numbers(false));
            }
            Preview::Plain => {
                ui.add(Text::new(action.summary()));
            }
        }
        ui.row(|ui| {
            ui.add(Button::new(t!("ask.allow")).variant("primary").shortcut("y").on_press(Msg::Allow)).id(ALLOW);
            ui.add(Button::new(t!("ask.deny")).shortcut("n").on_press(Msg::Deny)).id("deny");
        })
        .gap(2);
    });
}
