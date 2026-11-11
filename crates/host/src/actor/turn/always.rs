//! The turn path's half of a Claude "Always for this matter" (route A; `agent/always.rs` has
//! why and the rules). Only for a Claude agent outside forced ask; forced ask
//! (`agent/forced_ask.rs`) offers no `allow_always` option at all, and keeps and applies nothing.
//!
//! - [`ask`]: a request that comes in gets its rule and its first `allow_once` option, unless
//!   it is a plan exit (an `exit-plan-*` option, or the `ExitPlanMode` tool), which passes
//!   through unchanged. One with an `allow_always` option but no `allow_once` loses its
//!   `allow_always` options, so none can reach the adapter.
//! - [`answer`]: the lawyer's `allow_always` answer is logged as chosen, but the agent gets the
//!   request's `allow_once` option, so claude-agent-acp writes nothing into the matter; the
//!   rule, when the request gives one, is kept for the session and in the matter's file.
//! - [`auto_allow`]: a later request whose rule is kept is answered `allow_once` by the host,
//!   after the data-fence and held-open checks, logged as `PermissionRequested` and
//!   `PermissionResolved` in one commit, as `asks::refuse_held_open` logs its answer. The answer
//!   is `by: local_desktop`, the only principal there is: it applies the lawyer's own standing
//!   choice from this desktop app, as the held-open refusal answers for the lawyer.
//! - [`allow_in`]: a later launch's `_meta` carries the kept rules in `permissions.allow`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use clauseo_protocol::{
    AgentChoiceId, AgentKind, AgentSessionId, Deliverable, PermissionChoice, PermissionKind,
    PermissionOption, PermissionRequestId, PermissionRequested, PermissionResolved, Principal,
    StreamId, ThreadId,
};
use serde_json::{Map, Value};

use super::super::Actor;
use super::Then;
use super::agents::Agent;
use super::asks::PermissionAsk;
use super::launch_checks::RunFolders;
use super::out::Out;
use super::updates::{agent, known_tool};
use crate::agent::always;
use crate::error::Error;
use crate::log_file::Cause;

/// What a request needs for route A: its rule, if it gives one, and the option the agent gets.
#[derive(Debug, Clone)]
pub(super) struct AlwaysAsk {
    rule: Option<String>,
    once: AgentChoiceId,
}

/// A Claude agent's kept rules for its matter, and its pending requests' [`AlwaysAsk`].
#[derive(Debug, Default)]
pub(super) struct Always {
    /// The matter's rules file; none for any agent but Claude outside forced ask.
    file: Option<PathBuf>,
    rules: BTreeSet<String>,
    asked: BTreeMap<PermissionRequestId, AlwaysAsk>,
}

impl Always {
    /// For a launch of an agent of `kind`: the rules its prep read (`RunFolders::always`), for
    /// Claude outside forced ask only.
    pub(super) fn at_launch(run: &RunFolders, kind: Option<AgentKind>, forced_ask: bool) -> Self {
        if kind != Some(AgentKind::Claude) || forced_ask {
            return Self::default();
        }
        Self {
            file: always::file_beside(&run.notes),
            rules: run.always.iter().cloned().collect(),
            asked: BTreeMap::new(),
        }
    }
}

/// The plan exit's option ids start so (`agent/forced_ask.rs` `SKIPPING_OPTIONS`).
const PLAN_EXIT_OPTION: &str = "exit-plan-";

/// Claude Code's plan exit tool.
const PLAN_EXIT_TOOL: &str = "ExitPlanMode";

/// A request's [`AlwaysAsk`] (see the module doc); `options` may lose its `allow_always` ones.
pub(super) fn ask(
    actor: &mut Actor,
    thread: ThreadId,
    session: &AgentSessionId,
    acp_tool_call_id: &str,
    options: &mut Vec<PermissionOption>,
    raw_input: Option<&Value>,
) -> Option<AlwaysAsk> {
    agent(actor, thread)?.always.file.as_ref()?;
    let plan_exit = options.iter().any(|option| {
        let id = option.option_id.as_bytes();
        id.get(..PLAN_EXIT_OPTION.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(PLAN_EXIT_OPTION.as_bytes()))
    });
    let tool = tool_name(actor, thread, session, acp_tool_call_id);
    if plan_exit || tool.as_deref() == Some(PLAN_EXIT_TOOL) {
        return None;
    }
    let once = options
        .iter()
        .find(|option| option.kind == PermissionKind::AllowOnce)
        .and_then(|option| AgentChoiceId::new(&option.option_id).ok());
    let Some(once) = once else {
        options.retain(|option| option.kind != PermissionKind::AllowAlways);
        return None;
    };
    let rule = tool.and_then(|tool| always::rule(&tool, raw_input, cfg!(windows)));
    Some(AlwaysAsk { rule, once })
}

/// The tool `name` of the call the request is about, as the thread's log (or the replay being
/// loaded) has it.
fn tool_name(
    actor: &mut Actor,
    thread: ThreadId,
    session: &AgentSessionId,
    acp_id: &str,
) -> Option<String> {
    let logged = actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.sent.tool_call(session, acp_id))
        .and_then(|(_, tool)| tool.tool_name.as_deref().map(str::to_owned));
    logged.or_else(|| {
        let replay = agent(actor, thread)?.replay.as_ref()?;
        let (_, tool) = replay.tool_call(session, acp_id)?;
        tool.tool_name.as_deref().map(str::to_owned)
    })
}

/// The request was logged for the lawyer as `request`: its [`AlwaysAsk`] waits for the answer.
pub(super) fn asked(agent: &mut Agent, request: PermissionRequestId, ask: Option<AlwaysAsk>) {
    let permissions = &agent.permissions;
    agent
        .always
        .asked
        .retain(|id, _| permissions.contains_key(id));
    if let Some(ask) = ask {
        agent.always.asked.insert(request, ask);
    }
}

/// The lawyer answered `request` with `option`: for an `allow_always` one with an
/// [`AlwaysAsk`], the option the agent gets instead, its rule kept for the session and stored.
pub(super) fn answer(
    agent: &mut Agent,
    request: PermissionRequestId,
    option: &PermissionOption,
) -> Option<AgentChoiceId> {
    let ask = agent.always.asked.remove(&request)?;
    if option.kind != PermissionKind::AllowAlways {
        return None;
    }
    if let Some(rule) = ask.rule
        && agent.always.rules.len() < always::MAX_RULES
        && agent.always.rules.insert(rule.clone())
        && let Some(file) = &agent.always.file
    {
        always::store(file.clone(), rule);
    }
    tracing::info!("an \"Always for this matter\" answer goes to the agent as allow once");
    Some(ask.once)
}

/// Answers `ask` `allow_once` when its rule is kept, logged as one commit; gives it back when
/// it is not, for the lawyer.
pub(super) fn auto_allow(
    actor: &mut Actor,
    thread: ThreadId,
    ask: PermissionAsk,
) -> Option<PermissionAsk> {
    let kept = ask.always.as_ref().and_then(|always| always.rule.as_ref());
    let agent = agent(actor, thread)?;
    if !kept.is_some_and(|rule| agent.always.rules.contains(rule)) {
        return Some(ask);
    }
    if let Err(error) = allow(actor, thread, ask) {
        tracing::warn!(%thread, error = %Cause(&error), "a kept rule's answer was not logged");
    }
    None
}

fn allow(actor: &mut Actor, thread: ThreadId, ask: PermissionAsk) -> Result<(), Error> {
    let PermissionAsk {
        session,
        key,
        acp_tool_call_id,
        options,
        about,
        always,
    } = ask;
    let once = always
        .ok_or_else(|| super::lost("a kept rule's option"))?
        .once;
    let tool_call = known_tool(actor, thread, &session, &acp_tool_call_id);
    let agent_id = agent(actor, thread)
        .map(|agent| agent.id)
        .ok_or_else(|| super::lost("the agent of a kept rule's request"))?;
    let kind = PermissionKind::AllowOnce;
    let choice = PermissionChoice::Selected {
        option_id: once.as_str().into(),
        kind,
    };
    let request: PermissionRequestId = actor.ids.mint(actor.clock.now().wall)?;
    let mut out = Out::new(actor, thread, None)?;
    let deliverable = Deliverable::Live;
    out.push(
        actor,
        PermissionRequested {
            request,
            tool_call,
            options,
            deliverable,
            about,
        },
    )?;
    let by = Principal::LocalDesktop;
    let resolved = PermissionResolved {
        request,
        choice,
        by,
        held_open: None,
    };
    out.push(actor, resolved)?;
    let then = Then::Permission {
        agent: agent_id,
        key,
        option: once,
    };
    out.send(actor, then)?;
    if let Some(agent) = agent(actor, thread) {
        agent.run = None;
    }
    tracing::info!(%thread, "a request matching a kept rule was allowed once");
    Ok(())
}

/// Adds `thread`'s agent's kept rules to the `_meta` of its session (none in forced ask).
pub(super) fn allow_in(actor: &mut Actor, thread: ThreadId, meta: &mut Map<String, Value>) {
    if let Some(agent) = agent(actor, thread).filter(|agent| !agent.forced_ask) {
        always::allow_in_meta(meta, agent.always.rules.iter().map(String::as_str));
    }
}
