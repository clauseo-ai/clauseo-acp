//! What desktop takes from the settings, at start and on each `SettingsChanged` (the brief's
//! "plans snapshot, refreshed on each `SettingsChanged`"): each agent's firm plan for the
//! launch hook, whether it has an endpoint or provider, whether a firm policy sets one, and
//! whether an unreadable policy blocks it (`firm_policy.rs`, which also brings the
//! `firm-policy` markers up to date).
//!
//! [`apply_settings`] tells [`Compose`](super::compose::Compose) what changed. A
//! firm policy's endpoint or provider that arrives ends a running sign-in quietly, the same
//! superseding cancel a stored key or endpoint makes (no `signin.cancelled` reaches a window).
//! The host is told each change of the launch refusal (`set_launch_refused` with
//! `signin.policy_unreadable`, or `None`); it started with the blocked agents refused
//! (`SetupStart::launch_refusals`), so a follow that never runs leaves no launch open.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::PoisonError;

use clauseo_protocol::{AgentId, ErrorCode};
use clauseo_settings::{Effective, Plan, SystemFiles};

use super::backend::AgentsBackend;
use super::firm_policy::firm_policy;
use super::hook::plans_of;
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

/// Takes in `read`: the plans snapshot, then each agent's facts where they changed, then
/// the host's launch refusals where they changed.
pub(crate) async fn apply_settings(backend: &AgentsBackend, read: SettingsRead) {
    *backend
        .wiring
        .plans
        .write()
        .unwrap_or_else(PoisonError::into_inner) = read.plans.clone();
    let before = std::mem::replace(&mut *lock(&backend.wiring.applied), read.clone());
    for agent in backend.known.keys() {
        let now = |set: &BTreeSet<AgentId>| set.contains(agent);
        if now(&before.blocked) != now(&read.blocked) {
            backend.compose.set_blocked(agent, now(&read.blocked));
        }
        if now(&before.firm) != now(&read.firm) {
            backend.compose.set_firm(agent, now(&read.firm));
        }
        if now(&before.firm_policy) != now(&read.firm_policy) {
            backend.set_firm_policy(agent, now(&read.firm_policy));
        }
    }
    for agent in backend.known.keys() {
        let blocked = read.blocked.contains(agent);
        if lock(&backend.wiring.refused).contains(agent) == blocked {
            continue;
        }
        let code = blocked.then_some(ErrorCode::SigninPolicyUnreadable);
        match backend.host.set_launch_refused(agent.clone(), code).await {
            Ok(()) => {
                let mut refused = lock(&backend.wiring.refused);
                if blocked {
                    refused.insert(agent.clone());
                } else {
                    refused.remove(agent);
                }
            }
            Err(error) => {
                tracing::warn!(%agent, code = error.code().as_str(), "launch refusal not set");
            }
        }
    }
}
