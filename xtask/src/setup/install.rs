//! Installing a pinned binary: on macOS and Linux, download the release asset, check its pinned
//! sha256 and only then move the binary into `CARGO_HOME/bin`; on Windows, run winget, which
//! checks the installer hash from its own manifest.

use super::{Binary, Machine, Packing, Runner};
use crate::output::Printer;
use crate::process::{Cmd, Ended, Output, Program};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs::{self, File, TryLockError};
use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

/// curl's `--retry`: a failed attempt (a reset, a 502 from GitHub) is tried again this many times.
const CURL_RETRIES: u64 = 3;
/// curl's `--max-time`, the limit on each attempt, in seconds.
const CURL_MAX_TIME_S: u64 = 5 * 60;
/// What curl may take beyond its attempts: the waits between them (1, 2 then 4 s) and startup.
const CURL_MARGIN_S: u64 = 60;
/// `run_timed`'s limit for curl: every attempt at its `--max-time`, plus the margin. It is longer
/// than curl's own limits together, so curl always stops itself first and says why.
const DOWNLOAD_TIMEOUT: Duration =
    Duration::from_secs((CURL_RETRIES + 1) * CURL_MAX_TIME_S + CURL_MARGIN_S);
const UNPACK_TIMEOUT: Duration = Duration::from_secs(60);
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
    let message = match runner(cmd, root, Output::Inherit, WINGET_TIMEOUT) {
        Ok(Ended::Exited(ran)) if ran.success => return true,
        Ok(Ended::Exited(_)) => format!("installing {} failed", binary.name),
        Ok(Ended::TimedOut) => format!(
            "installing {} did not finish within {} min, so it was stopped",
            binary.name,
            WINGET_TIMEOUT.as_secs() / 60
        ),
        Err(error) => format!("could not run winget to install {}: {error}", binary.name),
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
        "{STAGING_PREFIX}{}-{}",
        binary.name,
        std::process::id()
    ));
    let placed = fresh_dir(bin, &staging)
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
    match fs::remove_dir_all(&staging) {
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

/// The start of the name of every staging directory `setup` makes in bin.
const STAGING_PREFIX: &str = ".xtask-setup-";

/// Makes `bin` if needed, and `staging` in it, empty.
fn fresh_dir(bin: &Path, staging: &Path) -> Result<(), Failure> {
    let made = fs::create_dir_all(bin).and_then(|()| match fs::remove_dir_all(staging) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => fs::create_dir(staging),
    });
    made.map_err(|error| {
        (
            format!("could not make {}: {error}", staging.display()),
            retry_fix(bin),
        )
    })
}

/// The file in bin whose exclusive lock `setup --binaries` holds for the whole run, so one run at a
/// time uses bin. It is never deleted, and `STAGING_PREFIX` does not match it.
const RUN_LOCK: &str = ".xtask-setup.lock";

/// On macOS and Linux, takes the run lock in `machine`'s cargo bin, waiting while another
/// `setup --binaries` holds it, then removes every `STAGING_PREFIX` entry. With the lock held no
/// other run uses one, so each is from a run that was interrupted (Ctrl-C, a kill). Prints a
/// failure for what fails. Returns the lock, held until dropped, and whether both steps worked.
pub(crate) fn lock_bin_and_sweep<W: Write>(
    out: &mut Printer<W>,
    machine: &Machine<'_>,
) -> (Option<File>, bool) {
    let Some(bin) = machine
        .cargo_bin
        .as_deref()
        .filter(|_| machine.os != "windows")
    else {
        return (None, true);
    };
    let lock = match lock_run(out, bin) {
        Ok(lock) => lock,
        Err(error) => {
            out.failure(
                &format!("could not lock {}: {error}", bin.join(RUN_LOCK).display()),
                &format!(
                    "check that you can write to {} and that its filesystem supports file locks, \
                     then run cargo xtask setup --binaries again",
                    bin.display()
                ),
            );
            return (None, false);
        }
    };
    let swept = remove_staging_leftovers(bin);
    if let Err(error) = &swept {
        out.failure(
            &format!(
                "could not remove what an interrupted setup left in {}: {error}",
                bin.display()
            ),
            &format!(
                "remove the {STAGING_PREFIX}* entries in {} by hand",
                bin.display()
            ),
        );
    }
    (Some(lock), swept.is_ok())
}

/// Opens `RUN_LOCK` in `bin`, making both if needed, and takes its exclusive lock, waiting while
/// another run holds it.
fn lock_run<W: Write>(out: &mut Printer<W>, bin: &Path) -> io::Result<File> {
    fs::create_dir_all(bin)?;
    let lock = File::create(bin.join(RUN_LOCK))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            out.note(format!(
                "another cargo xtask setup --binaries is using {}: waiting for it to finish",
                bin.display()
            ));
            lock.lock()?;
        }
        Err(TryLockError::Error(error)) => return Err(error),
    }
    Ok(lock)
}

/// Removes every entry in `bin` whose name starts with `STAGING_PREFIX`. A symlink is removed,
/// never followed.
fn remove_staging_leftovers(bin: &Path) -> io::Result<()> {
    for entry in fs::read_dir(bin)? {
        let entry = entry?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(STAGING_PREFIX)
        {
            continue;
        }
        let path = entry.path();
        if fs::symlink_metadata(&path)?.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
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
         setup --binaries again"
    );
    let what = format!("downloading {url}");
    run_step(runner, &curl, staging, DOWNLOAD_TIMEOUT, &what, &fix)?;
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
            let tar = Cmd::new(
                Program::Named("tar"),
                ["--no-same-owner", "-xzf", asset, name],
            );
            let fix = format!(
                "the download matched its sha256, so check tar: run `{tar}` by hand on a \
                 download of {url} and check its output, then run cargo xtask setup --binaries \
                 again"
            );
            run_step(
                runner,
                &tar,
                staging,
                UNPACK_TIMEOUT,
                &format!("unpacking {asset}"),
                &fix,
            )?;
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
    make_executable(&unpacked)
        .and_then(|()| fs::rename(&unpacked, &destination))
        .map_err(|error| {
            (
                format!("could not move {name} into {}: {error}", bin.display()),
                retry_fix(bin),
            )
        })
}

/// The curl command that downloads `url` to `asset`. A failed attempt (a reset, a 502) is tried
/// again `CURL_RETRIES` times; `--retry-all-errors` is needed for a reset, which curl 8.7 (macOS)
/// does not retry otherwise. A retry truncates what the attempt before wrote. curl's own limits
/// end it even when xtask is gone (Ctrl-C reaches xtask, not curl's group).
fn curl_cmd(asset: &str, url: &str) -> Cmd {
    Cmd::new(
        Program::Named("curl"),
        [
            "-sSfL",
            "--retry",
            &CURL_RETRIES.to_string(),
            "--retry-all-errors",
            "--connect-timeout",
            "30",
            "--max-time",
            &CURL_MAX_TIME_S.to_string(),
            "-o",
            asset,
            url,
        ],
    )
}

/// Runs one step of a download through `runner`. `what` names the step in its failure.
fn run_step(
    runner: &mut Runner<'_>,
    cmd: &Cmd,
    dir: &Path,
    timeout: Duration,
    what: &str,
    fix: &str,
) -> Result<(), Failure> {
    let message = match runner(cmd, dir, Output::Quiet, timeout) {
        Ok(Ended::Exited(ran)) if ran.success => return Ok(()),
        Ok(Ended::Exited(ran)) => {
            let said = ran.stderr.trim();
            format!(
                "{what} failed: {}",
                if said.is_empty() { "no output" } else { said }
            )
        }
        Ok(Ended::TimedOut) => format!(
            "{what} did not finish within {} s, so it was stopped",
            timeout.as_secs()
        ),
        Err(error) => format!(
            "{what} failed: could not run {}: {error}",
            cmd.program.name()
        ),
    };
    Err((message, fix.to_owned()))
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

/// On Unix makes `file` executable.
fn make_executable(file: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(file, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
