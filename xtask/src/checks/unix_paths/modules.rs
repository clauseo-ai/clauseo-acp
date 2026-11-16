//! Which files a cfg on their `mod name;` line keeps out of every Windows build. A file is
//! Unix-only when every `mod` line that pulls it in is Unix-only (`ModDecl::unix`), or sits in a
//! file that is itself Unix-only. A crate root, which no `mod` line pulls in, is not.
//!
//! A `mod name;` resolves as rustc resolves it: against the folder of a `lib.rs`, `main.rs`,
//! `mod.rs` or crate root, else against the folder named after the file, then down each inline
//! `mod a { .. }` around it, to `name.rs` or `name/mod.rs`. A `#[path]` resolves with core-io's
//! `paths::resolve`, against the folder of the file, or of the inline modules inside it.

use super::scan::{FileScan, ModDecl};
use crate::checks::core_io::cfgs::paths;
use std::collections::{BTreeMap, BTreeSet};

/// The files among `scans` (repo-relative `.rs` files under `crates/`) that are Unix-only.
pub(super) fn unix_files(scans: &BTreeMap<String, FileScan>) -> BTreeSet<String> {
    let mut parents: BTreeMap<String, Vec<(&str, bool)>> = BTreeMap::new();
    for (file, scan) in scans {
        for decl in &scan.mods {
            if let Some(child) = resolve(file, decl, scans) {
                parents
                    .entry(child)
                    .or_default()
                    .push((file.as_str(), decl.unix));
            }
        }
    }
    let mut known: BTreeMap<String, bool> = BTreeMap::new();
    for file in scans.keys() {
        unix_only(file, &parents, &mut known, &mut BTreeSet::new());
    }
    known
        .into_iter()
        .filter_map(|(file, unix)| unix.then_some(file))
        .collect()
}

/// True when `file` is Unix-only, given the `mod` lines that pull each file in. `known` keeps the
/// answers; `visiting` stops a cycle, which rustc would refuse, from recursing forever.
fn unix_only(
    file: &str,
    parents: &BTreeMap<String, Vec<(&str, bool)>>,
    known: &mut BTreeMap<String, bool>,
    visiting: &mut BTreeSet<String>,
) -> bool {
    if let Some(unix) = known.get(file) {
        return *unix;
    }
    if !visiting.insert(file.to_owned()) {
        return false;
    }
    let unix = parents.get(file).is_some_and(|lines| {
        lines
            .iter()
            .all(|(parent, unix)| *unix || unix_only(parent, parents, known, visiting))
    });
    known.insert(file.to_owned(), unix);
    unix
}

/// The file `decl` in `file` pulls in, when it is one of `scans`.
fn resolve(file: &str, decl: &ModDecl, scans: &BTreeMap<String, FileScan>) -> Option<String> {
    let crate_dir = crate_dir(file)?;
    if let Some(path) = &decl.path {
        let holder = if decl.inline.is_empty() {
            file.to_owned()
        } else {
            // A `#[path]` inside an inline module resolves against that module's folder; a
            // made-up file name there gives `paths::resolve` the folder.
            format!("{}/{}/_", module_dir(file), decl.inline.join("/"))
        };
        return paths::resolve(path, &holder, &crate_dir)
            .filter(|target| scans.contains_key(target));
    }
    let mut dir = module_dir(file);
    for name in &decl.inline {
        dir = format!("{dir}/{name}");
    }
    [
        format!("{dir}/{}.rs", decl.name),
        format!("{dir}/{}/mod.rs", decl.name),
    ]
    .into_iter()
    .find(|candidate| scans.contains_key(candidate))
}

/// `crates/<name>`, the crate folder of `file`.
fn crate_dir(file: &str) -> Option<String> {
    let mut parts = file.split('/');
    let (Some(top), Some(name)) = (parts.next(), parts.next()) else {
        return None;
    };
    Some(format!("{top}/{name}"))
}

/// The folder a `mod name;` in `file` resolves against.
fn module_dir(file: &str) -> String {
    let (dir, name) = file.rsplit_once('/').unwrap_or(("", file));
    let stem = name.strip_suffix(".rs").unwrap_or(name);
    if matches!(stem, "lib" | "main" | "mod" | "build") || is_crate_root(dir) {
        dir.to_owned()
    } else {
        format!("{dir}/{stem}")
    }
}

/// True when a file in `dir` is a crate root: `crates/<name>/tests`, `benches`, `examples` or
/// `src/bin`.
fn is_crate_root(dir: &str) -> bool {
    let parts: Vec<&str> = dir.split('/').collect();
    matches!(
        parts.as_slice(),
        [_, _, "tests" | "benches" | "examples"] | [_, _, "src", "bin"]
    )
}

#[cfg(test)]
mod tests;
