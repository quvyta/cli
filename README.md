# qcli

**A small coding agent in the terminal: chat with a model that reads, searches, edits and runs in one folder, and asks before it changes anything.**

[![crates.io](https://img.shields.io/crates/v/quvyta-cli.svg)](https://crates.io/crates/quvyta-cli)
[![Licence: MIT](https://img.shields.io/crates/l/quvyta-cli.svg)](LICENSE)
[![Status: alpha](https://img.shields.io/badge/status-alpha-red.svg)](CHANGELOG.md)

**quvyta-cli** opens a full-screen chat in the folder you start it in. You write, and the model's answer streams in. The model can look at the folder on its own; before it changes a file it shows you the diff, and before it runs a command it shows you the command, and nothing happens until you allow it. Nothing outside the folder can be reached. Each folder keeps its own conversation, so opening qcli there again goes on where you stopped. It is part of the Quvyta family of terminal applications, is built on [quvyta-framework](https://github.com/quvyta/framework) and is open source under the MIT licence.

> **Alpha.** This is a first, deliberately small version. It runs the commands you allow in your own shell, with your rights, so read each one before you allow it. Settings, keys and the saved conversations may change between alpha releases. Please report anything that looks wrong at <https://github.com/quvyta/cli/issues>.

## Install

```sh
cargo install quvyta-cli
```

This installs two commands that do the same thing: `qcli` and its long name `quvyta-cli`. Rust 1.95 or newer is needed.

## Set up a provider

qcli talks to one model provider that speaks the Anthropic Messages API, streaming. On the first start a form asks for it; later `ctrl+o` opens the same form. The settings are kept in `~/.config/quvyta/cli.conf`:

```toml
address = "https://api.example.com/anthropic"   # requests go to {address}/v1/messages
model = "model-name"
key-file = "~/.config/quvyta/example-key"        # the path of a file that holds only the key
key-header = "api-key"                           # leave it out for x-api-key; some servers want another name
```

The settings name the key **file**, never the key. qcli reads the key from that file each time it sends a request, and it never writes the key anywhere: not to the settings, not to a saved conversation, not to the screen and not into an error message. Keep the file readable only by you (`chmod 600`).

The language, theme and icons follow the settings the Quvyta family shares in `~/.config/quvyta/quvyta.conf`.

## What the model can do

| Action | What it does | Asks you first |
|---|---|---|
| read a file | reads a text file in the folder (a long one is cut, and the model is told so) | no |
| list a folder | lists what is in a folder | no |
| search | finds a piece of text in the folder's files and says where (skipping `.git`, `target` and `node_modules`) | no |
| edit a file | replaces one exact piece of text with another, or writes a new file; you see the diff | **yes** |
| run a command | runs a command with `sh -c` in the folder, for at most two minutes; you see the command | **yes** |

Every path is resolved inside the folder qcli was started in, links included; anything whose real place is outside it is refused. A command cannot be confined the same way, which is why every command is shown and asked about. When you decline an edit or a command, the model is told that you declined and goes on from there.

If the folder has an `AGENTS.md`, it is sent to the model with every request as the folder's instructions.

## Keys

| Key | What it does |
|---|---|
| `enter` | send |
| `esc` | stop the reply or the command going on |
| `y` / `n` | allow or decline the edit or command on the card |
| `ctrl+n` | put the conversation aside and start a new one (the old one is kept) |
| `ctrl+o` | provider settings |
| `ctrl+q` | quit |

The mouse works everywhere: the buttons can be clicked and the conversation scrolls with the wheel. While an answer streams in you can scroll up to read; the view stays where you are until you go back to the end.

## Network

qcli connects to two places.

- **Your provider**, at the address in your settings. Nothing goes to it when qcli starts; the first request leaves when you send your first message. Each request carries the conversation of that folder, the folder's `AGENTS.md` if there is one, and the key in the header you named.
- **crates.io**, to say when a newer version is out. When qcli opens, at most once a day and without waiting for the answer, it asks crates.io's index for the versions of `quvyta-cli`. Only the package's name and the version you run go out (as the request's `User-Agent`); nothing about you, the folder or the conversation. A newer version is said in the corner with how to update. No network is silence. It is one switch for the whole Quvyta family: `update-notice = false` in `~/.config/quvyta/quvyta.conf` turns it off for every Quvyta application.

qcli sends nothing anywhere else.

## Where things are kept

- Settings: `~/.config/quvyta/cli.conf`.
- When the update question was last asked: `~/.local/state/quvyta/cli/update-check`.
- Conversations: `~/.local/state/quvyta/cli/conversations/`, one file per folder, written after every message. `ctrl+n` renames the old file with the time in it instead of deleting it.

## Licence

MIT
