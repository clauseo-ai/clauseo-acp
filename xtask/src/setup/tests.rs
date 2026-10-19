//! Tests for `setup.rs`: the pins, the install commands, the version check and the install loop.

use super::*;
use crate::scratch::Scratch;
use std::path::PathBuf;

fn binary(name: &str) -> &'static Binary {
    BINARIES.iter().find(|b| b.name == name).unwrap()
}

#[test]
fn the_cargo_tools_are_pinned() {
    let pins: Vec<String> = TOOLS
        .iter()
        .map(|t| format!("{} {}", t.name, t.version))
        .collect();
    assert_eq!(
        pins,
        [
            "cargo-nextest 0.9.148",
            "cargo-deny 0.20.2",
            "cargo-shear 1.14.0",
            "typos-cli 1.51.1",
            "taplo-cli 0.10.0",
        ]
    );
}

#[test]
fn install_commands_fetch_the_pinned_release_for_each_os() {
    let gitleaks = binary("gitleaks");
    let lefthook = binary("lefthook");
    assert_eq!(
        install_command(gitleaks, "macos", "aarch64"),
        r#"curl -sSfL https://github.com/gitleaks/gitleaks/releases/download/v8.30.1/gitleaks_8.30.1_darwin_arm64.tar.gz | tar -xz -C "${CARGO_HOME:-$HOME/.cargo}/bin" gitleaks"#
    );
    assert!(
        install_command(gitleaks, "linux", "x86_64").contains("gitleaks_8.30.1_linux_x64.tar.gz")
    );
    assert_eq!(
        install_command(lefthook, "macos", "x86_64"),
        r#"curl -sSfL -o "${CARGO_HOME:-$HOME/.cargo}/bin/lefthook" https://github.com/evilmartians/lefthook/releases/download/v2.2.1/lefthook_2.2.1_MacOS_x86_64 && chmod +x "${CARGO_HOME:-$HOME/.cargo}/bin/lefthook""#
    );
    assert!(install_command(lefthook, "linux", "aarch64").contains("lefthook_2.2.1_Linux_arm64"));
    assert_eq!(
        install_command(gitleaks, "windows", "aarch64"),
        "winget install --id Gitleaks.Gitleaks --version 8.30.1 --exact --accept-source-agreements --accept-package-agreements --disable-interactivity"
    );
    assert_eq!(
        install_command(lefthook, "windows", "x86_64"),
        "winget install --id evilmartians.lefthook --version 2.2.1 --exact --accept-source-agreements --accept-package-agreements --disable-interactivity"
    );
    assert_eq!(
        install_command(lefthook, "freebsd", "x86_64"),
        "download v2.2.1 for freebsd-x86_64 from https://github.com/evilmartians/lefthook/releases and put it on PATH"
    );
}

#[test]
fn version_check_wants_the_exact_pin() {
    assert_eq!(
        compare_version("8.30.1", Ok((true, "8.30.1\n".into(), String::new()))),
        VersionCheck::Matches
    );
    assert_eq!(
        compare_version("2.2.1", Ok((true, "2.2.2\n".into(), String::new()))),
        VersionCheck::Differs("2.2.2".into())
    );
    assert_eq!(
        compare_version("2.2.1", Ok((false, String::new(), "boom\n".into()))),
        VersionCheck::Broken("boom".into())
    );
    assert_eq!(
        compare_version("2.2.1", Err("not found".into())),
        VersionCheck::Missing("not found".into())
    );
}

/// The platforms `--binaries` must install on: macOS, Linux and Windows, on both arches.
const PLATFORMS: [(&str, &str); 6] = [
    ("macos", "aarch64"),
    ("macos", "x86_64"),
    ("linux", "aarch64"),
    ("linux", "x86_64"),
    ("windows", "x86_64"),
    ("windows", "aarch64"),
];

/// The command the install of `binary` must run on `os`/`arch`, built from the pin.
fn expected_cmd(binary: &Binary, os: &str, arch: &str) -> Cmd {
    if os == "windows" {
        winget(binary)
    } else {
        let script = install_command(binary, os, arch);
        Cmd::new(Program::Named("sh"), ["-c", script.as_str()])
    }
}

/// The failure line for a failed install of `text`, the command that runs it.
fn fix_for(text: &str) -> String {
    format!("    fix: run `{text}` by hand and check its output, then run cargo xtask setup again")
}

#[test]
fn the_printed_winget_command_is_the_one_that_runs() {
    for binary in BINARIES {
        for arch in ["x86_64", "aarch64"] {
            let shown = install_command(binary, "windows", arch);
            let runs = install_cmd(binary, "windows", arch).expect("winget runs on windows");
            assert_eq!(shown, runs.to_string());
            assert_eq!(runs, winget(binary));
        }
    }
}

#[test]
fn install_runs_the_pinned_command_for_each_os_and_arch() {
    for (os, arch) in PLATFORMS {
        let dir = Scratch::new("setup-install-each");
        let mut ran = Vec::new();
        let mut runner = |cmd: &Cmd, root: &Path| -> io::Result<bool> {
            ran.push((cmd.clone(), root.to_path_buf()));
            Ok(true)
        };
        let mut out = Printer::new(Vec::new(), false);
        let installed = install_binaries(&mut out, BINARIES, os, arch, &dir, &mut runner);
        assert_eq!(installed, vec![true; BINARIES.len()], "{os}-{arch}");
        let expected: Vec<(Cmd, PathBuf)> = BINARIES
            .iter()
            .map(|b| (expected_cmd(b, os, arch), dir.to_path_buf()))
            .collect();
        assert_eq!(ran, expected, "{os}-{arch}");
    }
}

#[test]
fn a_failed_install_names_the_command_as_the_fix() {
    let dir = Scratch::new("setup-install-exit");
    let gitleaks = binary("gitleaks");
    let text = install_command(gitleaks, "macos", "aarch64");
    let mut runner = |_: &Cmd, _: &Path| -> io::Result<bool> { Ok(false) };
    let mut out = Printer::new(Vec::new(), false);
    let installed = install_binaries(
        &mut out,
        std::slice::from_ref(gitleaks),
        "macos",
        "aarch64",
        &dir,
        &mut runner,
    );
    assert_eq!(installed, [false]);
    let printed = String::from_utf8(out.into_inner()).expect("utf-8");
    assert_eq!(
        printed,
        format!(
            "installing gitleaks 8.30.1: {text}\ninstalling gitleaks failed\n{}\n",
            fix_for(&text)
        )
    );
}

#[test]
fn a_command_that_cannot_start_names_the_command_as_the_fix() {
    let dir = Scratch::new("setup-install-start");
    let lefthook = binary("lefthook");
    let text = install_command(lefthook, "linux", "x86_64");
    let mut runner = |_: &Cmd, _: &Path| -> io::Result<bool> {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "No such file or directory",
        ))
    };
    let mut out = Printer::new(Vec::new(), false);
    let installed = install_binaries(
        &mut out,
        std::slice::from_ref(lefthook),
        "linux",
        "x86_64",
        &dir,
        &mut runner,
    );
    assert_eq!(installed, [false]);
    let printed = String::from_utf8(out.into_inner()).expect("utf-8");
    assert_eq!(
        printed,
        format!(
            "installing lefthook 2.2.1: {text}\n\
                 could not start sh to install lefthook: No such file or directory\n{}\n",
            fix_for(&text)
        )
    );
}

#[test]
fn a_platform_with_no_release_runs_nothing_and_fails_with_the_download_text() {
    let dir = Scratch::new("setup-install-no-asset");
    let mut calls = 0;
    let mut runner = |_: &Cmd, _: &Path| -> io::Result<bool> {
        calls += 1;
        Ok(true)
    };
    let mut out = Printer::new(Vec::new(), false);
    let installed = install_binaries(&mut out, BINARIES, "freebsd", "x86_64", &dir, &mut runner);
    assert_eq!(installed, [false, false]);
    assert_eq!(calls, 0);
    let lines: Vec<String> = BINARIES
        .iter()
        .map(|binary| {
            let text = install_command(binary, "freebsd", "x86_64");
            let message = "has no release for freebsd-x86_64, so it is not installed";
            format!("{} {message}\n    fix: {text}\n", binary.name)
        })
        .collect();
    let printed = String::from_utf8(out.into_inner()).expect("utf-8");
    assert_eq!(printed, lines.concat());
}

#[test]
fn a_plain_setup_repeats_the_install_command_as_the_fix() {
    let gitleaks = binary("gitleaks");
    let install = install_command(gitleaks, "macos", "aarch64");
    assert_eq!(
        version_failure(
            gitleaks,
            &VersionCheck::Differs("8.21.0".into()),
            &install,
            false,
            "macos",
        ),
        Some((
            "gitleaks version is 8.21.0, the pin is 8.30.1".into(),
            format!("run `{install}`"),
        ))
    );
}

#[test]
fn after_an_install_a_mismatch_names_where_it_went_and_the_likely_cause() {
    let gitleaks = binary("gitleaks");
    let install = install_command(gitleaks, "macos", "aarch64");
    let found = VersionCheck::Differs("8.21.0".into());
    assert_eq!(
        version_failure(gitleaks, &found, &install, true, "macos"),
        Some((
            "gitleaks version is 8.21.0, the pin is 8.30.1".into(),
            "gitleaks was installed to $CARGO_HOME/bin, or ~/.cargo/bin without CARGO_HOME, \
             but this shell does not run that copy. Another copy earlier on PATH, or a PATH \
             not yet refreshed, is the likely cause: run `which -a gitleaks` to list the \
             copies, or open a new shell and run cargo xtask setup again"
                .into(),
        ))
    );
    assert_eq!(
        version_failure(gitleaks, &found, &install, true, "windows"),
        Some((
            "gitleaks version is 8.21.0, the pin is 8.30.1".into(),
            "gitleaks was installed to winget's Links folder, usually \
             %LOCALAPPDATA%\\Microsoft\\WinGet\\Links, but this shell does not run that copy. \
             Another copy earlier on PATH, or a PATH not yet refreshed, is the likely cause: \
             run `where gitleaks` to list the copies, or open a new shell and run cargo xtask \
             setup again"
                .into(),
        ))
    );
}

#[test]
fn after_an_install_a_missing_binary_is_installed_but_not_run() {
    let lefthook = binary("lefthook");
    let install = install_command(lefthook, "linux", "x86_64");
    let why = "`lefthook version` could not start: No such file or directory";
    assert_eq!(
        version_failure(
            lefthook,
            &VersionCheck::Missing(why.into()),
            &install,
            true,
            "linux",
        ),
        Some((
            format!("lefthook was installed, but this shell cannot run it ({why})"),
            "lefthook was installed to $CARGO_HOME/bin, or ~/.cargo/bin without CARGO_HOME, \
             but this shell does not run that copy. Another copy earlier on PATH, or a PATH \
             not yet refreshed, is the likely cause: run `which -a lefthook` to list the \
             copies, or open a new shell and run cargo xtask setup again"
                .into(),
        ))
    );
    assert_eq!(
        version_failure(
            lefthook,
            &VersionCheck::Missing(why.into()),
            &install,
            false,
            "linux",
        ),
        Some((
            format!("lefthook is not installed ({why})"),
            format!("run `{install}`"),
        ))
    );
}

#[test]
fn a_binary_that_broke_after_an_install_is_reinstalled_as_before() {
    let gitleaks = binary("gitleaks");
    let install = install_command(gitleaks, "linux", "x86_64");
    let broken = VersionCheck::Broken(String::new());
    assert_eq!(
        version_failure(gitleaks, &broken, &install, true, "linux"),
        Some((
            "gitleaks is installed, but `gitleaks version` failed: no output".into(),
            format!("reinstall it: run `{install}`"),
        ))
    );
}

#[test]
fn a_matching_version_has_no_failure_either_way() {
    let gitleaks = binary("gitleaks");
    let install = install_command(gitleaks, "linux", "x86_64");
    for installed_now in [false, true] {
        assert_eq!(
            version_failure(
                gitleaks,
                &VersionCheck::Matches,
                &install,
                installed_now,
                "linux",
            ),
            None
        );
    }
}
