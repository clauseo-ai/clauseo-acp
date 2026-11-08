//! The data folder's spellings a shell word is searched for ([`needles`]): a deny backstop on
//! unparsed text, where no identity can apply. Its path as configured and as resolved, its 8.3
//! short name on Windows, its place under the lawyer's home (`~/…`, `~\…`, `$HOME/…`,
//! `$env:USERPROFILE\…`, `%USERPROFILE%\…`) and under the folder variables it lives in
//! (`%APPDATA%\…`, `$env:APPDATA\…`, `${env:APPDATA}\…`, `$APPDATA\…`, and the same for
//! `LOCALAPPDATA`). Separators, doubled ones and case are left to the match
//! ([`super::command::embedded_paths`], which reads a run of separators as one and folds as a
//! deny does).

use std::path::{Path, PathBuf};

/// The folder variables the data folder may live under (Windows' roaming and local app data).
const FOLDER_VARS: [&str; 2] = ["APPDATA", "LOCALAPPDATA"];

/// What the data folder's spellings are made from.
pub(super) struct Spellings<'a> {
    /// The data folder as configured, and as resolved.
    pub(super) paths: [&'a Path; 2],
    /// Its 8.3 short name ([`short_name`]), when it has one.
    pub(super) short: Option<PathBuf>,
    /// The lawyer's home, for `~` and its variables.
    pub(super) home: Option<&'a Path>,
    /// A variable's value (`std::env::var_os`), for [`FOLDER_VARS`].
    pub(super) var: &'a dyn Fn(&str) -> Option<PathBuf>,
    /// Windows' rules: `\` separates, and PowerShell's and cmd's variable forms.
    pub(super) windows: bool,
}

/// Every spelling of the data folder a shell word may hold (the module's list), deduplicated.
pub(super) fn needles(spellings: &Spellings<'_>) -> Vec<String> {
    let separator = if spellings.windows { '\\' } else { '/' };
    let mut bases: Vec<&Path> = spellings.paths.to_vec();
    bases.extend(spellings.short.as_deref());
    let mut needles: Vec<String> = Vec::new();
    let mut push = |needle: String| {
        if !needle.is_empty() && !needles.contains(&needle) {
            needles.push(needle);
        }
    };
    for base in bases {
        let Some(text) = base.to_str() else {
            continue;
        };
        push(text.to_owned());
        // `prefix` then the rest of `base` under `root`, for each prefix.
        let mut under = |root: &Path, prefixes: &[String]| {
            if let Some(rest) = text_under(text, root, spellings.windows) {
                for prefix in prefixes {
                    push(if rest.is_empty() {
                        prefix.clone()
                    } else {
                        format!("{prefix}{separator}{rest}")
                    });
                }
            }
        };
        if let Some(home) = spellings.home {
            let mut forms = vec!["~".to_owned(), "$HOME".to_owned(), "${HOME}".to_owned()];
            if spellings.windows {
                forms.extend(var_forms("USERPROFILE"));
            }
            under(home, &forms);
        }
        for name in FOLDER_VARS {
            if let Some(value) = (spellings.var)(name) {
                under(&value, &var_forms(name));
            }
        }
    }
    needles
}

/// The rest of `text` after `root`, by whole names, separators trimmed: with Windows' rules
/// `/` is `\` too and ASCII case is ignored, as a variable's value may be spelled otherwise
/// than the data folder's path (the needle is only a deny's search string). `None` when
/// `text` is not at or under `root`.
fn text_under(text: &str, root: &Path, windows: bool) -> Option<String> {
    let root = root.to_str()?;
    let normal = |text: &str| {
        if windows {
            text.replace('/', "\\")
        } else {
            text.to_owned()
        }
    };
    let separator = if windows { '\\' } else { '/' };
    let (text, root) = (normal(text), normal(root));
    let root = root.trim_end_matches(separator);
    let head = text.get(..root.len())?;
    let same = head == root || (windows && head.eq_ignore_ascii_case(root));
    let rest = text.get(root.len()..)?;
    (same && !root.is_empty() && (rest.is_empty() || rest.starts_with(separator)))
        .then(|| rest.trim_start_matches(separator).to_owned())
}

/// The ways a shell writes the variable `name`: cmd's `%name%`, PowerShell's `$env:name` and
/// `${env:name}`, and a POSIX shell's `$name` and `${name}`.
fn var_forms(name: &str) -> Vec<String> {
    vec![
        format!("%{name}%"),
        format!("$env:{name}"),
        format!("${{env:{name}}}"),
        format!("${name}"),
        format!("${{{name}}}"),
    ]
}

/// The 8.3 short name of the proven-local data folder `data` (`GetShortPathNameW`), asked once
/// per path and kept. `None` off Windows, when it has none, or when it can't be read (logged
/// once, and that alias skipped: the request is not refused for it).
pub(super) fn short_name(data: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use std::collections::HashMap;
        use std::sync::{Mutex, OnceLock};
        static SHORT: OnceLock<Mutex<HashMap<PathBuf, Option<PathBuf>>>> = OnceLock::new();
        let mut known = SHORT.get_or_init(Mutex::default).lock().ok()?;
        known
            .entry(data.to_path_buf())
            .or_insert_with(|| {
                let short = crate::network_paths::short_path(data);
                if short.is_none() {
                    tracing::warn!("the data folder's short name could not be read; not searched");
                }
                short.filter(|short| short != data)
            })
            .clone()
    }
    #[cfg(not(windows))]
    {
        let _ = data;
        None
    }
}
