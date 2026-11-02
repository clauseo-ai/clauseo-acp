//! Where the profile's files go, and the bundled programs it names.

use std::path::{Path, PathBuf};

use clauseo_settings::Locations;

/// The registry id of Codex, whose home holds Codex's copy of the skills.
const CODEX_AGENT: &str = "codex-acp";

/// The two sidecars the `browser` entry runs, both in every bundle (S27): `clauseo-mcp`, whose
/// `browser-guard` subcommand starts and guards agent-browser's MCP server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserSidecars {
    /// The `clauseo-mcp` binary: the entry's command.
    pub clauseo_mcp: PathBuf,
    /// The agent-browser binary, passed as `--agent-browser`.
    pub agent_browser: PathBuf,
}

/// The folders the profile writes and the programs it names. Built once at app start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfilePaths {
    /// Claude's read-only local plugin, `<data>/profile/claude-plugin`.
    pub claude_plugin: PathBuf,
    /// Codex's skills, `<data>/agents/codex-acp/home/skills` (R15's agent home).
    pub codex_skills: PathBuf,
    /// The stetdoc binary (U13's sidecar). Its folder goes first on the agents' `PATH`.
    pub stetdoc: PathBuf,
    /// The browser's sidecars.
    pub browser: BrowserSidecars,
    /// agent-browser's config file, `<data>/browser/agent-browser.json`, as
    /// `AGENT_BROWSER_CONFIG`.
    pub browser_config: PathBuf,
}

impl ProfilePaths {
    /// The paths under `locations`' data folder, with the bundled programs.
    pub fn new(locations: &Locations, stetdoc: PathBuf, browser: BrowserSidecars) -> Self {
        let data = locations.data_dir();
        Self {
            claude_plugin: data.join("profile").join("claude-plugin"),
            codex_skills: locations.agent_home(CODEX_AGENT).join("skills"),
            stetdoc,
            browser,
            browser_config: data.join("browser").join("agent-browser.json"),
        }
    }

    /// The plugin's skills folder, `claude_plugin/skills`.
    pub fn claude_skills(&self) -> PathBuf {
        self.claude_plugin.join("skills")
    }

    /// The folder for `HostConfig::with_agent_path_prepend`: stetdoc's own. U13 is asked for a
    /// folder holding stetdoc alone, since everything in it becomes a bare command for the
    /// agents. `stetdoc` itself when it has no parent.
    pub fn path_prepend(&self) -> &Path {
        self.stetdoc.parent().unwrap_or(&self.stetdoc)
    }
}
