//! The agents' one network route (S17): which proxy and CA files the installs, the sign-ins
//! and the agents' launches take, from one resolver ([`resolve`]) over one read of the settings
//! ([`NetworkFacts`]), so the three never disagree.
//!
//! The proxy, first match wins:
//!
//! 1. `network.upstream_proxy` from the effective settings (a firm policy's over the lawyer's,
//!    as `settings` already resolves it): source `policy` or `setting`;
//! 2. `HTTPS_PROXY`, else `https_proxy`, from this process's environment: source `env`;
//! 3. with [`OS_FALLBACK`] on, the OS's proxy, asked per target host through the sandbox's
//!    `Upstream::System` lookup (`clauseo_sandbox::upstream_for`: macOS `CFNetwork`, Windows
//!    `WinHTTP`, static settings, PAC and WPAD alike, bounded and cached there; nothing on
//!    Linux): source `os`. The installs' target is the npm registry (the firm's mirror when
//!    set); a sign-in's is the agent's first sign-in host (the one its queued start looks up);
//!    a launch's is the agent's primary API host (a Claude plan's endpoint when set). A PAC
//!    `DIRECT` is no proxy.
//!
//! An empty or blank value counts as unset at every step. A proxy URL is passed to its
//! consumers as written, a user name and password included (proxies that ask for one are later:
//! nothing here answers a challenge), but never logged or printed: [`shown`] keeps only the
//! scheme, host and port.
//!
//! The CA files come from `network.ca_certs` alone.
//!
//! Where it goes: [`NetworkRoute::agents_network`] to `Agents` (at start, then
//! `Agents::set_network` on each change: the next install and sign-in take it), and the launch
//! hook's variables ([`NetworkRoute::launch_vars`], through `hook::setup_vars`, which the limits
//! readers share). The OS lookups can block (WPAD, a PAC script), so they run on a thread of
//! their own ([`follow`]); `Agents` starts with the settings' and the environment's route at
//! once, and the OS's lands on that thread a moment later.
//!
//! The sandbox's egress proxy (tiers A and B, S16) chains to the upstream through
//! [`NetworkRoute::upstream`] and takes [`NetworkRoute::ca_cert`]; wiring that into the host's
//! proxy start is stage C's. It speaks plain-HTTP `CONNECT` only, so an `https://` setting still
//! reaches the installs and sign-ins (their clients take one) but cannot chain: one warning says
//! so on each change, naming the rule and never the URL. Where one upstream serves every host
//! (the launch env, S16), the OS's answer for the agent's primary API host stands for all.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use clauseo_agents::{NPM_REGISTRY, Network, Snapshot};
use clauseo_protocol::{AgentId, AgentKind};
use clauseo_sandbox::{Route, Upstream, upstream_for};
use clauseo_settings::Effective;
use url::Url;

/// Whether the OS's proxy is asked when neither the settings nor the environment set one. On
/// (decided for launch); off is this one line.
pub(crate) const OS_FALLBACK: bool = true;

/// What `NO_PROXY` always holds in a launch with a proxy: this computer, which the host's own
/// listeners are on and no firm proxy can reach.
const LOOPBACK: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

/// The proxy variables of a launch. A firm plan's `env` or the agent's own entry that sets any
/// of these (in any case) keeps them all: none is added beside it.
const PROXY_VARS: [&str; 4] = ["HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY", "NO_PROXY"];

/// Claude's primary API host: the first host of the sandbox's built-in allowlists for both of
/// Claude's logins (`clauseo_sandbox::builtin_allowlist`, `ClaudePlan` and `ClaudeApiKey`).
const CLAUDE_API: &str = "https://api.anthropic.com/";
/// Codex's: the first of its allowlists (`CodexChatGpt` and `CodexApiKey`).
const CODEX_API: &str = "https://api.openai.com/";

/// Where the proxy came from, as the log names it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ProxySource {
    /// A firm policy's `network.upstream_proxy`.
    Policy,
    /// The lawyer's `network.upstream_proxy`.
    Setting,
    /// `HTTPS_PROXY` or `https_proxy`.
    Env,
    /// The OS's proxy (static or PAC: the sandbox's lookup does not say which).
    Os,
    /// None: every connection goes direct.
    #[default]
    None,
}

impl ProxySource {
    /// The word the log uses.
    pub(crate) fn word(self) -> &'static str {
        match self {
            ProxySource::Policy => "policy",
            ProxySource::Setting => "setting",
            ProxySource::Env => "env",
            ProxySource::Os => "os",
            ProxySource::None => "none",
        }
    }
}

/// What the resolver reads from the settings and the snapshot. Its `Debug` never prints the
/// proxy setting.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct NetworkFacts {
    /// `network.upstream_proxy`, as the effective settings give it.
    pub(crate) upstream: Option<String>,
    /// Whether a firm policy sets it.
    pub(crate) policy: bool,
    /// `network.ca_certs`, by agent id (an id that is no agent id is left out).
    pub(crate) ca_certs: BTreeMap<AgentId, PathBuf>,
    /// `install.registry`, the firm's npm mirror.
    pub(crate) registry: Option<String>,
    /// Each known agent's first sign-in host.
    pub(crate) signin_hosts: BTreeMap<AgentId, String>,
    /// Each known agent's primary API URL: a Claude plan's endpoint, else the vendor's API.
    pub(crate) api: BTreeMap<AgentId, String>,
    /// The variable names each agent's own entry (`agents.<id>.env`) sets, which the launch
    /// hook leaves alone.
    pub(crate) own_vars: BTreeMap<AgentId, BTreeSet<String>>,
}

impl std::fmt::Debug for NetworkFacts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetworkFacts")
            .field("upstream", &self.upstream.as_deref().map(shown))
            .field("policy", &self.policy)
            .field("ca_certs", &self.ca_certs)
            .field("registry", &self.registry.as_deref().map(shown))
            .field("signin_hosts", &self.signin_hosts)
            .field(
                "api",
                &self.api.values().map(|api| shown(api)).collect::<Vec<_>>(),
            )
            .field("own_vars", &self.own_vars)
            .finish()
    }
}

impl NetworkFacts {
    /// Reads `effective` for `known`, with the embedded snapshot's sign-in hosts.
    pub(crate) fn read<'a>(
        effective: &Effective,
        known: impl IntoIterator<Item = &'a AgentId>,
    ) -> Self {
        let settings = effective.settings();
        let snapshot = Snapshot::embedded();
        let mut facts = Self {
            upstream: settings.network.upstream_proxy.clone(),
            policy: effective.is_locked("network.upstream_proxy"),
            ca_certs: settings
                .network
                .ca_certs
                .iter()
                .filter_map(|(id, path)| Some((AgentId::new(id).ok()?, path.clone())))
                .collect(),
            registry: settings.install.registry.clone(),
            ..Self::default()
        };
        for agent in known {
            if let Some(host) = snapshot
                .agent(agent)
                .and_then(|entry| entry.signin_hosts.first())
            {
                facts.signin_hosts.insert(agent.clone(), host.clone());
            }
            let api = match AgentKind::of(agent) {
                Some(AgentKind::Claude) => Some(
                    effective
                        .plan(agent.as_str())
                        .and_then(|plan| plan.endpoint.clone())
                        .unwrap_or_else(|| CLAUDE_API.to_owned()),
                ),
                Some(AgentKind::Codex) => Some(CODEX_API.to_owned()),
                None => None,
            };
            if let Some(api) = api {
                facts.api.insert(agent.clone(), api);
            }
            if let Some(entry) = effective.agent(agent.as_str()) {
                facts
                    .own_vars
                    .insert(agent.clone(), entry.env.keys().cloned().collect());
            }
        }
        facts
    }
}

/// This process's proxy variables, as the resolver reads them.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct ProxyEnv {
    /// `HTTPS_PROXY`, else `https_proxy`.
    pub(crate) https: Option<String>,
    /// `NO_PROXY`, else `no_proxy`: kept in a launch's `NO_PROXY` beside the loopback names,
    /// since the launch's replaces the inherited one.
    pub(crate) no_proxy: Option<String>,
}

impl ProxyEnv {
    /// This process's, an empty or blank value as unset.
    pub(crate) fn from_process() -> Self {
        let read = |names: [&str; 2]| {
            names
                .into_iter()
                .filter_map(|name| std::env::var(name).ok())
                .find(|value| !value.trim().is_empty())
        };
        Self {
            https: read(["HTTPS_PROXY", "https_proxy"]),
            no_proxy: read(["NO_PROXY", "no_proxy"]),
        }
    }
}

/// The OS's proxy for a target URL, as `http://host:port`, or `None` to go direct. Blocking.
pub(crate) type OsLookup = dyn Fn(&Url) -> Option<String> + Send + Sync;

/// The sandbox's `Upstream::System` lookup, asked only once the settings and the environment
/// set no proxy (so it reaches the OS: static settings, PAC and WPAD).
pub(crate) fn os_lookup(target: &Url) -> Option<String> {
    match upstream_for(&Upstream::System, target) {
        Ok(Route::Http { host, port }) if host.contains(':') => {
            Some(format!("http://[{host}]:{port}"))
        }
        Ok(Route::Http { host, port }) => Some(format!("http://{host}:{port}")),
        Ok(Route::Direct) | Err(_) => None,
    }
}

/// One agent's proxies.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct AgentRoute {
    /// Its sign-in's.
    pub(crate) signin: Option<String>,
    /// Its launches'.
    pub(crate) launch: Option<String>,
}

/// The route every consumer takes, as [`resolve`] made it. Its `Debug` never prints a proxy
/// beyond its scheme, host and port.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct NetworkRoute {
    /// Where the proxy came from.
    pub(crate) source: ProxySource,
    /// The setting the proxy came from, for [`NetworkRoute::upstream`].
    setting: Option<String>,
    /// The installs' proxy.
    pub(crate) install: Option<String>,
    /// Each known agent's.
    pub(crate) agents: BTreeMap<AgentId, AgentRoute>,
    /// `network.ca_certs`.
    pub(crate) ca_certs: BTreeMap<AgentId, PathBuf>,
    /// A launch's `NO_PROXY`.
    no_proxy: String,
    /// The names each agent's own entry sets.
    own_vars: BTreeMap<AgentId, BTreeSet<String>>,
}

impl std::fmt::Debug for NetworkRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let agents: BTreeMap<&AgentId, (Option<String>, Option<String>)> = self
            .agents
            .iter()
            .map(|(agent, route)| {
                let show = |proxy: &Option<String>| proxy.as_deref().map(shown);
                (agent, (show(&route.signin), show(&route.launch)))
            })
            .collect();
        f.debug_struct("NetworkRoute")
            .field("source", &self.source)
            .field("install", &self.install.as_deref().map(shown))
            .field("agents", &agents)
            .field("ca_certs", &self.ca_certs)
            .finish_non_exhaustive()
    }
}

/// A set value: `None` for an empty or blank one.
fn set(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.trim().is_empty())
}

/// The route for `facts`: the setting's, else the environment's, else (with `os_fallback`)
/// `os`'s per target host, else none.
pub(crate) fn resolve(
    facts: &NetworkFacts,
    env: &ProxyEnv,
    os_fallback: bool,
    os: &OsLookup,
) -> NetworkRoute {
    let mut route = NetworkRoute {
        ca_certs: facts.ca_certs.clone(),
        own_vars: facts.own_vars.clone(),
        no_proxy: no_proxy(env.no_proxy.as_deref()),
        ..NetworkRoute::default()
    };
    let agents: BTreeSet<&AgentId> = facts.signin_hosts.keys().chain(facts.api.keys()).collect();
    let fixed = if let Some(setting) = set(facts.upstream.as_deref()) {
        route.setting = Some(setting.to_owned());
        route.source = if facts.policy {
            ProxySource::Policy
        } else {
            ProxySource::Setting
        };
        Some(setting.to_owned())
    } else if let Some(proxy) = set(env.https.as_deref()) {
        route.source = ProxySource::Env;
        Some(proxy.to_owned())
    } else {
        None
    };
    if let Some(proxy) = fixed {
        route.install = Some(proxy.clone());
        for &agent in &agents {
            let both = AgentRoute {
                signin: Some(proxy.clone()),
                launch: Some(proxy.clone()),
            };
            route.agents.insert(agent.clone(), both);
        }
        return route;
    }
    if !os_fallback {
        return route;
    }
    let ask = |target: &str| {
        let target = Url::parse(target).ok()?;
        set(os(&target).as_deref()).map(str::to_owned)
    };
    route.install = ask(set(facts.registry.as_deref()).unwrap_or(NPM_REGISTRY));
    for &agent in &agents {
        let signin = facts
            .signin_hosts
            .get(agent)
            .and_then(|host| ask(&format!("https://{host}/")));
        let launch = facts.api.get(agent).and_then(|api| ask(api));
        route
            .agents
            .insert(agent.clone(), AgentRoute { signin, launch });
    }
    let any = route.install.is_some()
        || route
            .agents
            .values()
            .any(|agent| agent.signin.is_some() || agent.launch.is_some());
    if any {
        route.source = ProxySource::Os;
    }
    route
}

/// The loopback names, then `inherited`'s entries not already there.
fn no_proxy(inherited: Option<&str>) -> String {
    let mut names: Vec<&str> = LOOPBACK.to_vec();
    for name in inherited.unwrap_or_default().split(',').map(str::trim) {
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    names.join(",")
}

impl NetworkRoute {
    /// What `Agents` takes: the installs' proxy, each agent's sign-in proxy and the CA files.
    pub(crate) fn agents_network(&self) -> Network {
        Network {
            proxy: self.install.clone(),
            signin_proxy: self
                .agents
                .iter()
                .map(|(agent, route)| (agent.clone(), route.signin.clone()))
                .collect(),
            ca_certs: self.ca_certs.clone(),
        }
    }

    /// The sandbox's egress proxy's upstream (S16, through the host's `ProxyConfig`): the
    /// firm's setting as `Fixed` when it came from a policy or the lawyer's setting, else
    /// `System` (which reads the environment and the OS itself, per host).
    pub(crate) fn upstream(&self) -> Upstream {
        match (&self.source, &self.setting) {
            (ProxySource::Policy | ProxySource::Setting, Some(setting)) => {
                // The settings crate has checked it is a URL; one that is not goes through the
                // OS's rather than none.
                Url::parse(setting).map_or(Upstream::System, Upstream::Fixed)
            }
            _ => Upstream::System,
        }
    }

    /// `agent`'s CA file (`network.ca_certs`), for the sandbox's egress wiring too: it tunnels
    /// without ending TLS, so a TLS-inspecting firm proxy's root must be in the agent's trust.
    pub(crate) fn ca_cert(&self, agent: &AgentId) -> Option<&Path> {
        self.ca_certs.get(agent).map(PathBuf::as_path)
    }

    /// What `agent`'s launches get, beside the variables in `taken` (a firm plan's, matched in
    /// any case), and never a variable its own entry sets:
    ///
    /// - with a proxy, `HTTPS_PROXY`, `HTTP_PROXY` and `NO_PROXY` (the loopback names and the
    ///   inherited ones), lower case too off Windows (Node's proxy agent reads `https_proxy`
    ///   first, and an inherited one would win); none of them when `taken` or the entry sets
    ///   any proxy variable;
    /// - its CA file under its CLI's variable (`clauseo_agents::ca_cert_var`, sign-in's mapping),
    ///   unless that variable is taken.
    ///
    /// The host's forced variables come after the hook's, so a proxy the launcher sets (the
    /// sandbox's egress proxy, tiers A and B) stays.
    pub(crate) fn launch_vars(
        &self,
        agent: &AgentId,
        taken: &dyn Fn(&str) -> bool,
    ) -> Vec<(String, String)> {
        let own = self.own_vars.get(agent);
        let held = |name: &str| {
            taken(name)
                || own.is_some_and(|own| own.iter().any(|set| set.eq_ignore_ascii_case(name)))
        };
        let mut vars = Vec::new();
        let proxy = self
            .agents
            .get(agent)
            .and_then(|route| route.launch.as_ref());
        if let Some(proxy) = proxy
            && !PROXY_VARS.iter().any(|name| held(name))
        {
            for name in ["HTTPS_PROXY", "HTTP_PROXY"] {
                vars.push((name.to_owned(), proxy.clone()));
                if cfg!(not(windows)) {
                    vars.push((name.to_ascii_lowercase(), proxy.clone()));
                }
            }
            vars.push(("NO_PROXY".to_owned(), self.no_proxy.clone()));
            if cfg!(not(windows)) {
                vars.push(("no_proxy".to_owned(), self.no_proxy.clone()));
            }
        }
        if let (Some(name), Some(ca)) = (clauseo_agents::ca_cert_var(agent), self.ca_cert(agent))
            && !held(name)
        {
            vars.push((name.to_owned(), ca.to_string_lossy().into_owned()));
        }
        vars
    }

    /// The log's view: the source and each distinct proxy's scheme, host and port.
    pub(crate) fn summary(&self) -> String {
        let mut proxies: BTreeSet<String> = BTreeSet::new();
        proxies.extend(self.install.as_deref().map(shown));
        for route in self.agents.values() {
            proxies.extend(route.signin.as_deref().map(shown));
            proxies.extend(route.launch.as_deref().map(shown));
        }
        let proxies: Vec<String> = proxies.into_iter().collect();
        if proxies.is_empty() {
            self.source.word().to_owned()
        } else {
            format!("{} {}", self.source.word(), proxies.join(", "))
        }
    }

    /// Whether the proxy is a setting the sandbox's egress proxy cannot chain to: `https://`.
    pub(crate) fn setting_is_https(&self) -> bool {
        self.setting
            .as_deref()
            .is_some_and(|setting| Url::parse(setting).is_ok_and(|url| url.scheme() == "https"))
    }
}

/// `proxy`'s scheme, host and port, with no user name, password, path or query; a bare
/// `host:port` is read as `http://`. `unreadable` for one that does not parse.
pub(crate) fn shown(proxy: &str) -> String {
    let url = if proxy.contains("://") {
        Url::parse(proxy)
    } else {
        Url::parse(&format!("http://{proxy}"))
    };
    let Ok(url) = url else {
        return "unreadable".to_owned();
    };
    let host = match url.host() {
        Some(url::Host::Ipv6(address)) => format!("[{address}]"),
        Some(host) => host.to_string(),
        None => return "unreadable".to_owned(),
    };
    match url.port_or_known_default() {
        Some(port) => format!("{}://{host}:{port}", url.scheme()),
        None => format!("{}://{host}", url.scheme()),
    }
}

mod follow;

pub(crate) use follow::{Follower, NetworkFollow, follow};

/// The launch hook's and the limits readers' snapshot of the route.
pub(crate) type NetworkSlot = Arc<RwLock<NetworkRoute>>;

#[cfg(test)]
mod tests;
