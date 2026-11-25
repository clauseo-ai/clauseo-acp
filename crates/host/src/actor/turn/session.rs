//! A launched agent's session: `session/load` with the stored session id when the agent offers
//! it (after reading the thread's whole log for replay dedupe), else `session/resume` when it
//! offers that (nothing is replayed), else `session/new`; for a Codex agent the checks before
//! its first prompt (its mode, the guard's trust), for a Claude agent the switch to
//! Clauseo's mode, for any agent the model and effort picks (`picks.rs`); then the attach
//! commit, the held replay and the work that waited. When `initialize` gave the agent a title,
//! its name is logged first (`AgentNamed`, `session/named.rs`), so a failed `session/new`
//! leaves it in the thread. A Codex agent with a key that answers a session call with
//! auth-required gets its one `authenticate` first (`key_retry.rs`).

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_acp::{AgentInfo, LoadSession, McpServer, NewSession, SessionStart};
use clauseo_protocol::{
    AgentChoiceId, AgentKind, AgentSessionAttached, ConfigOptionsReplaced, Envelope, ErrorCode,
    MatterId, ModeChanged, Notice, NoticeKind, NoticeLevel, StreamId, ThreadId, ViewEntry,
};

use super::super::Actor;
use super::super::input::Input;
use super::agents::{Stage, call, launch_failed, run_waiting};
use super::key_retry;
use super::out::Out;
use super::{AgentReply, LogFor, Then, TurnInput, lost, state, updates};
use crate::agent::{self, codex, launch_settings};
use crate::error::Error;
use crate::log_file::{Cause, LIFECYCLE};
use crate::sent::SentView;

mod claude_mode;
mod named;
mod prepare;

use claude_mode::claude_start;
pub(super) use claude_mode::{Auto, hold_auto_back};
pub(super) use prepare::on_prepared;
use prepare::{Switch, before_attach};

/// The fallback words of the notice a thread gets when its agent starts a new session after
/// earlier turns (`NoticeKind::ContextNotRestored`), for a view that does not know the kind.
const CONTEXT_NOT_RESTORED: &str = "Clauseo doesn't remember the earlier questions and answers \
     in this conversation. Include what it needs from them in your next question.";

pub(super) fn on_initialized(actor: &mut Actor, thread: ThreadId, info: AgentInfo) {
    tracing::info!(target: LIFECYCLE, %thread, "agent initialized");
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
    named::log_agent_name(actor, thread, name);
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
    if let Err(error) = &loads {
        // A launch refused here (forced ask without its guard) ends as a failed launch, not a
        // load that never goes out.
        let loading = state(actor, thread)
            .and_then(|state| state.agent.as_deref())
            .is_some_and(|agent| agent.stage == Stage::Loading);
        if loading {
            launch_failed(actor, thread, error);
            return;
        }
    }
    let clock = Arc::clone(&actor.clock);
    let matter = matter_of(actor, thread);
    let retry = key_retry::for_call(actor, thread);
    let settings = settings_target(actor, thread);
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    let (Some(session), Ok(cwd), Ok((meta, mcp_servers, additional_directories)), Stage::Loading) =
        (session, cwd, loads, agent.stage)
    else {
        return;
    };
    match result {
        Ok(log) => agent.replay = Some(SentView::rebuild(StreamId::Thread(thread), &log)),
        Err(error) => tracing::warn!(%thread, error = %Cause(&error), "no log for replay dedupe"),
    }
    agent.mcp_commands = commands(&mcp_servers);
    let (id, client, fence) = (agent.id, agent.link.client.clone(), agent.link.fence());
    let check = codex::prepare::recheck(
        Arc::clone(&clock),
        matter,
        agent.codex.is_some().then(|| agent.folder.clone()),
    );
    let ready = launch_settings::ready(check, clock, matter, settings, meta);
    call(actor, thread, id, async move {
        let meta = match ready.await {
            Ok(meta) => meta,
            Err(error) => return AgentReply::Refused(error),
        };
        let request = LoadSession {
            session,
            cwd,
            additional_directories,
            mcp_servers,
            meta: Some(meta),
        };
        let result = retry
            .run(&client, || client.load_session(request.clone()))
            .await;
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
    let (Some(session), Ok((cwd, (meta, mcp_servers, additional_directories)))) =
        (session, started)
    else {
        new_session(actor, thread);
        return;
    };
    let clock = Arc::clone(&actor.clock);
    let matter = matter_of(actor, thread);
    let retry = key_retry::for_call(actor, thread);
    let settings = settings_target(actor, thread);
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    agent.mcp_commands = commands(&mcp_servers);
    let (id, client, fence) = (agent.id, agent.link.client.clone(), agent.link.fence());
    let check = codex::prepare::recheck(
        Arc::clone(&clock),
        matter,
        agent.codex.is_some().then(|| agent.folder.clone()),
    );
    let ready = launch_settings::ready(check, clock, matter, settings, meta);
    call(actor, thread, id, async move {
        let meta = match ready.await {
            Ok(meta) => meta,
            Err(error) => return AgentReply::Refused(error),
        };
        let request = LoadSession {
            session,
            cwd,
            additional_directories,
            mcp_servers,
            meta: Some(meta),
        };
        let result = retry
            .run(&client, || client.resume_session(request.clone()))
            .await;
        fence.await;
        AgentReply::Session {
            loaded: true,
            result,
        }
    });
}

pub(super) fn new_session(actor: &mut Actor, thread: ThreadId) {
    let started = folder(actor, thread).and_then(|cwd| Ok((cwd, session_loads(actor, thread)?)));
    let (cwd, (meta, mcp_servers, additional_directories)) = match started {
        Ok(started) => started,
        Err(error) => {
            launch_failed(actor, thread, &error);
            return;
        }
    };
    let clock = Arc::clone(&actor.clock);
    let matter = matter_of(actor, thread);
    let retry = key_retry::for_call(actor, thread);
    let settings = settings_target(actor, thread);
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    agent.mcp_commands = commands(&mcp_servers);
    let (id, client, fence) = (agent.id, agent.link.client.clone(), agent.link.fence());
    let check = codex::prepare::recheck(
        Arc::clone(&clock),
        matter,
        agent.codex.is_some().then(|| agent.folder.clone()),
    );
    let ready = launch_settings::ready(check, clock, matter, settings, meta);
    call(actor, thread, id, async move {
        let meta = match ready.await {
            Ok(meta) => meta,
            Err(error) => return AgentReply::Refused(error),
        };
        let request = NewSession {
            cwd,
            additional_directories,
            mcp_servers,
            meta: Some(meta),
        };
        let result = retry
            .run(&client, || client.new_session(request.clone()))
            .await;
        fence.await;
        AgentReply::Session {
            loaded: false,
            result,
        }
    });
}

/// Where `thread`'s run writes Claude's settings for its session calls.
fn settings_target(actor: &Actor, thread: ThreadId) -> Option<launch_settings::Target> {
    let run = actor.threads.get(&thread)?.turn.run.as_ref()?;
    Some(run.launch_settings.clone())
}

/// `thread`'s matter, for the log of its `.codex` re-check.
fn matter_of(actor: &Actor, thread: ThreadId) -> Option<MatterId> {
    actor.threads.get(&thread).map(|entry| entry.row.matter)
}

/// The `_meta`, MCP servers and extra roots for `thread`'s `session/new` or `session/load`,
/// the same for both: the profile hook's `_meta` with the host's switches over it, the servers
/// the lawyer turned on, then the hook's (none when the firm's `managed-mcp.json` is present),
/// and Codex's `additionalDirectories`. See `agent/loads.rs` and `profile_loads.rs`.
fn session_loads(
    actor: &mut Actor,
    thread: ThreadId,
) -> Result<super::profile_loads::SessionLoads, Error> {
    let matter = folder(actor, thread)?;
    super::profile_loads::session_loads(actor, thread, &matter)
}

/// The program of each stdio MCP server, which a cancelled turn's sweep leaves running. An
/// http server runs nowhere on this computer, so it has none.
fn commands(servers: &[McpServer]) -> Vec<PathBuf> {
    servers
        .iter()
        .filter_map(|server| match server {
            McpServer::Stdio { command, .. } => Some(command.clone()),
            McpServer::Http { .. } => None,
        })
        .collect()
}

pub(super) fn folder(actor: &Actor, thread: ThreadId) -> Result<PathBuf, Error> {
    let matter = actor.check_thread(thread)?.row.matter;
    Ok(crate::agent::session_folder(std::path::Path::new(
        &actor.check_matter(matter)?.row.folder_path,
    )))
}

/// The session is up. A Codex agent's session first runs [`prepare`]'s checks; a Claude
/// agent's session that started in another mode than Clauseo's is first switched to it
/// ([`claude_start`]); a session with model or effort picks to set takes them there too
/// (`picks.rs`); any other attaches now.
pub(super) fn on_started(actor: &mut Actor, thread: ThreadId, start: SessionStart, loaded: bool) {
    super::profile_loads::show_tools(actor, thread);
    actor.agent_options.started(thread, &start.config_options);
    if !start.config_options.is_empty() {
        super::picks::drop_held(actor, thread, "the session's answer is newer");
    }
    super::picks::own::seed(actor, thread);
    let picks = super::picks::wanted(actor, thread, &start.config_options);
    let kind = actor
        .threads
        .get(&thread)
        .and_then(|entry| AgentKind::of(&entry.row.agent));
    match kind {
        Some(AgentKind::Codex) => {}
        Some(AgentKind::Claude) => return claude_start(actor, thread, (start, loaded), picks),
        None if picks.is_empty() => return attach(actor, thread, start, loaded),
        None => return before_attach(actor, thread, (start, loaded), (None, None, picks)),
    }
    // Codex's mode, or its fallback once the agent's account refused the reviewer.
    let wanted = super::codex_reviewer::mode(actor, thread);
    let mode = AgentChoiceId::new(wanted).map_err(|error| Error::Internal(error.to_string()));
    let checks = mode.and_then(|mode| {
        let offered = start
            .modes
            .as_ref()
            .is_some_and(|modes| modes.available.iter().any(|offered| offered.id == wanted));
        if !offered {
            return Err(Error::refused(
                ErrorCode::AgentModeMissing,
                format!(
                    "Codex offered no \"{wanted}\" mode, which Clauseo runs it in, so no message \
                     was sent"
                ),
            ));
        }
        let switch = start
            .modes
            .as_ref()
            .is_some_and(|modes| modes.current != wanted);
        Ok((mode, switch))
    });
    let (mode, switch) = match checks {
        Ok(checks) => checks,
        Err(error) => {
            tracing::warn!(
                %thread,
                error = %Cause(&error),
                "the Codex session cannot run in its mode"
            );
            launch_failed(actor, thread, &error);
            return;
        }
    };
    // Whether a hooks object went in was recorded at launch, with the folder resolved then.
    let guard = state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .and_then(|agent| agent.codex.as_ref())
        .filter(|codex| codex.guard)
        .map(|codex| codex.folder.clone());
    if !switch && guard.is_none() && picks.is_empty() {
        attach(actor, thread, start, loaded);
        return;
    }
    let mode = switch.then_some(Switch {
        mode,
        fallback: None,
    });
    before_attach(actor, thread, (start, loaded), (mode, guard, picks));
}

/// The session is up: one commit of `AgentSessionAttached`, the thread row, the session's mode
/// and options (and the "context not restored" notice), then the held replay and the waiting
/// work.
pub(super) fn attach(actor: &mut Actor, thread: ThreadId, start: SessionStart, loaded: bool) {
    // A new thread's first session: what it runs with becomes its kept values (`picks/own.rs`).
    super::picks::own::seed_first(actor, thread, &start.config_options);
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
    tracing::info!(target: LIFECYCLE, %thread, loaded, "session attached");
    agent.session = Some(start.session);
    agent.stage = Stage::Attached;
    agent.preparing = false;
    let replayed = std::mem::take(&mut agent.replayed);
    let waiting = std::mem::take(&mut agent.waiting);
    super::picks::released(thread, &replayed);
    for event in replayed {
        updates::on_event(actor, thread, Box::new(event));
    }
    if let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) {
        agent.replay = None;
    }
    for item in waiting {
        if let Some(item) = super::picks::waited(actor, &start.config_options, item) {
            run_waiting(actor, thread, item);
        }
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
    // A switch's new session has the handover instead (`switch.rs`).
    let notice = !loaded
        && has_earlier_turns(actor, thread)
        && !super::switch::started_by_switch(actor, thread);
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
                kind: Some(NoticeKind::ContextNotRestored),
            },
        )?;
    }
    if let Some(notice) = claude_mode::notice(actor, thread, &start.config_options) {
        out.push(actor, notice)?;
    }
    if let Some(notice) = super::profile_loads::claude::notice(actor, thread) {
        out.push(actor, notice)?;
    }
    if let Some(notice) = super::codex_reviewer::notice(actor, thread) {
        out.push(actor, notice)?;
    }
    // The skill packs the launch left out, each pack and reason told once per thread (0040).
    for notice in super::launch_checks::pack_notices(actor, thread) {
        out.push(actor, notice)?;
    }
    let now = actor.now().wall;
    let entry = actor
        .threads
        .get_mut(&thread)
        .ok_or_else(|| lost("a thread"))?;
    entry.row.acp_session_id = Some(start.session.clone());
    entry.logged_by = Some(entry.row.agent.clone());
    entry.row.updated = now;
    out.draft.thread_row(entry.row.clone());
    // A switch back to an agent that loads its own session: the handover, now that the load
    // answered, last, so nothing after it can fail the commit (`switch/resume.rs`).
    let handoff = super::switch::attached(actor, thread, (&start.session, loaded), &mut out)?;
    let sent = out.send(actor, handoff.map_or(Then::Nothing, Then::Attached));
    if sent.is_err()
        && let Some(handoff) = handoff
    {
        super::switch::unlogged(actor, thread, handoff);
    }
    sent
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
                | ViewEntry::Helper(_)
                | ViewEntry::HelperText(_)
                | ViewEntry::Permission(_)
                | ViewEntry::Elicitation(_)
                | ViewEntry::LimitStop(_)
                | ViewEntry::Notice(_)
                | ViewEntry::Marker(_)
                | ViewEntry::MatterHeader(_)
                | ViewEntry::Version(_)
                | ViewEntry::Handoff(_) => false,
            })
        })
}
