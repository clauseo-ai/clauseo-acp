//! Installing a pinned binary: on macOS and Linux, download the release asset, check its pinned
//! sha256 and only then move the binary into `CARGO_HOME/bin`; on Windows, run winget, which
//! checks the installer hash from its own manifest.

use super::{Binary, Machine, Packing, Runner};
use crate::output::Printer;
use crate::process::{self, Cmd, Ended, Output, Program};
use crate::write;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

mod staging;

pub(crate) use staging::lock_bin_and_sweep;

/// curl's `--retry`: a failed attempt (a reset, a 502 from GitHub) is tried again this many times.
const CURL_RETRIES: u64 = 3;
/// curl's `--max-time`, the limit on each attempt, in seconds.
const CURL_MAX_TIME_S: u64 = 5 * 60;
/// curl's `--retry-max-time`: no retry starts this long after the first attempt did, and curl
/// gives up rather than wait out a `Retry-After` that would pass it.
const CURL_RETRY_MAX_TIME_S: u64 = 3 * CURL_MAX_TIME_S;
/// What curl may take beyond its limits, to start up and exit.
const CURL_MARGIN_S: u64 = 60;
/// The door's limit for curl: the last retry starts by `--retry-max-time` and runs at most
/// `--max-time`, plus the margin. So curl always stops itself first and says why.
const DOWNLOAD_TIMEOUT: Duration =
    Duration::from_secs(CURL_RETRY_MAX_TIME_S + CURL_MAX_TIME_S + CURL_MARGIN_S);
/// tar unpacks one binary of tens of MB from a local file, which takes seconds.
const UNPACK_TIMEOUT: Duration = Duration::from_secs(60);
/// winget downloads and runs an installer, which takes minutes on a slow link.
const WINGET_TIMEOUT: Duration = Duration::from_mins(30);

/// How a binary installs on one OS and arch.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Install {
    /// Windows: this winget command.
    Winget(Cmd),
    /// macOS and Linux: this release asset.
    Download(Download),
    /// No release asset with a pinned sha256 exists for this OS and arch.
    NoRelease,
}

/// A release asset to download, and the sha256 it must have.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Download {
    /// Its file name.
    pub asset: String,
    pub url: String,
    pub sha256: &'static str,
    pub packing: Packing,
}

/// How `binary` installs on `os`/`arch`, as in `std::env::consts`.
pub(crate) fn install_plan(binary: &Binary, os: &str, arch: &str) -> Install {
    if os == "windows" {
        return Install::Winget(winget(binary));
    }
    let pinned = binary
        .sha256
        .iter()
        .find(|pin| pin.os == os && pin.arch == arch);
    match ((binary.asset)(os, arch, binary.version), pinned) {
        (Some(asset), Some(pin)) => Install::Download(Download {
            url: format!(
                "https://github.com/{}/releases/download/v{}/{asset}",
                binary.repo, binary.version
            ),
            asset,
            sha256: pin.sha256,
            packing: binary.packing,
        }),
        _ => Install::NoRelease,
    }
}

/// What to do to install `binary` on `os`/`arch`: the line `setup` prints, and the fix of a
/// version check that fails.
pub(crate) fn how_to_install(binary: &Binary, os: &str, arch: &str) -> String {
    match install_plan(binary, os, arch) {
        Install::Winget(cmd) => format!("run `{cmd}`"),
        Install::Download(download) => format!(
            "run `cargo xtask setup --binaries`, which downloads {} and installs {} only if its \
             sha256 matches the pin",
            download.url, binary.name
        ),
        Install::NoRelease => format!(
            "download v{} for {os}-{arch} from https://github.com/{}/releases and put it on PATH",
            binary.version, binary.repo
        ),
    }
}

/// The `winget` command that installs `binary` on Windows. It runs without prompts: winget asks
/// nothing about source or package agreements, and never goes interactive.
pub(crate) fn winget(binary: &Binary) -> Cmd {
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
        WINGET_TIMEOUT,
    )
}

/// Installs `binary` on `machine`, printing what it does. Every failure prints its message and
/// fix. Returns whether it installed.
pub(crate) fn install_binary<W: Write>(
    out: &mut Printer<W>,
    binary: &Binary,
    machine: &Machine<'_>,
    root: &Path,
    runner: &mut Runner<'_>,
) -> bool {
    let (os, arch) = (machine.os, machine.arch);
    match install_plan(binary, os, arch) {
        Install::NoRelease => {
            out.failure(
                &format!(
                    "{} has no release for {os}-{arch}, so it is not installed",
                    binary.name
                ),
                &how_to_install(binary, os, arch),
            );
            false
        }
        Install::Winget(cmd) => run_winget(out, binary, &cmd, root, runner),
        Install::Download(download) => {
            let Some(bin) = &machine.cargo_bin else {
                out.failure(
                    &format!(
                        "{} is not installed: neither CARGO_HOME nor the home directory is set, \
                         so there is no cargo bin directory to put it in",
                        binary.name
                    ),
                    "set CARGO_HOME to cargo's home directory, then run cargo xtask setup \
                     --binaries again",
                );
                return false;
            };
            download_into(out, binary, &download, bin, runner)
        }
    }
}

fn run_winget<W: Write>(
    out: &mut Printer<W>,
    binary: &Binary,
    cmd: &Cmd,
    root: &Path,
    runner: &mut Runner<'_>,
) -> bool {
    out.note(format!(
        "installing {} {}: {cmd}",
        binary.name, binary.version
    ));
    let fix = format!("run `{cmd}` by hand and check its output, then run cargo xtask setup again");
    let (message, fix) = match runner(cmd, root, Output::Inherit) {
        Ok(Ended::Exited(ran)) if ran.success => return true,
        Ok(Ended::Exited(_)) => (format!("installing {} failed", binary.name), fix),
        Ok(Ended::TimedOut) => (
            format!(
                "installing {} did not finish within {}, so it was stopped",
                binary.name,
                process::limit_text(cmd.timeout)
            ),
            process::timeout_fix(cmd),
        ),
        Err(error) => (
            format!("could not run winget to install {}: {error}", binary.name),
            fix,
        ),
    };
    out.failure(&message, &fix);
    false
}

/// A failure's message and fix.
type Failure = (String, String);

/// The fix after a failure on the way to `bin`, when trying again is all there is to do.
fn retry_fix(bin: &Path) -> String {
    format!(
        "check that you can write to {}, then run cargo xtask setup --binaries again",
        bin.display()
    )
}

/// Downloads `download` into a staging directory inside `bin`, so the last move is a rename on one
/// filesystem, and moves the binary into `bin` only when the download's sha256 is the pinned one.
/// The staging directory is removed either way, so a failure leaves `bin` as it was.
fn download_into<W: Write>(
    out: &mut Printer<W>,
    binary: &Binary,
    download: &Download,
    bin: &Path,
    runner: &mut Runner<'_>,
) -> bool {
    out.note(format!(
        "installing {} {}: downloading {}",
        binary.name, binary.version, download.url
    ));
    let staging = bin.join(format!(
        "{}{}-{}",
        staging::STAGING_PREFIX,
        binary.name,
        std::process::id()
    ));
    let placed = staging::fresh_dir(bin, &staging)
        .and_then(|()| fetch_check_place(binary, download, &staging, bin, runner));
    let mut ok = match placed {
        Ok(()) => {
            out.note(format!(
                "installed {} {} to {}: its sha256 matches the pin",
                binary.name,
                binary.version,
                bin.display()
            ));
            true
        }
        Err((message, fix)) => {
            out.failure(&message, &fix);
            false
        }
    };
    match write::remove_staging_dir(&staging) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            out.failure(
                &format!("could not remove {}: {error}", staging.display()),
                &format!("remove {} by hand", staging.display()),
            );
            ok = false;
        }
        _ => {}
    }
    ok
}

/// Downloads into `staging`, checks the sha256, unpacks if needed, and renames the binary into
/// `bin`.
fn fetch_check_place(
    binary: &Binary,
    download: &Download,
    staging: &Path,
    bin: &Path,
    runner: &mut Runner<'_>,
) -> Result<(), Failure> {
    let (name, asset, url) = (binary.name, download.asset.as_str(), download.url.as_str());
    let curl = curl_cmd(asset, url);
    let fix = format!(
        "run `{curl}` by hand in an empty directory and check its output, then run cargo xtask \
         setup --binaries again; setup needs curl 7.71 or newer, for --retry-all-errors"
    );
    let what = format!("downloading {url}");
    run_step(runner, &curl, staging, &what, &fix)?;
    let file = staging.join(asset);
    let found = sha256_of(&file).map_err(|error| {
        (
            format!("could not read the download of {url}: {error}"),
            retry_fix(bin),
        )
    })?;
    if found != download.sha256 {
        return Err((
            format!(
                "{url} has sha256 {found}, but the pin is {}, so {name} is not installed",
                download.sha256
            ),
            "run cargo xtask setup --binaries again. If the sha256 still differs, the file at \
             that URL is not the one the pin was taken from: do not install it by hand"
                .to_owned(),
        ));
    }
    let unpacked = match download.packing {
        Packing::Bare => file,
        Packing::TarGz => {
            let tar = tar_cmd(asset, name);
            let fix = format!(
                "the download matched its sha256, so check tar: run `{tar}` by hand on a \
                 download of {url} and check its output, then run cargo xtask setup --binaries \
                 again"
            );
            run_step(runner, &tar, staging, &format!("unpacking {asset}"), &fix)?;
            staging.join(name)
        }
    };
    let regular = fs::symlink_metadata(&unpacked).map_err(|error| {
        (
            format!("could not read {}: {error}", unpacked.display()),
            retry_fix(bin),
        )
    })?;
    if !regular.is_file() {
        return Err((
            format!("{asset} holds no regular file named {name}, so {name} is not installed"),
            format!(
                "the release at {url} holds no regular file named {name}: check the asset layout \
                 before bumping the pin"
            ),
        ));
    }
    let destination = bin.join(name);
    write::install_executable(&unpacked, &destination).map_err(|error| {
        (
            format!("could not move {name} into {}: {error}", bin.display()),
            retry_fix(bin),
        )
    })
}

/// The curl command that downloads `url` to `asset`. A failed attempt (a reset, a 502) is tried
/// again `CURL_RETRIES` times. `--retry-all-errors` is needed for a reset, which curl 8.7 (macOS)
/// does not retry otherwise, and it needs curl 7.71 or newer. A retry truncates what the attempt
/// before wrote. curl's own limits end it before the door's limit (`DOWNLOAD_TIMEOUT`) does.
fn curl_cmd(asset: &str, url: &str) -> Cmd {
    Cmd::new(
        Program::Named("curl"),
        [
            "-sSfL",
            "--retry",
            &CURL_RETRIES.to_string(),
            "--retry-all-errors",
            "--retry-max-time",
            &CURL_RETRY_MAX_TIME_S.to_string(),
            "--connect-timeout",
            "30",
            "--max-time",
            &CURL_MAX_TIME_S.to_string(),
            "-o",
            asset,
            url,
        ],
        DOWNLOAD_TIMEOUT,
    )
}

/// The tar command that unpacks only `name` from `asset`, owned by the user who runs it.
fn tar_cmd(asset: &str, name: &str) -> Cmd {
    Cmd::new(
        Program::Named("tar"),
        ["--no-same-owner", "-xzf", asset, name],
        UNPACK_TIMEOUT,
    )
}

/// Runs one step of a download through `runner`. `what` names the step in its failure.
fn run_step(
    runner: &mut Runner<'_>,
    cmd: &Cmd,
    dir: &Path,
    what: &str,
    fix: &str,
) -> Result<(), Failure> {
    let message = match runner(cmd, dir, Output::Quiet) {
        Ok(Ended::Exited(ran)) if ran.success => return Ok(()),
        Ok(Ended::Exited(ran)) => {
            let said = said_of(cmd, &ran.stderr);
            format!(
                "{what} failed: {}",
                if said.is_empty() { "no output" } else { &said }
            )
        }
        Ok(Ended::TimedOut) => format!(
            "{what} did not finish within {}, so it was stopped",
            process::limit_text(cmd.timeout)
        ),
        Err(error) => format!(
            "{what} failed: could not run {}: {error}",
            cmd.program.name()
        ),
    };
    Err((message, fix.to_owned()))
}

/// What a failed step says of its stderr. A retried curl prints one `curl: (N) ...` line per failed
/// attempt, and the last one is the attempt that ended the run, so only that line is said. When
/// curl prints none, as when it rejects an option, all of stderr is said.
fn said_of(cmd: &Cmd, stderr: &str) -> String {
    let last_attempt = stderr
        .lines()
        .rev()
        .find(|line| line.starts_with("curl: ("));
    match last_attempt {
        Some(line) if cmd.program == Program::Named("curl") => line.trim().to_owned(),
        _ => stderr.trim().to_owned(),
    }
}

/// The sha256 of `file`'s bytes, in lowercase hex.
pub(crate) fn sha256_of(file: &Path) -> io::Result<String> {
    let digest = Sha256::digest(fs::read(file)?);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        write!(hex, "{byte:02x}").map_err(io::Error::other)?;
    }
    Ok(hex)
}

#[cfg(test)]
mod tests;
