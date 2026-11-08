//! The two Rust-only settings writes desktop makes for R20 (decision record 0048), never a
//! window's: `HostHandle::set_agent_launch`, when an agent's install is ready (a webview must not
//! choose a program to run), and `HostHandle::set_plan`, for a firm's endpoint after its confirm.
//! Both run on the actor, the settings' one owner, as `SetUiSettings` does: the write goes to
//! `settings.json` through `Effective`, which then holds it, so the next launch reads it, and
//! every app subscriber gets `SettingsChanged` once something was written. The settings crate
//! makes the checks and the host maps its refusals: a locked key is `settings.locked`, a value
//! that breaks a rule `settings.refused` (`Error::code`), whose message names the key, or a
//! variable's place, never a value. An entry the lawyer or the firm wrote is left alone, which
//! is no failure. Also here: the `plans` part of `GetSettings`' view, names and never values.

use std::collections::BTreeMap;

use clauseo_protocol::{AgentId, EnvName, HostEvent, PlanProvider, PlanView};
use clauseo_settings as settings;
use clauseo_settings::{AgentLaunch, EntryWrite, Plan, SystemFiles};

use super::super::Actor;
use super::super::input::PlanWrite;
use super::view;
use crate::error::Error;

/// `set_agent_launch`: writes `agents.<agent>` as `launch` and sends `SettingsChanged`, or
/// answers [`EntryWrite::LeftAlone`] with nothing written or sent.
pub(in crate::actor) fn set_agent_launch(
    actor: &mut Actor,
    agent: &AgentId,
    launch: &AgentLaunch,
) -> Result<EntryWrite, Error> {
    let written = actor
        .effective
        .set_agent_launch(&SystemFiles, agent.as_str(), launch)?;
    match written {
        EntryWrite::Written => changed(actor),
        EntryWrite::LeftAlone => tracing::info!(
            %agent,
            "an agent's settings entry is the lawyer's or the firm's own and was left alone"
        ),
    }
    Ok(written)
}

/// `set_plan`: writes `plans.<agent>` as `write` says (the whole plan, or `None` to remove it;
/// or its endpoint alone, the provider and the variables kept), and sends `SettingsChanged`.
pub(in crate::actor) fn set_plan(
    actor: &mut Actor,
    agent: &AgentId,
    write: PlanWrite,
) -> Result<(), Error> {
    let plan = match write {
        PlanWrite::Whole(plan) => plan.map(|plan| *plan),
        PlanWrite::Endpoint(endpoint) => {
            let existing = actor.effective.plan(agent.as_str()).cloned();
            if existing.is_none() && endpoint.is_none() {
                // No plan and no endpoint: nothing to clear, so no write and no `SettingsChanged`.
                return Ok(());
            }
            // A plan policy sets is locked: `set_plan` refuses it before anything is merged or
            // written, so a firm plan's provider and variables never reach `settings.json`.
            let mut plan = existing.unwrap_or_default();
            plan.endpoint = endpoint;
            // A plan left with nothing in it is removed, never written as `{}`.
            (plan != Plan::default()).then_some(plan)
        }
    };
    actor
        .effective
        .set_plan(&SystemFiles, agent.as_str(), plan.as_ref())?;
    changed(actor);
    Ok(())
}

/// Tells every app subscriber the settings in force.
fn changed(actor: &mut Actor) {
    let changed = HostEvent::SettingsChanged(view(actor));
    super::super::app::publish(actor, &changed);
}

/// Each agent's plan in force, by agent id: the endpoint, the provider and the variables' names.
/// A plan whose id is no agent id, or a name that is no variable name, is left out (the settings
/// crate already refuses both, so none is expected).
pub(super) fn plans_view(plans: &BTreeMap<String, Plan>) -> BTreeMap<AgentId, PlanView> {
    plans
        .iter()
        .filter_map(|(id, plan)| {
            let id = AgentId::new(id.as_str()).ok()?;
            let env_names = plan
                .env
                .keys()
                .filter_map(|name| EnvName::new(name.as_str()).ok())
                .collect();
            let view = PlanView {
                endpoint: plan.endpoint.clone(),
                provider: plan.provider.map(provider_out),
                env_names,
            };
            Some((id, view))
        })
        .collect()
}

fn provider_out(provider: settings::PlanProvider) -> PlanProvider {
    match provider {
        settings::PlanProvider::Anthropic => PlanProvider::Anthropic,
        settings::PlanProvider::Bedrock => PlanProvider::Bedrock,
        settings::PlanProvider::Vertex => PlanProvider::Vertex,
    }
}
