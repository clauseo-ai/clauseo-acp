//! Whether a file a kept rule names is inside the matter by its real path, and the names an
//! agent or Clauseo reads its own settings from, which no rule covers (`always.rs`).

use std::path::Path;

/// Whether `requested`, a file a kept path rule names, is inside the matter `folder` by its
/// real path: both resolved through every link and junction (a file not made yet by its folder),
/// the file strictly inside the folder and under none of its own names ([`OWN_NAMES`]; any
/// case; on Windows each name without its trailing dots and spaces, as Windows opens it).
/// A link the agent swapped in after the rule was kept is caught here. False when either does
/// not resolve. File system calls: off the actor.
pub(crate) fn real_within(requested: &Path, folder: &Path, windows: bool) -> bool {
    real_rest(requested, folder, windows)
        .is_some_and(|rest| !rest.is_empty() && rest.iter().all(|name| !own_folder(name)))
}

/// Whether `requested` is the matter `folder` itself or inside it by its real path, in none of
/// its own folders ([`real_within`], the folder itself allowed): a command's working folder or
/// path word. False when either does not resolve. File system calls: off the actor.
pub(crate) fn real_inside(requested: &Path, folder: &Path, windows: bool) -> bool {
    real_rest(requested, folder, windows)
        .is_some_and(|rest| rest.iter().all(|name| !own_folder(name)))
}

/// The names of `requested`'s real path past `folder`'s, each as the system opens it; none when
/// it is not inside it or either does not resolve (a file not made yet by its folder).
fn real_rest(requested: &Path, folder: &Path, windows: bool) -> Option<Vec<String>> {
    if !requested.is_absolute() {
        return None;
    }
    let root = dunce::canonicalize(folder).ok()?;
    let real = match dunce::canonicalize(requested) {
        Ok(real) => real,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let (parent, name) = (requested.parent()?, requested.file_name()?);
            dunce::canonicalize(parent).ok()?.join(name)
        }
        Err(_) => return None,
    };
    let names = |path: &Path| -> Vec<String> {
        path.components()
            .map(|part| name_as_opened(&part.as_os_str().to_string_lossy(), windows))
            .collect()
    };
    let (root, real) = (names(&root), names(&real));
    real.strip_prefix(root.as_slice()).map(<[String]>::to_vec)
}

/// A file name as the system opens it, for comparing: on Windows lower case and without its
/// trailing dots and spaces.
pub(super) fn name_as_opened(name: &str, windows: bool) -> String {
    if windows {
        name.trim_end_matches(['.', ' ']).to_lowercase()
    } else {
        name.to_owned()
    }
}

/// A name an agent or Clauseo reads its own settings, skills or instructions from, in any
/// case: `.clauseo`, `.claude`, `.codex`, `.agents` (Codex's skills), `.mcp.json`, and the
/// instruction files `AGENTS.md`, `AGENTS.override.md`, `CLAUDE.md` and `CLAUDE.local.md`.
/// A kept rule never writes or reads one: an agent that could write its own next instructions
/// with no card would no longer be asking.
pub(super) fn own_folder(name: &str) -> bool {
    OWN_NAMES.iter().any(|own| own.eq_ignore_ascii_case(name))
}

/// The names [`own_folder`] keeps out of every kept rule.
pub(crate) const OWN_NAMES: &[&str] = &[
    ".clauseo",
    ".claude",
    ".codex",
    ".agents",
    ".mcp.json",
    "AGENTS.md",
    "AGENTS.override.md",
    "CLAUDE.md",
    "CLAUDE.local.md",
];
