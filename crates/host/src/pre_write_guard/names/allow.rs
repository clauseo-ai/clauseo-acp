//! The read-only exemption of the name backstop ([`read_only`]): narrow, whole-command, and when
//! in doubt not read-only (review round 3).

use super::{Held, MAX_READ_WORDS, Stetdoc, named, program_name, stetdoc};
use crate::pre_write_guard::shell::command_words;
use crate::pre_write_guard::words::{Escape, Segment, Segments, shell_segments};

/// Whether the shell command `command`, which names some of `held` (by name, or by the words in
/// `hits`, which the path layer found resolving to a held file through a symlink, another case or
/// another Unicode form), only reads them. Every part of it must pass:
///
/// - The text has no command or process substitution (`$(`, `` ` `` under POSIX shells, `<(`,
///   `>(`), no `$_`, no function definition (`name()`; `function` is not on the list), and under
///   PowerShell no script block (`{`).
/// - Every simple command (a heredoc's body is data) is, by a bare program name (no `/` or `\`,
///   no `NAME=value` before it; `time`, `command`, `!`, `{`, `then` and the like may come
///   first), one of:
///   - a reader, which may name a held file: `cat`, `head`, `tail`, `wc`, `strings`, `file`
///     (not `-C`), `unzip` with only `-l`, `-p`, `-v`, `-lv`, `-q`, `-qq`, `textutil` with
///     `-stdout` and no `-output`, `pandoc` with no option that writes or runs code but a
///     literal `-o` naming no held file ([`pandoc_reads`]), `stetdoc` in a form writing no held
///     file ([`stetdoc()`]), `sh`/`bash`/`zsh`/`dash`/`ksh -c` whose script passes this rule and
///     whose other words name no held file;
///   - a filter, which must not name a held file: `grep`, `rg` (no `--pre`), `sed` (no `-i`,
///     `--in-place`), `sort` (no `-o`, `--output`, `--compress-program`), `uniq` (one file at
///     most), `cut`, `tr`, `nl`, `column`;
///   - neutral, which must not name a held file either: `cd`, `pushd`, `popd`, `echo`, `pwd`,
///     `ls`, `true`.
/// - Every output redirection is `/dev/null`, `$null`, a duplication (`2>&1`, `>&2`, `>&-`), or
///   a literal path ([`literal`]) naming no held file; never `>&file`.
pub(in crate::pre_write_guard) fn read_only(
    command: &str,
    escape: Escape,
    held: &[Held],
    hits: &[String],
) -> bool {
    Check { held, hits }.command(command, escape, 1)
}

/// What [`read_only`] checks against.
struct Check<'a> {
    held: &'a [Held],
    hits: &'a [String],
}

impl Check<'_> {
    /// Whether `word` names a held file, by name or as a path-layer hit.
    fn names(&self, word: &str) -> bool {
        named(word, self.held).is_some() || self.hits.iter().any(|hit| hit == word)
    }

    /// Whether `target`, written to, writes no held file: a literal path naming none.
    fn writable(&self, target: &str) -> bool {
        literal(target) && !self.names(target)
    }

    fn command(&self, command: &str, escape: Escape, depth: usize) -> bool {
        if !plain(command, escape) {
            return false;
        }
        let mut segments = Segments::new(MAX_READ_WORDS);
        shell_segments(command, escape, &mut segments);
        !segments.full()
            && segments
                .iter()
                .all(|segment| !segment.command || self.segment(segment, depth))
    }

    /// Whether the simple command `segment` is a reader, filter or neutral command of
    /// [`read_only`] with its redirections fine.
    fn segment(&self, segment: &Segment, depth: usize) -> bool {
        let words = &segment.words;
        let rest = command_words(words);
        let prefix = words.get(..words.len() - rest.len()).unwrap_or_default();
        if prefix.iter().any(|word| word.contains('=')) || !self.redirects_ok(segment) {
            return false;
        }
        let Some((program, args)) = rest.split_first() else {
            return true;
        };
        if program.contains(['/', '\\']) {
            return false;
        }
        let free = || !words.iter().any(|word| self.names(word));
        match program_name(program).as_str() {
            "cat" | "head" | "tail" | "wc" | "strings" => true,
            "file" => !args
                .iter()
                .any(|arg| short_has(arg, 'C') || long_prefix(arg, "compile")),
            "unzip" => {
                let options: Vec<&String> =
                    args.iter().filter(|arg| arg.starts_with('-')).collect();
                options
                    .iter()
                    .any(|arg| matches!(arg.as_str(), "-l" | "-p" | "-v" | "-lv"))
                    && options.iter().all(|arg| {
                        matches!(arg.as_str(), "-l" | "-p" | "-v" | "-lv" | "-q" | "-qq")
                    })
            }
            "textutil" => {
                args.iter().any(|arg| arg == "-stdout") && !args.iter().any(|arg| arg == "-output")
            }
            "pandoc" => pandoc_reads(args, |out| self.writable(out)),
            "stetdoc" => stetdoc(words, self.held, self.hits) == Stetdoc::Fine,
            "sh" | "bash" | "zsh" | "dash" | "ksh" if depth > 0 => {
                let Some(at) = args.iter().position(|arg| {
                    arg.strip_prefix('-')
                        .is_some_and(|flags| !flags.starts_with('-') && flags.contains('c'))
                }) else {
                    return false;
                };
                let Some(script) = args.get(at + 1) else {
                    return false;
                };
                let others_name_held = args
                    .iter()
                    .enumerate()
                    .any(|(index, word)| index != at + 1 && self.names(word));
                !others_name_held && self.command(script, Escape::Backslash, depth - 1)
            }
            // Filters, then the neutral commands.
            "grep" | "cut" | "tr" | "nl" | "column" | "cd" | "pushd" | "popd" | "echo" | "pwd"
            | "ls" | "true" => free(),
            "rg" => {
                free()
                    && !args
                        .iter()
                        .any(|arg| arg == "--pre" || arg.starts_with("--pre="))
            }
            "sed" => {
                free()
                    && !args
                        .iter()
                        .any(|arg| short_has(arg, 'i') || long_prefix(arg, "in-place"))
            }
            "sort" => {
                free()
                    && !args.iter().any(|arg| {
                        short_has(arg, 'o')
                            || long_prefix(arg, "output")
                            || long_prefix(arg, "compress-program")
                    })
            }
            "uniq" => {
                free()
                    && args
                        .iter()
                        .filter(|arg| *arg == "-" || !arg.starts_with('-'))
                        .count()
                        <= 1
            }
            _ => false,
        }
    }

    /// Whether `segment`'s output redirections write no held file: each is `/dev/null`, `$null`,
    /// a duplication onto a descriptor (`&1`, `&-`: `2>&1`, `>&2`, `>&-`), or [`Self::writable`].
    /// `>&file` (the scanner's `&file`) writes `file` in bash, so it is refused.
    fn redirects_ok(&self, segment: &Segment) -> bool {
        segment.redirects.iter().all(|target| {
            target == "/dev/null"
                || target.eq_ignore_ascii_case("$null")
                || target.strip_prefix('&').map_or_else(
                    || self.writable(target),
                    |fd| fd == "-" || (!fd.is_empty() && fd.bytes().all(|b| b.is_ascii_digit())),
                )
        })
    }
}

/// Whether `command`'s text has nothing [`read_only`] refuses outright: a command or process
/// substitution, `$_`, a function definition, or under PowerShell a script block.
fn plain(command: &str, escape: Escape) -> bool {
    let refused = ["$(", "<(", ">(", "$_", "${_"]
        .iter()
        .any(|text| command.contains(text))
        || (escape == Escape::Backslash && command.contains('`'))
        || (escape == Escape::Backtick && command.contains('{'));
    let defines = command.match_indices('(').any(|(at, _)| {
        command
            .get(at + 1..)
            .is_some_and(|rest| rest.trim_start().starts_with(')'))
    });
    !refused && !defines
}

/// Whether `word` is a path the shell takes as written: not empty, no expansion (`$`, `` ` ``,
/// a glob, a brace, a leading `~`).
fn literal(word: &str) -> bool {
    !word.is_empty() && !word.starts_with('~') && !word.contains(['$', '`', '*', '?', '[', '{'])
}

/// Whether `arg` is a group of short options (`-xyz`, not `--`) holding `flag`.
fn short_has(arg: &str, flag: char) -> bool {
    arg.strip_prefix('-')
        .is_some_and(|flags| !flags.starts_with('-') && flags.contains(flag))
}

/// Whether `arg` is a long option (`--name` or `--name=value`) whose name is `option` or a
/// prefix of it, as getopt-style parsers accept.
fn long_prefix(arg: &str, option: &str) -> bool {
    arg.strip_prefix("--")
        .map(|long| long.split('=').next().unwrap_or(long))
        .is_some_and(|name| !name.is_empty() && option.starts_with(name))
}

/// The pandoc options that write a file or run code; Haskell's `GetOpt` takes any prefix of a
/// long option.
const PANDOC_REFUSED: [&str; 8] = [
    "extract-media",
    "filter",
    "lua-filter",
    "defaults",
    "data-dir",
    "log",
    "pdf-engine",
    "pdf-engine-opt",
];

/// Whether pandoc with `args` only reads and writes stdout or a file `writable` allows: no
/// option ([`PANDOC_REFUSED`], any prefix) that writes or runs code, no short group with `F`,
/// `L` or `d`; each output (`--output`, any prefix, `=value` or the next word; a short group
/// with `o`, the rest of the group or the next word) `writable`. `--` ends the options.
fn pandoc_reads(args: &[String], writable: impl Fn(&str) -> bool) -> bool {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            break;
        }
        let output = if let Some(long) = arg.strip_prefix("--") {
            let (name, value) = long.split_once('=').unwrap_or((long, ""));
            if PANDOC_REFUSED.iter().any(|option| long_prefix(arg, option)) {
                return false;
            }
            "output".starts_with(name).then_some(value)
        } else if let Some(flags) = arg.strip_prefix('-').filter(|flags| !flags.is_empty()) {
            if flags.contains(['F', 'L', 'd']) {
                return false;
            }
            flags.split_once('o').map(|(_, value)| value)
        } else {
            None
        };
        let Some(value) = output else {
            continue;
        };
        let value = if value.is_empty() {
            iter.next().map_or("", String::as_str)
        } else {
            value
        };
        if !value.is_empty() && !writable(value) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests;
