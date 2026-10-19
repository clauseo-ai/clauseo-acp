//! Tests for `setup.rs`: the pins, the install command for each OS and arch, and `run`. The
//! expected commands are written out here from `version` and `repo`, never built by the code under
//! test.

use super::*;
use crate::process::Quiet;
use crate::scratch::Scratch;

pub(super) fn binary(name: &str) -> &'static Binary {
    BINARIES.iter().find(|b| b.name == name).unwrap()
}

/// The platforms `--binaries` must install on: macOS, Linux and Windows, on both arches.
pub(super) const PLATFORMS: [(&str, &str); 6] = [
    ("macos", "aarch64"),
    ("macos", "x86_64"),
    ("linux", "aarch64"),
    ("linux", "x86_64"),
    ("windows", "x86_64"),
    ("windows", "aarch64"),
];

/// The release asset of `b` for a macOS or Linux `os`/`arch`. Each asset's OS and arch spelling
/// is written here once.
pub(super) fn expected_asset(b: &Binary, os: &str, arch: &str) -> String {
    let v = b.version;
    match (b.name, os, arch) {
        ("gitleaks", "macos", "aarch64") => format!("gitleaks_{v}_darwin_arm64.tar.gz"),
        ("gitleaks", "macos", "x86_64") => format!("gitleaks_{v}_darwin_x64.tar.gz"),
        ("gitleaks", "linux", "aarch64") => format!("gitleaks_{v}_linux_arm64.tar.gz"),
        ("gitleaks", "linux", "x86_64") => format!("gitleaks_{v}_linux_x64.tar.gz"),
        ("lefthook", "macos", "aarch64") => format!("lefthook_{v}_MacOS_arm64"),
        ("lefthook", "macos", "x86_64") => format!("lefthook_{v}_MacOS_x86_64"),
        ("lefthook", "linux", "aarch64") => format!("lefthook_{v}_Linux_arm64"),
        ("lefthook", "linux", "x86_64") => format!("lefthook_{v}_Linux_x86_64"),
        other => panic!("no asset written out for {other:?}"),
    }
}

/// The download URL of `asset`, a release asset of `b`.
pub(super) fn expected_url(b: &Binary, asset: &str) -> String {
    format!(
        "https://github.com/{}/releases/download/v{}/{asset}",
        b.repo, b.version
    )
}

/// The winget command that installs `b`.
pub(super) fn expected_winget(b: &Binary) -> String {
    format!(
        "winget install --id {} --version {} --exact --accept-source-agreements \
         --accept-package-agreements --disable-interactivity",
        b.winget, b.version
    )
}

/// The curl command that downloads `url` to `asset`: three retries, any error retried, 30 s to
/// connect and 300 s for each attempt.
pub(super) fn expected_curl(asset: &str, url: &str) -> String {
    format!(
        "curl -sSfL --retry 3 --retry-all-errors --connect-timeout 30 --max-time 300 -o {asset} \
         {url}"
    )
}

/// The file a curl command writes: the argument after `-o`.
pub(super) fn curl_output(cmd: &Cmd) -> &str {
    let at = cmd.args.iter().position(|arg| arg == "-o").unwrap();
    &cmd.args[at + 1]
}

/// A command that exited, with what it printed.
#[expect(clippy::unnecessary_wraps, reason = "it is what a runner returns")]
pub(super) fn exited(success: bool, stdout: &str, stderr: &str) -> io::Result<Ended> {
    Ok(Ended::Exited(Quiet {
        success,
        stdout: stdout.to_owned(),
        stderr: stderr.to_owned(),
    }))
}

/// A Linux x86-64 machine whose cargo bin is `bin`, on PATH when `on_path`.
pub(super) fn linux(bin: &Path, on_path: bool) -> Machine<'static> {
    let path = if on_path {
        std::env::join_paths([Path::new("/usr/bin"), bin]).unwrap()
    } else {
        OsString::from("/usr/bin")
    };
    Machine {
        os: "linux",
        arch: "x86_64",
        cargo_bin: Some(bin.to_path_buf()),
        path,
    }
}

/// The pinned asset of a fake tool, and its sha256.
pub(super) const ASSET_BYTES: &[u8] = b"faketool release asset";
pub(super) const ASSET_SHA256: &str =
    "19cf8bf2081d3eb1a73334c744a60f5f6accf3bd217bd1ee94a2319500a89d7e";

#[expect(
    clippy::unnecessary_wraps,
    reason = "it is a `Binary::asset`, which returns an Option"
)]
fn fake_asset(os: &str, arch: &str, version: &str) -> Option<String> {
    Some(format!("faketool_{version}_{os}_{arch}"))
}

/// A tool whose Linux x86-64 asset is `ASSET_BYTES`, held as `packing`.
pub(super) fn fake_tool(packing: Packing) -> Binary {
    Binary {
        name: "faketool",
        version: "fake-version",
        sha256: &[AssetSha256 {
            os: "linux",
            arch: "x86_64",
            sha256: ASSET_SHA256,
        }],
        repo: "example/faketool",
        winget: "Example.FakeTool",
        asset: fake_asset,
        packing,
    }
}

/// Each tool in `TOOLS`, the one place the versions live, is pinned to an exact `x.y.z` (what
/// `cargo install --version` then installs and nothing newer), and named once.
#[test]
fn the_cargo_tools_are_pinned() {
    let exact = |v: &str| {
        let parts: Vec<&str> = v.split('.').collect();
        parts.len() == 3
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    };
    for t in TOOLS {
        assert!(
            exact(t.version),
            "{} {} is not an exact x.y.z",
            t.name,
            t.version
        );
    }
    let mut names: Vec<&str> = TOOLS.iter().map(|t| t.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), TOOLS.len(), "a tool is pinned twice");
}

#[test]
fn each_binary_pins_one_sha256_per_unix_platform() {
    for b in BINARIES {
        let pinned: Vec<(&str, &str)> = b.sha256.iter().map(|p| (p.os, p.arch)).collect();
        let unix: Vec<(&str, &str)> = PLATFORMS
            .into_iter()
            .filter(|(os, _)| *os != "windows")
            .collect();
        assert_eq!(pinned, unix, "{}", b.name);
        for pin in b.sha256 {
            assert_eq!(pin.sha256.len(), 64, "{} {}-{}", b.name, pin.os, pin.arch);
            assert!(
                pin.sha256
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
                "{} {}-{} is lowercase hex",
                b.name,
                pin.os,
                pin.arch
            );
        }
    }
}

#[test]
fn install_commands_fetch_the_pinned_release_for_each_os() {
    for b in BINARIES {
        for (os, arch) in PLATFORMS {
            let expected = if os == "windows" {
                format!("run `{}`", expected_winget(b))
            } else {
                let url = expected_url(b, &expected_asset(b, os, arch));
                format!(
                    "run `cargo xtask setup --binaries`, which downloads {url} and installs {} \
                     only if its sha256 matches the pin",
                    b.name
                )
            };
            assert_eq!(
                install::how_to_install(b, os, arch),
                expected,
                "{os}-{arch}"
            );
        }
        assert_eq!(
            install::how_to_install(b, "freebsd", "x86_64"),
            format!(
                "download v{} for freebsd-x86_64 from https://github.com/{}/releases and put it \
                 on PATH",
                b.version, b.repo
            )
        );
    }
}

#[test]
fn install_runs_the_pinned_command_for_each_os_and_arch() {
    for (os, arch) in PLATFORMS {
        let dir = Scratch::new("setup-install-each");
        let bin = dir.join("bin");
        let mut ran: Vec<(String, PathBuf)> = Vec::new();
        // Each download gets bytes that match no pin, so each install stops after its download.
        let mut runner = |cmd: &Cmd, at: &Path, _: Output, _: Duration| -> io::Result<Ended> {
            ran.push((cmd.to_string(), at.to_path_buf()));
            if cmd.program == Program::Named("curl") {
                std::fs::write(at.join(curl_output(cmd)), b"not a release")?;
            }
            exited(true, "", "")
        };
        let machine = Machine {
            os,
            arch,
            ..linux(&bin, true)
        };
        let mut out = Printer::new(Vec::new(), false);
        for b in BINARIES {
            install::install_binary(&mut out, b, &machine, &dir, &mut runner);
        }
        let expected: Vec<(String, PathBuf)> = BINARIES
            .iter()
            .map(|b| {
                if os == "windows" {
                    (expected_winget(b), dir.to_path_buf())
                } else {
                    let asset = expected_asset(b, os, arch);
                    let url = expected_url(b, &asset);
                    let staging = format!(".xtask-setup-{}-{}", b.name, std::process::id());
                    (expected_curl(&asset, &url), bin.join(staging))
                }
            })
            .collect();
        assert_eq!(ran, expected, "{os}-{arch}");
    }
}

/// What `run_with` ran, as shell-like text, for a tool whose `version` prints each entry of
/// `versions` in turn (`None`: it cannot start).
fn run_fake(
    machine: &Machine<'_>,
    tool: &Binary,
    install: bool,
    versions: &[Option<&str>],
) -> (bool, Vec<String>, String) {
    let mut ran = Vec::new();
    let mut versions = versions.iter();
    let mut runner = |cmd: &Cmd, at: &Path, _: Output, _: Duration| -> io::Result<Ended> {
        ran.push(cmd.to_string());
        match cmd.program {
            Program::Named("curl") => std::fs::write(at.join(curl_output(cmd)), ASSET_BYTES)?,
            Program::Named("faketool") => {
                return match versions.next().expect("a version for each check") {
                    Some(version) => exited(true, &format!("{version}\n"), ""),
                    None => Err(io::Error::new(io::ErrorKind::NotFound, "not found")),
                };
            }
            _ => {}
        }
        exited(true, "", "")
    };
    let mut out = Printer::new(Vec::new(), false);
    let root = Path::new(".");
    let ok = run_with(
        &mut out,
        machine,
        TOOLS,
        std::slice::from_ref(tool),
        install,
        root,
        &mut runner,
    );
    let printed = String::from_utf8(out.into_inner()).unwrap();
    (ok, ran, printed)
}

/// The `cargo install` commands for `TOOLS`, which `run` runs first.
fn cargo_installs() -> Vec<String> {
    TOOLS
        .iter()
        .map(|t| format!("cargo install --locked {} --version {}", t.name, t.version))
        .collect()
}

#[test]
fn run_checks_the_version_after_the_install() {
    let dir = Scratch::new("setup-run-installs");
    let bin = dir.join("bin");
    let tool = fake_tool(Packing::Bare);
    let v = tool.version;
    let (ok, ran, printed) = run_fake(&linux(&bin, true), &tool, true, &[None, Some(v)]);
    assert!(ok, "{printed}");
    let asset = format!("faketool_{v}_linux_x86_64");
    let url = format!("https://github.com/example/faketool/releases/download/v{v}/{asset}");
    let mut expected = cargo_installs();
    expected.extend([
        "faketool version".to_owned(),
        expected_curl(&asset, &url),
        "faketool version".to_owned(),
    ]);
    assert_eq!(ran, expected);
    assert_eq!(std::fs::read(bin.join("faketool")).unwrap(), ASSET_BYTES);
    assert!(printed.ends_with(&format!(
        "installed faketool {v} to {}: its sha256 matches the pin\nfaketool version {v} ok\n\
         hooks: run `lefthook install` once per clone to turn on the pre-commit and pre-push \
         hooks\n",
        bin.display()
    )));
}

#[test]
fn a_second_run_on_a_warm_machine_installs_nothing() {
    let dir = Scratch::new("setup-run-warm");
    let bin = dir.join("bin");
    let tool = fake_tool(Packing::TarGz);
    for os in ["windows", "linux"] {
        let machine = Machine {
            os,
            ..linux(&bin, true)
        };
        let (ok, ran, printed) = run_fake(&machine, &tool, true, &[Some(tool.version)]);
        assert!(ok, "{printed}");
        let mut expected = cargo_installs();
        expected.push("faketool version".to_owned());
        assert_eq!(ran, expected, "{os}");
        if os == "windows" {
            assert!(!bin.exists(), "windows: nothing was written");
        } else {
            assert_eq!(
                names_in(&bin),
                [".xtask-setup.lock"],
                "linux: only the run lock"
            );
        }
    }
}

#[test]
fn an_install_whose_bin_is_not_on_path_says_to_put_it_there() {
    let dir = Scratch::new("setup-run-off-path");
    let bin = dir.join("bin");
    let tool = fake_tool(Packing::Bare);
    let (ok, ran, printed) = run_fake(&linux(&bin, false), &tool, true, &[None, None]);
    assert!(!ok);
    assert_eq!(ran.last().map(String::as_str), Some("faketool version"));
    let shown = bin.display();
    assert!(
        printed.contains(&format!(
            "faketool was installed, but this shell cannot run it (`faketool version` could not \
             start: not found)\n    fix: faketool was installed to {shown}, which is not on PATH: \
             put it on PATH (in your shell profile, for every new shell), then run cargo xtask \
             setup again\n"
        )),
        "{printed}"
    );
}

#[test]
fn a_plain_setup_checks_the_version_and_installs_nothing() {
    let dir = Scratch::new("setup-run-plain");
    let bin = dir.join("bin");
    let tool = fake_tool(Packing::Bare);
    let (ok, ran, printed) = run_fake(&linux(&bin, true), &tool, false, &[None]);
    assert!(!ok);
    let mut expected = cargo_installs();
    expected.push("faketool version".to_owned());
    assert_eq!(ran, expected);
    assert!(!bin.exists());
    let how = install::how_to_install(&tool, "linux", "x86_64");
    assert!(printed.contains(&format!(
        "faketool is not installed (`faketool version` could not start: not found)\n    fix: \
         {how}\n"
    )));
}

#[test]
fn a_version_check_that_hangs_or_cannot_run_is_a_failed_run_not_a_missing_binary() {
    let tool = fake_tool(Packing::Bare);
    let cases: [(io::Result<Ended>, &str); 3] = [
        (Ok(Ended::TimedOut), "it did not finish within 30 s"),
        (
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Permission denied",
            )),
            "Permission denied",
        ),
        (
            Err(io::Error::new(io::ErrorKind::NotFound, "not found")),
            "",
        ),
    ];
    for (ended, said) in cases {
        let mut ended = Some(ended);
        let mut runner = |_: &Cmd, _: &Path, _: Output, _: Duration| ended.take().unwrap();
        let expected = if said.is_empty() {
            VersionCheck::Missing("`faketool version` could not start: not found".into())
        } else {
            VersionCheck::Broken(said.into())
        };
        assert_eq!(check_version(&tool, Path::new("."), &mut runner), expected);
    }
}

#[test]
fn after_a_failed_install_the_version_failure_does_not_repeat_the_install() {
    let dir = Scratch::new("setup-run-install-fails");
    let bin = dir.join("bin");
    // Pinned to a sha256 that the fake download does not have.
    let tool = Binary {
        sha256: &[AssetSha256 {
            os: "linux",
            arch: "x86_64",
            sha256: "0000000000000000000000000000000000000000000000000000000000000000",
        }],
        ..fake_tool(Packing::Bare)
    };
    let (ok, ran, printed) = run_fake(&linux(&bin, true), &tool, true, &[None, None]);
    assert!(!ok);
    assert_eq!(ran.last().map(String::as_str), Some("faketool version"));
    assert!(
        printed.contains(
            "faketool is not installed (`faketool version` could not start: not found)\n    fix: \
             the install of faketool above failed: do what its fix says, then run cargo xtask \
             setup again\n"
        ),
        "{printed}"
    );
}

/// Fills `bin` with what an interrupted setup leaves (a staging dir with half a download, a stray
/// staging file) and two entries that are not setup's.
fn plant_leftovers(bin: &Path) {
    let stale = bin.join(".xtask-setup-gitleaks-99999");
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::write(stale.join("gitleaks_unverified.tar.gz"), b"half a download").unwrap();
    std::fs::write(bin.join(".xtask-setup-lefthook-1"), b"a stray file").unwrap();
    std::fs::write(bin.join("lefthook"), b"the working lefthook").unwrap();
    std::fs::write(bin.join(".xtask-other"), b"not ours").unwrap();
}

/// The names in `dir`, sorted.
fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_binaries_run_removes_leftovers_of_an_interrupted_run_even_when_nothing_downloads() {
    let dir = Scratch::new("setup-run-leftovers-warm");
    let bin = dir.join("bin");
    plant_leftovers(&bin);
    let tool = fake_tool(Packing::Bare);
    let (ok, ran, printed) = run_fake(&linux(&bin, true), &tool, true, &[Some(tool.version)]);
    assert!(ok, "{printed}");
    assert!(!ran.iter().any(|cmd| cmd.starts_with("curl")), "{ran:?}");
    assert_eq!(
        names_in(&bin),
        [".xtask-other", ".xtask-setup.lock", "lefthook"]
    );
    assert_eq!(
        std::fs::read(bin.join("lefthook")).unwrap(),
        b"the working lefthook"
    );
}

#[test]
fn a_plain_setup_leaves_bin_alone() {
    let dir = Scratch::new("setup-run-leftovers-plain");
    let bin = dir.join("bin");
    plant_leftovers(&bin);
    let before = names_in(&bin);
    let tool = fake_tool(Packing::Bare);
    run_fake(&linux(&bin, true), &tool, false, &[Some(tool.version)]);
    assert_eq!(names_in(&bin), before);
}
