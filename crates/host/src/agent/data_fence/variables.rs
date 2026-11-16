//! PowerShell's environment variables in a path word, for the data folder fence. Under Windows'
//! rules `\` is no escape, so the `\$env:Path` that Git Bash hands PowerShell as `$env:Path`
//! is a path word to the fence, and its name holds a `:`, which the name rule refuses as an
//! alternate data stream (`network_paths::refused_windows_name`): the 2026-11-11 dry run's
//! `powershell -NoProfile -Command "\$env:Path -split ';'"` was refused unseen.
//!
//! PowerShell expands the variable, so a word is judged where it leads ([`judged`]):
//! - a leading `$env:USERPROFILE` (`${env:…}`, any case) is `~`, as `$HOME` is
//!   (`command::home_as_tilde`);
//! - any other leading variable followed by more path is judged expanded (the agent's launch
//!   variables first, then the host's, as `words.rs` reads them) and with the variable removed
//!   (an unset one expands to nothing);
//! - a bare one (the whole word, or its last name) holding a list (`;`: `Path`, `PATHEXT`) is a
//!   value, not a path, and so is one holding no absolute path (a name, an address): its name is
//!   cut to the file Windows would open for the text (`$env`, `${env`), resolved like any
//!   other; one holding a single path is judged there too; an unknown one is only cut;
//! - every variable of a path (not a search folder) is expanded too, wherever it stands
//!   (`C:\Users\$env:USERNAME\…`), and that is judged beside the forms above; an unknown one
//!   before the last name refuses the request;
//! - in a search folder a variable that can't be expanded refuses the request.
//!
//! A stream with a type (a second `:`), a bare `$env:` and every other `:` name stay refused.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// A variable's value as the agent's shell would read it, `None` when it is not known.
pub(super) type Var<'a> = &'a dyn Fn(&str) -> Option<String>;

/// The paths to judge for the agent's `path` with Windows' rules (`windows`), as a search
/// folder when `root`: `None` refuses it (a search folder's variable that can't be expanded, or
/// a path's unknown variable before its last name). Without Windows' rules, or when it is not
/// UTF-8, `path` itself.
pub(super) fn judged(path: &Path, windows: bool, root: bool, var: Var<'_>) -> Option<Vec<PathBuf>> {
    let Some(text) = path.to_str().filter(|_| windows) else {
        return Some(vec![path.to_owned()]);
    };
    let mut out = leading_forms(text, root, var)?;
    if !root {
        match every_expanded(text, var) {
            Every::Refused => return None,
            Every::Path(every) => out.push(PathBuf::from(every)),
            Every::Nothing => {}
        }
    }
    let mut seen = Vec::new();
    out.retain(|path| {
        let new = !seen.contains(path);
        seen.push(path.clone());
        new
    });
    Some(out)
}

/// The forms of `text` by its leading variable (the module's list), later variables cut.
fn leading_forms(text: &str, root: bool, var: Var<'_>) -> Option<Vec<PathBuf>> {
    let cut = cut_variables(text);
    let body = text.trim_start_matches(['/', '\\']);
    let leading = (text.len() - body.len() <= 1)
        .then(|| leading_variable(body))
        .flatten();
    let Some((name, suffix)) = leading else {
        // A search folder with a variable inside it can't be judged.
        return (!root || cut == text).then(|| vec![PathBuf::from(cut)]);
    };
    let suffix = cut_variables(suffix);
    if name.eq_ignore_ascii_case("USERPROFILE") {
        return Some(vec![PathBuf::from(format!("~{suffix}"))]);
    }
    let bare = !suffix.contains(['/', '\\']);
    // A list, or a bare value that is no path (a name, an address), is a value, not a place.
    let single = var(name)
        .filter(|value| !value.is_empty() && !is_list(value))
        .filter(|value| root || !bare || is_absolute(value));
    let mut out = Vec::new();
    if let Some(value) = &single {
        out.push(PathBuf::from(format!("{value}{suffix}")));
    } else if root {
        return None;
    }
    if !bare && !suffix.is_empty() {
        out.push(PathBuf::from(suffix));
    }
    if bare || single.is_none() {
        out.push(PathBuf::from(cut));
    }
    Some(out)
}

/// `text` with every name that starts with a variable expanded (`USERPROFILE` first as `~`), as
/// PowerShell reads it: [`Every::Nothing`] when it holds none (or starts with two separators),
/// [`Every::Refused`] when one before the last name is unknown or a list. An unknown or list
/// last name is cut ([`variable_file`]), and so is a whole word's value that is no absolute path.
fn every_expanded(text: &str, var: Var<'_>) -> Every {
    let body = text.trim_start_matches(['/', '\\']);
    let mut lead = text.get(..text.len() - body.len()).unwrap_or_default();
    if lead.len() > 1 {
        return Every::Nothing;
    }
    let pieces: Vec<&str> = body.split_inclusive(['/', '\\']).collect();
    let mut out = String::with_capacity(text.len());
    let mut any = false;
    for (at, piece) in pieces.iter().enumerate() {
        let name = piece.trim_end_matches(['/', '\\']);
        let tail = piece.get(name.len()..).unwrap_or_default();
        let Some((variable, rest)) = leading_variable(name) else {
            out.push_str(name);
            out.push_str(tail);
            continue;
        };
        any = true;
        let last = at + 1 == pieces.len();
        let value = if at == 0 && variable.eq_ignore_ascii_case("USERPROFILE") {
            Some("~".to_owned())
        } else {
            var(variable).filter(|value| !value.is_empty() && !is_list(value))
        };
        let place = |value: &String| value == "~" || is_absolute(value);
        match value {
            Some(value) if pieces.len() != 1 || place(&value) => {
                if at == 0 && place(&value) {
                    lead = "";
                }
                out.push_str(&value);
                out.push_str(rest);
            }
            _ if last => out.push_str(variable_file(name).unwrap_or(name)),
            _ => return Every::Refused,
        }
        out.push_str(tail);
    }
    if any {
        Every::Path(format!("{lead}{out}"))
    } else {
        Every::Nothing
    }
}

/// What [`every_expanded`] found.
enum Every {
    /// No variable to expand.
    Nothing,
    /// A variable before the last name can't be expanded.
    Refused,
    /// The path with every variable expanded.
    Path(String),
}

/// Whether `word` starts with PowerShell's `$env:NAME` or `${env:NAME}` ([`leading_variable`]).
pub(super) fn starts_with_variable(word: &str) -> bool {
    leading_variable(word).is_some()
}

/// `word` with a leading `${env:NAME}` (past one separator) written `$env:NAME`, as PowerShell
/// reads it, so its braces are not taken for a glob's; as it is otherwise.
pub(super) fn unbraced(word: &str) -> String {
    let body = word.trim_start_matches(['/', '\\']);
    let lead = word.get(..word.len() - body.len()).unwrap_or_default();
    match leading_variable(body) {
        Some((name, suffix)) if lead.len() <= 1 && body.starts_with("${") => {
            format!("{lead}$env:{name}{suffix}")
        }
        _ => word.to_owned(),
    }
}

/// The agent's launch variables the fence expands ([`judged`]): those holding an absolute path,
/// as the agent's shell sees them (its `TEMP` in its run folder, `AGENT_BROWSER_CONFIG`). No
/// other value is kept, so no key or token.
pub(crate) fn launch_vars(env: &BTreeMap<OsString, OsString>) -> Vec<(String, String)> {
    env.iter()
        .filter_map(|(name, value)| Some((name.to_str()?, value.to_str()?)))
        .filter(|(_, value)| !is_list(value) && is_absolute(value))
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect()
}

/// Whether `value` is a list, not one path: it holds a `;` (Windows' `Path`, `PATHEXT`), or a
/// `:` past a drive's (Unix' `PATH`), which no Windows path holds.
fn is_list(value: &str) -> bool {
    value.contains(';') || value.get(2..).is_some_and(|rest| rest.contains(':'))
}

/// Whether `value` is an absolute path by Windows' or Unix' rules.
fn is_absolute(value: &str) -> bool {
    let drive = matches!(
        value.as_bytes(),
        [letter, b':', b'/' | b'\\', ..] if letter.is_ascii_alphabetic()
    );
    value.starts_with(['/', '\\']) || drive
}

/// The variable's name and what follows it, when `body` starts with `$env:NAME` or
/// `${env:NAME}` and the rest of its name holds no `:` (a stream form stays as it is; a later
/// name's `:`, another variable's or a stream's, is judged as that name).
fn leading_variable(body: &str) -> Option<(&str, &str)> {
    let (file, stream) = body.split_once(':')?;
    let braced = variable_head(file)?;
    let rest = stream.trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_');
    let name = stream.get(..stream.len() - rest.len())?;
    let rest = if braced {
        rest.strip_prefix('}')?
    } else {
        rest
    };
    (!name.is_empty()
        && !rest
            .split(['/', '\\'])
            .next()
            .unwrap_or_default()
            .contains(':'))
    .then_some((name, rest))
}

/// Whether `file` is `$env` (`Some(false)`) or `${env` (`Some(true)`), `env` in any case.
fn variable_head(file: &str) -> Option<bool> {
    if file.eq_ignore_ascii_case("$env") {
        Some(false)
    } else if file.eq_ignore_ascii_case("${env") {
        Some(true)
    } else {
        None
    }
}

/// `text` with each name that is PowerShell's environment variable ([`variable_file`]) cut to
/// the file Windows would open for it.
fn cut_variables(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for piece in text.split_inclusive(['/', '\\']) {
        let name = piece.trim_end_matches(['/', '\\']);
        out.push_str(variable_file(name).unwrap_or(name));
        out.push_str(piece.get(name.len()..).unwrap_or_default());
    }
    out
}

/// The file part of `name` (before its `:`) when it starts with PowerShell's `$env:NAME` or
/// `${env:NAME}` (`env` in any case, `NAME` letters, digits and `_`), and holds no other `:`.
fn variable_file(name: &str) -> Option<&str> {
    let (file, stream) = name.split_once(':')?;
    let braced = variable_head(file)?;
    let rest = stream.trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_');
    let named = rest.len() < stream.len();
    let closed = !braced || rest.starts_with('}');
    (named && closed && !rest.contains(':')).then_some(file)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::judged;

    fn var(name: &str) -> Option<String> {
        match name.to_ascii_uppercase().as_str() {
            "PATH" => Some(r"C:\Windows;C:\bin".to_owned()),
            "LOCALAPPDATA" => Some(r"C:\Users\a\AppData\Local".to_owned()),
            "USERNAME" => Some("a".to_owned()),
            "SYSTEMDRIVE" => Some("C:".to_owned()),
            _ => None,
        }
    }

    fn judge(text: &str, root: bool) -> Option<Vec<String>> {
        let paths = judged(Path::new(text), true, root, &var)?;
        Some(
            paths
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
        )
    }

    #[test]
    fn a_list_or_unknown_bare_variable_is_cut_and_only_with_windows_rules() {
        let cut = |text: &str| judge(text, false).unwrap();
        assert_eq!(cut(r"\$env:Path -split ';'"), [r"\$env"]);
        assert_eq!(cut(r"\${ENV:Path} -split ';'"), [r"\${ENV"]);
        assert_eq!(cut(r"\$env:NOPE"), [r"\$env"]);
        assert_eq!(cut("$env:USERNAME"), ["$env"]);
        assert_eq!(
            super::unbraced(r"\${Env:Path} -split"),
            r"\$env:Path -split"
        );
        assert_eq!(super::unbraced(r"${env:A}\*"), r"$env:A\*");
        assert_eq!(super::unbraced(r"${HOME}/*"), r"${HOME}/*");
        for kept in [
            r"\$env:",
            r"\${env:Path",
            r"\$env:Path:$DATA",
            r"\a.txt:Path",
            r"\env:Path",
            r"C:\x",
            r"D:",
        ] {
            assert_eq!(cut(kept), [kept], "{kept}");
        }
        let unix = judged(Path::new("/m/$env:Path"), false, true, &var).unwrap();
        assert_eq!(unix, [PathBuf::from("/m/$env:Path")]);
    }

    #[test]
    fn a_leading_variable_is_expanded_and_removed() {
        assert_eq!(
            judge(r"$env:USERPROFILE\Documents\x", false).unwrap(),
            [r"~\Documents\x"]
        );
        assert_eq!(judge(r"${env:userprofile}", true).unwrap(), ["~"]);
        assert_eq!(
            judge(r"$env:LOCALAPPDATA\..\x", false).unwrap(),
            [r"C:\Users\a\AppData\Local\..\x", r"\..\x"]
        );
        assert_eq!(
            judge(r"$env:LOCALAPPDATA\*", true).unwrap(),
            [r"C:\Users\a\AppData\Local\*", r"\*"]
        );
        assert_eq!(
            judge(r"$env:LOCALAPPDATA", false).unwrap(),
            [r"C:\Users\a\AppData\Local", "$env"]
        );
    }

    #[test]
    fn every_variable_of_a_path_is_expanded_and_an_unknown_one_inside_refuses() {
        assert_eq!(
            judge(r"C:\Users\$env:USERNAME\AppData\x", false).unwrap(),
            [r"C:\Users\$env\AppData\x", r"C:\Users\a\AppData\x"]
        );
        assert_eq!(
            judge(r"\Users\${env:USERNAME}\x", false).unwrap(),
            [r"\Users\${env\x", r"\Users\a\x"]
        );
        assert_eq!(
            judge(r"$env:SystemDrive\Users\$env:USERNAME\x", false).unwrap(),
            [r"C:\Users\$env\x", r"\Users\$env\x", r"C:\Users\a\x"]
        );
        assert_eq!(
            judge(r"$env:USERPROFILE\m\$env:USERNAME\x", false).unwrap(),
            [r"~\m\$env\x", r"~\m\a\x"]
        );
        assert_eq!(judge(r"C:\x\$env:NOPE", false).unwrap(), [r"C:\x\$env"]);
        for refused in [
            r"$env:NOPE\x",
            r"C:\x\$env:NOPE\y",
            r"C:\x\$Env:PATH.Split(\y",
        ] {
            assert_eq!(judge(refused, false), None, "{refused}");
        }
    }

    #[test]
    fn a_search_folder_variable_that_cannot_be_expanded_refuses() {
        for refused in [
            r"$env:NOPE",
            r"$env:NOPE\x",
            r"$env:Path",
            r"C:\x\$env:TEMP\y",
        ] {
            assert_eq!(judge(refused, true), None, "{refused}");
        }
    }
}
