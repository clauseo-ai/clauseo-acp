//! `cargo xtask setup`: installs the pinned cargo tools, and prints the install command for the
//! pinned tools that are not cargo crates, then checks their versions. With `--binaries` it also
//! runs those install commands, and a failed one fails setup with the command to run by hand.

use crate::output::Printer;
use crate::process::{self, Cmd, Program};
use crate::workspace;
use std::io::{self, Write};
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

/// The `winget` command that installs `binary` on Windows. It runs without prompts: winget asks
/// nothing about source or package agreements, and never goes interactive.
fn winget(binary: &Binary) -> Cmd {
    Cmd::new(
        Program::Named("winget"),
        [
            "install",
            "--id",
            binary.winget,
            "--version",
            binary.version,
            "--exact",
            "--accept-source-agreements",
            "--accept-package-agreements",
            "--disable-interactivity",
        ],
    )
}

/// The command that installs `binary` on `os`/`arch`, or where to download it by hand.
pub(crate) fn install_command(binary: &Binary, os: &str, arch: &str) -> String {
    if os == "windows" {
        return winget(binary).to_string();
    }
    (binary.unix)(os, arch, binary.version).unwrap_or_else(|| {
        format!(
            "download v{} for {os}-{arch} from https://github.com/{}/releases and put it on PATH",
            binary.version, binary.repo
        )
    })
}

/// The program and arguments that run what `install_command` shows: `winget` on Windows, and
/// `sh -c` with the shell command elsewhere. `None` where no release asset exists, so nothing runs.
pub(crate) fn install_cmd(binary: &Binary, os: &str, arch: &str) -> Option<Cmd> {
    if os == "windows" {
        return Some(winget(binary));
    }
    let script = (binary.unix)(os, arch, binary.version)?;
    Some(Cmd::new(Program::Named("sh"), ["-c", script.as_str()]))
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

/// Runs a command in a directory and returns whether it succeeded; `Err` means it could not start.
/// Production passes `process::run_inherit`, so the output goes to the terminal.
type Runner<'a> = dyn FnMut(&Cmd, &Path) -> io::Result<bool> + 'a;

/// Installs each cargo tool, then, with `binaries`, runs each binary's install command. Then
/// prints the install command for each binary and checks its version. Continues past failures;
/// fails if any failed.
pub(crate) fn run(binaries: bool) -> ExitCode {
    let mut out = Printer::stdout();
    let root = workspace::workspace_root();
    let mut any_failed = false;
    for tool in TOOLS {
        any_failed |= !install_tool(&mut out, tool, &root);
    }
    if binaries {
        let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
        let installed = install_binaries(
            &mut out,
            BINARIES,
            os,
            arch,
            &root,
            &mut process::run_inherit,
        );
        any_failed |= !installed;
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

/// Runs the install command of each binary for `os`/`arch` in `root`, printing each one before
/// it runs. Continues past failures. Returns whether every binary was installed.
fn install_binaries<W: Write>(
    out: &mut Printer<W>,
    binaries: &[Binary],
    os: &str,
    arch: &str,
    root: &Path,
    runner: &mut Runner<'_>,
) -> bool {
    let mut all_installed = true;
    for binary in binaries {
        all_installed &= install_binary(out, binary, os, arch, root, runner);
    }
    all_installed
}

/// Runs the install command of one binary. A platform with no release asset runs nothing and
/// fails with the download text as its fix. A failure, whether the command exits non-zero or
/// cannot start, fails with that command as the fix.
fn install_binary<W: Write>(
    out: &mut Printer<W>,
    binary: &Binary,
    os: &str,
    arch: &str,
    root: &Path,
    runner: &mut Runner<'_>,
) -> bool {
    let text = install_command(binary, os, arch);
    let Some(cmd) = install_cmd(binary, os, arch) else {
        out.failure(
            &format!(
                "{} has no release for {os}-{arch}, so it is not installed",
                binary.name
            ),
            &text,
        );
        return false;
    };
    out.note(format!(
        "installing {} {}: {text}",
        binary.name, binary.version
    ));
    let fix =
        format!("run `{text}` by hand and check its output, then run cargo xtask setup again");
    match runner(&cmd, root) {
        Ok(true) => true,
        Ok(false) => {
            out.failure(&format!("installing {} failed", binary.name), &fix);
            false
        }
        Err(error) => {
            out.failure(
                &format!(
                    "could not start {} to install {}: {error}",
                    cmd.program.name(),
                    binary.name
                ),
                &fix,
            );
            false
        }
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
mod tests;
