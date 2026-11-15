//! The turn path's half of a Claude "Always for this matter" (route A; `agent/always.rs` has
//! why and the rules). Only for a Claude agent outside forced ask; forced ask
//! (`agent/forced_ask.rs`) offers no `allow_always` option at all, and keeps and applies nothing.
//!
//! - [`ask`]: a request that comes in gets its rule (or why none) and its first `allow_once`
//!   option, unless it is a plan exit (an `exit-plan-*` option, or the `ExitPlanMode` tool),
//!   which passes through unchanged. One with an `allow_always` option but no `allow_once` loses
//!   its `allow_always` options, so none can reach the adapter. A path rule's file goes with the
//!   request's matter-thread check ([`real_path`]), which resolves its real path
//!   (`agent::always::real_within`).
//! - [`answer`]: the lawyer's `allow_always` answer is logged as chosen, but the agent gets the
//!   request's `allow_once` option, so claude-agent-acp writes nothing into the matter; the
//!   rule is kept for the session and in the matter's file ([`keep`], numbered by the actor's
//!   `kept::Ledger`), unless it is a path rule outside the matter folder by its text
//!   (`agent::always::within`) or its real path. What was not kept, and why, goes back for the
//!   lawyer's notice ([`Answered`]).
//! - [`auto_allow`]: a later request whose rule is kept is answered `allow_once` by the host,
//!   after the data-fence and held-open checks, logged as `PermissionRequested` and
//!   `PermissionResolved` in one commit, as `asks::refuse_held_open` logs its answer; a path
//!   rule only when its real path is inside the matter now. The answer is `by: local_desktop`,
//!   the only principal there is: it applies the lawyer's own standing choice from this desktop
//!   app, as the held-open refusal answers for the lawyer.
//!
//! The kept rules reach no launch settings: Claude Code asks the host for every request, and
//! only the host answers from them (`agent/always.rs`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use clauseo_protocol::{
    AgentChoiceId, AgentKind, AgentSessionId, Deliverable, PermissionChoice, PermissionKind,
    PermissionOption, PermissionRequestId, PermissionRequested, PermissionResolved, Principal,
    StreamId, ThreadId,
};
use serde_json::Value;

use super::super::Actor;
use super::super::kept::Ledger;
use super::Then;
use super::agents::Agent;
use super::asks::PermissionAsk;
use super::launch_checks::RunFolders;
use super::out::Out;
use super::updates::{agent, known_tool};
use crate::agent::always::{self, NotKept};
use crate::error::Error;
use crate::log_file::Cause;

/// What a request needs for route A: its rule, if it gives one, and the option the agent gets.
#[derive(Debug, Clone)]
pub(super) struct AlwaysAsk {
    rule: Result<String, NotKept>,
    once: AgentChoiceId,
    /// Whether a path rule's file is inside the matter by its real path, as the request's
    /// matter-thread check found it; none when it was not checked.
    real: Option<bool>,
}

/// The lawyer's `allow_always` answer as the agent gets it: the `allow_once` option, and why
/// nothing was kept, when nothing was.
#[derive(Debug)]
pub(super) struct Answered {
    pub(super) once: AgentChoiceId,
    pub(super) not_kept: Option<NotKept>,
    /// The rule newly kept and its matter's file, for [`keep`] to store.
    pub(super) keep: Option<(PathBuf, String)>,
}

/// A Claude agent's kept rules for its matter, and its pending requests' [`AlwaysAsk`].
#[derive(Debug, Default)]
pub(super) struct Always {
    /// The matter's rules file; none for any agent but Claude outside forced ask.
    file: Option<PathBuf>,
    rules: BTreeSet<String>,
    /// The matter folder's spellings as path rules (`always::matter_roots`): a path rule
    /// outside them is neither kept nor applied (`always::within`).
    roots: Vec<String>,
    asked: BTreeMap<PermissionRequestId, AlwaysAsk>,
}

impl Always {
    /// For a launch of an agent of `kind`: the rules its prep read (`RunFolders::always`), for
    /// Claude outside forced ask only, without those the lawyer removed since (`kept`, the
    /// actor's ledger): the prep read the file off the actor, maybe before a removal's write.
    pub(super) fn at_launch(
        run: &RunFolders,
        kind: Option<AgentKind>,
        forced_ask: bool,
        kept: &Ledger,
    ) -> Self {
        if kind != Some(AgentKind::Claude) || forced_ask {
            return Self::default();
        }
        let mut rules = run.always.iter().cloned().collect();
        kept.drop_removed(run.key.matter, &mut rules);
        Self {
            file: always::file_beside(&run.notes),
            rules,
            roots: Vec::new(),
            asked: BTreeMap::new(),
        }
    }

    /// Once the agent is launched in `folder` (spelled `spellings` too): the rules read that do
    /// not stay within it are dropped (`always::within`), so the data fence's deny rules are
    /// never the only thing between a kept rule and what lies outside the matter.
    pub(super) fn in_matter(mut self, folder: &std::path::Path, spellings: &[String]) -> Self {
        let folder = folder.to_str();
        let spellings = folder
            .into_iter()
            .chain(spellings.iter().map(String::as_str));
        self.roots = always::matter_roots(spellings, cfg!(windows));
        let roots = &self.roots;
        self.rules
            .retain(|rule| always::within(rule, roots, cfg!(windows)));
        self
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
    let rule = tool.map_or(Err(NotKept::NoRule), |tool| {
        always::rule(&tool, raw_input, cfg!(windows))
    });
    let real = None;
    Some(AlwaysAsk { rule, once, real })
}

/// The file whose real path the request's matter-thread check resolves: a path rule's.
pub(super) fn real_path(ask: Option<&AlwaysAsk>) -> Option<PathBuf> {
    let rule = ask?.rule.as_ref().ok()?;
    always::rule_path(rule, cfg!(windows))
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
/// [`AlwaysAsk`], the option the agent gets instead, its rule kept for the session and given
/// back to be stored ([`keep`]).
pub(super) fn answer(
    agent: &mut Agent,
    request: PermissionRequestId,
    option: &PermissionOption,
) -> Option<Answered> {
    let ask = agent.always.asked.remove(&request)?;
    if option.kind != PermissionKind::AllowAlways {
        return None;
    }
    let kept = ask.rule.and_then(|rule| {
        let path = always::rule_path(&rule, cfg!(windows)).is_some();
        let inside = always::within(&rule, &agent.always.roots, cfg!(windows))
            && (!path || ask.real == Some(true));
        let full = agent.always.rules.len() >= always::MAX_RULES;
        match (inside, full) {
            (false, _) => Err(NotKept::OutsideMatter),
            (true, true) => Err(NotKept::Full),
            (true, false) => Ok(rule),
        }
    });
    let (not_kept, keep) = match kept {
        Ok(rule) => {
            let new = agent.always.rules.insert(rule.clone());
            let file = agent.always.file.clone().filter(|_| new);
            (None, file.map(|file| (file, rule)))
        }
        Err(why) => (Some(why), None),
    };
    tracing::info!(
        ?not_kept,
        "an \"Always for this matter\" answer goes to the agent as allow once"
    );
    Some(Answered {
        once: ask.once,
        not_kept,
        keep,
    })
}

/// Stores a rule [`answer`] newly kept for `thread`'s matter, numbered by the actor's ledger
/// so a removal the lawyer asks for later always wins (`actor/kept.rs`).
pub(super) fn keep(actor: &mut Actor, thread: ThreadId, keep: Option<(PathBuf, String)>) {
    let Some((file, rule)) = keep else {
        return;
    };
    let Ok(entry) = actor.check_thread(thread) else {
        return;
    };
    let matter = entry.row.matter;
    let seq = actor.kept.keep(matter, &rule);
    always::store(file, rule, seq);
}

/// Answers `ask` `allow_once` when its rule is kept (a path rule only when `real`, its file's
/// real path inside the matter), logged as one commit; gives it back when it is not, for the
/// lawyer, with `real` kept for the answer.
pub(super) fn auto_allow(
    actor: &mut Actor,
    thread: ThreadId,
    mut ask: PermissionAsk,
    real: Option<bool>,
) -> Option<PermissionAsk> {
    if let Some(always) = ask.always.as_mut() {
        always.real = real;
    }
    let kept = ask
        .always
        .as_ref()
        .and_then(|always| always.rule.as_ref().ok());
    let agent = agent(actor, thread)?;
    let applies = kept.is_some_and(|rule| {
        agent.always.rules.contains(rule)
            && (always::rule_path(rule, cfg!(windows)).is_none() || real == Some(true))
    });
    if !applies {
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

/// The lawyer removed `rule` from `matter`'s kept rules: no agent of the matter applies it any
/// more.
pub(crate) fn forget(actor: &mut Actor, matter: clauseo_protocol::MatterId, rule: &str) {
    for entry in actor.threads.values_mut() {
        if entry.row.matter != matter {
            continue;
        }
        if let Some(agent) = entry.turn.agent.as_deref_mut() {
            agent.always.rules.remove(rule);
        }
    }
}
