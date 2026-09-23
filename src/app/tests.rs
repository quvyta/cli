//! The chat driven the way a person drives it: typing, Enter, the buttons on the card and Esc,
//! against a stand-in provider on the loopback.

use std::path::PathBuf;

use qframe::env::{AssetDirs, Env};
use qframe::event::MouseKind;
use qframe::icons::GlyphMode;
use serde_json::{Value, json};

use super::*;
use crate::provider::fake::{FakeServer, calls, says};

/// Not a key of anyone's: the characters spell what it is.
const MADE_UP: &str = "not-a-real-key-0000-wxyz";

/// A folder of its own for one test: the working folder, the conversations and the settings
/// live under it, and it is removed when the test ends.
struct Place {
    base: PathBuf,
}

impl Place {
    fn new(name: &str) -> Self {
        let base = std::env::temp_dir().join(format!("qcli-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("work")).expect("the working folder");
        std::fs::create_dir_all(base.join("config")).expect("the settings folder");
        std::fs::write(base.join("key"), format!("{MADE_UP}\n")).expect("the key file");
        Self { base }
    }

    fn work(&self) -> PathBuf {
        self.base.join("work")
    }

    fn store(&self) -> Store {
        let workspace = Workspace::new(&self.work()).expect("workspace");
        Store::new(&self.base.join("conversations"), workspace.root())
    }

    fn settings(&self, address: Option<&str>) -> Settings {
        let mut settings = config::load_in(&self.base.join("config"));
        if let Some(address) = address {
            Provider {
                address: address.to_owned(),
                model: "test-model".into(),
                key_file: self.base.join("key").display().to_string(),
                key_header: "api-key".into(),
            }
            .write(&mut settings);
        }
        settings
    }

    fn app(&self, address: Option<&str>) -> QCli {
        QCli::new(Workspace::new(&self.work()).expect("workspace"), self.store(), self.settings(address))
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn env() -> Env {
    let dirs = AssetDirs {
        locale_sources: crate::locales().iter().map(|(file, text)| ((*file).to_owned(), (*text).to_owned())).collect(),
        keymap_source: Some(("keymap.toml".to_owned(), KEYMAP.to_owned())),
        ..AssetDirs::default()
    };
    Env::load(&dirs).expect("the built-in files load")
}

fn harness(app: QCli) -> Harness<QCli> {
    let mut harness = Harness::with_env(app, env(), 100, 40);
    harness.set_locale("en").set_glyph_mode(GlyphMode::Unicode).set_reduced_motion(true);
    harness
}

/// Types `text` in the writing field and presses Enter.
fn say(harness: &mut Harness<QCli>, text: &str) {
    harness.type_text(text).press("enter");
}

/// The last message of the request the server received as number `index`.
fn last_message(server: &FakeServer, index: usize) -> Value {
    server.received()[index].body["messages"]
        .as_array()
        .and_then(|messages| messages.last().cloned())
        .expect("a message")
}

fn saved(place: &Place) -> String {
    std::fs::read_to_string(place.store().file()).expect("the conversation is saved")
}

#[test]
fn a_message_goes_out_and_the_reply_streams_onto_the_screen_and_into_the_file() {
    let place = Place::new("chat");
    let server = FakeServer::start(vec![(200, says("Hello from the stand-in."))]);
    let mut harness = harness(place.app(Some(&server.address)));
    assert!(harness.screen().contains("Ask anything about this folder"));
    say(&mut harness, "hello there");

    assert!(harness.screen().contains("hello there"), "{}", harness.screen());
    assert!(harness.screen().contains("Hello from the stand-in."), "{}", harness.screen());
    assert_eq!(harness.app().phase(), &Phase::Idle);

    let request = &server.received()[0];
    assert_eq!(request.body["messages"][0]["content"][0]["text"], "hello there");
    assert!(request.body["system"].as_str().is_some_and(|system| system.contains("qcli")));
    assert_eq!(request.body["tools"].as_array().map(Vec::len), Some(5));
    assert_eq!(request.header("api-key"), Some(MADE_UP));
    assert_eq!(request.body["model"], "test-model", "the model comes from the settings");

    let file = saved(&place);
    assert!(file.contains("Hello from the stand-in."));
    assert!(!file.contains(MADE_UP), "the key never reaches the saved conversation");
    assert!(!harness.screen().contains(MADE_UP), "nor the screen");
}

#[test]
fn the_conversation_comes_back_when_the_folder_is_opened_again() {
    let place = Place::new("resume");
    let server = FakeServer::start(vec![(200, says("First answer.")), (200, says("Second answer."))]);
    {
        let mut harness = harness(place.app(Some(&server.address)));
        say(&mut harness, "first question");
    }
    let mut harness = harness(place.app(Some(&server.address)));
    assert!(harness.screen().contains("first question"));
    assert!(harness.screen().contains("First answer."));
    say(&mut harness, "second question");
    let messages = server.received()[1].body["messages"].clone();
    assert_eq!(messages.as_array().map(Vec::len), Some(3), "the earlier turn goes out with the new one");
}

#[test]
fn a_read_is_done_without_asking_and_its_result_goes_back_to_the_model() {
    let place = Place::new("read");
    std::fs::write(place.work().join("notes.txt"), "the secret word is apricot\n").expect("write");
    let server = FakeServer::start(vec![
        (200, calls("c1", "read_file", &json!({"path": "notes.txt"}))),
        (200, says("The word is apricot.")),
    ]);
    let mut harness = harness(place.app(Some(&server.address)));
    say(&mut harness, "what is the word?");

    assert_eq!(server.received().len(), 2, "no question in between: reading does not ask");
    let result = &last_message(&server, 1)["content"][0];
    assert_eq!(result["type"], "tool_result");
    assert_eq!(result["tool_use_id"], "c1");
    assert!(result["content"].as_str().is_some_and(|text| text.contains("apricot")));
    let screen = harness.screen();
    assert!(screen.contains("read notes.txt"), "{screen}");
    assert!(screen.contains("The word is apricot."));
}

#[test]
fn an_edit_shows_its_diff_and_waits_until_allow_is_clicked() {
    let place = Place::new("edit");
    let file = place.work().join("greeting.txt");
    std::fs::write(&file, "hello world\n").expect("write");
    let server = FakeServer::start(vec![
        (200, calls("e1", "edit_file", &json!({"path": "greeting.txt", "old_text": "world", "new_text": "moon"}))),
        (200, says("Changed it.")),
    ]);
    let mut harness = harness(place.app(Some(&server.address)));
    say(&mut harness, "say hello to the moon");

    let screen = harness.screen();
    assert!(screen.contains("Change greeting.txt?"), "{screen}");
    assert!(screen.contains("hello moon") && screen.contains("hello world"), "both sides of the diff: {screen}");
    assert_eq!(std::fs::read_to_string(&file).expect("read"), "hello world\n", "nothing changes before the answer");
    assert_eq!(server.received().len(), 1);

    harness.click_text("Allow");
    assert_eq!(std::fs::read_to_string(&file).expect("read"), "hello moon\n");
    let result = &last_message(&server, 1)["content"][0];
    assert_eq!(result["is_error"], false);
    assert!(harness.screen().contains("Changed it."));
}

#[test]
fn a_declined_command_is_never_run_and_the_model_hears_why() {
    let place = Place::new("decline");
    let server = FakeServer::start(vec![
        (200, calls("r1", "run_command", &json!({"command": "printf ran > ran.txt"}))),
        (200, says("Understood.")),
    ]);
    let mut harness = harness(place.app(Some(&server.address)));
    say(&mut harness, "make a file");
    assert!(harness.screen().contains("Run this command?"));
    assert!(harness.screen().contains("printf ran > ran.txt"));

    harness.click_text("Decline");
    assert!(!place.work().join("ran.txt").exists(), "the command did not run");
    let result = &last_message(&server, 1)["content"][0];
    assert_eq!(result["content"], DECLINED);
    assert_eq!(result["is_error"], true);
}

#[test]
fn the_y_key_allows_a_command_and_its_output_goes_back() {
    let place = Place::new("run");
    let server = FakeServer::start(vec![
        (200, calls("r1", "run_command", &json!({"command": "printf 'from the command'"}))),
        (200, says("Done.")),
    ]);
    let mut harness = harness(place.app(Some(&server.address)));
    say(&mut harness, "run it");
    harness.press("y");
    let result = &last_message(&server, 1)["content"][0];
    assert!(result["content"].as_str().is_some_and(|text| text.contains("from the command")), "{result}");
    assert!(harness.screen().contains("from the command"), "the start of the output is shown");
}

#[test]
fn esc_on_a_card_stops_the_turn_and_answers_every_waiting_call() {
    let place = Place::new("stop");
    let reply = crate::provider::fake::sse(&[
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": "a", "name": "run_command", "input": {}}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": "{\"command\": \"printf a\"}"}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "id": "b", "name": "list_dir", "input": {}}}),
        json!({"type": "content_block_stop", "index": 1}),
        json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}}),
        json!({"type": "message_stop"}),
    ]);
    let server = FakeServer::start(vec![(200, reply), (200, says("Fine."))]);
    let mut harness = harness(place.app(Some(&server.address)));
    say(&mut harness, "two things");
    assert!(matches!(harness.app().phase(), Phase::Asking { .. }));
    harness.press("esc");

    assert_eq!(harness.app().phase(), &Phase::Idle);
    assert_eq!(harness.app().notice(), Some(&Notice::Stopped));
    assert_eq!(server.received().len(), 1, "nothing more goes out after a stop");
    let last = harness.app().conversation().messages().last().cloned().expect("a message");
    let ids: Vec<_> = conversation::blocks(&last).map(|block| block["tool_use_id"].clone()).collect();
    assert_eq!(ids, ["a", "b"], "both calls are answered, so the next request is one the API accepts");
    assert!(harness.screen().contains("Stopped."));

    say(&mut harness, "go on");
    assert_eq!(server.received().len(), 2, "a stopped turn does not keep the next one from starting");
}

#[test]
fn a_refusal_is_shown_in_the_servers_words() {
    let place = Place::new("refused");
    let body = json!({"type": "error", "error": {"type": "authentication_error", "message": "invalid api key"}});
    let server = FakeServer::start(vec![(401, body.to_string())]);
    let mut harness = harness(place.app(Some(&server.address)));
    say(&mut harness, "hi");
    let screen = harness.screen();
    assert!(screen.contains("The provider did not answer"), "{screen}");
    assert!(screen.contains("invalid api key"));
    assert_eq!(harness.app().phase(), &Phase::Idle);
}

#[test]
fn the_folders_agents_file_goes_out_as_instructions() {
    let place = Place::new("agents");
    std::fs::write(place.work().join("AGENTS.md"), "Answer like a pirate.\n").expect("write");
    let server = FakeServer::start(vec![(200, says("Arr."))]);
    let mut harness = harness(place.app(Some(&server.address)));
    assert!(harness.screen().contains("AGENTS.md is read as instructions."));
    say(&mut harness, "hi");
    let system = server.received()[0].body["system"].as_str().unwrap_or_default().to_owned();
    assert!(system.ends_with("Answer like a pirate."), "{system}");
}

#[test]
fn without_a_provider_the_form_opens_and_saving_it_writes_the_settings() {
    let place = Place::new("form");
    let mut harness = harness(place.app(None));
    assert!(harness.screen().contains("Provider"), "{}", harness.screen());
    assert!(harness.is_focused(form::FIRST));

    harness.click_text("Save");
    assert!(harness.screen().contains("This cannot be empty."), "the empty fields say so");
    assert!(harness.is_focused(form::FIRST), "and focus goes back to the first of them");

    harness.type_text("http://127.0.0.1:9/anthropic").press("tab").type_text("some-model").press("tab");
    harness.type_text(&place.base.join("missing-key").display().to_string());
    harness.click_text("Save");
    assert!(
        harness.screen().contains("cannot be read"),
        "a key file that is not there is caught: {}",
        harness.screen()
    );

    assert!(harness.is_focused("provider-key-file"));
    harness.press("ctrl+a").type_text(&place.base.join("key").display().to_string());
    harness.click_text("Save");
    assert!(!harness.screen().contains("Key file"), "the form closed: {}", harness.screen());
    let conf = std::fs::read_to_string(place.base.join("config/cli.conf")).expect("cli.conf is written");
    assert!(conf.contains("model = \"some-model\""), "{conf}");
    assert!(!conf.contains(MADE_UP), "only the key file's path is written, never the key");
}

#[test]
fn ctrl_n_puts_the_conversation_aside_and_starts_a_new_one() {
    let place = Place::new("new");
    let server = FakeServer::start(vec![(200, says("Remember me."))]);
    let mut harness = harness(place.app(Some(&server.address)));
    say(&mut harness, "keep this");
    harness.press("ctrl+n");
    assert!(harness.app().conversation().is_empty());
    assert!(harness.screen().contains("Ask anything about this folder"));
    let kept = std::fs::read_dir(place.base.join("conversations")).expect("folder").count();
    assert_eq!(kept, 1, "the old conversation is kept under another name");
}

#[test]
fn the_end_of_the_conversation_stays_in_view_as_it_grows() {
    let place = Place::new("follow");
    let long: String = (1..=60).map(|n| format!("Line number {n} of a long answer.\n\n")).collect();
    let server = FakeServer::start(vec![(200, says(&long))]);
    let mut harness = harness(place.app(Some(&server.address)));
    say(&mut harness, "tell me a lot");
    let screen = harness.screen();
    assert!(screen.contains("Line number 60"), "the last line is on screen: {screen}");
    assert!(screen.contains("Write a message"), "and the writing field under it");
}

#[test]
fn a_person_scrolled_up_to_read_is_not_pulled_down_while_the_reply_streams() {
    let place = Place::new("read-back");
    let long: String = (1..=60).map(|n| format!("Line number {n} of a long answer.\n\n")).collect();
    // A long answer that ends by asking to run a command; the reply after the command streams
    // in while the person reads further up.
    let first = crate::provider::fake::sse(&[
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": long}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "id": "r1", "name": "run_command", "input": {}}}),
        json!({"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "{\"command\": \"printf done\"}"}}),
        json!({"type": "content_block_stop", "index": 1}),
        json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}}),
        json!({"type": "message_stop"}),
    ]);
    let more: String = (1..=20).map(|n| format!("More text number {n}.\n\n")).collect();
    let server = FakeServer::start(vec![(200, first), (200, says(&more)), (200, says("Back at the end."))]);
    let mut harness = harness(place.app(Some(&server.address)));
    say(&mut harness, "tell me a lot");
    assert!(harness.screen().contains("Run this command?"), "the card is in view at the end");

    // The wheel over the conversation, the way a person scrolls up to read.
    for _ in 0..10 {
        harness.mouse(MouseKind::ScrollUp, 40, 10);
    }
    let reading = harness.screen();
    assert!(!reading.contains("Run this command?"), "scrolled up: {reading}");

    // Allowing the command from the keyboard lets the next reply stream in while the person reads.
    harness.press("y");
    assert_eq!(server.received().len(), 2, "the reply after the command came");
    // The lines being read stay where they are. The writing field comes back under the
    // conversation once the card is answered and takes the bottom row, nothing more.
    // Only the words: the scrollbar at the right edge rightly changes as the content grows.
    let lines = |screen: &str| {
        screen
            .lines()
            .filter_map(|line| line.split(" of a long").next().filter(|_| line.contains("Line number")))
            .map(|line| line.trim().to_owned())
            .collect::<Vec<_>>()
    };
    let (before, after) = (lines(&reading), lines(&harness.screen()));
    assert!(!after.is_empty() && before.starts_with(&after), "the view moved: {}", harness.screen());
    assert!(before.len() - after.len() <= 1);
    assert!(!harness.screen().contains("More text number 20"));
    assert!(harness.screen().contains("Write a message"), "the writing field stays under the conversation");

    // Sending the next message opens the conversation at its end again.
    say(&mut harness, "and now?");
    assert!(harness.screen().contains("Back at the end."), "{}", harness.screen());
}

/// A real model reads a real file through the whole chat. Ignored unless asked for by name, like
/// the provider's live tries; it takes the provider from the same `QCLI_LIVE_*` variables.
#[test]
#[ignore = "talks to a real provider; run by hand"]
fn live_a_real_model_reads_a_file_it_is_asked_about() {
    let place = Place::new("live");
    std::fs::write(place.work().join("pantry.txt"), "The jar on the top shelf holds cardamom.\n").expect("write");
    let variable = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set"));
    let mut settings = config::load_in(&place.base.join("config"));
    Provider {
        address: variable("QCLI_LIVE_ADDRESS"),
        model: variable("QCLI_LIVE_MODEL"),
        key_file: variable("QCLI_LIVE_KEY_FILE"),
        key_header: std::env::var("QCLI_LIVE_HEADER").unwrap_or_default(),
    }
    .write(&mut settings);
    let app = QCli::new(Workspace::new(&place.work()).expect("workspace"), place.store(), settings);
    let mut harness = harness(app);
    say(&mut harness, "Read pantry.txt and tell me in one short sentence what the jar on the top shelf holds.");
    println!("{}", harness.screen());
    assert_eq!(harness.app().notice(), None, "no error");
    let messages = harness.app().conversation().messages();
    let read = messages.iter().flat_map(conversation::blocks).any(|block| block["name"] == "read_file");
    assert!(read, "the model used the read action");
    let answer = messages.last().map(ToString::to_string).unwrap_or_default().to_lowercase();
    assert!(answer.contains("cardamom"), "the answer comes from the file: {answer}");
}

/// The folders of the update notice under the test's own place, never the person's.
fn update_folders(place: &Place) -> config::UpdateFolders {
    config::UpdateFolders { config: place.base.join("config"), state: place.base.join("state") }
}

#[test]
fn qcli_asks_once_at_start_for_a_newer_version_of_itself() {
    let place = Place::new("updates");
    let harness = harness(place.app(None).update_notice(Some(update_folders(&place))));
    let asked = harness.update_checks().to_vec();
    assert_eq!(asked.len(), 1, "one question at start");
    assert_eq!((asked[0].package(), asked[0].current()), ("quvyta-cli", env!("CARGO_PKG_VERSION")));
}

#[test]
fn with_the_familys_switch_off_nothing_is_asked() {
    let place = Place::new("updates-off");
    std::fs::write(place.base.join("config/quvyta.conf"), "update-notice = false\n").expect("the family's file");
    let mut harness = harness(place.app(None).update_notice(Some(update_folders(&place))));
    assert!(harness.update_checks().is_empty(), "nothing is asked");
    harness.set_latest_version(Some("9.4.7"));
    assert!(!harness.screen().contains("is out"));
}
