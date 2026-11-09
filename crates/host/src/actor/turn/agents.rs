//! A thread's agent: its launch (step 2: launch, `initialize`, `session/load` or `session/new`,
//! the attach commit) and the work that waits for its session. Its stop and its end are in
//! `agents/ended.rs`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use clauseo_acp::{AcpEvent, AgentInfo, MessageRole, PendingKey};
use clauseo_protocol::{
    AgentChoiceId, AgentKind, AgentSessionId, CompactString, ConfigValue, ElicitationId, ErrorCode,
    MessageId, PermissionRequestId, ThreadId, TurnId,
};

use clauseo_process::TreeWatch;

use super::super::Actor;
use super::super::commands::CommandCtx;
use super::super::input::Input;
use super::key_retry::KeyRetry;
use super::launch_checks::{self, Prepared};
use super::{
    AgentReply, TurnInput, TurnPhase, current, end, host_stopping, lost, session, state, steps,
};
use crate::agent::{self, AgentLink};
use crate::error::Error;
use crate::log_file::{Cause, LIFECYCLE};
use crate::rate::{RateCap, RatedUpdate};
use crate::sent::SentView;

mod ended;
mod forced;

use ended::{StopCause, stop_closed};
pub(super) use ended::{hold_stop, idle_stop, log_detach, on_ended, restart_for_switch, stop};

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
    /// A jailed Codex's mode switch held to the turn boundary (`forced_mode::switch`).
    pub(super) switch: Option<Waiting>,
    /// The matter folder it was started in, its working directory.
    pub(super) folder: PathBuf,
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
    /// Replayed updates, held until the session is attached.
    pub(super) replayed: Vec<AcpEvent>,
    /// A Codex session's checks, or a Claude session's switch to Clauseo's mode, run before its
    /// attach: every update is held with the replayed ones meanwhile, so none is logged before
    /// `AgentSessionAttached`.
    pub(super) preparing: bool,
    /// What a Codex agent's launch recorded; `None` for any other agent.
    pub(super) codex: Option<CodexAgent>,
    /// The programs of the MCP servers its session was given: a cancelled turn's sweep leaves
    /// a process running one of them (`agent/leftovers.rs`).
    pub(super) mcp_commands: Vec<PathBuf>,
    /// A Claude agent whose launch's folder check found a network drive or a drive without
    /// permissions: it runs in forced ask (`agent/forced_ask.rs`).
    pub(super) forced_ask: bool,
    /// The mode a Claude agent's launch asks for (`agent/claude_mode.rs`): auto when it went
    /// through the sandbox's launchers and is not forced ask. Decided at each launch, never stored.
    pub(super) launch_mode: &'static str,
    /// A Claude agent's plans folder, `<config folder>/plans`, from its launch's environment
    /// (`agent/plan_note.rs`); `None` for any other agent.
    pub(super) plan_notes: Option<PathBuf>,
    /// Its permission requests the host rejected for touching Clauseo's data folder
    /// (`agent/data_fence.rs`), logged as a count on its `agent ended` line.
    pub(super) denied_data_folder: u32,
    /// Its idle clock and config calls in flight (`idle.rs`).
    pub(super) idle: super::idle::Mark,
}

/// What a Codex agent's launch recorded.
#[derive(Debug)]
pub(super) struct CodexAgent {
    /// The matter folder as resolved through links at the launch: `_codex/hooks/list` names
    /// it. Each prompt and session call checks the stored folder for `.codex` first, resolved
    /// again.
    pub(super) folder: PathBuf,
    /// A hooks object went into its `CODEX_CONFIG`, so its session start checks that Codex
    /// trusts the guard.
    pub(super) guard: bool,
    /// Its one `authenticate` with its key (`key_retry.rs`).
    pub(super) key_retry: KeyRetry,
    /// In the jail, the mode its launch's policy is (one of `codex::jailed::MODES`,
    /// `forced_mode.rs`); `None` at tier C.
    pub(super) jail: Option<&'static str>,
    /// At tier C, the lawyer's mode pick and a switch back in flight; its mode log lines' counts
    /// (`forced_mode/unjailed.rs`).
    pub(super) modes: super::forced_mode::CodexModes,
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
/// them first (`sign_in::keys_wait`), and its checks look at the refusals again after. Every
/// launch comes here, and waits at the launch gate (`launch_checks::gate`) until the thread's
/// ended sessions' processes are gone.
pub(super) fn start(actor: &mut Actor, thread: ThreadId, waiting: Vec<Waiting>) {
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
    // Never beside an ended session's processes: they keep their launch's policy.
    let Some(waiting) = launch_checks::gate(actor, thread, waiting) else {
        return;
    };
    // A new session attaches after the old one's detach, which a waiting turn still holds.
    let held = current(actor, thread).and_then(|current| current.detach.take());
    if let Some(reason) = held {
        log_detach(actor, thread, reason);
    }
    super::idle::make_room(actor, thread);
    launch_checks::prepare(actor, thread, waiting, keys_wait);
}

/// The launch on the actor, with what its checks found (`prepared`), and `initialize`. When
/// the launch failed after the agent's process was started, answers its tree, for the run
/// folder's removal to wait for.
pub(super) fn launch_now(
    actor: &mut Actor,
    thread: ThreadId,
    waiting: Vec<Waiting>,
    prepared: Prepared,
) -> Option<TreeWatch> {
    // Only a Claude launch has a folder answer without a Codex preparation.
    let forced_ask =
        prepared.codex.is_none() && prepared.folder.is_some_and(agent::forced_ask::applies);
    if forced_ask {
        let matter = prepared.run.key.matter;
        tracing::info!(%matter, forced_ask, "Claude runs in forced ask: the matter folder is unprotected");
    }
    let launch_mode = agent::claude_mode::launch_mode(prepared.sandbox.is_some(), forced_ask);
    let mut tree = None;
    let (link, folder, codex, plan_notes) = match launch(actor, thread, prepared, &mut tree) {
        Ok(launched) => launched,
        Err(error) => {
            fail_all(actor, thread, waiting, &error);
            return tree;
        }
    };
    let id = next_agent_id();
    let client = link.client.clone();
    call(actor, thread, id, async move {
        AgentReply::Initialized(client.initialize().await)
    });
    let agent_id = actor
        .check_thread(thread)
        .map(|entry| entry.row.agent.clone())
        .ok();
    tracing::info!(
        target: LIFECYCLE,
        %thread,
        agent = agent_id.as_ref().map(clauseo_protocol::AgentId::as_str),
        pid = link.pid(),
        "agent launched"
    );
    let agent = Agent {
        id,
        link,
        stage: Stage::Initializing,
        info: None,
        waiting,
        after_stop: Vec::new(),
        switch: None,
        folder,
        stop_cause: StopCause::Asked,
        session: None,
        rate: RateCap::new(),
        rate_armed: false,
        permissions: BTreeMap::new(),
        elicitations: BTreeMap::new(),
        url_elicitations: BTreeMap::new(),
        messages: BTreeMap::new(),
        run: None,
        compaction: BTreeMap::new(),
        replay: None,
        replayed: Vec::new(),
        preparing: false,
        codex,
        mcp_commands: Vec::new(),
        forced_ask,
        launch_mode,
        plan_notes,
        denied_data_folder: 0,
        idle: super::idle::Mark::default(),
    };
    if let Some(state) = state(actor, thread) {
        state.agent = Some(Box::new(agent));
    }
    super::idle::launched(actor, thread);
    None
}

/// A new agent id (a Codex launch's preparation takes one too).
pub(super) fn next_agent_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// What a launch gives: the link, the folder it runs in, what a Codex launch recorded, and a
/// Claude launch's plans folder.
type Launched = (AgentLink, PathBuf, Option<CodexAgent>, Option<PathBuf>);

/// The launch itself: the spec from the settings (inherited environment, the agent's
/// variables, then the profile hook's with the host's switches over them, then what the host
/// forces for the agent's kind), in the matter folder that was checked (`prepared`). A Codex
/// agent's `.codex` refusal (#184), its home and its `config.toml` are done by then.
fn launch(
    actor: &Actor,
    thread: ThreadId,
    prepared: Prepared,
    tree: &mut Option<TreeWatch>,
) -> Result<Launched, Error> {
    let row = &actor.check_thread(thread)?.row;
    let settings = actor.effective.agent(row.agent.as_str()).ok_or_else(|| {
        Error::refused(
            ErrorCode::AgentLaunchFailed,
            format!("no agent {} in the settings", row.agent),
        )
    })?;
    // The relay token reaches the agent in a file; its stderr tail hides it all the same.
    let redacted = super::launch_checks::relay::redacted(&prepared.run);
    let jail = prepared.jail_mode();
    let folder = prepared.stored;
    let (mut forced, codex) = forced::of(
        actor,
        &settings.env,
        (prepared.codex, jail),
        &prepared.run,
        (prepared.claude_home.as_deref(), prepared.reused),
    );
    let (settings, hook, inherited, path_prepend) =
        forced::sources(actor, &row.agent, settings, prepared.sandbox.is_some());
    // The key retry is armed by the hook's key and the launch's sign-in check.
    let codex = codex.map(|(mut codex, login)| {
        codex.key_retry = KeyRetry::at_launch(&hook, login);
        codex
    });
    let removed = forced::jailed(&mut forced, prepared.sandbox.as_ref());
    let codex_dropped: &[&str] = if codex.is_some() {
        &agent::codex::DROPPED_VARS
    } else {
        &[]
    };
    let dropped: Vec<&str> = codex_dropped
        .iter()
        .copied()
        .chain(removed.iter().map(String::as_str))
        .collect();
    // Tier C (no jail) leaves the agent's environment alone: no session proxy variables here
    // (`ProxyHandle::env`); only the browser entry reaches the proxy, advisory without a jail.
    let spec = agent::launch_spec(
        &settings,
        hook,
        forced,
        &dropped,
        inherited.into_iter(),
        agent::session_folder(&folder),
        path_prepend.as_deref(),
    )
    .map_err(super::launch_checks::folder::gone_at_spawn)?
    .with_redacted(redacted);
    let claude = AgentKind::of(&row.agent) == Some(AgentKind::Claude);
    let plan_notes = claude
        .then(|| agent::plan_note::plans_folder(spec.env(), cfg!(windows)))
        .flatten();
    let post = actor.post.clone();
    let post: Rc<dyn Fn(agent::AgentInput)> = Rc::new(move |input| {
        // The actor may be gone by then.
        let _gone = post.try_send(Input::Agent { thread, input });
    });
    let link = agent::start(
        &actor.spawner,
        super::launch_checks::launcher(prepared.sandbox.as_ref(), actor.launcher.as_ref()),
        &spec,
        Arc::clone(&actor.clock),
        post,
        tree,
    )?;
    Ok((link, folder, codex, plan_notes))
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
    agent.link.stop();
    agent.stage = Stage::Stopping;
    agent.stop_cause = StopCause::Restarting;
    let waiting = std::mem::take(&mut agent.waiting);
    agent.after_stop.extend(waiting);
    agent.after_stop.push(Waiting::Turn(turn));
    if let Some(current) = current(actor, thread) {
        current.phase = TurnPhase::Launching;
    }
    true
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
        AgentReply::Prompt { turn, result } => end::on_prompt_done(actor, thread, turn, result),
        AgentReply::Config { ctx, result } => {
            super::answers::on_config(actor, thread, ctx, same, result);
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
        } => session::attach(actor, thread, start, loaded),
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
    let stopping = state(actor, thread).and_then(|state| state.agent.as_deref_mut());
    if let Some(agent) = stopping.filter(|agent| agent.stage == Stage::Stopping) {
        // A relaunch for a mode switch began before it (`forced_mode::switch`).
        agent.after_stop.push(waiting);
        return;
    }
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref()) else {
        fail_waiting(actor, thread, &waiting, &lost("the agent"));
        return;
    };
    let (id, client) = (agent.id, agent.link.client.clone());
    let Some(session) = agent.session.clone() else {
        fail_waiting(actor, thread, &waiting, &lost("the agent's session"));
        return;
    };
    // Forced ask's and a jailed Codex's modes (`forced_mode.rs`).
    if let Some((ctx, error)) = super::forced_mode::refusal(actor, thread, &waiting) {
        actor.finish_command(ctx, Err(error));
        return;
    }
    let Some(waiting) = super::forced_mode::switch(actor, thread, waiting) else {
        return;
    };
    match waiting {
        Waiting::Turn(turn) => steps::on_launched(actor, thread, turn),
        Waiting::Config(ctx, option, value) => {
            super::idle::call_started(actor, thread);
            call(actor, thread, id, async move {
                let result = client.set_config_option(&session, option, value).await;
                AgentReply::Config { ctx, result }
            });
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
            // The wire form keeps what a refusal carries (`matter.folder_nested`'s files).
            actor.finish_command(ctx.id, Err(Error::Refused(error.to_wire())));
        }
    }
}

/// The launch failed after the agent started: stop it and fail what waits for it.
pub(super) fn launch_failed(actor: &mut Actor, thread: ThreadId, error: &Error) {
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    tracing::warn!(%thread, error = %Cause(error), "the agent did not start");
    agent.link.stop();
    agent.stage = Stage::Stopping;
    let waiting = std::mem::take(&mut agent.waiting);
    for item in waiting {
        fail_waiting(actor, thread, &item, error);
    }
}
