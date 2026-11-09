//! The browser's own Seatbelt profile (decision 0038, "The browser profile"). macOS refuses a
//! nested `sandbox_init` under a profile with any deny, so Chrome's own sandbox cannot start in
//! the agent's jail. The browser stack (the guard, agent-browser and Chrome) runs outside the
//! jail, started by the host, under this profile, which stands in for Chrome's sandbox; Chrome
//! runs there with `--no-sandbox`, and nowhere else.
//!
//! The profile gives read and run of the Chrome bundle and the stack's binaries, read of the
//! system paths Chrome needs, read and write of one run folder
//! (`<data>/browser-runs/<run>/`), the guard's socket and Unix sockets in `ab/`, and TCP to three
//! loopback ports. The browser is chrome-headless-shell, the only one supported on macOS: a
//! Chrome app bundle needs `LaunchServices`, the window server and more. Every allow was measured
//! (0038 lists the failure seen without each). It compiles on every target, so its tests run on
//! every CI target.

use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
mod check;
#[cfg(target_os = "macos")]
mod launcher;

#[cfg(target_os = "macos")]
pub use check::{path_allowed, sandboxed};
#[cfg(target_os = "macos")]
pub use launcher::{BrowserLauncher, refuse_nesting};

/// The allow rules that name no path of the run: system reads, sysctls, the Mach services.
const BASE: &str = include_str!("browser.sb");

/// The message every deny carries, so a denial in the unified log names the browser profile.
pub const TAG: &str = "clauseo-browser";

/// The name of the folder that holds every run folder.
pub const RUNS_FOLDER: &str = "browser-runs";

/// The guard's socket in the run's `sock/`: the one name the agent's jail may connect to
/// ([`BrowserBridge`](crate::BrowserBridge)) and the one name this profile may bind there.
pub const GUARD_SOCKET: &str = "guard.sock";

/// The run folder's sub-folders: Chrome's user data, the staging folder the agent reads, the
/// guard's socket, agent-browser's own folder and the stack's temp folder.
pub const RUN_SUBFOLDERS: [&str; 5] = ["profile", "staging", "sock", "ab", "tmp"];

/// The Mach services the keychain is read through, each denied by name.
const KEYCHAIN_SERVICES: [&str; 3] = [
    "com.apple.SecurityServer",
    "com.apple.securityd.xpc",
    "com.apple.securityd",
];

/// The GPU and `IOSurface` user clients, denied by name (Chrome runs on `SwiftShader`).
const GPU_CLIENTS: [&str; 3] = [
    "IOSurfaceRootUserClient",
    "AGXDeviceUserClient",
    "IOAccelerator",
];

/// What the browser profile is built from. Every path must be absolute; each is canonicalised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserPaths {
    /// The run folder, `<data>/browser-runs/<run>/`, made by the host. It must exist and sit
    /// directly in a folder named [`RUNS_FOLDER`]. The only writable path.
    pub run_dir: PathBuf,
    /// chrome-headless-shell's folder: read, run and mapped as code. The only Chrome the
    /// profile supports on macOS: a path ending in `.app` is refused.
    pub chrome_bundle: PathBuf,
    /// The guard's and agent-browser's binaries (or their folders): read, run and mapped.
    pub exec_bins: Vec<PathBuf>,
    /// The egress proxy's loopback port: outbound only.
    pub proxy_port: u16,
    /// Chrome's `DevTools` port: bound by Chrome on loopback, reached by agent-browser.
    pub cdp_port: u16,
    /// agent-browser's stream port: bound and reached on loopback.
    pub stream_port: u16,
    /// Folders the profile must never open for writing: the matters, the home's sensitive
    /// folders, the data dir's other folders. A writable path equal to one, inside one or
    /// holding one is refused; so is a run path inside one.
    pub deny_roots: Vec<PathBuf>,
}

/// Why [`profile`] refused its paths, or the launcher refused to start.
#[derive(Debug, thiserror::Error)]
pub enum BrowserProfileError {
    /// A path was relative.
    #[error("the browser's {what} must be an absolute path: {path:?}")]
    Relative {
        /// Which input.
        what: &'static str,
        /// The path as given.
        path: PathBuf,
    },
    /// A path that must exist could not be resolved.
    #[error("the browser's {what} could not be resolved: {path:?}: {source}")]
    Unresolved {
        /// Which input.
        what: &'static str,
        /// The path as given.
        path: PathBuf,
        /// Why.
        #[source]
        source: std::io::Error,
    },
    /// A path holds a byte the profile's string syntax cannot carry safely: a quote, a
    /// backslash or a control character, or it is not UTF-8.
    #[error("the browser's {what} holds a quote, backslash or control character: {path:?}")]
    Unquotable {
        /// Which input.
        what: &'static str,
        /// The path.
        path: PathBuf,
    },
    /// The run folder is not directly inside a folder named `browser-runs`.
    #[error("the browser's run folder must sit in a `{RUNS_FOLDER}` folder: {0:?}")]
    NotARunFolder(PathBuf),
    /// A path meets a deny root: the run folder equals, holds or sits in one, or a binary sits
    /// in one.
    #[error("the browser's {what} meets a refused folder: {path:?} and {root:?}")]
    InDenyRoot {
        /// Which input.
        what: &'static str,
        /// The path, canonical.
        path: PathBuf,
        /// The deny root, canonical as far as it exists.
        root: PathBuf,
    },
    /// A binary sits in the run folder, which the stack can write.
    #[error("the browser's {what} sits in the writable run folder: {path:?}")]
    InRunFolder {
        /// Which input.
        what: &'static str,
        /// The path, canonical.
        path: PathBuf,
    },
    /// The Chrome bundle is an app bundle: only chrome-headless-shell is supported on macOS.
    #[error("only chrome-headless-shell is supported on macOS, not an app bundle: {0:?}")]
    AppBundle(PathBuf),
    /// A port was 0.
    #[error("the browser's {0} must not be 0")]
    ZeroPort(&'static str),
    /// This process already runs under a Seatbelt profile (`sandbox_check` with no operation
    /// was not 0), so the browser profile would nest in it; the launcher refuses.
    #[error("the browser profile refuses to start inside a sandbox")]
    AlreadySandboxed,
    /// The launch itself failed.
    #[error(transparent)]
    Launch(#[from] clauseo_process::Error),
}

/// The canonical paths a profile is written from.
#[derive(Debug, Clone)]
struct Checked {
    run_dir: PathBuf,
    chrome_bundle: PathBuf,
    exec_bins: Vec<PathBuf>,
}

/// The browser profile for `paths`: the text `sandbox-exec -p` takes. Paths are written into
/// the text as strings, so each one is refused if it holds a quote, a backslash or a control
/// character. Refuses relative paths, paths that do not exist (the deny roots need not), a run
/// folder outside a `browser-runs` folder, a run folder that equals, holds or sits in a deny
/// root, a binary in a deny root or in the run folder, an app bundle, and a port of 0.
pub fn profile(paths: &BrowserPaths) -> Result<String, BrowserProfileError> {
    let checked = check(paths)?;
    Ok(write(&checked, paths))
}

/// Checks and canonicalises `paths`.
fn check(paths: &BrowserPaths) -> Result<Checked, BrowserProfileError> {
    for (what, port) in [
        ("proxy port", paths.proxy_port),
        ("DevTools port", paths.cdp_port),
        ("stream port", paths.stream_port),
    ] {
        if port == 0 {
            return Err(BrowserProfileError::ZeroPort(what));
        }
    }
    let run_dir = existing(&paths.run_dir, "run folder")?;
    let in_runs = run_dir
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == RUNS_FOLDER);
    if !in_runs {
        return Err(BrowserProfileError::NotARunFolder(run_dir));
    }
    let chrome_bundle = existing(&paths.chrome_bundle, "Chrome bundle")?;
    if in_app_bundle(&paths.chrome_bundle) || in_app_bundle(&chrome_bundle) {
        return Err(BrowserProfileError::AppBundle(paths.chrome_bundle.clone()));
    }
    let exec_bins = paths
        .exec_bins
        .iter()
        .map(|bin| existing(bin, "binary"))
        .collect::<Result<Vec<_>, _>>()?;
    let roots = paths
        .deny_roots
        .iter()
        .map(|root| beneath_existing(root))
        .collect::<Result<Vec<_>, _>>()?;
    for root in &roots {
        if under(&run_dir, root) || under(root, &run_dir) {
            return Err(BrowserProfileError::InDenyRoot {
                what: "run folder",
                path: run_dir,
                root: root.clone(),
            });
        }
    }
    for (what, bin) in std::iter::once(("Chrome bundle", &chrome_bundle))
        .chain(exec_bins.iter().map(|bin| ("binary", bin)))
    {
        if let Some(root) = roots.iter().find(|root| under(bin, root)) {
            return Err(BrowserProfileError::InDenyRoot {
                what,
                path: bin.clone(),
                root: root.clone(),
            });
        }
        if under(bin, &run_dir) {
            return Err(BrowserProfileError::InRunFolder {
                what,
                path: bin.clone(),
            });
        }
    }
    Ok(Checked {
        run_dir,
        chrome_bundle,
        exec_bins,
    })
}

/// The profile text. Seatbelt applies the last rule that matches: allows first, then the
/// denies, and no allow after a deny.
fn write(c: &Checked, paths: &BrowserPaths) -> String {
    let deny = format!("(with message \"{TAG}\")");
    let mut t = format!("(version 1)\n(deny default {deny})\n");
    t.push_str("; Code is mapped only where an allow below says so.\n(deny file-map-executable ");
    t.push_str(&deny);
    t.push_str(")\n\n");
    t.push_str(BASE);
    let run = quoted(&c.run_dir);
    let bundle = quoted(&c.chrome_bundle);
    let bins: Vec<String> = c.exec_bins.iter().map(|bin| quoted(bin)).collect();

    t.push_str("\n; Chrome's bundle and the stack's binaries: read, run, mapped as code.\n");
    let mut code = vec![format!("(subpath {bundle})")];
    code.extend(bins.iter().map(|bin| format!("(subpath {bin})")));
    t.push_str(&rule(
        "allow",
        "file-read* process-exec file-map-executable",
        &code,
    ));
    t.push_str("; The folders above them and the run folder: metadata only, so paths resolve.\n");
    let mut above = vec![format!("(path-ancestors {run})")];
    above.extend(
        code.iter()
            .map(|c| c.replace("(subpath ", "(path-ancestors ")),
    );
    t.push_str(&rule("allow", "file-read-metadata", &above));
    t.push_str("; The run folder: read and write. Nothing else is writable.\n");
    t.push_str(&rule(
        "allow",
        "file-read* file-write*",
        &[format!("(subpath {run})")],
    ));

    let guard = quoted(&c.run_dir.join("sock").join(GUARD_SOCKET));
    let ab = format!("(subpath {})", quoted(&c.run_dir.join("ab")));
    t.push_str(
        "\n; Unix sockets: the guard's `sock/guard.sock`, and in ab/ agent-browser's daemon\n",
    );
    t.push_str(
        "; socket and Chrome's singleton socket. No other name in sock/, nothing elsewhere.\n",
    );
    let sockets = [format!("(path-literal {guard})"), ab];
    let bound: Vec<String> = sockets
        .iter()
        .map(|s| format!("(local unix-socket {s})"))
        .collect();
    t.push_str(&rule("allow", "network-bind", &bound));
    let mut out: Vec<String> = sockets
        .iter()
        .map(|s| format!("(remote unix-socket {s})"))
        .collect();
    t.push_str("; TCP over IPv4 loopback to the proxy, DevTools and stream ports only.\n");
    // Seatbelt takes only `*` or `localhost` as a host; tcp4 keeps it to 127.0.0.1.
    for port in [paths.proxy_port, paths.cdp_port, paths.stream_port] {
        out.push(format!("(remote tcp4 \"localhost:{port}\")"));
    }
    t.push_str(&rule("allow", "network-outbound", &out));
    t.push_str("; Chrome's DevTools listener and agent-browser's stream listener, exact ports.\n");
    let listen: Vec<String> = [paths.cdp_port, paths.stream_port]
        .iter()
        .map(|port| format!("(local tcp4 \"localhost:{port}\")"))
        .collect();
    t.push_str(&rule("allow", "network-bind network-inbound", &listen));

    t.push_str("\n; The last rules: denies only, so no allow above can reopen them.\n");
    t.push_str(
        "; Nothing in the run folder is run or mapped as code: the agent writes staging/.\n",
    );
    t.push_str(&rule(
        "deny",
        "process-exec file-map-executable",
        &[format!("(subpath {run})")],
    ));
    t.push_str("; No keychain: not its folders, not the services that read it.\n");
    t.push_str(&rule(
        "deny",
        "file-read* file-write*",
        &[
            "(subpath \"/Library/Keychains\")".to_owned(),
            "(regex #\"^/Users/[^/]+/Library/Keychains(/|$)\")".to_owned(),
        ],
    ));
    let services: Vec<String> = KEYCHAIN_SERVICES
        .iter()
        .map(|name| format!("(global-name \"{name}\")"))
        .collect();
    t.push_str(&rule("deny", "mach-lookup", &services));
    t.push_str("; No GPU, no IOSurface: Chrome draws with SwiftShader.\n");
    let gpu: Vec<String> = GPU_CLIENTS
        .iter()
        .map(|class| format!("(iokit-user-client-class \"{class}\")"))
        .collect();
    t.push_str(&rule("deny", "iokit-open", &gpu));
    t.push_str("; No window server and no display services: chrome-headless-shell needs none.\n");
    t.push_str(&rule(
        "deny",
        "mach-lookup",
        &[
            "(global-name-prefix \"com.apple.windowserver\")".to_owned(),
            "(global-name \"com.apple.CARenderServer\")".to_owned(),
        ],
    ));
    t.push_str("; No firmware variables, no device properties.\n");
    for op in ["nvram*", "iokit-get-properties"] {
        t.push_str(&rule("deny", op, &[]));
    }
    t
}

/// One rule, a filter per line; denies carry the tag.
fn rule(action: &str, ops: &str, filters: &[String]) -> String {
    let mut r = format!("({action} {ops}");
    for filter in filters {
        r.push_str("\n  ");
        r.push_str(filter);
    }
    if action == "deny" {
        r.push_str("\n  (with message \"");
        r.push_str(TAG);
        r.push_str("\")");
    }
    r.push_str(")\n");
    r
}

/// `path` as a Seatbelt string. [`check`] refused every path this cannot carry.
fn quoted(path: &Path) -> String {
    format!("\"{}\"", path.to_string_lossy())
}

/// Refuses a path the profile's string syntax cannot carry.
fn quotable(path: &Path, what: &'static str) -> Result<(), BrowserProfileError> {
    let ok = path
        .to_str()
        .is_some_and(|s| !s.chars().any(|c| c == '"' || c == '\\' || c.is_control()));
    if ok {
        Ok(())
    } else {
        Err(BrowserProfileError::Unquotable {
            what,
            path: path.to_owned(),
        })
    }
}

/// `p`, absolute and existing, canonical and quotable.
fn existing(p: &Path, what: &'static str) -> Result<PathBuf, BrowserProfileError> {
    absolute(p, what)?;
    let canonical = dunce::canonicalize(p).map_err(|source| BrowserProfileError::Unresolved {
        what,
        path: p.to_owned(),
        source,
    })?;
    quotable(&canonical, what)?;
    Ok(canonical)
}

/// A deny root canonical as far as it exists: the longest existing ancestor canonicalised, the
/// rest appended (it is compared, never written into the profile).
fn beneath_existing(p: &Path) -> Result<PathBuf, BrowserProfileError> {
    let what = "deny root";
    absolute(p, what)?;
    let mut rest = Vec::new();
    let mut at = p;
    loop {
        if let Ok(found) = dunce::canonicalize(at) {
            return Ok(rest.iter().rev().fold(found, |path, name| path.join(name)));
        }
        let (Some(parent), Some(name)) = (at.parent(), at.file_name()) else {
            return Err(BrowserProfileError::Unresolved {
                what,
                path: p.to_owned(),
                source: std::io::Error::from(std::io::ErrorKind::NotFound),
            });
        };
        rest.push(name);
        at = parent;
    }
}

fn absolute(p: &Path, what: &'static str) -> Result<(), BrowserProfileError> {
    if p.is_absolute() {
        Ok(())
    } else {
        Err(BrowserProfileError::Relative {
            what,
            path: p.to_owned(),
        })
    }
}

/// Whether a name of `path` ends in `.app`, in any letter case: an app bundle or inside one.
fn in_app_bundle(path: &Path) -> bool {
    path.components().any(|c| {
        let name = c.as_os_str().to_string_lossy();
        name.len() > 4
            && name
                .get(name.len() - 4..)
                .is_some_and(|ext| ext.eq_ignore_ascii_case(".app"))
    })
}

/// Whether `path` is `prefix` or inside it, as the policy compares paths.
fn under(path: &Path, prefix: &Path) -> bool {
    crate::policy::under(path, prefix)
}

#[cfg(test)]
mod tests;
