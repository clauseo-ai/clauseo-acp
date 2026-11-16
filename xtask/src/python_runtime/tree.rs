//! What `build` does to the unpacked runtime's files: replace symlinks with the files they point
//! at, drop what the runtime does not need, and measure what is left.

use super::Failure;
use crate::write;
use std::fs;
use std::path::{Path, PathBuf};

/// One kind of entry under the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kind {
    File,
    Dir,
    Symlink,
}

/// Every entry under `root`, as `/`-separated paths relative to it, sorted, with its kind and
/// (for a file) size. A symlink is listed, never followed.
pub(crate) fn walk(root: &Path) -> Result<Vec<(String, Kind, u64)>, Failure> {
    let mut found = Vec::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(rel) = pending.pop() {
        let dir = root.join(&rel);
        let entries = fs::read_dir(&dir)
            .map_err(|error| Failure::io(&format!("listing {}", dir.display()), &error))?;
        for entry in entries {
            let entry = entry
                .map_err(|error| Failure::io(&format!("listing {}", dir.display()), &error))?;
            let path = rel.join(entry.file_name());
            let meta = fs::symlink_metadata(entry.path()).map_err(|error| {
                Failure::io(&format!("reading {}", entry.path().display()), &error)
            })?;
            let name = path.to_string_lossy().replace('\\', "/");
            if meta.file_type().is_symlink() {
                found.push((name, Kind::Symlink, 0));
            } else if meta.is_dir() {
                found.push((name, Kind::Dir, 0));
                pending.push(path);
            } else {
                found.push((name, Kind::File, meta.len()));
            }
        }
    }
    found.sort();
    Ok(found)
}

/// The number of files under `root` and their total size in bytes.
pub(crate) fn file_count_and_size(root: &Path) -> Result<(usize, u64), Failure> {
    let entries = walk(root)?;
    let files = entries.iter().filter(|(_, kind, _)| *kind == Kind::File);
    Ok(files.fold((0, 0), |(count, bytes), (_, _, size)| {
        (count + 1, bytes + size)
    }))
}

/// A rule of what to drop: every entry directly in `dir` (relative to the runtime) whose name
/// starts with `prefix`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Trim {
    pub dir: &'static str,
    pub prefix: &'static str,
}

const fn trim(dir: &'static str, prefix: &'static str) -> Trim {
    Trim { dir, prefix }
}

/// What macOS's runtime drops: Tcl/Tk and tkinter, IDLE, turtle, ensurepip, `CPython`'s test
/// packages (should a PBS release ship them), the headers and build config, the man pages and
/// pkg-config files, the scripts of what is gone, and libpython3.13.dylib, which PBS's
/// `python3.13` does not load (it is linked statically; only an embedding program would). pip
/// stays: PR 2's wrapper runs it.
pub(crate) const MACOS_TRIM: [Trim; 20] = [
    trim("", "include"),
    trim("", "share"),
    trim("bin", "idle3"),
    trim("bin", "pydoc3"),
    trim("bin", "python3-config"),
    trim("bin", "python3.13-config"),
    trim("lib", "pkgconfig"),
    trim("lib", "libtcl"),
    trim("lib", "libpython"),
    trim("lib", "tcl"),
    trim("lib", "tk"),
    trim("lib", "itcl"),
    trim("lib", "thread"),
    trim("lib/python3.13", "tkinter"),
    trim("lib/python3.13", "idlelib"),
    trim("lib/python3.13", "turtle"),
    trim("lib/python3.13", "ensurepip"),
    trim("lib/python3.13", "config-3"),
    trim("lib/python3.13", "test"),
    trim("lib/python3.13/lib-dynload", "_tkinter"),
];

/// What Windows' runtime drops: Tcl/Tk and tkinter, IDLE, turtle, ensurepip, the headers and
/// import libraries, and `CPython`'s test extension modules.
pub(crate) const WINDOWS_TRIM: [Trim; 13] = [
    trim("", "include"),
    trim("", "libs"),
    trim("", "tcl"),
    trim("Lib", "tkinter"),
    trim("Lib", "idlelib"),
    trim("Lib", "turtle"),
    trim("Lib", "ensurepip"),
    trim("Lib", "test"),
    trim("DLLs", "_tkinter"),
    trim("DLLs", "tcl"),
    trim("DLLs", "tk"),
    trim("DLLs", "_test"),
    trim("DLLs", "_ctypes_test"),
];

/// Removes what `rules` match under `root` and returns the relative paths removed.
pub(crate) fn apply_trim(root: &Path, rules: &[Trim]) -> Result<Vec<String>, Failure> {
    let mut removed = Vec::new();
    for rule in rules {
        let dir = root.join(rule.dir);
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(rule.prefix))
            .collect();
        names.sort();
        for name in names {
            write::remove_in(&dir, &name).map_err(|error| {
                Failure::io(&format!("removing {}", dir.join(&name).display()), &error)
            })?;
            let rel = if rule.dir.is_empty() {
                name
            } else {
                format!("{}/{name}", rule.dir)
            };
            removed.push(rel);
        }
    }
    Ok(removed)
}

/// The packages whose own test suites (every folder named `tests` in them) `build` drops: about
/// 53 MB unpacked and 14 MB compressed, which nothing imports at run time (`numpy.testing` and
/// `pandas._testing` are elsewhere and stay).
pub(crate) const PACKAGES_WITHOUT_TESTS: [&str; 2] = ["numpy", "pandas"];

/// Removes every folder named `tests` inside the `packages` folders of `site_packages` and
/// returns them, relative to `site_packages`.
pub(crate) fn drop_package_tests(
    site_packages: &Path,
    packages: &[&str],
) -> Result<Vec<String>, Failure> {
    let mut removed: Vec<String> = Vec::new();
    for package in packages {
        let dir = site_packages.join(package);
        if !dir.is_dir() {
            continue;
        }
        for (rel, kind, _) in walk(&dir)? {
            let inside_removed = removed
                .iter()
                .any(|gone| format!("{package}/{rel}").starts_with(&format!("{gone}/")));
            if kind != Kind::Dir || inside_removed || rel.rsplit('/').next() != Some("tests") {
                continue;
            }
            let (parent, name) = split(&dir.join(&rel))?;
            write::remove_in(&parent, &name)
                .map_err(|error| Failure::io(&format!("removing {package}/{rel}"), &error))?;
            removed.push(format!("{package}/{rel}"));
        }
    }
    Ok(removed)
}

/// Replaces every symlink under `root` with a copy of the file it points at (Clauseo's extractor
/// skips links, and the install folder must hold none). A link whose target is missing, outside
/// `root`, or not a regular file fails. Returns `link -> target` for each.
pub(crate) fn materialize_symlinks(root: &Path) -> Result<Vec<String>, Failure> {
    let canonical_root = dunce::canonicalize(root)
        .map_err(|error| Failure::io(&format!("resolving {}", root.display()), &error))?;
    let mut done = Vec::new();
    for (rel, kind, _) in walk(root)? {
        if kind != Kind::Symlink {
            continue;
        }
        let link = root.join(&rel);
        let target = dunce::canonicalize(&link).map_err(|error| {
            Failure::new(
                format!("the symlink {rel} points at nothing that exists: {error}"),
                "check the PBS archive's layout; the lock may pin a broken release",
            )
        })?;
        if !target.starts_with(&canonical_root) || !target.is_file() {
            return Err(Failure::new(
                format!(
                    "the symlink {rel} points at {}, which is not a file inside the runtime",
                    target.display()
                ),
                "check the PBS archive's layout before building from it",
            ));
        }
        let (dir, name) = split(&link)?;
        write::remove_in(&dir, &name)
            .and_then(|()| write::copy_file_in(&dir, &name, &target))
            .map_err(|error| Failure::io(&format!("replacing the symlink {rel}"), &error))?;
        let shown = target.strip_prefix(&canonical_root).unwrap_or(&target);
        done.push(format!(
            "{rel} -> {}",
            shown.to_string_lossy().replace('\\', "/")
        ));
    }
    Ok(done)
}

/// The folder and file name of `path`.
pub(crate) fn split(path: &Path) -> Result<(PathBuf, String), Failure> {
    match (path.parent(), path.file_name()) {
        (Some(dir), Some(name)) => Ok((dir.to_path_buf(), name.to_string_lossy().into_owned())),
        _ => Err(Failure::new(
            format!("{} has no folder and file name", path.display()),
            "pass an --out folder below the root",
        )),
    }
}

/// The symlinks under `root`, which must be none once the runtime is laid out.
pub(crate) fn symlinks(root: &Path) -> Result<Vec<String>, Failure> {
    Ok(walk(root)?
        .into_iter()
        .filter(|(_, kind, _)| *kind == Kind::Symlink)
        .map(|(rel, _, _)| rel)
        .collect())
}

/// The names directly in `dir`, sorted; none when it is missing.
pub(crate) fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}
