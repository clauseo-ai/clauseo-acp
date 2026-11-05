//! The paths in a shell command line, for the data folder fence: its path words, the targets
//! of an `ln` taken from the link's own folder, and the data folder's text found anywhere in a
//! word (`python3 -c "open('<data>/…')"`, `grep -f<data>/…`, `curl file://<data>/…`); the
//! folders its recursive commands walk and its globs may match in; and all of that again in a
//! script it gives to `sh -c` and its kin, one level down.

use std::path::{Path, PathBuf};

use crate::pre_write_guard::{Escape, shell_script};

/// The tokens of a shell command line that may be paths: each word ([`segments`]), and of a
/// word holding `=` the part after it (`--file=/x`, `VAR=/x`), when it starts with `~`, `$HOME`
/// or `${HOME}` (each made `~`), holds a path separator, or is `..`; then the targets of each
/// `ln` ([`link_targets`]).
pub(crate) fn command_paths(line: &str, windows: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let segments = segments(line, windows);
    for word in segments.iter().flatten() {
        let word = match word.split_once('=') {
            Some((name, value)) if !name.contains('/') && !name.contains('\\') => value,
            _ => word.as_str(),
        };
        let word = home_as_tilde(word);
        let separator = word.contains('/') || (windows && word.contains('\\'));
        if word.starts_with('~') || separator || word == ".." {
            out.push(PathBuf::from(word));
        }
    }
    for segment in &segments {
        out.extend(link_targets(segment));
    }
    out
}

/// `word` with a leading `$HOME` or `${HOME}` made `~`.
pub(crate) fn home_as_tilde(word: &str) -> String {
    ["${HOME}", "$HOME"]
        .iter()
        .find_map(|var| word.strip_prefix(var).map(|rest| format!("~{rest}")))
        .unwrap_or_else(|| word.to_owned())
}

/// The folders the path words of a command line holding a glob or a brace (`*`, `?`, `[`, `{`)
/// may match in, as the shell expands them ([`super::search::pattern_roots`] from the working
/// folder; the part after `=` of a word holding one, a leading `$HOME` made `~`). Not a word
/// starting with `-`.
pub(crate) fn glob_roots(line: &str, windows: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for word in segments(line, windows).iter().flatten() {
        let word = match word.split_once('=') {
            Some((name, value)) if !name.contains('/') && !name.contains('\\') => value,
            _ => word.as_str(),
        };
        if word.starts_with('-') || !word.contains(['*', '?', '[', '{']) {
            continue;
        }
        let word = home_as_tilde(word);
        out.extend(super::search::pattern_roots(&word, ".", windows));
    }
    out
}

/// Commands that walk every folder they are given.
const WALKERS: [&str; 15] = [
    "rg", "find", "du", "tree", "tar", "bsdtar", "rsync", "fd", "fdfind", "ag", "ack", "zip", "7z",
    "robocopy", "xcopy",
];

/// Commands that walk a folder they are given with `-r`, `-R` or `--recursive` (or a cluster of
/// short options holding `r` or `R`; for `ls` only `R`, for `cp` `a` too; for `grep` also `-d
/// recurse` and `--directories=recurse`).
const RECURSIVE_WITH_FLAG: [&str; 11] = [
    "grep", "egrep", "fgrep", "cp", "rm", "chmod", "chown", "chgrp", "scp", "diff", "ls",
];

/// Words that run the command after them (`sudo grep -r …`), with their own options first.
const WRAPPERS: [&str; 12] = [
    "sudo", "doas", "env", "nice", "ionice", "nohup", "time", "command", "exec", "timeout",
    "stdbuf", "xargs",
];

/// The folders the recursive commands of a command line walk ([`WALKERS`], or
/// [`RECURSIVE_WITH_FLAG`] with its flag, or any command given PowerShell's `-Recurse`): each of
/// its operands (every word after the command that does not start with `-`, a leading `$HOME`
/// made `~`), and the working folder (`.`), where most start when given none.
pub(crate) fn recursive_roots(line: &str, windows: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for words in segments(line, windows) {
        let found = command_at(&words).and_then(|at| words.get(at..));
        let Some((command, args)) = found.and_then(<[String]>::split_first) else {
            continue;
        };
        let name = command_name(command, windows);
        if !is_recursive(&name, args) {
            continue;
        }
        out.push(PathBuf::from("."));
        let operands = args.iter().filter(|word| !word.starts_with('-'));
        out.extend(operands.map(|word| PathBuf::from(home_as_tilde(word))));
    }
    out
}

/// Where a simple command's command word is: past assignments (`A=b`) and [`WRAPPERS`] with
/// their options (and `timeout`'s duration).
fn command_at(words: &[String]) -> Option<usize> {
    let mut at = 0;
    while let Some(word) = words.get(at) {
        let assignment = word
            .split_once('=')
            .is_some_and(|(name, _)| !name.is_empty() && !name.contains(['/', '\\', '-']));
        let wrapper = WRAPPERS.contains(&word.as_str());
        if !assignment && !wrapper {
            return Some(at);
        }
        at += 1;
        if wrapper {
            while words.get(at).is_some_and(|next| next.starts_with('-')) {
                at += 1;
            }
            let duration = |next: &String| next.starts_with(|c: char| c.is_ascii_digit());
            if word == "timeout" && words.get(at).is_some_and(duration) {
                at += 1;
            }
        }
    }
    None
}

/// A command word's name: its last path part, lowercased, without `.exe`.
fn command_name(word: &str, windows: bool) -> String {
    let separator = |c: char| c == '/' || (windows && c == '\\');
    let name = word.rsplit(separator).next().unwrap_or(word).to_lowercase();
    name.strip_suffix(".exe").map(str::to_owned).unwrap_or(name)
}

/// Whether the command `name` with `args` walks the folders it is given.
fn is_recursive(name: &str, args: &[String]) -> bool {
    if WALKERS.contains(&name) || args.iter().any(|arg| arg.eq_ignore_ascii_case("-recurse")) {
        return true;
    }
    if !RECURSIVE_WITH_FLAG.contains(&name) {
        return false;
    }
    let letters: &[char] = match name {
        "ls" => &['R'],
        "cp" => &['r', 'R', 'a'],
        _ => &['r', 'R'],
    };
    let options: Vec<&String> = args.iter().take_while(|arg| *arg != "--").collect();
    let directories = name.ends_with("grep")
        && options.windows(2).any(|pair| {
            matches!(pair, [flag, value] if flag.as_str() == "-d" && value.as_str() == "recurse")
        });
    directories
        || options.iter().any(|arg| {
            matches!(
                arg.as_str(),
                "--recursive" | "--dereference-recursive" | "--archive" | "--directories=recurse"
            ) || (!arg.starts_with("--") && arg.starts_with('-') && arg.contains(letters))
        })
}

/// Every word of a command line, quotes removed, for the data folder's text.
pub(crate) fn command_words(line: &str, windows: bool) -> Vec<String> {
    segments(line, windows).into_iter().flatten().collect()
}

/// A command line's simple commands ([`split_line`]), then those of each script one of them
/// gives to another shell (`sh`, `bash`, `zsh`, `dash` or `ksh` with `-c`, `-lc`, `-ec`…,
/// PowerShell's `-Command`, `eval`; past `sudo` and the like), as the pre-write guard reads
/// them (`pre_write_guard`'s `shell_script`). One level only: a script's own scripts are not
/// read.
fn segments(line: &str, windows: bool) -> Vec<Vec<String>> {
    let outer = split_line(line, windows);
    let escape = if windows {
        Escape::Backtick
    } else {
        Escape::Backslash
    };
    let mut out = outer.clone();
    for words in &outer {
        let from = command_at(words).and_then(|at| words.get(at..));
        if let Some((script, inner)) = from.and_then(|from| shell_script(from, escape)) {
            out.extend(split_line(&script, matches!(inner, Escape::Backtick)));
        }
    }
    out
}

/// A command line's simple commands (split at `;`, `|`, `&`, `(`, `)`, backticks and line
/// ends), each as its words, as a POSIX shell would split them, near enough for paths: quotes
/// joined, `\` an escape outside single quotes except on Windows, words split at blanks, `<`
/// and `>`.
fn split_line(line: &str, windows: bool) -> Vec<Vec<String>> {
    let mut segments = Vec::new();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (Some('"') | None, '\\') if !windows => {
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            (None, '\'' | '"') => quote = Some(c),
            (None, c) if c.is_whitespace() || "<>;|&()`".contains(c) => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
                if c == '\n' || ";|&()`".contains(c) {
                    segments.push(std::mem::take(&mut words));
                }
            }
            (Some(_) | None, c) => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    segments.push(words);
    segments.retain(|words| !words.is_empty());
    segments
}

/// The targets of an `ln` in a simple command's `words`, each where the link would lead: a
/// relative target from the link's own folder (the folder named by `-t`, else both the last
/// operand's folder and the last operand itself, which may be a folder), and as written, which
/// `ln -r` and a hard link take from the working folder. An absolute or `~` target as written.
/// None when the command is not `ln`.
fn link_targets(words: &[String]) -> Vec<PathBuf> {
    let Some(at) = words
        .iter()
        .position(|word| word == "ln" || word.ends_with("/ln"))
    else {
        return Vec::new();
    };
    let mut operands: Vec<&str> = Vec::new();
    let mut folder: Option<&str> = None;
    let mut args = words.iter().skip(at + 1).map(String::as_str);
    let mut options = true;
    while let Some(arg) = args.next() {
        match arg {
            "--" if options => options = false,
            "-t" | "--target-directory" if options => folder = args.next(),
            "-S" | "--suffix" if options => {
                args.next();
            }
            _ if options && arg.starts_with("--target-directory=") => {
                folder = arg.split_once('=').map(|(_, dir)| dir);
            }
            _ if options && !arg.starts_with("--") && arg.starts_with("-t") => {
                folder = arg.strip_prefix("-t");
            }
            _ if options && arg.starts_with('-') && arg.len() > 1 => {}
            _ => operands.push(arg),
        }
    }
    let (targets, folders): (&[&str], Vec<PathBuf>) = match (folder, operands.split_last()) {
        (Some(folder), _) => (&operands, vec![PathBuf::from(folder)]),
        (None, Some((link, targets))) if !targets.is_empty() => {
            let link = Path::new(link);
            let parent = link.parent().map(Path::to_owned).unwrap_or_default();
            (targets, vec![parent, link.to_owned()])
        }
        (None, _) => (&operands, vec![PathBuf::new()]),
    };
    let mut out = Vec::new();
    for target in targets {
        let target = home_as_tilde(target);
        out.push(PathBuf::from(&target));
        if !target.starts_with('~') {
            out.extend(folders.iter().map(|folder| folder.join(&target)));
        }
    }
    out
}

/// The paths in `word` that start where one of `needles` (the data folder's text, matched in
/// ASCII case when `insensitive`) does, a leading `$HOME` or `${HOME}` made `~`, each
/// running to the next quote, blank, bracket, `,` or `;`, or the word's end.
pub(crate) fn embedded_paths(word: &str, needles: &[String], insensitive: bool) -> Vec<PathBuf> {
    let fold = |text: &str| {
        if insensitive {
            text.to_ascii_lowercase()
        } else {
            text.to_owned()
        }
    };
    let haystack = fold(word);
    let mut out = Vec::new();
    for needle in needles.iter().filter(|needle| !needle.is_empty()) {
        let needle = fold(needle);
        for (start, _) in haystack.match_indices(&needle) {
            // ASCII folding keeps every byte where it was, so `start` is a boundary in `word`.
            let tail = home_as_tilde(word.get(start..).unwrap_or_default());
            let path = tail
                .split(|c: char| c.is_whitespace() || "'\"()[]{},;<>|&`".contains(c))
                .next()
                .unwrap_or_default();
            out.push(PathBuf::from(path));
        }
    }
    out
}
