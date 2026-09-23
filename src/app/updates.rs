//! The ecosystem's update notice: once qcli is open it asks crates.io, at most once a day, whether a
//! newer version of itself is out, and says so in the corner when one is.
//!
//! The question goes out on a thread of the framework's own, so the start never waits for it,
//! and no network is silence. The switch is the ecosystem's, `update-notice` in the shared
//! `quvyta.conf`, and it is read here before anything is asked: with it off nothing is asked.

use qframe::prelude::*;
use qframe::runtime::UpdateCheck;
use qframe::storage::Family;

use super::{Msg, QCli};
use crate::config::{APP, UpdateFolders};

impl QCli {
    /// The same chat, asking at start whether a newer version is out while the ecosystem's update
    /// notice in `folders` is on. `None` asks nothing, which is every test that has not said
    /// otherwise.
    #[must_use]
    pub fn update_notice(mut self, folders: Option<UpdateFolders>) -> Self {
        self.updates = folders;
        self
    }

    /// The question for a newer version of qcli, when the ecosystem's switch is on.
    pub(super) fn ask_for_update(&self) -> Command<Msg> {
        let Some(folders) = &self.updates else { return Command::none() };
        if !Family::QUVYTA.update_notice_in(&folders.config) {
            return Command::none();
        }
        let check =
            UpdateCheck::new(Family::QUVYTA, APP, env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"), Msg::NewVersion)
                .in_folders(folders.config.clone(), folders.state.clone());
        Command::check_for_update(check)
    }
}
