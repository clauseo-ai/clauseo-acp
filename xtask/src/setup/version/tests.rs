//! Tests for `version.rs`: the version check and the text of each failure.

use super::*;
use crate::setup::tests::{binary, linux};
use std::path::PathBuf;

/// A version other than `pin`.
fn other_than(pin: &str) -> String {
    format!("{pin}.9")
}

#[test]
fn version_check_wants_the_exact_pin() {
    let pin = binary("gitleaks").version;
    let other = other_than(pin);
    assert_eq!(
        compare_version(pin, Ok((true, format!("{pin}\n"), String::new()))),
        VersionCheck::Matches
    );
    assert_eq!(
        compare_version(pin, Ok((true, format!("{other}\n"), String::new()))),
        VersionCheck::Differs(other)
    );
    assert_eq!(
        compare_version(pin, Ok((false, String::new(), "boom\n".into()))),
        VersionCheck::Broken("boom".into())
    );
    assert_eq!(
        compare_version(pin, Err("not found".into())),
        VersionCheck::Missing("not found".into())
    );
}

/// A cargo bin directory for the tests; nothing is written there.
fn bin() -> PathBuf {
    PathBuf::from("/home/dev/.cargo/bin")
}

#[test]
fn a_plain_setup_repeats_how_to_install_as_the_fix() {
    let gitleaks = binary("gitleaks");
    let other = other_than(gitleaks.version);
    let how = "run `cargo xtask setup --binaries`, which downloads the asset";
    assert_eq!(
        version_failure(
            gitleaks,
            &VersionCheck::Differs(other.clone()),
            how,
            false,
            &linux(&bin(), true),
        ),
        Some((
            format!(
                "gitleaks version is {other}, the pin is {}",
                gitleaks.version
            ),
            how.to_owned(),
        ))
    );
}

#[test]
fn after_an_install_a_mismatch_names_where_it_went_and_the_likely_cause() {
    let gitleaks = binary("gitleaks");
    let other = other_than(gitleaks.version);
    let found = VersionCheck::Differs(other.clone());
    let message = format!(
        "gitleaks version is {other}, the pin is {}",
        gitleaks.version
    );
    assert_eq!(
        version_failure(gitleaks, &found, "how", true, &linux(&bin(), true)),
        Some((
            message.clone(),
            "gitleaks was installed to /home/dev/.cargo/bin, but this shell runs another copy \
             that comes earlier on PATH: run `which -a gitleaks` to list the copies, then remove \
             the other one or put /home/dev/.cargo/bin before it on PATH"
                .into(),
        ))
    );
    let windows = Machine {
        os: "windows",
        ..linux(&bin(), true)
    };
    assert_eq!(
        version_failure(gitleaks, &found, "how", true, &windows),
        Some((
            message,
            "gitleaks was installed to winget's Links folder, usually \
             %LOCALAPPDATA%\\Microsoft\\WinGet\\Links, but this shell does not run that copy. \
             Another copy earlier on PATH, or a PATH not yet refreshed, is the likely cause: run \
             `where gitleaks` to list the copies, or open a new shell and run cargo xtask setup \
             again"
                .into(),
        ))
    );
}

#[test]
fn after_an_install_to_a_bin_off_path_the_fix_says_to_put_it_on_path() {
    let lefthook = binary("lefthook");
    let why = "`lefthook version` could not start: No such file or directory";
    let missing = VersionCheck::Missing(why.into());
    assert_eq!(
        version_failure(lefthook, &missing, "how", true, &linux(&bin(), false)),
        Some((
            format!("lefthook was installed, but this shell cannot run it ({why})"),
            "lefthook was installed to /home/dev/.cargo/bin, which is not on PATH: put \
             /home/dev/.cargo/bin on PATH (in your shell profile, for every new shell), then run \
             cargo xtask setup again"
                .into(),
        ))
    );
    assert_eq!(
        version_failure(lefthook, &missing, "how", false, &linux(&bin(), false)),
        Some((format!("lefthook is not installed ({why})"), "how".into()))
    );
}

#[test]
fn a_bin_with_a_trailing_slash_on_path_is_on_path() {
    let path = std::env::join_paths(["/usr/bin", "/home/dev/.cargo/bin/"]).unwrap();
    assert!(on_path(&bin(), &path));
    let elsewhere = std::env::join_paths(["/usr/bin", "/home/dev/.cargo"]).unwrap();
    assert!(!on_path(&bin(), &elsewhere));
}

#[test]
fn a_binary_that_broke_after_an_install_is_reinstalled_as_before() {
    let gitleaks = binary("gitleaks");
    let broken = VersionCheck::Broken(String::new());
    assert_eq!(
        version_failure(gitleaks, &broken, "run it", true, &linux(&bin(), true)),
        Some((
            "gitleaks is installed, but `gitleaks version` failed: no output".into(),
            "reinstall it: run it".into(),
        ))
    );
}

#[test]
fn a_matching_version_has_no_failure_either_way() {
    let gitleaks = binary("gitleaks");
    for installed_now in [false, true] {
        assert_eq!(
            version_failure(
                gitleaks,
                &VersionCheck::Matches,
                "how",
                installed_now,
                &linux(&bin(), true),
            ),
            None
        );
    }
}
