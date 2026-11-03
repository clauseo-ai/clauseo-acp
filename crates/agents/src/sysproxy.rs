//! The OS's static proxy setting, for `retry(agent, use_system_proxy: true)`: macOS's
//! `scutil --proxy`, Windows' Internet Settings. Linux has none. PAC files and proxies that ask
//! for a password are not read (G10).
//!
//! The parsers are plain functions, tested on every OS; reading the OS is not.

#[cfg(any(target_os = "macos", test))]
use std::time::Duration;

use clauseo_process::{Launcher, Timer};

#[cfg(windows)]
mod windows;

/// The longest `scutil --proxy` may run.
#[cfg(target_os = "macos")]
const SCUTIL_TIMEOUT: Duration = Duration::from_secs(5);
/// The most of `scutil`'s output read.
#[cfg(any(target_os = "macos", test))]
const OUTPUT_MAX: u64 = 256 * 1024;

/// The OS's static HTTPS proxy as `host:port` (or `host`), or `None` when none is set.
#[cfg(target_os = "macos")]
pub(crate) async fn system_proxy<T: Timer>(launcher: &dyn Launcher, timer: &T) -> Option<String> {
    let out = run_stdout(
        launcher,
        timer,
        "/usr/sbin/scutil",
        &["--proxy"],
        SCUTIL_TIMEOUT,
    )
    .await?;
    parse_scutil(&out)
}

/// The OS's static HTTPS proxy as `host:port` (or `host`), or `None` when none is set.
#[cfg(windows)]
#[expect(
    clippy::unused_async,
    reason = "one signature on every OS; only macOS's awaits"
)]
pub(crate) async fn system_proxy<T: Timer>(launcher: &dyn Launcher, timer: &T) -> Option<String> {
    let (_, _) = (launcher, timer);
    let (enable, server) = windows::internet_settings()?;
    parse_proxy_server(enable, &server)
}

/// The OS's static HTTPS proxy: Linux has none.
#[cfg(not(any(target_os = "macos", windows)))]
#[expect(
    clippy::unused_async,
    reason = "one signature on every OS; only macOS's awaits"
)]
pub(crate) async fn system_proxy<T: Timer>(launcher: &dyn Launcher, timer: &T) -> Option<String> {
    let (_, _) = (launcher, timer);
    None
}

/// Runs `program args` through `launcher` and returns its stdout, or `None` when it cannot
/// start, exits non-zero or outlasts `limit` (the handle is dropped, which kills its tree).
/// stderr's pump is driven beside the stdout read and the exit wait, as `limits`' readers do.
#[cfg(any(target_os = "macos", test))]
async fn run_stdout<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    program: &str,
    args: &[&str],
    limit: Duration,
) -> Option<String> {
    use std::pin::pin;

    use futures::AsyncReadExt as _;
    use futures::future::{Either, join3, select};

    let spec = clauseo_process::LaunchSpec::new(
        program.into(),
        args.iter().map(std::ffi::OsString::from).collect(),
        std::collections::BTreeMap::new(),
        std::env::temp_dir(),
    )
    .ok()?;
    let clauseo_process::Launched {
        stdout,
        stderr,
        mut handle,
        ..
    } = launcher.launch(&spec).ok()?;
    let ran = {
        let work = pin!(async {
            let read = async {
                let mut out = Vec::new();
                let mut limited = stdout.take(OUTPUT_MAX);
                limited.read_to_end(&mut out).await.map(|_| out)
            };
            let (read, drained, report) = join3(read, stderr.drain(), handle.exited()).await;
            drop(drained);
            (read, report.code())
        });
        match select(work, pin!(timer.after(limit))).await {
            Either::Left(((read, code), _)) => Some((read, code)),
            Either::Right(((), _)) => None,
        }
    };
    drop(handle);
    let (read, code) = ran?;
    let out = read.ok()?;
    (code == Some(0)).then(|| String::from_utf8_lossy(&out).into_owned())
}

/// Reads `scutil --proxy`'s dictionary: `HTTPSEnable : 1`, `HTTPSProxy : host` and
/// `HTTPSPort : port`. `None` unless HTTPS proxying is on with a host.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn parse_scutil(text: &str) -> Option<String> {
    let mut enabled = false;
    let mut host = None;
    let mut port = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(" : ") else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "HTTPSEnable" => enabled = value == "1",
            "HTTPSProxy" if !value.is_empty() => host = Some(value.to_owned()),
            "HTTPSPort" => port = value.parse::<u16>().ok(),
            _ => {}
        }
    }
    let host = host.filter(|_| enabled)?;
    Some(match port {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

/// Reads Internet Settings' `ProxyEnable` and `ProxyServer`: `host:port` for every protocol, or
/// `http=host:port;https=host:port;...` per protocol, of which the `https` entry is taken. A
/// leading `http://` on the address is dropped. `None` when disabled or with no HTTPS entry.
#[cfg(any(windows, test))]
pub(crate) fn parse_proxy_server(enable: u32, server: &str) -> Option<String> {
    if enable == 0 {
        return None;
    }
    let server = server.trim();
    let address = if server.contains('=') {
        server.split(';').find_map(|entry| {
            let (scheme, address) = entry.split_once('=')?;
            scheme
                .trim()
                .eq_ignore_ascii_case("https")
                .then_some(address.trim())
        })?
    } else {
        server.split(';').next().unwrap_or_default().trim()
    };
    let address = address
        .strip_prefix("http://")
        .unwrap_or(address)
        .trim_end_matches('/');
    (!address.is_empty()).then(|| address.to_owned())
}

#[cfg(test)]
mod tests;
