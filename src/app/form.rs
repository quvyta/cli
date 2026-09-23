//! The provider form: the four settings a provider needs, checked before they are written.

use qframe::prelude::*;
use qframe::widgets::{Field as FormFieldWidget, Form, Modal, TextInput};

use super::Msg;
use crate::config::Provider;
use crate::provider::{Key, KeyError};

/// The id of the form's first field, which has focus when the form opens.
pub const FIRST: &str = "provider-address";

/// The fields of the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// The address.
    Address,
    /// The model.
    Model,
    /// The key file's path.
    KeyFile,
    /// The header the key goes in.
    KeyHeader,
}

impl Field {
    const ALL: [Self; 4] = [Self::Address, Self::Model, Self::KeyFile, Self::KeyHeader];

    fn id(self) -> &'static str {
        match self {
            Self::Address => FIRST,
            Self::Model => "provider-model",
            Self::KeyFile => "provider-key-file",
            Self::KeyHeader => "provider-key-header",
        }
    }

    fn label(self) -> String {
        match self {
            Self::Address => t!("provider.address"),
            Self::Model => t!("provider.model"),
            Self::KeyFile => t!("provider.key-file"),
            Self::KeyHeader => t!("provider.key-header"),
        }
    }

    fn hint(self) -> String {
        match self {
            Self::Address => t!("provider.address-hint"),
            Self::Model => t!("provider.model-hint"),
            Self::KeyFile => t!("provider.key-file-hint"),
            Self::KeyHeader => t!("provider.key-header-hint"),
        }
    }
}

/// What is wrong with a field after Save was pressed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Problem {
    /// It is empty and must not be.
    Empty,
    /// The key file gave no key.
    Key(KeyError),
    /// The settings could not be written.
    Unsaved(String),
}

/// The form's values while it is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderForm {
    provider: Provider,
    problems: Vec<(Field, Problem)>,
}

impl ProviderForm {
    /// The form filled with `provider`.
    #[must_use]
    pub fn from_provider(provider: &Provider) -> Self {
        Self { provider: provider.clone(), problems: Vec::new() }
    }

    /// Changes one field.
    pub fn set(&mut self, field: Field, value: String) {
        match field {
            Field::Address => self.provider.address = value,
            Field::Model => self.provider.model = value,
            Field::KeyFile => self.provider.key_file = value,
            Field::KeyHeader => self.provider.key_header = value,
        }
    }

    fn value(&self, field: Field) -> &str {
        match field {
            Field::Address => &self.provider.address,
            Field::Model => &self.provider.model,
            Field::KeyFile => &self.provider.key_file,
            Field::KeyHeader => &self.provider.key_header,
        }
    }

    /// The provider when every field holds what it must, the key file included; otherwise the
    /// problems are kept for the form to show and there is nothing to save.
    pub fn check(&mut self) -> Option<Provider> {
        let provider = Provider {
            address: self.provider.address.trim().to_owned(),
            model: self.provider.model.trim().to_owned(),
            key_file: self.provider.key_file.trim().to_owned(),
            key_header: self.provider.key_header.trim().to_owned(),
        };
        self.problems = [Field::Address, Field::Model, Field::KeyFile]
            .into_iter()
            .filter(|field| {
                let value = match field {
                    Field::Address => &provider.address,
                    Field::Model => &provider.model,
                    _ => &provider.key_file,
                };
                value.is_empty()
            })
            .map(|field| (field, Problem::Empty))
            .collect();
        if !provider.key_file.is_empty()
            && let Err(error) = Key::from_file(&provider.key_path())
        {
            self.problems.push((Field::KeyFile, Problem::Key(error)));
        }
        self.problems.is_empty().then_some(provider)
    }

    /// The id of the first field with a problem, for focus to go to after a failed save.
    #[must_use]
    pub fn first_problem(&self) -> Option<&'static str> {
        Field::ALL.into_iter().find(|field| self.problems.iter().any(|(with, _)| with == field)).map(Field::id)
    }

    /// Says that the settings could not be written.
    pub fn failed(&mut self, error: &std::io::Error) {
        self.problems = vec![(Field::Address, Problem::Unsaved(error.to_string()))];
    }

    fn problem(&self, field: Field) -> Option<String> {
        self.problems.iter().find(|(with, _)| *with == field).map(|(_, problem)| match problem {
            Problem::Empty => t!("provider.empty"),
            Problem::Key(KeyError::Unreadable { path, reason }) => {
                t!("provider.key-unreadable", path = path.display().to_string(), reason = reason.clone())
            }
            Problem::Key(KeyError::NotAKey { path }) => t!("provider.not-a-key", path = path.display().to_string()),
            Problem::Unsaved(reason) => t!("provider.unsaved", reason = reason.clone()),
        })
    }

    /// Draws the form as a dialog over the chat.
    pub fn view(&self, ui: &mut View<'_, Msg>) {
        let dialog = Modal::new()
            .title(t!("provider.title"))
            .width(72)
            .on_close(Msg::CloseProvider)
            .action(Button::new(t!("provider.cancel")).on_press(Msg::CloseProvider))
            .action(Button::new(t!("provider.save")).variant("primary").on_press(Msg::SaveProvider));
        ui.add_with(dialog, |ui| {
            ui.add(Text::new(t!("provider.intro")).role("secondary"));
            Form::new().show(ui, |fields| {
                for field in Field::ALL {
                    let widget = FormFieldWidget::new(field.label())
                        .required(field != Field::KeyHeader)
                        .hint(field.hint())
                        .error(self.problem(field));
                    fields.field(widget, |ui| {
                        ui.add(
                            TextInput::new(self.value(field))
                                .invalid(self.problem(field).is_some())
                                .on_change(move |value| Msg::Form(field, value))
                                .on_submit(|_| Msg::SaveProvider),
                        )
                        .id(field.id())
                        .fill_width();
                    });
                }
            });
        });
    }
}
