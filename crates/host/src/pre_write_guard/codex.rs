//! The Codex side of the guard: a patch's targets, the matter's `.codex` folder
//! (clauseo-acp-private#184), and Codex's own home (`CODEX_HOME`).
//!
//! Codex's own home: with [`CODEX_HOME_FLAG`] and the folder (Codex's hook passes it,
//! `agent/loads/codex.rs`), a file tool's or a patch's target in that folder, the folder itself
//! too, or the folder above it or the one above that, is refused, held open or not, and so is a
//! shell command whose words may name one from any folder the command may be in (`cd <home>`
//! first, then a bare name). Under Codex's hook, with the flag or without, so is a shell command
//! whose whole text names the variable `CODEX_HOME` or the folder the home sits in
//! (`codex-acp`), reads included. The check recognises the folder's own path
//! and the variable's name; Codex's sandbox, which never lists the folder as writable
//! (`agent/codex_fence.rs`), the approval auto-deny and the launch removal
//! (`agent/codex/home_clear.rs`) stand behind it.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use super::{CODEX_HOME_FLAG, Reach, Targets, lexical};
use crate::network_paths::{FOLD_CASE, fold_name, name_folds_to};

/// The folder after [`CODEX_HOME_FLAG`], anywhere in `args`, when it is absolute.
pub(super) fn codex_home_arg(args: &[OsString]) -> Option<PathBuf> {
    let at = args.iter().position(|arg| arg == CODEX_HOME_FLAG)?;
    let home = PathBuf::from(args.get(at.saturating_add(1))?);
    home.is_absolute().then_some(home)
}

/// The variable that names Codex's home in its shell's environment.
const HOME_VAR: &str = "CODEX_HOME";

/// The name of the folder that holds Codex's home and install (`<data>/agents/codex-acp`),
/// which no other path a command needs holds.
const FOLDER_NAME: &str = "codex-acp";

/// Whether `targets`, taken from `base`, reach Codex's home `home`: a file tool's or a patch's
/// target in it, or the folder above it or the one above that ([`HomeForms::reached`]); for a
/// shell command, a patch target in its text, or a place one of its words may name from any
/// folder the command may be in after its `cd`s (so `cd <home>` and then a bare name too). Reads
/// count as well: the agent has no reason to touch that folder. A shell command's whole text is
/// read by [`names_codex_home`], which needs no home.
pub(super) fn reaches_codex_home(
    targets: &Targets,
    home: &Path,
    base: &Path,
    reach: &Reach,
) -> bool {
    let forms = HomeForms::of(home, reach);
    let inside = |paths: &[PathBuf]| {
        paths
            .iter()
            .any(|path| forms.reached(&base.join(path), reach))
    };
    match targets {
        Targets::Named(paths) | Targets::Patch(paths) => inside(paths),
        Targets::Shell { patch, named, .. } => inside(patch) || inside(named),
        // Refused for that alone (`judge`).
        Targets::Unread => false,
    }
}

/// Whether a shell command's whole text names Codex's home without its path: the variable's
/// name ([`names_home_var`]) or [`FOLDER_NAME`], in any case, anywhere in it.
/// It needs no home, so under Codex's hook it applies whether the hook line named the folder or
/// not (`judge`).
pub(super) fn names_codex_home(targets: &Targets) -> bool {
    match targets {
        Targets::Shell { text, .. } => {
            names_home_var(text) || text.to_ascii_lowercase().contains(FOLDER_NAME)
        }
        Targets::Named(_) | Targets::Patch(_) | Targets::Unread => false,
    }
}

/// Codex's home as given and as walked, and the two folders above each, as [`home_key`]s:
/// worked out once per call.
struct HomeForms {
    homes: Vec<Vec<String>>,
    above: Vec<Vec<String>>,
}

impl HomeForms {
    fn of(home: &Path, reach: &Reach) -> Self {
        let homes: Vec<Vec<String>> = std::iter::once(home.to_path_buf())
            .chain(reach.walked(home))
            .map(|home| home_key(&home))
            .filter(|home| !home.is_empty())
            .collect();
        let mut above = Vec::new();
        for home in &homes {
            for up in 1..=2_usize {
                let names = home
                    .get(..home.len().saturating_sub(up))
                    .unwrap_or_default();
                // Never the root or a drive, nor a folder right under one.
                if names.len() > 2 && !above.iter().any(|seen: &Vec<String>| seen == names) {
                    above.push(names.to_vec());
                }
            }
        }
        Self { homes, above }
    }

    /// Whether `absolute` is the home or lies in it, or is one of the two folders above it: by
    /// its text with `.` and `..` taken off, or as the network-safe walk resolved it
    /// ([`Reach::walked`]); names folded where the file system folds them, and on Windows with
    /// trailing dots and spaces off (a refusal, so over-matching is safe).
    fn reached(&self, absolute: &Path, reach: &Reach) -> bool {
        std::iter::once(absolute.to_path_buf())
            .chain(reach.walked(absolute))
            .any(|form| {
                let form = home_key(&form);
                self.homes.iter().any(|home| form.starts_with(home)) || self.above.contains(&form)
            })
    }
}

/// `path`'s names for [`HomeForms`]: a verbatim `\\?\` prefix off, `.` and `..` taken.
fn home_key(path: &Path) -> Vec<String> {
    lexical(dunce::simplified(path))
        .components()
        .map(|part| {
            let name = part.as_os_str().to_string_lossy();
            let name: &str = &name;
            let name = if cfg!(windows) {
                name.trim_end_matches(['.', ' '])
            } else {
                name
            };
            if FOLD_CASE {
                fold_name(name)
            } else {
                name.to_owned()
            }
        })
        .collect()
}

/// Whether `text` names the variable [`HOME_VAR`] as a whole name, in any case: `$CODEX_HOME`,
/// `${CODEX_HOME}`, `$env:CODEX_HOME`, `%CODEX_HOME%`, `printenv CODEX_HOME` and the like.
pub(super) fn names_home_var(text: &str) -> bool {
    let upper = text.to_ascii_uppercase();
    let part_of_name = |c: char| c.is_ascii_alphanumeric() || c == '_';
    upper.match_indices(HOME_VAR).any(|(at, _)| {
        let before = upper.get(..at).and_then(|head| head.chars().next_back());
        let after = upper
            .get(at.saturating_add(HOME_VAR.len())..)
            .and_then(|tail| tail.chars().next());
        !before.is_some_and(part_of_name) && !after.is_some_and(part_of_name)
    })
}

/// Whether `path`, taken from `base`, is `<folder>/.codex` or lies under it, resolved
/// by the network-safe walk (never when it is or leads to a network location, [`Reach::walked`],
/// its walked path used as it is) or lexically.
pub(super) fn in_codex_folder(folder: &Path, base: &Path, path: &Path, reach: &Reach) -> bool {
    let absolute = base.join(path);
    if let Some(walked) = reach.walked(&absolute)
        && let Some(inside) = clauseo_matters::matter_path_of_resolved(folder, &walked)
        && inside.as_str().split('/').next().is_some_and(is_codex)
    {
        return true;
    }
    lexical(&absolute)
        .strip_prefix(folder)
        .ok()
        .and_then(|relative| relative.components().next())
        .is_some_and(|first| first.as_os_str().to_str().is_some_and(is_codex))
}

/// Whether `name` is `.codex`, ignoring trailing dots and spaces and case, folded as a
/// case-insensitive volume folds names ([`name_folds_to`]: a refusal, so over-matching is safe).
fn is_codex(name: &str) -> bool {
    name_folds_to(OsStr::new(name.trim_end_matches(['.', ' '])), ".codex")
}

/// Whether any part of `path`, split at `/` and `\\`, is `.codex` ([`is_codex`]).
pub(super) fn has_codex_part(path: &Path) -> bool {
    path.to_str().is_some_and(text_has_codex_part)
}

/// Whether any part of `text`, split at `/` and `\\`, is `.codex` ([`is_codex`]).
pub(super) fn text_has_codex_part(text: &str) -> bool {
    text.split(['/', '\\']).any(is_codex)
}

/// The files a Codex `apply_patch` patch deletes (`*** Delete File:`).
pub(super) fn patch_deletes(patch: &str) -> Vec<PathBuf> {
    patch
        .lines()
        .filter_map(|line| line.trim().strip_prefix("*** Delete File:"))
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// The files a Codex `apply_patch` patch adds, updates, deletes or moves to.
pub(super) fn patch_targets(patch: &str) -> Vec<PathBuf> {
    const HEADERS: [&str; 4] = [
        "*** Add File:",
        "*** Update File:",
        "*** Delete File:",
        "*** Move to:",
    ];
    patch
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            HEADERS
                .iter()
                .find_map(|header| line.strip_prefix(header))
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    /// `.codex` is refused in every case and with trailing dots and spaces (it has no `s` or
    /// `k` for the long s or the Kelvin sign to stand for).
    #[test]
    fn a_codex_folder_is_found_in_any_folded_spelling() {
        for name in [".codex", ".CODEX", ".CoDeX", ".codex. "] {
            assert!(super::is_codex(name), "{name}");
        }
        assert!(!super::is_codex(".codexx"));
    }

    /// The variable is found in each shell's spelling, and only as a whole name.
    #[test]
    fn codex_homes_variable_is_found_as_a_whole_name() {
        for text in [
            "echo x > $CODEX_HOME/config.toml",
            "echo x > \"${CODEX_HOME}/config.toml\"",
            "Set-Content \"$env:CODEX_HOME\\config.toml\" x",
            "Set-Content ${env:codex_home}\\config.toml x",
            "type %CODEX_HOME%\\config.toml",
        ] {
            assert!(super::names_home_var(text), "{text}");
        }
        for text in [
            "echo $CODEX_HOMES",
            "echo $MY_CODEX_HOME",
            "echo codex home",
        ] {
            assert!(!super::names_home_var(text), "{text}");
        }
    }
}
