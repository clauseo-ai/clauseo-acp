//! The child's environment, built from scratch: the guard clears its own and gives agent-browser
//! exactly the map [`build`] returns. Every name the child may see is in this file; the rules and
//! the agent-browser source lines behind them are in `docs/decisions/0029-browser.md`.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

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
/// The session's egress proxy, `http://127.0.0.1:<port>`.
pub(crate) const PROXY: &str = "AGENT_BROWSER_PROXY";
/// The Chrome the host chose.
pub(crate) const EXECUTABLE_PATH: &str = "AGENT_BROWSER_EXECUTABLE_PATH";
/// The Clauseo session id; also the namespace.
pub(crate) const SESSION: &str = "AGENT_BROWSER_SESSION";
/// Set to the session value, whatever the guard's own environment says.
pub(crate) const NAMESPACE: &str = "AGENT_BROWSER_NAMESPACE";

/// Values fixed in the guard, whatever its environment says.
pub(crate) const FIXED: &[(&str, &str)] = &[
    ("AGENT_BROWSER_PROXY_BYPASS", "<-loopback>"),
    (
        "AGENT_BROWSER_ARGS",
        "--force-webrtc-ip-handling-policy=disable_non_proxied_udp",
    ),
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
/// Windows `SystemRoot` when the guard's environment has none.
const DEFAULT_SYSTEM_ROOT: &str = r"C:\Windows";

/// Passed through from the guard's environment, only when present.
const PASS_UNIX: &[&str] = &["LANG"];
const PASS_WINDOWS: &[&str] = &[
    "SystemRoot",
    "windir",
    "SystemDrive",
    "ComSpec",
    "PATHEXT",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "LOCALAPPDATA",
    "APPDATA",
    "USERPROFILE",
];

/// Folders under `--cwd`, each named by the variables that point at it.
const FOLDERS_UNIX: &[(&str, &[&str])] = &[
    ("downloads", &["AGENT_BROWSER_DOWNLOAD_PATH"]),
    ("run", &["AGENT_BROWSER_SOCKET_DIR"]),
    ("screenshots", &["AGENT_BROWSER_SCREENSHOT_DIR"]),
    ("home", &["HOME"]),
    ("tmp", &["TMPDIR"]),
];
const FOLDERS_WINDOWS: &[(&str, &[&str])] = &[
    ("downloads", &["AGENT_BROWSER_DOWNLOAD_PATH"]),
    ("run", &["AGENT_BROWSER_SOCKET_DIR"]),
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

fn folder_table(os: Os) -> &'static [(&'static str, &'static [&'static str])] {
    match os {
        Os::Unix => FOLDERS_UNIX,
        Os::Windows => FOLDERS_WINDOWS,
    }
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
    NotAbsolute,
    NotProxy,
    NotSession,
}

impl fmt::Display for EnvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = self.name;
        match self.problem {
            Problem::Missing => write!(f, "{name} is not set; the host sets it"),
            Problem::NotAbsolute => write!(f, "{name} must be an absolute path"),
            Problem::NotProxy => write!(
                f,
                "{name} must be exactly http://127.0.0.1:<port>, the port from 1 to 65535 with no \
                 leading zero"
            ),
            Problem::NotSession => write!(f, "{name} must be 1 to 64 of A-Z a-z 0-9 _ -"),
        }
    }
}

/// The child's whole environment. Fails on the first required variable that is missing or
/// invalid, in the order config, proxy, executable path, session; never falls back.
pub(crate) fn build(
    source: &Source,
    os: Os,
    cwd: &Path,
) -> Result<BTreeMap<String, OsString>, EnvError> {
    let config = required(source, os, CONFIG, is_absolute_path, Problem::NotAbsolute)?;
    let proxy = required(source, os, PROXY, is_loopback_proxy, Problem::NotProxy)?;
    let executable = required(
        source,
        os,
        EXECUTABLE_PATH,
        is_absolute_path,
        Problem::NotAbsolute,
    )?;
    let session = required(source, os, SESSION, is_session, Problem::NotSession)?;

    let mut env = BTreeMap::new();
    let mut set = |name: &str, value: &OsStr| {
        env.insert(name.to_owned(), value.to_owned());
    };
    set(CONFIG, config);
    set(PROXY, proxy);
    set(EXECUTABLE_PATH, executable);
    set(SESSION, session);
    set(NAMESPACE, session);
    for (name, value) in FIXED {
        set(name, OsStr::new(value));
    }
    for name in PROXY_VARS_UPPER {
        set(name, proxy);
    }
    if os == Os::Unix {
        for name in PROXY_VARS_LOWER {
            set(name, proxy);
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
            let root = source
                .get(os, "SystemRoot")
                .unwrap_or(OsStr::new(DEFAULT_SYSTEM_ROOT));
            let mut path = root.to_owned();
            path.push(r"\System32;");
            path.push(root);
            set("PATH", &path);
        }
    }
    Ok(env)
}

fn required<'a>(
    source: &'a Source,
    os: Os,
    name: &'static str,
    valid: fn(&OsStr) -> bool,
    problem: Problem,
) -> Result<&'a OsStr, EnvError> {
    let Some(value) = source.get(os, name) else {
        return Err(EnvError {
            name,
            problem: Problem::Missing,
        });
    };
    if valid(value) {
        Ok(value)
    } else {
        Err(EnvError { name, problem })
    }
}

/// An absolute path by this platform's rules (`C:\...` on Windows, `/...` elsewhere).
fn is_absolute_path(value: &OsStr) -> bool {
    Path::new(value).is_absolute()
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
