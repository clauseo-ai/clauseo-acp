//! Clauseo's launch profile in the app (decision record 0030, "The desktop adapter"):
//! [`DesktopProfile`], the host's `ProfileHook` over a `clauseo_profile::ClauseoProfile`.
//!
//! [`DesktopProfile::build`] does every file and process call once, on the host-start thread
//! (`host_start.rs`), before the host starts: it finds the bundled programs, runs stetdoc for
//! its skill, and renders the skills. The hook's methods then only map what it holds, as the
//! trait requires: they read no file, start nothing and keep nothing between calls.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::future::Either;

use clauseo_host::{McpServer, ProfileHook};
use clauseo_process::{AsyncIoTimer, PassthroughLauncher, SpawnOptions, Timer as _};
use clauseo_profile::skills::stetdoc;
use clauseo_profile::{
    BrowserSidecars, ClauseoProfile, ProfileError, ProfilePaths, ProfileServer, SkillSource,
    SkillWrite, SkillsWritten,
};
use clauseo_protocol::{AgentId, AgentKind};
use clauseo_settings::Locations;
use serde_json::{Map, Value};

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

/// How long loading stetdoc's skill may take at start-up, both runs together (`--version`, then
/// `skills get core` when the cached skill is stale). The host starts after it, and every
/// command waits for the host, so this bounds how long a slow stetdoc delays the app.
pub(crate) const STETDOC_START_LIMIT: Duration = Duration::from_secs(3);

/// The launch profile as the host's hook: the profile, built once, and whether stetdoc's folder
/// goes on the agents' `PATH` (stetdoc was found, its skill loaded, and its folder holds no
/// other program).
#[derive(Debug, Clone)]
pub(crate) struct DesktopProfile {
    profile: ClauseoProfile,
    prepend_stetdoc: bool,
}

impl DesktopProfile {
    /// The profile under `locations`: the bundled programs beside the app's executable,
    /// stetdoc as [`stetdoc_program`] finds it (its skill loaded when it is there), R22's
    /// built-in skills, and no author (tracked changes carry "Clauseo", 0030). Starts stetdoc and
    /// reads files: on the host-start thread, never the host's. A skill that fails is left out
    /// with a warning; only a refused program path fails the whole profile.
    pub(crate) fn build(locations: &Locations) -> Result<Self, BuildError> {
        Self::build_with(locations, std::env::var_os("PATH").as_deref())
    }

    /// [`Self::build`] with `path` as the `PATH` value stetdoc is looked up on.
    pub(crate) fn build_with(
        locations: &Locations,
        path: Option<&OsStr>,
    ) -> Result<Self, BuildError> {
        let exe = std::env::current_exe().map_err(BuildError::Exe)?;
        let found = stetdoc_program(path);
        let paths = paths_for(locations, exe, found.clone())?;
        let skill = found
            .as_deref()
            .and_then(|program| load_stetdoc_skill(program, &paths.claude_skills()));
        let prepend = skill.is_some() && found.as_deref().is_some_and(alone_in_its_folder);
        let (profile, refused) = ClauseoProfile::new(paths, None).with_builtin_skills(skill);
        for err in refused {
            tracing::warn!(%err, "a skill is left out of the profile");
        }
        Ok(Self::new(profile, prepend))
    }

    /// The hook over `profile`, built and rendered already; `prepend_stetdoc` says whether
    /// stetdoc's folder may go on the agents' `PATH`.
    pub(crate) fn new(profile: ClauseoProfile, prepend_stetdoc: bool) -> Self {
        Self {
            profile,
            prepend_stetdoc,
        }
    }

    /// The folder for `HostConfig::with_agent_path_prepend`: stetdoc's, when it may go first
    /// and holds no sidecar or the app (`ProfilePaths::path_prepend`).
    pub(crate) fn path_prepend(&self) -> Option<PathBuf> {
        if !self.prepend_stetdoc {
            return None;
        }
        self.profile.path_prepend().map(Path::to_path_buf)
    }
}

/// stetdoc's skill from `program`, within [`STETDOC_START_LIMIT`] for both runs. On the
/// deadline the run is dropped, and with it its process handle, which kills stetdoc's tree. A
/// failure or the deadline leaves the skill out with a warning, and stetdoc is then treated as
/// absent (its folder goes on no `PATH`).
fn load_stetdoc_skill(program: &Path, cached: &Path) -> Option<SkillSource> {
    let launcher = PassthroughLauncher::new(SpawnOptions::new());
    let loaded = async_io::block_on(async {
        let load = std::pin::pin!(stetdoc::load_skill(
            &launcher,
            &AsyncIoTimer,
            program,
            cached
        ));
        let deadline = std::pin::pin!(AsyncIoTimer.after(STETDOC_START_LIMIT));
        match futures::future::select(load, deadline).await {
            Either::Left((loaded, _)) => Some(loaded),
            Either::Right(((), _)) => None,
        }
    });
    match loaded {
        Some(Ok(skill)) => Some(skill),
        Some(Err(err)) => {
            tracing::warn!(%err, "stetdoc's skill is left out; stetdoc is treated as absent");
            None
        }
        None => {
            tracing::warn!(
                limit_secs = STETDOC_START_LIMIT.as_secs(),
                "stetdoc took too long and was stopped; it is treated as absent"
            );
            None
        }
    }
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
/// anyway), and stetdoc at `stetdoc`, or, when it was not found, where U13 will put it
/// (`<exe folder>/stetdoc/stetdoc`), which is then neither run nor put on `PATH`.
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
        agent_browser: dir.join(program("agent-browser")),
    };
    ProfilePaths::new(locations, exe, stetdoc, browser).map_err(BuildError::Paths)
}

/// The stetdoc to run. U13's bundled sidecar does not exist yet, so: in a debug build, the
/// first `stetdoc` on `path` (the `PATH` value); in a release build, none (R21's brief, "Waits
/// for U13").
pub(crate) fn stetdoc_program(path: Option<&OsStr>) -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        first_on_path(STETDOC, path?)
    } else {
        None
    }
}

/// The first program named `name` (with the OS's executable suffix, `.exe` on Windows) in an
/// absolute folder of `path`, as `which` finds it: a file that is not a program
/// ([`is_program`], no execute bit on Unix) is skipped and the walk goes on. A relative folder
/// is skipped too: the program path must be absolute. Only `.exe` is looked for on Windows,
/// since stetdoc is started with no shell.
#[expect(
    clippy::disallowed_methods,
    reason = "debug builds look stetdoc up on PATH until U13 bundles it; xtask's first_on_path is not reachable from the app"
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
    fn session_meta(&self, agent: &AgentId) -> Option<Map<String, Value>> {
        self.profile.session_meta(AgentKind::of(agent))
    }

    fn launch_env(&self, agent: &AgentId) -> BTreeMap<OsString, OsString> {
        if AgentKind::of(agent).is_none() {
            return BTreeMap::new();
        }
        self.profile
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
        self.profile
            .servers(Some(kind))
            .into_iter()
            .map(stdio)
            .collect()
    }

    fn codex_config(&self, agent: &AgentId, written: Option<&SkillsWritten>) -> Map<String, Value> {
        let kind = AgentKind::of(agent);
        match written {
            Some(written) => self.profile.codex_keys(kind, written),
            None => self.profile.codex_keys_without_index(kind),
        }
    }

    fn launch_writes(&self, agent: &AgentId) -> Vec<SkillWrite> {
        self.profile.launch_writes(AgentKind::of(agent))
    }
}

#[cfg(test)]
mod tests;
