//! Where qcli keeps its settings: `cli.conf` in the Quvyta family's folder, beside the other
//! applications of the family.
//!
//! ```text
//! ~/.config/quvyta/
//!     quvyta.conf     the language, theme and icons the family shares
//!     cli.conf        qcli's settings: the provider it talks to
//! ```
//!
//! The provider is four keys: the address, the model, the file the key is in and the header the
//! key goes in. The key itself is never in this file, which people copy between machines and
//! paste into reports; only the path of the file that holds it.

use std::path::{Path, PathBuf};

use qframe::i18n::I18n;
use qframe::storage::{Family, Preferences, Schema, Settings};

use crate::provider::{Endpoint, Key, KeyError};

/// The application's id in the family: its settings file is `cli.conf`.
pub const APP: &str = "cli";

/// The key of the address the Messages API lives under.
pub const ADDRESS: &str = "address";
/// The key of the model asked for.
pub const MODEL: &str = "model";
/// The key of the path of the file the key is read from.
pub const KEY_FILE: &str = "key-file";
/// The key of the header the key goes in.
pub const KEY_HEADER: &str = "key-header";

/// The header most servers of the Messages shape read the key from.
pub const DEFAULT_HEADER: &str = "x-api-key";

/// What qcli's own keys may hold. Every value is text; an empty one is the same as none.
#[must_use]
pub fn schema() -> Schema {
    Schema::builtin().text(ADDRESS, "").text(MODEL, "").text(KEY_FILE, "").text(KEY_HEADER, DEFAULT_HEADER)
}

/// The settings from the family's folder, checked and healed.
#[must_use]
pub fn load() -> Settings {
    checked(Settings::load_member(&Family::QUVYTA, APP))
}

/// [`load`] from `folder` as the family's folder, so a test never touches the person's own
/// settings.
#[must_use]
pub fn load_in(folder: &Path) -> Settings {
    checked(Settings::open(folder.join(format!("{APP}.conf"))))
}

fn checked(settings: Settings) -> Settings {
    settings.member_of(&Family::QUVYTA).schema(schema()).self_heal(true)
}

/// The language, theme and icons as qcli sees them: its own when its file names one, else the
/// family's, else what this machine asks for.
#[must_use]
pub fn preferences() -> Preferences {
    Family::QUVYTA.preferences(APP, &spoken())
}

/// The languages qcli speaks, for choosing the machine's one before the runtime is built.
#[must_use]
pub fn spoken() -> I18n {
    let mut i18n = I18n::builtin();
    for &(file, text) in crate::locales() {
        i18n.add_source(file, text);
    }
    i18n
}

/// The provider as the settings describe it. Every field is what the person wrote, trimmed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Provider {
    /// The address the Messages API lives under.
    pub address: String,
    /// The model asked for.
    pub model: String,
    /// The path of the key file, as written: `~/` stands for the home folder.
    pub key_file: String,
    /// The header the key goes in.
    pub key_header: String,
}

/// Why the provider cannot be asked yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    /// One of the fields is empty; the key of the first one.
    Field(&'static str),
    /// The key file gave no key.
    Key(KeyError),
}

impl Provider {
    /// The provider in `settings`.
    #[must_use]
    pub fn from_settings(settings: &Settings) -> Self {
        let text = |key: &str| settings.get::<String>(key).unwrap_or_default().trim().to_owned();
        let header = text(KEY_HEADER);
        Self {
            address: text(ADDRESS),
            model: text(MODEL),
            key_file: text(KEY_FILE),
            key_header: if header.is_empty() { DEFAULT_HEADER.to_owned() } else { header },
        }
    }

    /// Writes the provider into `settings`. A header that is the default is not written, the
    /// way the family writes no default.
    pub fn write(&self, settings: &mut Settings) {
        settings.set(ADDRESS, self.address.trim().to_owned());
        settings.set(MODEL, self.model.trim().to_owned());
        settings.set(KEY_FILE, self.key_file.trim().to_owned());
        let header = self.key_header.trim();
        if header.is_empty() || header == DEFAULT_HEADER {
            settings.remove(KEY_HEADER);
        } else {
            settings.set(KEY_HEADER, header.to_owned());
        }
    }

    /// The first empty field, if any: the provider cannot be asked without all of them.
    #[must_use]
    pub fn missing_field(&self) -> Option<&'static str> {
        [(ADDRESS, &self.address), (MODEL, &self.model), (KEY_FILE, &self.key_file)]
            .into_iter()
            .find_map(|(key, value)| value.is_empty().then_some(key))
    }

    /// The key file's path, with `~/` read as the home folder.
    #[must_use]
    pub fn key_path(&self) -> PathBuf {
        expand_home(&self.key_file)
    }

    /// The endpoint to ask, with the key read from its file now.
    ///
    /// # Errors
    ///
    /// When a field is empty or the key file gives no key.
    pub fn endpoint(&self) -> Result<Endpoint, Missing> {
        if let Some(field) = self.missing_field() {
            return Err(Missing::Field(field));
        }
        let key = Key::from_file(&self.key_path()).map_err(Missing::Key)?;
        Ok(Endpoint {
            address: self.address.clone(),
            model: self.model.clone(),
            key_header: self.key_header.clone(),
            key,
        })
    }
}

/// `path` with a leading `~/` replaced by the home folder.
fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("qcli-config-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("folder");
        folder
    }

    #[test]
    fn the_provider_is_written_and_read_back_and_the_default_header_is_not_written() {
        let folder = folder("round");
        let mut settings = load_in(&folder);
        let provider = Provider {
            address: " http://127.0.0.1:9/anthropic ".into(),
            model: "some-model".into(),
            key_file: "~/keys/made-up".into(),
            key_header: DEFAULT_HEADER.into(),
        };
        provider.write(&mut settings);
        settings.save().expect("saved");
        let text = std::fs::read_to_string(folder.join("cli.conf")).expect("the file");
        assert!(text.contains("address = \"http://127.0.0.1:9/anthropic\""), "{text}");
        assert!(!text.contains(KEY_HEADER), "the default header is not written: {text}");

        let read = Provider::from_settings(&load_in(&folder));
        assert_eq!(read.address, "http://127.0.0.1:9/anthropic");
        assert_eq!(read.key_header, DEFAULT_HEADER);

        let mut settings = load_in(&folder);
        Provider { key_header: "api-key".into(), ..read }.write(&mut settings);
        settings.save().expect("saved");
        assert_eq!(Provider::from_settings(&load_in(&folder)).key_header, "api-key");
        std::fs::remove_dir_all(&folder).expect("clean up");
    }

    #[test]
    fn an_endpoint_needs_every_field_and_a_key_in_the_file() {
        let folder = folder("endpoint");
        let key_file = folder.join("key");
        let mut provider = Provider {
            address: "http://127.0.0.1:9".into(),
            model: String::new(),
            key_file: key_file.display().to_string(),
            key_header: "api-key".into(),
        };
        assert_eq!(provider.endpoint().expect_err("no model"), Missing::Field(MODEL));
        provider.model = "m".into();
        assert!(matches!(provider.endpoint(), Err(Missing::Key(KeyError::Unreadable { .. }))));
        std::fs::write(&key_file, "not-a-real-key-0000-wxyz\n").expect("key");
        let endpoint = provider.endpoint().expect("an endpoint");
        assert_eq!((endpoint.model.as_str(), endpoint.key_header.as_str()), ("m", "api-key"));
        std::fs::remove_dir_all(&folder).expect("clean up");
    }

    #[test]
    fn a_home_path_is_read_from_the_home_folder() {
        let home = std::env::var_os("HOME").map(PathBuf::from).expect("a home in tests");
        assert_eq!(expand_home("~/a/b"), home.join("a/b"));
        assert_eq!(expand_home("/a/b"), PathBuf::from("/a/b"));
    }
}
