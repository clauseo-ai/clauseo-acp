//! What desktop takes from the settings, at start and on each `SettingsChanged` (the brief's
//! "plans snapshot, refreshed on each `SettingsChanged`"): each agent's firm plan for the
//! launch hook, whether it has an endpoint or provider, whether a firm policy sets one, and
//! whether an unreadable policy blocks it (`firm_policy.rs`, which also brings the
//! `firm-policy` markers up to date).
//!
//! [`apply_settings`] tells [`Compose`](super::compose::Compose) what changed. A
//! firm policy's endpoint or provider that arrives ends a running sign-in quietly, the same
//! superseding cancel a stored key or endpoint makes (no `signin.cancelled` reaches a window).
//! The host is told each change of the launch refusal (`setup/refusal.rs`: the policy's, else
//! an unreadable key's, else none); it started with the blocked agents refused
//! (`SetupStart::launch_refusals`), so a follow that never runs leaves no launch open. The
//! limits readers are told the blocked agents too, so none of their reads goes out (review M2).
//!
//! An endpoint or a provider that arrives restarts the agent's agents (`restart_if_usable`), as
//! one the lawyer sets does: a leave that finished with nothing usable kept them held for the
//! next login, and this is one (review L3), with nothing else on screen to restart them. A
//! leave of the agent still running (or given up, waiting for S18's late end) keeps its hold:
//! its end restarts them.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::PoisonError;

use clauseo_protocol::AgentId;
use clauseo_settings::{Effective, Plan, SystemFiles};

use super::backend::AgentsBackend;
use super::firm_policy::firm_policy;
use super::hook::plans_of;
use super::leave::restart_if_usable;
use crate::bridge::lock;

/// The settings as setup reads them.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SettingsRead {
    /// The agents with an endpoint or a provider.
    pub(crate) firm: BTreeSet<AgentId>,
    /// Those whose endpoint or provider a firm policy sets.
    pub(crate) firm_policy: BTreeSet<AgentId>,
    /// Those an unreadable policy blocks.
    pub(crate) blocked: BTreeSet<AgentId>,
    /// Every plan, for the launch hook.
    pub(crate) plans: BTreeMap<AgentId, Plan>,
}

impl SettingsRead {
    /// Reads `effective` for `known`, writing the `firm-policy` markers.
    pub(crate) fn read<'a>(
        effective: &Effective,
        known: impl IntoIterator<Item = &'a AgentId> + Clone,
    ) -> Self {
        let firm: BTreeSet<AgentId> = known
            .clone()
            .into_iter()
            .filter(|agent| {
                effective
                    .plan(agent.as_str())
                    .is_some_and(|plan| plan.endpoint.is_some() || plan.provider.is_some())
            })
            .cloned()
            .collect();
        let policy_set = firm
            .iter()
            .filter(|agent| effective.is_locked(&format!("plans.{agent}")))
            .cloned()
            .collect();
        let blocked = firm_policy(effective, &SystemFiles, known);
        Self {
            firm,
            firm_policy: policy_set,
            blocked,
            plans: plans_of(effective),
        }
    }
}

/// Takes in `read`: the plans snapshot and the readers' blocked agents, then each agent's facts
/// where they changed, then the host's launch refusals where they changed, then a restart for
/// each agent an endpoint or a provider arrived for.
pub(crate) async fn apply_settings(backend: &AgentsBackend, read: SettingsRead) {
    *backend
        .wiring
        .plans
        .write()
        .unwrap_or_else(PoisonError::into_inner) = read.plans.clone();
    backend.wiring.readers.set_blocked(read.blocked.clone());
    let before = std::mem::replace(&mut *lock(&backend.wiring.applied), read.clone());
    let mut arrived = Vec::new();
    for agent in backend.known.keys() {
        let now = |set: &BTreeSet<AgentId>| set.contains(agent);
        if now(&before.blocked) != now(&read.blocked) {
            backend.compose.set_blocked(agent, now(&read.blocked));
        }
        if now(&before.firm) != now(&read.firm) {
            backend.compose.set_firm(agent, now(&read.firm));
            if now(&read.firm) {
                arrived.push(agent);
            }
        }
        if now(&before.firm_policy) != now(&read.firm_policy) {
            backend.set_firm_policy(agent, now(&read.firm_policy));
        }
    }
    super::refusal::sync_all(backend).await;
    for agent in arrived {
        // Lifting the hold now would let agents launch mid-leave. The check and the record are
        // one critical section (`Leaves::endpoint_arrived`): a leave of the agent that frees its
        // slot after it sees the endpoint and restarts by the rule (`after.rs`); one that freed
        // it before leaves no leave here, and this restarts.
        if backend.wiring.leaves.endpoint_arrived(agent) {
            tracing::info!(%agent, "an endpoint arrived during a leave: its end restarts");
            continue;
        }
        restart_if_usable(backend, agent).await;
    }
}
