//! Pins the text of every `capabilities::Fix` variant with `fix_texts!`: one arm per variant, and
//! `assert_texts` fails until each arm has a sample, so a new variant needs both.

use super::Fix;
use crate::fix::tests::{assert_texts, fix_texts};

fix_texts! {
    fn pinned for Fix {
        Fix::FolderMissing => {
            "restore it with main.json: Tauri reads the window's grants from it, and this check \
             guards them"
        },
        Fix::CoreDefault => {
            "list only the narrow permissions the window needs; `core:default` also grants event \
             emit and devtools toggling"
        },
        Fix::Remote => {
            "remove the remote block, or reword the text: a remote page must never reach the IPC, \
             and `remote` is matched anywhere"
        },
        Fix::ForbiddenPrefix => {
            "do it in Rust behind a validated command; the webview gets no fs, shell, http, opener \
             or dialog permission"
        },
        Fix::Json5 => {
            "rename it to .json and write plain JSON: xtask cannot decode json5, so it cannot check \
             the grants"
        },
        Fix::Syntax => "fix the syntax: Tauri cannot read the file either",
        Fix::NoCapabilityFile => {
            "restore main.json in it: Tauri reads the window's grants from that folder, and this \
             check guards them"
        },
        Fix::LinkedFolder => {
            "replace the symlink with a real folder holding the files: Tauri follows it, and this \
             check reads only what is really in the folder"
        },
        Fix::RemotePermission => {
            "reword the text: a remote page must never reach the IPC, and `remote` is matched \
             anywhere in a permission file"
        },
    }
}

#[test]
fn every_fix_prints_its_pinned_text() {
    let samples = [
        Fix::FolderMissing,
        Fix::CoreDefault,
        Fix::Remote,
        Fix::ForbiddenPrefix,
        Fix::Json5,
        Fix::Syntax,
        Fix::NoCapabilityFile,
        Fix::LinkedFolder,
        Fix::RemotePermission,
    ];
    assert_texts(&samples, pinned, ARMS);
}
