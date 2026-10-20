//! The cargo runs of the graph checks, and the fix for a failed one, true for the cause cargo
//! names.

use crate::checks::Failure;
use crate::process::{self, Cmd, Program};
use std::path::Path;

/// The stdout of `cargo <args>` run in `root`. `Err` is the failure to report when cargo
/// cannot be started or fails. It has no `file:line`, since cargo's error names the cause.
pub(super) fn cargo_stdout(root: &Path, args: &[&str]) -> Result<String, Failure> {
    let cmd = Cmd::new(Program::Cargo, args.iter().copied());
    let quiet = process::run_quiet(&cmd, root).map_err(|error| Failure {
        message: format!("could not run `{cmd}`: {error}"),
        fix: "put `cargo` on PATH, since this check reads the dependency graph from cargo"
            .to_owned(),
    })?;
    if quiet.success {
        Ok(quiet.stdout)
    } else {
        let stderr = quiet.stderr.trim();
        Err(Failure {
            message: format!("`{cmd}` failed: {stderr}"),
            fix: failure_fix(&cmd, stderr, root),
        })
    }
}

/// The fix for a failed run of `cmd` in the workspace at `root` whose stderr is `stderr`, true
/// for the cause cargo names. Under `--offline` cargo adds its offline note to every resolution
/// error, so a resolution error is told apart from a crate that is not downloaded before the
/// offline branch (see `resolution_fix`).
fn failure_fix(cmd: &Cmd, stderr: &str, root: &Path) -> String {
    let offline = stderr.contains("offline mode");
    if stderr.contains("--locked was passed") {
        "Cargo.lock does not match the manifests, and the check never rewrites it. Run \
         `cargo update --workspace` to bring it in line, then commit Cargo.lock"
            .to_owned()
    } else if let Some(fix) = resolution_fix(stderr, offline, root) {
        fix.to_owned()
    } else if offline || stderr.contains("--offline") {
        FETCH_FIX.to_owned()
    } else {
        format!(
            "fix what cargo reports above until `{cmd}` succeeds, since this check reads the \
             dependency graph from it"
        )
    }
}

/// The fix for a crate that is locked but not downloaded while the check runs offline.
const FETCH_FIX: &str = "a crate is not downloaded and the check runs offline. Run \
                         `cargo fetch --locked`, then run the check again";

/// The fix for a manifest that names a package or version that does not exist.
const MANIFEST_FIX: &str = "cargo could not resolve the dependency that its error above names. \
                            Correct that dependency in the manifest of the package the error \
                            says requires it, then run the check again";

/// The fix when cargo's error fits both a wrong manifest and a crate not downloaded.
const EITHER_FIX: &str = "cargo could not resolve the dependency that its error above names. If \
                          Cargo.lock lists that package, it is not downloaded and the check \
                          runs offline, so run `cargo fetch --locked`. If it does not, correct \
                          that dependency in the manifest of the package the error says \
                          requires it. Then run the check again";

/// The fix for cargo's resolution errors, or `None` for another error. Online, these errors
/// mean the manifest names a package or version that does not exist. Offline, cargo also gives
/// them for a crate that is not downloaded. A missing package is then looked up in the
/// workspace's Cargo.lock: locked means not downloaded, not locked means a wrong manifest.
/// Offline with no answer from Cargo.lock, the fix names both causes.
fn resolution_fix(stderr: &str, offline: bool, root: &Path) -> Option<&'static str> {
    const NO_PACKAGE: &str = "no matching package named `";
    let missing = stderr
        .find(NO_PACKAGE)
        .map(|at| &stderr[at + NO_PACKAGE.len()..])
        .and_then(|rest| rest.split_once('`'))
        .map(|(name, _)| name);
    if missing.is_none() && !stderr.contains("failed to select a version") {
        return None;
    }
    if !offline {
        return Some(MANIFEST_FIX);
    }
    let locked = missing
        .zip(locked_packages(root))
        .map(|(name, packages)| packages.iter().any(|package| package == name));
    Some(match locked {
        Some(true) => FETCH_FIX,
        Some(false) => MANIFEST_FIX,
        None => EITHER_FIX,
    })
}

/// The `name` of every `[[package]]` in `root/Cargo.lock`, or `None` when the file cannot be
/// read or parsed.
fn locked_packages(root: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(root.join("Cargo.lock")).ok()?;
    let lock: toml::Table = text.parse().ok()?;
    let packages = lock.get("package")?.as_array()?;
    Some(
        packages
            .iter()
            .filter_map(|package| package.get("name")?.as_str())
            .map(str::to_owned)
            .collect(),
    )
}

#[cfg(test)]
mod tests;
