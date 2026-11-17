//! The production tools ([`SystemTools`]): curl for the version list and the zip, ditto (macOS)
//! or tar (Windows) to unpack, Windows' Authenticode check, and the free space on the disk. No
//! signature check on macOS: Chrome for Testing there is ad hoc signed, with no team id.
//!
//! Each tool is an absolute path, never a `PATH` lookup: `/usr/bin/curl` and `/usr/bin/ditto`
//! on macOS; `curl.exe` and `tar.exe` from `GetSystemDirectoryW()` (System32)
//! on Windows. A tool that is not there is `unsupported`. Each runs through
//! `clauseo_process::spawn_in_tree`, in a process tree of its own, and is watched every 250 ms:
//! a cancel ends its tree and waits for it to exit before the working folder is removed.
//!
//! Every curl gets `-q` first (no `.curlrc` may add flags), [`CURL_FLAGS`], a total time bound
//! ([`VERSIONS_MAX_SECS`], [`ZIP_MAX_SECS`]) and, when `network.upstream_proxy` is set (the
//! firm's policy value over the lawyer's), `--proxy` with it. The zip's download writes the
//! response headers with `-D`, and the progress comes from polling the partial file's size
//! against the final response's `Content-Length`.
//!
//! curl's exit decides the failure ([`curl_exit`]), as agent-browser's own `install` tells its
//! errors apart: a TLS failure is `tls`, an HTTP 4xx `server_refused`, a write error
//! `disk_space`, a redirect `host_refused`; only a failed or broken connection, a timeout or an
//! HTTP 5xx is `network`, which the lawyer's download tries again.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launched, SpawnOptions, Timer, spawn_in_tree};
use clauseo_protocol::BrowserInstallFailure;
use futures::AsyncReadExt;
use futures::future::{self, Either};

use super::{Cancel, Failure, InstallTools};
use crate::browser::chrome::Platform;

/// The flags every curl gets: HTTPS only, redirects included, TLS 1.2 or later, an HTTP error
/// fails, no redirect followed (one fails as curl's exit 47, `host_refused`: HTTPS to Google's
/// host is the integrity check, so a redirect elsewhere must not replace it; neither URL
/// redirects today), at most 400 MiB, 30 s to connect, and a transfer slower than 1 byte a
/// second for 60 s is ended.
pub const CURL_FLAGS: [&str; 17] = [
    "--proto",
    "=https",
    "--proto-redir",
    "=https",
    "--tlsv1.2",
    "--fail",
    "--location",
    "--max-redirs",
    "0",
    "--max-filesize",
    "419430400",
    "--connect-timeout",
    "30",
    "-y",
    "60",
    "-Y",
    "1",
];

/// The most the version list's fetch may take in all, connecting included: agent-browser's 120 s.
pub const VERSIONS_MAX_SECS: u32 = 120;

/// The most the zip's download may take in all: an hour, about 200 MB at under 0.5 Mbit/s, where
/// agent-browser's 120 s would end a download slower than about 13 Mbit/s. A transfer that drips
/// just above the stall limit ends here.
pub const ZIP_MAX_SECS: u32 = 3600;

/// curl's exit code for an HTTP error under `--fail`; the status is in its message.
pub const CURL_HTTP_ERROR: i32 = 22;

/// curl's exit code when it could not write what it received: on this path, a full disk.
pub const CURL_WRITE_ERROR: i32 = 23;

/// curl's exit codes for a TLS failure: the handshake (35), the peer's certificate (51, 60, 83,
/// 90, 91), a local certificate or CA store problem (58, 77, 82), the cipher or the crypto engine
/// (53, 54, 59, 66), the TLS shutdown (80).
pub const CURL_TLS: [i32; 14] = [35, 51, 53, 54, 58, 59, 60, 66, 77, 80, 82, 83, 90, 91];

/// curl's exit codes for a failed or broken connection, worth another try: the proxy or the
/// host not resolved (5, 6), no connection (7), HTTP/2 or HTTP/3 errors (16, 92, 95), a partial
/// transfer (18), a timeout or the stall limit (28), an empty reply (52), sending or receiving
/// failed (55, 56).
pub const CURL_CONNECTION: [i32; 11] = [5, 6, 7, 16, 18, 28, 52, 55, 56, 92, 95];

/// curl's exit code when `--max-filesize` stops a transfer.
pub const CURL_TOO_LARGE: i32 = 63;

/// curl's exit code when a redirect goes past `--max-redirs` (0: any redirect).
pub const CURL_REDIRECTED: i32 = 47;

/// How often a running tool is looked at: cancel, and the download's progress.
const TICK: Duration = Duration::from_millis(250);

/// The most the version list may be.
const MAX_VERSIONS_BYTES: usize = 1024 * 1024;

/// The most of a tool's stderr kept, for the log.
const MAX_STDERR_BYTES: usize = 64 * 1024;

/// The production tools.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemTools;

impl InstallTools for SystemTools {
    fn free_bytes(&self, folder: &Path) -> std::io::Result<u64> {
        free_bytes(folder)
    }

    fn fetch_versions(
        &self,
        url: &str,
        work: &Path,
        proxy: Option<&str>,
        cancel: &Cancel,
    ) -> Result<Vec<u8>, Failure> {
        let curl = tool("curl")?;
        let ran = run_tool(
            &curl,
            versions_args(url, proxy),
            work,
            cancel,
            MAX_VERSIONS_BYTES,
            &mut || {},
        )
        .map_err(|stop| stop.failure(BrowserInstallFailure::Network))?;
        curl_result(&ran)?;
        Ok(ran.stdout)
    }

    fn download(
        &self,
        url: &str,
        zip: &Path,
        proxy: Option<&str>,
        cancel: &Cancel,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<Option<u64>, Failure> {
        let curl = tool("curl")?;
        let work = zip.parent().unwrap_or(zip);
        let headers = work.join("headers.txt");
        let args = download_args(url, proxy, &headers, zip);
        let mut last = None;
        let mut tick = || {
            let total = std::fs::read_to_string(&headers)
                .ok()
                .and_then(|text| content_length(&text));
            let bytes = std::fs::symlink_metadata(zip).map_or(0, |meta| meta.len());
            if let Some(total) = total
                && last != Some((bytes, total))
            {
                last = Some((bytes, total));
                progress(bytes, total);
            }
        };
        let ran = run_tool(&curl, args, work, cancel, 0, &mut tick)
            .map_err(|stop| stop.failure(BrowserInstallFailure::Network))?;
        curl_result(&ran)?;
        tick();
        Ok(std::fs::read_to_string(&headers)
            .ok()
            .and_then(|text| content_length(&text)))
    }

    fn unpack(&self, zip: &Path, into: &Path, cancel: &Cancel) -> Result<(), Failure> {
        let (program, args): (PathBuf, Vec<OsString>) = if cfg!(windows) {
            let tar = tool("tar")?;
            let args = vec!["-xf".into(), zip.into(), "-C".into(), into.into()];
            (tar, args)
        } else {
            let ditto = tool("ditto")?;
            (
                ditto,
                vec!["-x".into(), "-k".into(), zip.into(), into.into()],
            )
        };
        let ran = run_tool(&program, args, into, cancel, 0, &mut || {})
            .map_err(|stop| stop.failure(BrowserInstallFailure::Unpack))?;
        if ran.code == Some(0) {
            Ok(())
        } else {
            let full = ran.stderr.contains("No space left on device");
            Err(Failure::new(
                if full {
                    BrowserInstallFailure::DiskSpace
                } else {
                    BrowserInstallFailure::Unpack
                },
                format!(
                    "unpacking exited with {:?}: {}",
                    ran.code,
                    ran.stderr.trim()
                ),
            ))
        }
    }

    fn verify_signature(
        &self,
        platform: Platform,
        folder: &Path,
        _cancel: &Cancel,
    ) -> Result<(), String> {
        match platform {
            Platform::MacArm64 | Platform::MacX64 => {
                Err("no signature check on macOS: Chrome for Testing is ad hoc signed".to_owned())
            }
            #[cfg(windows)]
            Platform::WindowsX64 => super::windows::verify_authenticode(
                &folder.join(platform.cft_binary().unwrap_or_default()),
            ),
            #[cfg(not(windows))]
            Platform::WindowsX64 => {
                let _ = folder;
                Err("no Authenticode check off Windows".to_owned())
            }
            Platform::WindowsArm64 | Platform::Linux | Platform::Other => {
                Err(format!("no signature check for {platform:?}"))
            }
        }
    }
}

/// The version list's curl arguments: its time bound, the firm's proxy, then `url`.
pub(super) fn versions_args(url: &str, proxy: Option<&str>) -> Vec<OsString> {
    let mut args = curl_args(["--max-time", VERSIONS_MAX_SECS.to_string().as_str()]);
    args.extend(proxy_args(proxy));
    args.push(url.into());
    args
}

/// The zip download's curl arguments: its time bound, the firm's proxy, the response headers to
/// `headers`, the body to `zip`.
pub(super) fn download_args(
    url: &str,
    proxy: Option<&str>,
    headers: &Path,
    zip: &Path,
) -> Vec<OsString> {
    let mut args = curl_args(["--max-time", ZIP_MAX_SECS.to_string().as_str()]);
    args.extend(proxy_args(proxy));
    args.extend([
        "-D".into(),
        headers.into(),
        "-o".into(),
        zip.into(),
        url.into(),
    ]);
    args
}

/// `--proxy` with `network.upstream_proxy` when it is set (an `http` or `https`
/// address with no user name or password, as the setting allows); nothing otherwise.
fn proxy_args(proxy: Option<&str>) -> Vec<OsString> {
    proxy
        .filter(|proxy| !proxy.is_empty())
        .map(|proxy| vec!["--proxy".into(), proxy.into()])
        .unwrap_or_default()
}

/// curl's arguments: `-q` first, the fixed flags, quiet but for errors, then `rest`.
fn curl_args<'a, S: Into<OsString> + 'a>(rest: impl IntoIterator<Item = S>) -> Vec<OsString> {
    ["-q", "--silent", "--show-error"]
        .into_iter()
        .chain(CURL_FLAGS)
        .map(OsString::from)
        .chain(rest.into_iter().map(Into::into))
        .collect()
}

/// A curl run's exit: 0 is fine, [`CURL_TOO_LARGE`] is `size_mismatch`, [`CURL_REDIRECTED`]
/// `host_refused` (a retry would meet the same redirect), [`CURL_HTTP_ERROR`] `server_refused`
/// for a 4xx and `network` (tried again) for a 5xx, as agent-browser does, [`CURL_WRITE_ERROR`]
/// `disk_space`, [`CURL_TLS`] `tls`, and [`CURL_CONNECTION`] `network`, which may be tried
/// again. Any other exit (or none: a signal) is `network` too, but final: it is no connection
/// failure, so another try would not help.
pub fn curl_exit(code: Option<i32>, stderr: &str) -> Result<(), Failure> {
    let said = |what: &str| format!("{what} (curl exited with {code:?}: {})", stderr.trim());
    match code {
        Some(CURL_HTTP_ERROR) if http_status(stderr).is_some_and(|status| status >= 500) => Err(
            Failure::new(BrowserInstallFailure::Network, said("the server failed")),
        ),
        Some(CURL_HTTP_ERROR) => Err(Failure::new(
            BrowserInstallFailure::ServerRefused,
            said("the server answered with an HTTP error"),
        )),
        Some(CURL_WRITE_ERROR) => Err(Failure::new(
            BrowserInstallFailure::DiskSpace,
            said("the download could not be written: the disk is likely full"),
        )),
        Some(code) if CURL_TLS.contains(&code) => Err(Failure::new(
            BrowserInstallFailure::Tls,
            said("the secure connection failed"),
        )),
        Some(code) if CURL_CONNECTION.contains(&code) => Err(Failure::new(
            BrowserInstallFailure::Network,
            said("the connection failed"),
        )),
        Some(0) => Ok(()),
        Some(CURL_TOO_LARGE) => Err(Failure::new(
            BrowserInstallFailure::SizeMismatch,
            "the download is larger than the 400 MiB cap",
        )),
        Some(CURL_REDIRECTED) => Err(Failure::new(
            BrowserInstallFailure::HostRefused,
            "the server answered with a redirect, which the host does not follow",
        )),
        _ => Err(Failure::new(BrowserInstallFailure::Network, said("curl failed")).last_try()),
    }
}

/// The HTTP status in curl's `--fail` message ("The requested URL returned error: 404").
fn http_status(stderr: &str) -> Option<u16> {
    let (_, rest) = stderr.split_once("returned error:")?;
    rest.trim_start()
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

fn curl_result(ran: &Ran) -> Result<(), Failure> {
    curl_exit(ran.code, &ran.stderr)
}

/// The final response's `Content-Length` in curl's `-D` file: the header block after the last
/// status line, once that block is complete (its blank line written) and its status is 2xx.
pub fn content_length(headers: &str) -> Option<u64> {
    let mut length = None;
    let mut ok = false;
    let mut complete = false;
    // A line counts once its newline is written: curl may be partway through one.
    for line in headers
        .split_inclusive('\n')
        .filter_map(|line| line.strip_suffix('\n'))
        .map(|line| line.trim_end_matches('\r'))
    {
        if line.starts_with("HTTP/") {
            ok = line
                .split(' ')
                .nth(1)
                .is_some_and(|code| code.starts_with('2'));
            length = None;
            complete = false;
        } else if line.is_empty() {
            complete = true;
        } else if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().ok();
        }
    }
    length.filter(|_| ok && complete)
}

/// The system tool `name`, by its fixed absolute path: `/usr/bin` on macOS, System32 on
/// Windows; `unsupported` when it is not there, and on every other OS.
fn tool(name: &str) -> Result<PathBuf, Failure> {
    #[cfg(target_os = "macos")]
    let path = Some(Path::new("/usr/bin").join(name));
    #[cfg(windows)]
    let path = super::windows::system_dir().map(|dir| dir.join(format!("{name}.exe")));
    #[cfg(not(any(target_os = "macos", windows)))]
    let path: Option<PathBuf> = None;
    match path {
        Some(path) if std::fs::metadata(&path).is_ok_and(|meta| meta.is_file()) => Ok(path),
        _ => Err(Failure::new(
            BrowserInstallFailure::Unsupported,
            format!("the system tool {name} is not there"),
        )),
    }
}

/// The bytes free to this user on the volume that holds `folder`.
#[cfg(unix)]
fn free_bytes(folder: &Path) -> std::io::Result<u64> {
    let stat = rustix::fs::statvfs(folder)?;
    Ok(stat.f_bavail.saturating_mul(stat.f_frsize))
}

#[cfg(windows)]
fn free_bytes(folder: &Path) -> std::io::Result<u64> {
    super::windows::free_bytes(folder)
}

/// What a finished tool left.
struct Ran {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: String,
}

/// Why a tool did not finish.
enum Stop {
    Cancelled,
    /// It did not start, or wrote more than its cap to stdout.
    Failed(String),
}

impl Stop {
    /// The failure: a tool that did not start, or said too much, is not tried again.
    fn failure(self, code: BrowserInstallFailure) -> Failure {
        match self {
            Self::Cancelled => Failure::cancelled(),
            Self::Failed(message) => Failure::new(code, message).last_try(),
        }
    }
}

/// Runs `program` with `args` in `cwd` to its exit, calling `tick` every 250 ms. A cancel, or
/// more than `stdout_cap` bytes on stdout, ends its tree.
fn run_tool(
    program: &Path,
    args: Vec<OsString>,
    cwd: &Path,
    cancel: &Cancel,
    stdout_cap: usize,
    tick: &mut dyn FnMut(),
) -> Result<Ran, Stop> {
    let spec = LaunchSpec::new(program.to_owned(), args, BTreeMap::new(), cwd.to_owned())
        .map_err(|error| Stop::Failed(error.to_string()))?;
    let launched = spawn_in_tree(&spec, &SpawnOptions::new())
        .map_err(|error| Stop::Failed(format!("{} did not start: {error}", program.display())))?;
    futures::executor::block_on(supervise(launched, cancel, stdout_cap, tick))
}

/// Drives a started tool: its stdout (to `cap`), its stderr, and its exit or the cancel.
async fn supervise(
    launched: Launched,
    cancel: &Cancel,
    cap: usize,
    tick: &mut dyn FnMut(),
) -> Result<Ran, Stop> {
    let Launched {
        stdin,
        stdout,
        stderr,
        mut handle,
        ..
    } = launched;
    drop(stdin);
    let errors = RefCell::new(String::new());
    let outcome = {
        let io = pin!(async {
            let limit = u64::try_from(cap).unwrap_or(u64::MAX).saturating_add(1);
            let mut out = Vec::new();
            let mut limited = stdout.take(limit);
            let read = limited.read_to_end(&mut out);
            let pump = stderr.lines(|line| {
                let mut errors = errors.borrow_mut();
                if errors.len() < MAX_STDERR_BYTES {
                    errors.push_str(line);
                    errors.push('\n');
                }
            });
            let (read, _pumped) = futures::join!(read, pump);
            match read {
                Ok(_) if out.len() > cap => Err(Stop::Failed("too much output".to_owned())),
                Ok(_) => Ok(out),
                Err(error) => Err(Stop::Failed(format!("reading the output: {error}"))),
            }
        });
        let wait = pin!(async {
            loop {
                let exited = pin!(handle.exited());
                let timer = pin!(async_io::Timer::after(TICK));
                match future::select(exited, timer).await {
                    Either::Left((report, _)) => return Ok(report),
                    Either::Right(_) if cancel.is_cancelled() => return Err(Stop::Cancelled),
                    Either::Right(_) => tick(),
                }
            }
        });
        match future::select(wait, io).await {
            Either::Left((Ok(report), io)) => io.await.map(|out| (report, out)),
            Either::Left((Err(stop), _)) | Either::Right((Err(stop), _)) => Err(stop),
            Either::Right((Ok(out), wait)) => wait.await.map(|report| (report, out)),
        }
    };
    match outcome {
        Ok((report, stdout)) => Ok(Ran {
            code: report.code(),
            stdout,
            stderr: errors.into_inner(),
        }),
        Err(stop) => {
            // Ends the tree and waits for it to exit, so nothing still writes into the working
            // folder when it is removed. No listing: the group (Unix) or the job (Windows) holds
            // the tool's whole tree, so the tree kill needs none.
            let _outcome = handle.shutdown(&KillSoon::default(), None).await;
            Err(stop)
        }
    }
}

/// The shutdown ladder's waits with the first one (after stdin closes) skipped: curl, ditto and
/// tar never read stdin, so the ladder goes straight to its signal or job end, then waits.
#[derive(Debug, Default)]
struct KillSoon(AtomicUsize);

impl Timer for KillSoon {
    fn after(&self, duration: Duration) -> impl Future<Output = ()> + Send {
        let first = self.0.fetch_add(1, Ordering::SeqCst) == 0;
        let timer = (!first).then(|| async_io::Timer::after(duration));
        async move {
            if let Some(timer) = timer {
                timer.await;
            }
        }
    }
}
