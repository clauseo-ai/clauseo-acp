//! A firm plan's endpoint with the first-party provider and no key (decision record 0048, "A
//! firm endpoint needs a key"): Claude Code 2.1.293 keeps its own sign-in on under a custom
//! `ANTHROPIC_BASE_URL` and sends the lawyer's claude.ai token there, so the agent is refused
//! with `agent.firm_key_needed` and its card says so until the firm's key is saved. A stored
//! key, a Bedrock or Vertex plan (the cloud's credentials) and no plan launch as before.

use std::collections::BTreeSet;

use clauseo_agents::keys::Keychain;
use clauseo_protocol::{AgentId, CommandOutcome, ErrorCode, SignIn, SignInMethod};
use clauseo_settings::{Effective, Locations, Plan, PlanProvider, SystemFiles};
use serde_json::json;

use super::fakes::{HostCall, command, rig, rig_on};
use super::key_reads::stored;
use super::switching::shown;
use super::{SETTINGS, claude, codex};
use crate::setup::inherited::Inherited;
use crate::setup::refresh::{SettingsRead, apply_settings};
use crate::setup::refusal::settle_after_load;
use crate::test_support::{Scratch, block_on};

const FIRM_KEY: &str = "sk-ant-api03-FIRM-KEY-0003";

/// The refusals the host is told once the keys have loaded from `keychain`, with the agents in
/// `firm` under a firm plan and those in `own_signin` under its endpoint with the first-party
/// provider.
fn settled(keychain: Keychain, firm: &[AgentId], own_signin: &[AgentId]) -> Vec<HostCall> {
    let rig = rig_on(true, keychain);
    block_on(settle_after_load(
        &*rig.host,
        &rig.backend.keys,
        [&claude(), &codex()],
        &BTreeSet::new(),
        &firm.iter().cloned().collect(),
        (
            &own_signin.iter().cloned().collect(),
            &std::collections::BTreeMap::new(),
        ),
    ));
    rig.host.calls()
}

fn refused(code: Option<ErrorCode>) -> Vec<HostCall> {
    vec![
        HostCall::SetLaunchRefused(claude(), code),
        HostCall::SetLaunchRefused(codex(), None),
    ]
}

#[test]
fn an_endpoint_with_no_key_is_refused_and_a_key_a_cloud_provider_or_no_plan_launches() {
    let both = [claude()];
    assert_eq!(
        settled(Keychain::memory(), &both, &both),
        refused(Some(ErrorCode::AgentFirmKeyNeeded)),
        "the leak state: no launch"
    );
    assert_eq!(settled(stored(), &both, &both), refused(None), "key stored");
    // A Bedrock or Vertex plan is firm but keeps no own sign-in (`SettingsRead::own_signin`).
    assert_eq!(
        settled(Keychain::memory(), &both, &[]),
        refused(None),
        "cloud"
    );
    assert_eq!(
        settled(Keychain::memory(), &[], &[]),
        refused(None),
        "no plan"
    );
}

/// An endpoint no plan set (the app's own environment, `inherited.rs`) with no key: refused
/// with `agent.endpoint_key_needed`, not the firm's code; a stored key launches. A plan's
/// endpoint with no key keeps the firm's code.
#[test]
fn an_endpoint_no_plan_set_with_no_key_is_refused_with_its_own_code() {
    let settle = |keychain: Keychain, own_signin: &[AgentId]| {
        let rig = rig_on(true, keychain);
        let host = Some("proxy.example".to_owned());
        let endpoint = Inherited {
            host,
            setting: None,
        };
        let inherited = std::collections::BTreeMap::from([(claude(), endpoint)]);
        block_on(settle_after_load(
            &*rig.host,
            &rig.backend.keys,
            [&claude(), &codex()],
            &BTreeSet::new(),
            &BTreeSet::new(),
            (&own_signin.iter().cloned().collect(), &inherited),
        ));
        rig.host.calls()
    };
    assert_eq!(
        settle(Keychain::memory(), &[]),
        refused(Some(ErrorCode::AgentEndpointKeyNeeded))
    );
    assert_eq!(settle(stored(), &[]), refused(None), "key stored");
    assert_eq!(
        settle(Keychain::memory(), &[claude()]),
        refused(Some(ErrorCode::AgentFirmKeyNeeded))
    );
}

/// The settings read with Claude's plan `claude` and Codex's `codex` written as the lawyer's
/// (`Effective::set_plan`, the settings crate's own write).
fn read_with(claude_plan: Option<Plan>, codex_plan: Option<Plan>) -> SettingsRead {
    let scratch = Scratch::folder("firm-endpoint-read", &["data"]);
    let locations = Locations::under_root(&scratch.path().join("data"));
    let mut effective = Effective::load(&locations, &SystemFiles, &[]);
    for (agent, plan) in [(claude(), claude_plan), (codex(), codex_plan)] {
        effective
            .set_plan(&SystemFiles, agent.as_str(), plan.as_ref())
            .unwrap();
    }
    SettingsRead::read(&effective, [&claude(), &codex()])
}

/// A plan with the firm's endpoint and `provider`.
fn endpoint(provider: Option<PlanProvider>) -> Plan {
    Plan {
        endpoint: Some("https://gateway.firm.example".to_owned()),
        provider,
        ..Plan::default()
    }
}

#[test]
fn only_a_claude_endpoint_with_the_first_party_provider_keeps_the_own_sign_in() {
    for provider in [None, Some(PlanProvider::Anthropic)] {
        let read = read_with(Some(endpoint(provider)), Some(endpoint(None)));
        assert_eq!(read.own_signin, BTreeSet::from([claude()]), "{provider:?}");
        assert_eq!(read.firm, BTreeSet::from([claude(), codex()]));
    }
    for provider in [PlanProvider::Bedrock, PlanProvider::Vertex] {
        let read = read_with(Some(endpoint(Some(provider))), None);
        assert_eq!(read.own_signin, BTreeSet::new(), "{provider:?}");
        assert_eq!(read.firm, BTreeSet::from([claude()]));
    }
    assert_eq!(read_with(None, None).own_signin, BTreeSet::new(), "no plan");
}

fn firm_key_needed(shown: Option<&SignIn>) -> bool {
    matches!(
        shown,
        Some(SignIn::Failed {
            code: ErrorCode::AgentFirmKeyNeeded,
            ..
        })
    )
}

#[test]
fn the_card_says_the_firm_key_is_needed_until_it_is_saved_then_shows_firm() {
    let rig = rig(true);
    let endpoint = Plan {
        endpoint: Some("https://gateway.firm.example".to_owned()),
        ..Plan::default()
    };
    let read = SettingsRead {
        firm: [claude()].into(),
        own_signin: [claude()].into(),
        plans: [(claude(), endpoint)].into(),
        ..SettingsRead::default()
    };
    block_on(apply_settings(&rig.backend, read));
    assert!(
        rig.host.calls().contains(&HostCall::SetLaunchRefused(
            claude(),
            Some(ErrorCode::AgentFirmKeyNeeded)
        )),
        "{:?}",
        rig.host.calls()
    );
    let line = shown(&rig, &claude());
    assert!(firm_key_needed(line.as_ref()), "{line:?}");
    assert!(
        rig.backend.compose.sign_in_bar(&claude()).is_some(),
        "the endpoint still bars the plan sign-in"
    );

    let save = command("set_agent_key", &claude(), &json!({ "key": FIRM_KEY }));
    assert_eq!(
        block_on(rig.setup().run(SETTINGS, save)),
        Ok(CommandOutcome::Done)
    );
    assert_eq!(
        rig.host.calls().into_iter().rev().find(
            |call| matches!(call, HostCall::SetLaunchRefused(agent, _) if *agent == claude())
        ),
        Some(HostCall::SetLaunchRefused(claude(), None)),
        "saved: launches may start"
    );
    assert!(matches!(
        shown(&rig, &claude()),
        Some(SignIn::SignedIn {
            method: SignInMethod::Firm,
            ..
        })
    ));
}
