//! `search`: an exact piece of text in the text files under a folder.

use std::fs;
use std::path::Path;

use super::workspace::Workspace;

/// At most this many matches are returned.
pub(super) const MAX_MATCHES: usize = 200;
/// Folders that hold history, build output or downloaded code, not the project's own text.
const SKIPPED: [&str; 3] = [".git", "target", "node_modules"];
/// A NUL byte in this many first bytes marks a file as binary.
const BINARY_PROBE: usize = 8192;
/// Files larger than this are not searched; they are data, not source.
const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
/// A matching line longer than this is shortened in the result.
const MAX_LINE_CHARS: usize = 300;

struct Found {
    lines: Vec<String>,
    cut: bool,
}

pub(super) fn search(workspace: &Workspace, pattern: &str, path: &str) -> Result<String, String> {
    if pattern.is_empty() {
        return Err("`search` needs a non-empty `pattern`.".to_owned());
    }
    let start = workspace.resolve(path)?;
    let shown = workspace.relative(&start);
    if !start.exists() {
        return Err(format!("`{shown}` does not exist."));
    }
    let mut found = Found { lines: Vec::new(), cut: false };
    visit(workspace, &start, pattern, &mut found);
    if found.lines.is_empty() {
        return Ok(format!("No matches for `{pattern}` in `{shown}`."));
    }
    let mut out = found.lines.join("\n");
    if found.cut {
        out.push_str(&format!(
            "\n(Stopped at {MAX_MATCHES} matches; narrow the search with `path` or a longer pattern.)"
        ));
    }
    Ok(out)
}

/// Walks in name order so the same tree always gives the same result. Links are not followed:
/// they could lead outside the workspace or around in a circle.
fn visit(workspace: &Workspace, path: &Path, pattern: &str, found: &mut Found) {
    if found.cut {
        return;
    }
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    if meta.is_file() {
        if meta.len() <= MAX_FILE_BYTES {
            search_file(workspace, path, pattern, found);
        }
        return;
    }
    if !meta.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    let mut children: Vec<_> = entries.filter_map(Result::ok).collect();
    children.sort_by_key(std::fs::DirEntry::file_name);
    for child in children {
        let is_dir = child.file_type().is_ok_and(|kind| kind.is_dir());
        if is_dir && SKIPPED.iter().any(|name| child.file_name() == *name) {
            continue;
        }
        visit(workspace, &child.path(), pattern, found);
        if found.cut {
            return;
        }
    }
}

fn search_file(workspace: &Workspace, path: &Path, pattern: &str, found: &mut Found) {
    let Ok(bytes) = fs::read(path) else {
        return;
    };
    if bytes[..bytes.len().min(BINARY_PROBE)].contains(&0) {
        return;
    }
    let text = String::from_utf8_lossy(&bytes);
    let shown = workspace.relative(path);
    for (index, line) in text.lines().enumerate() {
        if !line.contains(pattern) {
            continue;
        }
        if found.lines.len() == MAX_MATCHES {
            found.cut = true;
            return;
        }
        let number = index + 1;
        let line = shorten(line);
        found.lines.push(format!("{shown}:{number}: {line}"));
    }
}

fn shorten(line: &str) -> String {
    match line.char_indices().nth(MAX_LINE_CHARS) {
        Some((end, _)) => format!("{}…", &line[..end]),
        None => line.to_owned(),
    }
}
