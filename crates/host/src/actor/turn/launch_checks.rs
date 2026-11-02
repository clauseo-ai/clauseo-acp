//! An agent's launch, in two phases so no file system call runs on the actor. Phase one sends
//! the checks to a `clauseo-codex-fs` thread (`agent/codex/prepare.rs`): for every agent the
//! data-folder refusal (`agent/data_folder.rs`), and for a Codex agent its preparation
//! ([`crate::agent::codex::prepare_launch`]: the matter folder resolved through links, the
//! `.codex` refusal, the home made and the home's `config.toml`). The work that waits for the
//! agent is held on the thread's turn state meanwhile. Phase two, when they answer, launches
//! the agent on the actor with what they found ([`agents::launch_now`]), or fails the waiting
//! work.
//!
//! After both refusals, the same thread makes the session's folders ([`RunFolders`]: its
//! stand-in `run/<session id>` folder with `work` and `browser` under it, and the matter's
//! notes folder) and, for an agent with a kind, writes the profile's skill files
//! (`ProfileHook::skill_files`, `clauseo_profile::write_skills`). A skill write that fails is
//! a warning, never a failed launch. The run folder is removed off the actor when the agent
//! ends ([`remove_run`]); the notes folder stays.

use std::path::{Path, PathBuf};

use clauseo_protocol::{AgentKind, MatterId, ThreadId};

use super::super::Actor;
use super::super::input::Input;
use super::agents::{self, Waiting};
use super::{TurnInput, host_stopping, state};
use futures::task::LocalSpawnExt;

use crate::agent::{codex, data_folder};
use crate::config::SessionKey;
use crate::error::Error;

/// A launch whose checks are running.
#[derive(Debug)]
pub(super) struct Preparing {
    /// Its id: an answer for an earlier launch's checks is dropped.
    id: u64,
    /// The work that waits for the agent's session.
    pub(super) waiting: Vec<Waiting>,
}

/// What a launch's checks found.
#[derive(Debug)]
pub(crate) struct Prepared {
    /// The matter folder as the thread's matter row stored it: the agent's working directory.
    pub(super) stored: PathBuf,
    /// A Codex agent's preparation; `None` for any other agent.
    pub(super) codex: Option<CodexPrepared>,
    /// The session's folders, made.
    pub(super) run: RunFolders,
}

/// The folders a session gets, all under the data folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunFolders {
    /// The session.
    pub(crate) key: SessionKey,
    /// `<data>/run/<session id>`: the stand-in for stage C's per-agent state folder, made at
    /// the launch and removed when the agent ends.
    pub(crate) root: PathBuf,
    /// `{work_folder}`, `<root>/work`.
    pub(crate) work: PathBuf,
    /// `{browser_dir}`, `<root>/browser`.
    pub(crate) browser: PathBuf,
    /// `{matter_notes}`, `<data>/agent-state/<matter id>/notes`: the matter's, shared by its
    /// agents, kept when a session ends.
    pub(crate) notes: PathBuf,
}

impl RunFolders {
    /// The folders of `key`'s session under `data`. Paths only: nothing is made here.
    pub(crate) fn new(data: &Path, key: SessionKey) -> Self {
        let root = run_dir(data).join(key.id());
        Self {
            key,
            work: root.join("work"),
            browser: root.join("browser"),
            notes: notes_dir(data, key.matter),
            root,
        }
    }

    /// Makes them. Before that, the host's first launch takes out every `run` folder a crash
    /// left ([`RunSweep`]), and each launch takes out any an earlier launch of the same thread
    /// left (one whose agent never started). File system calls: off the actor.
    fn make(&self, data: &Path, sweep: &RunSweep) -> Result<(), Error> {
        sweep.once(data);
        let prefix = format!("{}-", self.key.thread);
        sweep_run(data, |name| name.starts_with(&prefix));
        for folder in [&self.work, &self.browser, &self.notes] {
            make_private(folder).map_err(|error| {
                Error::refused(
                    clauseo_protocol::ErrorCode::AgentLaunchFailed,
                    format!("cannot make the folder {}: {error}", folder.display()),
                )
            })?;
        }
        Ok(())
    }
}

/// The start-up sweep of `<data>/run`, once per host start: no launch is live before the
/// first, so every folder there is a crash's leftover. It runs on the first launch-prep thread,
/// under a lock every launch's prep takes before making its folders, so no live folder is
/// taken.
#[derive(Debug, Clone, Default)]
pub(crate) struct RunSweep(std::sync::Arc<std::sync::Mutex<bool>>);

impl RunSweep {
    /// Takes out everything under `<data>/run` the first time; nothing after.
    fn once(&self, data: &Path) {
        let mut done = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !*done {
            sweep_run(data, |_| true);
            *done = true;
        }
    }
}

/// Removes each folder under `<data>/run` whose name `takes`.
fn sweep_run(data: &Path, takes: impl Fn(&str) -> bool) {
    if let Ok(entries) = std::fs::read_dir(run_dir(data)) {
        for entry in entries.flatten() {
            if entry.file_name().to_str().is_some_and(&takes) {
                remove_tree(&entry.path());
            }
        }
    }
}

/// `<data>/run`.
fn run_dir(data: &Path) -> PathBuf {
    data.join("run")
}

/// `<data>/agent-state/<matter>/notes`.
pub(crate) fn notes_dir(data: &Path, matter: MatterId) -> PathBuf {
    data.join("agent-state")
        .join(matter.to_string())
        .join("notes")
}

/// Makes `folder` and its parents, each new one private (`write::create_private_dir_all`).
fn make_private(folder: &Path) -> std::io::Result<()> {
    crate::write::create_private_dir_all(folder)
}

/// Removes `folder` and all it holds, through no link; a failure is a warning.
fn remove_tree(folder: &Path) {
    if let Err(error) = crate::write::remove_tree(folder) {
        tracing::warn!(folder = %folder.display(), %error, "a session's run folder stays");
    }
}

/// What a Codex launch's preparation found.
#[derive(Debug)]
pub(crate) struct CodexPrepared {
    /// The matter folder resolved through links: `_codex/hooks/list` names it.
    pub(super) folder: PathBuf,
    /// The agent's home, made and checked, as `CODEX_HOME`.
    pub(super) home: PathBuf,
    /// The launch profile's `CODEX_CONFIG` keys, filled on the actor
    /// (`profile_loads::launch_keys`).
    pub(super) keys: serde_json::Map<String, serde_json::Value>,
}

/// Phase one: the checks go to their thread, and `waiting` waits on the thread's state.
pub(super) fn prepare(actor: &mut Actor, thread: ThreadId, waiting: Vec<Waiting>) {
    let id = agents::next_agent_id();
    let paths = (|| -> Result<_, Error> {
        let row = &actor.check_thread(thread)?.row;
        let stored = PathBuf::from(&actor.check_matter(row.matter)?.row.folder_path);
        let locations = actor.effective.locations();
        let kind = AgentKind::of(&row.agent);
        let home =
            (kind == Some(AgentKind::Codex)).then(|| locations.agent_home(row.agent.as_str()));
        let key = SessionKey {
            matter: row.matter,
            thread,
            launch: id,
        };
        let run = RunFolders::new(locations.data_dir(), key);
        let skills = kind.and_then(|_| actor.profile.skill_files(&row.agent));
        Ok((stored, locations.data_dir().to_owned(), home, run, skills))
    })();
    let sweep = actor.run_sweep.clone();
    let (stored, data, home, run, skills) = match paths {
        Ok(paths) => paths,
        Err(error) => {
            agents::fail_all(actor, thread, waiting, &error);
            return;
        }
    };
    let Some(state) = state(actor, thread) else {
        return;
    };
    state.preparing = Some(Box::new(Preparing { id, waiting }));
    let clock = std::sync::Arc::clone(&actor.clock);
    actor.spawn_post(async move {
        let result = codex::prepare::off_actor(clock, move || {
            data_folder::refuse_data_folder(&stored, &data)?;
            let codex = match home {
                Some(home) => {
                    let folder = codex::prepare_launch(&stored, &data, &home)?;
                    Some(CodexPrepared {
                        folder,
                        home,
                        keys: serde_json::Map::new(),
                    })
                }
                None => None,
            };
            run.make(&data, &sweep)?;
            if let Some(skills) = skills
                && let Err(error) = clauseo_profile::write_skills(&skills)
            {
                tracing::warn!(%error, "the profile's skills were not written; the agent starts with what the folder holds");
            }
            Ok(Prepared { stored, codex, run })
        })
        .await;
        let result = result.map(Box::new);
        Input::Turn(TurnInput::Prepared { thread, id, result })
    });
}

/// Phase two: the checks `id` answered. The agent launches with what they found, or the
/// waiting work fails with their error; during shutdown it fails as closed.
pub(super) fn on_prepared(
    actor: &mut Actor,
    thread: ThreadId,
    id: u64,
    result: Result<Box<Prepared>, Error>,
) {
    let Some(state) = state(actor, thread) else {
        return;
    };
    let Some(preparing) = state.preparing.take_if(|preparing| preparing.id == id) else {
        return;
    };
    let waiting = preparing.waiting;
    match result {
        Ok(_) if host_stopping(actor) => agents::fail_all(actor, thread, waiting, &Error::Closed),
        Ok(prepared) => match super::profile_loads::launch_keys(actor, thread, *prepared) {
            Ok(prepared) => agents::launch_now(actor, thread, waiting, prepared),
            Err(error) => agents::fail_all(actor, thread, waiting, &error),
        },
        Err(error) => agents::fail_all(actor, thread, waiting, &error),
    }
}

/// Removes `thread`'s run folder off the actor, once its agent has ended. The notes folder
/// stays.
pub(super) fn remove_run(actor: &mut Actor, thread: ThreadId) {
    let Some(run) = state(actor, thread).and_then(|state| state.run.take()) else {
        return;
    };
    let clock = std::sync::Arc::clone(&actor.clock);
    let spawned = actor.spawner.spawn_local(async move {
        let removed = codex::prepare::off_actor(clock, move || {
            remove_tree(&run.root);
            Ok(())
        })
        .await;
        if let Err(error) = removed {
            tracing::warn!(%thread, %error, "a session's run folder was not removed");
        }
    });
    if let Err(error) = spawned {
        tracing::warn!(%thread, %error, "a session's run folder was not removed");
    }
}
