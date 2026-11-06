//! The agent's asks and signals: permission requests and elicitations (logged `Live` during
//! the live turn, else answered cancelled with nothing logged) and limit signals (a rejection
//! or a stop marks the live turn; the first wins; a rate limit also feeds the limits model). A
//! permission request that names a file of the matter a program holds open (Word, say) is
//! answered for the lawyer, rejected, in the commit that logs it. One that touches Clauseo's own
//! data folder (`agent/data_fence.rs`) is rejected with nothing logged: the lawyer never sees it.

use std::path::PathBuf;

use clauseo_acp::{AcpEvent, LimitSignal, LimitStatus, Origin, PendingKey};
use clauseo_matters::HeldOpen;
use clauseo_protocol::{
    AgentChoiceId, AgentKind, AgentSessionId, CompactString, Deliverable, ElicitationId,
    ElicitationRequest, ElicitationRequested, ElicitationResponse, LimitStopReason, MatterPath,
    PermissionAbout, PermissionChoice, PermissionKind, PermissionOption, PermissionRequestId,
    PermissionRequested, PermissionResolved, Principal, StreamId, ThreadId, TurnId,
};

use super::super::input::MatterTag;
use super::super::{Actor, limits};
use super::out::Out;
use super::updates::{agent, commit, known_tool};
use super::{JobKind, LimitMark, Then, TurnJob, current, matter_of, spawn};
use crate::agent::data_fence::{self, FenceCheck, PathsChecked};
use crate::agent::state_folder;
use crate::error::Error;
use crate::log_file::Cause;
use crate::matter_worker::MatterJob;

/// A permission request from the agent, kept until it is logged.
#[derive(Debug, Clone)]
pub(super) struct PermissionAsk {
    pub(super) session: AgentSessionId,
    pub(super) key: PendingKey,
    pub(super) acp_tool_call_id: CompactString,
    pub(super) options: Vec<PermissionOption>,
    /// What it is about, when the host can tell (`agent/plan_note.rs`).
    pub(super) about: Option<PermissionAbout>,
}

/// A permission request and the turn it came in, while the matter thread checks its paths.
#[derive(Debug)]
pub(super) struct HeldOpenCheck {
    ask: PermissionAsk,
    turn: TurnId,
}

/// A permission request (`AcpEvent::PermissionRequested`). Outside the live turn it is
/// answered `cancelled`. In it, when it names a path (or, naming none, its tool call has matter
/// paths, or its `rawInput` has a path or a command with path words), the matter thread first
/// checks the paths against Clauseo's data folder (`agent/data_fence.rs`), then resolves them
/// into the matter folder and checks whether a program holds one open ([`on_held_open`]);
/// otherwise, or when the check cannot be sent, it is logged `Live` for the lawyer at once. In
/// forced ask it first loses the options `agent::forced_ask::keep_option` drops. A Claude request
/// naming only its own plan note is marked `about: agent_plan` (`agent/plan_note.rs`), a label that
/// changes nothing else.
pub(super) fn on_permission(actor: &mut Actor, thread: ThreadId, event: AcpEvent) {
    let AcpEvent::PermissionRequested {
        session,
        key,
        acp_tool_call_id,
        mut options,
        mut paths,
        raw_input,
    } = event
    else {
        return;
    };
    // Forced ask (`agent/forced_ask.rs`): no option that would stop later asks is offered, so
    // an answer naming one is refused as one the agent did not offer.
    if agent(actor, thread).is_some_and(|agent| agent.forced_ask) {
        options.retain(crate::agent::forced_ask::keep_option);
    }
    // Read from the paths the request itself names, before any fallback to the tool call's.
    let plans = agent(actor, thread).and_then(|agent| agent.plan_notes.clone());
    let about = crate::agent::plan_note::about(plans.as_deref(), &paths, cfg!(windows));
    let ask = PermissionAsk {
        session,
        key,
        acp_tool_call_id,
        options,
        about,
    };
    let Some(turn) = live_turn(actor, thread) else {
        cancel_outside_turn(actor, thread, &ask.session);
        return;
    };
    let fence = fence_check(actor, thread, &paths, raw_input.as_ref());
    if paths.is_empty() {
        paths = tool_call_paths(actor, thread, &ask);
    }
    if paths.is_empty() && fence.is_none() {
        log_for_lawyer(actor, thread, ask);
        return;
    }
    let sent = (|| -> Result<(), Error> {
        let matter = matter_of(actor, thread)?;
        let folder = PathBuf::from(&actor.check_matter(matter)?.row.folder_path);
        let job = MatterJob::HeldOpen {
            folder,
            paths,
            fence,
        };
        let check = HeldOpenCheck {
            ask: ask.clone(),
            turn,
        };
        let tag = MatterTag::Turn(TurnJob {
            thread,
            kind: JobKind::Permission(Box::new(check)),
        });
        actor.matter_worker(matter)?.send(job, tag)?;
        actor.work.matter_jobs += 1;
        Ok(())
    })();
    if let Err(error) = sent {
        tracing::warn!(
            %thread,
            error = %Cause(&error),
            "a permission request's paths were not checked"
        );
        log_for_lawyer(actor, thread, ask);
    }
}

/// The data-folder check of a request's paths and `rawInput` (`agent/data_fence.rs`), for
/// the thread's agent: its own folders are its run folder, this matter's notes, the parts of the
/// agent's state folder for this matter that are its own (`state_folder::fence_own`, less
/// `fence_denied`), a Claude agent's plans folder, and its working folder. None when the
/// request names no path and no command, or the thread has no agent.
fn fence_check(
    actor: &Actor,
    thread: ThreadId,
    paths: &[PathBuf],
    raw_input: Option<&serde_json::Value>,
) -> Option<Box<FenceCheck>> {
    let paths = data_fence::candidates(paths, raw_input, cfg!(windows));
    let words = data_fence::candidates_words(raw_input, cfg!(windows));
    let roots = data_fence::candidates_roots(raw_input, cfg!(windows));
    if paths.is_empty() && words.is_empty() && roots.is_empty() {
        return None;
    }
    let turn = &actor.threads.get(&thread)?.turn;
    let agent = turn.agent.as_deref()?;
    let data = actor.effective.locations().data_dir().to_owned();
    let kind = AgentKind::of(&actor.threads.get(&thread)?.row.agent);
    let state = turn.run.as_ref().and_then(|run| run.state.clone());
    let mut allowed: Vec<PathBuf> = turn
        .run
        .iter()
        .flat_map(|run| [run.root.clone(), run.notes.clone()])
        .collect();
    let mut denied = Vec::new();
    // The parts of the agent's state folder for this matter (R17 PR-C) that are its own, and
    // what in them stays refused: the login link, Clauseo's files, the databases.
    if let (Some(state), Some(kind)) = (&state, kind) {
        allowed.extend(state_folder::fence_own(state, kind));
        denied = state_folder::fence_denied(state, kind);
    }
    // Claude Code writes its plan note there (`agent/plan_note.rs`); a plans folder elsewhere
    // in the data folder is not the session's.
    let plans = agent.plan_notes.iter();
    allowed.extend(
        plans
            .filter(|plans| data_fence::is_agent_plans(&data, plans))
            .cloned(),
    );
    // Codex's shell has `HOME` the state folder on macOS and Linux: `~` is that there.
    let home = match (&state, kind) {
        (Some(state), Some(AgentKind::Codex)) if cfg!(unix) => Some(state.clone()),
        _ => std::env::home_dir(),
    };
    Some(Box::new(FenceCheck {
        data,
        allowed,
        denied,
        cwd: agent.folder.clone(),
        home,
        paths,
        roots,
        words,
    }))
}

/// The matter paths the host has logged for the request's tool call (its in-matter locations
/// and diffs), as paths relative to the matter folder: Codex's adapter may send a request whose
/// `locations` are still empty.
fn tool_call_paths(actor: &Actor, thread: ThreadId, ask: &PermissionAsk) -> Vec<PathBuf> {
    let Some(loaded) = actor.loaded(StreamId::Thread(thread)) else {
        return Vec::new();
    };
    let tool = loaded.sent.tool_call(&ask.session, &ask.acp_tool_call_id);
    tool.map(|(_, tool)| {
        tool.paths
            .iter()
            .map(|path| PathBuf::from(path.as_str()))
            .collect()
    })
    .unwrap_or_default()
}

/// The matter thread checked a permission request's paths. Still in the same live turn, a path
/// in Clauseo's data folder gets the request rejected unseen ([`refuse_data_folder`]), a file
/// held open gets it refused for the lawyer ([`refuse_held_open`]); neither, and the request is
/// logged for the lawyer. Once that turn is over, that one request is answered
/// `cancelled` (the others of the session, and any later turn, are left alone).
pub(super) fn on_held_open(
    actor: &mut Actor,
    thread: ThreadId,
    check: HeldOpenCheck,
    checked: PathsChecked,
) {
    let HeldOpenCheck { ask, turn } = check;
    if live_turn(actor, thread) != Some(turn) {
        if let Some(agent) = agent(actor, thread) {
            let client = agent.link.client.clone();
            spawn(actor, async move {
                // Already answered when the turn was cancelled: then nothing is pending.
                if let Err(error) = client.cancel_permission(ask.key).await {
                    tracing::debug!(
                        %error,
                        "a checked permission after its turn was not cancelled"
                    );
                }
            });
        }
        return;
    }
    let held = match checked {
        PathsChecked::DataFolder => {
            refuse_data_folder(actor, thread, &ask);
            return;
        }
        PathsChecked::Held(held) => held,
    };
    match held {
        Some((path, held)) => {
            let why = "refused a permission request: a program holds the file open";
            let held = match held {
                HeldOpen::OwnerFile(_) => "owner_file",
                HeldOpen::SharingViolation => "sharing_violation",
            };
            tracing::info!(%thread, held, "{why}");
            if let Err(error) = refuse_held_open(actor, thread, ask, path) {
                tracing::warn!(
                    %thread,
                    error = %Cause(&error),
                    "a refused permission request was not logged"
                );
            }
        }
        None => log_for_lawyer(actor, thread, ask),
    }
}

/// Whether any thread whose agent has kind `kind` has a permission request or an elicitation
/// pending, for the limits model's `Asking` (no read while the agent asks). A URL elicitation
/// the lawyer accepted counts until the agent says it is complete.
pub(crate) fn asks_pending(actor: &Actor, kind: AgentKind) -> bool {
    actor.threads.values().any(|entry| {
        entry.turn.agent.as_deref().is_some_and(|agent| {
            !(agent.permissions.is_empty()
                && agent.elicitations.is_empty()
                && agent.url_elicitations.is_empty())
                && clauseo_protocol::AgentKind::of(&entry.row.agent) == Some(kind)
        })
    })
}

/// The live turn of `thread`, if one is.
fn live_turn(actor: &mut Actor, thread: ThreadId) -> Option<TurnId> {
    current(actor, thread)
        .filter(|current| current.live())
        .map(|current| current.turn)
}

/// Answers the session's pending permission requests `cancelled`, outside the live turn.
fn cancel_outside_turn(actor: &mut Actor, thread: ThreadId, session: &AgentSessionId) {
    if let Some(agent) = agent(actor, thread) {
        let client = agent.link.client.clone();
        let session = session.clone();
        spawn(actor, async move {
            // `cancel` answers every pending permission of the session `cancelled`.
            if let Err(error) = client.cancel(&session).await {
                tracing::debug!(%error, "a permission outside a turn was not cancelled");
            }
        });
    }
}

/// Logs the request `Live`; the lawyer's answer goes to the agent.
fn log_for_lawyer(actor: &mut Actor, thread: ThreadId, ask: PermissionAsk) {
    let PermissionAsk {
        session,
        key,
        acp_tool_call_id,
        options,
        about,
    } = ask;
    let tool_call = known_tool(actor, thread, &session, &acp_tool_call_id);
    let logged = (|| -> Result<(), Error> {
        let request: PermissionRequestId = actor.ids.mint(actor.clock.now().wall)?;
        if let Some(agent) = agent(actor, thread) {
            agent.permissions.insert(request, key);
        }
        let event = PermissionRequested {
            request,
            tool_call,
            options,
            deliverable: Deliverable::Live,
            about,
        };
        commit(actor, thread, [event.into()], Vec::new())
    })();
    limits::asking_changed(actor);
    if let Err(error) = logged {
        tracing::warn!(%thread, error = %Cause(&error), "a permission request was not logged");
    }
}

/// One commit of the request and its resolution for the lawyer: the reject-once option, else
/// `Cancelled`, with `held_open` naming `path`. The agent hears the answer once the commit is
/// durable, and the lawyer is never asked.
fn refuse_held_open(
    actor: &mut Actor,
    thread: ThreadId,
    ask: PermissionAsk,
    path: MatterPath,
) -> Result<(), Error> {
    let PermissionAsk {
        session,
        key,
        acp_tool_call_id,
        options,
        about,
    } = ask;
    let tool_call = known_tool(actor, thread, &session, &acp_tool_call_id);
    let agent_id = agent(actor, thread)
        .map(|agent| agent.id)
        .ok_or_else(|| super::lost("the agent of a refused permission request"))?;
    let reject = reject_option(&options)
        .and_then(|option| Some((option, AgentChoiceId::new(&option.option_id).ok()?)));
    let (choice, then) = match reject {
        Some((option, id)) => (
            PermissionChoice::Selected {
                option_id: option.option_id.clone(),
                kind: option.kind,
            },
            Then::Permission {
                agent: agent_id,
                key,
                option: id,
            },
        ),
        None => (
            PermissionChoice::Cancelled,
            Then::CancelPermission {
                agent: agent_id,
                key,
            },
        ),
    };
    let request: PermissionRequestId = actor.ids.mint(actor.clock.now().wall)?;
    let mut out = Out::new(actor, thread, None)?;
    out.push(
        actor,
        PermissionRequested {
            request,
            tool_call,
            options,
            deliverable: Deliverable::Live,
            about,
        },
    )?;
    out.push(
        actor,
        PermissionResolved {
            request,
            choice,
            by: Principal::LocalDesktop,
            held_open: Some(path),
        },
    )?;
    out.send(actor, then)?;
    if let Some(agent) = agent(actor, thread) {
        agent.run = None;
    }
    Ok(())
}

/// Rejects a request that touches Clauseo's data folder, with nothing logged and the lawyer never
/// asked: the agent gets the reject-once option, else `cancelled`, and the agent's count for its
/// `agent ended` line goes up. No path is logged. ACP's answer carries no reason, so the agent
/// hears its adapter's own words for a refusal.
fn refuse_data_folder(actor: &mut Actor, thread: ThreadId, ask: &PermissionAsk) {
    let reject =
        reject_option(&ask.options).and_then(|option| AgentChoiceId::new(&option.option_id).ok());
    let Some(agent) = agent(actor, thread) else {
        return;
    };
    agent.denied_data_folder = agent.denied_data_folder.saturating_add(1);
    let client = agent.link.client.clone();
    let key = ask.key;
    spawn(actor, async move {
        let answered = match reject {
            Some(option) => client.answer_permission(key, option).await,
            None => client.cancel_permission(key).await,
        };
        if let Err(error) = answered {
            tracing::warn!(error = %Cause(&error), "a data-folder refusal did not reach the agent");
        }
    });
}

/// The option that rejects this request only: the first `RejectOnce`. A `RejectAlways` is
/// never taken, since it may leave a lasting rule in the agent the lawyer never chose.
fn reject_option(options: &[PermissionOption]) -> Option<&PermissionOption> {
    options
        .iter()
        .find(|option| option.kind == PermissionKind::RejectOnce)
}

/// An elicitation: logged during the live turn, else answered `Cancel`.
pub(super) fn on_elicitation(
    actor: &mut Actor,
    thread: ThreadId,
    _session: &AgentSessionId,
    key: PendingKey,
    elicitation_id: Option<CompactString>,
    request: ElicitationRequest,
) {
    let live = current(actor, thread).is_some_and(|current| current.live());
    if !live {
        if let Some(agent) = agent(actor, thread) {
            let client = agent.link.client.clone();
            spawn(actor, async move {
                if let Err(error) = client
                    .answer_elicitation(key, ElicitationResponse::Cancel)
                    .await
                {
                    tracing::debug!(%error, "an elicitation outside a turn was not cancelled");
                }
            });
        }
        return;
    }
    let logged = (|| -> Result<(), Error> {
        let elicitation: ElicitationId = actor.ids.mint(actor.clock.now().wall)?;
        if let Some(agent) = agent(actor, thread) {
            agent.elicitations.insert(elicitation, key);
            if let Some(acp) = elicitation_id {
                agent.url_elicitations.insert(acp, elicitation);
            }
        }
        let event = ElicitationRequested {
            elicitation,
            request,
        };
        commit(actor, thread, [event.into()], Vec::new())
    })();
    limits::asking_changed(actor);
    if let Err(error) = logged {
        tracing::warn!(%thread, error = %Cause(&error), "an elicitation was not logged");
    }
}

/// A limit signal: a rejection or a stop during the turn marks it (the first wins); anything
/// else goes to `tracing`. A rate limit, in a turn or outside one, also goes to the limits
/// model as a live signal, unless it is history replayed by `session/load`.
pub(super) fn on_limit(
    actor: &mut Actor,
    thread: ThreadId,
    session: &AgentSessionId,
    origin: Origin,
    signal: &LimitSignal,
) {
    if !matches!(origin, Origin::Replay) {
        limits::live(actor, thread, signal);
    }
    let mark = limit_mark(signal);
    let turn = match origin {
        Origin::Turn(turn) => Some(turn),
        Origin::Late(_) | Origin::Replay | Origin::Idle => None,
    };
    let current = current(actor, thread).filter(|current| Some(current.turn) == turn);
    if let (Some(current), Some(mark)) = (current, mark) {
        if current.limit.is_none() {
            current.limit = Some(mark);
        }
    } else {
        tracing::info!(%thread, %session, ?signal, "a limit signal");
    }
}

/// The limit stop a signal means, if it stops the turn.
pub(super) fn limit_mark(signal: &LimitSignal) -> Option<LimitMark> {
    match signal {
        LimitSignal::RateLimit {
            window,
            status,
            resets_at,
            ..
        } => match status {
            LimitStatus::Rejected => Some(LimitMark {
                window: window.clone(),
                resets_at: *resets_at,
                reason: LimitStopReason::PlanWindow,
            }),
            LimitStatus::Allowed | LimitStatus::Warning => None,
        },
        LimitSignal::Stopped { reason, .. } => Some(LimitMark {
            window: None,
            resets_at: None,
            reason: *reason,
        }),
    }
}
