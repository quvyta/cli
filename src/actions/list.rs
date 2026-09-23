//! `list_dir`: the entries of a folder, folders first.

use std::fs;

use super::workspace::{Workspace, io_problem};

/// At most this many entries are returned.
pub(super) const MAX_ENTRIES: usize = 500;

pub(super) fn list(workspace: &Workspace, path: &str) -> Result<String, String> {
    let real = workspace.resolve(path)?;
    let shown = workspace.relative(&real);
    if real.is_file() {
        return Err(format!("`{shown}` is a file; use read_file to read it."));
    }
    let entries = fs::read_dir(&real).map_err(|error| io_problem(&shown, &error))?;
    let mut items: Vec<(bool, String)> = entries
        .filter_map(Result::ok)
        .map(|entry| {
            // A link to a folder is shown as a folder; whether it may be entered is decided
            // when it is used.
            let is_dir = fs::metadata(entry.path()).is_ok_and(|meta| meta.is_dir());
            (is_dir, entry.file_name().to_string_lossy().into_owned())
        })
        .collect();
    if items.is_empty() {
        return Ok(format!("`{shown}` is empty."));
    }
    items.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let more = items.len().saturating_sub(MAX_ENTRIES);
    let mut out: Vec<String> = items
        .into_iter()
        .take(MAX_ENTRIES)
        .map(|(is_dir, name)| if is_dir { format!("{name}/") } else { name })
        .collect();
    if more > 0 {
        out.push(format!("(… and {more} more entries not shown.)"));
    }
    Ok(out.join("\n"))
}
