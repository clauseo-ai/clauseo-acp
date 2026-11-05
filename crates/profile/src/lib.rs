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
//! - The skill packs ([`packs`]): a lawyer's or a firm's skill folders, checked and hashed per
//!   launch, rendered once per hash into a cache, and written into a launch's own second
//!   plugin (decision record 0040).
//!
//! What differs per session goes out as [`placeholders`] the host fills. The host decides
//! everything the settings decide (web search on or off, the seeded `WebFetch` rules, the
//! preflight switch), since the hook sees only the agent's id.

pub mod claude;
pub mod codex;
pub mod error;
pub mod instructions;
pub mod packs;
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
pub use packs::{PackEntry, PackWrite, PacksWritten, write_packs};
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

    /// Codex's keys with no skills index: `developer_instructions` is [`Self::instructions`].
    /// The index goes in only through [`Self::codex_keys`], with a [`SkillsWritten`].
    pub fn codex_config(&self) -> CodexConfig {
        CodexConfig::with_instructions(self.instructions())
    }

    /// Codex's instructions with the index: [`Self::instructions`], then a blank line and the
    /// skills index ([`Self::skills_index`]) when a skill is rendered.
    fn codex_instructions_with_index(&self) -> String {
        let text = self.instructions();
        let index = self.skills_index();
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
        self.session_meta_with_packs(kind, None)
    }

    /// [`Self::session_meta`], with the launch's pack plugin as a second `plugins` entry when
    /// `packs` is given and wrote a skill: only [`write_packs`] makes the token, so Claude never
    /// names a plugin the launch did not write.
    pub fn session_meta_with_packs(
        &self,
        kind: Option<AgentKind>,
        packs: Option<&PacksWritten>,
    ) -> Option<Map<String, Value>> {
        (kind == Some(AgentKind::Claude)).then(|| {
            let options = self.claude_options();
            match packs.filter(|packs| !packs.skills().is_empty()) {
                Some(packs) => options.with_pack_plugin(packs.plugin().to_owned()),
                None => options,
            }
            .meta_fragment()
        })
    }

    /// The `CODEX_CONFIG` keys for an agent of `kind` with the index of what this launch
    /// wrote: the built-in lines when `written` is the Claude plugin's [`SkillsWritten`], the
    /// pack lines when `packs` is given ([`packs::index::codex_index_with_packs`]). With
    /// neither, [`Self::codex_keys_without_index`].
    pub fn codex_keys_with_packs(
        &self,
        kind: Option<AgentKind>,
        written: Option<&SkillsWritten>,
        packs: Option<&PacksWritten>,
    ) -> Map<String, Value> {
        if kind != Some(AgentKind::Codex) {
            return Map::new();
        }
        let skills_root = self.paths.claude_skills();
        let builtin = written.and_then(|written| {
            if written.root == skills_root {
                Some((skills_root.as_path(), self.skills.as_ref()))
            } else {
                tracing::warn!("a skills write of another folder: Codex gets no built-in index");
                None
            }
        });
        let index = packs::index::codex_index_with_packs(builtin, packs);
        let text = self.instructions();
        if index.is_empty() {
            return CodexConfig::with_instructions(text).into_keys();
        }
        CodexConfig::with_instructions(format!("{text}\n\n{index}")).into_keys()
    }

    /// The `CODEX_CONFIG` keys for an agent of `kind`: Codex's for Codex, none otherwise, with
    /// the skills index in `developer_instructions`. `written` is what [`write_skills`] returned
    /// for the Claude plugin's write (the first of [`Self::launch_writes`]): only a successful
    /// write makes one, so after a failed write the host has only
    /// [`Self::codex_keys_without_index`], and the index never names files the write did not put
    /// there (decision record 0031). A token from another folder's write gives no index, with a
    /// warning.
    pub fn codex_keys(
        &self,
        kind: Option<AgentKind>,
        written: &SkillsWritten,
    ) -> Map<String, Value> {
        if kind != Some(AgentKind::Codex) {
            return Map::new();
        }
        if written.root != self.paths.claude_skills() {
            tracing::warn!("a skills write of another folder: Codex gets no skills index");
            return self.codex_config().into_keys();
        }
        CodexConfig::with_instructions(self.codex_instructions_with_index()).into_keys()
    }

    /// The `CODEX_CONFIG` keys for an agent of `kind` with no skills index: for a launch whose
    /// skills write failed.
    pub fn codex_keys_without_index(&self, kind: Option<AgentKind>) -> Map<String, Value> {
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
        match kind? {
            AgentKind::Claude => Some(self.claude_write()),
            AgentKind::Codex if CODEX_SKILLS => Some(SkillWrite {
                root: self.paths.codex_skills.clone(),
                plugin: None,
                rendered: Arc::clone(&self.skills),
            }),
            AgentKind::Codex => None,
        }
    }

    /// Every skills write the launch-prep thread runs, with [`write_skills`], before an agent of
    /// `kind` starts. Claude: its plugin. Codex: the Claude plugin too, since Codex's index
    /// names the files there (and its own folder, once [`CODEX_SKILLS`] is on). None for an
    /// agent with no kind. Pure and cheap, as [`Self::skill_files`].
    pub fn launch_writes(&self, kind: Option<AgentKind>) -> Vec<SkillWrite> {
        match kind {
            Some(AgentKind::Claude) => vec![self.claude_write()],
            Some(AgentKind::Codex) => std::iter::once(self.claude_write())
                .chain(self.skill_files(kind))
                .collect(),
            None => Vec::new(),
        }
    }

    /// The skill packs an agent of `kind` loads and writes into its launch's own plugin, as
    /// data for the launch-prep thread ([`PackWrite::load`], then [`write_packs`]): `entries`
    /// (the packs the host admitted, each with the hash recorded when it was turned on) in
    /// [`ProfilePaths::skill_packs`]. None for an agent with no kind, or with no pack to load.
    /// Pure and cheap: no file is read here (decision record 0040).
    pub fn pack_write(
        &self,
        kind: Option<AgentKind>,
        entries: Vec<PackEntry>,
        allow_scripts: bool,
    ) -> Option<PackWrite> {
        kind?;
        (!entries.is_empty())
            .then(|| PackWrite::new(self.paths.skill_packs.clone(), entries, allow_scripts))
    }

    /// The write of Claude's plugin.
    fn claude_write(&self) -> SkillWrite {
        SkillWrite {
            root: self.paths.claude_skills(),
            plugin: Some(self.paths.claude_plugin.clone()),
            rendered: Arc::clone(&self.skills),
        }
    }
}

/// Skills to write into one skills folder. Only [`ClauseoProfile::skill_files`] and
/// [`ClauseoProfile::launch_writes`] make one.
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
/// Then the retired built-ins ([`skills::builtin::RETIRED_BUILTINS`]) go from the folder
/// ([`skills::retire`]), and their files are reported as removed.
///
/// The host logs an error from here as a warning and never fails a launch on it: the agent then
/// runs with the skills the folder already holds, or none, and a Codex agent gets
/// [`ClauseoProfile::codex_keys_without_index`].
pub fn write_skills(skills: &SkillWrite) -> Result<SkillsWritten, ProfileError> {
    let _held = skills::write_lock();
    if let Some(plugin) = &skills.plugin {
        skills::no_links(plugin, &PLUGIN_MANIFEST_PATH)?;
        skills::no_links(plugin, &["skills"])?;
        let [folder, file] = PLUGIN_MANIFEST_PATH;
        write::sweep_temps(&plugin.join(folder), false);
        write::replace_if_differs(&plugin.join(folder).join(file), PLUGIN_MANIFEST.as_bytes())?;
    }
    let mut report = skills::write_unlocked(&skills.root, &skills.rendered)?;
    report.removed.extend(skills::retire_unlocked(
        &skills.root,
        &skills::builtin::RETIRED_BUILTINS,
    )?);
    Ok(SkillsWritten {
        root: skills.root.clone(),
        report,
    })
}

/// Proof that [`write_skills`] wrote one skills folder: only it makes one. The Codex keys with
/// the skills index take it ([`ClauseoProfile::codex_keys`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillsWritten {
    root: PathBuf,
    report: RenderReport,
}

impl SkillsWritten {
    /// The skills folder written.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// What the write did.
    pub fn report(&self) -> &RenderReport {
        &self.report
    }
}
