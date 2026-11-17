//! The options a Claude or Codex agent's draft pickers show before its first session has
//! reported any: [`of`], which `AgentOptions::of` gives while nothing is kept for the agent.
//!
//! Each seed is the agent's model option and effort option, in the agent's own ids and
//! categories, each with one value, [`AGENT_DEFAULT`]: "the agent picks", selected. The window
//! names that value "{agent}'s default" (UC-874, `ui/app/src/room/pickers.ts`). It is never
//! sent: `picks.rs` leaves it out of a session's picks and refuses a `SetConfigOption` of it, so
//! the agent starts on its own default, exactly what ran before there was a seed. The session's
//! answer then logs the agent's real options, which replace the seed in the thread's header and
//! in `agent-options.json`, so the pickers move to the model and effort in use as it attaches.
//!
//! Only rows that are true for every account at the pinned versions are seeded, and none is:
//! - claude-agent-acp 0.89.1 lists the SDK's models for the account, cut by its settings'
//!   allowlist and managed `deniedModels` (`dist/session-model.js:163-200, 241-300`), and
//!   offers an effort only for a model that supports one, with that model's levels
//!   (`dist/session-effort.js:61-100`). Under the AIR `recommendedValue` capability Clauseo
//!   declares (`clauseo_acp::driver::client_capabilities`) its own "default" rows go too.
//! - codex-acp 2.2.2 lists the account's models from Codex (`dist/index.js:38286-38310`), the
//!   default one the model Codex marks `isDefault` (`:35551`), and each model's own efforts
//!   (`:38311-38327`).
//!
//! The option ids are tied to those versions: Claude's `model` and `effort`
//! (`dist/session-model.js:2`, `dist/session-config-ids.js:4`), Codex's `model` and
//! `reasoning_effort` (`dist/index.js:38273-38274`). Re-check them on every agent pin bump
//! (`docs/decisions/0033-agents-installs.md`, Consequences). A seed id the agent no longer
//! uses only refuses a `SetConfigOption` made before the first session with `command.invalid`
//! until it attaches.

use std::sync::LazyLock;

use clauseo_protocol::{AgentKind, ConfigCategory, ConfigChoice, ConfigOption, ConfigOptionKind};

/// The seed's one value: the agent picks its own default. Never sent to an agent; no agent at
/// the pinned versions offers a value of this id.
pub(crate) const AGENT_DEFAULT: &str = "clauseo-agent-default";

/// The ids of an agent's model option and effort option, as its pinned version reports them.
struct SeedIds {
    model: &'static str,
    effort: &'static str,
}

/// Claude's, from claude-agent-acp 0.89.1 (`dist/session-model.js:2` `MODEL_CONFIG_ID`,
/// `dist/session-config-ids.js:4` `EFFORT_CONFIG_ID`). Re-check on every pin bump.
const CLAUDE_AGENT_ACP_0_89_1: SeedIds = SeedIds {
    model: "model",
    effort: "effort",
};

/// Codex's, from codex-acp 2.2.2 (`dist/index.js:38273-38274`, `MODEL_CONFIG_ID` and
/// `REASONING_EFFORT_CONFIG_ID`). Re-check on every pin bump.
const CODEX_ACP_2_2_2: SeedIds = SeedIds {
    model: "model",
    effort: "reasoning_effort",
};

static CLAUDE: LazyLock<Vec<ConfigOption>> = LazyLock::new(|| options(&CLAUDE_AGENT_ACP_0_89_1));
static CODEX: LazyLock<Vec<ConfigOption>> = LazyLock::new(|| options(&CODEX_ACP_2_2_2));

/// The seed of the agent with registry id `agent`; empty for an agent of no kind.
pub(crate) fn of(agent: &str) -> &'static [ConfigOption] {
    if agent == AgentKind::Claude.id() {
        &CLAUDE
    } else if agent == AgentKind::Codex.id() {
        &CODEX
    } else {
        &[]
    }
}

/// True for an option at the seed's value: a select whose current value is [`AGENT_DEFAULT`].
pub(crate) fn is_seed(option: &ConfigOption) -> bool {
    matches!(
        &option.kind,
        ConfigOptionKind::Select { current_value, .. } if current_value.as_str() == AGENT_DEFAULT
    )
}

fn options(ids: &SeedIds) -> Vec<ConfigOption> {
    vec![
        option(ids.model, "Model", ConfigCategory::Model),
        option(ids.effort, "Effort", ConfigCategory::ThoughtLevel),
    ]
}

fn option(id: &str, name: &str, category: ConfigCategory) -> ConfigOption {
    ConfigOption {
        id: id.into(),
        name: name.into(),
        description: None,
        category,
        kind: ConfigOptionKind::Select {
            current_value: AGENT_DEFAULT.into(),
            options: vec![ConfigChoice {
                value: AGENT_DEFAULT.into(),
                name: "Default".into(),
                description: None,
                group: None,
            }],
        },
    }
}
