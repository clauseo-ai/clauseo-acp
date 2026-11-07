//! Windows paths compared as Windows compares them, on every OS (so the runner's checks are
//! tested on macOS too): a full local path `X:\…` or `\\?\X:\…`, split into its parts, with
//! no `.` or `..` part, compared part by part without case.
//!
//! And M3's check, [`codex_reuse`]: tier B never grants on, links from or points at the
//! lawyer's `~\.codex`.

use std::fmt;

/// A full local Windows path, split: the drive (`C:`) and the parts below it.
#[derive(Clone, PartialEq, Eq)]
pub struct WinPath {
    drive: Option<String>,
    parts: Vec<String>,
}

impl fmt::Debug for WinPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display())
    }
}

impl WinPath {
    /// `text` as a full local path: `X:\…` or `\\?\X:\…` (either slash), or `\\?\UNC\…` /
    /// `\\server\share\…` (no drive). `None` for a relative path, a drive-relative `X:a`, a
    /// device path, or any `.` or `..` part.
    pub fn parse(text: &str) -> Option<WinPath> {
        if text.contains('\0') {
            return None;
        }
        let text = text.replace('/', "\\");
        let (drive, rest) = if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            (None, rest.to_owned())
        } else if let Some(rest) = text.strip_prefix(r"\\?\") {
            drive_and_rest(rest)?
        } else if let Some(rest) = text.strip_prefix(r"\\") {
            if rest.starts_with(['.', '?']) {
                return None;
            }
            (None, rest.to_owned())
        } else {
            drive_and_rest(&text)?
        };
        let parts: Vec<String> = rest
            .split('\\')
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect();
        if parts.iter().any(|part| part == "." || part == "..") {
            return None;
        }
        if drive.is_none() && parts.len() < 2 {
            return None;
        }
        Some(WinPath { drive, parts })
    }

    /// The drive, `C:`, when the path is on one.
    pub fn drive(&self) -> Option<&str> {
        self.drive.as_deref()
    }

    /// The path below the drive, `\Users\a\b` (`\` for the drive's root): `HOMEPATH`'s form.
    pub fn below_drive(&self) -> String {
        if self.parts.is_empty() {
            return "\\".to_owned();
        }
        self.parts.iter().fold(String::new(), |mut text, part| {
            text.push('\\');
            text.push_str(part);
            text
        })
    }

    /// The path in its plain form: `C:\a\b`, or `\\server\share\a`.
    pub fn display(&self) -> String {
        match &self.drive {
            Some(drive) => format!("{drive}{}", self.below_drive()),
            None => format!("\\{}", self.below_drive()),
        }
    }

    /// `self` with `name` appended (a single part, no separator).
    pub fn join(&self, name: &str) -> WinPath {
        let mut joined = self.clone();
        joined.parts.extend(
            name.split('\\')
                .filter(|p| !p.is_empty())
                .map(str::to_owned),
        );
        joined
    }

    /// Whether `self` is `root` or lies below it, compared without case.
    pub fn within(&self, root: &WinPath) -> bool {
        same(self.drive.as_deref(), root.drive.as_deref())
            && root.parts.len() <= self.parts.len()
            && root
                .parts
                .iter()
                .zip(&self.parts)
                .all(|(a, b)| fold(a) == fold(b))
    }
}

/// Whether `text` names a path that lies in `root`.
pub fn text_within(text: &str, root: &WinPath) -> bool {
    WinPath::parse(text.trim().trim_matches('"')).is_some_and(|path| path.within(root))
}

/// What M3 found: why the launch would reuse the lawyer's Codex folder.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CodexReuse {
    /// A path the launch grants or names lies in `~\.codex`; which field.
    #[error("tier B never uses the lawyer's .codex folder: the launch's {0} lies in it")]
    Path(&'static str),
    /// `CODEX_HOME` lies in `~\.codex`.
    #[error("tier B never uses the lawyer's .codex folder: CODEX_HOME lies in it")]
    CodexHome,
    /// The credential link is the lawyer's `~\.codex\auth.json` (same file ID): granting on it
    /// would change that file's own DACL.
    #[error("tier B never grants on the lawyer's .codex\\auth.json: the credential link is it")]
    LinkedLogin,
    /// The credential link couldn't be checked against `~\.codex\auth.json`, so it is refused.
    #[error("tier B couldn't check the credential link against the lawyer's .codex ({0})")]
    Unchecked(std::io::ErrorKind),
    /// The credential link is a symbolic link or another reparse point: it must be a hard link
    /// (R17), and a link's own file ID says nothing about what it points at.
    #[error("tier B refuses a credential link that is a symbolic link")]
    ReparseLink,
}

/// M3, the pure part: refuses a launch any of whose `paths` (name, path), or whose `CODEX_HOME`
/// in `env`, lies in `<profile>\.codex`. The file-ID part (the credential link being that
/// folder's `auth.json` through a hard link) needs Windows; the outer runner does it, and so
/// should the launcher before it applies any entry.
pub fn codex_reuse<'a>(
    profile: &WinPath,
    paths: impl IntoIterator<Item = (&'static str, &'a str)>,
    env: &[(String, String)],
) -> Result<(), CodexReuse> {
    let codex = profile.join(".codex");
    for (name, path) in paths {
        if text_within(path, &codex) {
            return Err(CodexReuse::Path(name));
        }
    }
    let home = env
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("CODEX_HOME"))
        .any(|(_, value)| text_within(value, &codex));
    if home {
        return Err(CodexReuse::CodexHome);
    }
    Ok(())
}

/// Splits `X:\rest` into `X:` and `rest`; `None` unless it starts with a drive and a `\`.
fn drive_and_rest(text: &str) -> Option<(Option<String>, String)> {
    let mut chars = text.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    if chars.next() != Some(':') {
        return None;
    }
    let rest = chars.as_str();
    if !rest.starts_with('\\') {
        return None;
    }
    Some((
        Some(format!("{}:", letter.to_ascii_uppercase())),
        rest.to_owned(),
    ))
}

fn same(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        (None, None) => true,
        _ => false,
    }
}

/// A part folded as Windows compares names: upper case, character by character.
fn fold(part: &str) -> String {
    part.chars().flat_map(char::to_uppercase).collect()
}
