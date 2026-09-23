# Changelog

Every release of quvyta-cli, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/). While the version is an alpha, anything may change between releases, the settings and the saved conversations included.

## 0.1.0-alpha.2 - 2026-09-23

### Added

- qcli says when a newer version is out. When it opens, at most once a day and without waiting for the answer, it asks crates.io for the newest version of `quvyta-cli`; only the package's name and the version you run go out. It is on unless you turn it off with `update-notice = false` in `~/.config/quvyta/quvyta.conf`, the one switch for the whole Quvyta family. The README's Network section says exactly what is sent.

## 0.1.0-alpha.1 - 2026-09-23

### Added

- A full-screen chat in the folder qcli is started in, with the answer streaming in and the model's thinking shown faintly.
- One provider speaking the Anthropic Messages API, set in `~/.config/quvyta/cli.conf` or in the form behind `ctrl+o`. The key is read from a file and never written anywhere.
- Five actions the model can ask for: reading a file, listing a folder and searching are done at once; editing a file shows the diff and running a command shows the command, and both wait until you allow them. Nothing outside the folder can be reached.
- The folder's `AGENTS.md` goes to the model as instructions.
- One saved conversation per folder, reopened where it stopped; `ctrl+n` starts a new one and keeps the old.
- The conversation keeps its end in view while the answer streams, and stays put while you scroll up to read.
- Text in English, German, Spanish, French, Japanese, Brazilian Portuguese, Russian, Turkish and Simplified Chinese.
