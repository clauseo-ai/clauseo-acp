//! The setup exe's command line (S15 `PR4h`): its three modes, parsed with no IO, and the
//! quoting `UserSetup` gives each argument so the exe reads back exactly what was meant.
//!
//! - `--set-up --parent <pid> --data-dir <path> [--add <n>]`: set up, re-run or extend
//!   (`--add`, 1 to [`POOL_CAP`]). The data folder is a plain drive-letter path (`C:\...`).
//! - `--reclaim --parent <pid>`: the elevated clean-up (`PR4i`).
//! - `--adopt --user-install-dir <path> (--owner-sid <sid> --data-dir <path> | --per-machine)`:
//!   run by the installer's elevated step from the admin copy it just placed (`admin_copy`):
//!   lock the admin folder, record the copy's hash and the install folder, then, per user, set
//!   up. `--owner-sid` and `--data-dir` come from the unelevated installer, the lawyer's own;
//!   `--adopt` checks the SID against that process's token (`adopt.rs`).
//! - `--remove [--uninstall] [--per-machine]`: the removal (`PR4j`). The app's in-app removal
//!   passes `--remove` alone, which keeps the admin copy and its record values, so a later
//!   set-up runs the verified copy with no installer; the uninstaller's hook adds `--uninstall`,
//!   which also takes the admin copy away, last. The hook adds `--per-machine` only in
//!   the per-machine installer, chosen when the installer is built; nothing else ever. Who asked
//!   comes from the process's recorded parent (D-B2), never from an argument.
//! - `--refresh`: the admin copy's refresh (`refresh.rs`), which the SYSTEM task runs. It takes
//!   no other argument: everything it reads comes from the record and fixed paths.
//! - `--stop`: stops every process of the pool's users (`stop.rs`), which the SYSTEM task
//!   `\Clauseo\SandboxStop` runs. No other argument either: the pool comes from the record.
//!
//! `--per-machine` (taken by `--remove` and `--adopt`) and `--uninstall` (`--remove` only)
//! are the flags without a value. Every other flag takes its value as the next argument (never
//! `--flag=value`). Each flag comes once.
//! Anything else is a [`Usage`] error, and the exe exits `Usage`. `--parent` names the process
//! that asked; the exe checks it against the parent Windows recorded (D-B2), so it never decides
//! the owner alone. `--data-dir` is written as admin into the record's `DataDir`, so it is held
//! to a plain local path here and resolved as the owner after ([`data_dir_of`]).
//! `--remove` treats `DataDir` as untrusted: checked the same way, and acted on as the owner, or,
//! in a per-machine removal the owner didn't ask for, only as `remove.rs` limits it.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::windows::pool::POOL_CAP;
use crate::windows::sids::Sid;

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
    /// `--adopt`.
    Adopt {
        /// The app's install folder (`--user-install-dir`), recorded as `UserInstallDir`.
        user_install_dir: PathBuf,
        /// The lawyer the unelevated installer names, for the set-up that follows; `None` with
        /// `--per-machine`, which sets nobody up.
        user: Option<AdoptUser>,
    },
    /// `--remove`.
    Remove {
        /// `--per-machine`: a per-machine uninstall, which removes every pool the record names,
        /// whoever asked (D-B9 as ruled). Without it, only the record's owner removes anything.
        per_machine: bool,
        /// `--uninstall`: the uninstaller's removal, which also deletes the admin copy, its
        /// record values and the SYSTEM tasks (refresh and stop), last. Without it (the app's),
        /// they stay.
        uninstall: bool,
    },
    /// `--refresh`.
    Refresh,
    /// `--stop`.
    Stop,
}

/// The lawyer a per-user `--adopt` sets up, as the unelevated installer named them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptUser {
    /// `--owner-sid`: the unelevated installer's own user.
    pub owner_sid: Sid,
    /// `--data-dir`: that user's data folder.
    pub data_dir: PathBuf,
}

/// Why a command line isn't one the exe takes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "usage: clauseo-sandbox-setup --set-up --parent <pid> --data-dir <path> [--add <n>] | \
         --reclaim --parent <pid> | --adopt --user-install-dir <path> (--owner-sid <sid> \
         --data-dir <path> | --per-machine) | --remove [--uninstall] [--per-machine] | --refresh | \
         --stop ({0})"
)]
pub struct Usage(pub String);

/// Parses the arguments after the program's name.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Mode, Usage> {
    let mut args = args.into_iter();
    let mode = args.next().ok_or_else(|| Usage("no mode".to_owned()))?;
    // The SYSTEM tasks' modes take nothing else: what they act on comes from the record.
    for (alone, parsed) in [("--refresh", Mode::Refresh), ("--stop", Mode::Stop)] {
        if mode == alone {
            return match args.next() {
                None => Ok(parsed),
                Some(extra) => Err(Usage(format!(
                    "{alone} takes no {:?}",
                    extra.to_string_lossy()
                ))),
            };
        }
    }
    let mut parent = None;
    let mut data_dir = None;
    let mut add = None;
    let mut install_dir = None;
    let mut owner_sid = None;
    let mut per_machine = false;
    let mut uninstall = false;
    while let Some(flag) = args.next() {
        let flag = flag
            .into_string()
            .map_err(|arg| Usage(format!("{} isn't text", arg.to_string_lossy())))?;
        if flag == PER_MACHINE || flag == UNINSTALL {
            let seen = if flag == PER_MACHINE {
                &mut per_machine
            } else {
                &mut uninstall
            };
            if *seen {
                return Err(Usage(format!("{flag} given twice")));
            }
            *seen = true;
            continue;
        }
        let slot = match flag.as_str() {
            "--parent" => &mut parent,
            "--data-dir" => &mut data_dir,
            "--add" => &mut add,
            "--user-install-dir" => &mut install_dir,
            "--owner-sid" => &mut owner_sid,
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
    if (install_dir.is_some() || owner_sid.is_some()) && mode != "--adopt" {
        return Err(Usage(format!(
            "{mode} takes no --user-install-dir or --owner-sid"
        )));
    }
    if uninstall && mode != "--remove" {
        return Err(Usage(format!("{mode} takes no {UNINSTALL}")));
    }
    if per_machine && !matches!(mode, "--remove" | "--adopt") {
        return Err(Usage(format!("{mode} takes no {PER_MACHINE}")));
    }
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
        "--adopt" => {
            refuse("--parent", parent.is_some())?;
            refuse("--add", add.is_some())?;
            let user = match (per_machine, owner_sid, data_dir) {
                (true, None, None) => None,
                (false, Some(sid), Some(dir)) => Some(AdoptUser {
                    owner_sid: sid_of(&sid)?,
                    data_dir: data_dir_of(Some(dir))?,
                }),
                _ => {
                    return Err(Usage(format!(
                        "--adopt takes --owner-sid and --data-dir, or {PER_MACHINE} alone"
                    )));
                }
            };
            Ok(Mode::Adopt {
                user_install_dir: path_of("--user-install-dir", install_dir)?,
                user,
            })
        }
        "--remove" => {
            refuse("--parent", parent.is_some())?;
            refuse("--data-dir", data_dir.is_some())?;
            refuse("--add", add.is_some())?;
            Ok(Mode::Remove {
                per_machine,
                uninstall,
            })
        }
        other => Err(Usage(format!("unknown mode {other:?}"))),
    }
}

/// The flag the uninstaller's hook adds to `--remove`, and the app never does.
pub const UNINSTALL: &str = "--uninstall";

/// The flag the per-machine installer's hook adds to `--remove`.
pub const PER_MACHINE: &str = "--per-machine";

/// `--parent`: a process ID, in decimal, never 0.
fn parent_of(value: Option<OsString>) -> Result<u32, Usage> {
    let value = value.ok_or_else(|| Usage("--parent is missing".to_owned()))?;
    let text = value.to_str().unwrap_or_default();
    match text.parse::<u32>() {
        Ok(pid) if pid != 0 && text.bytes().all(|byte| byte.is_ascii_digit()) => Ok(pid),
        _ => Err(Usage(format!("--parent {text:?} isn't a process ID"))),
    }
}

/// `--owner-sid`: a SID in its text form (`S-1-5-21-...`).
fn sid_of(value: &OsString) -> Result<Sid, Usage> {
    let text = value.to_str().unwrap_or_default();
    Sid::parse(text).map_err(|_not_a_sid| Usage(format!("--owner-sid {text:?} isn't a SID")))
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
    path_of("--data-dir", value)
}

/// `flag`'s value held to [`data_dir_of`]'s rules: `--user-install-dir` is written as admin
/// into the record too.
fn path_of(flag: &str, value: Option<OsString>) -> Result<PathBuf, Usage> {
    let value = value.ok_or_else(|| Usage(format!("{flag} is missing")))?;
    let Some(text) = value.to_str() else {
        return Err(Usage(format!("{flag} isn't text")));
    };
    let refuse = |why: &str| Err(Usage(format!("{flag} {text:?} {why}")));
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
