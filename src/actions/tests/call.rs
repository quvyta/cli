use serde_json::json;

use super::super::{Action, ActionKind};
use super::{edit, list, read, run, search};

#[test]
fn every_tool_is_read_from_its_call() {
    assert_eq!(Action::from_call("read_file", &json!({ "path": "src/main.rs" })), Ok(read("src/main.rs")));
    assert_eq!(Action::from_call("list_dir", &json!({ "path": "src" })), Ok(list("src")));
    assert_eq!(
        Action::from_call("search", &json!({ "pattern": "fn main", "path": "src" })),
        Ok(search("fn main", "src"))
    );
    assert_eq!(
        Action::from_call("edit_file", &json!({ "path": "a.txt", "old_text": "one", "new_text": "two" })),
        Ok(edit("a.txt", "one", "two"))
    );
    assert_eq!(Action::from_call("run_command", &json!({ "command": "cargo test" })), Ok(run("cargo test")));
}

#[test]
fn optional_paths_default_to_the_root() {
    assert_eq!(Action::from_call("search", &json!({ "pattern": "x" })), Ok(search("x", "")));
    assert_eq!(Action::from_call("list_dir", &json!({})), Ok(list("")));
}

#[test]
fn bad_calls_give_a_sentence_for_the_model() {
    let unknown = Action::from_call("delete_file", &json!({ "path": "a" })).unwrap_err();
    assert!(unknown.contains("no tool named `delete_file`"), "{unknown}");

    let missing = Action::from_call("read_file", &json!({})).unwrap_err();
    assert!(missing.contains("needs the field `path`"), "{missing}");

    let wrong = Action::from_call("run_command", &json!({ "command": 3 })).unwrap_err();
    assert!(wrong.contains("must be a string"), "{wrong}");

    let not_object = Action::from_call("read_file", &json!("a.txt")).unwrap_err();
    assert!(not_object.contains("JSON object"), "{not_object}");

    let edit_missing = Action::from_call("edit_file", &json!({ "path": "a", "old_text": "" })).unwrap_err();
    assert!(edit_missing.contains("`new_text`"), "{edit_missing}");
}

#[test]
fn definitions_name_the_five_tools_the_parser_accepts() {
    let definitions = Action::definitions();
    let tools = definitions.as_array().expect("an array");
    let names: Vec<&str> = tools.iter().map(|tool| tool["name"].as_str().expect("a name")).collect();
    assert_eq!(
        names,
        [
            ActionKind::Read.tool_name(),
            ActionKind::List.tool_name(),
            ActionKind::Search.tool_name(),
            ActionKind::Edit.tool_name(),
            ActionKind::Run.tool_name(),
        ]
    );
    for tool in tools {
        let name = tool["name"].as_str().expect("a name");
        assert!(!tool["description"].as_str().expect("a description").is_empty());
        let schema = &tool["input_schema"];
        assert_eq!(schema["type"], "object", "{name}");
        // A call carrying exactly the required fields as strings must parse.
        let mut input = serde_json::Map::new();
        for field in schema["required"].as_array().expect("required fields") {
            let field = field.as_str().expect("a field name");
            assert!(schema["properties"][field].is_object(), "{name}.{field}");
            input.insert(field.to_owned(), json!("x"));
        }
        let parsed = Action::from_call(name, &input.into()).expect("parses");
        assert_eq!(parsed.kind().tool_name(), name);
    }
}

#[test]
fn only_edit_and_run_ask_first() {
    assert!(!read("a").needs_approval());
    assert!(!list("").needs_approval());
    assert!(!search("x", "").needs_approval());
    assert!(edit("a", "b", "c").needs_approval());
    assert!(run("printf hi").needs_approval());
}

#[test]
fn summary_kind_and_subject() {
    assert_eq!(read("src/main.rs").summary(), "read src/main.rs");
    assert_eq!(list("").summary(), "list .");
    assert_eq!(search("fn main", "").summary(), "search fn main");
    assert_eq!(search("fn main", "src").summary(), "search fn main in src");
    assert_eq!(edit("a.rs", "x", "y").summary(), "edit a.rs");
    assert_eq!(run("cargo test").summary(), "run cargo test");

    assert_eq!(search("fn main", "src").kind(), ActionKind::Search);
    assert_eq!(search("fn main", "src").subject(), "fn main");
    assert_eq!(run("cargo test").subject(), "cargo test");
    assert_eq!(edit("a.rs", "x", "y").subject(), "a.rs");
}
