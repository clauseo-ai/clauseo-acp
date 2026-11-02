//! The name backstop (clauseo-acp-private#162, review round 2): a shell command whose text names a
//! file of the matter that is held open right now is refused, whatever folder games it plays,
//! unless the whole command only reads ([`read_only`]); and `stetdoc`'s writing and serving forms
//! ([`stetdoc`]).

use std::path::{Path, PathBuf};

use clauseo_protocol::MatterPath;

use super::shell::script;
use super::words::{Escape, Segment, Segments, ansi_c_char, shell_segments};

/// The most folder entries [`held_files`] reads; a larger matter is read in part, shallow folders
/// first.
const MAX_ENTRIES: usize = 20_000;

/// The most words [`read_only`] reads; a longer command is not read-only.
const MAX_READ_WORDS: usize = 4096;

/// A file of the matter that is held open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Held {
    pub(super) path: MatterPath,
    /// Its last part, folded as [`fold`] does.
    name: String,
}

impl Held {
    /// The held file at `path`.
    pub(super) fn new(path: MatterPath) -> Self {
        let name = fold(path.as_str().rsplit('/').next().unwrap_or_default());
        Self { path, name }
    }
}

/// The files of the matter `folder` held open by an Office owner file
/// ([`clauseo_matters::held_open`], in each folder that has an owner file `~$…`), read breadth
/// first: no folder whose name starts with `.`, no symlink followed, at most [`MAX_ENTRIES`]
/// entries. A file held on Windows by a share mode alone (no owner file) is not found here.
pub(super) fn held_files(folder: &Path) -> Vec<Held> {
    let root = dunce::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let mut held = Vec::new();
    let mut queue: std::collections::VecDeque<PathBuf> = std::iter::once(PathBuf::new()).collect();
    let mut entries = 0;
    while let Some(relative) = queue.pop_front() {
        let Ok(read) = std::fs::read_dir(root.join(&relative)) else {
            continue;
        };
        let mut files = Vec::new();
        let mut owned = false;
        for entry in read.flatten() {
            entries += 1;
            if entries > MAX_ENTRIES {
                return held;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if !name.starts_with('.') {
                    queue.push_back(relative.join(&name));
                }
            } else if name.starts_with("~$") {
                owned = true;
            } else {
                files.push(name);
            }
        }
        if !owned {
            continue;
        }
        for name in files {
            if let Some(path) = MatterPath::from_relative(&relative.join(&name))
                && clauseo_matters::held_open(&root, &path).is_some()
            {
                held.push(Held::new(path));
            }
        }
    }
    held
}

/// `text` as the file system compares names: lower case on macOS and Windows, as it is elsewhere.
fn fold(text: &str) -> String {
    if cfg!(any(target_os = "macos", windows)) {
        text.to_lowercase()
    } else {
        text.to_owned()
    }
}

/// Whether `c` may stand before or after a whole file name in a command: whitespace, a quote or
/// a shell metacharacter; before it also a folder separator.
fn bounds(c: char, before: bool) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '\'' | '"'
                | '`'
                | ';'
                | '&'
                | '|'
                | '<'
                | '>'
                | '('
                | ')'
                | '{'
                | '}'
                | '['
                | ']'
                | '='
                | ','
                | ':'
                | '$'
                | '*'
                | '?'
                | '!'
                | '#'
        )
        || (before && matches!(c, '/' | '\\'))
}

/// Whether the folded `text` holds the folded `name` as a whole name ([`bounds`]).
fn holds(text: &str, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    text.match_indices(name).any(|(at, _)| {
        let before = text.get(..at).and_then(|head| head.chars().next_back());
        let after = text
            .get(at + name.len()..)
            .and_then(|tail| tail.chars().next());
        before.is_none_or(|c| bounds(c, true)) && after.is_none_or(|c| bounds(c, false))
    })
}

/// The first of `held` whose name `text` holds as a whole name.
fn named<'a>(text: &str, held: &'a [Held]) -> Option<&'a Held> {
    let text = fold(text);
    held.iter().find(|file| holds(&text, &file.name))
}

/// The first of `held` that the shell command `command` names, in its text with escaped newlines
/// joined and, under [`Escape::Backslash`], `$'…'` decoded; comments are not dropped.
pub(super) fn named_in<'a>(command: &str, escape: Escape, held: &'a [Held]) -> Option<&'a Held> {
    named(&raw_text(command, escape), held)
}

/// `command` with escaped newlines joined and (POSIX) `$'…'` replaced by its decoded text.
fn raw_text(command: &str, escape: Escape) -> String {
    let mut text = String::with_capacity(command.len());
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        match (escape, c) {
            (Escape::Backslash, '\\') | (Escape::Backtick, '`') if chars.peek() == Some(&'\n') => {
                chars.next();
            }
            (Escape::Backslash, '$') if chars.peek() == Some(&'\'') => {
                chars.next();
                while let Some(inner) = chars.next() {
                    match inner {
                        '\'' => break,
                        '\\' => text.extend(ansi_c_char(&mut chars)),
                        other => text.push(other),
                    }
                }
            }
            _ => text.push(c),
        }
    }
    text
}

/// What a `stetdoc` segment does to held files (stetdoc 0.9.0's `--help`, from the app thread
/// for U13; check again when U13's 0.9.1 pin lands).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Stetdoc<'a> {
    /// Not `stetdoc`, or a form that writes no held file.
    Fine,
    /// `stetdoc mcp` or `stetdoc init`: refused in the agent's shell outright.
    Serve,
    /// A form that writes this held file.
    Writes(&'a Held),
    /// A subcommand or flag the list does not know, naming a held file.
    Unknown,
}

/// What the segment `words` does if it runs `stetdoc` ([`Stetdoc`]):
///
/// - `view`, `inspect`, `check`, `lookup`, `capabilities`, `skills`: read only, whatever the flags.
/// - `unpack H [-o DIR] [--refresh]`: writes `DIR` (default `NAME.stet`).
/// - `edit|replace|comment H …`: with `--dry-run` nothing; with `--in-place` `H`; with `-o OUT`
///   or `--output OUT` `OUT`; else `NAME.edited.docx` beside `H`.
/// - `compare ORIGINAL REVISED [-o OUT]`: writes `OUT`; the two it reads may be held.
/// - `pack DIR [-o OUT] [--force] [--done]`: writes `OUT`, default `NAME.edited.docx` from
///   `DIR`'s name (`NAME.stet`).
/// - `mcp`, `init`: serve or install, refused.
///
/// `--json` may appear anywhere. Any other word naming a held file, and any unknown subcommand
/// naming one, is [`Stetdoc::Unknown`].
pub(super) fn stetdoc<'a>(words: &[String], held: &'a [Held]) -> Stetdoc<'a> {
    let Some((program, args)) = super::shell::command_words(words).split_first() else {
        return Stetdoc::Fine;
    };
    if program_name(program) != "stetdoc" {
        return Stetdoc::Fine;
    }
    let args: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|arg| *arg != "--json")
        .collect();
    let Some((sub, rest)) = args.split_first() else {
        return Stetdoc::Fine;
    };
    let names_held = |word: &str| named(word, held);
    let output = |rest: &[&str]| -> Option<String> {
        rest.iter().enumerate().find_map(|(at, arg)| match *arg {
            "-o" | "--output" => rest.get(at + 1).map(|out| (*out).to_owned()),
            _ => arg
                .strip_prefix("--output=")
                .or_else(|| arg.strip_prefix("-o").filter(|out| !out.is_empty()))
                .map(str::to_owned),
        })
    };
    // Words besides the read documents (the first `reads` positionals) and the given flags
    // with their values must not name a held file.
    let others_unknown = |reads: usize, flags: &[&str], valued: &[&str]| {
        let mut positional = 0;
        let mut skip = false;
        rest.iter().any(|arg| {
            if skip {
                skip = false;
                return false;
            }
            if valued.contains(arg) {
                skip = true;
                return false;
            }
            if flags.contains(arg) || arg.starts_with("--output=") {
                return false;
            }
            if !arg.starts_with('-') {
                positional += 1;
                if positional <= reads {
                    return false;
                }
            }
            names_held(arg).is_some()
        })
    };
    let edited = |source: &str, strip: &str| {
        let last = source.rsplit(['/', '\\']).next().unwrap_or(source);
        let stem = last
            .strip_suffix(strip)
            .unwrap_or_else(|| last.rsplit_once('.').map_or(last, |(stem, _)| stem));
        format!("{stem}.edited.docx")
    };
    let writes = |out: &str| names_held(out).map_or(Stetdoc::Fine, Stetdoc::Writes);
    let first = rest.iter().find(|arg| !arg.starts_with('-')).copied();
    match *sub {
        "view" | "inspect" | "check" | "lookup" | "capabilities" | "skills" => Stetdoc::Fine,
        "mcp" | "init" => Stetdoc::Serve,
        "unpack" => {
            if others_unknown(1, &["--refresh"], &["-o", "--output"]) {
                return Stetdoc::Unknown;
            }
            output(rest).map_or(Stetdoc::Fine, |out| writes(&out))
        }
        "edit" | "replace" | "comment" => {
            if rest.contains(&"--dry-run") {
                return Stetdoc::Fine;
            }
            if let Some(out) = output(rest) {
                return writes(&out);
            }
            if rest.contains(&"--in-place") {
                return first.map_or(Stetdoc::Fine, writes);
            }
            first.map_or(Stetdoc::Fine, |source| writes(&edited(source, ".docx")))
        }
        "compare" => {
            if others_unknown(2, &[], &["-o", "--output"]) {
                return Stetdoc::Unknown;
            }
            output(rest).map_or(Stetdoc::Fine, |out| writes(&out))
        }
        "pack" => {
            if others_unknown(1, &["--force", "--done"], &["-o", "--output"]) {
                return Stetdoc::Unknown;
            }
            match output(rest) {
                Some(out) => writes(&out),
                None => first.map_or(Stetdoc::Fine, |dir| {
                    writes(&edited(dir.trim_end_matches(['/', '\\']), ".stet"))
                }),
            }
        }
        _ => {
            if args.iter().any(|arg| names_held(arg).is_some()) {
                Stetdoc::Unknown
            } else {
                Stetdoc::Fine
            }
        }
    }
}

/// A program word's name: its last path part, lower case, without `.exe`.
fn program_name(program: &str) -> String {
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();
    name.strip_suffix(".exe").map(str::to_owned).unwrap_or(name)
}

/// The segments of `command`, up to [`MAX_READ_WORDS`] words, and whether they are all of it.
fn segments_of(command: &str, escape: Escape) -> (Segments, bool) {
    let mut segments = Segments::new(MAX_READ_WORDS);
    shell_segments(command, escape, &mut segments);
    let whole = !segments.full();
    (segments, whole)
}

/// Whether the shell command `command`, which names some of `held`, only reads them, judged
/// conservatively (when in doubt, it does not):
///
/// - No command substitution or process substitution (`$(`, `` ` `` under POSIX shells, `<(`,
///   `>(`), and under PowerShell no script block (`{`).
/// - Every segment that names a held file is a command (not a heredoc body) whose program, past
///   prefixes such as `time` and `NAME=value`, is `cat`, `head`, `tail`, `wc`, `file`,
///   `strings`, `less` (without a log file option), `unzip` with only `-l`, `-p`, `-v`, `-lv`,
///   `-q`, `-qq` options, `textutil` with `-stdout` and no `-output`, `pandoc` whose `-o` or
///   `--output` (if any) names no held file, `stetdoc` in a form that writes no held file
///   ([`stetdoc`]), or `sh`/`bash`/`zsh`/`dash`/`ksh -c` whose script is itself read-only.
/// - No segment redirects output to a held file or to a target built at run time (`$`, a glob,
///   `~`); `/dev/null`, `$null` and a duplication such as `2>&1` are fine.
pub(super) fn read_only(command: &str, escape: Escape, held: &[Held]) -> bool {
    read_only_at(command, escape, held, 1)
}

fn read_only_at(command: &str, escape: Escape, held: &[Held], depth: usize) -> bool {
    let substitutes = ["$(", "<(", ">("].iter().any(|s| command.contains(s))
        || (escape == Escape::Backslash && command.contains('`'))
        || (escape == Escape::Backtick && command.contains('{'));
    if substitutes {
        return false;
    }
    let (segments, whole) = segments_of(command, escape);
    whole
        && segments.iter().all(|segment| {
            redirects_ok(segment, held)
                && (!segment.words.iter().any(|word| named(word, held).is_some())
                    || (segment.command && reader(&segment.words, escape, held, depth)))
        })
}

/// Whether `segment`'s output redirections write no held file and no target built at run time.
fn redirects_ok(segment: &Segment, held: &[Held]) -> bool {
    segment.redirects.iter().all(|target| {
        target.starts_with('&')
            || target == "/dev/null"
            || target.eq_ignore_ascii_case("$null")
            || (named(target, held).is_none()
                && !target.contains(['$', '`', '*', '?', '[', '{', '~']))
    })
}

/// Whether the command `words`, which names a held file, is one of [`read_only`]'s readers.
fn reader(words: &[String], escape: Escape, held: &[Held], depth: usize) -> bool {
    let Some((program, args)) = super::shell::command_words(words).split_first() else {
        return false;
    };
    match program_name(program).as_str() {
        "cat" | "head" | "tail" | "wc" | "file" | "strings" => true,
        "less" => !args.iter().any(|arg| {
            arg.starts_with("-o")
                || arg.starts_with("-O")
                || arg.starts_with("--log-file")
                || arg.starts_with("--LOG-FILE")
        }),
        "unzip" => {
            let options: Vec<&String> = args.iter().filter(|arg| arg.starts_with('-')).collect();
            options
                .iter()
                .any(|arg| matches!(arg.as_str(), "-l" | "-p" | "-v" | "-lv"))
                && options
                    .iter()
                    .all(|arg| matches!(arg.as_str(), "-l" | "-p" | "-v" | "-lv" | "-q" | "-qq"))
        }
        "textutil" => {
            args.iter().any(|arg| arg == "-stdout") && !args.iter().any(|arg| arg == "-output")
        }
        "pandoc" => !args.iter().enumerate().any(|(at, arg)| {
            let out = match arg.as_str() {
                "-o" | "--output" => args.get(at + 1).map(String::as_str),
                _ => arg
                    .strip_prefix("--output=")
                    .or_else(|| arg.strip_prefix("-o").filter(|out| !out.is_empty())),
            };
            out.is_some_and(|out| named(out, held).is_some())
        }),
        "stetdoc" => stetdoc(words, held) == Stetdoc::Fine,
        "sh" | "bash" | "zsh" | "dash" | "ksh" if depth > 0 => {
            script(words, escape).is_some_and(|(inner, inner_escape)| {
                inner_escape == Escape::Backslash
                    && read_only_at(&inner, inner_escape, held, depth - 1)
            })
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests;
