//! Clauseo's launch profile in the app (decision record 0030, "The desktop adapter"):
//! [`DesktopProfile`], the host's `ProfileHook` over a `clauseo_profile::ClauseoProfile`.
//!
//! [`DesktopProfile::build`] does every file and process call once, on the host-start thread
//! (`host_start.rs`), before the host starts: it finds the bundled programs, runs stetdoc for
//! its skill, and renders the skills. When stetdoc's first `--version` only passed its limit,
//! one more check runs on a thread of its own and puts the profile it renders in place for the
//! next agent start (`profile/stetdoc.rs`). The hook's methods then only map what it holds, as
//! the trait requires: they read no file, start nothing and keep nothing between calls.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_host::{HostConfig, McpServer, PackRequest, ProfileHook};
use clauseo_profile::skills::stetdoc::start::State;
use clauseo_profile::{
    BrowserSidecars, ClauseoProfile, PackWrite, PacksWritten, ProfileError, ProfilePaths,
    ProfileServer, SkillWrite, SkillsWritten,
};
use clauseo_protocol::{AgentId, AgentKind};
use clauseo_settings::Locations;
use serde_json::{Map, Value};

use crate::setup::SetupHook;
use stetdoc::Live;

mod stetdoc;

/// The target of the profile's support lines: host.log keeps info only from `clauseo_*::lifecycle`
/// targets (`clauseo_host::log_file`).
const LIFECYCLE: &str = "clauseo_desktop::profile::lifecycle";

/// stetdoc's program name, without the OS's suffix.
const STETDOC: &str = "stetdoc";

/// Why the profile could not be built. The app then runs with no hook (`NoProfile`).
#[derive(Debug)]
pub(crate) enum BuildError {
    /// The app's own executable could not be found.
    Exe(std::io::Error),
    /// `ProfilePaths::new` refused a program path.
    Paths(ProfileError),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exe(err) => write!(f, "the app's executable was not found: {err}"),
            Self::Paths(err) => write!(f, "{err}"),
        }
    }
}

/// The launch profile as the host's hook: the profile, built at the start and rendered again
/// once if stetdoc's background check changes it, and whether stetdoc's folder goes on the
/// agents' `PATH` (stetdoc was found and ran or only passed the limit, and its folder holds no
/// other program; a skill text it refused does not matter here).
#[derive(Debug, Clone)]
pub(crate) struct DesktopProfile {
    /// Read at each hook call, so an agent start gets the profile as it is then.
    profile: Live,
    prepend_stetdoc: bool,
    /// `<data>/tools`, holding the `agent-browser` command, when its copy is there
    /// ([`install_browser`]): the host's `ProfileHook::browser_tool`.
    browser_tool: Option<PathBuf>,
    /// The stetdoc [`stetdoc_program`] found, for the Word blackline; `None` when none was.
    stetdoc: Option<PathBuf>,
}

impl DesktopProfile {
    /// The profile under `locations`: the bundled programs beside the app's executable (on macOS
    /// agent-browser in `Contents/Helpers`, [`agent_browser_path`]),
    /// stetdoc as [`stetdoc_program`] finds it, in its own folder under `resource_dir` (Tauri's
    /// resource folder) or on macOS under `Contents/Helpers` (its skill loaded when it is
    /// there), R22's built-in skills, and no author (tracked changes carry "Clauseo", 0030).
    /// Starts stetdoc and reads files: on the host-start thread, never the host's. A skill that
    /// fails is left out with a warning; only a refused program path fails the whole profile.
    pub(crate) fn build(
        locations: &Locations,
        resource_dir: Option<&Path>,
    ) -> Result<Self, BuildError> {
        Self::build_with(locations, resource_dir, std::env::var_os("PATH").as_deref())
    }

    /// [`Self::build`] with `path` as the `PATH` value a debug build looks stetdoc up on when
    /// no bundled copy is there.
    pub(crate) fn build_with(
        locations: &Locations,
        resource_dir: Option<&Path>,
        path: Option<&OsStr>,
    ) -> Result<Self, BuildError> {
        let exe = std::env::current_exe().map_err(BuildError::Exe)?;
        let layout = Layout {
            exe,
            macos: cfg!(target_os = "macos"),
            resource_dir,
        };
        Self::build_in(locations, &layout, path)
    }

    /// [`Self::build_with`] for the app laid out as `layout` says, so a test can stand a
    /// release bundle up in a scratch folder.
    pub(crate) fn build_in(
        locations: &Locations,
        layout: &Layout<'_>,
        path: Option<&OsStr>,
    ) -> Result<Self, BuildError> {
        let exe = layout.exe.clone();
        let exe_dir = exe.parent().unwrap_or_else(|| Path::new(""));
        let bundled = bundled_stetdoc(layout.macos, exe_dir, layout.resource_dir);
        let found = stetdoc_program(bundled.as_deref(), path, !cfg!(debug_assertions));
        let paths = paths_for(locations, exe, found.clone())?;
        let cached = paths.claude_skills();
        // The program in its plain form (`ProfilePaths::new`: no `\\?\` prefix on Windows), for
        // the checks and the Word blackline.
        let program = found.as_ref().map(|_| paths.stetdoc().to_path_buf());
        let start = program
            .as_deref()
            .map(|program| stetdoc::first(program, &cached));
        // stetdoc stays on when its skill fails or its `--version` only passed the limit: only a
        // stetdoc whose `--version` fails for real is treated as absent, with no PATH entry and
        // no stetdoc line.
        let state = start
            .as_ref()
            .map_or(State::Off, |start| start.state.clone());
        let prepend = state.is_on() && found.as_deref().is_some_and(alone_in_its_folder);
        let browser_tool = install_browser(&paths, path);
        // The browser's skill comes with the copy alone. Whether a session has a browser (its
        // Chrome found, `tools.browser` on, the firm's policy allowing it) is decided at each
        // launch and told in `{browser}`, so a Chrome installed later counts from the next
        // conversation, with no restart.
        let with_browser = browser_tool.is_some();
        let base = ClauseoProfile::new(paths, None);
        let live = Live::new(stetdoc::render(base.clone(), &state, with_browser));
        if let (Some(program), Some(start)) = (&program, &start)
            && start.recheck
        {
            let recheck = stetdoc::Recheck {
                program: program.clone(),
                cached,
                state,
                base,
                browser: with_browser,
            };
            stetdoc::recheck_later(recheck, live.clone());
        }
        Ok(Self {
            stetdoc: program,
            ..Self::over(live, prepend)
        }
        .with_browser_tool(browser_tool))
    }

    /// The same hook with `folder` as the shell browser's folder (none: no shell browser).
    pub(crate) fn with_browser_tool(mut self, folder: Option<PathBuf>) -> Self {
        self.browser_tool = folder;
        self
    }

    /// The hook over `profile`, built and rendered already; `prepend_stetdoc` says whether
    /// stetdoc's folder may go on the agents' `PATH`.
    #[cfg(test)]
    pub(crate) fn new(profile: ClauseoProfile, prepend_stetdoc: bool) -> Self {
        Self::over(Live::new(profile), prepend_stetdoc)
    }

    /// The hook over `profile`, read at each call; `prepend_stetdoc` as for `Self::new`.
    fn over(profile: Live, prepend_stetdoc: bool) -> Self {
        Self {
            profile,
            prepend_stetdoc,
            browser_tool: None,
            stetdoc: None,
        }
    }

    /// The stetdoc found when the profile was built (`stetdoc_program`), for
    /// `HostConfig::with_stetdoc`: the Word blackline runs it. `None` when none was found, and
    /// for a profile made with `Self::new` (tests).
    pub(crate) fn stetdoc(&self) -> Option<&Path> {
        self.stetdoc.as_deref()
    }

    /// The folder for `HostConfig::with_agent_path_prepend`: stetdoc's, when it may go first
    /// and holds no sidecar or the app (`ProfilePaths::path_prepend`).
    pub(crate) fn path_prepend(&self) -> Option<PathBuf> {
        if !self.prepend_stetdoc {
            return None;
        }
        self.profile().path_prepend().map(Path::to_path_buf)
    }

    /// The profile as it is now: the one built at the start, or the one stetdoc's background
    /// check rendered since.
    pub(crate) fn profile(&self) -> Arc<ClauseoProfile> {
        self.profile.get()
    }
}

/// The `agent-browser` command for the agents: the bundled sidecar where [`paths_for`] resolved
/// it (`ProfilePaths::browser`'s `agent_browser`: `Contents/Helpers` on macOS, beside the exe
/// elsewhere, [`agent_browser_path`]), or in a debug build with no sidecar the first
/// `agent-browser` on `path`, copied into `<data>/tools` (`clauseo_profile::tools`, only when its
/// SHA-256 changed). Returns the folder, or none, with a log line, when there is no program to
/// copy or the copy failed: then no session gets the browser, its skill or its instructions, and
/// the app runs on.
///
/// Each outcome is one line on [`LIFECYCLE`], for support: `copied`, `up_to_date`,
/// `copied_running_copy_set_aside` (Windows), `no_sidecar`, or a warning with the error's
/// [`ProfileError::log_text`]; never a path.
fn install_browser(paths: &ProfilePaths, path: Option<&OsStr>) -> Option<PathBuf> {
    let Some(source) = browser_program(paths, path) else {
        tracing::info!(
            target: LIFECYCLE,
            outcome = "no_sidecar",
            "agent-browser left out: the agents get no browser"
        );
        return None;
    };
    match clauseo_profile::tools::install_agent_browser(&source, paths) {
        Ok((_, installed)) => {
            tracing::info!(
                target: LIFECYCLE,
                outcome = installed.word(),
                "the agents' agent-browser command is in place"
            );
            Some(paths.tools().to_path_buf())
        }
        Err(err) => {
            tracing::warn!(
                target: LIFECYCLE,
                outcome = "failed",
                err = %err.log_text(),
                "agent-browser was not copied for the agents: they get no browser"
            );
            None
        }
    }
}

/// The agent-browser program to copy: the bundled sidecar when it is there; else, in a debug
/// build, the first `agent-browser` on `path` (as stetdoc's debug fallback); else none.
fn browser_program(paths: &ProfilePaths, path: Option<&OsStr>) -> Option<PathBuf> {
    let sidecar = &paths.browser().agent_browser;
    if is_program(sidecar) {
        return Some(sidecar.clone());
    }
    if cfg!(debug_assertions) {
        first_on_path(clauseo_profile::paths::AGENT_BROWSER_COMMAND, path?)
    } else {
        None
    }
}

/// Where the app's executable is and where it looks for the stetdoc a release bundles
/// ([`bundled_stetdoc`]).
#[derive(Debug, Clone)]
pub(crate) struct Layout<'a> {
    /// The app's executable.
    pub(crate) exe: PathBuf,
    /// Whether it is laid out as a macOS bundle (`Contents/MacOS`, `Contents/Helpers`).
    pub(crate) macos: bool,
    /// Tauri's resource folder, when it is known.
    pub(crate) resource_dir: Option<&'a Path>,
}

/// Whether `program` is the only program in its folder. A folder found on `PATH` that holds
/// others (`~/.cargo/bin`, `/usr/local/bin`) must not go first on the agents' `PATH`, ahead of
/// the system's folders (`ProfilePaths::path_prepend`: it holds stetdoc alone); the agent still
/// finds stetdoc through the `PATH` it inherits. An unreadable folder counts as shared.
fn alone_in_its_folder(program: &Path) -> bool {
    let Some(folder) = program.parent() else {
        return false;
    };
    let Ok(entries) = std::fs::read_dir(folder) else {
        return false;
    };
    let shared = entries.flatten().any(|entry| {
        let path = entry.path();
        path.file_name() != program.file_name() && is_program(&path)
    });
    if shared {
        tracing::warn!(
            folder = %folder.display(),
            "stetdoc's folder holds other programs: it does not go first on the agents' PATH"
        );
    }
    !shared
}

/// Whether `path` is a file the OS would run: on Unix, a file with an execute bit; on Windows,
/// a file whose extension is in `PATHEXT` (default `.COM;.EXE;.BAT;.CMD`).
fn is_program(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_owned());
        path.extension().is_some_and(|ext| {
            let ext = format!(".{}", ext.to_string_lossy());
            exts.split(';')
                .any(|known| known.eq_ignore_ascii_case(&ext))
        })
    }
}

/// The profile's paths: the browser's sidecars beside `exe` (where Tauri's `externalBin` puts
/// them; S27 bundles them, and until stage C's proxy the host drops the `browser` entry
/// anyway), except agent-browser on macOS ([`agent_browser_path`]), and stetdoc at `stetdoc`,
/// or, when none was found, the placeholder `<exe folder>/stetdoc/stetdoc`, which is then
/// neither run nor put on `PATH`.
pub(crate) fn paths_for(
    locations: &Locations,
    exe: PathBuf,
    stetdoc: Option<PathBuf>,
) -> Result<ProfilePaths, BuildError> {
    let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    let program = |name: &str| format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let stetdoc = stetdoc.unwrap_or_else(|| dir.join(STETDOC).join(program(STETDOC)));
    let browser = BrowserSidecars {
        clauseo_mcp: dir.join(program("clauseo-mcp")),
        agent_browser: agent_browser_path(&dir, cfg!(target_os = "macos"), Path::is_file),
    };
    ProfilePaths::new(locations, exe, stetdoc, browser).map_err(BuildError::Paths)
}

/// The agent-browser sidecar for an app whose exe is in `dir`. On macOS (`macos`) the release
/// puts it in `Clauseo.app/Contents/Helpers`, out of `Contents/MacOS`, which the agent's sandbox
/// can reach (.github/scripts/sidecars.mjs, decision record 0035): `<dir>/../Helpers/` is used
/// when `exists` finds the file there. Elsewhere, and on macOS when it is not there, beside the
/// exe, where Tauri's `externalBin` puts it.
///
/// The macOS fallback beside the exe is transitional, for an app bundled before the move. It is
/// removed in the same change as stage C's J3, the sandbox deny on `Contents/Helpers`, so that no
/// release ever has the helper where the agent can run it.
fn agent_browser_path(dir: &Path, macos: bool, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let name = format!("clauseo-agent-browser{}", std::env::consts::EXE_SUFFIX);
    if macos && let Some(contents) = dir.parent() {
        let helper = contents.join("Helpers").join(&name);
        if exists(&helper) {
            return helper;
        }
    }
    dir.join(name)
}

/// Where a release bundles stetdoc, in a folder of its own so that only that folder goes first
/// on the agents' `PATH` (U13; `.github/scripts/sidecars.mjs`, `crates/desktop/stetdoc.toml`).
/// On macOS (`macos`), `Clauseo.app/Contents/Helpers/stetdoc/stetdoc`, found from `exe_dir`
/// (`Contents/MacOS`): notarisation wants no program in `Contents/Resources`. Elsewhere
/// `<resource_dir>/stetdoc/stetdoc[.exe]`, Tauri's resource folder (on Windows the install
/// folder, beside `Clauseo.exe`; on Linux `/usr/lib/Clauseo`). None when that folder is not
/// known.
pub(crate) fn bundled_stetdoc(
    macos: bool,
    exe_dir: &Path,
    resource_dir: Option<&Path>,
) -> Option<PathBuf> {
    let program = format!("{STETDOC}{}", std::env::consts::EXE_SUFFIX);
    let folder = if macos {
        exe_dir.parent()?.join("Helpers")
    } else {
        resource_dir?.to_path_buf()
    };
    Some(folder.join(STETDOC).join(program))
}

/// The stetdoc to run: `bundled` (from [`bundled_stetdoc`]) when it is a program there. When
/// it is not, a `release` build runs none and warns (the profile then leaves stetdoc out), and
/// a debug build takes the first `stetdoc` on `path` (the `PATH` value).
pub(crate) fn stetdoc_program(
    bundled: Option<&Path>,
    path: Option<&OsStr>,
    release: bool,
) -> Option<PathBuf> {
    if let Some(bundled) = bundled.filter(|bundled| is_program(bundled)) {
        return Some(bundled.to_path_buf());
    }
    if release {
        tracing::warn!(
            expected = ?bundled,
            "the bundled stetdoc is missing: agents start without it"
        );
        return None;
    }
    first_on_path(STETDOC, path?)
}

/// The first program named `name` (with the OS's executable suffix, `.exe` on Windows) in an
/// absolute folder of `path`, as `which` finds it: a file that is not a program
/// ([`is_program`], no execute bit on Unix) is skipped and the walk goes on. A relative folder
/// is skipped too: the program path must be absolute. Only `.exe` is looked for on Windows,
/// since stetdoc is started with no shell.
#[expect(
    clippy::disallowed_methods,
    reason = "debug builds look stetdoc up on PATH when no bundled copy is there; xtask's first_on_path is not reachable from the app"
)]
pub(crate) fn first_on_path(name: &str, path: &OsStr) -> Option<PathBuf> {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(&file))
        .find(|candidate| is_program(candidate))
}

/// `server` as the host sends it.
fn stdio(server: ProfileServer) -> McpServer {
    McpServer::Stdio {
        name: server.name,
        command: server.command,
        args: server.args,
        env: server.env.into_iter().collect(),
    }
}

impl ProfileHook for DesktopProfile {
    fn session_meta(
        &self,
        agent: &AgentId,
        packs: Option<&PacksWritten>,
    ) -> Option<Map<String, Value>> {
        self.profile()
            .session_meta_with_packs(AgentKind::of(agent), packs)
    }

    fn launch_env(&self, agent: &AgentId) -> BTreeMap<OsString, OsString> {
        if AgentKind::of(agent).is_none() {
            return BTreeMap::new();
        }
        self.profile()
            .env()
            .0
            .into_iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
            .collect()
    }

    fn mcp_servers(&self, agent: &AgentId) -> Vec<McpServer> {
        let Some(kind) = AgentKind::of(agent) else {
            return Vec::new();
        };
        self.profile()
            .servers(Some(kind))
            .into_iter()
            .map(stdio)
            .collect()
    }

    fn codex_config(
        &self,
        agent: &AgentId,
        written: Option<&SkillsWritten>,
        packs: Option<&PacksWritten>,
    ) -> Map<String, Value> {
        self.profile()
            .codex_keys_with_packs(AgentKind::of(agent), written, packs)
    }

    fn launch_writes(&self, agent: &AgentId) -> Vec<SkillWrite> {
        self.profile().launch_writes(AgentKind::of(agent))
    }

    fn pack_write(&self, agent: &AgentId, request: &PackRequest) -> Option<PackWrite> {
        self.profile().pack_write(
            AgentKind::of(agent),
            request.entries().to_vec(),
            request.allow_scripts(),
        )
    }

    fn browser_tool(&self) -> Option<PathBuf> {
        self.browser_tool.clone()
    }
}

/// `config` with setup's hook (`setup/hook.rs`: the key and plan variables) over the launch
/// profile, stetdoc's folder first on the agents' `PATH` when stetdoc was found, that stetdoc
/// for the Word blackline (`HostConfig::with_stetdoc`), and the shell browser's folder when its
/// copy is there (the host puts it on a session's `PATH` only when that session has the
/// browser); with no
/// profile (it could not be built) the hook adds only setup's variables.
pub(crate) fn with_profile(
    config: HostConfig,
    profile: Option<DesktopProfile>,
    hook: SetupHook,
) -> HostConfig {
    let Some(profile) = profile else {
        return config.with_profile_hook(Box::new(hook));
    };
    let config = match profile.path_prepend().and_then(absolute_folder) {
        Some(folder) => config.with_agent_path_prepend(folder),
        None => config,
    };
    // The Word blackline runs the stetdoc found, under the profile's author (the lawyer's
    // name, or "Clauseo"); with none found, the host refuses it (`blackline.no_word_tool`).
    let config = match profile.stetdoc() {
        Some(program) => {
            config.with_stetdoc(program.to_path_buf(), profile.profile().author().to_owned())
        }
        None => config,
    };
    config.with_profile_hook(Box::new(hook.over(Box::new(profile))))
}

/// `folder` when it is absolute. The host takes the agents' `PATH` folder as given, and a
/// relative one would name a folder under each agent's working folder, so a relative folder is
/// a bug (a debug build stops on it) and a release build logs it and prepends nothing.
fn absolute_folder(folder: std::path::PathBuf) -> Option<std::path::PathBuf> {
    debug_assert!(
        folder.is_absolute(),
        "the agents' PATH folder must be absolute: {}",
        folder.display()
    );
    if folder.is_absolute() {
        Some(folder)
    } else {
        tracing::warn!(folder = %folder.display(), "a relative folder does not go on the agents' PATH");
        None
    }
}

#[cfg(test)]
mod tests;
