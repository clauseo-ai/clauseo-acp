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
//! And to Claude's and Codex's session `PATH` (`ProfileHook::agent_path`), Clauseo's Python
//! runtime folders from the [`PythonSlot`] (`setup/python_path.rs`), which the setup thread
//! fills once Python is `Ready` and empties otherwise, then, while there are any, the bundled
//! PDF tools' folder (`setup/pdf_tools.rs`: `pdftoppm` and `pdfinfo` run that runtime); the
//! host puts them after stetdoc's. On the same condition, Codex's instructions end with the line
//! that tells it to render PDF pages with them (`clauseo_profile::instructions::codex_pdf`).
//!
//! Both snapshots are refreshed on each `SettingsChanged` (`setup/refresh.rs`). [`setup_vars`] is
//! the one function that makes these variables: the limits readers take the same ones
//! (`setup/readers.rs`), so a read goes where the agent's launches go. Desktop's copy is
//! `Zeroizing`; the env's values are plain `OsString`s once handed to the host, which drops
//! them once the agent is spawned (decision record 0048, ruling 5).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, PoisonError, RwLock};

use clauseo_host::{McpServer, NoProfile, PackRequest, ProfileHook};
use clauseo_profile::{PackWrite, PacksWritten, SkillWrite, SkillsWritten};
use clauseo_protocol::{AgentId, AgentKind};
use clauseo_settings::{Effective, Plan, PlanProvider};
use serde_json::{Map, Value};
use zeroize::Zeroizing;

use super::keys::KeySlot;
use super::network::{NetworkSlot, with_loopback};
use super::python_path::PythonSlot;

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

/// The launch profile, if it was built, plus the keys, the plans, the network route and the
/// Python folders.
pub(crate) struct SetupHook {
    profile: Box<dyn ProfileHook>,
    keys: KeySlot,
    plans: Plans,
    network: NetworkSlot,
    python: PythonSlot,
    /// The bundled `pdftoppm` and `pdfinfo`'s folder (`setup::pdf_tools::found`), if found.
    pdf_tools: Option<PathBuf>,
}

impl SetupHook {
    /// The hook over `keys`, `plans` and `network`, with no profile (as `NoProfile`) and no
    /// Python folders.
    pub(crate) fn new(keys: KeySlot, plans: Plans, network: NetworkSlot) -> Self {
        Self {
            profile: Box::new(NoProfile),
            keys,
            plans,
            network,
            python: PythonSlot::default(),
            pdf_tools: None,
        }
    }

    /// This hook with Claude's and Codex's session `PATH` folders read from `python`.
    #[must_use]
    pub(crate) fn with_python(self, python: PythonSlot) -> Self {
        Self { python, ..self }
    }

    /// This hook with the PDF tools' folder `folder` after the Python folders, while there are
    /// any.
    #[must_use]
    pub(crate) fn with_pdf_tools(self, folder: Option<PathBuf>) -> Self {
        Self {
            pdf_tools: folder,
            ..self
        }
    }

    /// The Python runtime's folders, as the setup thread last put them in the slot.
    fn python_dirs(&self) -> Vec<PathBuf> {
        self.python
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The PDF tools' folder when it was found and `python` (the runtime's folders) has any:
    /// the tools run that runtime, so they are on an agent's `PATH` only with it.
    fn pdf_folder(&self, python: &[PathBuf]) -> Option<PathBuf> {
        self.pdf_tools.clone().filter(|_| !python.is_empty())
    }

    /// This hook over `profile`, whose calls it hands through.
    #[must_use]
    pub(crate) fn over(self, profile: Box<dyn ProfileHook>) -> Self {
        Self { profile, ..self }
    }

    /// What this hook adds for `agent` of `kind` ([`setup_vars`]), as the host takes it.
    fn setup_env(&self, agent: &AgentId, kind: AgentKind) -> BTreeMap<OsString, OsString> {
        let (vars, routed) = setup_parts(&self.keys, &self.plans, &self.network, agent, kind);
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
    let (mut vars, routed) = setup_parts(keys, plans, network, agent, kind);
    for (name, value) in routed {
        if let Ok(value) = value.into_string() {
            vars.insert(name, Zeroizing::new(value));
        }
    }
    vars
}

/// The plan's and the key's variables, and the network route's beside them.
type SetupParts = (BTreeMap<String, Zeroizing<String>>, Vec<(String, OsString)>);

/// [`setup_vars`]' parts: the plan's and the key's variables, and the network route's beside
/// them (none a plan variable takes), whose CA path stays an `OsString`.
fn setup_parts(
    keys: &KeySlot,
    plans: &Plans,
    network: &NetworkSlot,
    agent: &AgentId,
    kind: AgentKind,
) -> SetupParts {
    let mut vars = BTreeMap::new();
    let plan = plans
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .get(agent)
        .cloned();
    if let Some(mut plan) = plan {
        // A firm plan whose key read failed (`KeyCache::unreadable_for`) loses its endpoint and
        // provider here, whatever the host's refusal says yet: a launch in the gap before
        // `refusal::sync_all` (a settings change) never goes keyless to a firm's gateway, where
        // Claude Code would offer the lawyer's own claude.ai token (decision record 0048).
        let firm = plan.endpoint.is_some() || plan.provider.is_some();
        if firm
            && keys
                .get()
                .is_some_and(|keys| keys.unreadable_for(agent, true))
        {
            plan.endpoint = None;
            plan.provider = None;
        }
        plan_env(kind, &plan, &mut vars);
    }
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
    let key = keys.get().and_then(|keys| keys.get(agent));
    if let Some(key) = key.filter(|key| !key.is_empty()) {
        vars.insert(key_var(kind).to_owned(), key);
    }
    (vars, routed)
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

    /// The profile's Codex keys, a Codex agent's `developer_instructions` ending with the PDF
    /// line when the PDF tools are on its `PATH` ([`Self::pdf_folder`]).
    fn codex_config(
        &self,
        agent: &AgentId,
        written: Option<&SkillsWritten>,
        packs: Option<&PacksWritten>,
    ) -> Map<String, Value> {
        let mut keys = self.profile.codex_config(agent, written, packs);
        if AgentKind::of(agent) == Some(AgentKind::Codex)
            && let Some(Value::String(text)) = keys.get_mut("developer_instructions")
            && self.pdf_folder(&self.python_dirs()).is_some()
        {
            text.push_str("\n\n");
            text.push_str(clauseo_profile::instructions::codex_pdf());
        }
        keys
    }

    fn launch_writes(&self, agent: &AgentId) -> Vec<SkillWrite> {
        self.profile.launch_writes(agent)
    }

    fn pack_write(&self, agent: &AgentId, request: &PackRequest) -> Option<PackWrite> {
        self.profile.pack_write(agent, request)
    }

    /// The Python runtime's folders, as the setup thread last put them in the slot, then the PDF
    /// tools' folder when there are any, for a Claude or Codex agent; none for any other. A read
    /// of memory only: no I/O.
    fn agent_path(&self, agent: &AgentId) -> Vec<PathBuf> {
        if AgentKind::of(agent).is_none() {
            return Vec::new();
        }
        let mut dirs = self.python_dirs();
        if let Some(folder) = self.pdf_folder(&dirs) {
            dirs.push(folder);
        }
        dirs
    }

    fn browser_tool(&self) -> Option<PathBuf> {
        self.profile.browser_tool()
    }
}

#[cfg(test)]
mod tests;
