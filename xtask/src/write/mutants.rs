//! The files of `cargo xtask mutants`: the diff it hands cargo-mutants, and the output folder of
//! the run before.

use std::io;
use std::path::Path;

/// Where `cargo xtask mutants` saves the diff it hands cargo-mutants, from the repo root. The only
/// file `save_mutants_diff` writes.
pub(crate) const MUTANTS_DIFF: &str = "target/xtask/mutants.diff";

/// The output folder cargo-mutants writes for `cargo xtask mutants`, from the repo root. The only
/// folder `remove_mutants_output` removes.
pub(crate) const MUTANTS_OUTPUT: &str = "target/xtask/mutants.out";

/// Writes `diff`, the branch's diff against the merge-base with origin/main, to `MUTANTS_DIFF`
/// under `root`, creating its folders when missing and replacing an earlier diff.
pub(crate) fn save_mutants_diff(root: &Path, diff: &str) -> io::Result<()> {
    let path = root.join(MUTANTS_DIFF);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, diff)
}

/// Removes `dir`, a build dir cargo-mutants left in the temp dir when it was stopped. Refuses any
/// folder whose name is not `cargo-mutants-*.tmp`, the names cargo-mutants gives them.
pub(crate) fn remove_mutants_build_dir(dir: &Path) -> io::Result<()> {
    let name = dir.file_name().map(|name| name.to_string_lossy());
    let ours =
        name.is_some_and(|name| name.starts_with("cargo-mutants-") && name.ends_with(".tmp"));
    if !ours {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a cargo-mutants build dir (cargo-mutants-*.tmp)",
        ));
    }
    std::fs::remove_dir_all(dir)
}

/// Removes `MUTANTS_OUTPUT` under `root`, the folder an earlier run of cargo-mutants left, so its
/// `outcomes.json` is never read as this run's. A missing folder is not an error.
pub(crate) fn remove_mutants_output(root: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(root.join(MUTANTS_OUTPUT)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;

    #[test]
    fn the_old_output_folder_is_removed_and_a_missing_one_is_not_an_error() {
        let dir = Scratch::new("write-mutants-output");
        let out = dir.join(MUTANTS_OUTPUT);
        Scratch::create_dir_all(out.join("log")).expect("make an old mutants.out");
        remove_mutants_output(&dir).expect("remove it");
        assert!(!out.exists());
        assert!(dir.join("target/xtask").is_dir(), "only mutants.out goes");
        remove_mutants_output(&dir).expect("a missing folder is not an error");
        // Any other error still is: here the path is a file, not a folder.
        Scratch::write(&out, "x").expect("write a file in its place");
        assert!(remove_mutants_output(&dir).is_err());
    }

    #[test]
    fn only_a_folder_named_like_a_cargo_mutants_build_dir_is_removed() {
        let dir = Scratch::new("write-mutants-build-dir");
        let build = dir.join("cargo-mutants-tree-abc.tmp");
        Scratch::create_dir_all(build.join("target")).expect("make a build dir");
        remove_mutants_build_dir(&build).expect("remove it");
        assert!(!build.exists());
        let other = dir.join("cargo-target.tmp");
        Scratch::create_dir_all(&other).expect("make another folder");
        let refused = remove_mutants_build_dir(&other).expect_err("refused");
        assert_eq!(refused.kind(), io::ErrorKind::InvalidInput);
        assert!(other.is_dir());
    }
}
