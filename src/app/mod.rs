//! The chat: one screen where the person writes, the reply streams in, and the actions the model
//! asks for are done, or shown and asked about first.
//!
//! A turn goes round like this. What the person wrote joins the conversation and a request goes
//! out on a task, which hands the reply back piece by piece. When the reply asks for tools, each
//! call is taken in order: reading, listing and searching are done at once; an edit or a command
//! is shown on a card and waits for the person. When every call has its result the results go
//! back to the model in the next request, until it ends its turn. Esc stops whatever is going on
//! and every call still waiting is answered as stopped, so the conversation stays one the API
//! accepts.

mod form;
mod view;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, VecDeque};
use std::io;

use qframe::prelude::*;
use qframe::runtime::{Task, TaskId};
use qframe::storage::{Family, Settings};

use crate::actions::{Action, Outcome, Preview, Workspace};
use crate::config::{self, Missing, Provider};
use crate::conversation::{self, Call, Conversation};
use crate::instructions;
use crate::provider::{Delta, KeyError, ProviderError, Reply, Stop};
use crate::store::Store;

pub use form::{Field as FormField, ProviderForm};

/// The keys of the application, layered over the framework's.
pub const KEYMAP: &str = include_str!("../../keymap.toml");

/// How many requests one turn may make before it stops and says so. A model that keeps asking
/// for tools would otherwise go on spending the person's money without an end.
pub const MAX_ROUNDS: usize = 50;

/// What the model is told when the person declines an action.
const DECLINED: &str = "The person declined this action.";

/// What calls still waiting are answered with when the person stops the turn.
const STOPPED: &str = "Not done: the person stopped the turn.";

/// Everything that can happen.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The text in the writing field changed.
    Draft(String),
    /// The person sent what they wrote.
    Send,
    /// A piece of the reply of round `.0` arrived.
    Delta(u64, Delta),
    /// The reply of round `.0` is complete, or failed.
    Replied(u64, Result<Reply, ProviderError>),
    /// The action of round `.0` for call `.1` is done.
    Done(u64, String, Outcome),
    /// The person allows the action on the card.
    Allow,
    /// The person declines the action on the card.
    Deny,
    /// Stop the reply or the action going on.
    Stop,
    /// Put the conversation aside and start a new one.
    New,
    /// Open the provider form.
    OpenProvider,
    /// A field of the provider form changed.
    Form(FormField, String),
    /// Save the provider form.
    SaveProvider,
    /// Close the provider form without saving.
    CloseProvider,
}

/// What the chat is doing.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Phase {
    /// Waiting for the person to write.
    #[default]
    Idle,
    /// A reply is streaming in.
    Replying,
    /// An action waits for the person's answer.
    Asking {
        /// The call.
        call: Call,
        /// The action it asks for.
        action: Action,
        /// What it would do.
        preview: Preview,
    },
    /// An action is being done.
    Acting {
        /// The call.
        call: Call,
        /// The action.
        action: Action,
    },
}

/// Something the screen says under the conversation until the next turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// The provider could not be asked or did not answer.
    Provider(String),
    /// A field of the provider settings is empty.
    Unset,
    /// The key file gave no key.
    Key(KeyError),
    /// The person stopped the turn.
    Stopped,
    /// The reply ran out of tokens.
    Cut,
    /// The turn made as many requests as one may.
    Rounds,
    /// The conversation could not be saved.
    Unsaved(String),
}

/// A piece of the reply as it streams in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Live {
    /// Thinking.
    Thinking(String),
    /// Text.
    Text(String),
    /// A tool call by its tool's name.
    Call(String),
}

/// The application.
pub struct QCli {
    workspace: Workspace,
    store: Store,
    conversation: Conversation,
    settings: Settings,
    provider: Provider,
    draft: String,
    phase: Phase,
    live: Vec<Live>,
    /// The calls of the last reply still to be taken, and the results of those taken.
    queue: VecDeque<Call>,
    results: Vec<Value>,
    /// What each edit and command showed before it was allowed, by call id.
    previews: HashMap<String, Preview>,
    /// Requests made in the current turn.
    rounds: usize,
    /// The number of the current round. A message of an earlier one, which arrives after the
    /// person stopped it, is ignored.
    round: u64,
    task: Option<TaskId>,
    notice: Option<Notice>,
    form: Option<ProviderForm>,
    /// The number of the turn, part of the conversation view's id: a new turn opens the view at
    /// its end even when the person had scrolled up to read.
    turn: u64,
}

type Value = serde_json::Value;

impl QCli {
    /// The chat in `workspace`, going on with the conversation `store` keeps, talking to the
    /// provider in `settings`. The provider form opens at once when the provider is not set up.
    #[must_use]
    pub fn new(workspace: Workspace, store: Store, settings: Settings) -> Self {
        let provider = Provider::from_settings(&settings);
        let form = provider.missing_field().is_some().then(|| ProviderForm::from_provider(&provider));
        Self {
            workspace,
            conversation: store.load(),
            store,
            settings,
            provider,
            draft: String::new(),
            phase: Phase::Idle,
            live: Vec::new(),
            queue: VecDeque::new(),
            results: Vec::new(),
            previews: HashMap::new(),
            rounds: 0,
            round: 0,
            task: None,
            notice: None,
            form,
            turn: 0,
        }
    }

    /// The conversation so far.
    #[must_use]
    pub fn conversation(&self) -> &Conversation {
        &self.conversation
    }

    /// What the chat is doing.
    #[must_use]
    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    /// What the screen says under the conversation, if anything.
    #[must_use]
    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    fn busy(&self) -> bool {
        self.phase != Phase::Idle
    }

    /// Gives the writing field the focus: after a card is answered its buttons are gone, and the
    /// next key the person presses belongs to the field.
    fn to_input() -> Command<Msg> {
        Command::focus(view::INPUT)
    }

    fn save(&mut self) {
        if let Err(error) = self.store.save(&self.conversation) {
            self.notice = Some(Notice::Unsaved(error.to_string()));
        }
    }

    fn send(&mut self) -> Command<Msg> {
        let text = self.draft.trim().to_owned();
        if text.is_empty() || self.busy() {
            return Command::none();
        }
        if self.provider.missing_field().is_some() {
            self.form = Some(ProviderForm::from_provider(&self.provider));
            return Command::none();
        }
        self.draft.clear();
        self.notice = None;
        self.conversation.say(&text);
        self.save();
        self.rounds = 0;
        self.turn += 1;
        Command::batch([self.ask(), Self::to_input()])
    }

    /// Sends the conversation as it is and waits for the reply.
    fn ask(&mut self) -> Command<Msg> {
        let endpoint = match self.provider.endpoint() {
            Ok(endpoint) => endpoint,
            Err(Missing::Field(_)) => return self.stop_with(Notice::Unset),
            Err(Missing::Key(error)) => return self.stop_with(Notice::Key(error)),
        };
        if self.rounds >= MAX_ROUNDS {
            return self.stop_with(Notice::Rounds);
        }
        self.rounds += 1;
        self.round += 1;
        self.live.clear();
        self.phase = Phase::Replying;
        let round = self.round;
        let body = instructions::body(&self.conversation, &instructions::system(self.workspace.root()));
        let task = Task::new("reply", move |cx| {
            let reply = endpoint.stream(&body, &mut |delta| cx.send(Msg::Delta(round, delta)), &|| cx.is_cancelled());
            Ok(Msg::Replied(round, reply))
        });
        self.task = Some(task.id());
        Command::task(task)
    }

    fn stop_with(&mut self, notice: Notice) -> Command<Msg> {
        self.phase = Phase::Idle;
        self.notice = Some(notice);
        Self::to_input()
    }

    fn replied(&mut self, reply: Result<Reply, ProviderError>) -> Command<Msg> {
        self.task = None;
        self.live.clear();
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) => return self.stop_with(Notice::Provider(error.to_string())),
        };
        let stop = reply.stop.clone();
        self.conversation.reply(reply.content);
        self.save();
        match stop {
            Stop::ToolUse => {
                self.queue = self.conversation.open_calls().into();
                self.results.clear();
                self.next()
            }
            Stop::MaxTokens => self.stop_with(Notice::Cut),
            Stop::EndTurn | Stop::Other(_) => {
                self.phase = Phase::Idle;
                Self::to_input()
            }
        }
    }

    /// Takes the next call of the reply, or sends the results when every call has one.
    fn next(&mut self) -> Command<Msg> {
        let Some(call) = self.queue.pop_front() else {
            self.conversation.results(std::mem::take(&mut self.results));
            self.save();
            return Command::batch([self.ask(), Self::to_input()]);
        };
        let action = match Action::from_call(&call.name, &call.input) {
            Ok(action) => action,
            Err(reason) => return self.answer(&call.id, &reason, true),
        };
        if !action.needs_approval() {
            return self.act(call, action);
        }
        match self.workspace.preview(&action) {
            Ok(preview) => {
                self.previews.insert(call.id.clone(), preview.clone());
                self.phase = Phase::Asking { call, action, preview };
                Command::focus(view::ALLOW)
            }
            Err(reason) => self.answer(&call.id, &reason, true),
        }
    }

    /// Records the result of call `id` and goes on with the next.
    fn answer(&mut self, id: &str, text: &str, is_error: bool) -> Command<Msg> {
        self.results.push(conversation::result(id, text, is_error));
        self.next()
    }

    fn act(&mut self, call: Call, action: Action) -> Command<Msg> {
        let workspace = self.workspace.clone();
        let round = self.round;
        let id = call.id.clone();
        let doing = action.clone();
        let task = Task::new("action", move |cx| {
            let outcome = workspace.perform(&doing, &|| cx.is_cancelled());
            Ok(Msg::Done(round, id, outcome))
        });
        self.task = Some(task.id());
        self.phase = Phase::Acting { call, action };
        Command::batch([Command::task(task), Self::to_input()])
    }

    /// Stops what is going on. A reply keeps the text that had come; calls still waiting are
    /// answered as stopped.
    fn stop(&mut self) -> Command<Msg> {
        let cancel = self.task.take().map_or_else(Command::none, Command::cancel_task);
        self.round += 1;
        match std::mem::take(&mut self.phase) {
            Phase::Idle => return cancel,
            Phase::Replying => {
                let text: String = self
                    .live
                    .iter()
                    .filter_map(|live| if let Live::Text(text) = live { Some(text.as_str()) } else { None })
                    .collect();
                if !text.trim().is_empty() {
                    self.conversation.reply(vec![serde_json::json!({"type": "text", "text": text})]);
                }
                self.live.clear();
            }
            Phase::Asking { call, .. } | Phase::Acting { call, .. } => {
                self.results.push(conversation::result(&call.id, STOPPED, true));
                for waiting in std::mem::take(&mut self.queue) {
                    self.results.push(conversation::result(&waiting.id, STOPPED, true));
                }
                self.conversation.results(std::mem::take(&mut self.results));
            }
        }
        self.save();
        self.notice = Some(Notice::Stopped);
        Command::batch([cancel, Self::to_input()])
    }

    fn new_conversation(&mut self) -> Command<Msg> {
        let stopped = self.stop();
        if let Err(error) = self.store.put_aside() {
            self.notice = Some(Notice::Unsaved(error.to_string()));
            return stopped;
        }
        self.conversation = Conversation::default();
        self.previews.clear();
        self.notice = None;
        self.turn += 1;
        Command::batch([stopped, Self::to_input()])
    }

    fn save_provider(&mut self) -> Command<Msg> {
        let Some(form) = self.form.as_mut() else { return Command::none() };
        let Some(provider) = form.check() else {
            return form.first_problem().map_or_else(Command::none, Command::focus);
        };
        provider.write(&mut self.settings);
        if let Err(error) = self.settings.save() {
            form.failed(&error);
            return Command::none();
        }
        self.provider = provider;
        self.form = None;
        if matches!(self.notice, Some(Notice::Unset | Notice::Key(_))) {
            self.notice = None;
        }
        Self::to_input()
    }
}

impl App for QCli {
    type Msg = Msg;

    fn init(&mut self) -> Command<Msg> {
        match &self.form {
            Some(_) => Command::focus(form::FIRST),
            None => Self::to_input(),
        }
    }

    fn update(&mut self, msg: Msg) -> Command<Msg> {
        match msg {
            Msg::Draft(text) => {
                self.draft = text;
                Command::none()
            }
            Msg::Send => self.send(),
            Msg::Delta(round, delta) if round == self.round => {
                match (delta, self.live.last_mut()) {
                    (Delta::Text(piece), Some(Live::Text(text)))
                    | (Delta::Thinking(piece), Some(Live::Thinking(text))) => {
                        text.push_str(&piece);
                    }
                    (Delta::Text(piece), _) => self.live.push(Live::Text(piece)),
                    (Delta::Thinking(piece), _) => self.live.push(Live::Thinking(piece)),
                    (Delta::ToolCall(name), _) => self.live.push(Live::Call(name)),
                }
                Command::none()
            }
            Msg::Replied(round, reply) if round == self.round => self.replied(reply),
            Msg::Done(round, id, outcome) if round == self.round => {
                self.task = None;
                self.answer(&id, &outcome.text, outcome.is_error)
            }
            Msg::Delta(..) | Msg::Replied(..) | Msg::Done(..) => Command::none(),
            Msg::Allow => match std::mem::take(&mut self.phase) {
                Phase::Asking { call, action, .. } => self.act(call, action),
                other => {
                    self.phase = other;
                    Command::none()
                }
            },
            Msg::Deny => match std::mem::take(&mut self.phase) {
                Phase::Asking { call, .. } => {
                    self.phase = Phase::Idle;
                    self.answer(&call.id, DECLINED, true)
                }
                other => {
                    self.phase = other;
                    Command::none()
                }
            },
            Msg::Stop => self.stop(),
            Msg::New => self.new_conversation(),
            Msg::OpenProvider => {
                self.form = Some(ProviderForm::from_provider(&self.provider));
                Command::focus(form::FIRST)
            }
            Msg::Form(field, value) => {
                if let Some(form) = self.form.as_mut() {
                    form.set(field, value);
                }
                Command::none()
            }
            Msg::SaveProvider => self.save_provider(),
            Msg::CloseProvider => {
                self.form = None;
                Self::to_input()
            }
        }
    }

    fn action(&self, name: &str) -> Option<Msg> {
        if self.form.is_some() {
            return None;
        }
        match name {
            "stop" if self.busy() => Some(Msg::Stop),
            "allow" if matches!(self.phase, Phase::Asking { .. }) => Some(Msg::Allow),
            "deny" if matches!(self.phase, Phase::Asking { .. }) => Some(Msg::Deny),
            "new" => Some(Msg::New),
            "provider" => Some(Msg::OpenProvider),
            _ => None,
        }
    }

    fn view(&self, ui: &mut View<'_, Msg>) {
        view::view(self, ui);
    }
}

/// Opens qcli in the folder it was started in.
///
/// # Errors
///
/// When the folder cannot be read, or the terminal cannot be used.
pub fn run() -> io::Result<()> {
    let folder = std::env::current_dir()?;
    let workspace = Workspace::new(&folder)?;
    let conversations = Family::QUVYTA
        .state_dir(config::APP)
        .unwrap_or_else(|| std::env::temp_dir().join("quvyta-cli"))
        .join("conversations");
    let store = Store::new(&conversations, workspace.root());
    let settings = config::load();
    let preferences = config::preferences();
    let app = QCli::new(workspace, store, settings.clone());
    let mut runtime =
        Runtime::new(app).settings(&settings).preferences(&preferences).keymap_source("keymap.toml", KEYMAP);
    for &(file, text) in crate::locales() {
        runtime = runtime.locale_source(file, text);
    }
    runtime.run()
}
