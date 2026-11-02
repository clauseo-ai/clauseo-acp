//! Clauseo's launch profile (R21): what Clauseo adds to every agent session, as plain values
//! and files, translated into each agent's own options. `crates/desktop` adapts it to the
//! host's `ProfileHook`; this crate knows neither `host` nor `acp`.
//!
//! - The instructions ([`instructions`]): R21's tools part, with later parts appended (R22).
//!   Claude gets them in `_meta.systemPrompt.append`, Codex as `developer_instructions`.
//! - Claude's options ([`claude`]): the allow list, the skills plugin, and
//!   `disableSkillShellExecution`. Codex's keys ([`codex`]).
//! - The `browser` MCP server ([`servers`]), for every agent.
//! - The launch variables: `STETDOC_AUTHOR`.
//! - The skills ([`skills`]), rendered into Claude's plugin; Codex gets an index of them in
//!   its instructions ([`skills::builtin::codex_index`]).
//!
//! What differs per session goes out as [`placeholders`] the host fills. The host decides
//! everything the settings decide (web search on or off, the seeded `WebFetch` rules, the
//! preflight switch), since the hook sees only the agent's id.

pub mod claude;
pub mod codex;
pub mod error;
pub mod instructions;
pub mod paths;
pub mod placeholders;
pub mod servers;
pub mod skills;
#[cfg(feature = "test-support")]
pub mod test_support;
mod write;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_protocol::AgentKind;
use serde_json::{Map, Value};

pub use claude::ClaudeOptions;
pub use codex::CodexConfig;
pub use error::ProfileError;
pub use instructions::Instructions;
pub use paths::{BrowserSidecars, ProfilePaths};
pub use servers::ProfileServer;
pub use skills::{RenderReport, Rendered, RenderedSkill, SkillFile, SkillSource};

/// The variable stetdoc reads its author from when `--author` is absent.
pub const STETDOC_AUTHOR: &str = "STETDOC_AUTHOR";

/// Whether Codex gets the skills in its home's `skills/` folder. Off: R15 runs Codex with
/// `skills.include_instructions=false` (`clauseo_limits::CODEX_OVERRIDES`), so no skill catalog
/// reaches the model, and codex 0.162.1 has no key that keeps the lawyer's `~/.agents/skills`
/// out while letting a root under the agent home in (decision record 0030). Codex gets the
/// stetdoc rules, and an index of the skills Claude's plugin holds, through
/// `developer_instructions` instead (decision record 0031).
pub const CODEX_SKILLS: bool = false;

/// The Claude plugin's manifest, `.claude-plugin/plugin.json`.
const PLUGIN_MANIFEST: &str = concat!(
    "{\n",
    "  \"name\": \"clauseo\",\n",
    "  \"description\": \"Clauseo's built-in skills.\"\n",
    "}\n"
);

/// Launch variables the profile adds, by name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProfileEnv(pub BTreeMap<String, String>);

/// The profile, built once at app start.
#[derive(Debug, Clone)]
pub struct ClauseoProfile {
    paths: ProfilePaths,
    author: String,
    instructions: Instructions,
    skills: Arc<Rendered>,
}

impl ClauseoProfile {
    /// The profile with the launch instructions (R21's tools part, then R22's legal part) and no
    /// skills yet. `author` is the lawyer's name from the settings; there is no such key yet, so
    /// desktop passes `None` and tracked changes carry `Clauseo`. Never the OS account name.
    /// [`instructions::author_name`] makes a name safe.
    pub fn new(paths: ProfilePaths, author: Option<String>) -> Self {
        Self {
            paths,
            author: instructions::author_name(&author.unwrap_or_default()),
            instructions: Instructions::launch(),
            skills: Arc::default(),
        }
    }

    /// The same profile with `skills` rendered once, here, for every later
    /// [`Self::skill_files`]: stetdoc's from [`skills::stetdoc::load_skill`] (run before, off
    /// the host thread), and R22's built-ins. A skill the renderer refuses is left out with a
    /// warning ([`skills::render_each`]), so a bad stetdoc text never costs the other skills;
    /// the refusals are returned for the caller to log or show.
    #[must_use]
    pub fn with_skills(mut self, skills: &[SkillSource]) -> (Self, Vec<ProfileError>) {
        let (rendered, refused) = skills::render_each(skills);
        self.skills = Arc::new(rendered);
        (self, refused)
    }

    /// The same profile with stetdoc's skill (when it loaded) and R22's three built-ins
    /// ([`skills::builtin::with_stetdoc`]), rendered as [`Self::with_skills`] does.
    #[must_use]
    pub fn with_builtin_skills(self, stetdoc: Option<SkillSource>) -> (Self, Vec<ProfileError>) {
        let (sources, mut refused) = skills::builtin::with_stetdoc(stetdoc);
        let (profile, more) = self.with_skills(&sources);
        refused.extend(more);
        (profile, refused)
    }

    /// The same profile with `instructions` in place of [`Instructions::launch`].
    #[must_use]
    pub fn with_instructions(mut self, instructions: Instructions) -> Self {
        self.instructions = instructions;
        self
    }

    /// The paths.
    pub fn paths(&self) -> &ProfilePaths {
        &self.paths
    }

    /// The author name tracked changes carry.
    pub fn author(&self) -> &str {
        &self.author
    }

    /// The composed instructions, `{author}` filled, `{work_folder}` and `{matter_notes}` left.
    pub fn instructions(&self) -> String {
        self.instructions.compose(&self.author)
    }

    /// The servers for an agent of `kind`: `browser`, whatever the kind.
    pub fn servers(&self, _kind: Option<AgentKind>) -> Vec<ProfileServer> {
        vec![servers::browser(&self.paths)]
    }

    /// Claude's options.
    pub fn claude_options(&self) -> ClaudeOptions {
        ClaudeOptions::new(self.instructions(), Some(self.paths.claude_plugin.clone()))
    }

    /// Codex's keys: `developer_instructions` is [`Self::codex_instructions`].
    pub fn codex_config(&self) -> CodexConfig {
        CodexConfig::with_instructions(self.codex_instructions())
    }

    /// Codex's instructions: [`Self::instructions`], then a blank line and the skills index
    /// ([`Self::skills_index`]) when a skill is rendered. Codex gets no skills folder
    /// ([`CODEX_SKILLS`]), so the index points it at the files Claude's plugin holds. Built on
    /// each call.
    pub fn codex_instructions(&self) -> String {
        let index = self.skills_index();
        let text = self.instructions();
        if index.is_empty() {
            text
        } else {
            format!("{text}\n\n{index}")
        }
    }

    /// The skills index for Codex: each rendered skill's name, its use and the path of its
    /// rendered `SKILL.md` in the Claude plugin's `skills/` folder, where [`write_skills`]
    /// puts it ([`skills::builtin::codex_index`]). Claude gets none: it loads the plugin.
    pub fn skills_index(&self) -> String {
        skills::builtin::codex_index(&self.paths.claude_skills(), &self.skills)
    }

    /// The `_meta` for an agent of `kind`: Claude's fragment for Claude, none otherwise.
    pub fn session_meta(&self, kind: Option<AgentKind>) -> Option<Map<String, Value>> {
        (kind == Some(AgentKind::Claude)).then(|| self.claude_options().meta_fragment())
    }

    /// The `CODEX_CONFIG` keys for an agent of `kind`: Codex's for Codex, none otherwise.
    pub fn codex_keys(&self, kind: Option<AgentKind>) -> Map<String, Value> {
        if kind == Some(AgentKind::Codex) {
            self.codex_config().into_keys()
        } else {
            Map::new()
        }
    }

    /// The launch variables: [`STETDOC_AUTHOR`], for every agent.
    pub fn env(&self) -> ProfileEnv {
        ProfileEnv(BTreeMap::from([(
            STETDOC_AUTHOR.to_owned(),
            self.author.clone(),
        )]))
    }

    /// The folder to put first on the agents' `PATH`, or none when stetdoc's folder holds
    /// other programs ([`ProfilePaths::path_prepend`]).
    pub fn path_prepend(&self) -> Option<&Path> {
        self.paths.path_prepend()
    }

    /// The skills for an agent of `kind`, as data to write before its launch: Claude's into the
    /// plugin (with the plugin's manifest); none for Codex while [`CODEX_SKILLS`] is off, and none
    /// for an agent with no kind. Pure and cheap: what [`Self::with_skills`] rendered, shared
    /// through an `Arc`, with no file read, no process and no copy of the bytes. The
    /// launch-prep thread hands it to [`write_skills`].
    pub fn skill_files(&self, kind: Option<AgentKind>) -> Option<SkillWrite> {
        let (root, plugin) = match kind? {
            AgentKind::Claude => (
                self.paths.claude_skills(),
                Some(self.paths.claude_plugin.clone()),
            ),
            AgentKind::Codex if CODEX_SKILLS => (self.paths.codex_skills.clone(), None),
            AgentKind::Codex => return None,
        };
        Some(SkillWrite {
            root,
            plugin,
            rendered: Arc::clone(&self.skills),
        })
    }
}

/// Skills to write into one agent's skills folder. Only [`ClauseoProfile::skill_files`] makes
/// one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillWrite {
    root: PathBuf,
    plugin: Option<PathBuf>,
    rendered: Arc<Rendered>,
}

impl SkillWrite {
    /// The skills folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Claude's plugin folder, whose `.claude-plugin/plugin.json` the write keeps; none for
    /// Codex.
    pub fn plugin(&self) -> Option<&Path> {
        self.plugin.as_deref()
    }

    /// The skills.
    pub fn rendered(&self) -> &Rendered {
        &self.rendered
    }
}

/// The plugin manifest's folder and file under the plugin folder.
const PLUGIN_MANIFEST_PATH: [&str; 2] = [".claude-plugin", "plugin.json"];

/// Writes `skills` (and the plugin manifest, when there is one) as [`skills::write_rendered`]
/// does, under the same process-wide lock, and through no symlink. Reads and writes files: on
/// the launch-prep thread, never the host's.
///
/// The host logs an error from here as a warning and never fails a launch on it: the agent then
/// runs with the skills the folder already holds, or none.
pub fn write_skills(skills: &SkillWrite) -> Result<RenderReport, ProfileError> {
    let _held = skills::write_lock();
    if let Some(plugin) = &skills.plugin {
        skills::no_links(plugin, &PLUGIN_MANIFEST_PATH)?;
        skills::no_links(plugin, &["skills"])?;
        let [folder, file] = PLUGIN_MANIFEST_PATH;
        write::sweep_temps(&plugin.join(folder), false);
        write::replace_if_differs(&plugin.join(folder).join(file), PLUGIN_MANIFEST.as_bytes())?;
    }
    skills::write_unlocked(&skills.root, &skills.rendered)
}
