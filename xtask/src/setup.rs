//! `cargo xtask setup`: installs the pinned cargo tools, prints how to install the pinned tools
//! that are not cargo crates, and checks their versions. With `--binaries` it also installs each of
//! those that is missing or at another version, then checks its version again. On macOS and Linux
//! that install downloads the release asset, checks its pinned sha256 and only then moves the
//! binary into `CARGO_HOME/bin`; on Windows it runs winget. With `--only <tool>` (repeatable) it
//! sets up only the named tools, each named as in `TOOLS` or `BINARIES`; `--binaries` then installs
//! only the named binaries. A name that is neither fails the run before anything is installed.
//!
//! `setup --list` installs and checks nothing: it prints every pin, one line each, its fields
//! separated by tabs, for scripts that install the tools another way:
//!
//! - `tool<TAB><name><TAB><version>` for each entry of `TOOLS`, in order;
//! - `binary<TAB><name><TAB><version><TAB><os><TAB><arch><TAB><sha256>` for each pinned release
//!   asset of each entry of `BINARIES`, in order, with `os` and `arch` as in `std::env::consts`
//!   and `sha256` in lowercase hex.
//!
//! A change to these lines is a change of format for every script that reads them.

mod install;
mod version;

use crate::output::Printer;
use crate::process::{self, Cmd, Output, Program, Ran, Status, TimeoutFix};
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use version::{Attempt, VersionCheck, compare_version};

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
    CARGO_MUTANTS,
];

/// cargo-mutants, which `cargo xtask mutants` runs and checks the version of.
pub(crate) const CARGO_MUTANTS: Tool = Tool {
    name: "cargo-mutants",
    version: "27.1.0",
};

/// A pinned tool that is not a cargo crate. `setup` checks that `<name> version` prints exactly
/// `version`, and `setup --binaries` installs that version when it does not.
pub(crate) struct Binary {
    pub name: &'static str,
    pub version: &'static str,
    /// The sha256 of the release asset of `version` for each OS and arch it is pinned on, from
    /// the checksum file that release publishes: every macOS and Linux one, which
    /// `setup --binaries` checks its download against, and Windows x86-64, which `setup --list`
    /// prints for scripts that download it there (`setup --binaries` runs winget on Windows). A pin
    /// bump changes these with `version`.
    pub sha256: &'static [AssetSha256],
    /// The GitHub `owner/repo` its releases come from.
    pub repo: &'static str,
    /// Its winget package id, for Windows.
    pub winget: &'static str,
    /// The file name of the release asset for `(os, arch, version)` as in `std::env::consts`;
    /// `None` where no asset exists.
    pub asset: fn(&str, &str, &str) -> Option<String>,
    /// How the asset holds the binary.
    pub packing: Packing,
}

/// The sha256 of one release asset, for an OS and arch as in `std::env::consts`.
pub(crate) struct AssetSha256 {
    pub os: &'static str,
    pub arch: &'static str,
    /// Lowercase hex, as in the release's checksum file.
    pub sha256: &'static str,
}

/// How a release asset holds its binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Packing {
    /// A `.tar.gz` with the binary, named as the tool, at its top level.
    TarGz,
    /// The asset is the binary itself.
    Bare,
}

pub(crate) const BINARIES: &[Binary] = &[
    Binary {
        name: "gitleaks",
        version: "8.30.1",
        sha256: &[
            AssetSha256 {
                os: "macos",
                arch: "aarch64",
                sha256: "b40ab0ae55c505963e365f271a8d3846efbc170aa17f2607f13df610a9aeb6a5",
            },
            AssetSha256 {
                os: "macos",
                arch: "x86_64",
                sha256: "dfe101a4db2255fc85120ac7f3d25e4342c3c20cf749f2c20a18081af1952709",
            },
            AssetSha256 {
                os: "linux",
                arch: "aarch64",
                sha256: "e4a487ee7ccd7d3a7f7ec08657610aa3606637dab924210b3aee62570fb4b080",
            },
            AssetSha256 {
                os: "linux",
                arch: "x86_64",
                sha256: "551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb",
            },
            // gitleaks_<version>_windows_x64.zip
            AssetSha256 {
                os: "windows",
                arch: "x86_64",
                sha256: "d29144deff3a68aa93ced33dddf84b7fdc26070add4aa0f4513094c8332afc4e",
            },
        ],
        repo: "gitleaks/gitleaks",
        winget: "Gitleaks.Gitleaks",
        asset: gitleaks_asset,
        packing: Packing::TarGz,
    },
    Binary {
        name: "lefthook",
        version: "2.2.1",
        sha256: &[
            AssetSha256 {
                os: "macos",
                arch: "aarch64",
                sha256: "37e5120e2638ac660776b6bf4a6331e504957a8dde53aa3cb57c79dcd0996698",
            },
            AssetSha256 {
                os: "macos",
                arch: "x86_64",
                sha256: "dbff6fba0b69909df0310a6ccd75141c9cb196244b2d89f971ebe93fe96f4801",
            },
            AssetSha256 {
                os: "linux",
                arch: "aarch64",
                sha256: "c1903087d643cb26187665eaf6f484a8a295af438dad01138f3e54d6b4985b41",
            },
            AssetSha256 {
                os: "linux",
                arch: "x86_64",
                sha256: "ebee376ee803749c39dd8c6fecff47e08e63e03b02b5ef62e3ea616e4312b1e0",
            },
            // lefthook_<version>_Windows_x86_64.exe
            AssetSha256 {
                os: "windows",
                arch: "x86_64",
                sha256: "51dfc983a4b86852b33d3238ff45414d2d488a70c0e84d08b90907adb5ad0cbe",
            },
        ],
        repo: "evilmartians/lefthook",
        winget: "evilmartians.lefthook",
        asset: lefthook_asset,
        packing: Packing::Bare,
    },
];

fn gitleaks_asset(os: &str, arch: &str, version: &str) -> Option<String> {
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
    Some(format!("gitleaks_{version}_{os}_{arch}.tar.gz"))
}

fn lefthook_asset(os: &str, arch: &str, version: &str) -> Option<String> {
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
    Some(format!("lefthook_{version}_{os}_{arch}"))
}

/// How long `cargo install` may run before its tree is stopped: it builds a tool from source,
/// which on a cold cache and a slow machine takes many minutes, so this is xtask's longest limit.
const CARGO_INSTALL_TIMEOUT: Duration = Duration::from_hours(1);
/// How long `<binary> version` may run: it prints one line, so 30 s means it hangs.
const VERSION_TIMEOUT: Duration = Duration::from_secs(30);

/// The machine `setup` installs on.
pub(crate) struct Machine<'a> {
    /// The OS and arch, as in `std::env::consts`.
    pub os: &'a str,
    pub arch: &'a str,
    /// `CARGO_HOME/bin`, or `~/.cargo/bin` without `CARGO_HOME`; `None` when neither `CARGO_HOME`
    /// nor the home directory is set.
    pub cargo_bin: Option<PathBuf>,
    /// The `PATH` this process runs commands from.
    pub path: OsString,
}

impl Machine<'static> {
    /// This machine, from `std::env::consts` and the environment.
    fn this() -> Self {
        let home = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        let cargo_home = std::env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os(home).map(|home| PathBuf::from(home).join(".cargo")));
        Machine {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            cargo_bin: cargo_home.map(|cargo_home| cargo_home.join("bin")),
            path: std::env::var_os("PATH").unwrap_or_default(),
        }
    }
}

/// The tools a run can set up: `TOOLS` and `BINARIES` in production, fakes in tests.
struct Pins<'a> {
    tools: &'a [Tool],
    binaries: &'a [Binary],
}

/// Which pinned tools a run sets up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Selection<'a> {
    /// Every tool in `TOOLS` and `BINARIES`: `setup` without `--only`.
    All,
    /// The tools `--only` names, at least one; any name may be unknown, which fails the run.
    Only(&'a [String]),
}

impl<'a> Selection<'a> {
    /// The selection the `--only` values give: `All` when there are none.
    pub(crate) fn of(only: &'a [String]) -> Self {
        if only.is_empty() {
            Selection::All
        } else {
            Selection::Only(only)
        }
    }

    fn has(self, name: &str) -> bool {
        match self {
            Selection::All => true,
            Selection::Only(names) => names.iter().any(|n| n == name),
        }
    }
}

/// Runs a command in a directory, with where its output goes. Production passes `process::run`.
pub(crate) type Runner<'a> = dyn FnMut(&Cmd, &Path, Output) -> io::Result<Ran> + 'a;

/// Installs each selected cargo tool, then checks each selected binary's version and, with
/// `binaries`, installs it when that check fails and checks again. Continues past failures; fails
/// if any failed. Runs in `root`, the checkout, with every command going through `runner`.
/// Production passes `process::run`.
pub(crate) fn run(
    root: &Path,
    selection: Selection<'_>,
    binaries: bool,
    runner: &mut Runner<'_>,
) -> ExitCode {
    let mut out = Printer::stdout();
    let machine = Machine::this();
    let pins = Pins {
        tools: TOOLS,
        binaries: BINARIES,
    };
    let ok = run_with(&mut out, &machine, &pins, selection, binaries, root, runner);
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Prints every pin to `out`, as the module doc writes out, and installs nothing. Production passes
/// `Printer::stdout()`.
pub(crate) fn list<W: Write>(out: &mut Printer<W>) -> ExitCode {
    let pins = Pins {
        tools: TOOLS,
        binaries: BINARIES,
    };
    list_with(out, &pins);
    ExitCode::SUCCESS
}

/// The `setup --list` lines of `pins`, written in one go.
fn list_with<W: Write>(out: &mut Printer<W>, pins: &Pins<'_>) {
    let tools = (pins.tools.iter()).map(|t| format!("tool\t{}\t{}", t.name, t.version));
    let assets = pins.binaries.iter().flat_map(|b| {
        b.sha256.iter().map(move |pin| {
            format!(
                "binary\t{}\t{}\t{}\t{}\t{}",
                b.name, b.version, pin.os, pin.arch, pin.sha256
            )
        })
    });
    out.note(tools.chain(assets).collect::<Vec<_>>().join("\n"));
}

/// `run` on `machine`, with every command going through `runner`. Returns whether all passed.
fn run_with<W: Write>(
    out: &mut Printer<W>,
    machine: &Machine<'_>,
    pins: &Pins<'_>,
    selection: Selection<'_>,
    install: bool,
    root: &Path,
    runner: &mut Runner<'_>,
) -> bool {
    if !known_names(out, pins, selection) {
        return false;
    }
    let tools = pins.tools.iter().filter(|t| selection.has(t.name));
    let binaries: Vec<&Binary> = (pins.binaries.iter())
        .filter(|b| selection.has(b.name))
        .collect();
    let mut all_ok = true;
    for tool in tools {
        all_ok &= install_tool(out, tool, root, runner);
    }
    // Held to the end of the run.
    let _lock = if install && !binaries.is_empty() {
        let (lock, ok) = install::lock_bin_and_sweep(out, machine);
        all_ok &= ok;
        lock
    } else {
        None
    };
    for binary in binaries {
        all_ok &= setup_binary(out, binary, machine, install, root, runner);
    }
    out.note(
        "hooks: run `lefthook install` once per clone to turn on the pre-commit and pre-push hooks",
    );
    all_ok
}

/// Fails each name in `selection` that `pins` does not pin, with a fix that lists the names it
/// does. Returns whether every name is pinned.
fn known_names<W: Write>(out: &mut Printer<W>, pins: &Pins<'_>, selection: Selection<'_>) -> bool {
    let Selection::Only(names) = selection else {
        return true;
    };
    let valid: Vec<&str> = (pins.tools.iter().map(|t| t.name))
        .chain(pins.binaries.iter().map(|b| b.name))
        .collect();
    let mut all_known = true;
    // A name given twice fails once, at its first place in `names`, as a known name runs once.
    let unknown = names
        .iter()
        .enumerate()
        .filter(|(i, n)| !valid.contains(&n.as_str()) && !names[..*i].contains(n));
    for (_, name) in unknown {
        out.failure(
            &format!("setup --only {name}: no pinned tool is named {name}"),
            &format!(
                "name a pinned tool after --only, one of: {}; then run cargo xtask setup again",
                valid.join(", ")
            ),
        );
        all_known = false;
    }
    all_known
}

/// Checks `binary`'s version; with `install`, installs it when the check fails and checks again.
/// Prints how to install it, then the result. Returns whether it ends at the pinned version.
fn setup_binary<W: Write>(
    out: &mut Printer<W>,
    binary: &Binary,
    machine: &Machine<'_>,
    install: bool,
    root: &Path,
    runner: &mut Runner<'_>,
) -> bool {
    let how = install::how_to_install(binary, machine.os, machine.arch);
    out.note(format!("{} {}: {how}", binary.name, binary.version));
    let mut check = check_version(binary, root, runner);
    let mut attempt = Attempt::None;
    if install && check != VersionCheck::Matches {
        attempt = if install::install_binary(out, binary, machine, root, runner) {
            Attempt::Installed
        } else {
            Attempt::Failed
        };
        check = check_version(binary, root, runner);
    }
    if let Some((message, fix)) = version::version_failure(binary, &check, &how, attempt, machine) {
        out.failure(&message, &fix);
        return false;
    }
    out.note(format!("{} version {} ok", binary.name, binary.version));
    attempt != Attempt::Failed
}

/// Runs `<name> version` and compares what it prints with the pin.
fn check_version(binary: &Binary, root: &Path, runner: &mut Runner<'_>) -> VersionCheck {
    let cmd = version::version_cmd(binary);
    let ran = match runner(&cmd, root, Output::Quiet) {
        Ok(Ran {
            status: Status::TimedOut,
            ..
        }) => Ok((
            false,
            String::new(),
            format!(
                "it did not finish within {}",
                process::limit_text(cmd.timeout)
            ),
        )),
        Ok(quiet) => Ok((quiet.success(), quiet.stdout, quiet.stderr)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Err(format!("`{cmd}` could not start: {error}"))
        }
        // Found, but it could not be started or stopped: report it as a failed run.
        Err(error) => Ok((false, String::new(), error.to_string())),
    };
    compare_version(binary.version, ran)
}

fn install_tool<W: Write>(
    out: &mut Printer<W>,
    tool: &Tool,
    root: &Path,
    runner: &mut Runner<'_>,
) -> bool {
    let cmd = Cmd::new(
        Program::Cargo,
        ["install", "--locked", tool.name, "--version", tool.version],
        CARGO_INSTALL_TIMEOUT,
    );
    out.note(&cmd);
    let fix = format!("run `{cmd}` to reproduce");
    match runner(&cmd, root, Output::Inherit) {
        Ok(ran) if ran.success() => true,
        Ok(ran) if ran.status == Status::TimedOut => {
            out.failure(
                &format!(
                    "installing {} did not finish within {}, so it was stopped",
                    tool.name,
                    process::limit_text(cmd.timeout)
                ),
                &TimeoutFix::of(&cmd).to_string(),
            );
            false
        }
        Ok(_) => {
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

#[cfg(test)]
mod tests;
