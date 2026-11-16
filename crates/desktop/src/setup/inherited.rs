//! A custom endpoint Claude's launches get from the app's own environment or the agent's entry,
//! with no plan setting one (decision record 0048, "A firm endpoint needs a key"). Claude Code
//! would offer its own sign-in there as it would to a plan's endpoint, so while no key is in
//! memory the agent is refused with `agent.endpoint_key_needed` (`refusal.rs`), and its card
//! names the endpoint's host (`compose.rs`): no plan set it, so it is not called the firm's.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

use clauseo_protocol::{AgentId, AgentKind};
use clauseo_settings::{Effective, Plan, PlanProvider, SettingsEnv, own_signin_reaches_endpoint};

/// The base URL variable.
const BASE_URL: &str = "ANTHROPIC_BASE_URL";

/// The longest host shown; a longer value is cut.
const HOST_MAX: usize = 100;

/// Each of `known` whose launches get a custom endpoint from this process's environment or
/// their entry, with no plan setting one, to that endpoint's host ([`inherited_host`]).
pub(crate) fn inherited_hosts<'a>(
    effective: &Effective,
    known: impl IntoIterator<Item = &'a AgentId>,
) -> BTreeMap<AgentId, String> {
    let env: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    known
        .into_iter()
        .filter_map(|agent| {
            let entry = effective.agent(agent.as_str()).map(|launch| &launch.env);
            let plan = effective.plan(agent.as_str());
            let host = inherited_host(agent, plan, entry, &env)?;
            Some((agent.clone(), host))
        })
        .collect()
}

/// The host of the custom endpoint `agent`'s launches get from `env` (the app's own) and its
/// `entry`'s variables over it, when no `plan` sets an endpoint or a cloud provider: Claude only,
/// and only when Claude Code's own sign-in would reach it (`own_signin_reaches_endpoint`), the
/// key left out, as the launch hook sets it from the keys (`hook.rs`).
pub(crate) fn inherited_host(
    agent: &AgentId,
    plan: Option<&Plan>,
    entry: Option<&BTreeMap<String, String>>,
    env: &[(OsString, OsString)],
) -> Option<String> {
    if AgentKind::of(agent) != Some(AgentKind::Claude) {
        return None;
    }
    let cloud = matches!(
        plan.and_then(|plan| plan.provider),
        Some(PlanProvider::Bedrock | PlanProvider::Vertex)
    );
    if plan.is_some_and(|plan| plan.endpoint.is_some()) || cloud {
        return None;
    }
    let entry = entry.into_iter().flatten();
    let vars: Vec<(&OsStr, &OsStr)> = env
        .iter()
        .map(|(name, value)| (name.as_os_str(), value.as_os_str()))
        .chain(entry.map(|(name, value)| (OsStr::new(name), OsStr::new(value))))
        .filter(|(name, _)| !name.eq_ignore_ascii_case("ANTHROPIC_API_KEY"))
        .collect();
    if !own_signin_reaches_endpoint(vars.iter().copied(), &SettingsEnv::default()) {
        return None;
    }
    let url = vars.iter().rev().find_map(|(name, value)| {
        let named = if cfg!(windows) {
            name.eq_ignore_ascii_case(BASE_URL)
        } else {
            *name == BASE_URL
        };
        named.then(|| value.to_string_lossy())
    })?;
    let host = url::Url::parse(&url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned));
    Some(host.unwrap_or_else(|| url.chars().take(HOST_MAX).collect()))
}

#[cfg(test)]
mod tests;
