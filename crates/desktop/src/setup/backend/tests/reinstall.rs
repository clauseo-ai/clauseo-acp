//! An install that went missing after Clauseo installed it (#859): its `Missing` says
//! `installed_before`, read from the `agents.<id>` entry an earlier `Ready` wrote (kept in the
//! settings when the install folder goes), so the card offers "Install {plan}". An agent never
//! installed says nothing new, and the first run is as it was. The entry counts only when it is
//! Clauseo's own and not the policy's.

use std::collections::BTreeSet;

use clauseo_agents::{AgentsEvent, InstallState};
use clauseo_protocol::{AgentId, Count, InstallProgress, SetupEvent};
use clauseo_settings::{Effective, FilePolicy, Locations, PolicySource, SystemFiles};
use serde_json::json;

use super::fakes::rig;
use super::{claude, codex, setup_events};
use crate::setup::refresh::{SettingsRead, apply_settings, installed_before};
use crate::test_support::{Scratch, block_on};

fn install(agent: &AgentId, state: InstallState) -> AgentsEvent {
    AgentsEvent::Install {
        agent: agent.clone(),
        state,
    }
}

fn missing() -> InstallState {
    InstallState::Missing {
        download_bytes: 120_000_000,
    }
}

/// The `Missing` each agent was last shown with, by its `installed_before`.
fn missing_shown(events: &[SetupEvent]) -> Vec<(AgentId, bool)> {
    events
        .iter()
        .filter_map(|event| match event {
            SetupEvent::AgentInstallProgress {
                agent,
                state:
                    InstallProgress::Missing {
                        installed_before, ..
                    },
            } => Some((agent.clone(), *installed_before)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_missing_install_after_ready_says_installed_before_and_a_never_installed_one_does_not() {
    let rig = rig(true);
    let mut feed = rig.events.follow();
    // Claude was Ready, and its entry is in the settings (the write's `SettingsChanged`)
    block_on(rig.backend.pump(futures::stream::iter([install(
        &claude(),
        InstallState::Ready {
            version: "1.0.0".to_owned(),
        },
    )])));
    let read = SettingsRead {
        installed: BTreeSet::from([claude()]),
        ..SettingsRead::default()
    };
    block_on(apply_settings(&rig.backend, read));
    setup_events(&mut feed);
    // its folder goes; Codex was chosen and never installed
    block_on(rig.backend.pump(futures::stream::iter([
        install(&claude(), missing()),
        install(&codex(), missing()),
    ])));
    let events = setup_events(&mut feed);
    assert_eq!(missing_shown(&events), [(claude(), true), (codex(), false)]);
    assert!(events.contains(&SetupEvent::AgentInstallProgress {
        agent: claude(),
        state: InstallProgress::Missing {
            download_bytes: Count::clamped(120_000_000),
            installed_before: true,
        },
    }));
    assert!(
        rig.calls().iter().all(|(call, _)| *call != "install"),
        "nothing installs by itself: {:?}",
        rig.calls()
    );
}

#[test]
fn a_first_run_s_missing_installs_say_nothing_new() {
    let rig = rig(true);
    let mut feed = rig.events.follow();
    block_on(rig.backend.pump(futures::stream::iter([
        install(&claude(), missing()),
        install(&codex(), missing()),
    ])));
    assert_eq!(
        missing_shown(&setup_events(&mut feed)),
        [(claude(), false), (codex(), false)]
    );
}

/// The effective settings under `scratch` holding `agents`, and a policy holding `policy`.
fn effective(scratch: &Scratch, agents: &serde_json::Value, policy: Option<&str>) -> Effective {
    let locations = Locations::under_root(scratch.path());
    let rel = locations
        .settings_file()
        .strip_prefix(scratch.path())
        .unwrap();
    let user = json!({ "agents": agents }).to_string();
    scratch.file(rel.to_str().unwrap(), user.as_bytes());
    let Some(policy) = policy else {
        return Effective::load(&locations, &SystemFiles, &[]);
    };
    let path = scratch.file("policy.json", policy.as_bytes());
    let source = FilePolicy::new(path, SystemFiles);
    let sources: Vec<&dyn PolicySource> = vec![&source];
    Effective::load(&locations, &SystemFiles, &sources)
}

#[test]
fn installed_before_is_clauseo_s_own_entry_only() {
    let scratch = Scratch::folder("setup-installed-before", &[]);
    let data = Locations::under_root(scratch.path())
        .data_dir()
        .to_path_buf();
    let path = |rel: &str| data.join(rel).to_str().unwrap().to_owned();
    let ours = json!({
        "command": path("agents/runtime/node/bin/node"),
        "args": [path("agents/claude-acp/install/h1/index.js")],
    });
    let lawyers = json!({ "command": "/bin/sh", "args": ["-c", "claude-acp"] });
    let known = [claude(), codex()];
    let read = |agents, policy| installed_before(&effective(&scratch, &agents, policy), &known);
    assert_eq!(
        read(json!({ "claude-acp": ours }), None),
        BTreeSet::from([claude()]),
        "Clauseo's entry: installed here before"
    );
    assert_eq!(read(json!({}), None), BTreeSet::new(), "no entry: never");
    assert_eq!(
        read(json!({ "claude-acp": lawyers }), None),
        BTreeSet::new(),
        "the lawyer's own program is not Clauseo's install"
    );
    let policy = json!({ "agents": { "claude-acp": ours } }).to_string();
    assert_eq!(
        read(json!({ "claude-acp": ours }), Some(&policy)),
        BTreeSet::new(),
        "IT's runtime (managed) is never a reinstall"
    );
}
