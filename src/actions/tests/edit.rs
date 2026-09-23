//! `edit_file` and its preview.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use super::super::{Change, DiffLine, Preview, diff};
use super::{Scratch, edit, perform};

fn lines(pairs: &[(Change, &str)]) -> Vec<DiffLine> {
    pairs.iter().map(|(change, text)| DiffLine { change: *change, text: (*text).to_owned() }).collect()
}

#[test]
fn an_exact_match_is_replaced_and_described() {
    let scratch = Scratch::new("edit");
    scratch.write("a.txt", "one\ntwo\nthree\n");
    let outcome = perform(&scratch.workspace(), &edit("a.txt", "two", "2a\n2b"));
    assert!(!outcome.is_error, "{}", outcome.text);
    assert_eq!(outcome.text, "Replaced 1 line with 2 lines in `a.txt`.");
    assert_eq!(scratch.read("a.txt"), "one\n2a\n2b\nthree\n");
}

#[test]
fn a_missing_or_repeated_text_is_refused_and_named() {
    let scratch = Scratch::new("edit-ambiguous");
    scratch.write("a.txt", "x = 1\nx = 1\naaa\n");
    let workspace = scratch.workspace();

    let twice = perform(&workspace, &edit("a.txt", "x = 1", "x = 2"));
    assert!(twice.is_error);
    assert!(twice.text.contains("occurs 2 times"), "{}", twice.text);
    let never = perform(&workspace, &edit("a.txt", "y = 1", "y = 2"));
    assert!(never.is_error);
    assert!(never.text.contains("does not occur"), "{}", never.text);
    // Overlapping places are ambiguous too.
    let overlapping = perform(&workspace, &edit("a.txt", "aa", "b"));
    assert!(overlapping.text.contains("occurs 2 times"), "{}", overlapping.text);
    assert_eq!(scratch.read("a.txt"), "x = 1\nx = 1\naaa\n");

    let same = perform(&workspace, &edit("a.txt", "aaa", "aaa"));
    assert!(same.is_error && same.text.contains("the same"), "{}", same.text);
}

#[test]
fn an_empty_old_text_creates_a_new_file_and_its_folders() {
    let scratch = Scratch::new("edit-create");
    let outcome = perform(&scratch.workspace(), &edit("new/deep/b.rs", "", "a\nb\n"));
    assert!(!outcome.is_error, "{}", outcome.text);
    assert_eq!(outcome.text, "Created `new/deep/b.rs` with 2 lines.");
    assert_eq!(scratch.read("new/deep/b.rs"), "a\nb\n");
}

#[test]
fn an_empty_old_text_fills_an_empty_file_but_not_a_full_one() {
    let scratch = Scratch::new("edit-existing");
    scratch.write("empty.txt", "");
    scratch.write("full.txt", "keep\n");
    let workspace = scratch.workspace();

    let filled = perform(&workspace, &edit("empty.txt", "", "hi\n"));
    assert_eq!(filled.text, "Wrote 1 line to the empty file `empty.txt`.");
    assert_eq!(scratch.read("empty.txt"), "hi\n");

    let refused = perform(&workspace, &edit("full.txt", "", "gone\n"));
    assert!(refused.is_error);
    assert!(refused.text.contains("give the text to replace"), "{}", refused.text);
    assert_eq!(scratch.read("full.txt"), "keep\n");

    let missing = perform(&workspace, &edit("nope.txt", "x", "y"));
    assert!(missing.is_error && missing.text.contains("leave `old_text` empty"));
    assert!(perform(&workspace, &edit("", "", "x")).is_error);
}

#[test]
fn the_write_keeps_permissions_and_leaves_no_temporary_file() {
    let scratch = Scratch::new("edit-mode");
    scratch.write("tool.sh", "echo old\n");
    let target = scratch.path().join("tool.sh");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o751)).unwrap();
    let outcome = perform(&scratch.workspace(), &edit("tool.sh", "old", "new"));
    assert!(!outcome.is_error, "{}", outcome.text);
    assert_eq!(fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o751);
    let names: Vec<_> = fs::read_dir(scratch.path()).unwrap().map(|entry| entry.unwrap().file_name()).collect();
    assert_eq!(names, ["tool.sh"]);
}

#[test]
fn the_preview_marks_the_changed_lines_and_writes_nothing() {
    let scratch = Scratch::new("edit-preview");
    let before: String = (1..=10).map(|n| format!("l{n}\n")).collect();
    scratch.write("a.txt", &before);
    let workspace = scratch.workspace();

    let preview = workspace.preview(&edit("a.txt", "l5\n", "five\nfive-b\n"));
    assert_eq!(
        preview,
        Ok(Preview::Diff {
            path: "a.txt".to_owned(),
            lines: lines(&[
                (Change::Same, "l2"),
                (Change::Same, "l3"),
                (Change::Same, "l4"),
                (Change::Removed, "l5"),
                (Change::Added, "five"),
                (Change::Added, "five-b"),
                (Change::Same, "l6"),
                (Change::Same, "l7"),
                (Change::Same, "l8"),
            ]),
        })
    );
    assert_eq!(scratch.read("a.txt"), before);

    let refused = workspace.preview(&edit("a.txt", "l", "x"));
    assert!(refused.unwrap_err().contains("occurs 10 times"));
}

#[test]
fn a_new_file_previews_as_all_added() {
    let scratch = Scratch::new("edit-preview-new");
    let preview = scratch.workspace().preview(&edit("n.txt", "", "a\nb"));
    assert_eq!(
        preview,
        Ok(Preview::Diff { path: "n.txt".to_owned(), lines: lines(&[(Change::Added, "a"), (Change::Added, "b")]) })
    );
    assert!(!scratch.path().join("n.txt").exists());
}

#[test]
fn the_diff_keeps_common_lines_inside_the_region() {
    let before = "1\n2\n3\n4\n";
    let after = "1\nX\n3\nY\n";
    assert_eq!(
        diff::diff(before, after, 3),
        lines(&[
            (Change::Same, "1"),
            (Change::Removed, "2"),
            (Change::Added, "X"),
            (Change::Same, "3"),
            (Change::Removed, "4"),
            (Change::Added, "Y"),
        ])
    );
    assert!(diff::diff("same\n", "same\n", 3).is_empty());
    assert_eq!(diff::diff("a\nb\nc\n", "a\nc\n", 0), lines(&[(Change::Removed, "b")]));
}
