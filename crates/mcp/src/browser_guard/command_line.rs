//! Windows: the strings `CreateProcessW` takes for a child the guard starts in its job
//! (`job/spawn.rs`): the command line, the environment block and the working folder, each UTF-16
//! and NUL-terminated.
//!
//! Std builds all three inside `Command`, which cannot start a process in a job, and exports
//! none, so this is a port of std's rules (Rust 1.99, `library/std/src/sys/args/windows.rs`
//! `append_arg`, `sys/process/windows.rs` `make_command_line`, `make_envp` and `make_dirp`):
//! - the program always in quotes, then each argument after one space, quoted when it is empty
//!   or holds a space or tab, a `"` escaped with one backslash more than the backslashes before
//!   it, and the backslashes before a closing quote doubled, so `CommandLineToArgvW` and the C
//!   runtime read back exactly the arguments given;
//! - `name=value` entries, each NUL-terminated, then one more NUL (two for no variables), names
//!   compared without case as Windows does: one entry per name, the first spelling with the last
//!   value (what std's `BTreeMap<EnvKey, _>` keeps), sorted by the uppercase name's UTF-16
//!   code units. Uppercase is Windows' one-to-one mapping of each UTF-16 unit, as
//!   `CompareStringOrdinal` ignoring case uses it (`fold`): `ß` stays `ß`, so `STRAßE` and
//!   `STRASSE` are two names;
//! - the folder with a `\\?\` (or `\\?\UNC\`) prefix removed when the rest reads the same
//!   without it, since `CreateProcessW` takes no verbatim working folder. Std asks
//!   `GetFullPathNameW`; this asks a stricter rule that needs no call (`plain`).
//!
//! A NUL inside any part is refused, as std refuses it. [`is_batch`] says which programs
//! `job/spawn.rs` refuses. Pure functions, so the Linux tests run them too
//! (`command_line/tests.rs`).

use std::io;

const QUOTE: u16 = b'"' as u16;
const BACKSLASH: u16 = b'\\' as u16;
const SPACE: u16 = b' ' as u16;
const TAB: u16 = b'\t' as u16;
const EQUALS: u16 = b'=' as u16;

/// `program`, then each of `args`, as one NUL-terminated command line.
pub(crate) fn command_line(program: &[u16], args: &[Vec<u16>]) -> io::Result<Vec<u16>> {
    no_nul(program)?;
    // A file name holds no quote; one here would end the program's quotes early.
    if program.contains(&QUOTE) {
        return Err(invalid("the program's path holds a quote"));
    }
    let mut line = Vec::with_capacity(program.len() + 2);
    line.push(QUOTE);
    line.extend_from_slice(program);
    line.push(QUOTE);
    for arg in args {
        line.push(SPACE);
        append_arg(&mut line, arg)?;
    }
    line.push(0);
    Ok(line)
}

/// Std's `append_arg` for a regular argument, quoted only when it must be.
fn append_arg(line: &mut Vec<u16>, arg: &[u16]) -> io::Result<()> {
    no_nul(arg)?;
    let quote = arg.is_empty() || arg.iter().any(|&unit| unit == SPACE || unit == TAB);
    if quote {
        line.push(QUOTE);
    }
    let mut backslashes = 0_usize;
    for &unit in arg {
        if unit == BACKSLASH {
            backslashes += 1;
        } else {
            if unit == QUOTE {
                // n + 1 more, for 2n + 1 before the literal quote.
                line.extend(std::iter::repeat_n(BACKSLASH, backslashes + 1));
            }
            backslashes = 0;
        }
        line.push(unit);
    }
    if quote {
        // n more, for 2n before the closing quote.
        line.extend(std::iter::repeat_n(BACKSLASH, backslashes));
        line.push(QUOTE);
    }
    Ok(())
}

/// The environment block for `vars` (names, and values in UTF-16), in the order given.
pub(crate) fn env_block(vars: Vec<(String, Vec<u16>)>) -> io::Result<Vec<u16>> {
    // (uppercase name, name, value), one per name compared without case.
    let mut entries: Vec<(Vec<u16>, String, Vec<u16>)> = Vec::with_capacity(vars.len());
    for (name, value) in vars {
        let equals = name.chars().skip(1).any(|c| c == '=');
        if name.is_empty() || name.contains('\0') || equals {
            return Err(invalid(
                "an environment variable's name is empty or holds `=` or NUL",
            ));
        }
        no_nul(&value)?;
        let folded: Vec<u16> = name.encode_utf16().map(fold).collect();
        match entries.iter_mut().find(|entry| entry.0 == folded) {
            Some(entry) => entry.2 = value,
            None => entries.push((folded, name, value)),
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut block = Vec::new();
    if entries.is_empty() {
        block.push(0);
    }
    for (_, name, value) in entries {
        block.extend(name.encode_utf16());
        block.push(EQUALS);
        block.extend(value);
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

/// UTF-16 unit `unit` in uppercase, when its uppercase is one unit too; else itself. A
/// surrogate stays itself, as in Windows' table.
fn fold(unit: u16) -> u16 {
    let Some(c) = char::from_u32(u32::from(unit)) else {
        return unit;
    };
    let mut upper = c.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(one), None) => u16::try_from(u32::from(one)).unwrap_or(unit),
        _ => unit,
    }
}

/// Whether `program` names a batch file: its file name, less any `:stream` part and then
/// trailing dots and spaces (which Windows drops), ends in `.bat` or `.cmd`, in any case.
pub(crate) fn is_batch(program: &[u16]) -> bool {
    let path = String::from_utf16_lossy(program);
    let name = path.rsplit(['\\', '/']).next().unwrap_or_default();
    let name = name.split(':').next().unwrap_or_default();
    let name = name.trim_end_matches(['.', ' ']);
    name.rsplit_once('.').is_some_and(|(_, extension)| {
        extension.eq_ignore_ascii_case("bat") || extension.eq_ignore_ascii_case("cmd")
    })
}

/// `dir` as the NUL-terminated working folder `CreateProcessW` takes.
pub(crate) fn current_dir(dir: &[u16]) -> io::Result<Vec<u16>> {
    no_nul(dir)?;
    let unc: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
    let verbatim: Vec<u16> = r"\\?\".encode_utf16().collect();
    let mut out: Vec<u16> = Vec::with_capacity(dir.len() + 1);
    if let Some(rest) = dir.strip_prefix(unc.as_slice())
        && plain(rest, false)
    {
        out.extend_from_slice(&[BACKSLASH, BACKSLASH]);
        out.extend_from_slice(rest);
    } else if let Some(rest) = dir.strip_prefix(verbatim.as_slice())
        && plain(rest, true)
    {
        out.extend_from_slice(rest);
    } else {
        out.extend_from_slice(dir);
    }
    out.push(0);
    Ok(out)
}

/// Whether `rest`, after a verbatim prefix, names the same folder without it: a drive's root
/// and folders (`drive`) or a share's server, share and folders, then at most one trailing
/// backslash; no `/`, no empty part, no `.` or `..` part and no part ending in a dot or a
/// space, all of which the normal path rules would change.
fn plain(rest: &[u16], drive: bool) -> bool {
    let Ok(text) = String::from_utf16(rest) else {
        return false;
    };
    if text.contains('/') {
        return false;
    }
    let mut parts: Vec<&str> = text.split('\\').collect();
    if parts.len() < 2 {
        // A bare `C:` is relative to the drive's folder; a share needs a server and a share.
        return false;
    }
    if parts.last() == Some(&"") {
        parts.pop();
    }
    let named = if drive {
        let Some((root, folders)) = parts.split_first() else {
            return false;
        };
        let letter = root.as_bytes().first().is_some_and(u8::is_ascii_alphabetic);
        if !(letter && root.len() == 2 && root.ends_with(':')) {
            return false;
        }
        folders
    } else if parts.len() < 2 {
        return false;
    } else {
        parts.as_slice()
    };
    named
        .iter()
        .all(|part| !part.is_empty() && !part.ends_with('.') && !part.ends_with(' '))
}

fn no_nul(text: &[u16]) -> io::Result<()> {
    if text.contains(&0) {
        return Err(invalid(
            "a NUL in the program, an argument, a variable or the folder",
        ));
    }
    Ok(())
}

fn invalid(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, what)
}

#[cfg(test)]
mod tests;
