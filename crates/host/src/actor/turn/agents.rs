//! A thread's agent: its launch (step 2: launch, `initialize`, `session/load` or `session/new`,
//! the attach commit) and the work that waits for its session. Its stop and its end are in
//! `agents/ended.rs`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use clauseo_acp::{AcpEvent, AgentInfo, MessageRole, PendingKey};
use clauseo_protocol::{
    AgentChoiceId, AgentSessionId, CompactString, ConfigValue, ElicitationId, ErrorCode, MessageId,
    PermissionRequestId, ThreadId, TurnId,
};

use super::super::Actor;
use super::super::commands::CommandCtx;
use super::super::input::Input;
use super::key_retry::KeyRetry;
use super::launch_checks;
use super::{
    AgentReply, TurnInput, TurnPhase, current, end, host_stopping, lost, offline, session, state,
    steps,
};
use crate::agent::{self, AgentLink};
use crate::error::Error;
use crate::log_file::{Cause, LIFECYCLE};
use crate::rate::{RateCap, RatedUpdate};
use crate::sent::SentView;

mod ended;
mod launch;
mod launch_now;
mod path;
mod start_failure;
pub(super) mod start_timer;

use ended::{StopCause, stop_closed};
pub(super) use ended::{hold_stop, idle_stop, log_detach, on_ended, stop};
use launch::launch;
pub(super) use launch_now::launch_now;
pub(super) use start_failure::launch_failed;

/// Work that waits for the agent's session.
#[derive(Debug)]
pub(super) enum Waiting {
    /// A turn at step 2.
    Turn(TurnId),
    /// `SetConfigOption`.
    Config(CommandCtx, AgentChoiceId, ConfigValue),
    /// `SetMode`.
    Mode(CommandCtx, AgentChoiceId),
}

/// Where an agent is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    /// `initialize` is in flight.
    Initializing,
    /// The replay read or `session/load` or `session/new` is in flight.
    Loading,
    /// Its session is attached.
    Attached,
    /// It was attached and a call to it came back `Closed`, so `Ended` is due: work that comes
    /// first (Windows reports an exit late) waits for a new agent (`stop_closed`).
    Closed,
    /// The shutdown ladder runs; `Ended` follows.
    Stopping,
}

/// A thread's agent as the actor holds it.
#[derive(Debug)]
pub(super) struct Agent {
    /// Unique for the host's run: a call's result names the agent it went to.
    pub(super) id: u64,
    pub(super) link: AgentLink,
    pub(super) stage: Stage,
    pub(super) info: Option<AgentInfo>,
    pub(super) waiting: Vec<Waiting>,
    /// Work that came while it was stopping: a new agent takes it after `Ended`.
    pub(super) after_stop: Vec<Waiting>,
    /// The matter folder it was started in, its working directory.
    pub(super) folder: PathBuf,
    /// Where its shell calls start: the matter's folder, or unknown when Claude Code's policy
    /// settings may keep the folder from one call to the next (`SettingsRead::shell_carries`).
    pub(super) shell_start: crate::sent::ShellStart,
    /// Why it is being stopped, which picks the detach a stop logs.
    stop_cause: StopCause,
    /// The attached session.
    pub(super) session: Option<AgentSessionId>,
    pub(super) rate: RateCap<RatedUpdate>,
    pub(super) rate_armed: bool,
    pub(super) permissions: BTreeMap<PermissionRequestId, PendingKey>,
    pub(super) elicitations: BTreeMap<ElicitationId, PendingKey>,
    /// Pending URL elicitations by the agent's elicitation id.
    pub(super) url_elicitations: BTreeMap<CompactString, ElicitationId>,
    /// Message ids by ACP message id in this session, for text not durable yet.
    pub(super) messages: BTreeMap<CompactString, MessageId>,
    /// The role and id of the last chunk run: a chunk without an id continues it while the
    /// role stays and no other event comes between.
    pub(super) run: Option<(MessageRole, MessageId)>,
    /// Compaction summary text per compaction.
    pub(super) compaction: BTreeMap<CompactString, String>,
    /// The thread's whole log folded, for replay dedupe while `session/load` runs.
    pub(super) replay: Option<SentView>,
    /// Replayed updates, and options updates while the session starts, held until the session
    /// is attached.
    pub(super) replayed: Vec<AcpEvent>,
    /// A Codex session's checks, or a Claude session's switch to Clauseo's mode, run before its
    /// attach: every update is held with the replayed ones meanwhile, so none is logged before
    /// `AgentSessionAttached`.
    pub(super) preparing: bool,
    /// What a Codex agent's launch recorded; `None` for any other agent.
    pub(super) codex: Option<CodexAgent>,
    /// The service and route the host's offline probe asks, from its launch's final environment
    /// (`offline/probe/service.rs`); `None` for an agent of no known kind.
    pub(super) probe: Option<super::offline::probe::Target>,
    /// The programs of the MCP servers its session was given: a cancelled turn's sweep leaves
    /// a process running one of them (`agent/leftovers.rs`).
    pub(super) mcp_commands: Vec<PathBuf>,
    /// A Claude agent whose launch's folder check found a network drive or a drive without
    /// permissions: it runs in forced ask (`agent/forced_ask.rs`).
    pub(super) forced_ask: bool,
    /// The launch's folder check answer, when it ran; read through [`Agent::unprotected`].
    pub(super) folder_check: Option<crate::agent::folder_support::Answer>,
    /// A Claude agent's standing with `auto`, the mode it runs in outside forced ask
    /// (`session/claude_mode.rs`).
    pub(super) auto: session::Auto,
    /// A Claude agent's home, its `CLAUDE_CONFIG_DIR`, whose transcripts confirm a steer
    /// (`queue/confirm.rs`); `None` for any other agent.
    pub(super) claude_home: Option<PathBuf>,
    /// The MCP servers Clauseo itself sent this session that may run unasked
    /// (`agent/mcp_trust.rs`), filled by `profile_loads::session_loads`: the lawyer's trusted
    /// servers, and `case-law` when our entry went. A server Codex got from anywhere else is
    /// never answered for. `None` when the launch's folder check would put Claude in forced ask
    /// (`forced_ask::applies`; for Codex only a failed check, since the other two refuse it):
    /// then none is, so Codex asks where Claude would.
    pub(super) mcp_sent: Option<Vec<String>>,
    /// A Claude agent's plans folder, `<config folder>/plans`, from its launch's environment
    /// (`agent/plan_note.rs`); `None` for any other agent.
    pub(super) plan_notes: Option<PathBuf>,
    /// Its launch variables that hold a path, for the data fence's `$env:NAME`
    /// (`data_fence::launch_vars`).
    pub(super) fence_vars: Vec<(String, String)>,
    /// Its permission requests the host rejected for touching Clauseo's data folder
    /// (`agent/data_fence.rs`), logged as a count on its `agent ended` line.
    pub(super) denied_data_folder: u32,
    /// Its launch's failure code (`launch_failed`); its end then logs its start's stderr summary.
    pub(super) start_failure: Option<ErrorCode>,
    /// Its matter folder's spellings, read at the launch (`start_failure::matter_spellings`).
    pub(super) matter_spellings: Vec<String>,
    /// Its idle clock and config calls in flight (`idle.rs`).
    pub(super) idle: super::idle::Mark,
    /// What of its guard's record file is read and told (`updates/guard_blocks.rs`).
    pub(super) guard_blocks: super::updates::guard_blocks::Reads,
}

impl Agent {
    /// The launch's folder check would put Claude in forced ask (`forced_ask::applies`); for
    /// Codex only a failed check, since the other two refuse it. A Codex agent gets no browser
    /// auto-answer then (`asks.rs` `browser_tool_to_allow`).
    pub(super) fn unprotected(&self) -> bool {
        self.folder_check.is_some_and(agent::forced_ask::applies)
    }
}

/// What a Codex agent's launch recorded.
#[derive(Debug)]
pub(super) struct CodexAgent {
    /// The matter folder as resolved through links at the launch: `_codex/hooks/list` names
    /// it. Each prompt and session call checks the stored folder for `.codex` first, resolved
    /// again.
    pub(super) folder: PathBuf,
    /// Its `CODEX_CONFIG` got a hooks object: its session start checks Codex trusts the guard.
    pub(super) guard: bool,
    /// Its one `authenticate` with its key (`key_retry.rs`).
    pub(super) key_retry: KeyRetry,
}

/// Runs `waiting` once `thread`'s agent has a session, launching the agent first.
pub(super) fn ensure(actor: &mut Actor, thread: ThreadId, waiting: Waiting) {
    let Some(state) = state(actor, thread) else {
        fail_waiting(actor, thread, &waiting, &lost("a thread"));
        return;
    };
    match state.agent.as_deref_mut() {
        None => match state.preparing.as_deref_mut() {
            // The launch's folder checks run: the work waits for them.
            Some(preparing) => preparing.waiting.push(waiting),
            None => start(actor, thread, vec![waiting]),
        },
        Some(agent) => match agent.stage {
            // Its connection is gone (a call came back `Closed`, or its stdout ended while
            // idle): the work waits for a new agent (`stop_closed`).
            Stage::Closed => stop_closed(agent, waiting),
            Stage::Attached if agent.link.closed() => stop_closed(agent, waiting),
            Stage::Attached => run_waiting(actor, thread, waiting),
            Stage::Initializing | Stage::Loading => agent.waiting.push(waiting),
            Stage::Stopping => agent.after_stop.push(waiting),
        },
    }
}

/// Launches `thread`'s agent and sends `initialize`, its folder checks (and a Codex agent's
/// writes) off the actor first (`launch_checks.rs`). A launch the sign-in refuses fails what
/// waits for it (`sign_in::launch_refusal`), but one refused while the keys load waits for
/// them first (`sign_in::keys_wait`), and its checks look at the refusals again after.
fn start(actor: &mut Actor, thread: ThreadId, waiting: Vec<Waiting>) {
    if host_stopping(actor) {
        fail_all(actor, thread, waiting, &Error::Closed);
        return;
    }
    let keys_wait = super::sign_in::keys_wait(actor, thread);
    if keys_wait.is_none()
        && let Some(refused) = super::sign_in::launch_refusal(actor, thread)
    {
        fail_all(actor, thread, waiting, &refused);
        return;
    }
    // A new session attaches after the old one's detach, which a waiting turn still holds.
    let held = current(actor, thread).and_then(|current| current.detach.take());
    if let Some(reason) = held {
        log_detach(actor, thread, reason);
    }
    super::idle::make_room(actor, thread);
    launch_checks::prepare(actor, thread, waiting, keys_wait);
}

/// A new agent id (a Codex launch's preparation takes one too).
pub(super) fn next_agent_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Step 4 for a turn whose agent was started in another folder than its matter's row now names
/// (the matter was relocated since): stops that agent and starts a new one in the new folder for
/// the turn, which goes back to step 2 and keeps the writer. `false` when the agent's folder is
/// the matter's, or there is no running agent.
pub(super) fn restart_if_moved(actor: &mut Actor, thread: ThreadId, turn: TurnId) -> bool {
    let Ok(row) = actor.check_thread(thread).map(|entry| entry.row.matter) else {
        return false;
    };
    let Ok(folder) = actor
        .check_matter(row)
        .map(|entry| PathBuf::from(&entry.row.folder_path))
    else {
        return false;
    };
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return false;
    };
    if agent.folder == folder || agent.stage == Stage::Stopping {
        return false;
    }
    tracing::info!(%thread, "the matter moved; its agent starts again in the new folder");
    restart_for(actor, thread, turn);
    true
}

/// Step 4 for a prompted turn (typed, or taken from the queue) whose agent is marked to relaunch
/// (`idle::Relaunch`): with nothing else on that agent (no question or call open, no output held
/// for a turn of its own, nothing its mark waits for, `idle::holds_relaunch`) it stops and a new
/// one starts for the turn, as [`restart_if_moved`] does, so the prompt goes with the new inputs.
/// An agent-started turn that has opened is never cut: it is the current turn then. For a Tools
/// change, a command Claude put in the background (a steer during a Bash call) that has not
/// reported yet is cut: the host sees it only when it reports, so the relaunch ends it and its
/// result never comes. A relaunch within `cap_min_idle` of a turn that closed after a steer
/// during a tool call is logged. `false` when nothing was restarted.
pub(super) fn restart_if_marked(actor: &mut Actor, thread: ThreadId, turn: TurnId) -> bool {
    let (now, window) = (actor.now().mono, actor.turn_path.idle.window());
    let Some(state) = state(actor, thread) else {
        return false;
    };
    let prompted = state
        .current
        .as_ref()
        .is_some_and(|current| current.turn == turn && current.own.is_none());
    let held = super::own_turn::output_waits(&state.own_held);
    let Some(agent) = state.agent.as_deref() else {
        return false;
    };
    let open = !(agent.permissions.is_empty()
        && agent.elicitations.is_empty()
        && agent.url_elicitations.is_empty());
    let up = agent.stage == Stage::Attached && !agent.link.closed();
    let Some(why) = agent.idle.relaunch else {
        return false;
    };
    if !prompted || held || open || !up || super::idle::holds_relaunch(agent, now, window) {
        return false;
    }
    let steered_end = agent.idle.steered_end;
    tracing::info!(target: LIFECYCLE, %thread, why = why.as_str(), "agent relaunched for a prompt");
    if let Some(at) = steered_end
        && now.saturating_sub(at) <= window
    {
        let since_ms = u64::try_from(now.saturating_sub(at).as_millis()).unwrap_or(u64::MAX);
        tracing::info!(
            target: LIFECYCLE,
            %thread,
            since_ms,
            "agent relaunched soon after a steer during a tool call; a background command may end"
        );
    }
    restart_for(actor, thread, turn);
    true
}

/// Stops `thread`'s agent and starts a new one for `turn`, which goes back to step 2 and keeps
/// the writer (`Restarting`).
fn restart_for(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    agent.link.stop();
    agent.stage = Stage::Stopping;
    agent.stop_cause = StopCause::Restarting;
    let waiting = std::mem::take(&mut agent.waiting);
    agent.after_stop.extend(waiting);
    agent.after_stop.push(Waiting::Turn(turn));
    if let Some(current) = current(actor, thread) {
        current.phase = TurnPhase::Launching;
    }
}

/// Runs an agent call as a task that posts its result.
pub(super) fn call(
    actor: &Actor,
    thread: ThreadId,
    agent: u64,
    task: impl Future<Output = AgentReply> + 'static,
) {
    actor.spawn_post(async move {
        Input::Turn(TurnInput::Agent {
            thread,
            agent,
            reply: task.await,
        })
    });
}

/// An agent call returned.
pub(super) fn on_reply(actor: &mut Actor, thread: ThreadId, id: u64, reply: AgentReply) {
    let same = state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .is_some_and(|agent| agent.id == id);
    let closed = matches!(
        &reply,
        AgentReply::Prompt {
            result: Err(clauseo_acp::Error::Closed),
            ..
        } | AgentReply::Config {
            result: Err(clauseo_acp::Error::Closed),
            ..
        } | AgentReply::Mode {
            result: Err(clauseo_acp::Error::Closed),
            ..
        }
    ) || matches!(&reply, AgentReply::Steer(steered) if steered.closed());
    if same && matches!(&reply, AgentReply::Config { .. } | AgentReply::Mode { .. }) {
        super::idle::call_ended(actor, thread);
    }
    if same
        && closed
        && let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut())
        && agent.stage == Stage::Attached
    {
        agent.stage = Stage::Closed;
    }
    match reply {
        AgentReply::Prompt { turn, result } => {
            // A failure for the connection may wait for the host's probe first.
            if let Some(result) = offline::hold_for_probe(actor, thread, turn, result) {
                end::on_prompt_done(actor, thread, turn, result);
            }
        }
        AgentReply::Config { ctx, pick, result } => {
            super::answers::on_config(actor, thread, (ctx, pick), same, result);
        }
        AgentReply::Mode { ctx, mode, result } => {
            super::answers::on_mode(actor, thread, ctx, same, &mode, result);
        }
        AgentReply::PromptRefused { turn, error } => {
            end::on_prompt_refused(actor, thread, turn, &error);
        }
        AgentReply::Steer(steered) => super::queue::on_steered(actor, thread, *steered),
        AgentReply::ModeReset(mode, result) => {
            super::forced_mode::on_mode_reset(actor, thread, same, mode, result);
        }
        AgentReply::Initialized(_)
        | AgentReply::Session { .. }
        | AgentReply::Prepared { .. }
        | AgentReply::Refused(_)
            if !same => {}
        AgentReply::Initialized(Ok(info)) => session::on_initialized(actor, thread, info),
        AgentReply::Initialized(Err(error))
        | AgentReply::Session {
            result: Err(error),
            loaded: false,
        } => launch_failed(actor, thread, &Error::Acp(error)),
        AgentReply::Session {
            result: Ok(start),
            loaded,
        } => session::on_started(actor, thread, start, loaded),
        AgentReply::Prepared {
            result: Ok(start),
            loaded,
            picked,
            fell_back,
        } => session::on_prepared(actor, thread, start, (loaded, fell_back), &picked),
        // `Refused`: a session call's `.codex` re-check refused it, so no call was sent.
        AgentReply::Prepared {
            result: Err(error), ..
        }
        | AgentReply::Refused(error) => launch_failed(actor, thread, &error),
        AgentReply::Session {
            result: Err(error),
            loaded: true,
        } => {
            tracing::warn!(
                %thread,
                error = %Cause(&error),
                "session/load or session/resume failed; starting a new session"
            );
            // The old session's replay belongs to no session the new one knows.
            if let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) {
                agent.replayed.clear();
                agent.replay = None;
            }
            session::new_session(actor, thread);
        }
    }
}

pub(super) fn run_waiting(actor: &mut Actor, thread: ThreadId, waiting: Waiting) {
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref()) else {
        fail_waiting(actor, thread, &waiting, &lost("the agent"));
        return;
    };
    let (id, client, forced) = (agent.id, agent.link.client.clone(), agent.forced_ask);
    let Some(session) = agent.session.clone() else {
        fail_waiting(actor, thread, &waiting, &lost("the agent's session"));
        return;
    };
    match waiting {
        Waiting::Turn(turn) => steps::on_launched(actor, thread, turn),
        Waiting::Config(ctx, option, value)
            if forced && !agent::forced_ask::config_allowed(option.as_str(), select(&value)) =>
        {
            let error = Error::refused(
                ErrorCode::CommandInvalid,
                format!(
                    "config option {option} sets the mode, and this matter's folder is \
                     unprotected: only \"default\" and \"plan\" may be set"
                ),
            );
            actor.finish_command(ctx.id, Err(error));
        }
        Waiting::Config(ctx, option, value) => {
            super::idle::call_started(actor, thread);
            call(actor, thread, id, async move {
                let result = client.set_config_option(&session, option.clone(), value.clone());
                let (pick, result) = ((option, value), result.await);
                AgentReply::Config { ctx, pick, result }
            });
        }
        Waiting::Mode(ctx, mode) if forced && !agent::forced_ask::mode_allowed(mode.as_str()) => {
            let error = Error::refused(
                ErrorCode::CommandInvalid,
                format!(
                    "mode {mode} would approve changes without asking, and this matter's folder \
                     is unprotected: only \"default\" and \"plan\" may be set"
                ),
            );
            actor.finish_command(ctx.id, Err(error));
        }
        Waiting::Mode(ctx, mode) => {
            super::idle::call_started(actor, thread);
            call(actor, thread, id, async move {
                let result = client.set_mode(&session, mode.clone()).await;
                AgentReply::Mode { ctx, mode, result }
            });
        }
    }
}

/// The choice id `value` names, for a select option; `None` for a boolean.
fn select(value: &ConfigValue) -> Option<&str> {
    match value {
        ConfigValue::Select(choice) => Some(choice.as_str()),
        ConfigValue::Boolean(_) => None,
    }
}

/// Fails each of `waiting` with `error`.
pub(super) fn fail_all(actor: &mut Actor, thread: ThreadId, waiting: Vec<Waiting>, error: &Error) {
    for item in waiting {
        fail_waiting(actor, thread, &item, error);
    }
}

fn fail_waiting(actor: &mut Actor, thread: ThreadId, waiting: &Waiting, error: &Error) {
    match waiting {
        Waiting::Turn(turn) => steps::on_launch_failed(actor, thread, *turn, error),
        Waiting::Config(ctx, ..) | Waiting::Mode(ctx, ..) => {
            let error = Error::refused(error.code(), error.to_string());
            actor.finish_command(ctx.id, Err(error));
        }
    }
}
