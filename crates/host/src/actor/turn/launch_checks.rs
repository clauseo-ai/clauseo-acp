//! An agent's launch, in two phases so no file system call runs on the actor. Phase one sends
//! the checks to a `clauseo-codex-fs` thread (`agent/codex/prepare.rs`): for every agent the
//! data-folder refusal (`agent/data_folder.rs`), and for a Codex agent its preparation
//! ([`crate::agent::codex::prepare_launch`]: the matter folder resolved through links, the
//! `.codex` refusal, the folder check, the home made and the home's `config.toml`; for a Claude
//! agent the folder check, `agent/folder_support.rs`, and its home made, the folder it gets as
//! `CLAUDE_CONFIG_DIR`, with Claude Code's user settings written there, `agent/home.rs`). The
//! work that waits for the agent is held on the thread's turn state meanwhile. Phase two, when
//! they answer, launches the agent on the actor with what they found ([`agents::launch_now`]), or
//! fails the waiting work.
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
//! agent's process tree and its Chrome are gone ([`straggler`]), or the agent never started.
//! The session's case-law relay and its token file (R24, [`relay`]) are made on the same thread
//! and ride the same way, and so does its egress proxy ([`egress`]), opened there once Chrome
//! is found and closed as the run drops. Last, the same thread fixes the session's `PATH`
//! folders and what its shells get ([`claim`], `agent/shell_path.rs`): Claude's
//! `clauseo-env.sh` in the run folder, and on macOS the folders `path_helper` adds, for Codex.
//! It also reads the keychain values of the lawyer's enabled MCP servers and resolves their
//! programs (`agent::loads::lawyer`).
//!
//! At the first launch, before any folder is made, `<data>/run` from an earlier run is set
//! aside in one rename and deleted on a thread of its own ([`RunSweep`]). Nothing is made or
//! swept through a link at `<data>/run` or `<data>/agent-state`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_process::TreeWatch;
use clauseo_profile::{PacksWritten, SkillWrite, SkillsWritten};
use clauseo_protocol::{AgentId, AgentKind, ThreadId};

use super::super::Actor;
use super::super::input::Input;
use super::agents::{self, Waiting};
use super::offline::probe::settings::{Reads, SettingsRead};
use super::{TurnInput, host_stopping, state};

use crate::agent::codex_fence::Listed;
use crate::agent::data_fence::LaunchDeny;
use crate::agent::folder_support::{Answer, FolderCheck};
use crate::agent::loads::lawyer::Reads as McpReads;
use crate::agent::login_link::{self, Checked, LoginCheck};
use crate::agent::{claude_tmp, codex, data_folder};
use crate::browser::session::BrowserSession;
use crate::config::SessionKey;
use crate::error::Error;
use crate::log_file::{Cause, LIFECYCLE};

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
    /// What the agent's own settings files say ([`SettingsRead`]), read after Claude's home
    /// settings are written, as the agent will read them.
    pub(super) settings: SettingsRead,
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
    /// `{browser_dir}`, the guard's `--cwd`: `<data>/browser/run/<session id>`, outside the
    /// run folder, since Codex's shell may write the run folder and the guard, unfenced, writes
    /// through this one (its downloads, screenshots, home, temporary files and Chrome's profile).
    /// Made by the launch prep only for a session the entry goes to (`browser::entry_folder`),
    /// removed with the run folder ([`Self::remove`]); an earlier run's are swept at start
    /// ([`RunSweep`]).
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
    /// Why the firm's browser policy left the browser out, if it did (`browser::rd_policy`):
    /// `BrowserBlockedByPolicy` or `BrowserPolicyUnreadable`.
    pub(crate) browser_blocked: Option<clauseo_protocol::ToolsUnavailableReason>,
    /// Whether the launch prep read that policy (it does whenever the browser could come).
    pub(crate) browser_policy_read: bool,
    /// The browser as a shell tool (`ProfileHook::browser_tool`, `tools.browser` on): the
    /// session has it once [`Self::chrome`] is found and its two folders below are made.
    pub(crate) browser_shell: bool,
    /// `<root>/agent-browser` (`browser::SESSION_DIR`): the shell tool's own folder, holding
    /// `config.json` (`AGENT_BROWSER_CONFIG`), `downloads` (`AGENT_BROWSER_DOWNLOAD_PATH`) and
    /// `profile` (`AGENT_BROWSER_PROFILE`): made only for the shell tool, gone with the run
    /// folder. Inside the run folder, so the data fences reopen it for this session only.
    pub(crate) agent_browser: PathBuf,
    pub(crate) browser_config: PathBuf,
    pub(crate) downloads: PathBuf,
    pub(crate) browser_profile: PathBuf,
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
    /// What the launch prep listed for Codex's data fence (`agent::codex_fence::list`): the
    /// siblings of this session's run folder and notes, `run.old-*`, and what of `<data>/agents`
    /// and its runtime folder the shell needs not. Each folder is denied whole until then.
    pub(crate) codex_listed: Listed,
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
    /// The session's egress proxy ([`egress`]), opened on the launch-prep thread when the
    /// session can get the browser and its Chrome and claims were found; none otherwise, or
    /// when it did not open (`NoProxy`). It closes when the last clone drops, normally in
    /// [`discard`]'s off-actor removal once the agent's tree and Chrome are gone. Last of the
    /// fields, so the claims and the relay drop first. Stage C's R17 adds its ports to the
    /// session's policy (`proxy_ports`).
    pub(crate) egress: Option<Arc<egress::EgressGuard>>,
    /// The run's lease on its claims, relay and proxy ([`straggler::Lease`]), shared by its
    /// clones: they are freed only after a completed wait, and held until the host stops on
    /// any other drop.
    pub(crate) lease: Arc<straggler::Lease>,
    /// Whether the launch prep waited for a browser slot (`browser::slot_wait`).
    pub(crate) slot_waited: bool,
    /// Its `PATH` folders and what its shells get, found by the launch prep after the claims
    /// (`claim.rs`, `agent/shell_path.rs`); `<root>/clauseo-env.sh` goes with the folder.
    pub(crate) path: crate::agent::shell_path::SessionPath,
    /// Claude's short temp folder (`agent::claude_tmp`): set for a Claude session off Windows,
    /// and none again when the launch prep could not make it (its sandbox is then off).
    pub(crate) claude_tmp: Option<PathBuf>,
    /// The keychain values of the lawyer's enabled MCP servers, read by the launch prep
    /// (`agent::loads::lawyer`); each session call of the run fills its servers from them.
    pub(crate) mcp_secrets: Arc<crate::agent::loads::lawyer::McpSecrets>,
    /// The lawyer's servers of the run, made from those values at launch: what each session call
    /// sends and what the agent's stderr redactor hides (`agent::loads::lawyer::Sent`).
    pub(crate) lawyer: Arc<crate::agent::loads::lawyer::Sent>,
    /// Where the run's Claude session calls write their flag settings (`agent::launch_settings`,
    /// `<data>/launch-settings/<run id>.json`); gone with the run folder.
    pub(crate) launch_settings: crate::agent::launch_settings::Target,
}

impl RunFolders {
    /// The folders of `key`'s session under `data`. Paths only: nothing is made here.
    pub(crate) fn new(data: &Path, key: SessionKey) -> Self {
        let root = run_dir(data).join(key.id());
        let agent_browser = root.join(crate::browser::SESSION_DIR);
        Self {
            key,
            work: root.join("work"),
            browser: browser_run_dir(data).join(key.id()),
            browser_config: agent_browser.join(crate::browser::SESSION_CONFIG),
            downloads: agent_browser.join("downloads"),
            browser_profile: agent_browser.join("profile"),
            agent_browser,
            skill_packs: root.join("skill-packs"),
            relay_token: relay::token_file(&root),
            notes: notes_dir(data, key.matter),
            root,
            chrome: None,
            browser_session: None,
            browser_blocked: None,
            browser_policy_read: false,
            browser_shell: false,
            packs_written: None,
            data_deny: LaunchDeny::fixed(vec![data.to_owned()]),
            codex_listed: Listed::default(),
            case_law: relay::CaseLaw::Absent,
            egress: None,
            lease: Arc::default(),
            slot_waited: false,
            path: crate::agent::shell_path::SessionPath::default(),
            claude_tmp: None,
            mcp_secrets: Arc::default(),
            lawyer: Arc::default(),
            launch_settings: crate::agent::launch_settings::Target {
                folder: crate::agent::launch_settings::folder(data),
                run: key.id(),
            },
        }
    }

    /// Makes them, each through no link below `data`. Before that, the host's first launch sets
    /// aside the `run` folder an earlier run left ([`RunSweep`]). When one cannot be made, the
    /// run folder is taken back. File system calls: off the actor.
    pub(crate) fn make(&self, data: &Path, sweep: &RunSweep) -> Result<(), Error> {
        sweep.once(data);
        let tmp = crate::agent::temp::temp_dir(&self.root);
        for folder in [&self.work, &tmp, &self.notes] {
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

    /// Removes this launch's run folder, its browser folder and Claude's short temp folder,
    /// through no link ([`folders::remove_folders`]); the notes folder stays. File system
    /// calls: off the actor.
    pub(crate) fn remove(&self) {
        folders::remove_folders(self);
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
    /// What the sign-in hook found for the home (`agent/login_link.rs`), asked after the
    /// rest succeeded; `None` with no hook set.
    pub(super) login: Option<Checked>,
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
    tracing::info!(target: LIFECYCLE, %thread, "launch prep started");
    let id = agents::next_agent_id();
    let paths = launch_paths(actor, thread, id);
    let sweep = actor.turn_path.run_sweep.clone();
    let login = login_hook(actor, thread);
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
    let slot_wait = browser::slot_wait(actor, run.key, &claims.browser);
    run.slot_waited = slot_wait.is_some();
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
    let block_list_wait = claims.egress.block_list_wait(Arc::clone(&actor.clock));
    let matter = run.key.matter;
    let (mcp, settings) = (McpReads::of(actor), Reads::of(actor, claude));
    actor.spawn_post(async move {
        if let Some(keys_wait) = keys_wait {
            keys_wait.await;
        }
        // Before anything is made for the launch: a folder that is gone, or does not answer,
        // is `matter.folder_missing`, not a spawn that fails in it (`folder.rs`).
        let there = folder::check(Arc::clone(&clock), matter, stored.clone()).await;
        if let Err(error) = there {
            return Input::Turn(TurnInput::Prepared {
                thread,
                id,
                result: Err(error),
            });
        }
        let slot = browser::slot_then_block_list(slot_wait, block_list_wait).await;
        let result = codex::prepare::off_actor_or_undo(
            clock,
            job,
            move || {
                data_folder::refuse_data_folder(&stored, &data)?;
                // The working folder the agent starts in, as spawn takes it (`session_folder`).
                crate::matter_worker::refuse_too_deep(&crate::agent::session_folder(&stored))?;
                let mut answer = None;
                let (codex_home, claude_home) = if claude { (None, home) } else { (home, None) };
                let codex = match codex_home {
                    Some(home) => {
                        let (folder, support) =
                            codex::prepare_launch(&check, &stored, &data, &home)?;
                        answer = Some(support);
                        Some(CodexPrepared {
                            folder,
                            home,
                            keys: serde_json::Map::new(),
                            login: None,
                        })
                    }
                    None => None,
                };
                // Claude is never refused for its folder: the answer decides its forced ask.
                if let Some(home) = &claude_home {
                    answer = Some(check.run(&crate::agent::session_folder(&stored)));
                    codex::make_agent_home(&data, home, "Claude Code")?;
                    if let Err(error) = crate::agent::home::write_claude_settings(home) {
                        tracing::warn!(
                            error = %Cause(&error),
                            "Claude's home settings were not written"
                        );
                    }
                }
                run.make(&data, &sweep)?;
                let short = run.claude_tmp.take();
                run.claude_tmp = short.filter(|dir| claude_tmp::make_claude_tmp(&run.root, dir));
                run.data_deny = launch_deny(&run, &stored, &data, claude_home.as_deref());
                run.codex_listed = crate::agent::codex_fence::list(&data, &run.root, &run.notes);
                claim(&mut run, &data, claims, slot);
                run.mcp_secrets = Arc::new(mcp.read());
                let skills_written = write_skills(&skills);
                let packs_skipped;
                (run.packs_written, packs_skipped) = packs.run(&run.skill_packs);
                Ok(Prepared {
                    settings: settings.read(codex.as_ref(), claude_home.as_deref()),
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
                    let checked = login_link::check(check_clock, matter, hook, agent, home).await;
                    codex.login = Some(checked);
                }
                Ok(prepared)
            }
            (result, _) => result,
        };
        let result = result.map(Box::new);
        Input::Turn(TurnInput::Prepared { thread, id, result })
    });
}

/// A prep nobody waits for any more takes back what it made: the relay's token first, then the
/// run folder with the token file. No agent was started, so nothing waits.
fn undo_late(late: Prepared) {
    let Prepared { run, .. } = late;
    // Nothing to wait for: no agent was started, so its parts are freed as it drops.
    run.lease.wait_done();
    relay::end(&run);
    run.remove();
    // Its claims, relay and proxy go now, after the folder.
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
            discard(actor, thread, stale.run, None);
        }
        return;
    };
    let waiting = preparing.waiting;
    tracing::info!(target: LIFECYCLE, %thread, ok = result.is_ok(), "launch prep done");
    remember_folder_support(actor, thread, &result);
    let mut prepared = match result {
        Ok(prepared) => prepared,
        Err(error) => {
            agents::fail_all(actor, thread, waiting, &error);
            return;
        }
    };
    if host_stopping(actor) {
        discard(actor, thread, prepared.run, None);
        agents::fail_all(actor, thread, waiting, &Error::Closed);
        return;
    }
    browser::warn_if_still_full(actor, &prepared.run);
    // A refusal that came, or stayed, while the checks ran (a key refusal waited out
    // included): nothing launches.
    if let Some(error) = super::sign_in::launch_refusal(actor, thread) {
        discard(actor, thread, prepared.run, None);
        agents::fail_all(actor, thread, waiting, &error);
        return;
    }
    if let Err(error) = super::sign_in::check_launch(actor, thread, &prepared) {
        discard(actor, thread, prepared.run, None);
        agents::fail_all(actor, thread, waiting, &error);
        return;
    }
    if let Err(error) = super::profile_loads::launch_keys(actor, thread, &mut prepared) {
        discard(actor, thread, prepared.run, None);
        agents::fail_all(actor, thread, waiting, &error);
        return;
    }
    prepared.run.lawyer = Arc::new(super::profile_loads::lawyer_servers(actor, &prepared.run));
    let run = prepared.run.clone();
    let packs_skipped = std::mem::take(&mut prepared.packs_skipped);
    if let Some(state) = state(actor, thread) {
        state.run = Some(run.clone());
    }
    let failed_tree = agents::launch_now(actor, thread, waiting, *prepared);
    let launched = state(actor, thread).is_some_and(|state| state.agent.is_some());
    if launched {
        packs::hold_notice(actor, thread, packs_skipped);
    } else {
        if let Some(state) = state(actor, thread) {
            state.run = None;
        }
        // A process that was started and then failed is waited for like any other.
        discard(actor, thread, run, failed_tree);
    }
}

/// Removes `thread`'s run folder off the actor, once its agent (whose tree `tree` watches)
/// has ended. The notes folder stays.
pub(super) fn remove_run(actor: &mut Actor, thread: ThreadId, tree: TreeWatch) {
    if let Some(run) = state(actor, thread).and_then(|state| state.run.take()) {
        discard(actor, thread, run, Some(tree));
    }
}

mod browser;
mod claim;
use claim::claim;
mod egress;
use paths::Claims;
pub(super) mod folder;
pub(super) mod relay;
mod straggler;
pub(crate) use straggler::HeldRuns;
mod removal;
use removal::discard;
pub(super) use removal::on_run_removed;
mod sweep;
pub(crate) use sweep::RunSweep;
mod packs;
pub use packs::PackRequest;
use packs::PackSkip;
pub(super) use packs::{PackNotices, pack_notices};
mod folders;
use folders::{browser_run_dir, launch_deny, notes_dir, remove_tree, run_dir};
mod paths;
use paths::{LaunchPaths, launch_paths};

#[cfg(test)]
mod tests;
