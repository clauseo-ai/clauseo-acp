//! What a launch reads on the actor before its checks go to the launch-prep thread
//! ([`super::prepare`]): the matter folder, the agent's home, the session's folders, its skill
//! writes, its skill packs and its browser and case-law claims. Reads only; nothing is made here.

use std::path::PathBuf;

use clauseo_profile::SkillWrite;
use clauseo_protocol::{AgentKind, ThreadId};

use super::RunFolders;
use super::browser::{BrowserPrep, browser_prep};
use super::packs::{PackPrep, pack_prep};
use super::relay::{self, CaseLawPrep};
use crate::actor::Actor;
use crate::config::SessionKey;
use crate::error::Error;

/// What a launch's checks run on, read on the actor by [`launch_paths`].
pub(super) struct LaunchPaths {
    /// The matter folder as the thread's matter row stored it.
    pub(super) stored: PathBuf,
    /// The data folder.
    pub(super) data: PathBuf,
    /// The agent's login home, `Locations::agent_home` of its id, for an agent with a kind.
    pub(super) home: Option<PathBuf>,
    /// Whether the agent is Claude.
    pub(super) claude: bool,
    /// The session's folders, not yet made.
    pub(super) run: RunFolders,
    /// The profile's skill writes for the agent; none for an agent without a kind.
    pub(super) skills: Vec<SkillWrite>,
    /// The launch's skill packs (`packs.rs`).
    pub(super) packs: PackPrep,
    /// The browser claims and the case-law relay, claimed on the launch-prep thread.
    pub(super) claims: (BrowserPrep, CaseLawPrep),
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
    // The matter's state folder for an agent with a kind (`agent/state_folder.rs`).
    let state = kind.map(|_| {
        crate::agent::state_folder::state_dir(locations.data_dir(), row.matter, row.agent.as_str())
    });
    let run = RunFolders::new(locations.data_dir(), key).with_state(state);
    let skills = match kind {
        Some(_) => actor.profile.launch_writes(&row.agent),
        None => Vec::new(),
    };
    let claims = (
        browser_prep(actor, &row.agent, key),
        relay::case_law_prep(actor, &row.agent),
    );
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
