//! The one profile hook desktop gives the host (`HostConfig::with_profile_hook`; the brief's
//! "API keys" and "the firm endpoint"): [`SetupHook`] hands the launch profile's calls through
//! when there is a profile, and adds to each agent's launch env:
//!
//! - its stored key, from the [`KeySlot`], as `ANTHROPIC_API_KEY` (Claude) or `CODEX_API_KEY`
//!   (Codex), over the entry's empty placeholder; no key, or keys not loaded yet, adds nothing;
//! - its firm plan's variables, from the [`Plans`] snapshot: for Claude the endpoint as
//!   `ANTHROPIC_BASE_URL` and the provider as `CLAUDE_CODE_USE_BEDROCK=1` or
//!   `CLAUDE_CODE_USE_VERTEX=1` (`anthropic` sets nothing); for both, the plan's `env`, which
//!   the settings crate has already limited to its list. A Codex gateway is later (the brief),
//!   so a Codex endpoint adds no variable;
//! - the agents' network route, from the [`NetworkSlot`] (`setup/network.rs`): the proxy as
//!   `HTTPS_PROXY` and `HTTP_PROXY` with `NO_PROXY` for loopback, and the agent's
//!   `network.ca_certs` file under its CLI's variable (`NODE_EXTRA_CA_CERTS` for Claude,
//!   `CODEX_CA_CERTIFICATE` for Codex, sign-in's mapping), and `NODE_USE_SYSTEM_CA=1`. Never over
//!   one the plan's `env` or the agent's own entry sets, and never `SSL_CERT_FILE`; the host's
//!   forced variables (the sandbox's egress proxy, its root set) come after the hook's and win.
//!
//! Both snapshots are refreshed on each `SettingsChanged` (`setup/refresh.rs`). [`setup_vars`] is
//! the one function that makes these variables: the limits readers take the same ones
//! (`setup/readers.rs`), so a read goes where the agent's launches go. Desktop's copy is
//! `Zeroizing`; the env's values are plain `OsString`s once handed to the host, which drops
//! them once the agent is spawned (decision record 0048, ruling 5).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::sync::{Arc, PoisonError, RwLock};

use clauseo_host::{McpServer, NoProfile, PackRequest, ProfileHook};
use clauseo_profile::{PackWrite, PacksWritten, SkillWrite, SkillsWritten};
use clauseo_protocol::{AgentId, AgentKind};
use clauseo_settings::{Effective, Plan, PlanProvider};
use serde_json::{Map, Value};
use zeroize::Zeroizing;

use super::keys::{KeyCache, KeySlot};
use super::network::{NetworkSlot, with_loopback};

/// Each agent's firm plan, by agent id, as the settings last read gave it.
pub(crate) type Plans = Arc<RwLock<BTreeMap<AgentId, Plan>>>;

/// Every plan in `effective`, by agent id (an id that is no agent id is left out).
pub(crate) fn plans_of(effective: &Effective) -> BTreeMap<AgentId, Plan> {
    effective
        .settings()
        .plans
        .iter()
        .filter_map(|(id, plan)| Some((AgentId::new(id).ok()?, plan.clone())))
        .collect()
}

/// The launch profile, if it was built, plus the keys, the plans and the network route.
pub(crate) struct SetupHook {
    profile: Box<dyn ProfileHook>,
    keys: KeySlot,
    plans: Plans,
    network: NetworkSlot,
}

impl SetupHook {
    /// The hook over `keys`, `plans` and `network`, with no profile (as `NoProfile`).
    pub(crate) fn new(keys: KeySlot, plans: Plans, network: NetworkSlot) -> Self {
        Self {
            profile: Box::new(NoProfile),
            keys,
            plans,
            network,
        }
    }

    /// This hook over `profile`, whose calls it hands through.
    #[must_use]
    pub(crate) fn over(self, profile: Box<dyn ProfileHook>) -> Self {
        Self { profile, ..self }
    }

    /// What this hook adds for `agent` of `kind` ([`setup_vars`]), as the host takes it.
    fn setup_env(&self, agent: &AgentId, kind: AgentKind) -> BTreeMap<OsString, OsString> {
        let keys = self.keys.get().map(Arc::as_ref);
        let (vars, routed) = setup_parts(keys, &self.plans, &self.network, agent, kind);
        let vars = vars
            .iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value.as_str())));
        let routed = routed
            .into_iter()
            .map(|(name, value)| (OsString::from(name), value));
        vars.chain(routed).collect()
    }
}

/// The variables setup adds for `agent` of `kind`, to its launches (the hook) and its limits
/// reads (`setup/readers.rs`) alike: its firm plan's, then the network route's that the plan
/// leaves free, then its stored key's (none while the keys load, or with no key). Zeroed when
/// dropped. A CA path that is not UTF-8 is left out here (a limits read takes text); the
/// launches get it as it is ([`SetupHook`]'s env).
pub(crate) fn setup_vars(
    keys: &KeySlot,
    plans: &Plans,
    network: &NetworkSlot,
    agent: &AgentId,
    kind: AgentKind,
) -> BTreeMap<String, Zeroizing<String>> {
    let (mut vars, routed) = setup_parts(keys.get().map(Arc::as_ref), plans, network, agent, kind);
    for (name, value) in routed {
        if let Ok(value) = value.into_string() {
            vars.insert(name, Zeroizing::new(value));
        }
    }
    vars
}

/// The plan's and the key's variables, and the network route's beside them.
type SetupParts = (BTreeMap<String, Zeroizing<String>>, Vec<(String, OsString)>);

/// [`setup_vars`]' parts over `keys` itself (none while they load): the plan's and the key's
/// variables ([`setup_vars_with`]), and the network route's beside them (none a plan variable
/// takes), whose CA path stays an `OsString`.
fn setup_parts(
    keys: Option<&KeyCache>,
    plans: &Plans,
    network: &NetworkSlot,
    agent: &AgentId,
    kind: AgentKind,
) -> SetupParts {
    let mut vars = setup_vars_with(keys, plans, agent, kind);
    let route = network.read().unwrap_or_else(PoisonError::into_inner);
    let planned = |name: &str| {
        vars.keys()
            .any(|set: &String| set.eq_ignore_ascii_case(name))
    };
    let routed = route.launch_vars(agent, &planned);
    let proxied = route.proxied(agent);
    drop(route);
    if proxied {
        // A plan's own `NO_PROXY` stays, with the loopback names it lacks added.
        for (name, value) in &mut vars {
            if name.eq_ignore_ascii_case("NO_PROXY") {
                *value = Zeroizing::new(with_loopback(value));
            }
        }
    }
    (vars, routed)
}

/// The plan's and the key's variables of [`setup_vars`], without the network route's, over
/// `keys` itself (none while they load): the identity check's exemption reads the launch env
/// through this (`setup/identity/check.rs`); no route variable replaces the login.
pub(crate) fn setup_vars_with(
    keys: Option<&KeyCache>,
    plans: &Plans,
    agent: &AgentId,
    kind: AgentKind,
) -> BTreeMap<String, Zeroizing<String>> {
    let mut vars = BTreeMap::new();
    let plan = plans
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .get(agent)
        .cloned();
    if let Some(plan) = plan {
        plan_env(kind, &plan, &mut vars);
    }
    let key = keys.and_then(|keys| keys.get(agent));
    if let Some(key) = key.filter(|key| !key.is_empty()) {
        vars.insert(key_var(kind).to_owned(), key);
    }
    vars
}

/// The provider switches that send Claude to a cloud provider's own credentials (AWS, Google
/// Cloud, Azure), never to the OAuth login in Claude's store.
const PROVIDER_VARS: [&str; 3] = [
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
];

/// Whether the launch env `vars` ([`setup_vars`]) carries a credential that replaces the shared
/// login for `kind` (R17, the security review's H3): the API key variable the hook sets, or,
/// for Claude, a provider switch, which reads no OAuth file. An endpoint alone
/// (`ANTHROPIC_BASE_URL`, or a Codex endpoint, which adds no variable) still launches on the
/// shared login, so it is not one.
pub(crate) fn replaces_login(kind: AgentKind, vars: &BTreeMap<String, Zeroizing<String>>) -> bool {
    let set = |name: &str| vars.get(name).is_some_and(|value| !value.is_empty());
    let provider = kind == AgentKind::Claude && PROVIDER_VARS.iter().any(|name| set(name));
    set(key_var(kind)) || provider
}

/// The variable `kind`'s key goes in.
pub(crate) fn key_var(kind: AgentKind) -> &'static str {
    match kind {
        AgentKind::Claude => "ANTHROPIC_API_KEY",
        AgentKind::Codex => "CODEX_API_KEY",
    }
}

/// `plan`'s variables for `kind`, into `env`.
fn plan_env(kind: AgentKind, plan: &Plan, env: &mut BTreeMap<String, Zeroizing<String>>) {
    for (name, value) in &plan.env {
        env.insert(name.clone(), Zeroizing::new(value.clone()));
    }
    if kind != AgentKind::Claude {
        return;
    }
    if let Some(endpoint) = &plan.endpoint {
        env.insert(
            "ANTHROPIC_BASE_URL".to_owned(),
            Zeroizing::new(endpoint.clone()),
        );
    }
    let provider = match plan.provider {
        Some(PlanProvider::Bedrock) => Some("CLAUDE_CODE_USE_BEDROCK"),
        Some(PlanProvider::Vertex) => Some("CLAUDE_CODE_USE_VERTEX"),
        Some(PlanProvider::Anthropic) | None => None,
    };
    if let Some(name) = provider {
        env.insert(name.to_owned(), Zeroizing::new("1".to_owned()));
    }
}

impl ProfileHook for SetupHook {
    fn session_meta(
        &self,
        agent: &AgentId,
        packs: Option<&PacksWritten>,
    ) -> Option<Map<String, Value>> {
        self.profile.session_meta(agent, packs)
    }

    fn launch_env(&self, agent: &AgentId) -> BTreeMap<OsString, OsString> {
        let mut env = self.profile.launch_env(agent);
        if let Some(kind) = AgentKind::of(agent) {
            env.extend(self.setup_env(agent, kind));
        }
        env
    }

    fn mcp_servers(&self, agent: &AgentId) -> Vec<McpServer> {
        self.profile.mcp_servers(agent)
    }

    fn codex_config(
        &self,
        agent: &AgentId,
        written: Option<&SkillsWritten>,
        packs: Option<&PacksWritten>,
    ) -> Map<String, Value> {
        self.profile.codex_config(agent, written, packs)
    }

    fn launch_writes(&self, agent: &AgentId) -> Vec<SkillWrite> {
        self.profile.launch_writes(agent)
    }

    fn pack_write(&self, agent: &AgentId, request: &PackRequest) -> Option<PackWrite> {
        self.profile.pack_write(agent, request)
    }
}

#[cfg(test)]
mod tests;
