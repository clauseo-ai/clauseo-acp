//! A settings read through the backend's own refresh (`apply_settings`, each
//! `SettingsChanged`) gives `Agents` and the launch hook's snapshot the new network route, and
//! a command that reaches the network waits for the first route, so no try goes direct first.

use std::sync::Arc;

use std::pin::pin;
use std::task::{Context, Poll};

use clauseo_agents::InstallState;
use serde_json::json;

use clauseo_agents::keys::Keychain;

use super::fakes::{Rig, command, rig, rig_with};
use super::{MAIN, claude, codex};
use crate::bridge::lock;
use crate::setup::NetworkSlot;
use crate::setup::backend::SetupAgents;
use crate::setup::keys::KeyCache;
use crate::setup::network::{Follower, NetworkFollow, ProxyEnv};
use crate::setup::refresh::{SettingsRead, apply_settings};
use crate::test_support::block_on;

const FIRM: &str = "http://proxy.firm.example:3128";

/// A route slot and the follow that resolves each read into it on the caller's thread, giving
/// `agents` the network: no proxy variable, no OS proxy.
pub(super) fn inline(agents: Arc<dyn SetupAgents>) -> (NetworkSlot, NetworkFollow) {
    let slot = NetworkSlot::default();
    let follower = Follower::new(
        Arc::clone(&slot),
        agents,
        ProxyEnv::default,
        |_target: &url::Url| None,
        true,
    );
    (slot, NetworkFollow::inline(follower))
}

#[test]
fn a_settings_change_reaches_agents_and_the_launch_snapshot() {
    let rig = rig(true);
    let mut read = SettingsRead::default();
    read.network.upstream = Some(FIRM.to_owned());
    block_on(apply_settings(&rig.backend, read.clone()));
    let pushed = lock(&rig.agents.networks).clone();
    assert_eq!(pushed.last().and_then(|n| n.proxy.as_deref()), Some(FIRM));
    let route = rig.network.read().unwrap().clone();
    assert_eq!(route.install.as_deref(), Some(FIRM));
    read.network.upstream = None;
    block_on(apply_settings(&rig.backend, read));
    let pushed = lock(&rig.agents.networks).clone();
    assert_eq!(pushed.last().map(|n| n.proxy.clone()), Some(None));
    assert_eq!(rig.network.read().unwrap().install, None);
}

/// A rig whose first network route has not landed: no settings read yet.
fn rig_unrouted() -> Rig {
    let keys = KeyCache::load_all(Keychain::memory(), [&claude(), &codex()]);
    rig_with(true, Keychain::memory(), keys, false, false)
}

/// An install pressed before the first route lands waits for it: `Agents` gets the route, then
/// the install, so its first try already goes through the resolved proxy.
#[test]
fn an_install_waits_for_the_first_route_and_goes_through_it() {
    let rig = rig_unrouted();
    lock(&rig.agents.states).insert(claude(), InstallState::Missing { download_bytes: 1 });
    let setup = rig.setup();
    let mut run = pin!(setup.run(MAIN, command("install_agent", &claude(), &json!({}))));
    let mut context = Context::from_waker(std::task::Waker::noop());
    assert!(matches!(run.as_mut().poll(&mut context), Poll::Pending));
    let installs = || {
        let calls = lock(&rig.agents.calls).clone();
        calls.iter().filter(|(name, _)| *name == "install").count()
    };
    assert_eq!(installs(), 0, "no install before the route");
    assert!(lock(&rig.agents.networks).is_empty());
    let mut read = SettingsRead::default();
    read.network.upstream = Some(FIRM.to_owned());
    block_on(apply_settings(&rig.backend, read));
    block_on(run).unwrap();
    assert_eq!(installs(), 1);
    let pushed = lock(&rig.agents.networks).clone();
    assert_eq!(pushed.first().and_then(|n| n.proxy.as_deref()), Some(FIRM));
}
