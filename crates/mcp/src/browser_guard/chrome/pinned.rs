//! Which Chrome the guard starts: only one it can trust (decision record 0029, "The pinned
//! browser"). `AGENT_BROWSER_EXECUTABLE_PATH` comes from the guard's environment, which an agent
//! that runs `clauseo-mcp browser-guard` itself chooses, so it names a candidate, never the
//! answer: a wrapper script there could add `--no-sandbox` past every check on the guard's own
//! arguments.
//!
//! The candidate is resolved to its real path (every link, `..` and junction followed), and the
//! guard starts that real path, never the name it was given. The real path must be Clauseo's own
//! pinned download as the host lays it out, `<data>/browsers/<folder>/<binary>` ([`Layout`]):
//! Chrome for Testing in `chrome-<version>/chrome-<platform>/…`, or chrome-headless-shell in
//! `chrome-headless-shell-<version>/chrome-headless-shell-<platform>/chrome-headless-shell`.
//! An installed Google Chrome or Edge is refused by design, wherever it is: the guarded browser
//! runs pinned builds only.
//!
//! On macOS `<data>` is the default data folder of the guard's user, found from the user database
//! and never from the environment; the jail, not the permissions, keeps the agent (the same user
//! as the lawyer) out of it. On Windows the guard may run as another account than the data
//! folder's owner (the browser account under W2), so `<data>` is the host-set `--data-dir`;
//! without it no Chrome passes. There the agent must also be unable to change the binary or any
//! folder above it, and own none of them ([`modifiable_by`], `pinned/authz.rs`): an owner can
//! re-permission what it owns even after an `OWNER RIGHTS` entry took its rights away.
//!
//! Linux is not a shipped platform, only development and CI. Builds with the `test-support`
//! feature (the tests' own) trust the distributions' installed Chrome and Chromium at their
//! fixed places ([`Trusted::files`]), unpinned, and only as an ELF binary: Arch's
//! `/usr/bin/chromium` is a script that reads a flags file the user can write. A release build
//! trusts none. A link from a trusted place to anywhere else resolves to that elsewhere and is
//! refused.

use std::path::{Component, Path, PathBuf};

/// What the agent's tool call gets when the Chrome named is not one the guard trusts.
pub(crate) const REFUSAL: &str =
    "The browser Clauseo was asked to use isn't one it installed or trusts, so it won't start it.";

/// The folder under the data folder that holds the downloads (the host's
/// `browser::chrome::BROWSERS_DIR`).
#[cfg(any(target_os = "macos", windows, test))]
const BROWSERS_DIR: &str = "browsers";
/// The prefix of a Chrome for Testing download's folder (the host's `browser::chrome::CFT_PREFIX`).
#[cfg(any(target_os = "macos", windows, test))]
const CFT_PREFIX: &str = "chrome-";
/// The prefix of a chrome-headless-shell download's folder.
#[cfg(any(target_os = "macos", windows, test))]
const SHELL_PREFIX: &str = "chrome-headless-shell-";
/// The data folder's name under the OS's per-user folder (`clauseo_settings::IDENTIFIER`).
#[cfg(any(target_os = "macos", test))]
const IDENTIFIER: &str = "ai.clauseo.desktop";

/// Tests only (the `test-support` feature, never in a shipped build): a folder whose every file
/// the guard trusts, read from the guard's own environment, so the tests can start stubs.
#[cfg(feature = "test-support")]
const TRUSTED_FOR_TESTS: &str = "CLAUSEO_TEST_GUARD_TRUSTED_DIR";

/// Where a trusted Chrome may be.
#[derive(Debug, Default)]
pub(crate) struct Trusted {
    /// Installed browsers, each the exact file and an ELF binary (Linux test builds only).
    pub(crate) files: Vec<PathBuf>,
    /// `<data>/browsers`: the user's own data folder's (macOS), `--data-dir`'s (Windows).
    pub(crate) browsers: Option<PathBuf>,
    /// How a download is laid out inside `browsers`.
    pub(crate) layouts: Vec<Layout>,
    /// Tests only: folders whose every file is trusted.
    pub(crate) folders: Vec<PathBuf>,
}

/// Who the agent is, for the Windows check that it can change no part of the path: the host's
/// `--agent-sid`, and `--agent-package-sid` when the agent runs in an `AppContainer`. Both absent:
/// the guard's own token is the agent's (today's in-jail run, where they share an account).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Agent {
    /// The agent's user SID, `S-1-…`.
    pub(crate) user: Option<String>,
    /// The agent's `AppContainer` package SID, given only with `user`.
    pub(crate) package: Option<String>,
}

/// Whether `text` reads as a SID string: `S-1-`, then parts of decimal digits, at most 184
/// characters (`SECURITY_MAX_SID_STRING_CHARACTERS`). Windows parses it again, failing closed.
pub(crate) fn is_sid_text(text: &str) -> bool {
    text.len() <= 184
        && text.strip_prefix("S-1-").is_some_and(|rest| {
            rest.split('-')
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        })
}

/// One pinned download's layout: `<prefix><version>/<binary>`, the version digits and dots.
#[derive(Debug)]
pub(crate) struct Layout {
    /// The download folder's name before its version.
    pub(crate) prefix: &'static str,
    /// The binary inside the download's folder, as the zip lays it out.
    pub(crate) binary: PathBuf,
}

impl Trusted {
    /// This machine's; `data_dir` is the host's `--data-dir` (Windows only).
    pub(crate) fn here(data_dir: Option<&Path>) -> Trusted {
        Trusted {
            folders: folders_for_tests(),
            ..Trusted::system(data_dir)
        }
    }

    #[cfg(target_os = "macos")]
    fn system(_data_dir: Option<&Path>) -> Trusted {
        let platform = if cfg!(target_arch = "aarch64") {
            "mac-arm64"
        } else {
            "mac-x64"
        };
        // The user database's home, never `HOME`.
        let home = nix::unistd::User::from_uid(nix::unistd::getuid())
            .ok()
            .flatten()
            .map(|user| user.dir);
        Trusted::mac(home, platform)
    }

    /// macOS's place with `home` the user's home: the downloads in
    /// `<home>/Library/Application Support/<IDENTIFIER>/browsers`, `platform` `mac-arm64` or
    /// `mac-x64`.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn mac(home: Option<PathBuf>, platform: &str) -> Trusted {
        let cft = Path::new(&format!("{CFT_PREFIX}{platform}"))
            .join("Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
        let shell = Path::new(&format!("{SHELL_PREFIX}{platform}")).join("chrome-headless-shell");
        Trusted {
            files: Vec::new(),
            browsers: home.map(|home| {
                home.join("Library/Application Support")
                    .join(IDENTIFIER)
                    .join(BROWSERS_DIR)
            }),
            layouts: vec![
                Layout {
                    prefix: CFT_PREFIX,
                    binary: cft,
                },
                Layout {
                    prefix: SHELL_PREFIX,
                    binary: shell,
                },
            ],
            folders: Vec::new(),
        }
    }

    /// Windows: the downloads in `<data_dir>/browsers`, none without a `--data-dir`.
    #[cfg(windows)]
    fn system(data_dir: Option<&Path>) -> Trusted {
        Trusted {
            files: Vec::new(),
            browsers: data_dir.map(|data| data.join(BROWSERS_DIR)),
            layouts: vec![
                Layout {
                    prefix: CFT_PREFIX,
                    binary: Path::new("chrome-win64").join("chrome.exe"),
                },
                Layout {
                    prefix: SHELL_PREFIX,
                    binary: Path::new("chrome-headless-shell-win64")
                        .join("chrome-headless-shell.exe"),
                },
            ],
            folders: Vec::new(),
        }
    }

    /// Linux, never shipped, in a test build: the distributions' installs, each root's own,
    /// unpinned.
    #[cfg(all(target_os = "linux", feature = "test-support"))]
    fn system(_data_dir: Option<&Path>) -> Trusted {
        let files = [
            "/opt/google/chrome/google-chrome",
            "/opt/google/chrome/chrome",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/usr/lib/chromium/chromium",
            "/usr/lib/chromium-browser/chromium-browser",
        ];
        Trusted {
            files: files.iter().map(PathBuf::from).collect(),
            ..Trusted::default()
        }
    }

    /// A Linux release build, or any other system: none but the tests' own.
    #[cfg(not(any(
        target_os = "macos",
        windows,
        all(target_os = "linux", feature = "test-support")
    )))]
    fn system(_data_dir: Option<&Path>) -> Trusted {
        Trusted::default()
    }
}

/// Tests only: the folder [`TRUSTED_FOR_TESTS`] names, if any.
#[cfg(feature = "test-support")]
fn folders_for_tests() -> Vec<PathBuf> {
    std::env::var_os(TRUSTED_FOR_TESTS)
        .map(PathBuf::from)
        .into_iter()
        .collect()
}

/// A shipped build: none.
#[cfg(not(feature = "test-support"))]
fn folders_for_tests() -> Vec<PathBuf> {
    Vec::new()
}

/// The real path of `asked` if it is a Chrome the guard trusts, else `None`. `modifiable` says
/// whether the agent can change, or owns, a file or folder ([`modifiable_by`] in production);
/// it is asked of the binary and of every folder above it, the root included.
pub(crate) fn check(
    asked: &Path,
    trusted: &Trusted,
    modifiable: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if !asked.is_absolute() {
        return None;
    }
    let real = real_path(asked)?;
    if !std::fs::metadata(&real).is_ok_and(|meta| meta.is_file()) {
        return None;
    }
    // Compared without Windows' `\\?\`; started, and probed, as resolved.
    let shown = plain(&real);
    let under = |base: &Path| real_path(base).and_then(|base| relative(&shown, &plain(&base)));
    if trusted.folders.iter().any(|folder| under(folder).is_some()) {
        return Some(real);
    }
    let installed = trusted
        .files
        .iter()
        .any(|file| relative(&shown, file).is_some_and(|rest| rest.as_os_str().is_empty()))
        && is_elf(&real);
    if installed {
        return Some(real);
    }
    let rest = under(trusted.browsers.as_ref()?)?;
    let download = trusted
        .layouts
        .iter()
        .any(|layout| is_download(&rest, layout));
    let unmodifiable = || real.ancestors().all(|path| !modifiable(path));
    (download && unmodifiable()).then_some(real)
}

/// `path` with every link, junction, `.` and `..` resolved. Windows' `\\?\` form stays: the guard
/// starts exactly the file it checked, and [`plain`] gives the form it compares.
#[expect(
    clippy::disallowed_methods,
    reason = "the resolved path is compared through `plain` and handed only to CreateProcessW, \
              which takes the `\\\\?\\` form"
)]
fn real_path(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

/// Whether `rest`, a path under `browsers`, is `<prefix><version>/<binary>`: a version of digits
/// and dots, as the host names a download's folder.
fn is_download(rest: &Path, layout: &Layout) -> bool {
    let mut parts = rest.components();
    let Some(Component::Normal(folder)) = parts.next() else {
        return false;
    };
    let version = folder
        .to_str()
        .and_then(|name| name.strip_prefix(layout.prefix))
        .unwrap_or_default();
    !version.is_empty()
        && version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && same(parts.as_path(), &layout.binary)
}

/// `path` relative to `base`, comparing as the OS does (Windows: without ASCII case).
fn relative(path: &Path, base: &Path) -> Option<PathBuf> {
    let mut parts = path.components();
    for want in base.components() {
        let have = parts.next()?;
        if !same(Path::new(have.as_os_str()), Path::new(want.as_os_str())) {
            return None;
        }
    }
    Some(parts.as_path().to_owned())
}

/// Whether `path` starts with ELF's magic bytes: a binary, not a script an interpreter runs.
fn is_elf(path: &Path) -> bool {
    use std::io::Read;
    let mut magic = [0_u8; 4];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut magic))
        .is_ok()
        && magic == *b"\x7fELF"
}

/// Whether two paths are the same text, without ASCII case on Windows.
fn same(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        let (Some(a), Some(b)) = (a.to_str(), b.to_str()) else {
            return false;
        };
        a.replace('/', "\\")
            .eq_ignore_ascii_case(&b.replace('/', "\\"))
    } else {
        a == b
    }
}

/// `path` without Windows' `\\?\` prefix when a drive letter follows; anything else as it is.
fn plain(path: &Path) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path.to_owned();
    };
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path.to_owned(),
    }
}

/// What says whether `agent` can change, or owns, a path (Windows, `pinned/authz.rs`): any
/// right that would let it rewrite, replace or re-permission the file or a folder's entries,
/// asked of every path but the root, which any user may add folders to; ownership asked of
/// every path, the root included. When the identities cannot be read, everything is modifiable.
#[cfg(windows)]
pub(crate) fn modifiable_by(agent: &Agent) -> impl Fn(&Path) -> bool {
    let writers = authz::Writers::new(agent);
    move |path: &Path| {
        writers.as_ref().is_none_or(|writers| {
            writers.owns(path) || (path.parent().is_some() && writers.can_change(path))
        })
    }
}

/// macOS and Linux: the jail, not the permissions, keeps the agent out (see the module's text).
#[cfg(not(windows))]
pub(crate) fn modifiable_by(_agent: &Agent) -> impl Fn(&Path) -> bool {
    |_: &Path| false
}

#[cfg(windows)]
mod authz;

#[cfg(test)]
mod tests;
