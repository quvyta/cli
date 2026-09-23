//! `run_command`, with fixed harmless commands only.

use std::cell::Cell;
use std::time::{Duration, Instant};

use super::super::Preview;
use super::super::run as run_module;
use super::{Scratch, never, perform, run};

#[test]
fn output_and_exit_code_come_back() {
    let scratch = Scratch::new("run");
    let outcome = perform(&scratch.workspace(), &run("printf 'hello\\n'"));
    assert!(!outcome.is_error);
    assert_eq!(outcome.text, "hello\nExit code: 0.");
}

#[test]
fn a_failing_command_is_not_an_error_the_code_tells() {
    let scratch = Scratch::new("run-exit");
    let outcome = perform(&scratch.workspace(), &run("sh -c 'exit 3'"));
    assert!(!outcome.is_error);
    assert_eq!(outcome.text, "(no output)\nExit code: 3.");
}

#[test]
fn both_streams_arrive_in_the_order_written() {
    let scratch = Scratch::new("run-order");
    let outcome = perform(&scratch.workspace(), &run("printf 'a\\n'; printf 'b\\n' >&2; printf 'c\\n'"));
    assert_eq!(outcome.text, "a\nb\nc\nExit code: 0.");
}

#[test]
fn the_command_runs_in_the_root_without_input() {
    let scratch = Scratch::new("run-cwd");
    scratch.write("marker.txt", "here");
    let outcome =
        perform(&scratch.workspace(), &run("[ -f marker.txt ] && printf found; read line || printf ' no-input'"));
    assert_eq!(outcome.text, "found no-input\nExit code: 0.");
}

#[test]
fn a_long_output_keeps_its_end_and_says_it_was_cut() {
    let scratch = Scratch::new("run-cap");
    let outcome =
        perform(&scratch.workspace(), &run("i=0; while [ $i -lt 1000 ]; do printf 'line %s\\n' $i; i=$((i+1)); done"));
    let first_kept = 1000 - run_module::MAX_OUTPUT_LINES;
    assert!(outcome.text.starts_with(&format!(
        "(Output cut: only the last {} lines are shown.)\nline {first_kept}\n",
        run_module::MAX_OUTPUT_LINES
    )));
    assert!(outcome.text.ends_with("line 999\nExit code: 0."));
    assert!(!outcome.text.contains(&format!("line {}\n", first_kept - 1)));
}

#[test]
fn the_output_is_also_capped_by_bytes() {
    let wide = vec![b'x'; run_module::MAX_OUTPUT_BYTES * 2];
    let text = run_module::tail(&wide, false);
    assert!(text.starts_with("(Output cut"));
    assert!(text.len() <= run_module::MAX_OUTPUT_BYTES + 60);
    assert_eq!(run_module::tail(b"", false), "(no output)");
}

#[test]
fn the_time_limit_stops_the_command() {
    let scratch = Scratch::new("run-limit");
    let started = Instant::now();
    let outcome =
        run_module::run(&scratch.workspace(), "printf 'started\\n'; sleep 30", &never, Duration::from_secs(1));
    assert!(started.elapsed() < Duration::from_secs(15), "stopped in time");
    assert!(!outcome.is_error);
    assert_eq!(outcome.text, "started\nStopped: the command ran longer than 1 seconds.");
}

#[test]
fn cancelling_stops_the_command() {
    let scratch = Scratch::new("run-cancel");
    let asked = Cell::new(0);
    let cancelled = || {
        asked.set(asked.get() + 1);
        asked.get() > 4
    };
    let started = Instant::now();
    let outcome = scratch.workspace().perform(&run("printf 'started\\n'; sleep 30"), &cancelled);
    assert!(started.elapsed() < Duration::from_secs(15), "stopped in time");
    assert_eq!(outcome.text, "started\nStopped: the person cancelled the command.");
}

#[test]
fn a_process_left_in_the_background_is_stopped_with_its_group() {
    let scratch = Scratch::new("run-background");
    let started = Instant::now();
    let outcome = perform(&scratch.workspace(), &run("(sleep 4; printf late > late.txt) & printf done"));
    assert!(started.elapsed() < Duration::from_secs(15), "returned in time");
    assert_eq!(outcome.text, "done\nExit code: 0.");
    // Had the background part survived, it would have written its file by now.
    std::thread::sleep(Duration::from_secs(6).saturating_sub(started.elapsed()));
    assert!(!scratch.path().join("late.txt").exists(), "the group was stopped");
}

#[test]
fn a_time_limit_stops_what_the_command_started_too() {
    let scratch = Scratch::new("run-limit-group");
    let started = Instant::now();
    let outcome = run_module::run(
        &scratch.workspace(),
        "(sleep 3; printf late > late.txt) & sleep 30",
        &never,
        Duration::from_secs(1),
    );
    assert!(outcome.text.ends_with("longer than 1 seconds."), "{}", outcome.text);
    std::thread::sleep(Duration::from_secs(5).saturating_sub(started.elapsed()));
    assert!(!scratch.path().join("late.txt").exists(), "the group was stopped");
}

#[test]
fn a_command_that_cannot_start_is_an_error() {
    let scratch = Scratch::new("run-start");
    let workspace = scratch.workspace();
    std::fs::remove_dir(scratch.path()).unwrap();
    let outcome = perform(&workspace, &run("printf hi"));
    assert!(outcome.is_error);
    assert!(outcome.text.starts_with("The command could not be started"), "{}", outcome.text);

    let empty = perform(&workspace, &run("  "));
    assert!(empty.is_error);
}

#[test]
fn the_preview_shows_the_command() {
    let scratch = Scratch::new("run-preview");
    assert_eq!(
        scratch.workspace().preview(&run("cargo test")),
        Ok(Preview::Command { command: "cargo test".to_owned() })
    );
}
