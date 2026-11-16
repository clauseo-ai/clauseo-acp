//! The turn path's half of "Always for this matter" (decision record 0063; `agent/always.rs`
//! has why and the rules), for both agents, in forced ask too.
//!
//! - [`ask`]: a request that comes in gets its rule (or why none) and its first `allow_once`
//!   option, unless it is a plan exit (an `exit-plan-*` option, or the `ExitPlanMode` tool).
//!   The files whose real paths the request's matter-thread check resolves go with it
//!   ([`real_paths`], `agent::always::real_within`). The agent's rule-writing options are
//!   dropped in `asks.rs` (`agent::always::options`).
//! - [`offer`]: when the request is logged for the lawyer, `clauseo.always` is added and the
//!   card says what it would keep (`PermissionRequested.always`), when the rule can be kept.
//! - [`answer`]: the lawyer's `clauseo.always` answer is logged as chosen, but the agent gets
//!   the request's `allow_once` option, so no agent writes a rule; the rule is kept for the
//!   session and in the matter's file ([`keep`], numbered by the actor's `kept::Ledger`).
//!   What was not kept, and why, goes back for the lawyer's notice ([`Answered`]).
//! - [`auto_allow`]: a later request whose rule is kept is answered `allow_once` by the host,
//!   after the data-fence and held-open checks, logged as `PermissionRequested` and
//!   `PermissionResolved` in one commit, as `asks::refuse_held_open` logs its answer; a path
//!   or `Edit(*)` rule only when its files' real paths are inside the matter now. The answer is
//!   `by: local_desktop`: it applies the lawyer's own standing choice from this desktop app.
//!
//! Rules apply only to threads of their own matter: an agent reads its matter's file at launch,
//! and a removal reaches only that matter's agents ([`forget`]).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use clauseo_protocol::{
    AgentChoiceId, AgentKind, AgentSessionId, AlwaysOffer, Deliverable, PermissionChoice,
    PermissionKind, PermissionOption, PermissionRequestId, PermissionRequested, PermissionResolved,
    Principal, StreamId, ThreadId, ToolKind,
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
use crate::agent::always::{self, Kept, NotKept, options};
use crate::error::Error;
use crate::log_file::Cause;

/// What a request needs for "Always": its rule and what the card says, if it gives one, and
/// the option the agent gets.
#[derive(Debug, Clone)]
pub(super) struct AlwaysAsk {
    kept: Result<Kept, NotKept>,
    once: AgentChoiceId,
    /// Whether a path or `Edit(*)` rule's files are inside the matter by their real paths, as
    /// the request's matter-thread check found them; none when they were not checked.
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

/// An agent's kept rules for its matter, and its pending requests' [`AlwaysAsk`].
#[derive(Debug, Default)]
pub(super) struct Always {
    /// The matter's rules file; none for an agent other than Claude or ChatGPT.
    file: Option<PathBuf>,
    /// The agent's kind: commands are kept per agent.
    kind: Option<AgentKind>,
    rules: BTreeSet<String>,
    /// The matter folder's spellings as path rules (`always::matter_roots`): a path rule
    /// outside them is neither kept nor applied (`always::within`).
    roots: Vec<String>,
    asked: BTreeMap<PermissionRequestId, AlwaysAsk>,
}

impl Always {
    /// For a launch of an agent of `kind`: the rules its prep read (`RunFolders::always`), for
    /// Claude or ChatGPT, in forced ask too, without those the lawyer removed since (`kept`, the
    /// actor's ledger): the prep read the file off the actor, maybe before a removal's write.
    pub(super) fn at_launch(run: &RunFolders, kind: Option<AgentKind>, kept: &Ledger) -> Self {
        if !matches!(kind, Some(AgentKind::Claude | AgentKind::Codex)) {
            return Self::default();
        }
        let mut rules = run.always.iter().cloned().collect();
        kept.drop_removed(run.key.matter, &mut rules);
        Self {
            file: always::file_beside(&run.notes),
            kind,
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

/// Claude Code's plan exit tool.
const PLAN_EXIT_TOOL: &str = "ExitPlanMode";

/// A request's [`AlwaysAsk`] (see the module doc), for an agent that keeps rules.
pub(super) fn ask(
    actor: &mut Actor,
    thread: ThreadId,
    session: &AgentSessionId,
    acp_tool_call_id: &str,
    options: &[PermissionOption],
    raw_input: Option<&Value>,
    tool_kind: Option<ToolKind>,
) -> Option<AlwaysAsk> {
    let agent_ref = agent(actor, thread)?;
    agent_ref.always.file.as_ref()?;
    let kind = agent_ref.always.kind?;
    let matter = agent_ref.folder.to_str()?.to_owned();
    let tool = tool_name(actor, thread, session, acp_tool_call_id);
    if options::plan_exit(options) || tool.as_deref() == Some(PLAN_EXIT_TOOL) {
        return None;
    }
    let once = options
        .iter()
        .find(|option| option.kind == PermissionKind::AllowOnce)
        .and_then(|option| AgentChoiceId::new(&option.option_id).ok())?;
    let request = always::Request {
        agent: kind,
        tool: tool.as_deref(),
        kind: tool_kind,
        raw_input,
        matter: &matter,
        windows: cfg!(windows),
    };
    let kept = always::rule(&request);
    Some(AlwaysAsk {
        kept,
        once,
        real: None,
    })
}

/// The files whose real paths the request's matter-thread check resolves: a `Read` rule's file,
/// or every file an `Edit(*)` request names (`paths`, absolute or relative to `folder`).
pub(super) fn real_paths(
    ask: Option<&AlwaysAsk>,
    paths: &[PathBuf],
    folder: &std::path::Path,
) -> Vec<PathBuf> {
    let Some(Ok(kept)) = ask.map(|ask| ask.kept.as_ref()) else {
        return Vec::new();
    };
    if kept.rule == always::EDIT_ALL {
        return paths.iter().map(|path| folder.join(path)).collect();
    }
    always::rule_path(&kept.rule, cfg!(windows))
        .into_iter()
        .collect()
}

/// What the card's `clauseo.always` would keep, when the request's rule can be kept now: in the
/// matter (a path or `Edit(*)` rule's files by their real paths, `real`), and room left.
pub(super) fn offer(
    agent: &Agent,
    ask: Option<&AlwaysAsk>,
    real: Option<bool>,
) -> Option<AlwaysOffer> {
    let kept = ask?.kept.as_ref().ok()?;
    let always = &agent.always;
    always.file.as_ref()?;
    let inside = always::within(&kept.rule, &always.roots, cfg!(windows))
        && (!always::needs_real(&kept.rule) || real == Some(true));
    let room = always.rules.len() < always::MAX_RULES || always.rules.contains(&kept.rule);
    (inside && room).then(|| kept.offer.clone())
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

/// Whether `request` waits for an answer with an [`AlwaysAsk`].
pub(super) fn has_ask(agent: &Agent, request: PermissionRequestId) -> bool {
    agent.always.asked.contains_key(&request)
}

/// The lawyer answered `request` with `option`: for `clauseo.always` with an [`AlwaysAsk`],
/// the option the agent gets instead, its rule kept for the session and given back to be stored
/// ([`keep`]).
pub(super) fn answer(
    agent: &mut Agent,
    request: PermissionRequestId,
    option: &PermissionOption,
) -> Option<Answered> {
    let ask = agent.always.asked.remove(&request)?;
    if option.option_id != options::ALWAYS {
        return None;
    }
    let kept = ask.kept.and_then(|Kept { rule, .. }| {
        let inside = always::within(&rule, &agent.always.roots, cfg!(windows))
            && (!always::needs_real(&rule) || ask.real == Some(true));
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
        kind = ?agent.always.kind,
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
        .and_then(|always| always.kept.as_ref().ok());
    let agent = agent(actor, thread)?;
    let applies = kept.is_some_and(|kept| {
        agent.always.rules.contains(&kept.rule)
            && (!always::needs_real(&kept.rule) || real == Some(true))
    });
    let entry = kept.map(|kept| kept.offer.kind);
    if !applies {
        return Some(ask);
    }
    let kind = agent.always.kind;
    if let Err(error) = allow(actor, thread, ask) {
        tracing::warn!(%thread, error = %Cause(&error), "a kept rule's answer was not logged");
    }
    tracing::info!(%thread, agent = ?kind, entry = ?entry, "a request matching a kept rule was allowed once");
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
            always: None,
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
