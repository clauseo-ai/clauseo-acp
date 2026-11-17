//! What the agent may not change, whatever the matter: the paths Claude Code's entry names with
//! [`FLAG`] (`agent::loads::guard_entry`): Claude's own `settings.json`, `settings.local.json`,
//! `CLAUDE.md`, `skills` and `plugins` in its config folder, the built-in plugin and the run's
//! packs plugin. Codex's hook names none.
//!
//! A file tool's path, or a shell word's path as the path layer takes it (`shell.rs`: from the
//! input's `cwd` and from every folder a `cd`, `pushd` or `Set-Location` may have reached), names
//! one when it is that path or lies under it, compared by its text normalised and by the walk's
//! resolved path against the protected path's same two forms, in any ASCII case and with trailing
//! dots and spaces off on macOS and Windows. Claude's config folder ([`HOME_FLAG`]) stands in for
//! a reference to its variable in a word (`$env:CLAUDE_CONFIG_DIR`, `${env:…}`,
//! `$CLAUDE_CONFIG_DIR`, `${…}`, `%…%`; any ASCII case on Windows), there and in a folder
//! change. A file tool naming one is refused; so is a shell command naming one, in each reading
//! the guard makes of it, unless the whole command only reads (`names::read_only`, the held-file
//! rule's).

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use super::shell::walked_resolutions;
use super::{Place, Reach, Refusal, Targets, lexical, names};
use crate::network_paths::name_folds_to;

/// The argument that names a protected path, followed by it; repeated for each.
pub const FLAG: &str = "--protected";

/// The argument that names Claude's config folder, followed by it.
pub const HOME_FLAG: &str = "--claude-home";

/// The ways a shell word refers to Claude's config folder's variable
/// (`agent::home::CLAUDE_CONFIG_VAR`), longest first.
const VARIABLE_FORMS: [&str; 5] = [
    "${env:CLAUDE_CONFIG_DIR}",
    "$env:CLAUDE_CONFIG_DIR",
    "${CLAUDE_CONFIG_DIR}",
    "$CLAUDE_CONFIG_DIR",
    "%CLAUDE_CONFIG_DIR%",
];

/// Whether names compare as a case-insensitive volume compares them.
const FOLD: bool = cfg!(any(windows, target_os = "macos"));

/// The protected paths of one guard run, each by its text normalised and as the walk resolves
/// it, and Claude's config folder, when the arguments name them.
#[derive(Debug, Default)]
pub(super) struct Protected {
    roots: Vec<PathBuf>,
    home: Option<String>,
}

impl Protected {
    /// The absolute paths after each [`FLAG`] in `args`, and the folder after [`HOME_FLAG`].
    pub(super) fn of(args: &[OsString], reach: &Reach) -> Self {
        let mut roots = Vec::new();
        let mut home = None;
        for pair in args.windows(2) {
            let [flag, value] = pair else {
                continue;
            };
            let path = PathBuf::from(value);
            if !path.is_absolute() {
                continue;
            }
            if flag == FLAG {
                for form in std::iter::once(lexical(&path)).chain(reach.walked(&path)) {
                    if !roots.contains(&form) {
                        roots.push(form);
                    }
                }
            } else if flag == HOME_FLAG {
                home = value.to_str().map(str::to_owned);
            }
        }
        Self { roots, home }
    }

    /// Whether no path is protected.
    pub(super) fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// Whether `path` (absolute), by its text normalised or as one of `walked` (its resolved
    /// places), is a protected path or lies under one.
    pub(super) fn covers(&self, path: &Path, walked: &[PathBuf]) -> bool {
        std::iter::once(lexical(path))
            .chain(walked.iter().cloned())
            .any(|form| self.roots.iter().any(|root| within(&form, root, FOLD)))
    }

    /// `word` with each reference to Claude's config folder's variable replaced by the folder,
    /// when the arguments name it ([`expand_variable`]).
    pub(super) fn expand<'a>(&self, word: &'a str) -> Cow<'a, str> {
        match &self.home {
            Some(home) => expand_variable(word, home, cfg!(windows)),
            None => Cow::Borrowed(word),
        }
    }
}

/// Whether `path` is `root` or lies under it, part by part; with `fold`, two names are the same
/// in any ASCII case (and each code point a case-insensitive volume folds to an ASCII letter)
/// and with trailing dots and spaces off.
pub(super) fn within(path: &Path, root: &Path, fold: bool) -> bool {
    let mut parts = path.components();
    root.components().all(|want| {
        parts
            .next()
            .is_some_and(|got| same_name(got.as_os_str(), want.as_os_str(), fold))
    })
}

/// Whether `got` and `want` name the same entry ([`within`]).
fn same_name(got: &OsStr, want: &OsStr, fold: bool) -> bool {
    if got == want {
        return true;
    }
    if !fold {
        return false;
    }
    match (got.to_str(), want.to_str()) {
        (Some(got), Some(want)) => {
            let got = got.trim_end_matches(['.', ' ']);
            let want = want.trim_end_matches(['.', ' ']);
            got.eq_ignore_ascii_case(want)
                || (want.is_ascii() && name_folds_to(OsStr::new(got), want))
        }
        _ => got.eq_ignore_ascii_case(want),
    }
}

/// `word` with each of [`VARIABLE_FORMS`] in it replaced by `value`: in any ASCII case when
/// `any_case` (Windows, whose variables ignore case), else as written. A form without braces or
/// `%` counts only when no letter, digit or `_` follows it (`$CLAUDE_CONFIG_DIRS` is another
/// variable).
pub(super) fn expand_variable<'a>(word: &'a str, value: &str, any_case: bool) -> Cow<'a, str> {
    if !word.contains(['$', '%']) {
        return Cow::Borrowed(word);
    }
    let mut out = String::with_capacity(word.len() + value.len());
    let mut rest = word;
    let mut changed = false;
    while let Some(at) = rest.find(['$', '%']) {
        out.push_str(rest.get(..at).unwrap_or_default());
        let from = rest.get(at..).unwrap_or_default();
        let form = VARIABLE_FORMS.iter().find(|form| {
            let Some(text) = from.get(..form.len()) else {
                return false;
            };
            let same = if any_case {
                text.eq_ignore_ascii_case(form)
            } else {
                text == **form
            };
            let after = from.get(form.len()..).unwrap_or_default();
            let ended = form.ends_with(['}', '%'])
                || !after.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
            same && ended
        });
        if let Some(form) = form {
            out.push_str(value);
            rest = from.get(form.len()..).unwrap_or_default();
            changed = true;
        } else {
            // `$` and `%` are one byte each.
            out.push_str(from.get(..1).unwrap_or_default());
            rest = from.get(1..).unwrap_or_default();
        }
    }
    if !changed {
        return Cow::Borrowed(word);
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// Why `targets` are refused at `place` for naming a protected path, if they are: a file tool's
/// (or a patch's) path, or a shell command naming one (`Targets::Shell`'s `protected` words)
/// that does more than read.
pub(super) fn refusal(targets: &Targets, place: &Place<'_>) -> Option<Refusal> {
    let protected = place.protected;
    if protected.is_empty() {
        return None;
    }
    let named = match targets {
        Targets::Named(paths) | Targets::Patch(paths) => paths.iter().any(|path| {
            let path = place.base.join(path);
            protected.covers(&path, &walked_resolutions(&path, place.reach))
        }),
        Targets::Shell {
            protected: words,
            line,
            escape,
            ..
        } => {
            !words.is_empty()
                && !names::read_only(line, *escape, &[], words, place.folder, place.reach)
        }
    };
    named.then_some(Refusal::Protected)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{VARIABLE_FORMS, expand_variable, within};

    /// A path is within a protected one when it is it or lies under it, part by part: not a
    /// sibling whose name only starts the same. Folded, any ASCII case and trailing dots match.
    #[test]
    fn a_path_is_within_a_protected_one_part_by_part() {
        let root = Path::new("/d/home/settings.json");
        assert!(within(Path::new("/d/home/settings.json"), root, false));
        assert!(within(
            Path::new("/d/home/skills/x/SKILL.md"),
            Path::new("/d/home/skills"),
            false
        ));
        for other in ["/d/home/settings.json.bak", "/d/home/settings", "/d/home"] {
            assert!(!within(Path::new(other), root, false), "{other}");
            assert!(!within(Path::new(other), root, true), "{other}");
        }
        assert!(!within(
            Path::new("/d/home/skills2/x"),
            Path::new("/d/home/skills"),
            true
        ));
        assert!(!within(Path::new("/D/Home/Settings.JSON"), root, false));
        assert!(within(Path::new("/D/Home/Settings.JSON"), root, true));
        assert!(within(Path::new("/d/home/settings.json. "), root, true));
    }

    /// Each form of the variable stands for the folder, alone or with a path after it; in any
    /// case only when asked; never a longer variable's name.
    #[test]
    fn each_form_of_the_config_variable_is_the_folder() {
        assert!(
            VARIABLE_FORMS
                .iter()
                .all(|form| form.contains(crate::agent::home::CLAUDE_CONFIG_VAR))
        );
        for form in VARIABLE_FORMS {
            assert_eq!(expand_variable(form, "/h", false), "/h");
            assert_eq!(
                expand_variable(&format!("{form}/settings.json"), "/h", false),
                "/h/settings.json"
            );
            assert_eq!(
                expand_variable(&format!("{form}\\skills\\x"), "/h", false),
                "/h\\skills\\x"
            );
            let lower = form.to_ascii_lowercase();
            assert_eq!(expand_variable(&lower, "/h", true), "/h");
            assert_eq!(expand_variable(&lower, "/h", false), lower);
        }
        assert_eq!(
            expand_variable("--path=$CLAUDE_CONFIG_DIR/x", "/h", false),
            "--path=/h/x"
        );
        for kept in [
            "$CLAUDE_CONFIG_DIRS/x",
            "$env:CLAUDE_CONFIG_DIR_X",
            "$HOME/x",
            "100%",
            "$",
        ] {
            assert_eq!(expand_variable(kept, "/h", true), kept, "{kept}");
        }
    }
}
