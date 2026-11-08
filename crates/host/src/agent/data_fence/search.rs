//! The folders a search may reach the whole of, for the data folder fence: a Glob's or a
//! Grep's root (`path`, else the working folder), each folder a glob pattern under it may
//! start in (braces expanded, cut at the first part that holds a glob character, climbed by
//! each `..` after it), and the folders a recursive shell command walks
//! ([`super::command::recursive_roots`]). The fence counts one as touching `<data>` when it
//! holds `<data>` or lies in it outside the session's own folders.

use std::path::PathBuf;

use serde_json::Value;

/// The most patterns one pattern's braces may expand to; past it the pattern is cut at its
/// first brace instead, which reaches more.
const MAX_EXPANSIONS: usize = 64;

/// Keys only Claude Code's Grep takes: an input with one is a Grep, whose `pattern` is a regular
/// expression and not a path.
const GREP_KEYS: [&str; 12] = [
    "glob",
    "output_mode",
    "type",
    "-i",
    "-n",
    "-A",
    "-B",
    "-C",
    "context",
    "multiline",
    "head_limit",
    "offset",
];

/// The search roots of a request's `rawInput`: for a search (an input with a string `pattern`
/// or `glob`), its `path`, or the working folder (`.`) without one, and the folders its
/// patterns may start in ([`pattern_roots`]): a Glob's `pattern`, a Grep's `glob`. Then the
/// folders each recursive command in `rawInput.command` walks.
pub(crate) fn search_roots(input: &Value, lines: &[&str], windows: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let text = |key: &str| input.get(key).and_then(Value::as_str);
    if text("pattern").is_some() || text("glob").is_some() {
        let base = text("path").filter(|path| !path.is_empty()).unwrap_or(".");
        out.push(PathBuf::from(base));
        let grep = GREP_KEYS.iter().any(|key| input.get(key).is_some());
        let pattern = if grep { text("glob") } else { text("pattern") };
        if let Some(pattern) = pattern {
            out.extend(pattern_roots(pattern, base, windows));
        }
    }
    for line in lines {
        out.extend(super::command::recursive_roots(line, windows));
        out.extend(super::command::glob_roots(line, windows));
    }
    out
}

/// The folders a glob `pattern` under the folder `base` may match in: for each of its brace
/// expansions (or, past [`MAX_EXPANSIONS`] of them, the pattern itself), its literal start
/// ([`literal_root`]), taken from `base` unless it is absolute or starts with `~`.
pub(crate) fn pattern_roots(pattern: &str, base: &str, windows: bool) -> Vec<PathBuf> {
    let mut expanded = Vec::new();
    if !expand(pattern, &mut expanded, windows) {
        expanded = vec![pattern.to_owned()];
    }
    expanded
        .iter()
        .map(|pattern| {
            let root = literal_root(pattern, windows);
            let own_start = root.starts_with('~') || absolute(&root, windows);
            if own_start {
                PathBuf::from(root)
            } else if root.is_empty() {
                PathBuf::from(base)
            } else {
                PathBuf::from(base).join(root)
            }
        })
        .collect()
}

/// Whether `text` is an absolute path: from `/`, or on Windows from `\` or a drive.
fn absolute(text: &str, windows: bool) -> bool {
    if text.starts_with('/') {
        return true;
    }
    if !windows {
        return false;
    }
    let mut chars = text.chars();
    text.starts_with('\\')
        || matches!((chars.next(), chars.next()), (Some(drive), Some(':')) if drive.is_ascii_alphabetic())
}

/// The folder a brace-free glob `pattern` may match anywhere under: its parts up to the first
/// one that holds a glob character (`*`, `?`, `[`, `{`), then `..` once for each level the parts
/// after that may climb above it (`a/*/../../b` climbs one; `**` may be no level at all). The
/// whole pattern when no part holds one. Separators are `/`, and `\` too on Windows.
pub(crate) fn literal_root(pattern: &str, windows: bool) -> String {
    let separator = |c: char| c == '/' || (windows && c == '\\');
    let mut cut: Option<usize> = None;
    let mut depth: i64 = 0;
    let mut lowest: i64 = 0;
    let mut at = 0;
    for part in pattern.split(separator) {
        match cut {
            None if part.contains(['*', '?', '[', '{']) => {
                cut = Some(at);
                depth = i64::from(part != "**");
            }
            None => {}
            Some(_) if part == ".." => {
                depth -= 1;
                lowest = lowest.min(depth);
            }
            Some(_) if part.is_empty() || part == "." || part == "**" => {}
            Some(_) => depth += 1,
        }
        at += part.len() + 1;
    }
    let Some(cut) = cut else {
        return pattern.to_owned();
    };
    let mut root = pattern.get(..cut).unwrap_or_default().to_owned();
    // Keep the root's own separator (`/`, `C:\`), drop a trailing one.
    if root.len() > 1 && root.ends_with(separator) && !root.ends_with(":\\") {
        root.pop();
    }
    for _ in 0..lowest.unsigned_abs() {
        if !root.is_empty() && !root.ends_with(separator) {
            root.push('/');
        }
        root.push_str("..");
    }
    root
}

/// Pushes every brace expansion of `pattern` onto `out` (`x{1,2}y` is `x1y` and `x2y`, nested
/// braces too); `false` past [`MAX_EXPANSIONS`]. A brace with no match is left as it is.
fn expand(pattern: &str, out: &mut Vec<String>, windows: bool) -> bool {
    let Some((open, close)) = first_group(pattern, windows) else {
        out.push(pattern.to_owned());
        return out.len() <= MAX_EXPANSIONS;
    };
    let head = pattern.get(..open).unwrap_or_default();
    let inner = pattern.get(open + 1..close).unwrap_or_default();
    let tail = pattern.get(close + 1..).unwrap_or_default();
    for choice in split_top(inner, windows) {
        if !expand(&format!("{head}{choice}{tail}"), out, windows) {
            return false;
        }
    }
    true
}

/// The byte offsets of the first `{` with its matching `}`, skipping `\`-escaped characters
/// (off Windows, where `\` is a separator).
fn first_group(pattern: &str, windows: bool) -> Option<(usize, usize)> {
    let mut depth = 0usize;
    let mut open = None;
    let mut chars = pattern.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '\\' if !windows => {
                chars.next();
            }
            '{' => {
                if depth == 0 {
                    open = Some(at);
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    return open.map(|open| (open, at));
                }
            }
            _ => {}
        }
    }
    None
}

/// `inner` split at its commas outside nested braces.
fn split_top(inner: &str, windows: bool) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut from = 0;
    let mut chars = inner.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '\\' if !windows => {
                chars.next();
            }
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(inner.get(from..at).unwrap_or_default());
                from = at + 1;
            }
            _ => {}
        }
    }
    parts.push(inner.get(from..).unwrap_or_default());
    parts
}
