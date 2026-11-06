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

use ended::{StopCause, stop_closed};
pub(super) use ended::{log_detach, on_ended, stop};

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
    after_stop: Vec<Waiting>,
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
    /// A Claude agent's plans folder, `<config folder>/plans`, from its launch's environment
    /// (`agent/plan_note.rs`); `None` for any other agent.
    pub(super) plan_notes: Option<PathBuf>,
    /// Its permission requests the host rejected for touching Clauseo's data folder
    /// (`agent/data_fence.rs`), logged as a count on its `agent ended` line.
    pub(super) denied_data_folder: u32,
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
    launch_checks::prepare(actor, thread, waiting, keys_wait);
}

/// The launch on the actor, with what its checks found (`prepared`), and `initialize`.
pub(super) fn launch_now(
    actor: &mut Actor,
    thread: ThreadId,
    waiting: Vec<Waiting>,
    prepared: Prepared,
) {
    // Only a Claude launch has a folder answer without a Codex preparation.
    let forced_ask =
        prepared.codex.is_none() && prepared.folder.is_some_and(agent::forced_ask::applies);
    if forced_ask {
        let matter = prepared.run.key.matter;
        tracing::info!(%matter, forced_ask, "Claude runs in forced ask: the matter folder is unprotected");
    }
    let (link, folder, codex, plan_notes) = match launch(actor, thread, prepared) {
        Ok(launched) => launched,
        Err(error) => {
            fail_all(actor, thread, waiting, &error);
            return;
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
        plan_notes,
        denied_data_folder: 0,
    };
    if let Some(state) = state(actor, thread) {
        state.agent = Some(Box::new(agent));
    }
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
fn launch(actor: &Actor, thread: ThreadId, prepared: Prepared) -> Result<Launched, Error> {
    let row = &actor.check_thread(thread)?.row;
    let settings = actor.effective.agent(row.agent.as_str()).ok_or_else(|| {
        Error::refused(
            ErrorCode::AgentLaunchFailed,
            format!("no agent {} in the settings", row.agent),
        )
    })?;
    // The relay token reaches the agent in a file; its stderr tail hides it all the same.
    let redacted = super::launch_checks::relay::redacted(&prepared.run);
    let folder = prepared.stored;
    let prepared_run_tmp = prepared.run.tmp.clone();
    let (forced, codex) = if let Some(prepared) = prepared.codex {
        let hooks = codex_hooks(actor);
        let guard = hooks.is_some();
        let mut forced =
            agent::codex::launch_env(&prepared.state, &settings.env, prepared.keys, hooks);
        // The shell's temp folder is the session's run folder's (`RunFolders::tmp`), never the
        // state folder's `tmp`, whose `arg0` links Codex puts first on `PATH` and runs
        // unsandboxed.
        forced.insert(
            std::ffi::OsString::from(agent::codex::TMPDIR_VAR),
            prepared_run_tmp.as_os_str().to_owned(),
        );
        let codex = CodexAgent {
            folder: prepared.folder,
            guard,
            key_retry: KeyRetry::default(),
        };
        (forced, Some((codex, prepared.login)))
    } else {
        // A Claude agent's state folder goes in before `plans_folder` reads the merged map
        // below; its login store is the login home, or the lawyer's default store while
        // desktop says the login is reused.
        // The reused store `""` is the lawyer's default store (on macOS their Keychain). Right
        // at tier C, all PR-C has: no jail stands between the agent and that store. The tier A
        // and B rule (`SignedOut { found: ClaudeSandbox }`, S18) lands with the jail in R17
        // PR-D.
        let reused = actor.wiring.sign_in.reused.contains(&row.agent);
        let store = prepared.claude_home.as_deref().filter(|_| !reused);
        let forced = prepared
            .run
            .state
            .as_deref()
            .filter(|_| prepared.claude_home.is_some())
            .map(|state| agent::home::claude_launch_env(state, store));
        (forced.unwrap_or_default(), None)
    };
    let hook = agent::loads::launch_env(actor.profile.launch_env(&row.agent));
    // The key retry is armed by the hook's key and the launch's sign-in check.
    let codex = codex.map(|(mut codex, login)| {
        codex.key_retry = KeyRetry::at_launch(&hook, login);
        codex
    });
    let dropped: &[&str] = if codex.is_some() {
        &agent::codex::DROPPED_VARS
    } else {
        &[]
    };
    let inherited: Vec<_> = match &actor.wiring.inherited_env {
        Some(env) => env.clone(),
        None => std::env::vars_os().collect(),
    };
    let spec = agent::launch_spec(
        settings,
        hook,
        forced,
        dropped,
        inherited.into_iter(),
        agent::session_folder(&folder),
        actor.wiring.path_prepend.as_deref(),
    )?
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
        actor.launcher.as_ref(),
        &spec,
        Arc::clone(&actor.clock),
        post,
    )?;
    Ok((link, folder, codex, plan_notes))
}

/// THE CODEX HOOKS SEAM: the `hooks` object a Codex agent's `CODEX_CONFIG` carries, with its
/// `state` trust entry and `trusted_hash`: the one a test set (`HostConfig::with_codex_hooks`),
/// else [`codex_hook_config`](agent::loads::codex_hook_config) for the guard program the
/// Claude Code entry runs (`HostConfig::with_pre_write_guard`), so its `PreToolUse` holds one
/// group, the guard's. `None` with no guard program, or (with a warning) one that
/// `codex_hook_config` cannot quote. It is read once per launch, which records on the agent
/// whether one went in (`Agent::guard`); with none the session start skips the guard's trust
/// check.
fn codex_hooks(actor: &Actor) -> Option<serde_json::Map<String, serde_json::Value>> {
    if let Some(hooks) = &actor.codex_hooks {
        return Some(hooks.clone());
    }
    let program = actor.pre_write_guard.as_deref()?;
    let Some(serde_json::Value::Object(hooks)) = agent::loads::codex_hook_config(program) else {
        tracing::warn!(
            "the pre-write guard's path cannot go in Codex's hook command, so Codex runs without \
             the guard"
        );
        return None;
    };
    Some(hooks)
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
        AgentReply::ModeReset(result) => {
            super::forced_mode::on_mode_reset(actor, thread, same, result);
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
        Waiting::Config(ctx, option, value) => call(actor, thread, id, async move {
            let result = client.set_config_option(&session, option, value).await;
            AgentReply::Config { ctx, result }
        }),
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
        Waiting::Mode(ctx, mode) => call(actor, thread, id, async move {
            let result = client.set_mode(&session, mode.clone()).await;
            AgentReply::Mode { ctx, mode, result }
        }),
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
