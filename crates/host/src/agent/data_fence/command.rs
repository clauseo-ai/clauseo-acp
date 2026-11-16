//! The paths in a shell command line, for the data folder fence: its path words, the targets
//! of an `ln` taken from the link's own folder, and the data folder's text found anywhere in a
//! word (`python3 -c "open('<data>/…')"`, `grep -f<data>/…`, `curl file://<data>/…`); the
//! folders its recursive commands walk and its globs may match in; and all of that again in a
//! script it gives to `sh -c` and its kin, one level down.
//!
//! The rule of this file: when a word could be read two ways, check it both ways. Each reading
//! adds paths, commands or starting folders and none takes one away, since an extra one only
//! refuses more: a word after an en dash is a PowerShell option and a folder's name; a walker
//! after a script block's `{` is a command, while the words before it are read as they were;
//! `x}` is both `x` closing a block and a name ending in `}`.

use std::path::{Path, PathBuf};

use crate::network_paths::fold_name;

use crate::pre_write_guard::{Escape, powershell_words, shell_script};

/// The tokens of a shell command line that may be paths: each word ([`segments`]), and of a
/// word holding `=` the part after it (`--file=/x`, `VAR=/x`), when it starts with `~`, `$HOME`
/// or `${HOME}` (each made `~`), holds a path separator, is `..`, or with Windows' rules starts
/// with PowerShell's `$env:NAME` (judged where it leads, `variables.rs`); then the targets of
/// each `ln` ([`link_targets`]).
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
        let variable = windows && super::variables::starts_with_variable(&word);
        if word.starts_with('~') || separator || word == ".." || variable {
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
        // PowerShell's `${env:NAME}` is no brace to expand.
        let word = if windows {
            super::variables::unbraced(word)
        } else {
            word.to_owned()
        };
        if word.starts_with('-') || !word.contains(['*', '?', '[', '{']) {
            continue;
        }
        let word = home_as_tilde(&word);
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
/// short options holding `r` or `R`; for `ls` and GNU's `dir` only `R`, for `cp` `a` too; for
/// `grep` also `-d recurse` and `--directories=recurse`).
const RECURSIVE_WITH_FLAG: [&str; 12] = [
    "grep", "egrep", "fgrep", "cp", "rm", "chmod", "chown", "chgrp", "scp", "diff", "ls", "dir",
];

/// Words that run the command after them (`sudo grep -r …`), with their own options first.
const WRAPPERS: [&str; 12] = [
    "sudo", "doas", "env", "nice", "ionice", "nohup", "time", "command", "exec", "timeout",
    "stdbuf", "xargs",
];

/// PowerShell commands that walk a folder given `-Recurse` in any of its spellings
/// ([`powershell_recurses`]): Get-ChildItem, Copy-Item and Remove-Item (the cmdlets of
/// PowerShell's file system provider that take `-Recurse`; Get-Item takes none), with their
/// aliases that no POSIX system ships but `dir`. Read so in a line not known to be POSIX: when
/// the shell is unknown, GNU's `dir -r` (reverse order) walks too, a rare over-block.
const POWERSHELL_WALKERS: [&str; 11] = [
    "get-childitem",
    "gci",
    "dir",
    "copy-item",
    "copy",
    "cpi",
    "remove-item",
    "ri",
    "del",
    "erase",
    "rd",
];

/// The aliases of [`POWERSHELL_WALKERS`]' cmdlets that are POSIX commands too (`ls -r` sorts in
/// reverse, `rm -s` is no option): read as PowerShell's only in a PowerShell line.
const POWERSHELL_SHARED: [&str; 4] = ["ls", "cp", "rm", "rmdir"];

/// Verbs of PowerShell's cmdlet names (`Get-Item`): a line running one is PowerShell.
const POWERSHELL_VERBS: [&str; 24] = [
    "get", "set", "new", "remove", "copy", "move", "rename", "write", "select", "where", "foreach",
    "invoke", "test", "out", "format", "measure", "sort", "resolve", "push", "pop", "clear", "add",
    "import", "export",
];

/// The folders the recursive commands of a command line walk ([`WALKERS`], or
/// [`RECURSIVE_WITH_FLAG`] with its flag, or any command given PowerShell's `-Recurse` in full,
/// or a PowerShell walker given one of its spellings, [`is_recursive`]): each of its operands
/// (every word after the command that does not start with `-`, a leading `$HOME` made `~`,
/// as written and with a script block's closing `}` cut; for a PowerShell walker the value of
/// `-Path:x` and `-LiteralPath:x` too), and the working folder (`.`), where most start when
/// given none; each of a segment's [`commands`]. A
/// word starting with an en dash, em dash or horizontal bar stays a folder in every shell: Bash
/// takes it for a name (a folder or junction `–j`), and in PowerShell it only adds a root.
///
/// Which shell reads each part of the line ([`segment_groups`]): `shell`, the tool that sent
/// it, for the line itself; a script's shell for a script given to `pwsh` or `powershell`
/// (PowerShell) or `sh` and its kin (POSIX); the line's for `eval`'s. Where the tool is not
/// known ([`ShellKind::Unknown`]) a group of commands is PowerShell by its text: one of them is a
/// cmdlet (`Get-…`, a [`POWERSHELL_VERBS`] verb and a dash), `iex` or a
/// [`POWERSHELL_WALKERS`] name; with Windows' rules (`windows`) it is PowerShell either way, as
/// Codex's commands there are and so that `ls`, `cp` and `rm` err towards walking.
pub(crate) fn recursive_roots(line: &str, windows: bool, shell: ShellKind) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for (group, group_shell) in segment_groups(line, windows, shell) {
        for words in group {
            for (name, args) in commands(&words, windows) {
                if !is_recursive(&name, args, group_shell) {
                    continue;
                }
                out.push(PathBuf::from("."));
                let walker = powershell_walker(&name, group_shell);
                for word in args {
                    if let Some(value) = walker.then(|| path_value(word)).flatten() {
                        out.push(PathBuf::from(home_as_tilde(value)));
                    }
                    if word.starts_with('-') {
                        continue;
                    }
                    out.push(PathBuf::from(home_as_tilde(word)));
                    // A `}` closing a script block (`{ gci -r x}`), cut too.
                    if let Some(cut) = word.strip_suffix('}').filter(|cut| !cut.is_empty()) {
                        out.push(PathBuf::from(home_as_tilde(cut)));
                    }
                }
            }
        }
    }
    out
}

/// The shell a command line is written for, as the tool that sent it says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShellKind {
    /// A POSIX shell: Claude Code's `Bash` tool (Git Bash on Windows).
    Posix,
    /// Claude Code's `PowerShell` tool.
    PowerShell,
    /// Not known (no tool name, or another tool's, as Codex's): read by the line's text.
    Unknown,
}

impl ShellKind {
    /// The shell of a call whose tool is named `tool_name` (ACP's `name`): `Bash` or
    /// `PowerShell`, in any case; any other name or none is [`ShellKind::Unknown`].
    pub(crate) fn of_tool(tool_name: Option<&str>) -> Self {
        match tool_name {
            Some(name) if name.eq_ignore_ascii_case("PowerShell") => Self::PowerShell,
            Some(name) if name.eq_ignore_ascii_case("Bash") => Self::Posix,
            _ => Self::Unknown,
        }
    }
}

/// A command line's simple commands ([`segments`]) in groups, the line's own then each
/// script's, each with the shell that reads it ([`recursive_roots`]): [`ShellKind::Unknown`] made
/// PowerShell when one of the group's commands is PowerShell's own ([`powershell_name`]).
fn segment_groups(
    line: &str,
    windows: bool,
    shell: ShellKind,
) -> Vec<(Vec<Vec<String>>, ShellKind)> {
    let outer = split_line(line, windows);
    let escape = if windows {
        Escape::Backtick
    } else {
        Escape::Backslash
    };
    let line_shell = if shell == ShellKind::Unknown && windows {
        ShellKind::PowerShell
    } else {
        shell
    };
    let mut groups = vec![(outer.clone(), line_shell)];
    for words in &outer {
        for at in command_starts(words) {
            let from = words.get(at..).unwrap_or_default();
            let Some((script, inner)) = shell_script(from, escape) else {
                continue;
            };
            let name = from.first().map(|word| command_name(word, windows));
            let script_shell = match name.as_deref() {
                Some("pwsh" | "powershell") => ShellKind::PowerShell,
                Some("sh" | "bash" | "zsh" | "dash" | "ksh") => ShellKind::Posix,
                _ => line_shell,
            };
            let inner = split_line(&script, matches!(inner, Escape::Backtick));
            groups.push((inner, script_shell));
        }
    }
    for (group, shell) in &mut groups {
        let powershell = || {
            group.iter().any(|words| {
                let mut names = commands(words, windows).into_iter();
                names.any(|(name, _)| powershell_name(&name))
            })
        };
        if *shell == ShellKind::Unknown && powershell() {
            *shell = ShellKind::PowerShell;
        }
    }
    groups
}

/// Whether the command name `name` is PowerShell's own: a [`POWERSHELL_WALKERS`] name,
/// `iex`, or a cmdlet (a [`POWERSHELL_VERBS`] verb, a dash and a noun).
fn powershell_name(name: &str) -> bool {
    let cmdlet = name.split_once('-').is_some_and(|(verb, noun)| {
        POWERSHELL_VERBS.contains(&verb) && noun.starts_with(|c: char| c.is_ascii_alphabetic())
    });
    cmdlet || name == "iex" || POWERSHELL_WALKERS.contains(&name)
}

/// Whether `name` is a PowerShell command that takes `-Recurse` in a line `shell` reads: in
/// PowerShell a [`POWERSHELL_WALKERS`] or [`POWERSHELL_SHARED`] name, where the shell is
/// unknown a [`POWERSHELL_WALKERS`] one, in a POSIX shell none.
fn powershell_walker(name: &str, shell: ShellKind) -> bool {
    match shell {
        ShellKind::PowerShell => {
            POWERSHELL_WALKERS.contains(&name) || POWERSHELL_SHARED.contains(&name)
        }
        ShellKind::Unknown => POWERSHELL_WALKERS.contains(&name),
        ShellKind::Posix => false,
    }
}

/// The dashes PowerShell takes before a parameter's name: `-`, en dash, em dash, horizontal bar.
const DASHES: [char; 4] = ['-', '\u{2013}', '\u{2014}', '\u{2015}'];

/// A PowerShell parameter word's name, lowercased, without its dash ([`DASHES`]) and its
/// attached `:value` (`-Recurse:$true` is `recurse`). None for a word without a dash.
fn parameter_name(word: &str) -> Option<String> {
    let rest = word.strip_prefix(DASHES)?;
    let name = rest.split(':').next().unwrap_or(rest);
    Some(name.to_lowercase())
}

/// The value of a `-Path:x` or `-LiteralPath:x` word (any prefix of the name, `-lp:x`).
fn path_value(word: &str) -> Option<&str> {
    let name = parameter_name(word)?;
    let (_, value) = word.split_once(':')?;
    let path = !name.is_empty()
        && ("path".starts_with(&name) || "literalpath".starts_with(&name) || name == "lp");
    path.then_some(value)
}

/// Whether `word` makes a PowerShell walker recurse: `-Recurse` by any prefix of its name
/// (`-r`, `-rec`…), its alias `-s` (PowerShell 7), or `-Depth` by a prefix from `-de` (which
/// recurses, in PowerShell 5.1 too; `-d` alone is `-Directory`'s alias), in any case, after
/// any of PowerShell's [`DASHES`], with or without an attached `:value` (`-Recurse:$false`
/// counts too: erring towards a walk only refuses more).
fn powershell_recurses(word: &str) -> bool {
    let Some(name) = parameter_name(word) else {
        return false;
    };
    let recurse = !name.is_empty() && "recurse".starts_with(&name);
    let depth = name.len() >= 2 && "depth".starts_with(&name);
    recurse || depth || name == "s"
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

/// Where a simple command's commands may start, each read ([`command_at`]): its command word,
/// then after each word ending in `{` (a script block's opening: `1 | % { gci -r x }`,
/// `ForEach-Object -Begin {} -Process { … }`, a POSIX shell's `{ grep -r x /p; }`), and at
/// each word that starts with `{` or `%{` and goes on (`%{gci`; [`commands`] reads its name
/// with them cut). The words before a block stay read as they were (the file's rule).
fn command_starts(words: &[String]) -> Vec<usize> {
    let mut out: Vec<usize> = command_at(words).into_iter().collect();
    for (at, word) in words.iter().enumerate() {
        if word.ends_with('{') {
            let rest = words.get(at + 1..).unwrap_or_default();
            out.extend(command_at(rest).map(|start| at + 1 + start));
        }
        let joined = word.trim_start_matches(['%', '{']);
        if word.starts_with(['%', '{']) && word.contains('{') && !joined.is_empty() {
            out.push(at);
        }
    }
    out.dedup();
    out
}

/// Each command of a simple command (at each of [`command_starts`]): its name
/// ([`command_name`]; a word starting with `{` or `%{` also with them cut) and its arguments.
fn commands(words: &[String], windows: bool) -> Vec<(String, &[String])> {
    let mut out = Vec::new();
    for at in command_starts(words) {
        let Some((word, args)) = words.get(at..).and_then(<[String]>::split_first) else {
            continue;
        };
        out.push((command_name(word, windows), args));
        let joined = word.trim_start_matches(['%', '{']);
        if joined.len() < word.len() && !joined.is_empty() {
            out.push((command_name(joined, windows), args));
        }
    }
    out
}

/// A command word's name: its last path part, lowercased, without `.exe`.
fn command_name(word: &str, windows: bool) -> String {
    let separator = |c: char| c == '/' || (windows && c == '\\');
    let name = word.rsplit(separator).next().unwrap_or(word).to_lowercase();
    name.strip_suffix(".exe").map(str::to_owned).unwrap_or(name)
}

/// Whether the command `name` with `args` walks the folders it is given: a [`WALKERS`] one,
/// any command given `-Recurse` in full (any dash, any `:value`), a PowerShell walker given
/// any spelling of it ([`powershell_walker`] in a line `shell` reads, [`powershell_recurses`]),
/// or a [`RECURSIVE_WITH_FLAG`] one with its flag.
fn is_recursive(name: &str, args: &[String], shell: ShellKind) -> bool {
    let full = |arg: &String| parameter_name(arg).is_some_and(|name| name == "recurse");
    if WALKERS.contains(&name) || args.iter().any(full) {
        return true;
    }
    if powershell_walker(name, shell) && args.iter().any(|arg| powershell_recurses(arg)) {
        return true;
    }
    if !RECURSIVE_WITH_FLAG.contains(&name) {
        return false;
    }
    let letters: &[char] = match name {
        "ls" | "dir" => &['R'],
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

/// The command name of each simple command of a command line, its scripts' included
/// ([`segments`]): the word past assignments and [`WRAPPERS`] (`sudo`, `env A=b`…), as its last
/// path part, lowercased, without `.exe` ([`command_name`]); and of each command a script
/// block in it starts ([`commands`]).
pub(crate) fn command_names(line: &str, windows: bool) -> Vec<String> {
    segments(line, windows)
        .iter()
        .flat_map(|words| commands(words, windows).into_iter().map(|(name, _)| name))
        .collect()
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
    let groups = segment_groups(line, windows, ShellKind::Unknown);
    groups.into_iter().flat_map(|(group, _)| group).collect()
}

/// A command line's simple commands (split at `;`, `|`, `&`, `(`, `)`, backticks and line
/// ends), each as its words, as a POSIX shell would split them, near enough for paths: quotes
/// joined, `\` an escape outside single quotes, words split at blanks, `<` and `>`. With
/// Windows' rules the line is PowerShell's, read by the reader the pre-write guard and every
/// other PowerShell reader share (`pre_write_guard`'s `powershell_words`).
fn split_line(line: &str, windows: bool) -> Vec<Vec<String>> {
    if windows {
        return powershell_words(line);
    }
    let mut segments = Vec::new();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (Some('"') | None, '\\') => {
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

/// The paths in `word` that start where one of `needles` (the data folder's spellings,
/// [`super::words::needles`]) does, each the data folder's `target` path followed by what comes
/// after the needle in the word, up to the next quote, blank, bracket, `,` or `;`, or the
/// word's end: so `%APPDATA%\Clauseo\x` or `~\AppData\Roaming\Clauseo\x` is checked as the
/// path it names. Matched as a deny matches, never as an allow: in any case, folded as
/// [`fold_name`] does, when `insensitive`; each run of separators read as one (`\\` and `//`
/// inside a word, as a shell's escaping or a script's string leaves them), `\` a separator too
/// with Windows' rules (`windows`).
pub(crate) fn embedded_paths(
    word: &str,
    needles: &[String],
    target: &Path,
    insensitive: bool,
    windows: bool,
) -> Vec<PathBuf> {
    let fold = |c: char| {
        let c = if windows && c == '\\' { '/' } else { c };
        if insensitive {
            fold_name(c.encode_utf8(&mut [0; 4]))
        } else {
            c.to_string()
        }
    };
    // The word folded, a run of separators made one, and for each byte of it the start of the
    // character of `word` it came from: folding may change a character's length (`K` is three
    // bytes, `k` one).
    let folded = |text: &str| {
        let mut haystack = String::with_capacity(text.len());
        let mut origin: Vec<usize> = Vec::with_capacity(text.len());
        for (at, c) in text.char_indices() {
            let folded = fold(c);
            if folded == "/" && haystack.ends_with('/') {
                continue;
            }
            origin.extend(std::iter::repeat_n(at, folded.len()));
            haystack.push_str(&folded);
        }
        (haystack, origin)
    };
    let (haystack, origin) = folded(word);
    let mut out = Vec::new();
    for needle in needles.iter().filter(|needle| !needle.is_empty()) {
        let (needle, _) = folded(needle);
        for (start, _) in haystack.match_indices(&needle) {
            let end = start.saturating_add(needle.len());
            let after = origin.get(end).copied().unwrap_or(word.len());
            let rest = word
                .get(after..)
                .unwrap_or_default()
                .split(|c: char| c.is_whitespace() || "'\"()[]{},;<>|&`".contains(c))
                .next()
                .unwrap_or_default();
            let mut path = target.as_os_str().to_owned();
            path.push(rest);
            out.push(PathBuf::from(path));
        }
    }
    out
}
