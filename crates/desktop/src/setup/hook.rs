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
//!   so a Codex endpoint adds no variable.
//!
//! The snapshot is refreshed on each `SettingsChanged` (`setup/refresh.rs`). The env's values
//! are plain `OsString`s once handed to the host: the key there is not zeroed.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::sync::{Arc, PoisonError, RwLock};

use clauseo_host::{McpServer, NoProfile, PackRequest, ProfileHook};
use clauseo_profile::{PackWrite, PacksWritten, SkillWrite, SkillsWritten};
use clauseo_protocol::{AgentId, AgentKind};
use clauseo_settings::{Effective, Plan, PlanProvider};
use serde_json::{Map, Value};

use super::keys::KeySlot;

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

/// The launch profile, if it was built, plus the keys and the plans.
pub(crate) struct SetupHook {
    profile: Box<dyn ProfileHook>,
    keys: KeySlot,
    plans: Plans,
}

impl SetupHook {
    /// The hook over `keys` and `plans`, with no profile (as `NoProfile`).
    pub(crate) fn new(keys: KeySlot, plans: Plans) -> Self {
        Self {
            profile: Box::new(NoProfile),
            keys,
            plans,
        }
    }

    /// This hook over `profile`, whose calls it hands through.
    #[must_use]
    pub(crate) fn over(self, profile: Box<dyn ProfileHook>) -> Self {
        Self { profile, ..self }
    }

    /// What this hook adds for `agent` of `kind`: the key variable, then the plan's.
    fn setup_env(&self, agent: &AgentId, kind: AgentKind) -> BTreeMap<OsString, OsString> {
        let mut env = BTreeMap::new();
        let plan = self
            .plans
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(agent)
            .cloned();
        if let Some(plan) = plan {
            plan_env(kind, &plan, &mut env);
        }
        let key = self.keys.get().and_then(|keys| keys.get(agent));
        if let Some(key) = key.filter(|key| !key.is_empty()) {
            env.insert(OsString::from(key_var(kind)), OsString::from(key.as_str()));
        }
        env
    }
}

/// The variable `kind`'s key goes in.
pub(crate) fn key_var(kind: AgentKind) -> &'static str {
    match kind {
        AgentKind::Claude => "ANTHROPIC_API_KEY",
        AgentKind::Codex => "CODEX_API_KEY",
    }
}

/// `plan`'s variables for `kind`, into `env`.
fn plan_env(kind: AgentKind, plan: &Plan, env: &mut BTreeMap<OsString, OsString>) {
    for (name, value) in &plan.env {
        env.insert(OsString::from(name), OsString::from(value));
    }
    if kind != AgentKind::Claude {
        return;
    }
    if let Some(endpoint) = &plan.endpoint {
        env.insert(
            OsString::from("ANTHROPIC_BASE_URL"),
            OsString::from(endpoint),
        );
    }
    let provider = match plan.provider {
        Some(PlanProvider::Bedrock) => Some("CLAUDE_CODE_USE_BEDROCK"),
        Some(PlanProvider::Vertex) => Some("CLAUDE_CODE_USE_VERTEX"),
        Some(PlanProvider::Anthropic) | None => None,
    };
    if let Some(name) = provider {
        env.insert(OsString::from(name), OsString::from("1"));
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
