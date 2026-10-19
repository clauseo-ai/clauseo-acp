//! Running cargo for a dependency graph, for the checks that read one: `dependencies` reads each
//! checked crate's graph from `cargo tree`, `plugins` reads the workspace's resolved graph from
//! `cargo metadata`. A failed run becomes one violation at `Cargo.toml:1`, with a fix for the
//! cause cargo names.

use crate::checks::violation;
use crate::process::{self, Cmd, Program};
use crate::violation::Violation;
use crate::workspace::{self, ResolvedGraph};
use std::path::Path;

/// The root manifest, where a failed run points.
pub(crate) const ROOT_MANIFEST: &str = "Cargo.toml";

/// The stdout of `cargo <args>` run in `root`; `Err` is the violation to report when cargo
/// cannot be started or fails.
pub(crate) fn cargo_stdout(root: &Path, args: &[&str]) -> Result<String, Violation> {
    let cmd = Cmd::new(Program::Cargo, args.iter().copied());
    let quiet = process::run_quiet(&cmd, root).map_err(|error| {
        violation(
            ROOT_MANIFEST,
            1,
            format!("could not run `{cmd}`: {error}"),
            "put `cargo` on PATH: this check reads the dependency graph from cargo",
        )
    })?;
    if quiet.success {
        Ok(quiet.stdout)
    } else {
        let stderr = quiet.stderr.trim();
        Err(violation(
            ROOT_MANIFEST,
            1,
            format!("`{cmd}` failed: {stderr}"),
            failure_fix(&cmd, stderr),
        ))
    }
}

/// The fix for a failed run of `cmd` whose stderr is `stderr`, true for the cause cargo names.
fn failure_fix(cmd: &Cmd, stderr: &str) -> String {
    if stderr.contains("--locked was passed") {
        "Cargo.lock does not match the manifests and the check never rewrites it: run `cargo update --workspace` to bring it in line, then commit Cargo.lock".to_owned()
    } else if stderr.contains("--offline") || stderr.contains("offline mode") {
        "a crate is not downloaded and the check runs offline: run `cargo fetch --locked`, then run the check again".to_owned()
    } else {
        format!(
            "fix what cargo reports above until `{cmd}` succeeds: this check reads the dependency graph from it"
        )
    }
}

/// The graph cargo resolved for the whole workspace, from `cargo metadata --format-version 1
/// --all-features <flags>` with deps: all features, so a crate behind an optional dependency
/// counts; no `--filter-platform`, so every target's dependencies count.
pub(crate) fn load_resolved_graph(root: &Path, flags: &[&str]) -> Result<ResolvedGraph, Violation> {
    let mut args = vec!["metadata", "--format-version", "1", "--all-features"];
    args.extend_from_slice(flags);
    let stdout = cargo_stdout(root, &args)?;
    workspace::parse_resolved_graph(&stdout).map_err(|error| {
        violation(
            ROOT_MANIFEST,
            1,
            format!(
                "could not read the output of `cargo {}`: {error}",
                args.join(" ")
            ),
            "update xtask/src/workspace.rs to the JSON this cargo writes: this check reads the resolved graph from it",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fix_of_a_failed_run_fits_its_cause() {
        let cmd = Cmd::new(Program::Cargo, ["tree", "-p", "x"]);
        let lock = "error: the lock file /ws/Cargo.lock needs to be updated but --locked was passed to prevent this";
        assert!(failure_fix(&cmd, lock).contains("cargo update --workspace"));
        let offline = "error: no matching package named `serde` found\nAs a reminder, you're using offline mode (--offline)";
        assert!(failure_fix(&cmd, offline).contains("cargo fetch --locked"));
        assert_eq!(
            failure_fix(&cmd, "error: bad manifest"),
            "fix what cargo reports above until `cargo tree -p x` succeeds: this check reads the dependency graph from it"
        );
    }
}
