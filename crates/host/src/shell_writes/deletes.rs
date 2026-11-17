//! What a delete or a move takes, read the same way for every caller: the paths `rm`, `rmdir`
//! and `unlink` delete, and PowerShell's `Remove-Item` with its aliases (`del`, `erase`, `ri`,
//! `rd`, `rm`, `rmdir`); and a move's SOURCES apart from its DESTINATION: `mv`, `git mv`,
//! PowerShell's `Move-Item` (`mi`, `move`, `mv`) and `Rename-Item` (`rni`, `ren`), and `cmd /c`
//! or `/k` with `move`, `ren`, `rename`, `del`, `erase`, `rd` and `rmdir`.
//!
//! The paths are the words that are not options; after `--` every word is a path. A POSIX `mv`
//! reads `-t DIR`, `-tDIR` (in a cluster too: `-vt DIR`), `--target-directory DIR` and
//! `--target-directory=DIR` as the folder its sources go in, `-T` as naming the source's new
//! path, and `-S`/`--suffix` as taking a value. A cmdlet's parameters are bound as PowerShell
//! binds them ([`cmdlet`]): by name, unique prefix or alias, a value after a blank or after the
//! name's colon (`-Path:a.docx`), `-Path`/`-LiteralPath` and `-Destination`/`-NewName` as paths,
//! and positions for what is left. A move's destination is its last path (`mv a b`), the folder
//! `-t` names, PowerShell's `-Destination` or second position, or, for a rename, the new name
//! beside its source (`Rename-Item Drafts\a.docx b.docx` is `Drafts\b.docx`). A destination
//! ending in a separator, or one that takes several sources, is a folder: each source's name in
//! it.
//!
//! Each path is given as where it lies among the arguments ([`Arg`]): the caller reads the word
//! there, a PowerShell array (`a.docx, b.docx`) as each of its elements.

use super::Dialect;

mod cmdlet;

/// The programs that delete every path they name, in a POSIX shell (PowerShell's aliases are
/// `Remove-Item`'s, [`cmdlet`]), lower case, without `.exe`.
const DELETERS: [&str; 6] = ["rm", "unlink", "rmdir", "del", "erase", "rd"];

/// A path among a command's arguments: the argument at `at`, from its byte `from` on
/// (`-Path:a.docx` is its argument from byte 6, `-tArchive` from byte 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Arg {
    pub(crate) at: usize,
    pub(crate) from: usize,
}

impl Arg {
    /// The whole argument at `at`.
    pub(crate) const fn whole(at: usize) -> Self {
        Self { at, from: 0 }
    }
}

/// Whether a command deletes or moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Act {
    Delete,
    Move,
}

/// What one command deletes and moves, as places among its arguments.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Taken {
    /// Whether it deletes or moves (`None`: neither).
    pub(crate) act: Option<Act>,
    /// The paths it deletes.
    pub(crate) deleted: Vec<Arg>,
    /// The paths it moves away (a move's or a rename's sources).
    pub(crate) sources: Vec<Arg>,
    /// Where they go, when the command says.
    pub(crate) to: Option<To>,
    /// A parameter or option that names what it takes has no value after it (`-Path` last).
    pub(crate) unclear: bool,
    /// It names no path itself, so a cmdlet run on a pipeline's input takes the paths from it.
    pub(crate) input: bool,
}

/// Where a move puts its sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum To {
    /// This path: the moved file's new path, or a folder it goes in under its own name.
    Path(Arg),
    /// This path, the moved file's or folder's new path (`mv -T`).
    New(Arg),
    /// This folder: each source goes in it under its own name.
    Folder(Arg),
    /// This new name, beside each source.
    Beside(Arg),
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
        "remove-item" | "ri" => cmdlet::taken(&texts, cmdlet::Cmdlet::Remove),
        "move-item" | "mi" => cmdlet::taken(&texts, cmdlet::Cmdlet::Move),
        "rename-item" | "rni" => cmdlet::taken(&texts, cmdlet::Cmdlet::Rename),
        "rm" | "rmdir" | "del" | "erase" | "rd" if powershell => {
            cmdlet::taken(&texts, cmdlet::Cmdlet::Remove)
        }
        "mv" | "move" if powershell => cmdlet::taken(&texts, cmdlet::Cmdlet::Move),
        "ren" if powershell => cmdlet::taken(&texts, cmdlet::Cmdlet::Rename),
        "mv" => posix_move(&texts),
        _ if DELETERS.contains(&name.as_str()) => Taken {
            act: Some(Act::Delete),
            deleted: paths(&texts),
            ..Taken::default()
        },
        _ => Taken::default(),
    }
}

/// `taken` with every place moved on by `by` (the words before the inner command).
fn offset(taken: Taken, by: usize) -> Taken {
    let on = |arg: Arg| Arg {
        at: arg.at + by,
        from: arg.from,
    };
    let shift = |to: To| match to {
        To::Path(arg) => To::Path(on(arg)),
        To::New(arg) => To::New(on(arg)),
        To::Folder(arg) => To::Folder(on(arg)),
        To::Beside(arg) => To::Beside(on(arg)),
    };
    Taken {
        deleted: taken.deleted.into_iter().map(on).collect(),
        sources: taken.sources.into_iter().map(on).collect(),
        to: taken.to.map(shift),
        ..taken
    }
}

/// The paths among `args`: every word but an option (`-rf`), every word after `--`.
fn paths(args: &[&str]) -> Vec<Arg> {
    let mut out = Vec::new();
    let mut options = true;
    for (i, arg) in args.iter().enumerate() {
        if options && *arg == "--" {
            options = false;
        } else if !(options && arg.starts_with('-') && arg.len() > 1) {
            out.push(Arg::whole(i));
        }
    }
    out
}

/// What the next word of a POSIX `mv` is, after an option that takes it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Next {
    /// The folder the sources go in (`-t`).
    Folder,
    /// A value that is no path (`-S`).
    Value,
}

/// A POSIX `mv` (or `git mv`): the last path the destination, unless `-t` names the folder.
fn posix_move(args: &[&str]) -> Taken {
    let mut found = Vec::new();
    let mut folder = None;
    let mut no_folder = false;
    let mut options = true;
    let mut next = None;
    for (i, arg) in args.iter().enumerate() {
        if let Some(wanted) = next.take() {
            if wanted == Next::Folder {
                folder = Some(Arg::whole(i));
            }
        } else if !options || *arg == "-" || !arg.starts_with('-') {
            found.push(Arg::whole(i));
        } else if *arg == "--" {
            options = false;
        } else if let Some(long) = arg.strip_prefix("--") {
            // GNU takes a unique prefix of a long option's name.
            let (name, value) = long
                .split_once('=')
                .map_or((long, None), |(n, v)| (n, Some(v)));
            let is = |whole: &str, least: usize| name.len() >= least && whole.starts_with(name);
            if is("target-directory", 1) {
                match value {
                    Some(value) => {
                        folder = Some(Arg {
                            at: i,
                            from: arg.len() - value.len(),
                        });
                    }
                    None => next = Some(Next::Folder),
                }
            } else if is("suffix", 2) && value.is_none() {
                next = Some(Next::Value);
            } else if is("no-target-directory", 4) {
                no_folder = true;
            }
        } else {
            // A cluster of short options (`-fv`): `-t` and `-S` take the rest of the word as
            // their value, else the next word.
            for (at, c) in arg.char_indices().skip(1) {
                if c == 'T' {
                    no_folder = true;
                } else if c == 't' || c == 'S' {
                    let from = at + 1;
                    if from < arg.len() {
                        if c == 't' {
                            folder = Some(Arg { at: i, from });
                        }
                    } else {
                        next = Some(if c == 't' { Next::Folder } else { Next::Value });
                    }
                    break;
                }
            }
        }
    }
    let unclear = next == Some(Next::Folder);
    if let Some(dir) = folder {
        return Taken {
            act: Some(Act::Move),
            to: (!found.is_empty()).then_some(To::Folder(dir)),
            sources: found,
            unclear,
            ..Taken::default()
        };
    }
    let last = found.pop();
    let Some(last) = last.filter(|_| !found.is_empty()) else {
        return Taken {
            act: Some(Act::Move),
            unclear,
            ..Taken::default()
        };
    };
    let into_folder = !no_folder && (found.len() > 1 || ends_in_separator(args, last));
    Taken {
        act: Some(Act::Move),
        to: Some(if no_folder {
            To::New(last)
        } else if into_folder {
            To::Folder(last)
        } else {
            To::Path(last)
        }),
        sources: found,
        unclear,
        ..Taken::default()
    }
}

/// Whether the path at `arg` ends in a folder separator.
fn ends_in_separator(args: &[&str], arg: Arg) -> bool {
    args.get(arg.at)
        .and_then(|text| text.get(arg.from..))
        .is_some_and(|text| text.ends_with(['/', '\\']))
}

/// `cmd /c`'s `move`, `ren` and `rename` (`/Y`-style options left out), and its deletes.
fn cmd(name: &str, args: &[&str]) -> Taken {
    let found: Vec<Arg> = args
        .iter()
        .enumerate()
        .filter(|(_, arg)| !(arg.starts_with('/') && arg.len() <= 3))
        .map(|(i, _)| Arg::whole(i))
        .collect();
    if matches!(name, "del" | "erase" | "rd" | "rmdir") {
        return Taken {
            act: Some(Act::Delete),
            deleted: found,
            ..Taken::default()
        };
    }
    let rename = matches!(name, "ren" | "rename");
    if !rename && name != "move" {
        return Taken::default();
    }
    let Some((last, sources)) = found
        .split_last()
        .filter(|(_, sources)| !sources.is_empty())
    else {
        return Taken {
            act: Some(Act::Move),
            ..Taken::default()
        };
    };
    let to = if rename {
        To::Beside(*last)
    } else if sources.len() > 1 || ends_in_separator(args, *last) {
        To::Folder(*last)
    } else {
        To::Path(*last)
    };
    Taken {
        act: Some(Act::Move),
        sources: sources.to_vec(),
        to: Some(to),
        ..Taken::default()
    }
}
