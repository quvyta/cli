//! `edit_file`: replace one exact piece of text, or create a new file; written atomically.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::workspace::{Workspace, io_problem};
use super::{Preview, diff};

/// Lines of unchanged text shown on each side of an edit.
const CONTEXT: usize = 3;

/// An edit worked out in full but not yet written.
pub(super) struct Plan {
    real: PathBuf,
    shown: String,
    before: String,
    after: String,
    existed: bool,
    replaced: Option<usize>,
}

impl Plan {
    pub(super) fn preview(&self) -> Preview {
        Preview::Diff { path: self.shown.clone(), lines: diff::diff(&self.before, &self.after, CONTEXT) }
    }
}

pub(super) fn plan(workspace: &Workspace, path: &str, old_text: &str, new_text: &str) -> Result<Plan, String> {
    if path.trim().is_empty() || path.trim() == "." {
        return Err("`edit_file` needs the path of a file.".to_owned());
    }
    if !old_text.is_empty() && old_text == new_text {
        return Err("`old_text` and `new_text` are the same; there is nothing to change.".to_owned());
    }
    let real = workspace.resolve(path)?;
    let shown = workspace.relative(&real);
    if real.is_dir() {
        return Err(format!("`{shown}` is a folder, not a file."));
    }
    let current = match fs::read(&real) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(io_problem(&shown, &error)),
    };
    let Some(bytes) = current else {
        if !old_text.is_empty() {
            return Err(format!("`{shown}` does not exist; to create it, leave `old_text` empty."));
        }
        return Ok(Plan {
            real,
            shown,
            before: String::new(),
            after: new_text.to_owned(),
            existed: false,
            replaced: None,
        });
    };
    let before = String::from_utf8(bytes).map_err(|_| format!("`{shown}` is not a text file; it cannot be edited."))?;
    if old_text.is_empty() {
        if !before.is_empty() {
            return Err(format!("`{shown}` already exists and is not empty; give the text to replace in `old_text`."));
        }
        return Ok(Plan { real, shown, before, after: new_text.to_owned(), existed: true, replaced: None });
    }
    match occurrences(&before, old_text) {
        0 => Err(format!(
            "`old_text` does not occur in `{shown}`; read the file and copy the text exactly, including spaces and indentation."
        )),
        1 => {
            let after = before.replacen(old_text, new_text, 1);
            Ok(Plan { real, shown, before, after, existed: true, replaced: Some(count_lines(old_text)) })
        }
        many => Err(format!(
            "`old_text` occurs {many} times in `{shown}`; include more of the surrounding lines so it matches exactly once."
        )),
    }
}

pub(super) fn edit(workspace: &Workspace, path: &str, old_text: &str, new_text: &str) -> Result<String, String> {
    let plan = plan(workspace, path, old_text, new_text)?;
    let shown = &plan.shown;
    if !plan.existed
        && let Some(parent) = plan.real.parent()
    {
        // The parent is the resolved, confined location, so the new folders stay inside.
        fs::create_dir_all(parent).map_err(|error| io_problem(shown, &error))?;
    }
    write_atomically(&plan.real, plan.after.as_bytes()).map_err(|error| io_problem(shown, &error))?;
    let added = lines_word(count_lines(new_text));
    Ok(match plan.replaced {
        Some(removed) => format!("Replaced {} with {added} in `{shown}`.", lines_word(removed)),
        None if plan.existed => format!("Wrote {added} to the empty file `{shown}`."),
        None => format!("Created `{shown}` with {added}."),
    })
}

/// Counts every place the text starts, overlapping ones too: `aa` in `aaa` is ambiguous.
fn occurrences(haystack: &str, needle: &str) -> usize {
    let step = needle.chars().next().map_or(1, char::len_utf8);
    let mut count = 0;
    let mut from = 0;
    while let Some(at) = haystack[from..].find(needle) {
        count += 1;
        from += at + step;
    }
    count
}

fn count_lines(text: &str) -> usize {
    text.lines().count()
}

fn lines_word(count: usize) -> String {
    if count == 1 { "1 line".to_owned() } else { format!("{count} lines") }
}

/// Writes next to the target and renames over it, so a reader never sees half a file and a
/// failure leaves the old file whole. The old file's permissions are kept.
fn write_atomically(target: &Path, content: &[u8]) -> io::Result<()> {
    let (Some(folder), Some(name)) = (target.parent(), target.file_name()) else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "the path has no file name"));
    };
    let temporary = folder.join(format!(".{}.{}.tmp", name.to_string_lossy(), std::process::id()));
    let permissions = fs::metadata(target).ok().map(|meta| meta.permissions());
    let result = (|| {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(content)?;
        file.sync_all()?;
        if let Some(permissions) = permissions {
            fs::set_permissions(&temporary, permissions)?;
        }
        fs::rename(&temporary, target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
