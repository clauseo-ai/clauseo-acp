//! The data folder's spellings a shell word is searched for ([`needles`]): a deny backstop on
//! unparsed text, where no identity can apply. Its path as configured and as resolved, its 8.3
//! short name on Windows, its place under the lawyer's home (`~/…`, `~\…`, `$HOME/…`,
//! `$env:USERPROFILE\…`, `%USERPROFILE%\…`) and under the folder variables it lives in
//! (`%APPDATA%\…`, `$env:APPDATA\…`, `${env:APPDATA}\…`, `$APPDATA\…`, and the same for
//! `LOCALAPPDATA`). The short name is mixed name by name with the long one too
//! ([`mixed_spellings`]: `C:\Users\JONATH~1\AppData\…` and `C:\Users\Jonathan\APPDAT~1\…`), as
//! Windows opens any such mix. Separators, doubled ones and case are left to the match
//! ([`super::command::embedded_paths`], which reads a run of separators as one and folds as a
//! deny does).

use std::path::{Path, PathBuf};

/// The folder variables the data folder may live under (Windows' roaming and local app data).
const FOLDER_VARS: [&str; 2] = ["APPDATA", "LOCALAPPDATA"];

/// The most mixes of long and short names [`mixed_spellings`] makes (four names with a short
/// form of their own); past it the all-long and all-short spellings are searched, and any `~digit`
/// name next to the data folder's names ([`tilde_tails`]).
const MAX_MIXED: usize = 16;

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
    let mut bases: Vec<PathBuf> = spellings
        .paths
        .iter()
        .map(|path| path.to_path_buf())
        .collect();
    if let Some(short) = &spellings.short {
        bases.extend(mixed_spellings(
            spellings.paths[1],
            short,
            spellings.windows,
        ));
    }
    let mut needles: Vec<String> = Vec::new();
    let mut push = |needle: String| {
        if !needle.is_empty() && !needles.contains(&needle) {
            needles.push(needle);
        }
    };
    for base in &bases {
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

/// Every spelling of the data folder whose names are each its long name (in `long`, the
/// resolved path) or its 8.3 short name (in `short`, `GetShortPathNameW`'s answer for it, which
/// shortens each name on its own, so the two have the same names one for one): the all-long and
/// all-short ones and each mix, as Windows opens `C:\Users\JONATH~1\AppData\Roaming\Clauseo` and
/// `C:\Users\Jonathan\APPDAT~1\Roaming\Clauseo` alike. A name whose short form is the same (ASCII
/// case aside) gives one choice. Only `short` when the two don't pair up name for name; past
/// [`MAX_MIXED`] mixes the all-long and all-short ones and [`tilde_tails`], which match every mix
/// and more (a deny's search strings: over-matching is its safe side).
fn mixed_spellings(long: &Path, short: &Path, windows: bool) -> Vec<PathBuf> {
    let (Some(long), Some(short)) = (long.to_str(), short.to_str()) else {
        return vec![short.to_path_buf()];
    };
    let separator = |c: char| c == '/' || (windows && c == '\\');
    let (long_names, short_names): (Vec<&str>, Vec<&str>) = (
        long.split(separator).collect(),
        short.split(separator).collect(),
    );
    if long_names.len() != short_names.len() {
        return vec![PathBuf::from(short)];
    }
    let choices: Vec<Vec<&str>> = long_names
        .iter()
        .zip(&short_names)
        .map(|(long, short)| {
            if long.eq_ignore_ascii_case(short) {
                vec![*long]
            } else {
                vec![*long, *short]
            }
        })
        .collect();
    let count = choices
        .iter()
        .try_fold(1_usize, |count, names| count.checked_mul(names.len()));
    let joiner = if windows { "\\" } else { "/" };
    if count.is_none_or(|count| count > MAX_MIXED) {
        let mut spellings = vec![PathBuf::from(long), PathBuf::from(short)];
        spellings.extend(tilde_tails(&choices, joiner).into_iter().map(PathBuf::from));
        return spellings;
    }
    let mut spellings: Vec<Vec<&str>> = vec![Vec::new()];
    for names in &choices {
        spellings = spellings
            .iter()
            .flat_map(|head| {
                names.iter().map(move |name| {
                    let mut next = head.clone();
                    next.push(*name);
                    next
                })
            })
            .collect();
    }
    spellings
        .into_iter()
        .map(|names| PathBuf::from(names.join(joiner)))
        .collect()
}

/// Past [`MAX_MIXED`]: for each name with a short form, that name as any `~digit` name followed
/// by the long names after it (its own short name, and `~1` to `~9` with its extension: the end
/// of any short name), or, for the last name, preceded by the name before it in either form (its
/// own short name, and its stem with `~1` to `~9` and its extension: a whole name, since the
/// search is by substring). Every mix holds one: the last name it spells short is followed by long
/// names only. Over-matching (`~3\Roaming\Clauseo` after any folder) is a deny's safe side; a
/// lone `~1` is never searched.
fn tilde_tails(choices: &[Vec<&str>], joiner: &str) -> Vec<String> {
    let mut tails = Vec::new();
    for (at, names) in choices.iter().enumerate() {
        let [_, short] = names.as_slice() else {
            continue;
        };
        let rest: Vec<&str> = choices
            .get(at + 1..)
            .unwrap_or_default()
            .iter()
            .filter_map(|names| names.first().copied())
            .collect();
        let mut forms = vec![(*short).to_owned()];
        if let Some((stem, after)) = short.rsplit_once('~') {
            let ext = after.trim_start_matches(|c: char| c.is_ascii_digit());
            if ext.len() < after.len() {
                let stem = if rest.is_empty() { stem } else { "" };
                forms.extend((1..=9).map(|digit| format!("{stem}~{digit}{ext}")));
            }
        }
        if rest.is_empty() {
            let Some(before) = at.checked_sub(1).and_then(|before| choices.get(before)) else {
                continue;
            };
            for prefix in before {
                tails.extend(forms.iter().map(|form| format!("{prefix}{joiner}{form}")));
            }
        } else {
            let rest = rest.join(joiner);
            tails.extend(forms.iter().map(|form| format!("{form}{joiner}{rest}")));
        }
    }
    tails
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
