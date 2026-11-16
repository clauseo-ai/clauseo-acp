//! A launch's variables from the route ([`NetworkRoute::launch_vars`]): the proxy, `NO_PROXY`,
//! the CA file and `NODE_USE_SYSTEM_CA`, each beside what the plan, the entry and the launcher
//! set, never over it.

use std::ffi::OsString;

use clauseo_protocol::AgentId;

use super::NetworkRoute;

/// What `NO_PROXY` always holds in a launch with a proxy: this computer, which the host's own
/// listeners are on and no firm proxy can reach.
pub(super) const LOOPBACK: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

/// The proxy variables a launch with a proxy gets, each unless a firm plan's `env` or the
/// agent's own entry sets it (in any case). An `ALL_PROXY` set there keeps the first two out
/// too: they would win over it.
const PROXY_VARS: [&str; 2] = ["HTTPS_PROXY", "HTTP_PROXY"];

/// `list` (a `NO_PROXY` an entry, a plan or the launcher set) with the loopback names it does not
/// hold added after it, so the agent's calls to Clauseo's own local endpoints (the MCP relay on
/// 127.0.0.1) stay off the proxy; nothing in it is dropped.
pub(crate) fn with_loopback(list: &str) -> String {
    let held: Vec<&str> = list.split(',').map(str::trim).collect();
    let missing = LOOPBACK.iter().filter(|name| !held.contains(name));
    let mut out = list.trim().trim_end_matches(',').to_owned();
    for name in missing {
        if !out.is_empty() {
            out.push(',');
        }
        out.push_str(name);
    }
    out
}

/// [`NetworkRoute::launch_vars`].
pub(super) fn launch_vars(
    route: &NetworkRoute,
    agent: &AgentId,
    taken: &dyn Fn(&str) -> bool,
) -> Vec<(String, OsString)> {
    let own = route.own_vars.get(agent);
    let held = |name: &str| {
        taken(name) || own.is_some_and(|own| own.iter().any(|set| set.eq_ignore_ascii_case(name)))
    };
    let mut vars = Vec::new();
    let proxy = route
        .agents
        .get(agent)
        .and_then(|route| route.launch.as_ref());
    if let Some(proxy) = proxy {
        let all = held("ALL_PROXY");
        let free = PROXY_VARS.iter().filter(|name| !all && !held(name));
        let mut wanted: Vec<(&str, &str)> = free.map(|name| (*name, proxy.as_str())).collect();
        if !held("NO_PROXY") {
            wanted.push(("NO_PROXY", route.no_proxy.as_str()));
        } else if !taken("NO_PROXY")
            && let Some((name, list)) = route.own_no_proxy.get(agent)
        {
            // The entry's own, with the loopback names added (a plan's: `hook::setup_parts`).
            vars.push((name.clone(), with_loopback(list).into()));
        }
        for (name, value) in wanted {
            vars.push((name.to_owned(), value.into()));
            if cfg!(not(windows)) {
                vars.push((name.to_ascii_lowercase(), value.into()));
            }
        }
    }
    if let (Some(name), Some(ca)) = (clauseo_agents::ca_cert_var(agent), route.ca_cert(agent))
        && !held(name)
    {
        vars.push((name.to_owned(), ca.as_os_str().to_owned()));
    }
    if !held(clauseo_agents::SYSTEM_CA_VAR) && !route.system_ca_inherited {
        vars.push((clauseo_agents::SYSTEM_CA_VAR.to_owned(), "1".into()));
    }
    vars
}
