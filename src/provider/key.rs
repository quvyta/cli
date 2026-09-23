//! A provider's key, read from the person's key file and never shown.
//!
//! The key is a value of its own rather than a `String` so that nothing can print it by
//! accident: it has no `Display`, its `Debug` writes only the marker and the last four
//! characters, and the one method that hands the secret out ([`expose`](Key::expose)) is visible
//! only inside this crate, where the single caller is the transport that puts it in a request
//! header. A key never travels in an address, so nothing that records or reports an address can
//! record it either.

use std::fmt;
use std::path::{Path, PathBuf};

/// How many characters of a key are ever shown.
const SHOWN: usize = 4;

/// The shortest key whose last four characters can be shown without showing most of it.
const SHOWABLE: usize = 12;

/// A provider's key.
#[derive(Clone, PartialEq, Eq)]
pub struct Key(String);

/// Why a key file gave no key. It names the file and never its contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    /// The file could not be read.
    Unreadable {
        /// The key file.
        path: PathBuf,
        /// What the system said.
        reason: String,
    },
    /// The file holds nothing that can be a key: it is empty, or more than one line.
    NotAKey {
        /// The key file.
        path: PathBuf,
    },
}

impl fmt::Display for KeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { path, reason } => write!(formatter, "{}: {reason}", path.display()),
            Self::NotAKey { path } => write!(formatter, "{}", path.display()),
        }
    }
}

impl Key {
    /// The key in `text` with the spaces around it dropped, or `None` when there is none. A key
    /// file often ends with a newline, and a key with a newline in it makes a header no server
    /// accepts.
    #[must_use]
    pub fn new(text: &str) -> Option<Self> {
        let trimmed = text.trim();
        // A header holds no control characters, and a file that is a whole paragraph is not a key.
        if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
            return None;
        }
        Some(Self(trimmed.to_owned()))
    }

    /// Reads the key from `path`.
    ///
    /// # Errors
    ///
    /// When the file cannot be read or holds no key. The error names the file only.
    pub fn from_file(path: &Path) -> Result<Self, KeyError> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| KeyError::Unreadable { path: path.to_owned(), reason: error.kind().to_string() })?;
        Self::new(&text).ok_or_else(|| KeyError::NotAKey { path: path.to_owned() })
    }

    /// The last four characters, or `None` for a key so short that four would give most of it
    /// away.
    #[must_use]
    pub fn last_four(&self) -> Option<String> {
        let characters: Vec<char> = self.0.chars().collect();
        (characters.len() >= SHOWABLE).then(|| characters[characters.len() - SHOWN..].iter().collect())
    }

    /// The secret itself, for the one place that has to send it.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Key {
    /// The marker and the last four characters, so a key inside a structure that is printed
    /// while something is being looked into does not end up in a log.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.last_four() {
            Some(tail) => write!(formatter, "Key(…{tail})"),
            None => formatter.write_str("Key(…)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Not a key of anyone's: the characters spell what it is.
    const MADE_UP: &str = "not-a-real-key-0000-wxyz";

    #[test]
    fn only_the_last_four_characters_are_ever_shown() {
        let key = Key::new(MADE_UP).expect("a key");
        assert_eq!(format!("{key:?}"), "Key(…wxyz)");
        assert!(!format!("{key:?}").contains("not-a-real"));
        let short = Key::new("abcdefgh").expect("a key");
        assert_eq!(format!("{short:?}"), "Key(…)", "four of eight characters is most of the key");
    }

    #[test]
    fn a_key_file_gives_its_one_line_and_its_errors_name_only_the_file() {
        let folder = std::env::temp_dir().join(format!("qcli-key-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("folder");
        let file = folder.join("key");
        std::fs::write(&file, format!("{MADE_UP}\n")).expect("write");
        assert_eq!(Key::from_file(&file).expect("a key").expose(), MADE_UP);

        std::fs::write(&file, format!("{MADE_UP}\nsecond line")).expect("write");
        let error = Key::from_file(&file).expect_err("two lines are not a key");
        assert_eq!(error, KeyError::NotAKey { path: file.clone() });
        assert!(!error.to_string().contains("not-a-real"), "the error does not quote the file");

        let missing = Key::from_file(&folder.join("missing")).expect_err("no file");
        assert!(matches!(missing, KeyError::Unreadable { .. }));
        std::fs::remove_dir_all(&folder).expect("clean up");
    }
}
