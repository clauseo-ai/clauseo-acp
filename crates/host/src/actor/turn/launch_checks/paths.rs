//! What a launch reads on the actor before its checks go to the launch-prep thread
//! ([`super::prepare`]): the matter folder, the agent's home, the session's folders, its skill
//! writes, its skill packs, its egress proxy and its browser and case-law claims. Reads only;
//! nothing is made here.

use std::path::PathBuf;

use clauseo_profile::SkillWrite;
use clauseo_protocol::{AgentKind, ThreadId};

use super::RunFolders;
use super::browser::{BrowserPrep, browser_prep};
use super::egress::{EgressPrep, egress_prep};
use super::packs::{PackPrep, pack_prep};
use super::relay::{self, CaseLawPrep};
use crate::actor::Actor;
use crate::agent::shell_path::{self, SessionPath};
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
    /// The profile's skill writes for the agent; none for an agent without a kind.
    pub(super) skills: Vec<SkillWrite>,
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
    /// Stetdoc's folder (`HostConfig::with_agent_path_prepend`).
    prepend: Option<PathBuf>,
    /// The shell browser's tools folder (`ProfileHook::browser_tool`), when the profile has it.
    browser_tool: Option<PathBuf>,
    /// The profile hook's absolute folders (`agent::session_path`), and whether there are any.
    hook: (Vec<PathBuf>, bool),
    /// The agent's kind.
    kind: Option<AgentKind>,
}

impl PathPrep {
    /// The session's folders, in order: stetdoc's, then the tools for a session that has the
    /// browser as a shell tool with its Chrome (so no `agent-browser` in the Python runtime's
    /// folders shadows Clauseo's), then the hook's; and what its shells get
    /// (`agent::shell_path::prepare`). On the launch-prep thread, after the claims.
    pub(super) fn resolve(self, run: &RunFolders) -> SessionPath {
        let shell = run.browser_shell && run.chrome.is_some();
        let tools = self.browser_tool.filter(|_| shell);
        let (hook, hook_path) = self.hook;
        let dirs = self.prepend.into_iter().chain(tools).chain(hook).collect();
        shell_path::prepare(dirs, hook_path, self.kind, &run.root)
    }
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
    let run = RunFolders::new(locations.data_dir(), key);
    let skills = match kind {
        Some(_) => actor.profile.launch_writes(&row.agent),
        None => Vec::new(),
    };
    let browser = browser_prep(actor, &row.agent, key);
    let claims = Claims {
        egress: egress_prep(actor, &row.agent, &browser),
        browser,
        case_law: relay::case_law_prep(actor, &row.agent),
        held: actor.wiring.held.clone(),
        path: PathPrep {
            prepend: actor.wiring.path_prepend.clone(),
            browser_tool: actor.profile.browser_tool(),
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
