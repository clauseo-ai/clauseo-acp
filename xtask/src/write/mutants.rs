//! The files of `cargo xtask mutants`: the diff it hands cargo-mutants, and the output folder of
//! the run before.

use std::io;
use std::path::Path;

/// Writes `diff`, the branch's diff against the merge-base with origin/main, to `path`, creating
/// its folders when missing and replacing an earlier diff.
pub(crate) fn save_mutants_diff(path: &Path, diff: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, diff)
}

/// Removes `dir`, the `mutants.out` folder an earlier run of cargo-mutants left, so its
/// `outcomes.json` is never read as this run's. A missing folder is not an error.
pub(crate) fn remove_mutants_output(dir: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(dir) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}
