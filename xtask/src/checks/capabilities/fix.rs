//! The fix lines of the capabilities check, one variant per failure kind. Each text is written
//! once, in `Display`; `fix/tests.rs` pins every variant's text.

use std::fmt;

/// A fix line of the capabilities check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {
    /// The capability folder is missing: Tauri reads the window's grants from it.
    FolderMissing,
    /// A file grants `core:default`, which also lets the webview emit events and toggle devtools.
    CoreDefault,
    /// A file mentions `remote`, which must never reach the IPC.
    Remote,
    /// A file grants a plugin permission by a forbidden prefix (`fs:`, `shell:`, ...).
    ForbiddenPrefix,
    /// A `.json5` file is not plain JSON, which `xtask` cannot decode.
    Json5,
    /// A capability file does not parse as JSON or TOML, so Tauri cannot read it either.
    Syntax,
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Fix::FolderMissing => {
                "restore it with main.json: Tauri reads the window's grants from it, and this check \
                 guards them"
            }
            Fix::CoreDefault => {
                "list only the narrow permissions the window needs; `core:default` also grants event \
                 emit and devtools toggling"
            }
            Fix::Remote => {
                "remove the remote block, or reword the text: a remote page must never reach the IPC, \
                 and `remote` is matched anywhere"
            }
            Fix::ForbiddenPrefix => {
                "do it in Rust behind a validated command; the webview gets no fs, shell, http, opener \
                 or dialog permission"
            }
            Fix::Json5 => {
                "rename it to .json and write plain JSON: xtask cannot decode json5, so it cannot check \
                 the grants"
            }
            Fix::Syntax => "fix the syntax: Tauri cannot read the file either",
        })
    }
}

#[cfg(test)]
mod tests;
