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

/// The fix for `check` of gitleaks after `attempt`, on Linux with the test bin on PATH when
/// `on_path`.
fn fix_for(check: &VersionCheck, attempt: Attempt, on_path: bool) -> String {
    let machine = linux(&bin(), on_path);
    version_failure(binary("gitleaks"), check, "HOW", attempt, &machine)
        .expect("a failed check")
        .1
}

fn differs() -> VersionCheck {
    VersionCheck::Differs(other_than(binary("gitleaks").version))
}

fn missing() -> VersionCheck {
    VersionCheck::Missing("`gitleaks version` could not start: No such file or directory".into())
}

fn broken() -> VersionCheck {
    VersionCheck::Broken(String::new())
}

#[test]
fn a_plain_setup_repeats_how_to_install_as_the_fix() {
    let gitleaks = binary("gitleaks");
    let other = other_than(gitleaks.version);
    assert_eq!(
        version_failure(
            gitleaks,
            &differs(),
            "HOW",
            Attempt::None,
            &linux(&bin(), true)
        ),
        Some((
            format!(
                "gitleaks version is {other}, the pin is {}",
                gitleaks.version
            ),
            "HOW".to_owned(),
        ))
    );
    assert_eq!(fix_for(&missing(), Attempt::None, true), "HOW");
    assert_eq!(fix_for(&broken(), Attempt::None, true), "reinstall it: HOW");
}

#[test]
fn after_an_install_another_version_earlier_on_path_is_named() {
    assert_eq!(
        fix_for(&differs(), Attempt::Installed, true),
        "gitleaks was installed to /home/dev/.cargo/bin, but this shell runs another copy that \
         comes earlier on PATH: run `which -a gitleaks` to list the copies, then remove the other \
         one or put /home/dev/.cargo/bin before it on PATH"
    );
}

#[test]
fn after_an_install_to_a_bin_off_path_with_another_copy_the_fix_says_put_bin_before_it() {
    let expected = "gitleaks was installed to /home/dev/.cargo/bin, which is not on PATH, so this \
                    shell runs another copy: put /home/dev/.cargo/bin on PATH before that copy \
                    (in your shell profile, for every new shell; `which -a gitleaks` lists the \
                    copies), then run cargo xtask setup again";
    assert_eq!(fix_for(&differs(), Attempt::Installed, false), expected);
    assert_eq!(fix_for(&broken(), Attempt::Installed, false), expected);
}

#[test]
fn after_an_install_a_failing_run_with_bin_on_path_asks_which_copy_ran() {
    let file = bin().join("gitleaks").display().to_string();
    assert_eq!(
        fix_for(&broken(), Attempt::Installed, true),
        format!(
            "gitleaks was installed to /home/dev/.cargo/bin, but `gitleaks version` fails in \
             this shell: run `which -a gitleaks` to see which copy runs. If it is {file}, run \
             `{file} version` to see why it fails; otherwise remove the other copy or put \
             /home/dev/.cargo/bin before it on PATH"
        )
    );
}

#[test]
fn after_an_install_to_a_bin_off_path_the_fix_says_to_put_it_on_path() {
    let lefthook = binary("lefthook");
    let why = "`lefthook version` could not start: No such file or directory";
    let missing = VersionCheck::Missing(why.into());
    let machine = linux(&bin(), false);
    assert_eq!(
        version_failure(lefthook, &missing, "HOW", Attempt::Installed, &machine),
        Some((
            format!("lefthook was installed, but this shell cannot run it ({why})"),
            "lefthook was installed to /home/dev/.cargo/bin, which is not on PATH: put \
             /home/dev/.cargo/bin on PATH (in your shell profile, for every new shell), then run \
             cargo xtask setup again"
                .into(),
        ))
    );
    assert_eq!(
        version_failure(lefthook, &missing, "HOW", Attempt::None, &machine),
        Some((format!("lefthook is not installed ({why})"), "HOW".into()))
    );
}

#[test]
fn after_an_install_to_a_bin_on_path_a_binary_that_cannot_start_names_the_file() {
    let file = bin().join("gitleaks").display().to_string();
    assert_eq!(
        fix_for(&missing(), Attempt::Installed, true),
        format!(
            "gitleaks was installed to /home/dev/.cargo/bin, which is on PATH, but it could not be \
             started: run `{file}` to see why, then run cargo xtask setup again"
        )
    );
}

#[test]
fn after_an_install_on_windows_the_fix_names_the_winget_links_folder() {
    let windows = Machine {
        os: "windows",
        ..linux(&bin(), true)
    };
    let fix = version_failure(
        binary("gitleaks"),
        &differs(),
        "HOW",
        Attempt::Installed,
        &windows,
    );
    assert_eq!(
        fix.expect("a failed check").1,
        "gitleaks was installed to winget's Links folder, usually \
         %LOCALAPPDATA%\\Microsoft\\WinGet\\Links, but this shell does not run that copy. \
         Another copy earlier on PATH, or a PATH not yet refreshed, is the likely cause: run \
         `where gitleaks` to list the copies, or open a new shell and run cargo xtask setup again"
    );
}

#[test]
fn after_a_failed_install_the_fix_points_at_that_failure_instead_of_repeating_how() {
    for check in [differs(), missing(), broken()] {
        assert_eq!(
            fix_for(&check, Attempt::Failed, true),
            "the install of gitleaks above failed: do what its fix says, then run cargo xtask \
             setup again",
            "{check:?}"
        );
    }
}

#[test]
fn a_matching_version_has_no_failure_whatever_was_attempted() {
    for attempt in [Attempt::None, Attempt::Installed, Attempt::Failed] {
        let machine = linux(&bin(), true);
        let gitleaks = binary("gitleaks");
        assert_eq!(
            version_failure(gitleaks, &VersionCheck::Matches, "HOW", attempt, &machine),
            None
        );
    }
}

#[test]
fn a_bin_with_a_trailing_slash_on_path_is_on_path() {
    let path = std::env::join_paths(["/usr/bin", "/home/dev/.cargo/bin/"]).unwrap();
    assert!(on_path(&bin(), &path));
    let elsewhere = std::env::join_paths(["/usr/bin", "/home/dev/.cargo"]).unwrap();
    assert!(!on_path(&bin(), &elsewhere));
}

#[cfg(unix)]
#[test]
fn a_bin_reached_through_a_symlink_is_on_path() {
    let dir = crate::scratch::Scratch::new("setup-version-symlink");
    let real = dir.join("real-cargo-home");
    std::fs::create_dir_all(real.join("bin")).unwrap();
    std::os::unix::fs::symlink(&real, dir.join("linked-cargo-home")).unwrap();
    let path = std::env::join_paths([real.join("bin")]).unwrap();
    assert!(on_path(&dir.join("linked-cargo-home").join("bin"), &path));
    let linked = std::env::join_paths([dir.join("linked-cargo-home").join("bin")]).unwrap();
    assert!(on_path(&real.join("bin"), &linked));
    let other = std::env::join_paths([dir.to_path_buf()]).unwrap();
    assert!(!on_path(&real.join("bin"), &other));
}
