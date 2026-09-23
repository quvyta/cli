//! The `quvyta-cli` command, the long name of `qcli`: opens the chat in the folder it is started in.

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
