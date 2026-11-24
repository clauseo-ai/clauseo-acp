//! A Tools change reaching a live agent (Settings › Tools). An agent's servers are made once, at
//! its launch (`agent::loads::lawyer`), so an agent that would get other servers now is stopped
//! as an idle agent is: its session detaches `IdleStopped`, which the app shows as resting, and
//! its next prompt relaunches it with the new servers and resumes the session.
//!
//! - Each change to the list bumps the Tools revision (`McpOnActor::tools_rev`). After each
//!   input, each live agent not compared at the current revision has its launch's key
//!   (`ToolsKey`, taken with its keychain reads) compared with the key a launch would take now,
//!   as that agent sees servers (a remote one only when it takes remote servers): different
//!   marks it (`Relaunch::ToolsChanged`), the same clears that mark, so an edit put back before
//!   its turn ends stops nothing. An agent that has not answered `initialize` is compared once
//!   it has.
//! - A marked agent is stopped once [`super::stoppable`] (`super::stop_marked`): nothing
//!   pending, so a turn, a queued prompt or steer, a sign-in hold, a permission or elicitation,
//!   a launch or a call keeps it until it ends. Quick edits make one stop: a stopping agent is
//!   not stoppable, and the agent relaunched takes the newest key.
//! - A prompt does not wait for the grace: a prompted turn (typed, or taken from the queue) for a
//!   marked agent with nothing else open restarts it at step 4 (`agents::restart_if_marked`),
//!   so the prompt goes with the new servers. A removed server or a changed key thus stays only
//!   for the turn running at the edit and any agent-started turn after it. What the relaunch can
//!   cut: a command Claude put in the background (a steer during a Bash call ends the turn with
//!   the command still running) that has not reported yet. The host sees such a command only
//!   when it reports, as an agent-started turn, so it cannot know one is running; a prompt in
//!   that window relaunches the agent, the command ends with it, and its result never comes.

use std::collections::BTreeMap;

use clauseo_protocol::{MatterId, ThreadId};

use super::super::super::Actor;
use super::Relaunch;
use crate::actor::addon_rules::Placed;
use crate::agent::loads::lawyer::ToolsKey;

/// Compares each live agent's servers if the list changed since it was last compared.
pub(super) fn compare(actor: &mut Actor) {
    let rev = actor.mcp.tools_rev;
    let live: Vec<ThreadId> = actor.turn_path.idle.live.iter().copied().collect();
    let mut now: BTreeMap<MatterId, ToolsKey> = BTreeMap::new();
    for thread in live {
        compare_one(actor, thread, rev, &mut now);
    }
}

/// Marks `thread`'s agent or clears its Tools mark at revision `rev`, unless it was compared at
/// it already or has not answered `initialize`; `now` holds the key a launch in each matter
/// takes now, made once per matter (its switches and the items' scopes make it differ).
fn compare_one(
    actor: &mut Actor,
    thread: ThreadId,
    rev: u64,
    now: &mut BTreeMap<MatterId, ToolsKey>,
) {
    let Some(state) = actor.threads.get(&thread).map(|entry| &entry.turn) else {
        return;
    };
    let (Some(agent), Some(run)) = (state.agent.as_deref(), state.run.as_ref()) else {
        return;
    };
    let launched = &run.mcp_secrets.key;
    if agent.idle.tools_seen.max(launched.rev) == rev {
        return;
    }
    let Some(http) = agent.info.as_ref().map(|info| info.mcp_http) else {
        return;
    };
    let matter = run.key.matter;
    let now = now.entry(matter).or_insert_with(|| {
        ToolsKey::of(
            &Placed::of(actor, matter),
            actor.protection.safe_workspace(),
            &actor.mcp.secret_revs,
            rev,
        )
    });
    let stale = !launched.same_for(now, http);
    if let Some(agent) = actor
        .threads
        .get_mut(&thread)
        .and_then(|entry| entry.turn.agent.as_deref_mut())
    {
        agent.idle.tools_seen = rev;
        if !stale && agent.idle.relaunch == Some(Relaunch::ToolsChanged) {
            agent.idle.relaunch = None;
        }
    }
    if stale {
        super::mark_relaunch(actor, thread, Relaunch::ToolsChanged);
    }
}
