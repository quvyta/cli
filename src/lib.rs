//! A small coding agent in the terminal: a chat with a model that can read, list, search, edit
//! and run inside one folder, and asks before it changes anything.

pub mod actions;
pub mod app;
pub mod config;
pub mod conversation;
pub mod instructions;
pub mod provider;
pub mod store;

pub use app::run;

/// The application's own locale files, compiled in so an installed program carries its text
/// with it. Each is `(file name, contents)`. English comes first: it is the language every other
/// file is checked against.
#[must_use]
pub fn locales() -> &'static [(&'static str, &'static str)] {
    &[
        ("en.toml", include_str!("../locales/en.toml")),
        ("de.toml", include_str!("../locales/de.toml")),
        ("es.toml", include_str!("../locales/es.toml")),
        ("fr.toml", include_str!("../locales/fr.toml")),
        ("ja.toml", include_str!("../locales/ja.toml")),
        ("pt-BR.toml", include_str!("../locales/pt-BR.toml")),
        ("ru.toml", include_str!("../locales/ru.toml")),
        ("tr.toml", include_str!("../locales/tr.toml")),
        ("zh-Hans.toml", include_str!("../locales/zh-Hans.toml")),
    ]
}

#[cfg(test)]
mod languages;
