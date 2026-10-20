//! Tests of the fix for a failed cargo run.

use super::*;

/// The reminder cargo adds to every resolution error under `--offline`.
const OFFLINE_NOTE: &str = "note: offline mode (via `--offline`) can sometimes cause surprising \
                            resolution failures\nhelp: if this error is too confusing you may \
                            wish to retry without `--offline`";

/// A Cargo.lock that locks `serde` and `serde_json` from crates.io and the path package
/// `clauseo-desktop`, which has no source.
const LOCK: &str = "version = 4\n\n\
                    [[package]]\nname = \"clauseo-desktop\"\nversion = \"0.0.0\"\n\n\
                    [[package]]\nname = \"serde\"\nversion = \"1.0.228\"\n\
                    source = \"registry+https://github.com/rust-lang/crates.io-index\"\n\n\
                    [[package]]\nname = \"serde_json\"\nversion = \"1.0.154\"\n\
                    source = \"registry+https://github.com/rust-lang/crates.io-index\"\n";

/// A workspace root whose Cargo.lock holds `lock`.
fn root_with_lock(name: &str, lock: &str) -> crate::scratch::Scratch {
    let dir = crate::scratch::Scratch::new(name);
    std::fs::write(dir.join("Cargo.lock"), lock).expect("write Cargo.lock");
    dir
}

/// Cargo's error for a package it found in no registry index it can read, with `note` after
/// it, as `cargo tree --locked --offline` prints it.
fn no_package(name: &str, note: &str) -> String {
    format!(
        "error: no matching package named `{name}` found\nlocation searched: crates.io index\n\
         required by package `xtask v0.0.0 (/ws/xtask)`\n{note}"
    )
}

/// Cargo's error for a requirement that no version it can read fits, with `note` after it, as
/// `cargo tree --locked --offline` prints it.
fn no_version(requirement: &str, note: &str) -> String {
    format!(
        "error: failed to select a version for the requirement `{requirement}`\n\
         candidate versions found which didn't match: 1.0.154, 1.0.151, 1.0.150, ...\n\
         location searched: crates.io index\n\
         required by package `clauseo-desktop v0.0.0 (/ws/crates/desktop)`\n{note}"
    )
}

#[test]
fn the_fix_of_a_failed_run_fits_its_cause() {
    let cmd = Cmd::new(Program::Cargo, ["tree", "-p", "x"]);
    let root = crate::scratch::Scratch::new("graph-cargo-fits");
    let lock = "error: the lock file /ws/Cargo.lock needs to be updated but --locked was passed to prevent this";
    assert!(
        failure_fix(&cmd, lock, &root)
            .to_string()
            .contains("cargo update --workspace")
    );
    let missing = "error: failed to download `serde v1.0.0`\n\nCaused by:\n  attempting to make \
                   an HTTP request, but --offline was specified";
    assert_eq!(failure_fix(&cmd, missing, &root), Fix::Fetch);
    assert_eq!(
        failure_fix(&cmd, "error: bad manifest", &root),
        Fix::CargoReports {
            command: "cargo tree -p x".to_owned()
        }
    );
}

/// Online, a resolution error is always a wrong manifest, even for a package Cargo.lock lists.
#[test]
fn online_a_resolution_error_is_a_wrong_manifest() {
    let cmd = Cmd::new(Program::Cargo, ["tree", "-p", "x"]);
    let root = root_with_lock("graph-cargo-online", LOCK);
    assert_eq!(
        failure_fix(&cmd, &no_package("serde_json", ""), &root),
        Fix::Manifest
    );
    assert_eq!(
        failure_fix(&cmd, &no_version("serde_json = \"^1.0.100\"", ""), &root),
        Fix::Manifest
    );
}

/// Offline, a missing package with a registry entry in Cargo.lock is not downloaded. A path
/// package of that name has no source, so it is a wrong manifest.
#[test]
fn offline_a_missing_package_is_not_downloaded_only_when_a_registry_locks_it() {
    let cmd = Cmd::new(Program::Cargo, ["tree", "--locked", "--offline"]);
    let root = root_with_lock("graph-cargo-missing", LOCK);
    assert_eq!(
        failure_fix(&cmd, &no_package("serde_json", OFFLINE_NOTE), &root),
        Fix::Fetch
    );
    assert_eq!(
        failure_fix(&cmd, &no_package("clauseo-desktop", OFFLINE_NOTE), &root),
        Fix::Manifest
    );
}

/// Offline, a requirement that a locked registry version fits is a crate not downloaded. One
/// that no locked version fits is a wrong manifest.
#[test]
fn offline_a_version_is_not_downloaded_only_when_a_locked_version_fits() {
    let cmd = Cmd::new(Program::Cargo, ["tree", "--locked", "--offline"]);
    let root = root_with_lock("graph-cargo-version", LOCK);
    assert_eq!(
        failure_fix(
            &cmd,
            &no_version("serde_json = \"^1.0.100\"", OFFLINE_NOTE),
            &root
        ),
        Fix::Fetch
    );
    assert_eq!(
        failure_fix(
            &cmd,
            &no_version("serde_json = \"^99\"", OFFLINE_NOTE),
            &root
        ),
        Fix::Manifest
    );
}

/// Offline, when Cargo.lock cannot be read or parsed, or cargo's requirement cannot be read,
/// the fix names both causes.
#[test]
fn offline_without_an_answer_the_fix_names_both_causes() {
    let cmd = Cmd::new(Program::Cargo, ["tree", "--locked", "--offline"]);
    let error = no_package("serde_json", OFFLINE_NOTE);
    let missing = crate::scratch::Scratch::new("graph-cargo-no-lock");
    assert_eq!(failure_fix(&cmd, &error, &missing), Fix::Either);
    let broken = root_with_lock("graph-cargo-bad-lock", "[[package]\nname = ");
    assert_eq!(failure_fix(&cmd, &error, &broken), Fix::Either);
    let locked = root_with_lock("graph-cargo-bad-req", LOCK);
    let unreadable = no_version("serde_json = \"not a req\"", OFFLINE_NOTE);
    assert_eq!(failure_fix(&cmd, &unreadable, &locked), Fix::Either);
}
