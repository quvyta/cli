use std::os::unix::fs::symlink;

use super::super::Preview;
use super::{Scratch, edit, list, perform, read, search};

fn assert_refused(outcome: &super::Outcome) {
    assert!(outcome.is_error, "{}", outcome.text);
    assert!(
        outcome.text.contains("outside the working folder")
            || outcome.text.contains("cannot be resolved")
            || outcome.text.contains("leads nowhere"),
        "{}",
        outcome.text
    );
}

#[test]
fn parent_folder_tricks_are_refused() {
    let outside = Scratch::new("outside-dots");
    let scratch = Scratch::new("dots");
    scratch.write("sub/a.txt", "a");
    let workspace = scratch.workspace();
    let escape = format!("../{}/secret", outside.path().file_name().unwrap().to_string_lossy());
    outside.write("secret", "no");

    let outcome = perform(&workspace, &read(&escape));
    assert_refused(&outcome);
    assert!(outcome.text.contains("outside the working folder"));
    assert_refused(&perform(&workspace, &read("sub/../../x")));
    assert_refused(&perform(&workspace, &list("..")));
    assert_refused(&perform(&workspace, &search("no", "..")));
    // A new file cannot be created next to the workspace either.
    assert_refused(&perform(&workspace, &edit("../made.txt", "", "x")));
    assert!(!scratch.path().parent().unwrap().join("made.txt").exists());
    // A missing folder followed by `..` is not guessed at.
    assert_refused(&perform(&workspace, &edit("missing/../../x", "", "x")));
    assert!(workspace.preview(&read("../x")).is_err());
}

#[test]
fn absolute_paths_count_only_inside() {
    let scratch = Scratch::new("absolute");
    scratch.write("a.txt", "inside\n");
    let workspace = scratch.workspace();

    assert_refused(&perform(&workspace, &read("/etc/hostname")));
    let inside = workspace.root().join("a.txt");
    let outcome = perform(&workspace, &read(&inside.to_string_lossy()));
    assert!(!outcome.is_error, "{}", outcome.text);
    assert_eq!(outcome.text, "inside\n");
}

#[test]
fn links_leading_outside_are_refused() {
    let outside = Scratch::new("outside-links");
    outside.write("secret.txt", "secret\n");
    let scratch = Scratch::new("links");
    symlink(outside.path(), scratch.path().join("away")).unwrap();
    symlink(outside.path().join("secret.txt"), scratch.path().join("secret-link")).unwrap();
    symlink(outside.path().join("not-there.txt"), scratch.path().join("dangling")).unwrap();
    let workspace = scratch.workspace();

    assert_refused(&perform(&workspace, &read("secret-link")));
    assert_refused(&perform(&workspace, &read("away/secret.txt")));
    assert_refused(&perform(&workspace, &list("away")));
    assert_refused(&perform(&workspace, &search("secret", "away")));
    assert_refused(&perform(&workspace, &edit("away/new.txt", "", "x")));
    assert_refused(&perform(&workspace, &edit("secret-link", "secret", "gone")));
    assert_refused(&perform(&workspace, &edit("dangling", "", "x")));
    assert!(!outside.path().join("new.txt").exists());
    assert!(!outside.path().join("not-there.txt").exists());
    assert_eq!(outside.read("secret.txt"), "secret\n");

    // A search from the root does not follow the link out either.
    let outcome = perform(&workspace, &search("secret", ""));
    assert_eq!(outcome.text, "No matches for `secret` in `.`.");
}

#[test]
fn links_inside_are_followed() {
    let scratch = Scratch::new("inner-links");
    scratch.write("real/a.txt", "hello\n");
    symlink(scratch.path().join("real"), scratch.path().join("alias")).unwrap();
    let workspace = scratch.workspace();

    let outcome = perform(&workspace, &read("alias/a.txt"));
    assert_eq!(outcome.text, "hello\n");
    assert_eq!(workspace.preview(&read("alias/a.txt")), Ok(Preview::Plain));
}

#[test]
fn the_root_is_named_by_an_empty_path_or_a_dot() {
    let scratch = Scratch::new("root");
    scratch.write("a.txt", "");
    let workspace = scratch.workspace();
    assert_eq!(perform(&workspace, &list("")).text, "a.txt");
    assert_eq!(perform(&workspace, &list(".")).text, "a.txt");
}
