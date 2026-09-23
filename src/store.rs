//! Where conversations are kept: one file for each working folder, so opening qcli in a folder
//! goes on where the last time in that folder stopped.
//!
//! ```text
//! ~/.local/state/quvyta/cli/conversations/
//!     myproject-3f2a9c1d5e7b8a60.json                 the folder's conversation
//!     myproject-3f2a9c1d5e7b8a60-1790000000.json      one put aside by a new conversation
//! ```
//!
//! The name carries the folder's name, for a person looking through the files, and a hash of
//! its whole path, since two folders can share a name.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use qframe::storage::atomic_write;

use crate::conversation::Conversation;

/// The conversations of one working folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    file: PathBuf,
}

impl Store {
    /// The store for `folder` under `root`, the folder conversations are kept in.
    #[must_use]
    pub fn new(root: &Path, folder: &Path) -> Self {
        let name = folder.file_name().map_or_else(|| "root".to_owned(), |name| name.to_string_lossy().into_owned());
        let name: String =
            name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
        Self { file: root.join(format!("{name}-{:016x}.json", fnv(folder.as_os_str().as_encoded_bytes()))) }
    }

    /// The file the conversation is kept in.
    #[must_use]
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// The conversation kept for the folder; an empty one when there is none yet, or when the
    /// file cannot be read as one (it is then left as it is, not overwritten until the next
    /// save).
    #[must_use]
    pub fn load(&self) -> Conversation {
        std::fs::read_to_string(&self.file)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .map(|value| Conversation::from_json(&value))
            .unwrap_or_default()
    }

    /// Writes `conversation` in one step, so a crash leaves the old file or the new one.
    ///
    /// # Errors
    ///
    /// When the folder cannot be made or the file cannot be written.
    pub fn save(&self, conversation: &Conversation) -> io::Result<()> {
        if let Some(parent) = self.file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&conversation.to_json()).map_err(io::Error::other)?;
        atomic_write(&self.file, text.as_bytes())
    }

    /// Moves the folder's conversation aside under a name with the time in it, so a new one can
    /// start without the old one being lost. Nothing happens when there is none.
    ///
    /// # Errors
    ///
    /// When the file cannot be moved.
    pub fn put_aside(&self) -> io::Result<()> {
        if !self.file.exists() {
            return Ok(());
        }
        let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
        let stem = self.file.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        let mut aside = self.file.with_file_name(format!("{stem}-{seconds}.json"));
        let mut extra = 1;
        while aside.exists() {
            aside = self.file.with_file_name(format!("{stem}-{seconds}-{extra}.json"));
            extra += 1;
        }
        std::fs::rename(&self.file, aside)
    }
}

/// The 64-bit FNV-1a hash of `bytes`: small, stable across versions and platforms, which a file
/// name that must be found again needs more than strength.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("qcli-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn a_folder_gets_its_conversation_back_and_two_folders_of_one_name_do_not_share() {
        let root = root("round");
        let store = Store::new(&root, Path::new("/work/my project"));
        assert!(store.load().is_empty(), "nothing yet");
        let mut conversation = Conversation::default();
        conversation.say("hello");
        store.save(&conversation).expect("saved");
        assert_eq!(Store::new(&root, Path::new("/work/my project")).load(), conversation);
        assert!(store.file().file_name().is_some_and(|name| name.to_string_lossy().starts_with("my_project-")));
        assert_ne!(Store::new(&root, Path::new("/other/my project")).file(), store.file());
        std::fs::remove_dir_all(&root).expect("clean up");
    }

    #[test]
    fn a_new_conversation_puts_the_old_one_aside_and_a_broken_file_reads_as_empty() {
        let root = root("aside");
        let store = Store::new(&root, Path::new("/work/a"));
        let mut conversation = Conversation::default();
        conversation.say("keep me");
        store.save(&conversation).expect("saved");
        store.put_aside().expect("moved");
        assert!(store.load().is_empty());
        let kept: Vec<_> = std::fs::read_dir(&root).expect("folder").collect();
        assert_eq!(kept.len(), 1, "the old conversation is still there");

        std::fs::write(store.file(), "{ not json").expect("write");
        assert!(store.load().is_empty());
        std::fs::remove_dir_all(&root).expect("clean up");
    }
}
