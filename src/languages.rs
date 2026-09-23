//! Every language file carries all of the English text, in the same shape, and in its own words.
//!
//! A key missing from a translation would show English in the middle of another language, and a
//! placeholder dropped or misspelt would print `{name}` literally. Having every key is not being
//! translated, though: a file whose values are all English passes that check, so a second test
//! asks that most values differ from English.

use std::collections::{BTreeMap, BTreeSet};

use toml::de::{DeTable, DeValue};

use crate::locales;

/// The messages of a language file by dotted key, `[meta]` left out; plural tables become one
/// entry per form.
fn messages(file: &str, text: &str) -> BTreeMap<String, String> {
    let root = DeTable::parse(text).unwrap_or_else(|error| panic!("{file} is not TOML: {error}"));
    let mut out = BTreeMap::new();
    for (section, value) in root.get_ref() {
        if section.get_ref() != "meta" {
            flatten(file, section.get_ref(), value.get_ref(), &mut out);
        }
    }
    out
}

fn flatten(file: &str, prefix: &str, value: &DeValue<'_>, out: &mut BTreeMap<String, String>) {
    match value {
        DeValue::String(text) => {
            out.insert(prefix.to_owned(), text.to_string());
        }
        DeValue::Table(table) => {
            for (key, value) in table {
                flatten(file, &format!("{prefix}.{}", key.get_ref()), value.get_ref(), out);
            }
        }
        _ => panic!("{file}: `{prefix}` is not text"),
    }
}

fn placeholders(text: &str) -> BTreeSet<&str> {
    text.split('{').skip(1).filter_map(|rest| rest.split_once('}').map(|(name, _)| name)).collect()
}

fn english() -> BTreeMap<String, String> {
    let (file, text) = locales()[0];
    assert_eq!(file, "en.toml");
    messages(file, text)
}

/// The message a key belongs to: a plural form's key is its table's, since languages have
/// different forms (Japanese only `other`, Russian `one`, `few`, `many` and `other`).
fn message_of(key: &str) -> &str {
    match key.rsplit_once('.') {
        Some((base, form)) if ["zero", "one", "two", "few", "many", "other"].contains(&form) => base,
        _ => key,
    }
}

/// Every message's placeholders, whichever of its forms they are in.
fn by_message(messages: &BTreeMap<String, String>) -> BTreeMap<&str, BTreeSet<&str>> {
    let mut out: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (key, value) in messages {
        out.entry(message_of(key)).or_default().extend(placeholders(value));
    }
    out
}

#[test]
fn every_language_has_every_key_with_its_placeholders() {
    let english = english();
    let english = by_message(&english);
    assert_eq!(locales().len(), 9, "the ecosystem's nine languages");
    for &(file, text) in &locales()[1..] {
        let other = messages(file, text);
        let other = by_message(&other);
        for (key, wanted) in &english {
            let found = other.get(key).unwrap_or_else(|| panic!("{file} has no `{key}`"));
            assert_eq!(found, wanted, "{file} `{key}` changes the placeholders");
        }
        for key in other.keys() {
            assert!(english.contains_key(key), "{file} has `{key}`, which English does not");
        }
    }
}

#[test]
fn every_language_says_things_in_its_own_words() {
    let english = english();
    for &(file, text) in &locales()[1..] {
        let other = messages(file, text);
        let same: Vec<&String> = english.keys().filter(|key| other.get(*key) == english.get(*key)).collect();
        // Names are the same in every language: the program's, the header's and the model's.
        assert!(
            same.len() * 10 <= english.len(),
            "{file} leaves {} of {} values in English: {same:?}",
            same.len(),
            english.len()
        );
    }
}
