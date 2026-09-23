//! The `qcli` command: opens the chat in the folder it is started in.

use std::process::ExitCode;

fn main() -> ExitCode {
    match qcli::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
