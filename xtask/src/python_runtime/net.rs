//! Downloads with curl, each file checked against its pinned sha256 before anything reads it,
//! and PBS's build attestation checked with `gh`. Every command goes through the process door.

use super::Failure;
use crate::process::{self, Cmd, Output, Program, Status};
use crate::write;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// curl's `--max-time` per attempt, in seconds: a 60 MB archive on a slow link.
const CURL_MAX_TIME_S: u64 = 10 * 60;
/// curl's `--retry-max-time`: no retry starts this long after the first attempt.
const CURL_RETRY_MAX_TIME_S: u64 = 2 * CURL_MAX_TIME_S;
/// The door's limit for curl: the last retry's start plus one attempt and a minute, so curl
/// always stops itself first and says why.
const DOWNLOAD_TIMEOUT: Duration =
    Duration::from_secs(CURL_RETRY_MAX_TIME_S + CURL_MAX_TIME_S + 60);
/// `gh attestation verify` fetches the attestation and the Sigstore trust root: seconds.
const GH_TIMEOUT: Duration = Duration::from_mins(5);

/// The repository whose release workflow builds and attests the PBS archives.
pub(crate) const PBS_REPO: &str = "astral-sh/python-build-standalone";
/// The workflow that signs PBS's attestations; `gh` refuses one signed by any other.
const PBS_SIGNER: &str = "astral-sh/python-build-standalone/.github/workflows/release.yml";

/// The download URL of a PBS release asset.
pub(crate) fn pbs_url(release: &str, asset: &str) -> String {
    format!(
        "https://github.com/{PBS_REPO}/releases/download/{release}/{}",
        asset.replace('+', "%2B")
    )
}

/// The curl command that downloads `url` to `name` in its working dir, HTTPS only, redirects
/// included.
fn curl(name: &str, url: &str) -> Cmd {
    Cmd::new(
        Program::Named("curl"),
        [
            "-sSfL",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--retry",
            "3",
            "--retry-all-errors",
            "--retry-max-time",
            &CURL_RETRY_MAX_TIME_S.to_string(),
            "--connect-timeout",
            "30",
            "--max-time",
            &CURL_MAX_TIME_S.to_string(),
            "-o",
            name,
            url,
        ],
        DOWNLOAD_TIMEOUT,
    )
}

/// Runs `cmd` in `dir` with its output kept, and fails with what it printed.
pub(crate) fn run(cmd: &Cmd, dir: &Path, what: &str, fix: &str) -> Result<String, Failure> {
    match process::run(cmd, dir, Output::Quiet) {
        Ok(ran) if ran.success() => Ok(ran.stdout),
        Ok(ran) if ran.status == Status::TimedOut => Err(Failure::new(
            process::timeout_message(cmd),
            process::TimeoutFix::of(cmd).to_string(),
        )),
        Ok(ran) => {
            let said = format!("{}\n{}", ran.stdout.trim(), ran.stderr.trim());
            Err(Failure::new(
                format!("{what} failed: `{cmd}` said:\n{}", said.trim()),
                fix,
            ))
        }
        Err(error) => Err(Failure::new(
            format!("{what} failed: could not run `{cmd}`: {error}"),
            fix,
        )),
    }
}

/// The sha256 of `file`, read in pieces, in lowercase hex.
pub(crate) fn sha256_file(file: &Path) -> io::Result<String> {
    let mut reader = File::open(file)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1 << 16];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(buffer.get(..read).unwrap_or_default());
    }
    Ok(hex(&hasher.finalize()))
}

/// `bytes` in lowercase hex.
pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

/// Downloads `url` to `<dir>/<name>` and returns the path once its sha256 is `sha256` (and its
/// size `size`, when given). A file already there with that sha256 is kept, so a second run
/// downloads nothing; one with another is removed and fetched again. A download with the wrong
/// sha256 is removed and fails.
pub(crate) fn fetch_verified(
    dir: &Path,
    name: &str,
    url: &str,
    sha256: &str,
    size: Option<u64>,
) -> Result<PathBuf, Failure> {
    let path = dir.join(name);
    if path.is_file() && sha256_file(&path).is_ok_and(|found| found == sha256) {
        return check_size(&path, url, size);
    }
    if path.exists() {
        remove(dir, name)?;
    }
    fetch(dir, name, url)?;
    let found = sha256_file(&path)
        .map_err(|error| Failure::io(&format!("reading the download of {url}"), &error))?;
    if found != sha256 {
        remove(dir, name)?;
        return Err(Failure::new(
            format!("{url} has sha256 {found}, but the lock pins {sha256}; it was removed"),
            "do not use this file. If the lock is right, the server sent other bytes: try again \
             later and report it. Only `cargo xtask python-runtime bump` changes a pin",
        ));
    }
    check_size(&path, url, size)
}

/// `path` when its size is `size` (or no size is pinned).
fn check_size(path: &Path, url: &str, size: Option<u64>) -> Result<PathBuf, Failure> {
    let found = path
        .metadata()
        .map_err(|error| Failure::io(&format!("reading {}", path.display()), &error))?
        .len();
    match size {
        Some(size) if size != found => Err(Failure::new(
            format!("{url} has {found} bytes, but the lock pins {size}"),
            "the sha256 matched, so the lock's size is wrong: run `cargo xtask python-runtime \
             bump`",
        )),
        _ => Ok(path.to_path_buf()),
    }
}

/// Downloads `url` to `<dir>/<name>`, unchecked: for files whose content is checked by reading it
/// (PBS's SHA256SUMS, `PyPI`'s JSON).
pub(crate) fn fetch(dir: &Path, name: &str, url: &str) -> Result<PathBuf, Failure> {
    write::make_dirs(dir).map_err(|error| Failure::io("making the download folder", &error))?;
    run(
        &curl(name, url),
        dir,
        &format!("downloading {url}"),
        "check the network and that curl 7.71 or newer is on PATH, then run it again",
    )?;
    Ok(dir.join(name))
}

/// The JSON at `url`, downloaded to `<dir>/<name>`.
pub(crate) fn fetch_json(dir: &Path, name: &str, url: &str) -> Result<serde_json::Value, Failure> {
    let path = fetch(dir, name, url)?;
    let bytes = std::fs::read(&path)
        .map_err(|error| Failure::io(&format!("reading {}", path.display()), &error))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| Failure::new(format!("{url} is not JSON: {error}"), "try again later"))
}

/// Removes `<dir>/<name>`.
pub(crate) fn remove(dir: &Path, name: &str) -> Result<(), Failure> {
    write::remove_in(dir, name)
        .map_err(|error| Failure::io(&format!("removing {}", dir.join(name).display()), &error))
}

/// The sha256 `SHA256SUMS` gives `asset`, from its `<sha256>  <name>` lines.
pub(crate) fn sha256sums_entry(sums: &str, asset: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (sha, name) = line.split_once(char::is_whitespace)?;
        (name.trim_start_matches([' ', '*']) == asset).then(|| sha.to_ascii_lowercase())
    })
}

/// Fetches the PBS release's `SHA256SUMS` into `dir` and fails unless it gives `asset` the
/// `sha256` the lock pins.
pub(crate) fn check_pbs_sums(
    dir: &Path,
    release: &str,
    asset: &str,
    sha256: &str,
) -> Result<(), Failure> {
    let sums_path = fetch(dir, "SHA256SUMS", &pbs_url(release, "SHA256SUMS"))?;
    let sums = std::fs::read_to_string(&sums_path)
        .map_err(|error| Failure::io("reading PBS's SHA256SUMS", &error))?;
    match sha256sums_entry(&sums, asset) {
        Some(found) if found == sha256 => Ok(()),
        Some(found) => Err(Failure::new(
            format!("PBS's SHA256SUMS gives {asset} sha256 {found}, but the lock pins {sha256}"),
            "do not build: either the release changed or the lock is wrong. Run `cargo xtask \
             python-runtime bump` and review the diff",
        )),
        None => Err(Failure::new(
            format!("PBS's SHA256SUMS for {release} does not list {asset}"),
            "check runtime.python and runtime.pbs_release in python-runtime/lock.toml",
        )),
    }
}

/// Checks PBS's Sigstore build attestation of `file` with `gh attestation verify`: signed by
/// PBS's release workflow in `PBS_REPO`. Fails closed: any error from gh fails.
pub(crate) fn verify_pbs_attestation(file: &Path) -> Result<(), Failure> {
    let dir = file.parent().unwrap_or(Path::new("."));
    let name = file
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cmd = Cmd::new(
        Program::Named("gh"),
        [
            "attestation",
            "verify",
            &name,
            "--repo",
            PBS_REPO,
            "--signer-workflow",
            PBS_SIGNER,
        ],
        GH_TIMEOUT,
    );
    run(
        &cmd,
        dir,
        &format!("checking PBS's build attestation of {name}"),
        "do not build from this file. gh 2.49 or newer must be on PATH with a token (GH_TOKEN); \
         `--no-attestation` skips this for a local try only, never in CI",
    )
    .map(drop)
}
