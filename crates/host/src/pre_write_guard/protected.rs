//! What the agent may not change, whatever the matter: the paths Claude Code's entry names with
//! [`FLAG`] (`agent::loads::guard_entry`): in Claude's config folder its `settings.json`, its own
//! `.claude.json` and `.config.json`, and each name a launch clears there (`CLAUDE.md`,
//! `skills`, `commands`, `agents`, `rules`, `plugins` and the rest), and the built-in plugin;
//! and, with [`EDIT_FLAG`], the run's packs plugin, which only a file tool may not change (a
//! command runs a pack's scripts there). Codex's hook names none.
//!
//! A file tool's path, or a shell word's path as the path layer takes it (`shell.rs`: from the
//! input's `cwd` and from every folder a `cd`, `pushd` or `Set-Location` may have reached), names
//! one when it is that path or lies under it, compared by its text normalised and by the walk's
//! resolved path against the protected path's same two forms, in any ASCII case and with trailing
//! dots and spaces off on macOS and Windows, and on Windows with an NTFS stream suffix off
//! (`settings.json::$DATA`). Claude's config folder ([`HOME_FLAG`]) stands in for
//! a reference to its variable in a word (`$env:CLAUDE_CONFIG_DIR`, `${env:…}`,
//! `$CLAUDE_CONFIG_DIR`, `${…}`, `%…%`; any ASCII case on Windows), there and in a folder
//! change. Past the words read one by one, a shell command's whole text is searched for each
//! protected path's tail, its last three names ([`Protected::named_in_text`]). A file tool
//! naming one is refused; so is a shell command naming one, in each reading the guard makes of
//! it, unless the whole command only reads (`names::read_only`, the held-file rule's).

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use super::shell::walked_resolutions;
use super::{Place, Reach, Refusal, Targets, lexical, names};
use crate::network_paths::name_folds_to;

/// The argument that names a protected path, followed by it; repeated for each.
pub(super) const FLAG: &str = "--protected";

/// The argument that names a path only a file tool may not change, followed by it; repeated for
/// each: a shell command may name it (the run's packs plugin, whose scripts it runs).
pub(super) const EDIT_FLAG: &str = "--protected-edit";

/// How many of a protected path's last parts its tail is ([`Protected::named_in_text`]).
const TAIL_PARTS: usize = 3;

/// The argument that names Claude's config folder, followed by it.
pub(super) const HOME_FLAG: &str = "--claude-home";

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
    /// The paths only a file tool may not change ([`EDIT_FLAG`]), in the same two forms.
    edit_roots: Vec<PathBuf>,
    /// Each of `roots`' last [`TAIL_PARTS`] parts, lowercased and joined by `/`.
    tails: Vec<String>,
    home: Option<String>,
}

impl Protected {
    /// The absolute paths after each [`FLAG`] and [`EDIT_FLAG`] in `args`, and the folder after
    /// [`HOME_FLAG`]. A value that is not absolute, or a folder that is not UTF-8, is left out
    /// ([`skipped`] counts them).
    pub(super) fn of(args: &[OsString], reach: &Reach) -> Self {
        let mut found = Self::default();
        for pair in args.windows(2) {
            let [flag, value] = pair else {
                continue;
            };
            let edit = flag == EDIT_FLAG;
            if flag != FLAG && !edit && flag != HOME_FLAG {
                continue;
            }
            let path = PathBuf::from(value);
            if left_out(flag, value) {
                continue;
            }
            if flag == HOME_FLAG {
                found.home = value.to_str().map(str::to_owned);
                continue;
            }
            if !edit && let Some(tail) = tail_of(&path) {
                found.tails.push(tail);
            }
            let roots = if edit {
                &mut found.edit_roots
            } else {
                &mut found.roots
            };
            for form in std::iter::once(lexical(&path)).chain(reach.walked(&path)) {
                if !roots.contains(&form) {
                    roots.push(form);
                }
            }
        }
        found
    }

    /// Whether no path is protected.
    pub(super) fn is_empty(&self) -> bool {
        self.roots.is_empty() && self.edit_roots.is_empty()
    }

    /// Whether `path` (absolute), by its text normalised or as one of `walked` (its resolved
    /// places), is a protected path or lies under one, those only a file tool may not change
    /// included: a file tool's path.
    pub(super) fn covers(&self, path: &Path, walked: &[PathBuf]) -> bool {
        self.covered(path, walked, true)
    }

    /// [`Self::covers`] for a shell word's path: not the paths only a file tool may not change.
    pub(super) fn covers_shell(&self, path: &Path, walked: &[PathBuf]) -> bool {
        self.covered(path, walked, false)
    }

    /// [`Self::covers`], with or without the paths of [`EDIT_FLAG`].
    fn covered(&self, path: &Path, walked: &[PathBuf], edit: bool) -> bool {
        let edit_roots = if edit { &self.edit_roots[..] } else { &[] };
        std::iter::once(lexical(path))
            .chain(walked.iter().cloned())
            .any(|form| {
                self.roots
                    .iter()
                    .chain(edit_roots)
                    .any(|root| within(&form, root, FOLD, cfg!(windows)))
            })
    }

    /// The words of a shell command's whole `text`, however long, in which a protected path's
    /// tail (its last [`TAIL_PARTS`] parts) appears, in any case and with either separator
    /// (repeats of one counting once): what the word-by-word reading, which stops after
    /// `MAX_WORDS` words, may not reach. Empty when none does.
    pub(super) fn named_in_text(&self, text: &str) -> Vec<String> {
        if self.tails.is_empty() {
            return Vec::new();
        }
        text.split(|c: char| c.is_whitespace() || matches!(c, ';' | '|' | '&' | '(' | ')'))
            .filter(|word| {
                let folded = fold_separators(&self.expand(word));
                self.tails.iter().any(|tail| folded.contains(tail.as_str()))
            })
            // As the command's own reading spells the word: no redirection before it, no quotes.
            .map(|word| {
                let redirection = |c: char| matches!(c, '>' | '<' | '&') || c.is_ascii_digit();
                word.trim_start_matches(redirection)
                    .trim_matches(['"', '\''])
                    .to_owned()
            })
            .collect()
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

/// Whether the value after `flag` (one of [`FLAG`], [`EDIT_FLAG`], [`HOME_FLAG`]) is left out:
/// not absolute, or a config folder that is not UTF-8.
fn left_out(flag: &OsStr, value: &OsStr) -> bool {
    !Path::new(value).is_absolute() || (flag == HOME_FLAG && value.to_str().is_none())
}

/// How many protected values in `args` are left out ([`left_out`]): the guard says so on
/// stderr, whatever it decides.
pub(super) fn skipped(args: &[OsString]) -> usize {
    args.windows(2)
        .filter(|pair| match pair {
            [flag, value] => {
                (flag == FLAG || flag == EDIT_FLAG || flag == HOME_FLAG) && left_out(flag, value)
            }
            _ => false,
        })
        .count()
}

/// Whether `path` is `root` or lies under it, part by part; with `fold`, two names are the same
/// in any ASCII case (and each code point a case-insensitive volume folds to an ASCII letter)
/// and with trailing dots and spaces off; with `streams` (NTFS), a name of `path` is taken
/// without an alternate data stream suffix (`settings.json::$DATA`, `CLAUDE.md:x`).
fn within(path: &Path, root: &Path, fold: bool, streams: bool) -> bool {
    let mut parts = path.components();
    root.components().all(|want| {
        parts.next().is_some_and(|got| {
            let got = if let std::path::Component::Normal(name) = got
                && streams
            {
                without_stream(name)
            } else {
                Cow::Borrowed(got.as_os_str())
            };
            same_name(&got, want.as_os_str(), fold)
        })
    })
}

/// `name` without what follows its first `:`, an NTFS alternate data stream's name and type;
/// as it is when it has none or is not UTF-8.
fn without_stream(name: &OsStr) -> Cow<'_, OsStr> {
    match name.to_str().and_then(|text| text.split_once(':')) {
        Some((file, _stream)) => Cow::Owned(OsString::from(file)),
        None => Cow::Borrowed(name),
    }
}

/// The last [`TAIL_PARTS`] names of `path`, lowercased and joined by `/`; none when it has
/// fewer or one is not UTF-8.
fn tail_of(path: &Path) -> Option<String> {
    let names: Vec<&str> = path
        .components()
        .filter_map(|part| {
            if let std::path::Component::Normal(name) = part {
                Some(name.to_str())
            } else {
                None
            }
        })
        .collect::<Option<_>>()?;
    let from = names.len().checked_sub(TAIL_PARTS)?;
    Some(names.get(from..)?.join("/").to_lowercase())
}

/// `word` lowercased, with `\` as `/`, each run of separators as one, and quotes taken out.
fn fold_separators(word: &str) -> String {
    let mut folded = String::with_capacity(word.len());
    for c in word.chars() {
        let c = if c == '\\' { '/' } else { c };
        if matches!(c, '"' | '\'' | '`') || (c == '/' && folded.ends_with('/')) {
            continue;
        }
        folded.extend(c.to_lowercase());
    }
    folded
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
fn expand_variable<'a>(word: &'a str, value: &str, any_case: bool) -> Cow<'a, str> {
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
            text,
            line,
            escape,
            ..
        } => {
            // Past the words read one by one, the whole text is searched for a protected
            // path's tail.
            let tailed = protected.named_in_text(text);
            let hits: Vec<String> = words.iter().chain(&tailed).cloned().collect();
            !hits.is_empty()
                && !names::read_only(line, *escape, &[], &hits, place.folder, place.reach)
        }
        // A command too nested to read is refused as such (`judge`).
        Targets::Unread => false,
    };
    named.then_some(Refusal::Protected)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::Path;

    use super::{
        EDIT_FLAG, FLAG, HOME_FLAG, Protected, VARIABLE_FORMS, expand_variable, tail_of, within,
    };
    use crate::pre_write_guard::Reach;

    /// A path is within a protected one when it is it or lies under it, part by part: not a
    /// sibling whose name only starts the same. Folded, any ASCII case and trailing dots match.
    #[test]
    fn a_path_is_within_a_protected_one_part_by_part() {
        let root = Path::new("/d/home/settings.json");
        let within = |path: &str, root: &Path, fold| within(Path::new(path), root, fold, false);
        assert!(within("/d/home/settings.json", root, false));
        assert!(within(
            "/d/home/skills/x/SKILL.md",
            Path::new("/d/home/skills"),
            false
        ));
        for other in ["/d/home/settings.json.bak", "/d/home/settings", "/d/home"] {
            assert!(!within(other, root, false), "{other}");
            assert!(!within(other, root, true), "{other}");
        }
        assert!(!within(
            "/d/home/skills2/x",
            Path::new("/d/home/skills"),
            true
        ));
        assert!(!within("/D/Home/Settings.JSON", root, false));
        assert!(within("/D/Home/Settings.JSON", root, true));
        assert!(within("/d/home/settings.json. ", root, true));
    }

    /// With NTFS streams, a name with a stream suffix is the file's name: the default stream
    /// spelled out, a named one, and one under a protected folder. Without, it is another name.
    #[test]
    fn an_ntfs_stream_suffix_names_the_file() {
        let root = Path::new("/d/home/settings.json");
        for path in [
            "/d/home/settings.json::$DATA",
            "/d/home/settings.json:x",
            "/d/home/Settings.JSON:x:$DATA",
        ] {
            assert!(within(Path::new(path), root, true, true), "{path}");
            assert!(!within(Path::new(path), root, false, false), "{path}");
        }
        let skills = Path::new("/d/home/skills");
        assert!(within(
            Path::new("/d/home/skills::$INDEX_ALLOCATION/x"),
            skills,
            true,
            true
        ));
        assert!(!within(Path::new("/d/home/skillsx:y"), skills, true, true));
    }

    /// A tail is a path's last three names, lowercased; a shorter path has none.
    #[test]
    fn a_protected_paths_tail_is_its_last_three_names() {
        assert_eq!(
            tail_of(Path::new("/d/agents/claude-acp/home/Settings.json")).as_deref(),
            Some("claude-acp/home/settings.json")
        );
        assert_eq!(tail_of(Path::new("/a/b")), None);
    }

    /// The whole text is searched for a protected path's tail, past any word limit, in any
    /// case, with either separator, doubled ones, quotes, a redirection before it and the
    /// config variable; each word that holds it comes back as the command spells it. Not a
    /// path only a file tool may not change, and not a word that holds just the last name.
    #[test]
    fn a_tail_anywhere_in_the_text_names_the_protected_path() {
        let base = if cfg!(windows) { "C:/d" } else { "/d" };
        let home = format!("{base}/agents/claude-acp/home");
        let settings = format!("{home}/settings.json");
        let packs = format!("{base}/run/s1/skill-packs");
        let args: Vec<OsString> = [
            HOME_FLAG,
            home.as_str(),
            FLAG,
            settings.as_str(),
            EDIT_FLAG,
            packs.as_str(),
            FLAG,
            "relative/x",
        ]
        .iter()
        .map(OsString::from)
        .collect();
        let protected = Protected::of(&args, &Reach::new(Path::new("/m")));
        assert_eq!(super::skipped(&args), 1);
        let filler = "x ".repeat(200);
        let cases = [
            (
                "C:\\D\\Agents\\Claude-ACP\\Home\\Settings.json",
                "C:\\D\\Agents\\Claude-ACP\\Home\\Settings.json",
            ),
            (
                "'/d/agents//claude-acp/home/settings.json'",
                "/d/agents//claude-acp/home/settings.json",
            ),
            (
                ">C:\\d\\agents\\claude-acp\\home\\settings.json::$DATA",
                "C:\\d\\agents\\claude-acp\\home\\settings.json::$DATA",
            ),
            (
                "\"$CLAUDE_CONFIG_DIR/settings.json\"",
                "$CLAUDE_CONFIG_DIR/settings.json",
            ),
        ];
        for (word, named) in cases {
            let text = format!("echo {filler}; cp a {word}");
            assert_eq!(protected.named_in_text(&text), [named], "{word}");
        }
        for text in [
            format!("echo {filler} > /m/settings.json"),
            format!("python {filler} /d/run/s1/skill-packs/skills/x/fill.py"),
        ] {
            assert!(protected.named_in_text(&text).is_empty(), "{text}");
        }
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
