//! The build dirs cargo-mutants makes in the temp dir, and those a run stopped at the cap left.

use super::{Fix, fail};
use crate::output::Printer;
use crate::write;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The start of the name of each build dir cargo-mutants makes for the tree at `root`:
/// `cargo-mutants-` and the name of its folder (cargo-mutants 27.1.0, `BuildDir::copy_from` in
/// `src/build_dir.rs`). tempfile then adds a random part and `.tmp` (`src/copy_tree.rs`).
pub(super) fn build_dir_prefix(root: &Path) -> String {
    let name = root
        .file_name()
        .map_or_else(|| "unnamed".into(), |name| name.to_string_lossy());
    format!("cargo-mutants-{name}-")
}

/// The build dirs of cargo-mutants runs of the tree at `root` now in `temp`.
pub(super) fn build_dirs(temp: &Path, root: &Path) -> BTreeSet<PathBuf> {
    let prefix = build_dir_prefix(root);
    let Ok(entries) = std::fs::read_dir(temp) else {
        return BTreeSet::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(&prefix) && name.ends_with(".tmp") && entry.path().is_dir()
        })
        .map(|entry| entry.path())
        .collect()
}

/// After the cap: removes the build dirs that appeared in `temp` during the run (those not in
/// `before`). cargo-mutants deletes them itself when it stops, but the door's SIGKILL, after its
/// grace period, can land before it has: one 20-minute run left four, of 0.5 to 0.7 GB each.
pub(super) fn remove_left_build_dirs<W: Write>(
    out: &mut Printer<W>,
    temp: &Path,
    root: &Path,
    before: &BTreeSet<PathBuf>,
) {
    let left: Vec<PathBuf> = build_dirs(temp, root).difference(before).cloned().collect();
    let mut removed = 0;
    for dir in &left {
        let name = dir.file_name().unwrap_or_default().to_string_lossy();
        match write::remove_in(temp, &name) {
            Ok(()) => removed += 1,
            Err(error) => {
                let message = format!(
                    "could not remove {}, a build dir cargo-mutants left when it was stopped: \
                     {error}",
                    dir.display()
                );
                fail(out, &message, &Fix::RemoveByHand);
            }
        }
    }
    if removed > 0 {
        let noun = if removed == 1 { "dir" } else { "dirs" };
        out.note(format!(
            "removed {removed} build {noun} cargo-mutants left in {} when it was stopped",
            temp.display()
        ));
    }
}
