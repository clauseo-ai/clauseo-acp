//! What a delete or a move takes from the matter, read the same way for every caller: the paths
//! `rm`, `rmdir` and `unlink` delete, and PowerShell's `Remove-Item` with its aliases (`del`,
//! `erase`, `ri`, `rd`, `rm`, `rmdir`); and a move's SOURCES apart from its DESTINATION: `mv`,
//! `git mv`, PowerShell's `Move-Item` (`mi`, `move`, `mv`) and `Rename-Item` (`rni`, `ren`), and
//! `cmd /c` `move`, `ren`, `del`, `erase`, `rd`. A file a turn's shell deletes or moves away is
//! the agent's change, as one it writes is (`clauseo_matters::Matter::after_turn_named` credits a
//! deleted or renamed file named here, or under a folder named here).
//!
//! Options are left out (`-rf`, `-Force`, `-Recurse`, `cmd`'s `/Y`), and so is the value of an
//! option that takes a pattern or a setting rather than a path (`-Filter`, `-Include`,
//! `-Exclude`, `-Credential`, `-Stream`; `mv -S`/`--suffix`); after `--` every word is a path.
//! A move's destination is its last path (`mv a b`), the folder `-t DIR` names (each source's
//! name in it), PowerShell's `-Destination` or second position, or, for a rename, the new name
//! beside its source (`Rename-Item Drafts\a.docx b.docx` is `Drafts\b.docx`). A destination
//! ending in a separator, or one that takes several sources, is a folder: each source's name in
//! it. A target that is unclear (an expansion, a glob, an array) is the caller's to drop.

use super::Dialect;

/// The deleting programs, lower case, without `.exe`: every path they name goes.
const DELETERS: [&str; 8] = [
    "rm",
    "unlink",
    "remove-item",
    "del",
    "erase",
    "ri",
    "rd",
    "rmdir",
];

/// Options whose value is no path.
const VALUED: [&str; 7] = [
    "-filter",
    "-include",
    "-exclude",
    "-credential",
    "-stream",
    "-s",
    "--suffix",
];

/// What one command deletes and moves, as indexes into its arguments.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Taken {
    /// The paths it deletes.
    pub(crate) deleted: Vec<usize>,
    /// The paths it moves away (a move's or a rename's sources).
    pub(crate) sources: Vec<usize>,
    /// Where they go, when the command says.
    pub(crate) to: Option<To>,
}

/// Where a move puts its sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum To {
    /// The path at this index: the moved file's new path (or a folder the rule cannot tell).
    Path(usize),
    /// The folder at this index: each source goes in it under its own name.
    Folder(usize),
    /// The new name at this index, beside each source.
    Beside(usize),
}

/// `program`'s last part, lower case, without `.exe`.
fn name_of(program: &str) -> String {
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();
    name.strip_suffix(".exe")
        .map_or_else(|| name.clone(), str::to_owned)
}

/// What `program` with `args`, read in `dialect`, deletes and moves.
pub(crate) fn taken<W: AsRef<str>>(program: &str, args: &[W], dialect: Dialect) -> Taken {
    let name = name_of(program);
    let texts: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
    let powershell = dialect == Dialect::PowerShell;
    match name.as_str() {
        "git" if texts.first() == Some(&"mv") => {
            offset(posix_move(texts.get(1..).unwrap_or_default()), 1)
        }
        "cmd" => {
            let Some(at) = texts
                .iter()
                .position(|arg| matches!(arg.to_ascii_lowercase().as_str(), "/c" | "/k"))
            else {
                return Taken::default();
            };
            let (Some(inner), Some(rest)) = (texts.get(at + 1), texts.get(at + 2..)) else {
                return Taken::default();
            };
            offset(cmd(&name_of(inner), rest), at + 2)
        }
        "move-item" | "mi" => cmdlet_move(&texts, false),
        "rename-item" | "rni" => cmdlet_move(&texts, true),
        "mv" | "move" if powershell => cmdlet_move(&texts, false),
        "ren" if powershell => cmdlet_move(&texts, true),
        "mv" => posix_move(&texts),
        _ if DELETERS.contains(&name.as_str()) => Taken {
            deleted: paths(&texts),
            ..Taken::default()
        },
        _ => Taken::default(),
    }
}

/// `taken` with every index moved on by `by` (the words before the inner command).
fn offset(taken: Taken, by: usize) -> Taken {
    let shift = |to: To| match to {
        To::Path(at) => To::Path(at + by),
        To::Folder(at) => To::Folder(at + by),
        To::Beside(at) => To::Beside(at + by),
    };
    Taken {
        deleted: taken.deleted.into_iter().map(|at| at + by).collect(),
        sources: taken.sources.into_iter().map(|at| at + by).collect(),
        to: taken.to.map(shift),
    }
}

/// The indexes of the paths among `args`: no option, no option's value ([`VALUED`]), every word
/// after `--`.
fn paths(args: &[&str]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut options = true;
    let mut skip = false;
    for (i, arg) in args.iter().enumerate() {
        if std::mem::take(&mut skip) {
            continue;
        }
        if options && *arg == "--" {
            options = false;
        } else if options && arg.starts_with('-') && arg.len() > 1 {
            skip = VALUED.contains(&arg.to_ascii_lowercase().as_str());
        } else {
            out.push(i);
        }
    }
    out
}

/// A POSIX `mv` (or `git mv`): the last path the destination, unless `-t DIR` names the folder.
fn posix_move(args: &[&str]) -> Taken {
    let target = args.iter().enumerate().find_map(|(i, arg)| {
        if *arg == "-t" || *arg == "--target-directory" {
            Some((i, i + 1))
        } else if arg.starts_with("-t") || arg.starts_with("--target-directory=") {
            Some((i, i))
        } else {
            None
        }
    });
    let mut found = paths(args);
    if let Some((flag, value)) = target {
        // `-tDIR`, `--target-directory=DIR`: the folder is part of the option's word, which
        // the caller cannot split, so the destination stays unknown.
        found.retain(|at| *at != value && *at != flag);
        let to = (value != flag).then_some(To::Folder(value));
        return Taken {
            sources: found,
            to,
            ..Taken::default()
        };
    }
    let Some(last) = found.pop() else {
        return Taken::default();
    };
    if found.is_empty() {
        return Taken::default();
    }
    let into = args
        .iter()
        .any(|arg| *arg == "-T" || *arg == "--no-target-directory");
    let folder = !into && (found.len() > 1 || ends_in_separator(args, last));
    Taken {
        to: Some(if folder {
            To::Folder(last)
        } else {
            To::Path(last)
        }),
        sources: found,
        ..Taken::default()
    }
}

/// Whether `args[at]` ends in a folder separator.
fn ends_in_separator(args: &[&str], at: usize) -> bool {
    args.get(at).is_some_and(|arg| arg.ends_with(['/', '\\']))
}

/// `cmd /c`'s `move` and `ren` (`/Y`-style options left out), and its deletes.
fn cmd(name: &str, args: &[&str]) -> Taken {
    let found: Vec<usize> = args
        .iter()
        .enumerate()
        .filter(|(_, arg)| !(arg.starts_with('/') && arg.len() <= 3))
        .map(|(i, _)| i)
        .collect();
    let (sources, last) = match found.split_last() {
        Some((last, sources)) if !sources.is_empty() => (sources.to_vec(), *last),
        _ if matches!(name, "del" | "erase" | "rd" | "rmdir") => {
            return Taken {
                deleted: found,
                ..Taken::default()
            };
        }
        _ => return Taken::default(),
    };
    match name {
        "move" => Taken {
            to: Some(if sources.len() > 1 || ends_in_separator(args, last) {
                To::Folder(last)
            } else {
                To::Path(last)
            }),
            sources,
            ..Taken::default()
        },
        "ren" | "rename" => Taken {
            to: Some(To::Beside(last)),
            sources,
            ..Taken::default()
        },
        "del" | "erase" | "rd" | "rmdir" => Taken {
            deleted: found,
            ..Taken::default()
        },
        _ => Taken::default(),
    }
}

/// The slot a `Move-Item` or `Rename-Item` parameter fills.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Path,
    To,
    /// A parameter whose value is no path.
    Valued,
    /// A switch (`-Force`, `-PassThru`).
    Switch,
}

/// The slot of the parameter `name` (lower case, no `-`), for a rename (`-NewName`) or a move
/// (`-Destination`), as PowerShell takes a unique prefix.
fn slot(name: &str, rename: bool) -> Slot {
    let prefix = |whole: &str, least: usize| name.len() >= least && whole.starts_with(name);
    if prefix("literalpath", 1) || prefix("path", 3) || name == "lp" || name == "pspath" {
        Slot::Path
    } else if (rename && prefix("newname", 1)) || (!rename && prefix("destination", 3)) {
        Slot::To
    } else if ["filter", "include", "exclude", "credential"]
        .iter()
        .any(|whole| prefix(whole, 2))
    {
        Slot::Valued
    } else {
        Slot::Switch
    }
}

/// PowerShell's `Move-Item` (or `Rename-Item`, `rename`): `-Path`/`-LiteralPath` or the first
/// position, `-Destination`/`-NewName` or the second, `-Name:value` too.
fn cmdlet_move(args: &[&str], rename: bool) -> Taken {
    let mut sources = Vec::new();
    let mut to = None;
    // Whether `-Path` or `-Destination` was named, which takes its position away.
    let (mut path_named, mut to_named) = (false, false);
    let mut positional = Vec::new();
    let mut words = args.iter().enumerate();
    while let Some((i, arg)) = words.next() {
        let Some(name) = arg.strip_prefix('-').filter(|name| !name.is_empty()) else {
            positional.push(i);
            continue;
        };
        // `-Path:x` holds its value: the caller cannot split it, so it stays unknown.
        let (name, inline) = match name.split_once(':') {
            Some((name, _)) => (name, true),
            None => (name, false),
        };
        let slot = slot(&name.to_ascii_lowercase(), rename);
        if slot == Slot::Switch {
            continue;
        }
        let value = if inline {
            None
        } else {
            words.next().map(|(at, _)| at)
        };
        match slot {
            Slot::Path => {
                path_named = true;
                sources.extend(value);
            }
            Slot::To => {
                to_named = true;
                to = to.or(value);
            }
            Slot::Valued | Slot::Switch => {}
        }
    }
    let mut open = positional.into_iter();
    if !path_named {
        sources.extend(open.next());
    }
    if !to_named {
        to = open.next();
    }
    let to = to.map(|at| {
        if rename {
            To::Beside(at)
        } else if ends_in_separator(args, at) {
            To::Folder(at)
        } else {
            To::Path(at)
        }
    });
    Taken {
        sources,
        to,
        ..Taken::default()
    }
}
