//! A thread's agent: its launch (step 2: launch, `initialize`, `session/load` or `session/new`,
//! the attach commit), the work that waits for its session, and its end.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use clauseo_acp::{AcpEvent, AgentInfo, MessageRole, PendingKey};
use clauseo_protocol::{
    AgentChoiceId, AgentSessionDetached, AgentSessionId, CompactString, ConfigValue, DetachReason,
    ElicitationId, ErrorCode, MessageId, PermissionRequestId, StreamId, ThreadId, TurnId,
};

use super::super::Actor;
use super::super::commands::CommandCtx;
use super::super::input::Input;
use super::out::Out;
use super::{
    AgentReply, End, Phase, Then, TurnInput, current, end, lost, session, state, steps, stopping,
};
use crate::agent::{self, AgentLink, Ended, StopStep};
use crate::error::Error;
use crate::rate::{RateCap, RatedUpdate};
use crate::sent::SentView;

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
}

/// Runs `waiting` once `thread`'s agent has a session, launching the agent first.
pub(super) fn ensure(actor: &mut Actor, thread: ThreadId, waiting: Waiting) {
    let Some(state) = state(actor, thread) else {
        fail_waiting(actor, thread, &waiting, &lost("a thread"));
        return;
    };
    match state.agent.as_deref_mut() {
        None => start(actor, thread, vec![waiting]),
        Some(agent) => match agent.stage {
            Stage::Attached => run_waiting(actor, thread, waiting),
            Stage::Initializing | Stage::Loading => agent.waiting.push(waiting),
            Stage::Stopping => agent.after_stop.push(waiting),
        },
    }
}

/// Launches `thread`'s agent and sends `initialize`.
fn start(actor: &mut Actor, thread: ThreadId, waiting: Vec<Waiting>) {
    if stopping(actor) {
        for item in waiting {
            fail_waiting(actor, thread, &item, &Error::Closed);
        }
        return;
    }
    let link = match launch(actor, thread) {
        Ok(link) => link,
        Err(error) => {
            for item in waiting {
                fail_waiting(actor, thread, &item, &error);
            }
            return;
        }
    };
    let id = next_agent_id();
    let client = link.client.clone();
    call(actor, thread, id, async move {
        AgentReply::Initialized(client.initialize().await)
    });
    tracing::debug!(%thread, pid = link.pid(), "agent launched");
    let agent = Agent {
        id,
        link,
        stage: Stage::Initializing,
        info: None,
        waiting,
        after_stop: Vec::new(),
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
    };
    if let Some(state) = state(actor, thread) {
        state.agent = Some(Box::new(agent));
    }
}

/// A new agent id.
fn next_agent_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// The launch itself: the spec from the settings (inherited environment, the agent's
/// variables, no profile variables yet), in the matter folder.
fn launch(actor: &Actor, thread: ThreadId) -> Result<AgentLink, Error> {
    let row = &actor.check_thread(thread)?.row;
    let folder = PathBuf::from(&actor.check_matter(row.matter)?.row.folder_path);
    let effective = Arc::clone(&actor.effective);
    let settings = effective.agent(row.agent.as_str()).ok_or_else(|| {
        Error::refused(
            ErrorCode::AgentLaunchFailed,
            format!("no agent {} in the settings", row.agent),
        )
    })?;
    let spec = agent::launch_spec(settings, BTreeMap::new(), std::env::vars_os(), folder)?;
    let post = actor.post.clone();
    let post: Rc<dyn Fn(agent::AgentInput)> = Rc::new(move |input| {
        // The actor may be gone by then.
        let _gone = post.try_send(Input::Agent { thread, input });
    });
    agent::start(
        &actor.spawner,
        actor.launcher.as_ref(),
        &spec,
        Arc::clone(&actor.clock),
        post,
    )
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
    match reply {
        AgentReply::Prompt { turn, result } => end::on_prompt_done(actor, thread, turn, result),
        AgentReply::Config { ctx, result } => {
            super::answers::on_config(actor, thread, ctx, same, result);
        }
        AgentReply::Mode { ctx, mode, result } => {
            super::answers::on_mode(actor, thread, ctx, same, &mode, result);
        }
        AgentReply::Initialized(_) | AgentReply::Session { .. } if !same => {}
        AgentReply::Initialized(Ok(info)) => session::on_initialized(actor, thread, info),
        AgentReply::Initialized(Err(error))
        | AgentReply::Session {
            result: Err(error),
            loaded: false,
        } => launch_failed(actor, thread, &Error::Acp(error)),
        AgentReply::Session {
            result: Ok(start),
            loaded,
        } => session::attach(actor, thread, start, loaded),
        AgentReply::Session {
            result: Err(error),
            loaded: true,
        } => {
            tracing::warn!(%thread, %error, "session/load failed; starting a new session");
            session::new_session(actor, thread);
        }
    }
}

pub(super) fn run_waiting(actor: &mut Actor, thread: ThreadId, waiting: Waiting) {
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref()) else {
        fail_waiting(actor, thread, &waiting, &lost("the agent"));
        return;
    };
    let (id, client) = (agent.id, agent.link.client.clone());
    let Some(session) = agent.session.clone() else {
        fail_waiting(actor, thread, &waiting, &lost("the agent's session"));
        return;
    };
    match waiting {
        Waiting::Turn(turn) => steps::on_launched(actor, thread, turn),
        Waiting::Config(ctx, option, value) => call(actor, thread, id, async move {
            let result = client.set_config_option(&session, option, value).await;
            AgentReply::Config { ctx, result }
        }),
        Waiting::Mode(ctx, mode) => call(actor, thread, id, async move {
            let result = client.set_mode(&session, mode.clone()).await;
            AgentReply::Mode { ctx, mode, result }
        }),
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
    tracing::warn!(%thread, %error, "the agent did not start");
    agent.link.stop();
    agent.stage = Stage::Stopping;
    let waiting = std::mem::take(&mut agent.waiting);
    for item in waiting {
        fail_waiting(actor, thread, &item, error);
    }
}

/// Stops `thread`'s agent with the shutdown ladder; `Ended` follows.
pub(super) fn stop(actor: &mut Actor, thread: ThreadId) {
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    if agent.stage == Stage::Stopping {
        return;
    }
    agent.link.stop();
    agent.stage = Stage::Stopping;
    let waiting = std::mem::take(&mut agent.waiting);
    for item in waiting {
        fail_waiting(actor, thread, &item, &Error::Closed);
    }
}

/// The agent's tasks are done: log the detach, end a turn the exit cut short, and start a new
/// agent for work that came while it stopped.
pub(super) fn on_ended(actor: &mut Actor, thread: ThreadId, ended: Ended) {
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.take()) else {
        return;
    };
    let Agent {
        session,
        waiting,
        after_stop,
        ..
    } = *agent;
    let reason = match ended {
        Ended::Exited(reason) => reason,
        Ended::Stopped { step, report } => {
            if step == StopStep::Unresponsive {
                tracing::warn!(%thread, ?report, "the agent did not exit after its kill");
            }
            DetachReason::Closed
        }
    };
    let exited = Error::refused(ErrorCode::AgentExited, "the agent exited");
    for item in waiting {
        fail_waiting(actor, thread, &item, &exited);
    }
    let detach = session.is_some().then_some(reason);
    let phase = current(actor, thread).map(|current| (current.phase, current.cancelled));
    match (phase, detach) {
        (Some((Phase::Prompting, cancelled)), detach) => {
            let end = if cancelled {
                End::cancelled(detach)
            } else {
                End::failed(ErrorCode::AgentExited, None, detach)
            };
            end::end_turn(actor, thread, end);
        }
        (Some((Phase::Stopping, _)), detach) => {
            if let Some(reason) = detach {
                log_detach(actor, thread, reason);
            }
            end::end_turn(actor, thread, End::cancelled(None));
        }
        (Some((Phase::Ending, _)), Some(reason)) => {
            // Logged after the turn's `TurnEnded`, which is still to come.
            if let Some(current) = current(actor, thread) {
                match &mut current.end {
                    Some(end) => end.detach = Some(reason),
                    None => {
                        current.end = Some(End::failed(ErrorCode::AgentExited, None, Some(reason)));
                    }
                }
            }
        }
        (_, Some(reason)) => log_detach(actor, thread, reason),
        (_, None) => {}
    }
    if !after_stop.is_empty() {
        start(actor, thread, after_stop);
    }
    actor.release(StreamId::Thread(thread));
}

/// Logs `AgentSessionDetached { reason }`.
pub(super) fn log_detach(actor: &mut Actor, thread: ThreadId, reason: DetachReason) {
    let sent = Out::new(actor, thread, None).and_then(|mut out| {
        out.push(actor, AgentSessionDetached { reason })?;
        out.send(actor, Then::Nothing)
    });
    if let Err(error) = sent {
        tracing::warn!(%thread, %error, "the detach was not logged");
    }
}
