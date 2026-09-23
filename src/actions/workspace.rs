//! The workspace folder: confining paths to it and dispatching actions.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{Action, Outcome, Preview, edit, list, read, run, search};

/// The folder the actions are confined to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// The folder the actions are confined to; it is canonicalized, so links are resolved once.
    ///
    /// # Errors
    ///
    /// When the folder does not exist, cannot be resolved or is not a folder.
    pub fn new(root: &Path) -> io::Result<Self> {
        let root = fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err(io::Error::new(io::ErrorKind::NotADirectory, "the workspace must be a folder"));
        }
        Ok(Self { root })
    }

    /// The canonical workspace folder.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// What an action would do, for the approval card. Nothing changes on disk.
    ///
    /// # Errors
    ///
    /// When the action cannot be done as asked (a path outside the workspace, an edit whose text
    /// does not match exactly once); the sentence is the one [`Self::perform`] would give.
    pub fn preview(&self, action: &Action) -> Result<Preview, String> {
        match action {
            Action::Read { path } | Action::List { path } | Action::Search { path, .. } => {
                self.resolve(path).map(|_| Preview::Plain)
            }
            Action::Edit { path, old_text, new_text } => {
                edit::plan(self, path, old_text, new_text).map(|plan| plan.preview())
            }
            Action::Run { command } => Ok(Preview::Command { command: command.clone() }),
        }
    }

    /// Does the action. It never panics: every failure becomes an [`Outcome`] with
    /// `is_error` set and a sentence the model can act on. `cancelled` is asked about every
    /// 50 ms while a command runs; when it answers `true` the command is stopped.
    pub fn perform(&self, action: &Action, cancelled: &dyn Fn() -> bool) -> Outcome {
        let result = match action {
            Action::Read { path } => read::read(self, path),
            Action::List { path } => list::list(self, path),
            Action::Search { pattern, path } => search::search(self, pattern, path),
            Action::Edit { path, old_text, new_text } => edit::edit(self, path, old_text, new_text),
            Action::Run { command } => return run::run(self, command, cancelled, run::LIMIT),
        };
        match result {
            Ok(text) => Outcome::done(text),
            Err(text) => Outcome::failed(text),
        }
    }

    /// Finds the real location of `path` and makes sure it is inside the workspace.
    ///
    /// The path is relative to the root; an absolute path is accepted only when it lands inside.
    /// Links are followed: an existing path is canonicalized, and for one that does not exist yet
    /// its nearest existing ancestor is, with the missing names added back. Anything whose real
    /// location is outside the root is refused.
    pub(super) fn resolve(&self, path: &str) -> Result<PathBuf, String> {
        let given = Path::new(path);
        let joined = if given.is_absolute() { given.to_path_buf() } else { self.root.join(given) };
        let mut existing = joined.as_path();
        let mut missing = Vec::new();
        while fs::symlink_metadata(existing).is_err() {
            // A missing name followed by `..` cannot be resolved without guessing, so it is
            // refused rather than read lexically.
            match (existing.file_name(), existing.parent()) {
                (Some(name), Some(parent)) => {
                    missing.push(name);
                    existing = parent;
                }
                _ => {
                    return Err(format!("`{path}` cannot be resolved; use a plain path inside the working folder."));
                }
            }
        }
        let real = fs::canonicalize(existing).map_err(|_| {
            format!("`{path}` goes through a link that leads nowhere; use a path inside the working folder.")
        })?;
        let real = missing.iter().rev().fold(real, |acc, name| acc.join(name));
        if real.starts_with(&self.root) {
            Ok(real)
        } else {
            Err(format!("`{path}` is outside the working folder; only paths inside it can be used."))
        }
    }

    /// A resolved path as the model sees it: relative to the root, `.` for the root itself.
    pub(super) fn relative(&self, real: &Path) -> String {
        let relative = real.strip_prefix(&self.root).unwrap_or(real);
        if relative.as_os_str().is_empty() { ".".to_owned() } else { relative.to_string_lossy().into_owned() }
    }
}

/// A file-system failure as a sentence for the model.
pub(super) fn io_problem(shown: &str, error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::NotFound => format!("`{shown}` does not exist."),
        io::ErrorKind::PermissionDenied => format!("`{shown}` cannot be used: permission denied."),
        _ => format!("`{shown}` cannot be used: {error}."),
    }
}
