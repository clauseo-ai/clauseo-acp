//! An agent's launch, in two phases so no file system call runs on the actor. Phase one sends the
//! checks to a `clauseo-codex-fs` thread (`agent/codex/prepare.rs`): for every agent the
//! data-folder refusal (`agent/data_folder.rs`), and for a Codex agent its preparation
//! ([`crate::agent::codex::prepare_launch`]: the matter folder resolved through links, the `.codex`
//! refusal, the folder check, the home made and the home's `config.toml`; for a Claude agent the
//! folder check, `agent/folder_support.rs`, and its login home made, the folder it gets as its
//! login store, with Claude Code's user settings written there, `agent/home.rs`). Then, for an
//! agent with a kind, the matter's state folder (`agent/state_folder.rs`, R17 PR-C): made through
//! no link at `agent-state`, cleaned of the config surfaces, with Clauseo's own `settings.json` or
//! `config.toml` written whole, and for Codex on Clauseo's own login the hard link to the login
//! home's `auth.json` (while reused, made after the sign-in hook answered). The work that waits for
//! the agent is held on the thread's turn state meanwhile. Phase two, when they answer, launches
//! the agent on the actor with what they found ([`agents::launch_now`]), or fails the waiting work.
//!
//! After both refusals, the same thread makes the session's folders ([`RunFolders`]: its
//! stand-in `run/<session id>` folder with `work` and `browser` under it, and the matter's
//! notes folder) and, for an agent with a kind, writes the profile's skills
//! (`ProfileHook::launch_writes`, `clauseo_profile::write_skills`). A skill write that fails is
//! a warning, never a failed launch. The first write's token goes in [`Prepared`], so only that
//! launch's Codex keys carry the skills index; a failed write leaves none. Then the launch's
//! skill packs go into its own plugin under the run folder ([`packs`]). The notes folder
//! stays; the run folder is the launch's own (its id is in the name) and is removed off the
//! actor on every way out: by the prep thread when nobody waits for its answer any more
//! ([`codex::prepare::off_actor_or_undo`]), and otherwise from the actor ([`discard`]) when
//! the answer is for an earlier launch, the host is stopping, the profile keys or the launch
//! fail, or the agent ends ([`remove_run`]). No launch
//! removes another's folder: a prep that came late can only take its own. Shutdown waits for
//! the actor's removals in flight, so the host answers it with the run folders gone.
//!
//! For a session that can get the browser, the same thread checks that the browser entry's
//! sidecars (`clauseo-mcp` and agent-browser) exist, finds its Chrome, and claims its ports and,
//! on macOS and Linux, its socket folder ([`BrowserSession`]); a missing sidecar or a failed
//! claim leaves the browser out (`BrowserNotInstalled`) with a log line, and the launch goes
//! on. The claims ride on [`RunFolders`] and drop with the run folder, off the actor, once the
//! agent's process tree has been killed (or the agent never started). The session's case-law
//! relay and its token file (R24, [`relay`]) are made on the same thread and ride the same way.
//!
//! At the first launch, before any folder is made, `<data>/run` from an earlier run is set
//! aside in one rename and deleted on a thread of its own ([`RunSweep`]). Nothing is made or
//! swept through a link at `<data>/run` or `<data>/agent-state`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_profile::{PacksWritten, SkillWrite, SkillsWritten};
use clauseo_protocol::{AgentId, AgentKind, ThreadId};

use super::super::Actor;
use super::super::input::Input;
use super::agents::{self, Waiting};
use super::{TurnInput, host_stopping, state};

use crate::agent::data_fence::LaunchDeny;
use crate::agent::folder_support::{Answer, FolderCheck};
use crate::agent::login_link::{self, Checked, LoginCheck};
use crate::agent::state_folder::StateLink;
use crate::agent::{codex, data_folder};
use crate::browser::session::BrowserSession;
use crate::config::SessionKey;
use crate::error::Error;
use crate::log_file::Cause;

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
    /// A Claude agent's home, `Locations::agent_home` of its id, made: its `CLAUDE_CONFIG_DIR`
    /// (`agent/home.rs`). `None` for any other agent.
    pub(super) claude_home: Option<PathBuf>,
    /// The folder check's answer at this launch (`agent/folder_support.rs`), when it ran: for a
    /// Codex or a Claude agent. A Claude agent's decides its forced ask (`agent/forced_ask.rs`).
    pub(super) folder: Option<Answer>,
    /// The session's folders, made.
    pub(super) run: RunFolders,
    /// This launch's own token for its first skills write (Claude's plugin), taken by
    /// `profile_loads::launch_keys`; `None` when that write failed or there was none.
    pub(super) skills_written: Option<SkillsWritten>,
    /// The skill packs this launch left out, by id, with the reason, for its notice.
    pub(super) packs_skipped: Vec<(String, PackSkip)>,
}

/// The folders a session gets, all under the data folder, and its browser claims.
#[derive(Debug, Clone)]
pub(crate) struct RunFolders {
    /// The session.
    pub(crate) key: SessionKey,
    /// `<data>/run/<session id>`: the session's own folder, made at the launch and removed
    /// when the agent ends.
    pub(crate) root: PathBuf,
    /// `<data>/agent-state/<matter id>/<agent id>`: the matter's state folder for an agent
    /// with a kind (`agent/state_folder.rs`), its `CLAUDE_CONFIG_DIR` or `CODEX_HOME`, made at
    /// the matter's first launch of the agent and kept; `None` for an agent without a kind.
    pub(crate) state: Option<PathBuf>,
    /// `{work_folder}`, `<root>/work`.
    pub(crate) work: PathBuf,
    /// `<root>/tmp`: a Codex session's `TMPDIR`, the per-session temp folder its shell may
    /// write (inside the run folder's writable root), never the state folder's `tmp`, where
    /// Codex keeps the `arg0` links it puts first on `PATH`.
    pub(crate) tmp: PathBuf,
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
    /// `<root>/skill-packs`: the launch's own plugin of skill packs (decision record 0040),
    /// written by the launch prep only when a pack loaded. R17 passes this to
    /// `PolicyBuilder::read_only_in_state`.
    pub(crate) skill_packs: PathBuf,
    /// What the launch prep wrote into [`Self::skill_packs`]; none when it wrote nothing. This
    /// launch's `session_meta` and `codex_config` take it, and no other launch's.
    pub(crate) packs_written: Option<Arc<PacksWritten>>,
    /// Claude's data folder deny rules (`agent/data_fence.rs`): the data folder's canonical
    /// path, then the path as configured when it differs, and the patterns under it, every
    /// entry that is not this session's own enumerated by the launch prep. Run folders and
    /// matters' notes made after the launch are in no rule (the accepted residual).
    pub(crate) data_deny: LaunchDeny,
    /// `<root>/relay-token`: the case-law relay's token file (R24), written by the launch prep
    /// only when the session's relay opened, and gone with the run folder. R17 passes this to
    /// `PolicyBuilder::read_only_in_state`.
    pub(crate) relay_token: PathBuf,
    /// The session's case law ([`relay::CaseLaw`]): its relay, opened on the launch-prep
    /// thread, or why there is none. Like the browser claims, the relay stops when the last
    /// clone drops in [`discard`], after the agent's tree was killed; its token is revoked
    /// before that, when the agent is stopped and again before the run folder is removed.
    /// Stage C's R17 adds its port to the session's `Loopback::Ports`.
    pub(crate) case_law: relay::CaseLaw,
}

impl RunFolders {
    /// The folders of `key`'s session under `data`. Paths only: nothing is made here.
    pub(crate) fn new(data: &Path, key: SessionKey) -> Self {
        let root = run_dir(data).join(key.id());
        Self {
            key,
            work: root.join("work"),
            tmp: root.join("tmp"),
            browser: root.join("browser"),
            skill_packs: root.join("skill-packs"),
            relay_token: relay::token_file(&root),
            notes: notes_dir(data, key.matter),
            root,
            state: None,
            chrome: None,
            browser_session: None,
            packs_written: None,
            data_deny: LaunchDeny::fixed(vec![data.to_owned()]),
            case_law: relay::CaseLaw::Absent,
        }
    }

    /// These folders with `state`, the agent's state folder for the matter.
    pub(crate) fn with_state(mut self, state: Option<PathBuf>) -> Self {
        self.state = state;
        self
    }

    /// Makes them, each through no link below `data`. Before that, the host's first launch sets
    /// aside the `run` folder an earlier run left ([`RunSweep`]). When one cannot be made, the
    /// run folder is taken back. File system calls: off the actor.
    pub(crate) fn make(&self, data: &Path, sweep: &RunSweep) -> Result<(), Error> {
        sweep.once(data);
        for folder in [&self.work, &self.browser, &self.tmp, &self.notes] {
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
            tracing::warn!("not a run folder the host made; it stays");
            return;
        }
        if crate::write::is_real_dir(&self.root) {
            remove_tree(&self.root);
        }
    }
}

/// What a Codex launch's preparation found.
#[derive(Debug)]
pub(crate) struct CodexPrepared {
    /// The matter folder resolved through links: `_codex/hooks/list` names it.
    pub(super) folder: PathBuf,
    /// The agent's login home, made and checked: where its own `auth.json` is, and what the
    /// sign-in hook is asked about.
    pub(super) home: PathBuf,
    /// The matter's state folder (`agent/state_folder.rs`), made, cleaned and with its
    /// `config.toml`: the launch's `CODEX_HOME` (and `HOME` on macOS and Linux).
    pub(super) state: PathBuf,
    /// The launch profile's `CODEX_CONFIG` keys, filled on the actor
    /// (`profile_loads::launch_keys`).
    pub(super) keys: serde_json::Map<String, serde_json::Value>,
    /// What the sign-in hook found for the home (`agent/login_link.rs`), asked after the
    /// rest succeeded; `None` with no hook set.
    pub(super) login: Option<Checked>,
    /// The state folder's `auth.json` link (`state_folder::link_own`, or `link_reused` once the
    /// hook found the reused login's link holding); `None` when it was not tried (reused, and
    /// the hook found no shared login: the split check refuses the launch then).
    pub(super) link: Option<Result<StateLink, Error>>,
}

/// Phase one: the checks go to their thread, and `waiting` waits on the thread's state. With
/// `keys_wait` (the agent's waited-out key refusal, `sign_in::keys_wait`) they start once
/// it has resolved.
pub(super) fn prepare(
    actor: &mut Actor,
    thread: ThreadId,
    waiting: Vec<Waiting>,
    keys_wait: Option<impl std::future::Future<Output = ()> + Send + 'static>,
) {
    let id = agents::next_agent_id();
    let paths = launch_paths(actor, thread, id);
    let sweep = actor.turn_path.run_sweep.clone();
    let login = login_hook(actor, thread);
    let reused = reused_login(actor, thread);
    let gated = actor.wiring.launch_gate.is_some();
    let gate_agent = actor
        .threads
        .get(&thread)
        .map(|entry| entry.row.agent.clone());
    let state_use = match state_use(actor, thread) {
        Ok(state_use) => state_use,
        Err(error) => {
            agents::fail_all(actor, thread, waiting, &error);
            return;
        }
    };
    let LaunchPaths {
        stored,
        data,
        home,
        claude,
        mut run,
        skills,
        packs,
        claims,
    } = match paths {
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
    let check = FolderCheck {
        matter: Some(run.key.matter),
        source: actor.folder_check,
    };
    let clock = std::sync::Arc::clone(&actor.clock);
    let check_clock = std::sync::Arc::clone(&actor.clock);
    let matter = run.key.matter;
    actor.spawn_post(async move {
        if let Some(keys_wait) = keys_wait {
            keys_wait.await;
        }
        let result = codex::prepare::off_actor_or_undo(
            clock,
            job,
            move || {
                data_folder::refuse_data_folder(&stored, &data)?;
                // The working folder the agent starts in, as spawn takes it (`session_folder`).
                crate::matter_worker::refuse_too_deep(&crate::agent::session_folder(&stored))?;
                // Fail closed: a launch with a state folder shares the one login, and with no
                // identity check wired (`HostConfig::with_launch_gate`, #554) it may not start.
                if run.state.is_some() && !gated {
                    refuse_ungated(gate_agent.as_ref())?;
                }
                let mut answer = None;
                let (codex_home, claude_home) = if claude { (None, home) } else { (home, None) };
                let codex = match (codex_home, run.state.clone()) {
                    (Some(home), Some(state)) => {
                        let (prepared, support) =
                            prepare_codex(&check, &stored, &data, home, state, reused, state_use)?;
                        answer = Some(support);
                        Some(prepared)
                    }
                    _ => None,
                };
                // Claude is never refused for its folder: the answer decides its forced ask.
                if let Some(home) = &claude_home {
                    answer = Some(check.run(&crate::agent::session_folder(&stored)));
                    prepare_claude(&data, home, run.state.as_deref(), state_use.in_use)?;
                }
                run.make(&data, &sweep)?;
                let claude_state = run.state.clone().filter(|_| claude);
                run.data_deny = launch_deny(&run, &stored, &data, claude_state.as_deref());
                claim(&mut run, &data, claims);
                let skills_written = write_skills(&skills);
                let packs_skipped;
                (run.packs_written, packs_skipped) = packs.run(&run.skill_packs);
                Ok(Prepared {
                    stored,
                    codex,
                    claude_home,
                    folder: answer,
                    run,
                    skills_written,
                    packs_skipped,
                })
            },
            undo_late,
        )
        .await;
        let result = match (result, login) {
            (Ok(mut prepared), Some((hook, agent))) => {
                if let Some(codex) = prepared.codex.as_mut() {
                    let home = codex.home.clone();
                    let checked =
                        login_link::check(Arc::clone(&check_clock), matter, hook, agent, home)
                            .await;
                    codex.login = Some(checked);
                }
                Ok(prepared)
            }
            (result, _) => result,
        };
        let result = match result {
            Ok(mut prepared) if reused => {
                if let Some(codex) = prepared.codex.as_mut() {
                    link_reused_after(check_clock, matter, codex).await;
                }
                Ok(prepared)
            }
            result => result,
        };
        let result = result.map(Box::new);
        Input::Turn(TurnInput::Prepared { thread, id, result })
    });
}

/// The refusal of a launch with a state folder when no launch gate is wired: `agent.launch_failed`,
/// logged with the agent.
fn refuse_ungated(agent: Option<&AgentId>) -> Result<(), Error> {
    let agent = agent.map(AgentId::as_str).unwrap_or_default();
    tracing::warn!(agent, "no identity check is wired; the launch is refused");
    Err(Error::refused(
        clauseo_protocol::ErrorCode::AgentLaunchFailed,
        "Clauseo can't start the agent: no sign-in check is set up.",
    ))
}

/// The session's browser claims and case-law relay (`claims`), made on the launch-prep thread.
fn claim(run: &mut RunFolders, data: &Path, claims: (browser::BrowserPrep, relay::CaseLawPrep)) {
    let (browser, case_law) = claims;
    (run.chrome, run.browser_session) = browser.prepare(data, &run.key.id());
    run.case_law = case_law.open(run);
}

/// A prep nobody waits for any more takes back what it made: the relay's token first, then the
/// run folder with the token file.
fn undo_late(late: Prepared) {
    let Prepared { run, .. } = late;
    relay::end(&run);
    run.remove();
    // Its claims and relay go now, after the folder.
    drop(run);
}

/// The sign-in hook and the agent it asks about, for a Codex thread when a hook is set.
fn login_hook(
    actor: &Actor,
    thread: ThreadId,
) -> Option<(Arc<dyn LoginCheck + Send + Sync>, AgentId)> {
    let hook = actor.wiring.sign_in.check.clone()?;
    let agent = actor.threads.get(&thread)?.row.agent.clone();
    (AgentKind::of(&agent) == Some(AgentKind::Codex)).then_some((hook, agent))
}

/// A launch's folder check answered (`agent/folder_support.rs`, a Codex or a Claude agent):
/// its answer goes to the matter's row, and so does a refusal it made. Any other agent, and a
/// launch that failed before or without the check's refusal, leaves the row as it was.
fn remember_folder_support(
    actor: &mut Actor,
    thread: ThreadId,
    result: &Result<Box<Prepared>, Error>,
) {
    let support = match result {
        Ok(prepared) => match prepared.folder {
            Some(answer) => answer.row_kind(),
            None => return,
        },
        Err(error) => match crate::agent::folder_support::of_refusal(error.code()) {
            Some(kind) => Some(kind),
            None => return,
        },
    };
    let Some(matter) = actor.threads.get(&thread).map(|entry| entry.row.matter) else {
        return;
    };
    crate::actor::query::on_folder_support(actor, matter, support);
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
                error = %Cause(&error),
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
    remember_folder_support(actor, thread, &result);
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
    // A refusal that came, or stayed, while the checks ran (a key refusal waited out
    // included): nothing launches.
    if let Some(error) = super::sign_in::launch_refusal(actor, thread) {
        discard(actor, thread, prepared.run);
        agents::fail_all(actor, thread, waiting, &error);
        return;
    }
    if let Err(error) = super::sign_in::check_launch(actor, thread, &prepared) {
        discard(actor, thread, prepared.run);
        agents::fail_all(actor, thread, waiting, &error);
        return;
    }
    if let Err(error) = super::profile_loads::launch_keys(actor, thread, &mut prepared) {
        discard(actor, thread, prepared.run);
        agents::fail_all(actor, thread, waiting, &error);
        return;
    }
    let run = prepared.run.clone();
    let packs_skipped = std::mem::take(&mut prepared.packs_skipped);
    if let Some(state) = state(actor, thread) {
        state.run = Some(run.clone());
    }
    agents::launch_now(actor, thread, waiting, *prepared);
    let launched = state(actor, thread).is_some_and(|state| state.agent.is_some());
    if launched {
        packs::hold_notice(actor, thread, packs_skipped);
    } else {
        if let Some(state) = state(actor, thread) {
            state.run = None;
        }
        discard(actor, thread, run);
    }
}

/// Removes `run` (`thread`'s launch's run folder) off the actor and drops its browser claims
/// there; the notes folder stays. The session has ended (its agent's tree killed) or never
/// started, so its browser slot, if it held one, is free. The removal counts as work in flight
/// ([`super::super::Work::run_removals`]) until its [`TurnInput::RunRemoved`], so shutdown
/// answers only once the folder and the claims are gone (or the removal's wait has passed).
fn discard(actor: &mut Actor, thread: ThreadId, run: RunFolders) {
    actor.wiring.browser_slots.release(run.key);
    let clock = std::sync::Arc::clone(&actor.clock);
    let job = codex::prepare::Job::short("run folder removal", Some(run.key.matter));
    actor.work.run_removals += 1;
    let hook = actor.wiring.run_removal_hook.clone();
    actor.spawn_post(async move {
        let removed = codex::prepare::off_actor(clock, job, move || {
            // The token is revoked before its file goes with the folder.
            relay::end(&run);
            if let Some(hook) = &hook {
                hook(&run.root);
            }
            run.remove();
            // The browser claims and the case-law relay go here, off the actor and after the
            // agent's tree was killed: the socket folder is removed, the relay stops listening
            // and the ports are free for the next session.
            drop(run);
            Ok(())
        })
        .await;
        if let Err(error) = removed {
            tracing::warn!(
                %thread,
                error = %Cause(&error),
                "a session's run folder was not removed"
            );
        }
        Input::Turn(TurnInput::RunRemoved)
    });
}

/// A run folder's removal ended: shutdown no longer waits for it.
pub(super) fn on_run_removed(actor: &mut Actor) {
    actor.work.run_removals = actor.work.run_removals.saturating_sub(1);
}

/// Removes `thread`'s run folder off the actor, once its agent has ended or failed to launch.
/// The notes folder stays.
pub(super) fn remove_run(actor: &mut Actor, thread: ThreadId) {
    if let Some(run) = state(actor, thread).and_then(|state| state.run.take()) {
        discard(actor, thread, run);
    }
}

mod browser;
pub(super) mod relay;
mod sweep;
pub(crate) use sweep::RunSweep;
mod packs;
pub use packs::PackRequest;
use packs::PackSkip;
pub(super) use packs::{PackNotices, pack_notices};
mod folders;
use folders::{launch_deny, notes_dir, remove_tree, run_dir};
mod paths;
use paths::{LaunchPaths, launch_paths};
mod state_prep;
use state_prep::{link_reused_after, prepare_claude, prepare_codex, reused_login, state_use};

#[cfg(test)]
mod tests;
