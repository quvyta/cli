//! `read_file`, `list_dir` and `search`.

use std::fmt::Write as _;

use super::super::{list as list_module, read as read_module, search as search_module};
use super::{Scratch, list, perform, read, search};

#[test]
fn read_returns_the_plain_text() {
    let scratch = Scratch::new("read");
    scratch.write("src/main.rs", "fn main() {}\n");
    let outcome = perform(&scratch.workspace(), &read("src/main.rs"));
    assert!(!outcome.is_error);
    assert_eq!(outcome.text, "fn main() {}\n");
}

#[test]
fn read_refuses_what_is_not_text() {
    let scratch = Scratch::new("binary");
    scratch.write("nul.bin", b"ab\0cd");
    scratch.write("latin.bin", [0xff_u8, 0xfe, 0x41]);
    let workspace = scratch.workspace();
    for name in ["nul.bin", "latin.bin"] {
        let outcome = perform(&workspace, &read(name));
        assert!(outcome.is_error);
        assert!(outcome.text.contains("is not a text file"), "{}", outcome.text);
    }
}

#[test]
fn read_names_folders_missing_files_and_empty_files() {
    let scratch = Scratch::new("read-kinds");
    scratch.write("sub/empty.txt", "");
    let workspace = scratch.workspace();

    let folder = perform(&workspace, &read("sub"));
    assert!(folder.is_error && folder.text.contains("use list_dir"), "{}", folder.text);
    let missing = perform(&workspace, &read("nope.txt"));
    assert!(missing.is_error);
    assert_eq!(missing.text, "`nope.txt` does not exist.");
    let empty = perform(&workspace, &read("sub/empty.txt"));
    assert!(!empty.is_error);
    assert_eq!(empty.text, "`sub/empty.txt` is empty.");
}

#[test]
fn a_long_file_is_cut_by_lines_and_says_so() {
    let scratch = Scratch::new("read-lines");
    let total = read_module::MAX_LINES + 500;
    let content: String = (1..=total).fold(String::new(), |mut text, number| {
        let _ = writeln!(text, "line {number}");
        text
    });
    scratch.write("long.txt", &content);
    let outcome = perform(&scratch.workspace(), &read("long.txt"));
    assert!(!outcome.is_error);
    let last_shown = format!("line {}\n", read_module::MAX_LINES);
    assert!(outcome.text.contains(&last_shown));
    assert!(!outcome.text.contains(&format!("line {}\n", read_module::MAX_LINES + 1)));
    assert!(outcome.text.ends_with(&format!(
        "(Cut: the file has {total} lines; only the first {} are shown.)",
        read_module::MAX_LINES
    )));
}

#[test]
fn a_wide_file_is_cut_by_bytes() {
    let scratch = Scratch::new("read-bytes");
    let line = format!("{}\n", "x".repeat(999));
    scratch.write("wide.txt", line.repeat(300));
    let outcome = perform(&scratch.workspace(), &read("wide.txt"));
    // 100 KB holds 102 lines of 1000 bytes.
    assert!(
        outcome.text.ends_with("(Cut: the file has 300 lines; only the first 102 are shown.)"),
        "{}",
        &outcome.text[outcome.text.len() - 80..]
    );
    assert!(outcome.text.len() <= read_module::MAX_BYTES + 100);

    scratch.write("one.txt", "é".repeat(read_module::MAX_BYTES));
    let outcome = perform(&scratch.workspace(), &read("one.txt"));
    assert!(outcome.text.contains("only its start is shown"), "cut note");
    assert!(outcome.text.len() <= read_module::MAX_BYTES + 200);
}

#[test]
fn list_puts_folders_first_with_a_slash_in_name_order() {
    let scratch = Scratch::new("list");
    scratch.write("b.txt", "");
    scratch.write("a.txt", "");
    scratch.write("zeta/x", "");
    scratch.write("alpha/x", "");
    let outcome = perform(&scratch.workspace(), &list(""));
    assert!(!outcome.is_error);
    assert_eq!(outcome.text, "alpha/\nzeta/\na.txt\nb.txt");
    assert_eq!(perform(&scratch.workspace(), &list("zeta")).text, "x");
}

#[test]
fn list_is_capped_and_says_how_many_more() {
    let scratch = Scratch::new("list-cap");
    for number in 0..list_module::MAX_ENTRIES + 7 {
        scratch.write(&format!("f{number:04}"), "");
    }
    let outcome = perform(&scratch.workspace(), &list("."));
    let lines: Vec<&str> = outcome.text.lines().collect();
    assert_eq!(lines.len(), list_module::MAX_ENTRIES + 1);
    assert_eq!(lines[0], "f0000");
    assert_eq!(lines[lines.len() - 1], "(… and 7 more entries not shown.)");
}

#[test]
fn list_names_files_and_empty_folders() {
    let scratch = Scratch::new("list-kinds");
    scratch.write("a.txt", "x");
    std::fs::create_dir(scratch.path().join("empty")).unwrap();
    let workspace = scratch.workspace();
    let file = perform(&workspace, &list("a.txt"));
    assert!(file.is_error && file.text.contains("use read_file"), "{}", file.text);
    assert_eq!(perform(&workspace, &list("empty")).text, "`empty` is empty.");
    assert!(perform(&workspace, &list("nope")).is_error);
}

#[test]
fn search_finds_lines_in_a_sorted_walk() {
    let scratch = Scratch::new("search");
    scratch.write("b.rs", "fn main() {}\nlet x = 1;\n");
    scratch.write("a/z.rs", "// no\nfn main_helper() {}\n");
    scratch.write("a/y.rs", "FN MAIN\n");
    let outcome = perform(&scratch.workspace(), &search("fn main", ""));
    assert!(!outcome.is_error);
    assert_eq!(outcome.text, "a/z.rs:2: fn main_helper() {}\nb.rs:1: fn main() {}");

    let narrowed = perform(&scratch.workspace(), &search("fn main", "a"));
    assert_eq!(narrowed.text, "a/z.rs:2: fn main_helper() {}");
    let one_file = perform(&scratch.workspace(), &search("let", "b.rs"));
    assert_eq!(one_file.text, "b.rs:2: let x = 1;");
}

#[test]
fn search_skips_history_build_output_and_binaries() {
    let scratch = Scratch::new("search-skip");
    scratch.write(".git/config", "needle\n");
    scratch.write("target/debug/out.txt", "needle\n");
    scratch.write("node_modules/pkg/index.js", "needle\n");
    scratch.write("data.bin", b"needle\n\0\x01");
    scratch.write("src/lib.rs", "needle\n");
    // A file merely named like a skipped folder is still searched.
    scratch.write("src/target", "needle\n");
    let outcome = perform(&scratch.workspace(), &search("needle", ""));
    assert_eq!(outcome.text, "src/lib.rs:1: needle\nsrc/target:1: needle");
}

#[test]
fn search_is_capped_and_says_so() {
    let scratch = Scratch::new("search-cap");
    scratch.write("many.txt", "hit\n".repeat(search_module::MAX_MATCHES + 50));
    let outcome = perform(&scratch.workspace(), &search("hit", ""));
    let lines: Vec<&str> = outcome.text.lines().collect();
    assert_eq!(lines.len(), search_module::MAX_MATCHES + 1);
    assert!(lines[lines.len() - 1].starts_with("(Stopped at 200 matches"));
}

#[test]
fn search_reports_nothing_found_and_empty_patterns() {
    let scratch = Scratch::new("search-none");
    scratch.write("a.txt", "hello\n");
    let workspace = scratch.workspace();
    let none = perform(&workspace, &search("absent", ""));
    assert!(!none.is_error);
    assert_eq!(none.text, "No matches for `absent` in `.`.");
    assert!(perform(&workspace, &search("", "")).is_error);
    assert!(perform(&workspace, &search("x", "nope")).is_error);
}

#[test]
fn search_shortens_very_long_lines() {
    let scratch = Scratch::new("search-long");
    scratch.write("a.txt", format!("needle{}\n", "y".repeat(1000)));
    let outcome = perform(&scratch.workspace(), &search("needle", ""));
    assert!(outcome.text.ends_with('…'));
    assert!(outcome.text.chars().count() < 330);
}
