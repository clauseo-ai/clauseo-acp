//! Where a command runs from: an absolute path kept as it is, or the first match on the search
//! path (the PATH variable, then the folders each OS's tools install into).

use super::path::{self, file_name, is_absolute, join};
use super::{Env, Os, Problem};
use std::path::{Path, PathBuf};

/// The PATHEXT extensions when the variable is not set.
const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

/// The file `program` runs from, or `None` when no file is found.
///
/// A bare name (no separator) is searched for on the search path: the PATH variable, then the
/// folders each OS's tools install into (on Mac: Homebrew, the newest nvm Node, Volta, `.bun`,
/// `.local`, `.cargo`; on Windows: `%APPDATA%\npm`, `%ProgramFiles%\nodejs`,
/// `%LOCALAPPDATA%\Volta\bin`, `%SystemRoot%\System32`). On Windows a name without an extension
/// is tried with each PATHEXT extension, in order (`npx` finds `npx.cmd`).
///
/// An absolute path is returned when it is a file (on Windows too, with the same extension rule
/// when it has none). Any other path, such as `bin/tool`, `./tool` or `.\npx`, gives `None`: a
/// relative path with a separator is not searched for.
pub fn resolve_command(env: &Env<'_>, program: &str) -> Option<PathBuf> {
    if program.is_empty() {
        return None;
    }
    let absolute = is_absolute(env.os, program);
    if !absolute && has_separator(env.os, program) {
        return None;
    }
    let names = file_names(env, program);
    let dirs = if absolute {
        vec![String::new()]
    } else {
        search_dirs(env)
    };
    for dir in &dirs {
        for name in &names {
            let candidate = if dir.is_empty() {
                name.clone()
            } else {
                join(env.os, dir, name)
            };
            if env.fs.is_file(Path::new(&candidate)) {
                return Some(PathBuf::from(candidate));
            }
        }
    }
    None
}

/// Whether `p` contains a separator on `os`.
fn has_separator(os: Os, p: &str) -> bool {
    match os {
        Os::Mac => p.contains('/'),
        Os::Windows => p.contains(['/', '\\']),
    }
}

/// `command` as the file name or names to look for. On Windows a name without an extension gets
/// each PATHEXT extension, lower-cased (`npx` gives `npx.cmd`); one with an extension is tried as
/// it is.
fn file_names(env: &Env<'_>, command: &str) -> Vec<String> {
    if env.os == Os::Mac || file_name(Os::Windows, command).contains('.') {
        return vec![command.to_owned()];
    }
    pathext(env)
        .into_iter()
        .map(|ext| format!("{command}{}", ext.to_ascii_lowercase()))
        .collect()
}

/// The PATHEXT extensions, in order.
fn pathext(env: &Env<'_>) -> Vec<String> {
    let raw = path::var(env, "PATHEXT").unwrap_or_else(|| DEFAULT_PATHEXT.to_owned());
    raw.split(';')
        .map(str::trim)
        .filter(|ext| !ext.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The folders to search, in order: PATH, then the well-known folders for the OS.
fn search_dirs(env: &Env<'_>) -> Vec<String> {
    let os = env.os;
    let mut dirs = Vec::new();
    if let Some(search) = path::var(env, "PATH") {
        let separator = match os {
            Os::Mac => ':',
            Os::Windows => ';',
        };
        dirs.extend(
            search
                .split(separator)
                .map(|dir| dir.trim().trim_matches('"').to_owned())
                .filter(|dir| !dir.is_empty()),
        );
    }
    match os {
        Os::Mac => {
            let home = path::home(env);
            dirs.push("/opt/homebrew/bin".to_owned());
            dirs.push("/usr/local/bin".to_owned());
            dirs.push(join(os, &home, ".volta/bin"));
            dirs.push(join(os, &home, ".bun/bin"));
            dirs.push(join(os, &home, ".local/bin"));
            dirs.push(join(os, &home, ".cargo/bin"));
            dirs.extend(newest_nvm_bin(env, &home));
        }
        Os::Windows => {
            let folders = [
                ("APPDATA", "npm"),
                ("ProgramFiles", "nodejs"),
                ("LOCALAPPDATA", "Volta/bin"),
                ("SystemRoot", "System32"),
            ];
            for (var, rel) in folders {
                if let Some(base) = path::var(env, var) {
                    dirs.push(join(os, &base, rel));
                }
            }
        }
    }
    dirs
}

/// The `bin` folder of the newest Node version nvm has installed, by version number.
fn newest_nvm_bin(env: &Env<'_>, home: &str) -> Option<String> {
    let root = join(Os::Mac, home, ".nvm/versions/node");
    env.fs
        .list_dir(Path::new(&root))
        .iter()
        .filter_map(|entry| {
            let text = entry.to_string_lossy().into_owned();
            let version = parse_version(file_name(Os::Mac, &text))?;
            Some((version, text))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, dir)| join(Os::Mac, &dir, "bin"))
}

/// `v20.11.1` (or `20.11.1`) as its numbers, or `None` when it is not a version.
fn parse_version(name: &str) -> Option<Vec<u64>> {
    let digits = name.strip_prefix('v').unwrap_or(name);
    digits
        .split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect()
}

/// The problems a command adds to a candidate: the command as given when it is not found.
pub(crate) fn resolve_or_problem(
    env: &Env<'_>,
    original: &str,
    command: &str,
    problems: &mut Vec<Problem>,
) -> PathBuf {
    if let Some(found) = resolve_command(env, command) {
        return found;
    }
    problems.push(Problem::CommandNotFound {
        original: original.to_owned(),
    });
    PathBuf::from(command)
}
