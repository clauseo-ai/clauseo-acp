//! A launched agent's session: `session/load` with the stored session id when the agent offers
//! it (after reading the thread's whole log for replay dedupe), else `session/new`, then the
//! attach commit, the held replay and the work that waited.

use std::path::PathBuf;

use clauseo_acp::{AgentInfo, LoadSession, NewSession, SessionStart};
use clauseo_protocol::{
    AgentSessionAttached, ConfigOptionsReplaced, Envelope, ModeChanged, Notice, NoticeLevel,
    StreamId, ThreadId, ViewEntry,
};

use super::super::Actor;
use super::super::input::Input;
use super::agents::{Stage, call, launch_failed, run_waiting};
use super::out::Out;
use super::{AgentReply, LogFor, Then, TurnInput, lost, state, updates};
use crate::error::Error;
use crate::sent::SentView;

/// The notice a thread gets when its agent starts a new session after earlier turns.
const CONTEXT_NOT_RESTORED: &str = "The agent started a new session for this thread, so it \
     does not remember the earlier turns.";

pub(super) fn on_initialized(actor: &mut Actor, thread: ThreadId, info: AgentInfo) {
    let stored = actor
        .threads
        .get(&thread)
        .and_then(|entry| entry.row.acp_session_id.clone());
    let loads = info.capabilities.load_session && stored.is_some();
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    agent.info = Some(info);
    agent.stage = Stage::Loading;
    if !loads {
        new_session(actor, thread);
        return;
    }
    // Replay dedupe needs every ACP id the thread logged, which a snapshot does not keep.
    let store = actor.store.clone();
    actor.spawn_post(async move {
        let result = super::super::streams::read_whole_log(&store, StreamId::Thread(thread))
            .await
            .map(|reopen| reopen.tail);
        Input::Turn(TurnInput::Log {
            thread,
            why: LogFor::Replay,
            result,
        })
    });
}

/// The whole log for replay dedupe came back: send `session/load`.
pub(super) fn on_replay_log(
    actor: &mut Actor,
    thread: ThreadId,
    result: Result<Vec<Envelope>, clauseo_store::Error>,
) {
    let session = actor
        .threads
        .get(&thread)
        .and_then(|entry| entry.row.acp_session_id.clone());
    let cwd = folder(actor, thread);
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    let (Some(session), Ok(cwd), Stage::Loading) = (session, cwd, agent.stage) else {
        return;
    };
    match result {
        Ok(log) => agent.replay = Some(SentView::rebuild(StreamId::Thread(thread), &log)),
        Err(error) => tracing::warn!(%thread, %error, "no log for replay dedupe"),
    }
    let (id, client, fence) = (agent.id, agent.link.client.clone(), agent.link.fence());
    call(actor, thread, id, async move {
        let request = LoadSession {
            session,
            cwd,
            mcp_servers: Vec::new(),
        };
        let result = client.load_session(request).await;
        fence.await;
        AgentReply::Session {
            loaded: true,
            result,
        }
    });
}

pub(super) fn new_session(actor: &mut Actor, thread: ThreadId) {
    let cwd = match folder(actor, thread) {
        Ok(cwd) => cwd,
        Err(error) => {
            launch_failed(actor, thread, &error);
            return;
        }
    };
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref()) else {
        return;
    };
    let (id, client, fence) = (agent.id, agent.link.client.clone(), agent.link.fence());
    call(actor, thread, id, async move {
        let request = NewSession {
            cwd,
            mcp_servers: Vec::new(),
            meta: None,
        };
        let result = client.new_session(request).await;
        fence.await;
        AgentReply::Session {
            loaded: false,
            result,
        }
    });
}

fn folder(actor: &Actor, thread: ThreadId) -> Result<PathBuf, Error> {
    let matter = actor.check_thread(thread)?.row.matter;
    Ok(PathBuf::from(&actor.check_matter(matter)?.row.folder_path))
}

/// The session is up: one commit of `AgentSessionAttached`, the thread row, the session's mode
/// and options (and the "context not restored" notice), then the held replay and the waiting
/// work.
pub(super) fn attach(actor: &mut Actor, thread: ThreadId, start: SessionStart, loaded: bool) {
    match attach_commit(actor, thread, &start, loaded) {
        Ok(()) => {}
        Err(error) => {
            launch_failed(actor, thread, &error);
            return;
        }
    }
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    agent.session = Some(start.session);
    agent.stage = Stage::Attached;
    let replayed = std::mem::take(&mut agent.replayed);
    let waiting = std::mem::take(&mut agent.waiting);
    for event in replayed {
        updates::on_event(actor, thread, Box::new(event));
    }
    if let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) {
        agent.replay = None;
    }
    for item in waiting {
        run_waiting(actor, thread, item);
    }
}

fn attach_commit(
    actor: &mut Actor,
    thread: ThreadId,
    start: &SessionStart,
    loaded: bool,
) -> Result<(), Error> {
    let info = state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .and_then(|agent| agent.info.clone())
        .ok_or_else(|| lost("the agent's info"))?;
    let notice = !loaded && has_earlier_turns(actor, thread);
    let mut out = Out::new(actor, thread, None)?;
    out.push(
        actor,
        AgentSessionAttached {
            session: start.session.clone(),
            agent_version: info.version,
            capabilities: info.capabilities,
        },
    )?;
    if let Some(modes) = &start.modes {
        out.push(
            actor,
            ModeChanged {
                mode_id: modes.current.clone(),
            },
        )?;
    }
    if !start.config_options.is_empty() {
        out.push(
            actor,
            ConfigOptionsReplaced {
                options: start.config_options.clone(),
            },
        )?;
    }
    if notice {
        out.push(
            actor,
            Notice {
                level: NoticeLevel::Info,
                text: CONTEXT_NOT_RESTORED.to_owned(),
            },
        )?;
    }
    let now = actor.now().wall;
    let entry = actor
        .threads
        .get_mut(&thread)
        .ok_or_else(|| lost("a thread"))?;
    entry.row.acp_session_id = Some(start.session.clone());
    entry.row.updated = now;
    out.draft.thread_row(entry.row.clone());
    out.send(actor, Then::Nothing)
}

/// Whether the thread has turns before its current one.
fn has_earlier_turns(actor: &Actor, thread: ThreadId) -> bool {
    let current = actor
        .threads
        .get(&thread)
        .and_then(|entry| entry.turn.current.as_ref())
        .map(|current| current.turn);
    actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .is_some_and(|state| {
            state.entries().values().any(|entry| match entry {
                ViewEntry::UserMessage(message) => Some(message.turn) != current,
                ViewEntry::Step(_) => true,
                ViewEntry::Header(_)
                | ViewEntry::AgentText(_)
                | ViewEntry::AgentContent(_)
                | ViewEntry::ToolCall(_)
                | ViewEntry::Permission(_)
                | ViewEntry::Elicitation(_)
                | ViewEntry::LimitStop(_)
                | ViewEntry::Notice(_)
                | ViewEntry::Marker(_)
                | ViewEntry::MatterHeader(_)
                | ViewEntry::Version(_) => false,
            })
        })
}
