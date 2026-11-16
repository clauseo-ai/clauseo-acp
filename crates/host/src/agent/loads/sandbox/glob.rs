//! Another matter's registered folder holding `*` or `?` in Claude's sandbox: kept as the
//! pattern it is on macOS, widened on Linux, and the sandbox off when that deny would cover the
//! session's own folders ([`fence_others`]).
//!
//! How Claude Code 2.1.293 reads a deny path, from the sandbox runtime it bundles
//! (`@anthropic-ai/sandbox-runtime` 0.0.77/0.0.78, whose `globToRegex` is the bundle's `W4t`
//! byte for byte; 0.0.79 on reads names beside patterns, which the bundle does not): a path
//! holding `*`, `?`, `[` or `]` becomes a Seatbelt `(regex …)` of `globToRegex` with `(/.*)?`
//! before its `$` (`denyGlobRegex`, every deny list), so it covers all beneath what it matches.
//! `globToRegex` escapes `.^$+{}()|\` (a `\` is a literal backslash), escapes a `[` that no
//! `]` follows, then turns `**/` into `(.*/)?`, `**` into `.*`, `*` into `[^/]*` and `?` into
//! `[^/]`, and leaves `[…]` a regex class. So `*` and `?` can only match more than the folder
//! they came from, never miss it, so they stay bare: escaped as `[*]` or `[?]` they would miss
//! it, since the converter rewrites them inside a class too (`[[^/]*]`, a class of `[`, `^` and
//! `/`; 0.0.83's tokenizer would not). `[x]` alone would miss `[x]` (a class matching `x`),
//! which is why every `[` is escaped as `[[]` (`super::escape_brackets`), and a lone `]` is
//! literal. On Linux a deny glob is matched against what is on disk when each command is
//! wrapped (the runtime's walk), so the sandbox widens it to a folder instead.

use std::path::{Path, PathBuf};

use super::links::{fold, holds, keys, off_data_volume};
use super::{ClaudeSandbox, SandboxOs, WILDCARDS, texts};

/// Whether the sandbox can name `path` as the folder it is: UTF-8, no `*` or `?`.
pub(super) fn writable(path: &Path) -> bool {
    path.to_str().is_some_and(|text| !text.contains(WILDCARDS))
}

/// The longest folder above `path` the sandbox can name as itself ([`writable`]), which covers
/// it; `/` at worst.
pub(super) fn widened(path: &Path) -> PathBuf {
    let above = path.ancestors().skip(1).find(|above| writable(above));
    above.unwrap_or_else(|| Path::new("/")).to_path_buf()
}

/// A deny would cover one of the session's own folders.
#[derive(Debug)]
pub(super) struct CoversOwn;

/// Each deny in `sandbox` that holds `*` or `?` (another matter's registered folder:
/// `ClaudeSandbox::wildcard` turned every other off) on `os`: on macOS kept as the pattern,
/// [`CoversOwn`] when [`covers`] has it cover one of the session's own folders (what its
/// lists allow, before escaping, and `matter`, the matter folder as stored and where it leads);
/// on Linux replaced by the folder above it with neither ([`widened`]), [`CoversOwn`] when
/// that folder holds one of them, compared by name.
pub(super) fn fence_others(
    os: SandboxOs,
    sandbox: &mut ClaudeSandbox,
    matter: &[String],
) -> Result<(), CoversOwn> {
    let own: Vec<String> = sandbox
        .allow_write
        .iter()
        .chain(&sandbox.allow_read)
        .chain(matter)
        .cloned()
        .collect();
    for list in [&mut sandbox.deny_read, &mut sandbox.deny_write] {
        let mut kept: Vec<&Path> = Vec::new();
        let mut widen: Vec<PathBuf> = Vec::new();
        for deny in list.iter() {
            if !deny.contains(WILDCARDS) {
                kept.push(Path::new(deny));
                continue;
            }
            if os == SandboxOs::Mac {
                if own.iter().any(|path| covers(deny, path)) {
                    return Err(CoversOwn);
                }
                kept.push(Path::new(deny));
                continue;
            }
            let above = widened(Path::new(deny));
            let above_keys = keys(os, &above);
            if own
                .iter()
                .any(|path| holds(&keys(os, Path::new(path)), &above_keys))
            {
                return Err(CoversOwn);
            }
            widen.push(above);
        }
        if widen.is_empty() {
            continue;
        }
        let mut next = texts(kept);
        for path in texts(widen.iter().map(PathBuf::as_path)) {
            if !next.contains(&path) {
                next.push(path);
            }
        }
        *list = next;
    }
    Ok(())
}

/// Whether Claude Code's macOS deny for `pattern` covers `path` as Seatbelt matches it, which
/// ignores case and Unicode form (measured): [`deny_covers`] of the two as they are, folded
/// (`network_paths::fold_name`: lower case, NFD) and lower case in NFC, each with and without a
/// leading data volume (`/System/Volumes/Data`, a firmlink twin). Any of them covering refuses,
/// which only ever refuses more.
pub(crate) fn covers(pattern: &str, path: &str) -> bool {
    use unicode_normalization::UnicodeNormalization;
    let nfc = |text: &str| -> String { text.nfc().flat_map(char::to_lowercase).collect() };
    let forms = |text: &str| {
        let mut forms = vec![text.to_owned()];
        forms.extend(off_data_volume(SandboxOs::Mac, text));
        forms
    };
    forms(pattern).iter().any(|pattern| {
        forms(path).iter().any(|path| {
            deny_covers(pattern, path)
                || deny_covers(&fold(SandboxOs::Mac, pattern), &fold(SandboxOs::Mac, path))
                || deny_covers(&nfc(pattern), &nfc(path))
        })
    })
}

/// One step of `globToRegex`'s regex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// The character itself.
    Char(char),
    /// `?`: `[^/]`.
    One,
    /// `*`: `[^/]*`.
    Name,
    /// `**`: `.*`.
    Any,
    /// `**/`: `(.*/)?`.
    Dirs,
}

/// `globToRegex`'s placeholders, which it turns into regex even where the path spelled them.
const DIRS_MARK: &str = "__GLOBSTAR_SLASH__";
const ANY_MARK: &str = "__GLOBSTAR__";

/// The steps of `pattern`, read as `globToRegex` reads it once `[` is escaped: a `[`, a `]` and
/// a `\` are themselves, and of a run of `*` the last two go with a `/` after it, the rest in
/// pairs as `**` and a single one left as `*`.
fn steps(pattern: &str) -> Vec<Step> {
    let mut out = Vec::new();
    let mut rest = pattern;
    while let Some(c) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix(DIRS_MARK) {
            out.push(Step::Dirs);
            rest = after;
            continue;
        }
        if let Some(after) = rest.strip_prefix(ANY_MARK) {
            out.push(Step::Any);
            rest = after;
            continue;
        }
        if c == '*' {
            let after = rest.trim_start_matches('*');
            let run = rest.len() - after.len();
            let beyond = after.strip_prefix('/').filter(|_| run >= 2);
            let single = run - if beyond.is_some() { 2 } else { 0 };
            out.extend(std::iter::repeat_n(Step::Any, single / 2));
            if single % 2 == 1 {
                out.push(Step::Name);
            }
            if beyond.is_some() {
                out.push(Step::Dirs);
            }
            rest = beyond.unwrap_or(after);
            continue;
        }
        out.push(if c == '?' { Step::One } else { Step::Char(c) });
        let mut chars = rest.chars();
        chars.next();
        rest = chars.as_str();
    }
    out
}

/// Whether Claude Code's macOS deny for `pattern` (`[` not yet escaped) covers `path`: its
/// `denyGlobRegex`, `^…(/.*)?$`, matches it, so `path` is what the pattern names or lies
/// beneath it. Character by character, where Seatbelt goes byte by byte: a `?` against a
/// character of more than one byte matches here and not there, which only refuses more.
pub(crate) fn deny_covers(pattern: &str, path: &str) -> bool {
    let chars: Vec<char> = path.chars().collect();
    // `at`: the positions in `path` the steps so far can end at.
    let mut at = vec![false; chars.len() + 1];
    mark(&mut at, 0);
    for step in steps(pattern) {
        let mut next = vec![false; chars.len() + 1];
        let from_all: Vec<usize> = (0..at.len())
            .filter(|&i| at.get(i) == Some(&true))
            .collect();
        for from in from_all {
            let rest = chars.get(from..).unwrap_or_default();
            let name_len = rest.iter().take_while(|&&c| c != '/').count();
            match step {
                Step::Char(c) if rest.first() == Some(&c) => mark(&mut next, from + 1),
                Step::One if name_len > 0 => mark(&mut next, from + 1),
                Step::Char(_) | Step::One => {}
                Step::Name => (from..=from + name_len).for_each(|to| mark(&mut next, to)),
                Step::Any => (from..=chars.len()).for_each(|to| mark(&mut next, to)),
                Step::Dirs => {
                    mark(&mut next, from);
                    let slashes = rest.iter().enumerate().filter(|(_, c)| **c == '/');
                    slashes.for_each(|(i, _)| mark(&mut next, from + i + 1));
                }
            }
        }
        at = next;
    }
    (0..at.len())
        .any(|end| at.get(end) == Some(&true) && matches!(chars.get(end), None | Some('/')))
}

/// Marks position `i` in `set` reached.
fn mark(set: &mut [bool], i: usize) {
    if let Some(slot) = set.get_mut(i) {
        *slot = true;
    }
}

#[cfg(test)]
mod tests {
    use super::deny_covers;

    /// `*` and `?` match the very folder they came from and what is beneath it, and more.
    #[test]
    fn a_wildcard_only_ever_covers_more() {
        for name in [
            "/x/What?", "/x/A*B", "/x/[x]", "/x/a\\b", "/x/**/y", "/x/***",
        ] {
            assert!(deny_covers(name, name), "{name}");
            assert!(deny_covers(name, &format!("{name}/inner")), "{name}");
        }
        assert!(deny_covers("/x/What?", "/x/Whats"));
        assert!(!deny_covers("/x/What?", "/x/What"));
        assert!(!deny_covers("/x/What?", "/x/What/s"));
        assert!(deny_covers("/x/A*B", "/x/AB"));
        assert!(!deny_covers("/x/A*B", "/x/A/B"));
        assert!(deny_covers("/x/**/y", "/x/y"));
        assert!(deny_covers("/x/**/y", "/x/p/q/y"));
        assert!(deny_covers("/x/?", "/x/A"));
        assert!(deny_covers("/x/?", "/x/A/notes"));
        assert!(!deny_covers("/x/?", "/x/AB"));
        assert!(!deny_covers("/x/?", "/x"));
        assert!(deny_covers("/x/__GLOBSTAR__", "/x/p/q"));
    }
}
