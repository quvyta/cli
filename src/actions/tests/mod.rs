//! Tests for the actions. Each test works in its own fresh folder under the system temporary
//! folder and removes it at the end; commands are only fixed, harmless shell built-ins,
//! `printf` and `sleep`.

mod call;
mod confine;
mod edit;
mod files;
mod run;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{Action, Outcome, Workspace};

/// A fresh folder, removed when dropped.
pub(super) struct Scratch {
    path: PathBuf,
}

impl Scratch {
    pub(super) fn new(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("qcli-actions-{}-{label}-{number}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create the test folder");
        Self { path }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    /// Writes a file under the folder, creating its parents.
    pub(super) fn write(&self, relative: &str, content: impl AsRef<[u8]>) {
        let target = self.path.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).expect("create parent folders");
        }
        fs::write(target, content).expect("write a test file");
    }

    pub(super) fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.path.join(relative)).expect("read a test file")
    }

    pub(super) fn workspace(&self) -> Workspace {
        Workspace::new(&self.path).expect("open the workspace")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub(super) fn never() -> bool {
    false
}

pub(super) fn perform(workspace: &Workspace, action: &Action) -> Outcome {
    workspace.perform(action, &never)
}

pub(super) fn read(path: &str) -> Action {
    Action::Read { path: path.to_owned() }
}

pub(super) fn list(path: &str) -> Action {
    Action::List { path: path.to_owned() }
}

pub(super) fn search(pattern: &str, path: &str) -> Action {
    Action::Search { pattern: pattern.to_owned(), path: path.to_owned() }
}

pub(super) fn edit(path: &str, old_text: &str, new_text: &str) -> Action {
    Action::Edit { path: path.to_owned(), old_text: old_text.to_owned(), new_text: new_text.to_owned() }
}

pub(super) fn run(command: &str) -> Action {
    Action::Run { command: command.to_owned() }
}
