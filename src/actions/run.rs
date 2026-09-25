//! `run_command`: `sh -c` in the workspace root, with a time limit and a way to stop it.

use std::io::{self, Read};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process_group};

use super::Outcome;
use super::workspace::Workspace;

/// How long a command may run before it is stopped.
pub(super) const LIMIT: Duration = Duration::from_secs(120);
/// How often the command and the cancel question are checked.
const POLL: Duration = Duration::from_millis(50);
/// At most this many bytes of output are returned, from the end.
pub(super) const MAX_OUTPUT_BYTES: usize = 30 * 1024;
/// At most this many lines of output are returned, from the end.
pub(super) const MAX_OUTPUT_LINES: usize = 400;
/// Output kept in memory while the command runs; older bytes are dropped.
const KEEP_BYTES: usize = 256 * 1024;
/// How long to wait for the output to close after the command ends, and after a stop signal.
const GRACE: Duration = Duration::from_secs(2);

enum Ending {
    Exited(ExitStatus),
    Cancelled,
    TimedOut,
    Lost(io::Error),
}

#[derive(Default)]
struct Captured {
    bytes: Vec<u8>,
    dropped: bool,
}

pub(super) fn run(workspace: &Workspace, command: &str, cancelled: &dyn Fn() -> bool, limit: Duration) -> Outcome {
    if command.trim().is_empty() {
        return Outcome::failed("`run_command` needs a non-empty `command`.".to_owned());
    }
    let (mut process, reader) = match start(workspace, command) {
        Ok(started) => started,
        Err(error) => {
            return Outcome::failed(format!("The command could not be started: {error}."));
        }
    };

    // Standard output and error share one pipe, so their lines arrive in the order written.
    let captured = Arc::new(Mutex::new(Captured::default()));
    let (closed, closed_signal) = mpsc::channel();
    let sink = Arc::clone(&captured);
    thread::spawn(move || {
        drain(reader, &sink);
        let _ = closed.send(());
    });

    let started = Instant::now();
    let ending = loop {
        match process.try_wait() {
            Ok(Some(status)) => break Ending::Exited(status),
            Ok(None) => {}
            Err(error) => {
                stop(&mut process);
                break Ending::Lost(error);
            }
        }
        if cancelled() {
            stop(&mut process);
            break Ending::Cancelled;
        }
        if started.elapsed() >= limit {
            stop(&mut process);
            break Ending::TimedOut;
        }
        thread::sleep(POLL);
    };
    // A process the command left in the background can keep the pipe open after the command
    // itself has ended; it belongs to the same group, so the group is stopped.
    if closed_signal.recv_timeout(GRACE).is_err() {
        signal_group(process.id(), Signal::KILL);
        let _ = closed_signal.recv_timeout(GRACE);
    }
    let captured = captured.lock().unwrap_or_else(PoisonError::into_inner);
    let output = tail(&captured.bytes, captured.dropped);
    match ending {
        Ending::Exited(status) => Outcome::done(format!("{output}\n{}", describe(status))),
        Ending::Cancelled => Outcome::done(format!("{output}\nStopped: the person cancelled the command.")),
        Ending::TimedOut => {
            Outcome::done(format!("{output}\nStopped: the command ran longer than {} seconds.", limit.as_secs()))
        }
        Ending::Lost(error) => {
            Outcome::failed(format!("{output}\nThe command was stopped because its state could not be read: {error}."))
        }
    }
}

fn start(workspace: &Workspace, command: &str) -> io::Result<(Child, io::PipeReader)> {
    let (reader, writer) = io::pipe()?;
    let error_writer = writer.try_clone()?;
    let mut shell = Command::new("sh");
    shell
        .arg("-c")
        .arg(command)
        .current_dir(workspace.root())
        .stdin(Stdio::null())
        .stdout(writer)
        .stderr(error_writer)
        // Its own group, so stopping it also stops whatever it started.
        .process_group(0);
    let child = shell.spawn()?;
    // The command's copies of the write end must close, or the reader never sees the end.
    drop(shell);
    Ok((child, reader))
}

fn drain(mut reader: io::PipeReader, sink: &Mutex<Captured>) {
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => {
                let mut captured = sink.lock().unwrap_or_else(PoisonError::into_inner);
                captured.bytes.extend_from_slice(&buffer[..read]);
                if captured.bytes.len() > KEEP_BYTES {
                    let excess = captured.bytes.len() - KEEP_BYTES;
                    captured.bytes.drain(..excess);
                    captured.dropped = true;
                }
            }
        }
    }
}

/// Asks the whole group to end, then forces it if it has not ended within the grace time.
fn stop(process: &mut Child) {
    signal_group(process.id(), Signal::TERM);
    let deadline = Instant::now() + GRACE;
    while Instant::now() < deadline {
        if matches!(process.try_wait(), Ok(Some(_))) {
            return;
        }
        thread::sleep(POLL);
    }
    signal_group(process.id(), Signal::KILL);
    let _ = process.kill();
    let _ = process.wait();
}

/// Signals the command's whole group. This calls `kill(2)` directly: the `kill` program is not
/// on every system (a minimal container has none), and without it nothing would be stopped.
fn signal_group(group: u32, signal: Signal) {
    let Some(group) = i32::try_from(group).ok().and_then(Pid::from_raw) else { return };
    // The group may already be gone; there is nothing left to stop then.
    let _ = kill_process_group(group, signal);
}

fn describe(status: ExitStatus) -> String {
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("Exit code: {code}."),
        (None, Some(signal)) => format!("Ended by signal {signal}."),
        (None, None) => "Ended without an exit code.".to_owned(),
    }
}

/// The end of the output, cut to the byte and line caps, with a note when anything was cut.
pub(super) fn tail(bytes: &[u8], dropped: bool) -> String {
    let start = bytes.len().saturating_sub(MAX_OUTPUT_BYTES);
    let text = String::from_utf8_lossy(&bytes[start..]);
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return "(no output)".to_owned();
    }
    let keep = lines.len().min(MAX_OUTPUT_LINES);
    let cut = dropped || start > 0 || keep < lines.len();
    let shown = lines[lines.len() - keep..].join("\n");
    if cut { format!("(Output cut: only the last {keep} lines are shown.)\n{shown}") } else { shown }
}
