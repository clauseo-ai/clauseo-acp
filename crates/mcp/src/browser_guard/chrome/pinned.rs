//! Which Chrome the guard starts: only one it can trust (decision record 0029, "The pinned
//! browser"). `AGENT_BROWSER_EXECUTABLE_PATH` comes from the guard's environment, which an agent
//! that runs `clauseo-mcp browser-guard` itself chooses, so it names a candidate, never the
//! answer: a wrapper script there could add `--no-sandbox` past every check on the guard's own
//! arguments.
//!
//! The candidate is resolved to its real path (every link, `..` and junction followed), and the
//! guard starts that real path, never the name it was given. The real path must be one of:
//! - a system install of Chrome (or Edge on Windows) at its fixed place ([`Trusted::files`]):
//!   `/Applications/Google Chrome.app` on macOS, `C:\Program Files` and `C:\Program Files (x86)`
//!   on Windows, the distribution's folders on Linux;
//! - Chrome for Testing as the host installs it, `<data>/browsers/chrome-<version>/<binary>`
//!   ([`Cft`]). On macOS `<data>` is the default data folder of the guard's user, found from
//!   the user database and never from the environment. On Windows the guard may run as the
//!   slot's sandbox user, whose folders are not the lawyer's, so any data folder passes, but
//!   only where that user can change neither the binary nor any folder above it ([`modifiable`]):
//!   a file the agent wrote is its own, and an owner may always rewrite its permissions.
//!
//! Windows' system installs get the same test. macOS and Linux get none: there the agent is the
//! same user as the lawyer, and the jail, not the permissions, keeps it out of those folders.
//! A link from a trusted place to anywhere else resolves to that elsewhere and is refused.

use std::path::{Component, Path, PathBuf};

/// What the agent's tool call gets when the Chrome named is not one the guard trusts.
pub(crate) const REFUSAL: &str =
    "The browser Clauseo was asked to use isn't one it installed or trusts, so it won't start it.";

/// The folder under the data folder that holds Chrome for Testing (the host's
/// `browser::chrome::BROWSERS_DIR`).
const BROWSERS_DIR: &str = "browsers";
/// The prefix of one download's folder (the host's `browser::chrome::CFT_PREFIX`).
const CFT_PREFIX: &str = "chrome-";
/// The data folder's name under the OS's per-user folder (`clauseo_settings::IDENTIFIER`).
#[cfg(target_os = "macos")]
const IDENTIFIER: &str = "ai.clauseo.desktop";

/// Tests only (the `test-support` feature, never in a shipped build): a folder whose every file
/// the guard trusts, read from the guard's own environment, so the tests can start stubs.
#[cfg(feature = "test-support")]
const TRUSTED_FOR_TESTS: &str = "CLAUSEO_TEST_GUARD_TRUSTED_DIR";

/// Where a trusted Chrome may be.
#[derive(Debug, Default)]
pub(crate) struct Trusted {
    /// System installs, each the exact file.
    pub(crate) files: Vec<PathBuf>,
    /// Chrome for Testing as the host lays it out.
    pub(crate) cft: Option<Cft>,
    /// Any data folder's Chrome for Testing, if the guard's user cannot change it (Windows).
    pub(crate) any_unmodifiable_cft: bool,
    /// Tests only: folders whose every file is trusted.
    pub(crate) folders: Vec<PathBuf>,
}

/// Chrome for Testing's place: `<browsers>/chrome-<version>/<binary>`.
#[derive(Debug)]
pub(crate) struct Cft {
    /// `<data>/browsers`; `None` on Windows, where any data folder's may pass.
    pub(crate) browsers: Option<PathBuf>,
    /// The binary inside one download's folder, as the zip lays it out.
    pub(crate) binary: PathBuf,
}

impl Trusted {
    /// This machine's.
    pub(crate) fn here() -> Trusted {
        #[cfg_attr(
            not(feature = "test-support"),
            expect(unused_mut, reason = "tests add one")
        )]
        let mut trusted = Trusted::system();
        #[cfg(feature = "test-support")]
        if let Some(folder) = std::env::var_os(TRUSTED_FOR_TESTS) {
            trusted.folders.push(PathBuf::from(folder));
        }
        trusted
    }

    #[cfg(target_os = "macos")]
    fn system() -> Trusted {
        let arch = if cfg!(target_arch = "aarch64") {
            "chrome-mac-arm64"
        } else {
            "chrome-mac-x64"
        };
        let home = nix::unistd::User::from_uid(nix::unistd::getuid())
            .ok()
            .flatten()
            .map(|user| user.dir);
        Trusted {
            files: vec![PathBuf::from(
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            )],
            cft: Some(Cft {
                browsers: home.map(|home| {
                    home.join("Library/Application Support")
                        .join(IDENTIFIER)
                        .join(BROWSERS_DIR)
                }),
                binary: Path::new(arch)
                    .join("Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"),
            }),
            any_unmodifiable_cft: false,
            folders: Vec::new(),
        }
    }

    #[cfg(windows)]
    fn system() -> Trusted {
        let chrome = r"Google\Chrome\Application\chrome.exe";
        let edge = r"Microsoft\Edge\Application\msedge.exe";
        let files = [r"C:\Program Files", r"C:\Program Files (x86)"]
            .iter()
            .flat_map(|base| [Path::new(base).join(chrome), Path::new(base).join(edge)])
            .collect();
        Trusted {
            files,
            cft: Some(Cft {
                browsers: None,
                binary: Path::new("chrome-win64").join("chrome.exe"),
            }),
            any_unmodifiable_cft: true,
            folders: Vec::new(),
        }
    }

    /// Linux, for development and CI: the distributions' installs, each root's own.
    #[cfg(not(any(target_os = "macos", windows)))]
    fn system() -> Trusted {
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
}

/// The real path of `asked` if it is a Chrome the guard trusts, else `None`. `modifiable` says
/// whether the guard's user can change a file or folder ([`modifiable`] in production).
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
    let unmodifiable = || {
        real.ancestors()
            .take_while(|path| path.parent().is_some())
            .all(|path| !modifiable(path))
    };
    let system = trusted
        .files
        .iter()
        .any(|file| relative(&shown, file).is_some_and(|rest| rest.as_os_str().is_empty()));
    if system && unmodifiable() {
        return Some(real);
    }
    let cft = trusted.cft.as_ref()?;
    let rest = match &cft.browsers {
        Some(browsers) => under(browsers)?,
        None if trusted.any_unmodifiable_cft => after_browsers(&shown)?,
        None => return None,
    };
    (is_download(&rest, &cft.binary) && unmodifiable()).then_some(real)
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

/// Whether `rest`, a path under `browsers`, is `chrome-<version>/<binary>`: a version of digits
/// and dots, as the host names a download's folder.
fn is_download(rest: &Path, binary: &Path) -> bool {
    let mut parts = rest.components();
    let Some(Component::Normal(folder)) = parts.next() else {
        return false;
    };
    let version = folder
        .to_str()
        .and_then(|name| name.strip_prefix(CFT_PREFIX))
        .unwrap_or_default();
    !version.is_empty()
        && version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && same(parts.as_path(), binary)
}

/// What follows the last `browsers` folder in `real` (Windows: any data folder's).
fn after_browsers(real: &Path) -> Option<PathBuf> {
    let parts: Vec<Component<'_>> = real.components().collect();
    let at = parts
        .iter()
        .rposition(|part| matches!(part, Component::Normal(name) if same(Path::new(name), Path::new(BROWSERS_DIR))))?;
    Some(parts[at + 1..].iter().collect())
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

/// Whether the guard's user can change `path`: Windows opens it once for each right that would
/// let it rewrite, replace or re-permission the file or a folder's entries; any open that
/// succeeds says yes. An owner always gets `WRITE_DAC`, so a file the user wrote always says yes.
#[cfg(windows)]
#[expect(
    clippy::disallowed_methods,
    reason = "a probe that opens and closes with no read or write, changing nothing"
)]
pub(crate) fn modifiable(path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_WRITE_DATA: u32 = 0x2;
    const FILE_APPEND_DATA: u32 = 0x4;
    const FILE_DELETE_CHILD: u32 = 0x40;
    const DELETE: u32 = 0x1_0000;
    const WRITE_DAC: u32 = 0x4_0000;
    const WRITE_OWNER: u32 = 0x8_0000;
    const SHARE_ALL: u32 = 0x7;
    // Opens a folder too; follows no reparse point (the path is already resolved).
    const FLAGS: u32 = 0x0200_0000 | 0x0020_0000;
    [
        FILE_WRITE_DATA,
        FILE_APPEND_DATA,
        FILE_DELETE_CHILD,
        DELETE,
        WRITE_DAC,
        WRITE_OWNER,
    ]
    .iter()
    .any(|&right| {
        std::fs::OpenOptions::new()
            .access_mode(right)
            .share_mode(SHARE_ALL)
            .custom_flags(FLAGS)
            .open(path)
            .is_ok()
    })
}

/// macOS and Linux: the jail, not the permissions, keeps the agent out (see the module's text).
#[cfg(not(windows))]
pub(crate) fn modifiable(_path: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests;
