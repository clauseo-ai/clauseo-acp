//! `cargo xtask setup`: installs the pinned cargo tools, and prints the install command for the
//! pinned tools that are not cargo crates, then checks their versions.

use crate::output::Printer;
use crate::process::{self, Cmd, Program};
use crate::workspace;
use std::path::Path;
use std::process::ExitCode;

/// A cargo crate `cargo xtask setup` installs at a pinned version.
pub(crate) struct Tool {
    pub name: &'static str,
    pub version: &'static str,
}

/// Bump these together with the toolchain, in their own pull request.
pub(crate) const TOOLS: &[Tool] = &[
    Tool {
        name: "cargo-nextest",
        version: "0.9.148",
    },
    Tool {
        name: "cargo-deny",
        version: "0.20.2",
    },
    Tool {
        name: "cargo-shear",
        version: "1.14.0",
    },
    Tool {
        name: "typos-cli",
        version: "1.51.1",
    },
    Tool {
        name: "taplo-cli",
        version: "0.10.0",
    },
];

/// A pinned tool that is not a cargo crate. `setup` prints its install command for this OS and
/// checks that `<name> version` prints exactly `version`.
pub(crate) struct Binary {
    pub name: &'static str,
    pub version: &'static str,
    /// The GitHub `owner/repo` its releases come from.
    pub repo: &'static str,
    /// Its winget package id, for Windows.
    pub winget: &'static str,
    /// The macOS or Linux command that installs the release binary into `CARGO_BIN`, for
    /// `(os, arch, version)` as in `std::env::consts`; `None` where no release asset exists.
    pub unix: fn(&str, &str, &str) -> Option<String>,
}

pub(crate) const BINARIES: &[Binary] = &[
    Binary {
        name: "gitleaks",
        version: "8.30.1",
        repo: "gitleaks/gitleaks",
        winget: "Gitleaks.Gitleaks",
        unix: gitleaks_unix,
    },
    Binary {
        name: "lefthook",
        version: "2.2.1",
        repo: "evilmartians/lefthook",
        winget: "evilmartians.lefthook",
        unix: lefthook_unix,
    },
];

/// Cargo's bin directory in shell syntax: `$CARGO_HOME/bin`, or `~/.cargo/bin` without
/// `CARGO_HOME`. `cargo install` puts the other tools there, so it is already on PATH.
const CARGO_BIN: &str = "${CARGO_HOME:-$HOME/.cargo}/bin";

fn gitleaks_unix(os: &str, arch: &str, version: &str) -> Option<String> {
    let os = match os {
        "macos" => "darwin",
        "linux" => "linux",
        _ => return None,
    };
    let arch = match arch {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        _ => return None,
    };
    Some(format!(
        "curl -sSfL https://github.com/gitleaks/gitleaks/releases/download/v{version}/gitleaks_{version}_{os}_{arch}.tar.gz | tar -xz -C \"{CARGO_BIN}\" gitleaks"
    ))
}

fn lefthook_unix(os: &str, arch: &str, version: &str) -> Option<String> {
    let os = match os {
        "macos" => "MacOS",
        "linux" => "Linux",
        _ => return None,
    };
    let arch = match arch {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        _ => return None,
    };
    Some(format!(
        "curl -sSfL -o \"{CARGO_BIN}/lefthook\" https://github.com/evilmartians/lefthook/releases/download/v{version}/lefthook_{version}_{os}_{arch} && chmod +x \"{CARGO_BIN}/lefthook\""
    ))
}

/// The command that installs `binary` on `os`/`arch`, or where to download it by hand.
pub(crate) fn install_command(binary: &Binary, os: &str, arch: &str) -> String {
    if os == "windows" {
        return format!(
            "winget install --id {} --version {} --exact",
            binary.winget, binary.version
        );
    }
    (binary.unix)(os, arch, binary.version).unwrap_or_else(|| {
        format!(
            "download v{} for {os}-{arch} from https://github.com/{}/releases and put it on PATH",
            binary.version, binary.repo
        )
    })
}

/// How the installed version compares with the pin.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum VersionCheck {
    Matches,
    Differs(String),
    /// Found, but `<name> version` failed; holds what it printed on stderr.
    Broken(String),
    /// Could not start; holds the error.
    Missing(String),
}

/// Reads `<name> version` output: the trimmed stdout must equal the pin.
/// `ran` is the exit status, stdout and stderr of `<name> version`, or why it could not start.
pub(crate) fn compare_version(
    pinned: &str,
    ran: Result<(bool, String, String), String>,
) -> VersionCheck {
    match ran {
        Ok((true, stdout, _)) if stdout.trim() == pinned => VersionCheck::Matches,
        Ok((true, stdout, _)) => VersionCheck::Differs(stdout.trim().to_owned()),
        Ok((false, _, stderr)) => VersionCheck::Broken(stderr.trim().to_owned()),
        Err(error) => VersionCheck::Missing(error),
    }
}

/// Installs each cargo tool, then prints the install command for each binary and checks its
/// version. Continues past failures; fails if any failed.
pub(crate) fn run() -> ExitCode {
    let mut out = Printer::stdout();
    let root = workspace::workspace_root();
    let mut any_failed = false;
    for tool in TOOLS {
        any_failed |= !install_tool(&mut out, tool, &root);
    }
    for binary in BINARIES {
        any_failed |= !check_binary(&mut out, binary, &root);
    }
    out.note(
        "hooks: run `lefthook install` once per clone to turn on the pre-commit and pre-push hooks",
    );
    if any_failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn install_tool(out: &mut Printer<std::io::Stdout>, tool: &Tool, root: &Path) -> bool {
    let cmd = Cmd::new(
        Program::Cargo,
        ["install", "--locked", tool.name, "--version", tool.version],
    );
    out.note(&cmd);
    let fix = format!("run `{cmd}` to reproduce");
    match process::run_inherit(&cmd, root) {
        Ok(true) => true,
        Ok(false) => {
            out.failure(&format!("installing {} failed", tool.name), &fix);
            false
        }
        Err(error) => {
            out.failure(
                &format!("could not run cargo to install {}: {error}", tool.name),
                &fix,
            );
            false
        }
    }
}

fn check_binary(out: &mut Printer<std::io::Stdout>, binary: &Binary, root: &Path) -> bool {
    let install = install_command(binary, std::env::consts::OS, std::env::consts::ARCH);
    out.note(format!("{} {}: {install}", binary.name, binary.version));
    let cmd = Cmd::new(Program::Named(binary.name), ["version"]);
    let ran = process::run_quiet(&cmd, root)
        .map(|quiet| (quiet.success, quiet.stdout, quiet.stderr))
        .map_err(|error| format!("`{cmd}` could not start: {error}"));
    match compare_version(binary.version, ran) {
        VersionCheck::Matches => {
            out.note(format!("{} version {} ok", binary.name, binary.version));
            true
        }
        VersionCheck::Differs(found) => {
            out.failure(
                &format!(
                    "{} version is {found}, the pin is {}",
                    binary.name, binary.version
                ),
                &format!("run `{install}`"),
            );
            false
        }
        VersionCheck::Broken(stderr) => {
            out.failure(
                &format!(
                    "{} is installed, but `{cmd}` failed: {}",
                    binary.name,
                    if stderr.is_empty() {
                        "no output"
                    } else {
                        stderr.as_str()
                    }
                ),
                &format!("reinstall it: run `{install}`"),
            );
            false
        }
        VersionCheck::Missing(why) => {
            out.failure(
                &format!("{} is not installed ({why})", binary.name),
                &format!("run `{install}`"),
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            install_command(gitleaks, "linux", "x86_64")
                .contains("gitleaks_8.30.1_linux_x64.tar.gz")
        );
        assert_eq!(
            install_command(lefthook, "macos", "x86_64"),
            r#"curl -sSfL -o "${CARGO_HOME:-$HOME/.cargo}/bin/lefthook" https://github.com/evilmartians/lefthook/releases/download/v2.2.1/lefthook_2.2.1_MacOS_x86_64 && chmod +x "${CARGO_HOME:-$HOME/.cargo}/bin/lefthook""#
        );
        assert!(
            install_command(lefthook, "linux", "aarch64").contains("lefthook_2.2.1_Linux_arm64")
        );
        assert_eq!(
            install_command(gitleaks, "windows", "aarch64"),
            "winget install --id Gitleaks.Gitleaks --version 8.30.1 --exact"
        );
        assert_eq!(
            install_command(lefthook, "windows", "x86_64"),
            "winget install --id evilmartians.lefthook --version 2.2.1 --exact"
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
}
