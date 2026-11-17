//! What a launch reads on the actor before its checks go to the launch-prep thread
//! ([`super::prepare`]): the matter folder, the agent's home, the session's folders, its skill
//! writes and their wait (`skill_wait.rs`), its skill packs, its egress proxy and its browser
//! and case-law claims. Reads only; nothing is made here.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_protocol::{AgentId, AgentKind, ThreadId};

use super::RunFolders;
use super::browser::{BrowserPrep, browser_prep};
use super::egress::{EgressPrep, egress_prep};
use super::packs::{PackPrep, pack_prep};
use super::relay::{self, CaseLawPrep};
use super::skill_wait::{SkillsPrep, skills_prep};
use crate::actor::Actor;
use crate::agent::loads::tls_roots;
use crate::agent::no_git;
use crate::agent::shell_path::{self, SessionPath};
use crate::clock::Clock;
use crate::config::SessionKey;
use crate::error::Error;

/// What a launch's checks run on, read on the actor by [`launch_paths`].
pub(super) struct LaunchPaths {
    /// The matter folder as the thread's matter row stored it.
    pub(super) stored: PathBuf,
    /// The data folder.
    pub(super) data: PathBuf,
    /// The agent's home, `Locations::agent_home` of its id, for an agent with a kind.
    pub(super) home: Option<PathBuf>,
    /// Whether the agent is Claude.
    pub(super) claude: bool,
    /// The session's folders, not yet made.
    pub(super) run: RunFolders,
    /// The profile's skill writes for the agent and their wait; none for an agent without a
    /// kind.
    pub(super) skills: SkillsPrep,
    /// The launch's skill packs (`packs.rs`).
    pub(super) packs: PackPrep,
    /// The egress proxy, the browser claims and the case-law relay, made on the launch-prep
    /// thread.
    pub(super) claims: Claims,
}

/// What the launch-prep thread opens and claims for the session.
pub(super) struct Claims {
    /// The browser's sidecar check, Chrome search and claims.
    pub(super) browser: BrowserPrep,
    /// The case-law relay.
    pub(super) case_law: CaseLawPrep,
    /// The egress proxy.
    pub(super) egress: EgressPrep,
    /// Where the run's parts go when it drops without a completed wait.
    pub(super) held: super::straggler::HeldRuns,
    /// The session's `PATH` folders, found once the browser claims are known.
    pub(super) path: PathPrep,
}

/// The parts of a session's `PATH` read on the actor; the launch-prep thread adds the shell
/// browser's tools once it knows whether the session has them ([`Self::resolve`]).
pub(super) struct PathPrep {
    /// The `git` that does nothing (`HostConfig::with_no_git`) to check, for an agent with a
    /// kind ([`no_git_check`]).
    no_git: Result<Option<no_git::Check>, no_git::Unusable>,
    /// Its folder, once [`Self::check_no_git`] found the program sound.
    no_git_folder: Option<PathBuf>,
    /// Stetdoc's folder (`HostConfig::with_agent_path_prepend`).
    prepend: Option<PathBuf>,
    /// The shell browser's tools folder (`ProfileHook::browser_tool`), when the profile has it.
    browser_tool: Option<PathBuf>,
    /// For a Claude session on macOS: the data folder the trust roots file goes in, and the
    /// clock its `security` runs' time limit runs on.
    tls_roots: Option<(PathBuf, Arc<dyn Clock>)>,
    /// The profile hook's absolute folders (`agent::session_path`), and whether there are any.
    hook: (Vec<PathBuf>, bool),
    /// The agent's kind; the launch's folder-name refusal reads it too (`agent/folder_name.rs`).
    pub(super) kind: Option<AgentKind>,
}

impl PathPrep {
    /// Checks the `git` that does nothing, when the launch has one (`agent::no_git::check_all`):
    /// a program missing, changed or not named refuses the launch with `agent.tool_missing`.
    /// On the launch-prep thread, before anything is made.
    pub(super) fn check_no_git(&mut self) -> Result<(), Error> {
        match &self.no_git {
            Err(why) => return Err(no_git::refusal(*why)),
            Ok(Some(check)) => {
                let folder = no_git::check_all(check).map_err(no_git::refusal)?;
                self.no_git_folder = Some(folder);
            }
            Ok(None) => {}
        }
        Ok(())
    }

    /// The session's folders, in order: the `git` that does nothing's ([`Self::check_no_git`]),
    /// stetdoc's, then the tools for a session that has the
    /// browser as a shell tool with its Chrome (so no `agent-browser` in the Python runtime's
    /// folders shadows Clauseo's), then the hook's; and what its shells get
    /// (`agent::shell_path::prepare`). On the launch-prep thread, after the claims.
    pub(super) fn resolve(self, run: &RunFolders) -> SessionPath {
        let shell = run.browser_shell && run.chrome.is_some();
        let tools = self.browser_tool.filter(|_| shell);
        let (hook, hook_path) = self.hook;
        let no_git = self.no_git_folder.is_some();
        let first = self.no_git_folder.into_iter().chain(self.prepend);
        let dirs = first.chain(tools).chain(hook).collect();
        // The trust roots for a sandbox that will be on: a Claude session on macOS with its
        // short temp folder (`agent::loads::tls_roots`).
        let roots = self
            .tls_roots
            .filter(|_| run.claude_tmp.is_some())
            .and_then(|(data, clock)| tls_roots::write_roots(&data, &tls_roots::security(clock)));
        let path = shell_path::prepare(dirs, hook_path, self.kind, &run.root, roots.as_deref());
        SessionPath { no_git, ..path }
    }
}

/// What a launch of `agent`, and each prompt and session call of it, checks of the `git` that
/// does nothing (`agent::no_git::check_all`): nothing for an agent with no kind; else the
/// program named to the host, with a Windows Codex agent's install folder
/// (`agent::no_git::codex_install`), or the refusal when none was named in a build for the
/// lawyer (`agent::no_git::required`).
pub(in crate::actor::turn) fn no_git_check(
    actor: &Actor,
    agent: &AgentId,
) -> Result<Option<no_git::Check>, no_git::Unusable> {
    no_git::check_for(
        AgentKind::of(agent),
        actor.wiring.no_git.as_ref(),
        actor.effective.agent(agent.as_str()),
        (no_git::TEST_BUILD, cfg!(windows)),
    )
}

/// Reads [`LaunchPaths`] for `thread`'s launch `id`, or the error of the thread or its matter.
pub(super) fn launch_paths(actor: &Actor, thread: ThreadId, id: u64) -> Result<LaunchPaths, Error> {
    let row = &actor.check_thread(thread)?.row;
    let stored = PathBuf::from(&actor.check_matter(row.matter)?.row.folder_path);
    let locations = actor.effective.locations();
    let kind = AgentKind::of(&row.agent);
    // Clauseo's own home for an agent with a kind (`agent/home.rs`), never the lawyer's.
    let home = kind.map(|_| locations.agent_home(row.agent.as_str()));
    let key = SessionKey {
        matter: row.matter,
        thread,
        launch: id,
    };
    let mut run = RunFolders::new(locations.data_dir(), key);
    // Claude's sandbox's short temp folder (`agent::claude_tmp`), made by the launch prep.
    if kind == Some(AgentKind::Claude) && !cfg!(windows) {
        run.claude_tmp = crate::agent::claude_tmp::claude_tmp_dir();
    }
    let skills = skills_prep(actor, &row.agent);
    let browser = browser_prep(actor, &row.agent, key);
    let claims = Claims {
        egress: egress_prep(actor, &row.agent, &browser),
        browser,
        case_law: relay::case_law_prep(actor, &row.agent),
        held: actor.wiring.held.clone(),
        path: PathPrep {
            no_git: no_git_check(actor, &row.agent),
            no_git_folder: None,
            prepend: actor.wiring.path_prepend.clone(),
            browser_tool: actor.profile.browser_tool(),
            tls_roots: (kind == Some(AgentKind::Claude) && cfg!(target_os = "macos"))
                .then(|| (locations.data_dir().to_owned(), Arc::clone(&actor.clock))),
            hook: crate::agent::session_path(None, actor.profile.agent_path(&row.agent)),
            kind,
        },
    };
    let matter = actor.check_matter(row.matter)?.row.skill_packs.as_ref();
    let packs = pack_prep(actor, &row.agent, matter);
    Ok(LaunchPaths {
        stored,
        data: locations.data_dir().to_owned(),
        home,
        claude: kind == Some(AgentKind::Claude),
        run,
        skills,
        packs,
        claims,
    })
}
