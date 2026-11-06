//! The setup exe's command line (S15 `PR4h`): its three modes, parsed with no IO, and the
//! quoting `UserSetup` gives each argument so the exe reads back exactly what was meant.
//!
//! - `--set-up --parent <pid> --data-dir <path> [--add <n>]`: set up, re-run or extend
//!   (`--add`, 1 to [`POOL_CAP`]). The data folder is a plain drive-letter path (`C:\...`).
//! - `--reclaim --parent <pid>`: the elevated clean-up (`PR4i`).
//! - `--remove`: the uninstall (`PR4j`); the hook passes nothing else.
//!
//! Each flag takes its value as the next argument (never `--flag=value`) and comes once.
//! Anything else is a [`Usage`] error, and the exe exits `Usage`. `--parent` names the process
//! that asked; the exe checks it against the parent Windows recorded (D-B2), so it never decides
//! the owner alone. `--data-dir` is written as admin into the record's `DataDir`, so it is held
//! to a plain local path here and resolved as the owner after ([`data_dir_of`]).
//! `PR4j`'s `--remove` treats `DataDir` as untrusted and acts on it only as the owner.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::windows::pool::POOL_CAP;

/// What the exe was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// `--set-up`.
    SetUp {
        /// The process that asked (`--parent`).
        parent: u32,
        /// The owner's data folder (`--data-dir`).
        data_dir: PathBuf,
        /// 0 for a set-up or re-run; above 0 an extend by that many (`--add`).
        add: u8,
    },
    /// `--reclaim`.
    Reclaim {
        /// The process that asked (`--parent`).
        parent: u32,
    },
    /// `--remove`.
    Remove,
}

/// Why a command line isn't one the exe takes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "usage: clauseo-sandbox-setup --set-up --parent <pid> --data-dir <path> [--add <n>] | \
         --reclaim --parent <pid> | --remove ({0})"
)]
pub struct Usage(pub String);

/// Parses the arguments after the program's name.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Mode, Usage> {
    let mut args = args.into_iter();
    let mode = args.next().ok_or_else(|| Usage("no mode".to_owned()))?;
    let mut parent = None;
    let mut data_dir = None;
    let mut add = None;
    while let Some(flag) = args.next() {
        let flag = flag
            .into_string()
            .map_err(|arg| Usage(format!("{} isn't text", arg.to_string_lossy())))?;
        let slot = match flag.as_str() {
            "--parent" => &mut parent,
            "--data-dir" => &mut data_dir,
            "--add" => &mut add,
            other => return Err(Usage(format!("unknown argument {other:?}"))),
        };
        if slot.is_some() {
            return Err(Usage(format!("{flag} given twice")));
        }
        *slot = Some(
            args.next()
                .ok_or_else(|| Usage(format!("{flag} needs a value")))?,
        );
    }
    let mode = mode.to_str().unwrap_or_default();
    let refuse = |flag: &str, given: bool| {
        if given {
            Err(Usage(format!("{mode} takes no {flag}")))
        } else {
            Ok(())
        }
    };
    match mode {
        "--set-up" => Ok(Mode::SetUp {
            parent: parent_of(parent)?,
            data_dir: data_dir_of(data_dir)?,
            add: add.as_ref().map_or(Ok(0), add_of)?,
        }),
        "--reclaim" => {
            refuse("--data-dir", data_dir.is_some())?;
            refuse("--add", add.is_some())?;
            Ok(Mode::Reclaim {
                parent: parent_of(parent)?,
            })
        }
        "--remove" => {
            refuse("--parent", parent.is_some())?;
            refuse("--data-dir", data_dir.is_some())?;
            refuse("--add", add.is_some())?;
            Ok(Mode::Remove)
        }
        other => Err(Usage(format!("unknown mode {other:?}"))),
    }
}

/// `--parent`: a process ID, in decimal, never 0.
fn parent_of(value: Option<OsString>) -> Result<u32, Usage> {
    let value = value.ok_or_else(|| Usage("--parent is missing".to_owned()))?;
    let text = value.to_str().unwrap_or_default();
    match text.parse::<u32>() {
        Ok(pid) if pid != 0 && text.bytes().all(|byte| byte.is_ascii_digit()) => Ok(pid),
        _ => Err(Usage(format!("--parent {text:?} isn't a process ID"))),
    }
}

/// `--add`: 1 to [`POOL_CAP`].
fn add_of(value: &OsString) -> Result<u8, Usage> {
    let text = value.to_str().unwrap_or_default();
    match text.parse::<u8>() {
        Ok(n) if (1..=POOL_CAP).contains(&n) && text.bytes().all(|b| b.is_ascii_digit()) => Ok(n),
        _ => Err(Usage(format!("--add {text:?} isn't 1 to {POOL_CAP}"))),
    }
}

/// `--data-dir`: a full local path from a drive letter (`C:\...`). Refused: a relative path,
/// a bare drive (`C:x`), a share (`\\server\...`), any `\\?\` or `\\.\` form (`\\?\UNC\`
/// and device paths included), a `.` or `..` part, an empty part, a `:` past the drive (a
/// stream), and a part ending in a dot or a space (which Windows would drop). The exe then
/// resolves what passes as the owner (`exe.rs`), with no link on the way.
pub fn data_dir_of(value: Option<OsString>) -> Result<PathBuf, Usage> {
    let value = value.ok_or_else(|| Usage("--data-dir is missing".to_owned()))?;
    let Some(text) = value.to_str() else {
        return Err(Usage("--data-dir isn't text".to_owned()));
    };
    let refuse = |why: &str| Err(Usage(format!("--data-dir {text:?} {why}")));
    let Some((letter, rest)) = text.split_at_checked(2) else {
        return refuse("isn't a full path from a drive");
    };
    let drive = matches!(letter.as_bytes(), [l, b':'] if l.is_ascii_alphabetic());
    if !drive || !rest.starts_with(['\\', '/']) {
        return refuse("isn't a full path from a drive");
    }
    let parts: Vec<&str> = rest.split(['\\', '/']).skip(1).collect();
    for (at, part) in parts.iter().enumerate() {
        // Only the last part may be empty: `C:\` itself, or a trailing separator.
        if part.is_empty() && at + 1 < parts.len() {
            return refuse("has an empty part");
        }
        if *part == "." || *part == ".." {
            return refuse("has a . or .. part");
        }
        if part.contains(':') {
            return refuse("has a : past the drive");
        }
        if part.ends_with(['.', ' ']) {
            return refuse("has a part ending in a dot or a space");
        }
    }
    Ok(PathBuf::from(text))
}

/// `arg` quoted for a Windows command line, so the C runtime's parsing (and Rust's `args`)
/// gives it back exactly: as it is when it has no space, tab, quote or newline and isn't empty;
/// else in quotes, each quote escaped, and the backslashes before a quote or the end doubled.
pub fn quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\u{b}', '"']) {
        return arg.to_owned();
    }
    let mut quoted = String::from('"');
    let mut slashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => {
                quoted.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
                quoted.push('"');
                slashes = 0;
            }
            other => {
                quoted.extend(std::iter::repeat_n('\\', slashes));
                quoted.push(other);
                slashes = 0;
            }
        }
    }
    quoted.extend(std::iter::repeat_n('\\', slashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests;
