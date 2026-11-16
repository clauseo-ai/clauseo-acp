//! The child's environment, built from scratch: the guard clears its own and gives agent-browser
//! exactly the map [`build`] returns. Every name the child may see is in this file; the rules and
//! the agent-browser source lines behind them are in `docs/decisions/0029-browser.md`.
//!
//! The host gives every guard the session's egress proxy and its password (decision record 0053),
//! the plain client's Codex entry included. The password is the guard's alone, and comes by file,
//! never by variable: [`PROXY_SECRET_FILE`] names a file the host wrote in the session's private
//! run folder, which [`proxy_secret`] reads once at the guard's start and then deletes (as the
//! case-law connector reads `CLAUSEO_RELAY_TOKEN_FILE`, decision record 0050). No map built here
//! holds the variable, so neither agent-browser, its daemon nor Chrome learns where the file was.
//! The proxy variables carry the bare URL, with no credential: agent-browser's own HTTP client
//! then gets the proxy's 407 and fails closed, and Chrome gets the credential over CDP
//! (`proxy_auth.rs`). `AGENT_BROWSER_PROXY_USERNAME` and `_PASSWORD` are never set: with them
//! agent-browser would intercept each page's requests itself.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

use super::proxy_auth::Password;
use crate::write;
use crate::write::secret::Refused;

/// Which family's rules apply: the names, the folders and how names compare.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Os {
    /// macOS and Linux: names compare exactly.
    Unix,
    /// Windows: names compare without ASCII case, so each is emitted once.
    Windows,
}

impl Os {
    /// The family this binary was built for.
    pub(crate) const CURRENT: Os = if cfg!(windows) { Os::Windows } else { Os::Unix };
}

/// The config file Clauseo owns; an explicit file replaces agent-browser's config discovery.
pub(crate) const CONFIG: &str = "AGENT_BROWSER_CONFIG";
/// The session's egress proxy, `http://127.0.0.1:<port>`, in every entry the host makes now.
/// When it is set it must be valid and come with [`PROXY_SECRET_FILE`] ([`proxy_secret`]), and
/// everything the proxy brings (Chrome's `--proxy-server`, the bypass, the `*_PROXY` variables,
/// the credential) follows it. With neither, no proxy variable reaches a child.
pub(crate) const PROXY: &str = "AGENT_BROWSER_PROXY";
/// The absolute path of the file holding the proxy's password, which the host writes for the
/// guard alone (`<data>/run/<session id>/proxy-secret`): mode 0600 in the session's 0700 run
/// folder, or on Windows with that folder's ACL. It holds exactly 64 of `0-9 a-f`, then at most
/// one `\n`. The guard reads it once at its start, through a handle opened without following a
/// link, then empties and deletes it ([`proxy_secret`]). The variable is never in a child's
/// environment, and the password never on a command line, in a log or in `Debug`. Valid only
/// with [`PROXY`], and required with it.
pub(crate) const PROXY_SECRET_FILE: &str = "CLAUSEO_PROXY_SECRET_FILE";
/// The most of that file the guard reads: twice what a valid one holds.
const SECRET_FILE_MAX_BYTES: u64 = 128;
/// agent-browser's proxy bypass, one of [`FIXED`]: set only with [`PROXY`].
const PROXY_BYPASS: &str = "AGENT_BROWSER_PROXY_BYPASS";
/// The Chrome the host chose.
pub(crate) const EXECUTABLE_PATH: &str = "AGENT_BROWSER_EXECUTABLE_PATH";
/// The Clauseo session id. `AGENT_BROWSER_NAMESPACE` is never set: it would put the socket at
/// `<socket dir>/namespaces/<ns>/run/<session>.sock` (`connection.rs:136-141`), past the Unix
/// limit, and the socket folder is per session already.
pub(crate) const SESSION: &str = "AGENT_BROWSER_SESSION";
/// The socket folder. Unix: the host's folder (`/tmp/clauseo-ab-<16 random>`), from the
/// guard's environment, which `socket.rs` checks. Windows: `<cwd>\run`, whatever the
/// environment says.
pub(crate) const SOCKET_DIR: &str = "AGENT_BROWSER_SOCKET_DIR";
/// Tells the patched agent-browser (route 2's W2) to start no stream server, an unauthenticated
/// listener that drives the browser; set on every system, and agent-browser gets no stream port
/// (`AGENT_BROWSER_STREAM_PORT`, which the host no longer claims) anywhere. agent-browser 0.41.1
/// has no such switch and always starts one on a port the OS picks: on Windows the guard refuses
/// its `.stream` file and ends it (`port_file.rs`); on macOS and Linux it lets it be, until the
/// patched agent-browser ships there.
pub(crate) const STREAM_OFF: &str = "AGENT_BROWSER_STREAM_DISABLED";
/// Chrome's extra arguments, were agent-browser to launch one: only [`WEBRTC_ARG`]. With
/// [`CDP`] set it never does; the guard starts Chrome (`chrome.rs`). The debugging port is no
/// longer pinned here: past agent-browser's own `--remote-debugging-port=0` it stopped Chrome
/// writing the `DevToolsActivePort` file agent-browser waits for (decision record 0029).
pub(crate) const ARGS: &str = "AGENT_BROWSER_ARGS";
/// The browser endpoint of the Chrome the guard started, `ws://127.0.0.1:<--cdp-port>/devtools/
/// browser/<id>`, read from Chrome's stderr. A full `ws://` value: agent-browser connects to it
/// directly (`browser.rs:2920-2923`), where a bare port would send its `/json/version` lookup
/// through the egress proxy (`discovery.rs:307-308`, a plain reqwest client).
pub(crate) const CDP: &str = "AGENT_BROWSER_CDP";
/// Keeps WebRTC's UDP off every path but the proxy.
pub(crate) const WEBRTC_ARG: &str = "--force-webrtc-ip-handling-policy=disable_non_proxied_udp";
/// The lowest port the guard accepts from the host: below it are the privileged ports.
pub(crate) const MIN_PORT: u16 = 1024;

/// Values fixed in the guard, whatever its environment says.
pub(crate) const FIXED: &[(&str, &str)] = &[
    ("AGENT_BROWSER_PROXY_BYPASS", "<-loopback>"),
    ("AGENT_BROWSER_IDLE_TIMEOUT_MS", "600000"),
    ("AGENT_BROWSER_MAX_OUTPUT", "25000"),
    ("AGENT_BROWSER_DEFAULT_TIMEOUT", "30000"),
];

/// Set to the validated proxy URL, so `agent_browser_read`'s own HTTP client goes through the
/// proxy too. On Windows names have no case, so only the upper-case three are set.
const PROXY_VARS_UPPER: &[&str] = &["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"];
const PROXY_VARS_LOWER: &[&str] = &["http_proxy", "https_proxy", "all_proxy"];

/// Unix `PATH`.
const UNIX_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";
/// Windows folder when the Windows directory cannot be read (see [`WindowsDirs`]).
const DEFAULT_WINDOWS_DIR: &str = r"C:\Windows";
/// Windows `PATHEXT` when the guard's environment has none: Windows' own default.
const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

/// Passed through from the guard's environment, only when present. Windows' `SystemRoot`,
/// `windir`, `ComSpec` and `PATHEXT` are not here: [`build`] always sets them.
const PASS_UNIX: &[&str] = &["LANG"];
const PASS_WINDOWS: &[&str] = &[
    "SystemDrive",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "LOCALAPPDATA",
    "APPDATA",
    "USERPROFILE",
];

/// Folders under `--cwd`, each named by the variables that point at it. The Unix socket folder
/// is not among them: the host makes it (`socket.rs`).
const FOLDERS_UNIX: &[(&str, &[&str])] = &[
    ("downloads", &["AGENT_BROWSER_DOWNLOAD_PATH"]),
    ("screenshots", &["AGENT_BROWSER_SCREENSHOT_DIR"]),
    ("home", &["HOME"]),
    ("tmp", &["TMPDIR"]),
];
const FOLDERS_WINDOWS: &[(&str, &[&str])] = &[
    ("downloads", &["AGENT_BROWSER_DOWNLOAD_PATH"]),
    ("run", &[SOCKET_DIR]),
    ("screenshots", &["AGENT_BROWSER_SCREENSHOT_DIR"]),
    ("tmp", &["TEMP", "TMP"]),
];

/// The folders under `--cwd` the child's variables point at, which the guard makes.
pub(crate) fn folders(os: Os, cwd: &Path) -> Vec<PathBuf> {
    folder_table(os)
        .iter()
        .map(|(name, _)| cwd.join(name))
        .collect()
}

/// Windows: `<cwd>\run`, one of [`folders`], where agent-browser writes `<session>.port`.
pub(crate) fn windows_socket_dir(cwd: &Path) -> PathBuf {
    cwd.join("run")
}

fn folder_table(os: Os) -> &'static [(&'static str, &'static [&'static str])] {
    match os {
        Os::Unix => FOLDERS_UNIX,
        Os::Windows => FOLDERS_WINDOWS,
    }
}

/// The Windows folders the child's defaults come from. The caller reads them from Windows
/// (`job/spawn.rs`) and passes them in, so [`build`] stays pure and runs on every OS. `None`
/// where the read failed or off Windows; the defaults then are `C:\Windows` and its `System32`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct WindowsDirs {
    /// The Windows directory (`GetWindowsDirectoryW`): the default `SystemRoot` and `windir`.
    pub(crate) windows: Option<PathBuf>,
    /// The system directory (`GetSystemDirectoryW`): where `ComSpec`'s `cmd.exe` is.
    pub(crate) system: Option<PathBuf>,
}

/// The guard's own environment, in the order the OS gave it.
pub(crate) struct Source {
    vars: Vec<(OsString, OsString)>,
}

impl Source {
    /// This process's environment.
    pub(crate) fn from_process() -> Self {
        Self {
            vars: std::env::vars_os().collect(),
        }
    }

    /// The given pairs, for tests.
    #[cfg(test)]
    pub(crate) fn from_pairs(pairs: &[(&str, &str)]) -> Self {
        Self {
            vars: pairs
                .iter()
                .map(|(name, value)| (OsString::from(name), OsString::from(value)))
                .collect(),
        }
    }

    /// The first value whose name matches `name`: exactly on Unix, without ASCII case on Windows.
    fn get(&self, os: Os, name: &str) -> Option<&OsStr> {
        self.vars
            .iter()
            .find(|(key, _)| match os {
                Os::Unix => key == name,
                Os::Windows => key.eq_ignore_ascii_case(name),
            })
            .map(|(_, value)| value.as_os_str())
    }
}

/// A required variable that is missing or does not hold what the guard accepts.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct EnvError {
    name: &'static str,
    problem: Problem,
}

#[derive(Debug, PartialEq, Eq)]
enum Problem {
    Missing,
    NotFile,
    NotProxy,
    NotSession,
    NotAbsolute,
    Unreadable,
    NotSecret,
    NotSpent,
    NotPlain,
    SecretWithoutProxy,
    ProxyWithoutSecret,
}

impl fmt::Display for EnvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = self.name;
        match self.problem {
            Problem::Missing => write!(f, "{name} is not set; the host sets it"),
            Problem::NotFile => write!(
                f,
                "{name} must be the absolute path of an existing file, not a network or device path"
            ),
            Problem::NotProxy => write!(
                f,
                "{name} must be exactly http://127.0.0.1:<port>, the port from 1 to 65535 with no \
                 leading zero"
            ),
            Problem::NotSession => write!(f, "{name} must be 1 to 64 of A-Z a-z 0-9 _ -"),
            Problem::NotAbsolute => write!(f, "{name} must be an absolute path"),
            Problem::Unreadable => write!(
                f,
                "{name} names a file the guard cannot open to read and empty"
            ),
            Problem::NotPlain => write!(
                f,
                "{name} must name a plain file with one name, not a link, a folder or a device"
            ),
            Problem::NotSecret => write!(
                f,
                "{name} must name a file holding exactly 64 of 0-9 a-f, then at most one newline"
            ),
            Problem::NotSpent => {
                write!(f, "{name} names a file the guard read but could not empty")
            }
            Problem::ProxyWithoutSecret => write!(
                f,
                "{name} is set without {PROXY_SECRET_FILE}, the file of its password; the host \
                 sets both"
            ),
            Problem::SecretWithoutProxy => {
                write!(f, "{name} is set without {PROXY}, whose password it is")
            }
        }
    }
}

/// The child's whole environment but [`CDP`], which [`set_cdp`] adds once Chrome is up. Fails
/// on the first required variable that is missing or invalid, in the order config, proxy (only
/// when set: with none, no proxy variable reaches the child), executable path, session, socket
/// folder (Unix only); never falls back. The Unix
/// socket folder is checked here only for being absolute; `socket.rs` checks the folder itself.
/// On Windows, `dirs` supplies the defaults [`windows_defaults`] gives the variables the guard's
/// environment lacks.
pub(crate) fn build(
    source: &Source,
    os: Os,
    cwd: &Path,
    dirs: &WindowsDirs,
) -> Result<BTreeMap<String, OsString>, EnvError> {
    let config = required(source, os, CONFIG, is_local_file, Problem::NotFile)?;
    let proxy = match source.get(os, PROXY) {
        Some(_) => Some(required(
            source,
            os,
            PROXY,
            |v, _| is_loopback_proxy(v),
            Problem::NotProxy,
        )?),
        None => None,
    };
    let executable = required(source, os, EXECUTABLE_PATH, is_local_file, Problem::NotFile)?;
    let session = required(
        source,
        os,
        SESSION,
        |v, _| is_session(v),
        Problem::NotSession,
    )?;
    let socket_dir = match os {
        Os::Unix => required(
            source,
            os,
            SOCKET_DIR,
            |v, _| is_unix_absolute(v),
            Problem::NotAbsolute,
        )?
        .to_owned(),
        Os::Windows => windows_socket_dir(cwd).into_os_string(),
    };

    let mut env = BTreeMap::new();
    let mut set = |name: &str, value: &OsStr| {
        env.insert(name.to_owned(), value.to_owned());
    };
    set(CONFIG, config);
    if let Some(proxy) = proxy {
        set(PROXY, proxy);
    }
    set(EXECUTABLE_PATH, executable);
    set(SESSION, session);
    set(SOCKET_DIR, &socket_dir);
    set(STREAM_OFF, OsStr::new("1"));
    set(ARGS, OsStr::new(WEBRTC_ARG));
    for (name, value) in FIXED {
        if *name != PROXY_BYPASS || proxy.is_some() {
            set(name, OsStr::new(value));
        }
    }
    if let Some(proxy) = proxy {
        for name in PROXY_VARS_UPPER {
            set(name, proxy);
        }
        if os == Os::Unix {
            for name in PROXY_VARS_LOWER {
                set(name, proxy);
            }
        }
    }
    for (folder, names) in folder_table(os) {
        let path = cwd.join(folder);
        for name in *names {
            set(name, path.as_os_str());
        }
    }
    let passed = match os {
        Os::Unix => PASS_UNIX,
        Os::Windows => PASS_WINDOWS,
    };
    for name in passed {
        if let Some(value) = source.get(os, name) {
            set(name, value);
        }
    }
    match os {
        Os::Unix => set("PATH", OsStr::new(UNIX_PATH)),
        Os::Windows => {
            for (name, value) in windows_defaults(source, dirs) {
                set(name, &value);
            }
        }
    }
    Ok(env)
}

/// Windows' `SystemRoot`, `windir`, `ComSpec`, `PATHEXT` and `PATH`. The first four are the
/// guard's own value when it has one (matched without case, first match wins); otherwise a
/// default: `SystemRoot` the Windows directory from `dirs`, `windir` the resolved `SystemRoot`,
/// `ComSpec` `cmd.exe` in the system directory from `dirs` (else `<SystemRoot>\System32`),
/// `PATHEXT` Windows' own. `PATH` is that system directory, then `<SystemRoot>`. Winsock needs
/// `SystemRoot`; agent-browser, Chrome and the relay need `windir` and `ComSpec` to run a shell,
/// and `PATHEXT` to find `.cmd` and `.exe` programs.
fn windows_defaults(source: &Source, dirs: &WindowsDirs) -> Vec<(&'static str, OsString)> {
    let os = Os::Windows;
    let windows = dirs
        .windows
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_WINDOWS_DIR))
        .into_os_string();
    let root = source
        .get(os, "SystemRoot")
        .map_or(windows, OsStr::to_owned);
    let windir = source
        .get(os, "windir")
        .map_or_else(|| root.clone(), OsStr::to_owned);
    let system = dirs
        .system
        .clone()
        .map_or_else(|| windows_join(&root, "System32"), PathBuf::into_os_string);
    let comspec = source
        .get(os, "ComSpec")
        .map_or_else(|| windows_join(&system, "cmd.exe"), OsStr::to_owned);
    let pathext = source
        .get(os, "PATHEXT")
        .map_or_else(|| OsString::from(DEFAULT_PATHEXT), OsStr::to_owned);
    let mut path = system;
    path.push(";");
    path.push(&root);
    vec![
        ("SystemRoot", root),
        ("windir", windir),
        ("ComSpec", comspec),
        ("PATHEXT", pathext),
        ("PATH", path),
    ]
}

/// `dir\name`: a Windows path, joined with a backslash whatever OS builds it.
fn windows_join(dir: &OsStr, name: &str) -> OsString {
    let mut joined = dir.to_owned();
    joined.push(r"\");
    joined.push(name);
    joined
}

/// The proxy's password, from the file [`PROXY_SECRET_FILE`] names: `None` when the guard's
/// environment has neither that variable nor [`PROXY`]. An error when one comes without the
/// other, or the variable names no local file at an absolute path ([`is_local_file`]), or names
/// one that is not a plain file with one name (a link, a folder; never followed), cannot be
/// opened to read and write, is over [`SECRET_FILE_MAX_BYTES`] or does not hold exactly 64 of
/// `0-9 a-f` and at most one `\n`. The file is opened once, without following a link at its
/// name, and read, emptied and deleted through that handle (`write/secret.rs`), whatever it
/// held; one that cannot be deleted is left empty (and noted), and one that cannot be emptied
/// is an error too, so the password never outlives the guard's start on disk. Nothing that
/// failed the checks is changed. The error names the variable, never the file's content.
pub(crate) fn proxy_secret(source: &Source, os: Os) -> Result<Option<Password>, EnvError> {
    let Some(value) = source.get(os, PROXY_SECRET_FILE) else {
        return match source.get(os, PROXY) {
            Some(_) => Err(EnvError {
                name: PROXY,
                problem: Problem::ProxyWithoutSecret,
            }),
            None => Ok(None),
        };
    };
    let error = |problem| EnvError {
        name: PROXY_SECRET_FILE,
        problem,
    };
    if source.get(os, PROXY).is_none() {
        return Err(error(Problem::SecretWithoutProxy));
    }
    if !is_local_file(value, os) {
        return Err(error(Problem::NotFile));
    }
    let file = Path::new(value);
    let taken = write::secret::take(file, SECRET_FILE_MAX_BYTES).map_err(|refused| {
        error(match refused {
            Refused::NotPlain => Problem::NotPlain,
            Refused::Unopened | Refused::Unread => Problem::Unreadable,
            Refused::NotEmptied => Problem::NotSpent,
        })
    })?;
    if let Some(failed) = &taken.undeleted {
        super::note(&format!(
            "cannot delete the proxy's secret file {} ({failed}); it was emptied instead",
            file.display()
        ));
    }
    let bytes = taken.bytes;
    let text = bytes.strip_suffix(b"\n").unwrap_or(bytes.as_slice());
    std::str::from_utf8(text)
        .ok()
        .and_then(Password::parse)
        .map(Some)
        .ok_or_else(|| error(Problem::NotSecret))
}

/// Adds [`CDP`]: the browser endpoint `chrome.rs` read and checked.
pub(crate) fn set_cdp(env: &mut BTreeMap<String, OsString>, endpoint: &str) {
    env.insert(CDP.to_owned(), OsString::from(endpoint));
}

/// Chrome's environment: the child's without agent-browser's own variables, so Chrome gets the
/// same `HOME`, temp folder, `PATH`, locale and proxy variables (its proxy is set by flag).
pub(crate) fn chrome_env(env: &BTreeMap<String, OsString>) -> BTreeMap<String, OsString> {
    env.iter()
        .filter(|(name, _)| !name.starts_with("AGENT_BROWSER_"))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

fn required<'a>(
    source: &'a Source,
    os: Os,
    name: &'static str,
    valid: fn(&OsStr, Os) -> bool,
    problem: Problem,
) -> Result<&'a OsStr, EnvError> {
    let Some(value) = source.get(os, name) else {
        return Err(EnvError {
            name,
            problem: Problem::Missing,
        });
    };
    if valid(value, os) {
        Ok(value)
    } else {
        Err(EnvError { name, problem })
    }
}

/// An existing file at an absolute path by this platform's rules (`C:\...` on Windows, `/...`
/// elsewhere). On Windows, a path starting with two slashes or backslashes in any mix (a network
/// share `\\host\share`, or a device path `\\?\`, `\\.\`) or with `\??` is refused before
/// anything touches it, since reading a network path reaches the network.
fn is_local_file(value: &OsStr, os: Os) -> bool {
    if os == Os::Windows {
        let Some(text) = value.to_str() else {
            return false;
        };
        let mut chars = text.chars();
        let slash = |c: Option<char>| matches!(c, Some('\\' | '/'));
        if (slash(chars.next()) && slash(chars.next())) || text.starts_with("\\??") {
            return false;
        }
    }
    let path = Path::new(value);
    path.is_absolute() && path.is_file()
}

/// Exactly `http://127.0.0.1:<port>`: the port 1 to 65535 in decimal with no leading zero, and
/// nothing after it (no path, no slash, no user).
pub(crate) fn is_loopback_proxy(value: &OsStr) -> bool {
    let Some(port) = value
        .to_str()
        .and_then(|text| text.strip_prefix("http://127.0.0.1:"))
    else {
        return false;
    };
    !port.is_empty()
        && port.len() <= 5
        && port.bytes().all(|b| b.is_ascii_digit())
        && !port.starts_with('0')
        && port.parse::<u32>().is_ok_and(|n| (1..=65_535).contains(&n))
}

/// Absolute by Unix's rule, a leading `/`, whatever OS reads it: `Path::has_root`, which on
/// Unix is `Path::is_absolute`, so the guard's own check is unchanged there, and the tests that
/// model Unix read `/tmp/...` the same way on a Windows machine (where `is_absolute` wants a
/// drive).
fn is_unix_absolute(value: &OsStr) -> bool {
    Path::new(value).has_root()
}

/// A port from [`MIN_PORT`] to 65535, in decimal with no sign and no leading zero.
pub(crate) fn port_of(value: &OsStr) -> Option<u16> {
    let text = value.to_str()?;
    let plain = !text.is_empty()
        && text.len() <= 5
        && text.bytes().all(|b| b.is_ascii_digit())
        && !text.starts_with('0');
    if !plain {
        return None;
    }
    text.parse::<u16>().ok().filter(|port| *port >= MIN_PORT)
}

/// 1 to 64 of `A-Z a-z 0-9 _ -`.
pub(crate) fn is_session(value: &OsStr) -> bool {
    value.to_str().is_some_and(|text| {
        (1..=64).contains(&text.len())
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    })
}

#[cfg(test)]
mod tests;
