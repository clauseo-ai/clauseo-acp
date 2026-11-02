//! The agent's asks and signals: permission requests and elicitations (logged `Live` during
//! the live turn, else answered cancelled with nothing logged) and limit signals (a rejection
//! or a stop marks the live turn; the first wins; a rate limit also feeds the limits model). A
//! permission request that names a file of the matter a program holds open (Word, say) is
//! answered for the lawyer, rejected, in the commit that logs it.

use std::path::PathBuf;

use clauseo_acp::{AcpEvent, LimitSignal, LimitStatus, Origin, PendingKey};
use clauseo_matters::HeldOpen;
use clauseo_protocol::{
    AgentChoiceId, AgentSessionId, CompactString, Deliverable, ElicitationId, ElicitationRequest,
    ElicitationRequested, ElicitationResponse, LimitStopReason, MatterPath, PermissionChoice,
    PermissionKind, PermissionOption, PermissionRequestId, PermissionRequested, PermissionResolved,
    Principal, StreamId, ThreadId, TurnId,
};

use super::super::input::MatterTag;
use super::super::{Actor, limits};
use super::out::Out;
use super::updates::{agent, commit, known_tool};
use super::{JobKind, LimitMark, Then, TurnJob, current, matter_of, spawn};
use crate::error::Error;
use crate::matter_worker::MatterJob;

/// A permission request from the agent, kept until it is logged.
#[derive(Debug, Clone)]
pub(super) struct PermissionAsk {
    pub(super) session: AgentSessionId,
    pub(super) key: PendingKey,
    pub(super) acp_tool_call_id: CompactString,
    pub(super) options: Vec<PermissionOption>,
}

/// A permission request and the turn it came in, while the matter thread checks its paths.
#[derive(Debug)]
pub(super) struct HeldOpenCheck {
    ask: PermissionAsk,
    turn: TurnId,
}

/// A permission request (`AcpEvent::PermissionRequested`). Outside the live turn it is
/// answered `cancelled`. In it, when it names a path (or, naming none, its tool call has matter
/// paths), the matter thread first resolves the paths into the matter folder and checks whether
/// a program holds one open ([`on_held_open`]); otherwise, or when the check cannot be sent, it
/// is logged `Live` for the lawyer at once.
pub(super) fn on_permission(actor: &mut Actor, thread: ThreadId, event: AcpEvent) {
    let AcpEvent::PermissionRequested {
        session,
        key,
        acp_tool_call_id,
        options,
        mut paths,
    } = event
    else {
        return;
    };
    let ask = PermissionAsk {
        session,
        key,
        acp_tool_call_id,
        options,
    };
    let Some(turn) = live_turn(actor, thread) else {
        cancel_outside_turn(actor, thread, &ask.session);
        return;
    };
    if paths.is_empty() {
        paths = tool_call_paths(actor, thread, &ask);
    }
    if paths.is_empty() {
        log_for_lawyer(actor, thread, ask);
        return;
    }
    let sent = (|| -> Result<(), Error> {
        let matter = matter_of(actor, thread)?;
        let folder = PathBuf::from(&actor.check_matter(matter)?.row.folder_path);
        let job = MatterJob::HeldOpen { folder, paths };
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
        tracing::warn!(%thread, %error, "a permission request's paths were not checked");
        log_for_lawyer(actor, thread, ask);
    }
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

/// The matter thread checked a permission request's paths. Still in the same live turn, a
/// file held open gets the request refused for the lawyer ([`refuse_held_open`]); none, and
/// the request is logged for the lawyer. Once that turn is over, that one request is answered
/// `cancelled` (the others of the session, and any later turn, are left alone).
pub(super) fn on_held_open(
    actor: &mut Actor,
    thread: ThreadId,
    check: HeldOpenCheck,
    held: Option<(MatterPath, HeldOpen)>,
) {
    let HeldOpenCheck { ask, turn } = check;
    if live_turn(actor, thread) != Some(turn) {
        if let Some(agent) = agent(actor, thread) {
            let client = agent.link.client.clone();
            spawn(actor, async move {
                // Already answered when the turn was cancelled: then nothing is pending.
                if let Err(error) = client.cancel_permission(ask.key).await {
                    tracing::debug!(%error, "a checked permission after its turn was not cancelled");
                }
            });
        }
        return;
    }
    match held {
        Some((path, held)) => {
            let why = "refused a permission request: a program holds the file open";
            tracing::info!(%thread, %path, ?held, "{why}");
            if let Err(error) = refuse_held_open(actor, thread, ask, path) {
                tracing::warn!(%thread, %error, "a refused permission request was not logged");
            }
        }
        None => log_for_lawyer(actor, thread, ask),
    }
}

/// Whether any thread whose agent has kind `kind` has a permission request or an elicitation
/// pending, for the limits model's `Asking` (no read while the agent asks). A URL elicitation
/// the lawyer accepted counts until the agent says it is complete.
pub(crate) fn asks_pending(actor: &Actor, kind: clauseo_protocol::AgentKind) -> bool {
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
        };
        commit(actor, thread, [event.into()], Vec::new())
    })();
    limits::asking_changed(actor);
    if let Err(error) = logged {
        tracing::warn!(%thread, %error, "a permission request was not logged");
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
        tracing::warn!(%thread, %error, "an elicitation was not logged");
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
