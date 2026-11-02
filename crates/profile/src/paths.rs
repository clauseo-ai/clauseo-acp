//! Where the profile's files go, and the bundled programs it names.

use std::path::{Path, PathBuf};

use clauseo_settings::Locations;

use crate::error::ProfileError;

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

/// The folders the profile writes and the programs it names. Built once at app start, with
/// [`Self::new`]; the fields are read-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfilePaths {
    pub(crate) claude_plugin: PathBuf,
    pub(crate) codex_skills: PathBuf,
    pub(crate) stetdoc: PathBuf,
    pub(crate) browser: BrowserSidecars,
    pub(crate) browser_config: PathBuf,
    pub(crate) app_exe: PathBuf,
}

impl ProfilePaths {
    /// The paths under `locations`' data folder, with the bundled programs: `app_exe` (the
    /// app's own executable, `std::env::current_exe()`), `stetdoc` (U13's sidecar) and the
    /// browser's sidecars. Each program path must be absolute; a relative one is refused, since
    /// its folder on `PATH` would be the agent's working folder.
    pub fn new(
        locations: &Locations,
        app_exe: PathBuf,
        stetdoc: PathBuf,
        browser: BrowserSidecars,
    ) -> Result<Self, ProfileError> {
        for program in [
            &app_exe,
            &stetdoc,
            &browser.clauseo_mcp,
            &browser.agent_browser,
        ] {
            if !program.is_absolute() || program.parent().is_none() {
                return Err(ProfileError::path(
                    program,
                    "a bundled program's path must be absolute",
                ));
            }
        }
        let data = locations.data_dir();
        Ok(Self {
            claude_plugin: data.join("profile").join("claude-plugin"),
            codex_skills: locations.agent_home(CODEX_AGENT).join("skills"),
            stetdoc,
            browser,
            browser_config: data.join("browser").join("agent-browser.json"),
            app_exe,
        })
    }

    /// Claude's read-only local plugin, `<data>/profile/claude-plugin`.
    pub fn claude_plugin(&self) -> &Path {
        &self.claude_plugin
    }

    /// Codex's skills, `<data>/agents/codex-acp/home/skills` (R15's agent home).
    pub fn codex_skills(&self) -> &Path {
        &self.codex_skills
    }

    /// The stetdoc binary (U13's sidecar), absolute.
    pub fn stetdoc(&self) -> &Path {
        &self.stetdoc
    }

    /// The browser's sidecars, absolute.
    pub fn browser(&self) -> &BrowserSidecars {
        &self.browser
    }

    /// agent-browser's config file, `<data>/browser/agent-browser.json`, as
    /// `AGENT_BROWSER_CONFIG`.
    pub fn browser_config(&self) -> &Path {
        &self.browser_config
    }

    /// The plugin's skills folder, `claude_plugin/skills`.
    pub fn claude_skills(&self) -> PathBuf {
        self.claude_plugin.join("skills")
    }

    /// The folder for `HostConfig::with_agent_path_prepend`: stetdoc's own (U13's `stetdoc/`
    /// resource folder). Everything in it becomes a bare command for the agents, so it must hold
    /// stetdoc alone. None, with a warning, when it is the folder of a browser sidecar or of the
    /// app itself (Tauri's `externalBin` puts every sidecar beside the app), or a root: the
    /// host then prepends nothing, and stetdoc is not on the agents' `PATH`.
    pub fn path_prepend(&self) -> Option<&Path> {
        let dir = self.stetdoc.parent()?;
        dir.parent()?;
        let shared = [
            &self.app_exe,
            &self.browser.clauseo_mcp,
            &self.browser.agent_browser,
        ]
        .into_iter()
        .filter_map(|program| program.parent())
        .any(|other| same_folder(other, dir));
        if shared {
            tracing::warn!(
                folder = %dir.display(),
                "stetdoc's folder holds other programs: it does not go on the agents' PATH"
            );
            return None;
        }
        Some(dir)
    }
}

/// Whether `a` and `b` name the same folder: each `dunce::simplified`, then compared
/// lexically, so it reads Windows-style paths the same on every OS. A `\\?\` prefix goes
/// (`\\?\UNC\` becomes `\\`), `/` and `\\` are one separator, empty parts are skipped,
/// and case is ignored (macOS and Windows disks ignore case by default; a false match only
/// keeps stetdoc off `PATH`).
fn same_folder(a: &Path, b: &Path) -> bool {
    lexical_parts(dunce::simplified(a)) == lexical_parts(dunce::simplified(b))
}

/// `path`'s parts as [`same_folder`] compares them.
fn lexical_parts(path: &Path) -> Vec<String> {
    let text = path.to_string_lossy().to_lowercase();
    let text = if let Some(unc) = text.strip_prefix(r"\\?\unc\") {
        format!(r"\\{unc}")
    } else {
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
    };
    let rooted = text.starts_with(['/', '\\']);
    std::iter::once(if rooted { "/" } else { "" }.to_owned())
        .chain(
            text.split(['/', '\\'])
                .filter(|part| !part.is_empty())
                .map(str::to_owned),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_style_folders_match_with_or_without_the_verbatim_prefix() {
        let same = |a: &str, b: &str| same_folder(Path::new(a), Path::new(b));
        assert!(same(r"\\?\C:\x", r"C:\x"));
        assert!(same(
            r"\\?\C:\Program Files\Clauseo",
            r"c:/program files/clauseo/"
        ));
        assert!(same(r"\\?\UNC\server\share\app", r"\\server\share\app"));
        assert!(!same(r"\\?\C:\x", r"C:\x\stetdoc"));
        assert!(!same(r"C:\x", r"D:\x"));
        assert!(same("/app/bin", "/app//bin/"));
        assert!(!same("/app", "app"));
    }
}
