//! The cargo runs of the graph checks, and the fix for a failed one, true for the cause cargo
//! names.

use super::Fix;
use crate::checks::Failure;
use crate::process::{self, Cmd, Program};
use semver::{Version, VersionReq};
use std::path::Path;

/// The stdout of `cargo <args>` run in `root`. `Err` is the failure to report when cargo
/// cannot be started or fails. It has no `file:line`, since cargo's error names the cause.
pub(super) fn cargo_stdout(root: &Path, args: &[&str]) -> Result<String, Failure> {
    let cmd = Cmd::new(Program::Cargo, args.iter().copied());
    let quiet = process::run_quiet(&cmd, root).map_err(|error| Failure {
        message: format!("could not run `{cmd}`: {error}"),
        fix: Fix::CargoOnPath.into(),
    })?;
    if quiet.success {
        Ok(quiet.stdout)
    } else {
        let stderr = quiet.stderr.trim();
        Err(Failure {
            message: format!("`{cmd}` failed: {stderr}"),
            fix: failure_fix(&cmd, stderr, root).into(),
        })
    }
}

/// The fix for a failed run of `cmd` in the workspace at `root` whose stderr is `stderr`, true
/// for the cause cargo names. Under `--offline` cargo adds its offline note to every resolution
/// error, so a resolution error is told apart from a crate that is not downloaded before the
/// offline branch (see `resolution_fix`).
fn failure_fix(cmd: &Cmd, stderr: &str, root: &Path) -> Fix {
    let offline = stderr.contains("offline mode");
    if stderr.contains("--locked was passed") {
        Fix::Lockfile
    } else if let Some(fix) = resolution_fix(stderr, offline, root) {
        fix
    } else if offline || stderr.contains("--offline") {
        Fix::Fetch
    } else {
        Fix::CargoReports {
            command: cmd.to_string(),
        }
    }
}

/// The fix for cargo's resolution errors, or `None` for another error. Online, these errors
/// mean the manifest names a package or version that does not exist. Offline, cargo also gives
/// them for a crate that is not downloaded. The workspace's Cargo.lock then decides. A registry
/// entry with the name cargo could not resolve, at a version the requirement allows, means the
/// crate is not downloaded. No such entry means a wrong manifest. Offline, when Cargo.lock or
/// cargo's line cannot be read, the fix names both causes.
fn resolution_fix(stderr: &str, offline: bool, root: &Path) -> Option<Fix> {
    let wanted = unresolved(stderr)?;
    if !offline {
        return Some(Fix::Manifest);
    }
    let locked = match wanted {
        Unresolved::Known { name, req } => locked_packages(root).map(|packages| {
            packages
                .iter()
                .any(|(locked, version)| *locked == name && req.matches(version))
        }),
        Unresolved::Unreadable => None,
    };
    Some(match locked {
        Some(true) => Fix::Fetch,
        Some(false) => Fix::Manifest,
        None => Fix::Either,
    })
}

/// The dependency that a cargo resolution error names.
enum Unresolved {
    /// Its name and requirement. A missing package allows any version.
    Known { name: String, req: VersionReq },
    /// A requirement line whose name or requirement cannot be read.
    Unreadable,
}

/// The dependency that cargo's resolution error in `stderr` could not resolve, or `None` when
/// `stderr` has no resolution error.
fn unresolved(stderr: &str) -> Option<Unresolved> {
    const NO_PACKAGE: &str = "no matching package named `";
    const NO_VERSION: &str = "failed to select a version for the requirement `";
    if let Some(rest) = after(stderr, NO_PACKAGE) {
        let name = rest.split_once('`').map(|(name, _)| name.to_owned());
        return Some(
            name.map_or(Unresolved::Unreadable, |name| Unresolved::Known {
                name,
                req: VersionReq::STAR,
            }),
        );
    }
    let rest = after(stderr, NO_VERSION)?;
    let known = rest.split_once('`').and_then(|(line, _)| requirement(line));
    Some(
        known.map_or(Unresolved::Unreadable, |(name, req)| Unresolved::Known {
            name,
            req,
        }),
    )
}

/// The text of `haystack` after the first `needle`.
fn after<'a>(haystack: &'a str, needle: &str) -> Option<&'a str> {
    haystack
        .find(needle)
        .map(|at| &haystack[at + needle.len()..])
}

/// The name and requirement of cargo's `serde_json = "^99"`, which is a TOML key and string.
fn requirement(line: &str) -> Option<(String, VersionReq)> {
    let table: toml::Table = line.parse().ok()?;
    let mut entries = table.into_iter();
    let (name, req) = entries.next()?;
    if entries.next().is_some() {
        return None;
    }
    let req = VersionReq::parse(req.as_str()?).ok()?;
    Some((name, req))
}

/// The name and version of every `[[package]]` in `root/Cargo.lock` whose source is a
/// registry, or `None` when the file cannot be read or parsed. A path package has no source,
/// so it is never listed.
fn locked_packages(root: &Path) -> Option<Vec<(String, Version)>> {
    let text = std::fs::read_to_string(root.join("Cargo.lock")).ok()?;
    let lock: toml::Table = text.parse().ok()?;
    let packages = lock.get("package")?.as_array()?;
    packages
        .iter()
        .filter(|package| {
            package
                .get("source")
                .and_then(toml::Value::as_str)
                .is_some_and(|source| {
                    source.starts_with("registry+") || source.starts_with("sparse+")
                })
        })
        .map(|package| {
            let name = package.get("name")?.as_str()?.to_owned();
            let version = Version::parse(package.get("version")?.as_str()?).ok()?;
            Some((name, version))
        })
        .collect()
}

#[cfg(test)]
mod tests;
