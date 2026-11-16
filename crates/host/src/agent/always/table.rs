//! The programs whose commands "Always for this matter" may keep (decision record 0063,
//! "Commands"): an allowlist. A program not here, an option a program's entry does not list, or
//! a word in a form the entry does not know makes the command one the lawyer is asked about
//! each time. Nothing is refused by being named: `find -exec`, `rg --pre`, `sort
//! --compress-program` or `pandoc --lua-filter` are out because no entry lets them in.
//!
//! Each entry says where the program may be (`At`), which options it takes and what their
//! values are, and what its other words are (`Word`). A `Word::Path` word is a file the
//! command reads or writes: the host checks, at keep time and again at every use, that it lies
//! inside the matter by its real path (`reach.rs`). A `Word::Text` word is data the program
//! never opens as a file; a `Word::Number` word is digits.
//!
//! The list comes from the commands that raised cards in recorded runs (2026-11-16 census) and
//! the plain commands beside them that read or move a matter's own files. Commands that run
//! code (`python3`, `swift`, `osascript`), reach the network (`curl`) or drive a browser
//! (`agent-browser eval`) are not here, and never will be.

use super::plain::Grammar;

/// What a word of a command is to the program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Word {
    /// A file or folder the program opens: checked to be inside the matter.
    Path,
    /// Data the program never opens as a file (a pattern, a metadata key).
    Text,
    /// ASCII digits, with an optional leading `+`.
    Number,
}

/// An option a program may take, by its exact spelling (in any case for a PowerShell cmdlet),
/// with the value it takes, if any: the next word, or after `=` for a `--long` option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Opt {
    pub(crate) name: &'static str,
    pub(crate) value: Option<Word>,
}

/// Where a program may be, by its real path, at keep time and at every use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum At {
    /// A system program: found on the agent's search path, in one of these folders.
    System {
        unix: &'static [&'static str],
        windows: &'static [&'static str],
    },
    /// One of Clauseo's own tools: found on the agent's search path, in one of the folders
    /// Clauseo puts first on it (`shell_path::SessionPath::dirs`).
    Bundled,
    /// A PowerShell cmdlet: part of the shell, no file.
    Cmdlet,
}

/// A program whose commands may be kept.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Spec {
    /// Its name as typed: no folder, no `.exe`.
    pub(crate) name: &'static str,
    pub(crate) grammar: Grammar,
    pub(crate) at: At,
    /// The options it may take.
    pub(crate) options: &'static [Opt],
    /// Its one-letter flags that take no value; each may be given alone (`-l`) or joined
    /// (`-la`).
    pub(crate) flags: &'static str,
    /// When not empty, its first word past the options is one of these.
    pub(crate) subcommands: &'static [&'static str],
    /// What its first operand is, when it differs from the rest (`grep`'s pattern).
    pub(crate) lead: Option<Word>,
    /// What its operands are.
    pub(crate) operands: Word,
    /// The fewest and the most operands, not counting a subcommand.
    pub(crate) count: (usize, usize),
}

/// The macOS and Linux system folders, and Git for Windows' (Claude's `Bash` on Windows).
const SYSTEM: At = At::System {
    unix: &["/bin", "/usr/bin"],
    windows: &["C:/Program Files/Git/usr/bin"],
};

/// macOS' own tools, in `/usr/bin` only.
const MAC_ONLY: At = At::System {
    unix: &["/usr/bin"],
    windows: &[],
};

const fn opt(name: &'static str, value: Option<Word>) -> Opt {
    Opt { name, value }
}

const NUMBER: Option<Word> = Some(Word::Number);
const PATH: Option<Word> = Some(Word::Path);
const TEXT: Option<Word> = Some(Word::Text);

/// The most operands any entry takes.
const MANY: usize = 64;

/// Every program a kept command may run.
pub(crate) const PROGRAMS: &[Spec] = &[
    // Read a file or a folder.
    Spec {
        name: "ls",
        grammar: Grammar::Posix,
        at: SYSTEM,
        options: &[],
        flags: "1aAdFhlrRSt",
        subcommands: &[],
        lead: None,
        operands: Word::Path,
        count: (0, MANY),
    },
    Spec {
        name: "cat",
        grammar: Grammar::Posix,
        at: SYSTEM,
        options: &[],
        flags: "bns",
        subcommands: &[],
        lead: None,
        operands: Word::Path,
        count: (1, MANY),
    },
    Spec {
        name: "head",
        grammar: Grammar::Posix,
        at: SYSTEM,
        options: &[opt("-n", NUMBER), opt("-c", NUMBER)],
        flags: "",
        subcommands: &[],
        lead: None,
        operands: Word::Path,
        count: (1, MANY),
    },
    Spec {
        name: "tail",
        grammar: Grammar::Posix,
        at: SYSTEM,
        options: &[opt("-n", NUMBER), opt("-c", NUMBER)],
        flags: "",
        subcommands: &[],
        lead: None,
        operands: Word::Path,
        count: (1, MANY),
    },
    Spec {
        name: "wc",
        grammar: Grammar::Posix,
        at: SYSTEM,
        options: &[],
        flags: "clmw",
        subcommands: &[],
        lead: None,
        operands: Word::Path,
        count: (1, MANY),
    },
    Spec {
        name: "file",
        grammar: Grammar::Posix,
        at: SYSTEM,
        options: &[],
        flags: "bI",
        subcommands: &[],
        lead: None,
        operands: Word::Path,
        count: (1, MANY),
    },
    Spec {
        name: "grep",
        grammar: Grammar::Posix,
        at: SYSTEM,
        options: &[
            opt("-m", NUMBER),
            opt("-A", NUMBER),
            opt("-B", NUMBER),
            opt("-C", NUMBER),
        ],
        flags: "cEFhHilnoqrsvwx",
        subcommands: &[],
        lead: Some(Word::Text),
        operands: Word::Path,
        count: (2, MANY),
    },
    // macOS Spotlight metadata of a file (the census' one "Always": `mdls -name … file.pdf`).
    Spec {
        name: "mdls",
        grammar: Grammar::Posix,
        at: MAC_ONLY,
        options: &[opt("-name", TEXT), opt("-raw", None)],
        flags: "",
        subcommands: &[],
        lead: None,
        operands: Word::Path,
        count: (1, MANY),
    },
    // Copy, move, make a folder: every operand inside the matter.
    Spec {
        name: "cp",
        grammar: Grammar::Posix,
        at: SYSTEM,
        options: &[],
        flags: "fnpRrv",
        subcommands: &[],
        lead: None,
        operands: Word::Path,
        count: (2, MANY),
    },
    Spec {
        name: "mv",
        grammar: Grammar::Posix,
        at: SYSTEM,
        options: &[],
        flags: "fnv",
        subcommands: &[],
        lead: None,
        operands: Word::Path,
        count: (2, MANY),
    },
    Spec {
        name: "mkdir",
        grammar: Grammar::Posix,
        at: SYSTEM,
        options: &[],
        flags: "pv",
        subcommands: &[],
        lead: None,
        operands: Word::Path,
        count: (1, MANY),
    },
    // Clauseo's document tool, its reading subcommands (`stetdoc_args::Form::Reads`).
    Spec {
        name: "stetdoc",
        grammar: Grammar::Posix,
        at: At::Bundled,
        options: &[
            opt("--help", None),
            opt("--revisions", None),
            opt("--comments", None),
        ],
        flags: "",
        subcommands: &["view", "inspect", "check"],
        lead: None,
        operands: Word::Path,
        count: (0, MANY),
    },
    // PowerShell (Claude's `PowerShell`, Codex on Windows).
    Spec {
        name: "Get-Content",
        grammar: Grammar::PowerShell,
        at: At::Cmdlet,
        options: &[
            opt("-Path", PATH),
            opt("-LiteralPath", PATH),
            opt("-TotalCount", NUMBER),
            opt("-Tail", NUMBER),
            opt("-Raw", None),
        ],
        flags: "",
        subcommands: &[],
        lead: None,
        operands: Word::Path,
        count: (0, 1),
    },
    Spec {
        name: "Get-ChildItem",
        grammar: Grammar::PowerShell,
        at: At::Cmdlet,
        options: &[
            opt("-Path", PATH),
            opt("-LiteralPath", PATH),
            opt("-File", None),
            opt("-Directory", None),
            opt("-Name", None),
            opt("-Force", None),
        ],
        flags: "",
        subcommands: &[],
        lead: None,
        operands: Word::Path,
        count: (0, 1),
    },
    Spec {
        name: "Get-Item",
        grammar: Grammar::PowerShell,
        at: At::Cmdlet,
        options: &[opt("-Path", PATH), opt("-LiteralPath", PATH)],
        flags: "",
        subcommands: &[],
        lead: None,
        operands: Word::Path,
        count: (0, 1),
    },
    Spec {
        name: "Test-Path",
        grammar: Grammar::PowerShell,
        at: At::Cmdlet,
        options: &[opt("-Path", PATH), opt("-LiteralPath", PATH)],
        flags: "",
        subcommands: &[],
        lead: None,
        operands: Word::Path,
        count: (0, 1),
    },
    Spec {
        name: "Select-String",
        grammar: Grammar::PowerShell,
        at: At::Cmdlet,
        options: &[
            opt("-Pattern", TEXT),
            opt("-Path", PATH),
            opt("-LiteralPath", PATH),
            opt("-SimpleMatch", None),
            opt("-CaseSensitive", None),
        ],
        flags: "",
        subcommands: &[],
        lead: Some(Word::Text),
        operands: Word::Path,
        count: (0, 2),
    },
    Spec {
        name: "Copy-Item",
        grammar: Grammar::PowerShell,
        at: At::Cmdlet,
        options: &[
            opt("-Path", PATH),
            opt("-LiteralPath", PATH),
            opt("-Destination", PATH),
            opt("-Force", None),
        ],
        flags: "",
        subcommands: &[],
        lead: None,
        operands: Word::Path,
        count: (0, 2),
    },
    Spec {
        name: "Move-Item",
        grammar: Grammar::PowerShell,
        at: At::Cmdlet,
        options: &[
            opt("-Path", PATH),
            opt("-LiteralPath", PATH),
            opt("-Destination", PATH),
            opt("-Force", None),
        ],
        flags: "",
        subcommands: &[],
        lead: None,
        operands: Word::Path,
        count: (0, 2),
    },
];

/// The entry for `program` as typed in `grammar`: exact for a POSIX program, in any case for
/// a cmdlet (PowerShell's own rule).
pub(crate) fn find(program: &str, grammar: Grammar) -> Option<&'static Spec> {
    PROGRAMS.iter().find(|spec| {
        spec.grammar == grammar
            && match grammar {
                Grammar::Posix => spec.name == program,
                Grammar::PowerShell => spec.name.eq_ignore_ascii_case(program),
            }
    })
}

/// A command's words read against its program's entry: what each word is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Read {
    /// The words the program opens as files, as typed.
    pub(crate) paths: Vec<String>,
}

/// Why a command's words do not fit its program's entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unfit {
    /// An option the entry does not list, in a form it does not know, or missing its value.
    Option,
    /// A word that is not a plain path, text or number where the entry wants one.
    Word,
    /// Too few or too many operands, or no known subcommand.
    Count,
}

/// `args` read against `spec`: each option one the entry lists, in a form it knows; each value
/// and operand of the kind the entry says. Fails closed on anything else.
pub(crate) fn read(spec: &Spec, args: &[String]) -> Result<Read, Unfit> {
    let mut paths = Vec::new();
    let mut operands: Vec<&String> = Vec::new();
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        at += 1;
        if !arg.starts_with('-') || arg == "-" {
            operands.push(arg);
            continue;
        }
        let same = |name: &str| match spec.grammar {
            Grammar::Posix => name == arg,
            Grammar::PowerShell => name.eq_ignore_ascii_case(arg),
        };
        if let Some(option) = spec.options.iter().find(|option| same(option.name)) {
            if let Some(kind) = option.value {
                let value = args.get(at).ok_or(Unfit::Option)?;
                at += 1;
                take(kind, value, &mut paths)?;
            }
            continue;
        }
        // `--name=value`, for a long option with a value.
        if let Some((name, value)) = arg.split_once('=')
            && name.starts_with("--")
            && spec.grammar == Grammar::Posix
        {
            let option = spec.options.iter().find(|option| option.name == name);
            let kind = option
                .and_then(|option| option.value)
                .ok_or(Unfit::Option)?;
            take(kind, value, &mut paths)?;
            continue;
        }
        // `-la`: one-letter flags that take no value, alone or joined.
        let letters = arg.strip_prefix('-').unwrap_or_default();
        let joined = spec.grammar == Grammar::Posix
            && !letters.is_empty()
            && letters
                .chars()
                .all(|c| c.is_ascii_alphanumeric() && spec.flags.contains(c));
        if !joined {
            return Err(Unfit::Option);
        }
    }
    let mut operands = operands.into_iter();
    if !spec.subcommands.is_empty() {
        // No subcommand only with `--help` alone.
        let help = args.len() == 1 && args.first().is_some_and(|arg| arg == "--help");
        let fits = match operands.next() {
            Some(sub) => spec.subcommands.contains(&sub.as_str()),
            None => help,
        };
        if !fits {
            return Err(Unfit::Count);
        }
    }
    let operands: Vec<&String> = operands.collect();
    if operands.len() < spec.count.0 || operands.len() > spec.count.1 {
        return Err(Unfit::Count);
    }
    for (index, operand) in operands.into_iter().enumerate() {
        let kind = match spec.lead {
            Some(lead) if index == 0 => lead,
            _ => spec.operands,
        };
        take(kind, operand, &mut paths)?;
    }
    Ok(Read { paths })
}

/// Checks `word` as a `kind`, and keeps it when it is a path.
fn take(kind: Word, word: &str, paths: &mut Vec<String>) -> Result<(), Unfit> {
    let fits = match kind {
        Word::Path => path_word(word),
        Word::Text => !word.is_empty() && !word.starts_with(['-', '=']),
        Word::Number => {
            let digits = word.strip_prefix('+').unwrap_or(word);
            !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
        }
    };
    if !fits {
        return Err(Unfit::Word);
    }
    if kind == Word::Path {
        paths.push(word.to_owned());
    }
    Ok(())
}

/// A path word as typed: not empty, not starting with `-` (an option) or `=`; no `=` (zsh's
/// `=prog`, a variable), no `:` but a leading drive letter's (`C:/…`; a PowerShell provider such
/// as `HKCU:` or `FileSystem::` has more), no `~`, no `\`, no `..` name. Whether it is inside
/// the matter is judged elsewhere (`plain::outside`, then by real path).
pub(crate) fn path_word(word: &str) -> bool {
    let rest = match word.as_bytes() {
        [drive, b':', b'/', ..] if drive.is_ascii_alphabetic() => word.get(2..).unwrap_or(""),
        _ => word,
    };
    !word.is_empty()
        && !word.starts_with(['-', '='])
        && !rest.contains([':', '=', '~', '\\'])
        && rest.split('/').all(|name| name != "..")
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;
