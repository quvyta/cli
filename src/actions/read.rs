//! `read_file`: a text file, cut when long.

use std::fs;

use super::workspace::{Workspace, io_problem};

/// At most this many lines are returned.
pub(super) const MAX_LINES: usize = 2000;
/// At most this many bytes are returned.
pub(super) const MAX_BYTES: usize = 100 * 1024;

pub(super) fn read(workspace: &Workspace, path: &str) -> Result<String, String> {
    let real = workspace.resolve(path)?;
    let shown = workspace.relative(&real);
    if real.is_dir() {
        return Err(format!("`{shown}` is a folder; use list_dir to see what is in it."));
    }
    let bytes = fs::read(&real).map_err(|error| io_problem(&shown, &error))?;
    let size = bytes.len();
    let text = match String::from_utf8(bytes) {
        Ok(text) if !text.contains('\0') => text,
        _ => {
            return Err(format!("`{shown}` is not a text file ({size} bytes); it cannot be shown."));
        }
    };
    if text.is_empty() {
        return Ok(format!("`{shown}` is empty."));
    }
    let mut end = 0;
    let mut lines = 0;
    for line in text.split_inclusive('\n') {
        if lines == MAX_LINES || end + line.len() > MAX_BYTES {
            break;
        }
        end += line.len();
        lines += 1;
    }
    if end == text.len() {
        return Ok(text);
    }
    let total = text.lines().count();
    let note = if lines == 0 {
        // The first line alone is over the byte cap: show its start rather than nothing.
        end = text.floor_char_boundary(MAX_BYTES);
        format!(
            "(Cut: the file has {total} lines and the first one alone is longer than {} KB; only its start is shown.)",
            MAX_BYTES / 1024
        )
    } else {
        format!("(Cut: the file has {total} lines; only the first {lines} are shown.)")
    };
    let mut out = text[..end].to_owned();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&note);
    Ok(out)
}
