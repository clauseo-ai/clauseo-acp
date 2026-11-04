//! A launched agent's session: `session/load` with the stored session id when the agent offers
//! it (after reading the thread's whole log for replay dedupe), else `session/resume` when it
//! offers that (nothing is replayed), else `session/new`; for a Codex agent the checks before
//! its first prompt (the ask mode, the guard's trust), for a Claude agent the switch to
//! Clauseo's mode; then the attach commit, the held replay and the work that waited. When
//! `initialize` gave the agent a title, its name is logged first (`AgentNamed`,
//! `log_agent_name`), so a failed `session/new` leaves it in the thread.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_acp::{AcpClient, AgentInfo, LoadSession, McpServer, NewSession, SessionStart};
use clauseo_protocol::{
    AgentChoiceId, AgentKind, AgentNamed, AgentSessionAttached, CompactString,
    ConfigOptionsReplaced, Envelope, ErrorCode, MatterId, ModeChanged, Notice, NoticeLevel,
    StreamId, ThreadId, ViewEntry,
};

use super::super::Actor;
use super::super::input::Input;
use super::agents::{Stage, call, launch_failed, run_waiting};
use super::out::Out;
use super::{AgentReply, LogFor, Then, TurnInput, lost, state, updates};
use crate::agent::{self, codex};
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
    let resumes = !loads && info.capabilities.session.resume && stored.is_some();
    let name = info.title.as_deref().and_then(agent::title::clean_title);
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    agent.info = Some(info);
    agent.stage = Stage::Loading;
    // Before the session starts, so a `session/new` that fails leaves the name in the log first.
    log_agent_name(actor, thread, name);
    if resumes {
        resume_session(actor, thread);
        return;
    }
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
    let loads = session_loads(actor, thread);
    let clock = Arc::clone(&actor.clock);
    let matter = matter_of(actor, thread);
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    let (Some(session), Ok(cwd), Ok((meta, mcp_servers)), Stage::Loading) =
        (session, cwd, loads, agent.stage)
    else {
        return;
    };
    match result {
        Ok(log) => agent.replay = Some(SentView::rebuild(StreamId::Thread(thread), &log)),
        Err(error) => tracing::warn!(%thread, %error, "no log for replay dedupe"),
    }
    let (id, client, fence) = (agent.id, agent.link.client.clone(), agent.link.fence());
    let check = codex::prepare::recheck(
        clock,
        matter,
        agent.codex.is_some().then(|| agent.folder.clone()),
    );
    call(actor, thread, id, async move {
        if let Err(error) = check.await {
            return AgentReply::Refused(error);
        }
        let request = LoadSession {
            session,
            cwd,
            mcp_servers,
            meta: Some(meta),
        };
        let result = client.load_session(request).await;
        fence.await;
        AgentReply::Session {
            loaded: true,
            result,
        }
    });
}

/// `session/resume` with the stored session id: the agent takes it back and replays nothing.
fn resume_session(actor: &mut Actor, thread: ThreadId) {
    let session = actor
        .threads
        .get(&thread)
        .and_then(|entry| entry.row.acp_session_id.clone());
    let started = folder(actor, thread).and_then(|cwd| Ok((cwd, session_loads(actor, thread)?)));
    let (Some(session), Ok((cwd, (meta, mcp_servers)))) = (session, started) else {
        new_session(actor, thread);
        return;
    };
    let clock = Arc::clone(&actor.clock);
    let matter = matter_of(actor, thread);
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref()) else {
        return;
    };
    let (id, client, fence) = (agent.id, agent.link.client.clone(), agent.link.fence());
    let check = codex::prepare::recheck(
        clock,
        matter,
        agent.codex.is_some().then(|| agent.folder.clone()),
    );
    call(actor, thread, id, async move {
        if let Err(error) = check.await {
            return AgentReply::Refused(error);
        }
        let request = LoadSession {
            session,
            cwd,
            mcp_servers,
            meta: Some(meta),
        };
        let result = client.resume_session(request).await;
        fence.await;
        AgentReply::Session {
            loaded: true,
            result,
        }
    });
}

pub(super) fn new_session(actor: &mut Actor, thread: ThreadId) {
    let started = folder(actor, thread).and_then(|cwd| Ok((cwd, session_loads(actor, thread)?)));
    let (cwd, (meta, mcp_servers)) = match started {
        Ok(started) => started,
        Err(error) => {
            launch_failed(actor, thread, &error);
            return;
        }
    };
    let clock = Arc::clone(&actor.clock);
    let matter = matter_of(actor, thread);
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref()) else {
        return;
    };
    let (id, client, fence) = (agent.id, agent.link.client.clone(), agent.link.fence());
    let check = codex::prepare::recheck(
        clock,
        matter,
        agent.codex.is_some().then(|| agent.folder.clone()),
    );
    call(actor, thread, id, async move {
        if let Err(error) = check.await {
            return AgentReply::Refused(error);
        }
        let request = NewSession {
            cwd,
            mcp_servers,
            meta: Some(meta),
        };
        let result = client.new_session(request).await;
        fence.await;
        AgentReply::Session {
            loaded: false,
            result,
        }
    });
}

/// `thread`'s matter, for the log of its `.codex` re-check.
fn matter_of(actor: &Actor, thread: ThreadId) -> Option<MatterId> {
    actor.threads.get(&thread).map(|entry| entry.row.matter)
}

/// The `_meta` and MCP servers for `thread`'s `session/new` or `session/load`, the same for
/// both: the profile hook's `_meta` with the host's switches over it, and the servers the
/// lawyer turned on, then the hook's (none when the firm's `managed-mcp.json` is present). See
/// `agent/loads.rs` and `profile_loads.rs`.
fn session_loads(
    actor: &mut Actor,
    thread: ThreadId,
) -> Result<(serde_json::Map<String, serde_json::Value>, Vec<McpServer>), Error> {
    let matter = folder(actor, thread)?;
    super::profile_loads::session_loads(actor, thread, &matter)
}

fn folder(actor: &Actor, thread: ThreadId) -> Result<PathBuf, Error> {
    let matter = actor.check_thread(thread)?.row.matter;
    Ok(crate::agent::session_folder(std::path::Path::new(
        &actor.check_matter(matter)?.row.folder_path,
    )))
}

/// The session is up. A Codex agent's session first runs [`prepare`]'s checks; a Claude
/// agent's session that started in another mode than Clauseo's is first switched to it
/// ([`claude_start`]); any other attaches now.
pub(super) fn on_started(actor: &mut Actor, thread: ThreadId, start: SessionStart, loaded: bool) {
    super::profile_loads::show_tools(actor, thread);
    let kind = actor
        .threads
        .get(&thread)
        .and_then(|entry| AgentKind::of(&entry.row.agent));
    match kind {
        Some(AgentKind::Codex) => {}
        Some(AgentKind::Claude) => return claude_start(actor, thread, start, loaded),
        None => return attach(actor, thread, start, loaded),
    }
    let ask =
        AgentChoiceId::new(codex::ASK_MODE).map_err(|error| Error::Internal(error.to_string()));
    let checks = ask.and_then(|ask| {
        let offered = start.modes.as_ref().is_some_and(|modes| {
            modes
                .available
                .iter()
                .any(|mode| mode.id == codex::ASK_MODE)
        });
        if !offered {
            return Err(Error::refused(
                ErrorCode::AgentProtocolError,
                format!(
                    "Codex offered no \"{}\" mode, which Clauseo runs it in, so no message was \
                     sent",
                    codex::ASK_MODE
                ),
            ));
        }
        let switch = start
            .modes
            .as_ref()
            .is_some_and(|modes| modes.current != codex::ASK_MODE);
        Ok((ask, switch))
    });
    let (ask, switch) = match checks {
        Ok(checks) => checks,
        Err(error) => {
            tracing::warn!(%thread, %error, "the Codex session cannot run in its ask mode");
            launch_failed(actor, thread, &error);
            return;
        }
    };
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    // Whether a hooks object went in was recorded at launch, with the folder resolved then.
    let guard = agent
        .codex
        .as_ref()
        .filter(|codex| codex.guard)
        .map(|codex| codex.folder.clone());
    if !switch && guard.is_none() {
        attach(actor, thread, start, loaded);
        return;
    }
    agent.preparing = true;
    let (id, client, fence) = (agent.id, agent.link.client.clone(), agent.link.fence());
    call(actor, thread, id, async move {
        let result = prepare(&client, start, (ask, switch), guard).await;
        fence.await;
        AgentReply::Prepared { loaded, result }
    });
}

/// A session's checks before its first prompt, in order: `session/set_mode` to `ask` when
/// `switch` (Codex's ask mode: the host owns the mode, `INITIAL_AGENT_MODE` set it at launch,
/// and this covers an adapter that did not take it; Claude's [`agent::loads::CLAUDE_MODE`],
/// see [`claude_start`]); then, for Codex, when a hooks object went into
/// `CODEX_CONFIG` (`guard` is the session's folder), `_codex/hooks/list` once, which must
/// show the guard trusted. The start comes back with the mode it now has.
async fn prepare(
    client: &AcpClient,
    mut start: SessionStart,
    (ask, switch): (AgentChoiceId, bool),
    guard: Option<PathBuf>,
) -> Result<SessionStart, Error> {
    if switch {
        client.set_mode(&start.session, ask.clone()).await?;
        if let Some(modes) = &mut start.modes {
            modes.current = CompactString::from(ask.as_str());
        }
    }
    if let Some(cwd) = guard {
        let answer = client.codex_hooks_list(&cwd).await?;
        if !codex::guard_trusted(&answer) {
            return Err(codex::guard_untrusted(&answer));
        }
    }
    Ok(start)
}

/// A Claude agent's session started in another mode than [`CLAUDE_MODE`]: claude-agent-acp
/// 0.89.1 starts it in the user's own `permissions.defaultMode` (or a resumed transcript's
/// mode), which `_meta` cannot override (see `agent/loads.rs`). So `session/set_mode` goes
/// first, before any prompt, through [`prepare`]; a failed switch fails the launch, so no
/// prompt runs in the user's mode. A session already in it attaches now. One that reports no
/// modes, or offers no such mode, fails the launch ([`claude_no_mode`]): the host could not
/// tell or change the mode it runs in. On load this also resets a mode the lawyer reached in
/// the session (see [`agent::loads::CLAUDE_MODE`]).
fn claude_start(actor: &mut Actor, thread: ThreadId, start: SessionStart, loaded: bool) {
    let Some(modes) = start.modes.as_ref() else {
        return claude_no_mode(actor, thread, "reported no modes");
    };
    if modes.current == agent::loads::CLAUDE_MODE {
        return attach(actor, thread, start, loaded);
    }
    let offered = modes
        .available
        .iter()
        .any(|mode| mode.id == agent::loads::CLAUDE_MODE);
    let mode = AgentChoiceId::new(agent::loads::CLAUDE_MODE);
    let (true, Ok(mode)) = (offered, mode) else {
        return claude_no_mode(actor, thread, "offered no \"default\" mode");
    };
    tracing::info!(%thread, from = %modes.current, "switching the Claude session to Clauseo's mode");
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    agent.preparing = true;
    let (id, client, fence) = (agent.id, agent.link.client.clone(), agent.link.fence());
    call(actor, thread, id, async move {
        let result = prepare(&client, start, (mode, true), None).await;
        fence.await;
        AgentReply::Prepared { loaded, result }
    });
}

/// Fails the launch of a Claude session whose mode the host cannot set to [`CLAUDE_MODE`]:
/// `why` says what the agent's answer lacked.
fn claude_no_mode(actor: &mut Actor, thread: ThreadId, why: &str) {
    let error = Error::refused(
        ErrorCode::AgentProtocolError,
        format!(
            "Claude Code {why}, so Clauseo cannot run it in its \"{}\" mode and sent no message",
            agent::loads::CLAUDE_MODE
        ),
    );
    tracing::warn!(%thread, %error, "the Claude session cannot run in Clauseo's mode");
    launch_failed(actor, thread, &error);
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
    agent.preparing = false;
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

/// The name the thread's header holds: from the loaded stream, else from the header the actor
/// kept when the stream was unloaded.
fn held_name(actor: &Actor, thread: ThreadId) -> Option<CompactString> {
    let entry = actor.threads.get(&thread)?;
    let header = actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .and_then(|state| state.header())
        .or(entry.header.as_ref())?;
    header.agent_name.clone()
}

/// Logs `AgentNamed` for `thread` when `name` is set and differs from the name its header holds,
/// so a restart with the same agent logs nothing. A push or send refused at once is logged and
/// the session goes on without the name. A store write that fails later is a store failure like
/// any other commit's: `turn.rs` fails the launching turn with it.
fn log_agent_name(actor: &mut Actor, thread: ThreadId, name: Option<CompactString>) {
    let Some(name) = name else {
        return;
    };
    if held_name(actor, thread).as_ref() == Some(&name) {
        return;
    }
    let logged = (|| -> Result<(), Error> {
        let mut out = Out::new(actor, thread, None)?;
        out.push(
            actor,
            AgentNamed {
                agent_name: name.clone(),
            },
        )?;
        out.send(actor, Then::Nothing)
    })();
    match logged {
        Ok(()) => {
            // An unloaded stream folds nothing, so the header the actor kept takes the name here:
            // a second start while it is unloaded then logs nothing.
            if let Some(header) = actor
                .threads
                .get_mut(&thread)
                .and_then(|entry| entry.header.as_mut())
            {
                header.agent_name = Some(name);
            }
        }
        Err(error) => tracing::warn!(%thread, %error, "the agent's name was not logged"),
    }
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
