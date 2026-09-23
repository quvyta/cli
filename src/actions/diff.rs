//! A small line diff for the edit preview.
//!
//! An edit changes one region, so the common start and end are taken off first and only the
//! middle is compared with a longest-common-subsequence table.

use super::{Change, DiffLine};

/// Past this many table cells the middle is shown as all removed, then all added: correct,
/// only less precise, and the preview stays quick on a huge file.
const MAX_CELLS: usize = 4_000_000;

/// The changed region of `before` → `after` with up to `context` unchanged lines on each side.
/// Empty when nothing changed.
pub(super) fn diff(before: &str, after: &str, context: usize) -> Vec<DiffLine> {
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();
    if old == new {
        return Vec::new();
    }
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let room = old.len().min(new.len()) - prefix;
    let suffix = old.iter().rev().zip(new.iter().rev()).take(room).take_while(|(a, b)| a == b).count();
    let mut out = Vec::new();
    push(&mut out, Change::Same, &old[prefix.saturating_sub(context)..prefix]);
    middle(&old[prefix..old.len() - suffix], &new[prefix..new.len() - suffix], &mut out);
    let tail = old.len() - suffix;
    push(&mut out, Change::Same, &old[tail..tail + suffix.min(context)]);
    out
}

fn push(out: &mut Vec<DiffLine>, change: Change, lines: &[&str]) {
    out.extend(lines.iter().map(|text| DiffLine { change, text: (*text).to_owned() }));
}

fn middle(old: &[&str], new: &[&str], out: &mut Vec<DiffLine>) {
    if old.len().saturating_mul(new.len()) > MAX_CELLS {
        push(out, Change::Removed, old);
        push(out, Change::Added, new);
        return;
    }
    // common[i * width + j] is the length of the longest common run of old[i..] and new[j..].
    let width = new.len() + 1;
    let mut common = vec![0_u32; (old.len() + 1) * width];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            common[i * width + j] = if old[i] == new[j] {
                common[(i + 1) * width + j + 1] + 1
            } else {
                common[(i + 1) * width + j].max(common[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < old.len() && j < new.len() {
        if old[i] == new[j] {
            push(out, Change::Same, &old[i..=i]);
            i += 1;
            j += 1;
        } else if common[(i + 1) * width + j] >= common[i * width + j + 1] {
            push(out, Change::Removed, &old[i..=i]);
            i += 1;
        } else {
            push(out, Change::Added, &new[j..=j]);
            j += 1;
        }
    }
    push(out, Change::Removed, &old[i..]);
    push(out, Change::Added, &new[j..]);
}
