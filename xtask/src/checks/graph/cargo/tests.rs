//! Tests of the fix for a failed cargo run.

use super::*;

/// The reminder cargo adds to every resolution error under `--offline`.
const OFFLINE_NOTE: &str = "note: offline mode (via `--offline`) can sometimes cause surprising \
                            resolution failures\nhelp: if this error is too confusing you may \
                            wish to retry without `--offline`";

/// A workspace root with no Cargo.lock, so `failure_fix` cannot look a package up.
fn no_lock(name: &str) -> crate::scratch::Scratch {
    crate::scratch::Scratch::new(name)
}

/// A workspace root whose Cargo.lock locks `serde_json` and `serde`.
fn serde_lock(name: &str) -> crate::scratch::Scratch {
    let dir = crate::scratch::Scratch::new(name);
    let lock = "version = 4\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.228\"\n\n\
                [[package]]\nname = \"serde_json\"\nversion = \"1.0.140\"\n";
    std::fs::write(dir.join("Cargo.lock"), lock).expect("write Cargo.lock");
    dir
}

/// Cargo's error for a package it found in no registry index it can read, with `note` after it.
fn no_package(name: &str, note: &str) -> String {
    format!(
        "error: no matching package named `{name}` found\nlocation searched: crates.io index\n\
         required by package `x v0.1.0 (/ws/x)`\n{note}"
    )
}

#[test]
fn the_fix_of_a_failed_run_fits_its_cause() {
    let cmd = Cmd::new(Program::Cargo, ["tree", "-p", "x"]);
    let root = no_lock("graph-cargo-fits");
    let lock = "error: the lock file /ws/Cargo.lock needs to be updated but --locked was passed to prevent this";
    assert!(failure_fix(&cmd, lock, &root).contains("cargo update --workspace"));
    let missing = "error: failed to download `serde v1.0.0`\n\nCaused by:\n  attempting to make \
                   an HTTP request, but --offline was specified";
    assert_eq!(failure_fix(&cmd, missing, &root), FETCH_FIX);
    assert_eq!(
        failure_fix(&cmd, "error: bad manifest", &root),
        "fix what cargo reports above until `cargo tree -p x` succeeds, since this check reads \
         the dependency graph from it"
    );
}

/// Offline with an empty cargo cache, a locked package is missing from the index too. Cargo.lock
/// lists it, so it is not downloaded.
#[test]
fn offline_a_missing_package_that_cargo_lock_lists_is_not_downloaded() {
    let cmd = Cmd::new(Program::Cargo, ["tree", "-p", "x", "--locked", "--offline"]);
    let error = no_package("serde_json", OFFLINE_NOTE);
    assert_eq!(
        failure_fix(&cmd, &error, &serde_lock("graph-cargo-locked")),
        FETCH_FIX
    );
}

/// Offline, a missing package that Cargo.lock does not list is a manifest naming a package
/// that does not exist.
#[test]
fn offline_a_missing_package_that_cargo_lock_does_not_list_is_a_wrong_manifest() {
    let cmd = Cmd::new(Program::Cargo, ["tree", "-p", "x", "--offline"]);
    let error = no_package("zzz-not-a-crate-xq", OFFLINE_NOTE);
    assert_eq!(
        failure_fix(&cmd, &error, &serde_lock("graph-cargo-unlocked")),
        MANIFEST_FIX
    );
}

/// Online, a resolution error is always a wrong manifest. Offline with no Cargo.lock to ask, or
/// for a version cargo could not select, the fix names both causes.
#[test]
fn a_resolution_error_gets_the_manifest_fix_only_when_that_is_the_cause() {
    let cmd = Cmd::new(Program::Cargo, ["tree", "-p", "x"]);
    let lock = serde_lock("graph-cargo-only-lock");
    let none = no_lock("graph-cargo-only-none");
    let no_version = |note: &str| {
        format!(
            "error: failed to select a version for the requirement `serde = \"^999\"`\n\
             candidate versions found which didn't match: 1.0.228\n{note}"
        )
    };
    assert_eq!(
        failure_fix(&cmd, &no_package("serde_json", ""), &lock),
        MANIFEST_FIX
    );
    assert_eq!(failure_fix(&cmd, &no_version(""), &none), MANIFEST_FIX);
    assert_eq!(
        failure_fix(&cmd, &no_package("serde_json", OFFLINE_NOTE), &none),
        EITHER_FIX
    );
    assert_eq!(
        failure_fix(&cmd, &no_version(OFFLINE_NOTE), &lock),
        EITHER_FIX
    );
}
