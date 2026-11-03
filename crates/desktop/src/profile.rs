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

use clauseo_host::{McpServer, ProfileHook};
use clauseo_process::{AsyncIoTimer, PassthroughLauncher, SpawnOptions};
use clauseo_profile::skills::stetdoc;
use clauseo_profile::{
    BrowserSidecars, ClauseoProfile, ProfileError, ProfilePaths, ProfileServer, SkillWrite,
    SkillsWritten,
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

/// The launch profile as the host's hook: the profile, built once, and whether stetdoc was
/// found (only then does its folder go on the agents' `PATH`).
#[derive(Debug, Clone)]
pub(crate) struct DesktopProfile {
    profile: ClauseoProfile,
    stetdoc_found: bool,
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
        let skill = found.as_deref().and_then(|program| {
            let launcher = PassthroughLauncher::new(SpawnOptions::new());
            let loaded = async_io::block_on(stetdoc::load_skill(
                &launcher,
                &AsyncIoTimer,
                program,
                &paths.claude_skills(),
            ));
            loaded
                .map_err(|err| tracing::warn!(%err, "stetdoc's skill is left out"))
                .ok()
        });
        let (profile, refused) = ClauseoProfile::new(paths, None).with_builtin_skills(skill);
        for err in refused {
            tracing::warn!(%err, "a skill is left out of the profile");
        }
        Ok(Self::new(profile, found.is_some()))
    }

    /// The hook over `profile`, built and rendered already.
    pub(crate) fn new(profile: ClauseoProfile, stetdoc_found: bool) -> Self {
        Self {
            profile,
            stetdoc_found,
        }
    }

    /// The folder for `HostConfig::with_agent_path_prepend`: stetdoc's, when stetdoc was found
    /// and its folder holds no other program (`ProfilePaths::path_prepend`).
    pub(crate) fn path_prepend(&self) -> Option<PathBuf> {
        if !self.stetdoc_found {
            return None;
        }
        self.profile.path_prepend().map(Path::to_path_buf)
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

/// The first file named `name` (with the OS's executable suffix) in an absolute folder of
/// `path`. A relative folder is skipped: the program path must be absolute.
#[expect(
    clippy::disallowed_methods,
    reason = "debug builds look stetdoc up on PATH until U13 bundles it; xtask's first_on_path is not reachable from the app"
)]
pub(crate) fn first_on_path(name: &str, path: &OsStr) -> Option<PathBuf> {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(&file))
        .find(|candidate| candidate.is_file())
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
