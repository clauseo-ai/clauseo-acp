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
//! notes folder) and, for an agent with a kind, writes the profile's skills
//! (`ProfileHook::launch_writes`, `clauseo_profile::write_skills`). A skill write that fails is
//! a warning, never a failed launch. The first write's token goes in [`Prepared`], so only that
//! launch's Codex keys carry the skills index; a failed write leaves none. The notes folder
//! stays; the run folder is the launch's own (its id is in the name) and is removed off the
//! actor on every way out: by the prep thread when nobody waits for its answer any more
//! ([`codex::prepare::off_actor_or_undo`]), and otherwise from the actor ([`discard`]) when
//! the answer is for an earlier launch, the host is stopping, the profile keys or the launch
//! fail, or the agent ends ([`remove_run`]). No launch
//! removes another's folder: a prep that came late can only take its own.
//!
//! For a session that can get the browser, the same thread checks that the browser entry's
//! sidecars (`clauseo-mcp` and agent-browser) exist, finds its Chrome, and claims its ports and,
//! on macOS and Linux, its socket folder ([`BrowserSession`]); a missing sidecar or a failed
//! claim leaves the browser out (`BrowserNotInstalled`) with a log line, and the launch goes
//! on. The claims ride on [`RunFolders`] and drop with the run folder, off the actor, once the
//! agent's process tree has been killed (or the agent never started).
//!
//! At the first launch, before any folder is made, `<data>/run` from an earlier run is set
//! aside in one rename and deleted on a thread of its own ([`RunSweep`]). Nothing is made or
//! swept through a link at `<data>/run` or `<data>/agent-state`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_profile::{SkillWrite, SkillsWritten};
use clauseo_protocol::{AgentKind, MatterId, ThreadId};

use super::super::Actor;
use super::super::input::Input;
use super::agents::{self, Waiting};
use super::{TurnInput, host_stopping, state};
use futures::task::LocalSpawnExt;

use crate::agent::{codex, data_folder};
use crate::browser::session::BrowserSession;
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
    /// This launch's own token for its first skills write (Claude's plugin), taken by
    /// `profile_loads::launch_keys`; `None` when that write failed or there was none.
    pub(super) skills_written: Option<SkillsWritten>,
}

/// The folders a session gets, all under the data folder, and its browser claims.
#[derive(Debug, Clone)]
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
    /// `{browser_executable}`: the Chrome the launch prep found for this session
    /// (`browser::chrome::find`), or none (`BrowserNotInstalled`). Not a folder, but the
    /// session's like them, and found on the same thread.
    pub(crate) chrome: Option<PathBuf>,
    /// The session's browser ports, socket folder and agent-browser name
    /// ([`BrowserSession`]), claimed on the launch-prep thread; none when the session cannot
    /// get the browser. The one holder is the thread's turn state; clones elsewhere are
    /// short-lived, so the last reference drops in [`discard`]'s off-actor removal, after the
    /// agent's tree was killed. Stage C's R17 builds the session's jail policy from it:
    /// `PolicyBuilder::browser_socket_dir(&session.socket_dir)` and
    /// `Loopback::Ports(session.ports.loopback())`.
    pub(crate) browser_session: Option<Arc<BrowserSession>>,
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
            chrome: None,
            browser_session: None,
        }
    }

    /// Makes them, each through no link below `data`. Before that, the host's first launch sets
    /// aside the `run` folder an earlier run left ([`RunSweep`]). When one cannot be made, the
    /// run folder is taken back. File system calls: off the actor.
    pub(crate) fn make(&self, data: &Path, sweep: &RunSweep) -> Result<(), Error> {
        sweep.once(data);
        for folder in [&self.work, &self.browser, &self.notes] {
            if let Err(error) = crate::write::create_private_dirs_under(data, folder) {
                self.remove();
                return Err(Error::refused(
                    clauseo_protocol::ErrorCode::AgentLaunchFailed,
                    format!("cannot make the folder {}: {error}", folder.display()),
                ));
            }
        }
        // The notes folder may be an earlier session's: it is set to 0700 whatever it was.
        #[cfg(unix)]
        if let Err(error) = crate::write::make_private_dir(&self.notes) {
            self.remove();
            return Err(Error::refused(
                clauseo_protocol::ErrorCode::AgentLaunchFailed,
                format!(
                    "cannot make the folder {} private: {error}",
                    self.notes.display()
                ),
            ));
        }
        Ok(())
    }

    /// Removes this launch's run folder and all it holds, when it is a real folder; the notes
    /// folder stays. Only the folder the host made: the path as built (`<data>/run/<session id>`),
    /// never canonicalized, removed with `remove_dir_all`, which removes a link inside and never
    /// follows it. Called once the agent's process has exited, or when it never started. File
    /// system calls: off the actor.
    pub(crate) fn remove(&self) {
        let ours = self.root.file_name() == Some(std::ffi::OsStr::new(&self.key.id()))
            && self.root.parent().and_then(Path::file_name) == Some(std::ffi::OsStr::new("run"));
        if !ours {
            tracing::warn!(folder = %self.root.display(), "not a run folder the host made; it stays");
            return;
        }
        if crate::write::is_real_dir(&self.root) {
            remove_tree(&self.root);
        }
    }
}

/// The start-up sweep of `<data>/run`, once per host start: no launch is live before the
/// first, so every folder there is an earlier run's leftover. The first launch-prep thread, under
/// a lock every launch's prep takes before making its folders, renames `run` to
/// `run.old-<n>` in one call, so no live folder is taken and no launch waits on a delete; a
/// `clauseo-run-sweep` thread then deletes every `run.old-*` folder. A `run` that is a link
/// is left as it is, and the launches refuse it.
#[derive(Debug, Clone, Default)]
pub(crate) struct RunSweep(std::sync::Arc<std::sync::Mutex<Swept>>);

/// Whether the start-up sweep ran, and (for the tests) its delete's thread.
#[derive(Debug, Default)]
struct Swept {
    done: bool,
    #[cfg(test)]
    delete: Option<std::thread::JoinHandle<()>>,
}

impl RunSweep {
    /// Sets `<data>/run` aside the first time and starts its delete; nothing after.
    pub(crate) fn once(&self, data: &Path) {
        let mut swept = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if swept.done {
            return;
        }
        swept.done = true;
        set_aside(data);
        let data = data.to_owned();
        let started = std::thread::Builder::new()
            .name("clauseo-run-sweep".to_owned())
            .spawn(move || delete_set_aside(&data));
        match started {
            #[cfg(test)]
            Ok(delete) => swept.delete = Some(delete),
            #[cfg(not(test))]
            Ok(_delete) => {}
            Err(error) => {
                tracing::warn!(%error, "the earlier run's folders stay until the next start");
            }
        }
    }

    /// Waits for the start-up sweep's delete, when it started.
    #[cfg(test)]
    pub(crate) fn join_delete(&self) {
        let delete = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .delete
            .take();
        if let Some(delete) = delete {
            delete.join().unwrap();
        }
    }
}

/// The prefix of a `run` folder set aside.
const SET_ASIDE: &str = "run.old-";

/// Renames `<data>/run` to the first free `run.old-<n>`, when it is a real folder.
fn set_aside(data: &Path) {
    let run = run_dir(data);
    match std::fs::symlink_metadata(&run) {
        Err(_) => return,
        Ok(meta) if !meta.is_dir() => {
            tracing::warn!(folder = %run.display(), "the run folder is a link or no folder; left as is");
            return;
        }
        Ok(_) => {}
    }
    let free = (0..1000_u32)
        .map(|n| data.join(format!("{SET_ASIDE}{n}")))
        .find(|old| std::fs::symlink_metadata(old).is_err());
    let renamed = match free {
        Some(old) => crate::write::rename_folder(&run, &old),
        None => Err(std::io::Error::other("no free run.old-<n> name")),
    };
    if let Err(error) = renamed {
        tracing::warn!(folder = %run.display(), %error, "the earlier run's folders stay");
    }
}

/// Deletes every real `run.old-*` folder under `data`; a link of that name is left alone.
fn delete_set_aside(data: &Path) {
    let Ok(entries) = std::fs::read_dir(data) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let named = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(SET_ASIDE));
        if named && crate::write::is_real_dir(&path) {
            remove_tree(&path);
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
        let skills = match kind {
            Some(_) => actor.profile.launch_writes(&row.agent),
            None => Vec::new(),
        };
        let browser = browser_prep(actor, &row.agent, key);
        Ok((
            stored,
            locations.data_dir().to_owned(),
            home,
            run,
            skills,
            browser,
        ))
    })();
    let sweep = actor.run_sweep.clone();
    let (stored, data, home, mut run, skills, browser) = match paths {
        Ok(paths) => paths,
        Err(error) => {
            agents::fail_all(actor, thread, waiting, &error);
            return;
        }
    };
    let job = codex::prepare::Job::launch(run.key.matter);
    let Some(state) = state(actor, thread) else {
        return;
    };
    state.preparing = Some(Box::new(Preparing { id, waiting }));
    let clock = std::sync::Arc::clone(&actor.clock);
    actor.spawn_post(async move {
        let result = codex::prepare::off_actor_or_undo(
            clock,
            job,
            move || {
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
                (run.chrome, run.browser_session) = browser.prepare(&data, &run.key.id());
                let skills_written = write_skills(&skills);
                Ok(Prepared {
                    stored,
                    codex,
                    run,
                    skills_written,
                })
            },
            |late: Prepared| late.run.remove(),
        )
        .await;
        let result = result.map(Box::new);
        Input::Turn(TurnInput::Prepared { thread, id, result })
    });
}

/// Writes `writes` in order, on the launch-prep thread; a failure is a warning. The first
/// write's token, when it succeeded: the one the launch's Codex keys may take the index from.
fn write_skills(writes: &[SkillWrite]) -> Option<SkillsWritten> {
    let mut first = None;
    for (at, write) in writes.iter().enumerate() {
        match clauseo_profile::write_skills(write) {
            Ok(written) if at == 0 => first = Some(written),
            Ok(_) => {}
            Err(error) => tracing::warn!(
                %error,
                "the profile's skills were not written; the agent starts with what the folder holds"
            ),
        }
    }
    first
}

/// Phase two: the checks `id` answered. The agent launches with what they found, or the
/// waiting work fails with their error; during shutdown it fails as closed. On every way that
/// does not end in a launched agent, the launch's run folder is removed off the actor; an answer
/// for an earlier launch removes only its own.
pub(super) fn on_prepared(
    actor: &mut Actor,
    thread: ThreadId,
    id: u64,
    result: Result<Box<Prepared>, Error>,
) {
    let preparing = state(actor, thread)
        .and_then(|state| state.preparing.take_if(|preparing| preparing.id == id));
    let Some(preparing) = preparing else {
        if let Ok(stale) = result {
            discard(actor, thread, stale.run);
        }
        return;
    };
    let waiting = preparing.waiting;
    let mut prepared = match result {
        Ok(prepared) => prepared,
        Err(error) => {
            agents::fail_all(actor, thread, waiting, &error);
            return;
        }
    };
    if host_stopping(actor) {
        discard(actor, thread, prepared.run);
        agents::fail_all(actor, thread, waiting, &Error::Closed);
        return;
    }
    if let Err(error) = super::profile_loads::launch_keys(actor, thread, &mut prepared) {
        discard(actor, thread, prepared.run);
        agents::fail_all(actor, thread, waiting, &error);
        return;
    }
    let run = prepared.run.clone();
    if let Some(state) = state(actor, thread) {
        state.run = Some(run.clone());
    }
    agents::launch_now(actor, thread, waiting, *prepared);
    let launched = state(actor, thread).is_some_and(|state| state.agent.is_some());
    if !launched {
        if let Some(state) = state(actor, thread) {
            state.run = None;
        }
        discard(actor, thread, run);
    }
}

/// Removes `run` (`thread`'s launch's run folder) off the actor and drops its browser claims
/// there; the notes folder stays. The session has ended (its agent's tree killed) or never
/// started, so its browser slot, if it held one, is free.
fn discard(actor: &mut Actor, thread: ThreadId, run: RunFolders) {
    actor.wiring.browser_slots.release(run.key);
    let clock = std::sync::Arc::clone(&actor.clock);
    let job = codex::prepare::Job::short("run folder removal", Some(run.key.matter));
    let spawned = actor.spawner.spawn_local(async move {
        let removed = codex::prepare::off_actor(clock, job, move || {
            run.remove();
            // The browser claims go here, off the actor and after the agent's tree was killed:
            // the socket folder is removed and the ports are free for the next session.
            drop(run);
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

/// Removes `thread`'s run folder off the actor, once its agent has ended or failed to launch.
/// The notes folder stays.
pub(super) fn remove_run(actor: &mut Actor, thread: ThreadId) {
    if let Some(run) = state(actor, thread).and_then(|state| state.run.take()) {
        discard(actor, thread, run);
    }
}

mod browser;
use browser::browser_prep;

#[cfg(test)]
mod tests;
