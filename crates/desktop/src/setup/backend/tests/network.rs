//! A settings read through the backend's own refresh (`apply_settings`, each
//! `SettingsChanged`) gives `Agents` and the launch hook's snapshot the new network route; a
//! command that reaches the network waits for the first route, so no try goes direct first; and
//! the start's `agent.keys_loading` holds launches until that route is known.

use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use clauseo_agents::keys::Keychain;
use clauseo_agents::{Clock as _, InstallState, SystemClock};
use clauseo_host::ProfileHook as _;
use clauseo_process::{AsyncIoTimer, Timer as _};
use clauseo_protocol::ErrorCode;
use serde_json::json;

use super::fakes::{HostCall, Rig, command, rig, rig_with};
use super::{MAIN, claude, codex};
use crate::bridge::lock;
use crate::setup::backend::SetupAgents;
use crate::setup::keys::KeyCache;
use crate::setup::network::{Follower, NetworkFacts, NetworkFollow, ProxyEnv};
use crate::setup::refresh::{SettingsRead, apply_settings};
use crate::setup::refusal::lift_at_start;
use crate::setup::{NetworkSlot, SetupHook};
use crate::test_support::block_on;

const FIRM: &str = "http://proxy.firm.example:3128";
/// What the rig's OS answers for every host: a proxy set only in the OS.
const OS_PROXY: &str = "http://os-proxy.firm.example:8080";

/// A route slot and the follow that resolves each read into it on the caller's thread, giving
/// `agents` the network: no proxy variable, and [`OS_PROXY`] from the OS for every host.
pub(super) fn inline(agents: Arc<dyn SetupAgents>) -> (NetworkSlot, NetworkFollow) {
    let slot = NetworkSlot::default();
    let follower = Follower::new(
        Arc::clone(&slot),
        agents,
        ProxyEnv::default,
        |_target: &url::Url| Some(OS_PROXY.to_owned()),
        true,
    );
    (slot, NetworkFollow::inline(follower))
}

/// A rig whose first network route has not landed: no settings read yet.
fn rig_unrouted() -> Rig {
    let keys = KeyCache::load_all(Keychain::memory(), [&claude(), &codex()]);
    rig_with(true, Keychain::memory(), keys, false, false)
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
    assert_eq!(
        pushed.last().and_then(|n| n.proxy.as_deref()),
        Some(OS_PROXY)
    );
    assert_eq!(
        rig.network.read().unwrap().install.as_deref(),
        Some(OS_PROXY)
    );
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

/// Claude's `agent.keys_loading` from the host's start, as `SetupStart::launch_refusals` gives it.
fn loading(rig: &Rig) {
    lock(&rig.backend.wiring.refused).insert(claude(), ErrorCode::AgentKeysLoading);
}

fn lifted(rig: &Rig) -> bool {
    rig.host
        .calls()
        .contains(&HostCall::SetLaunchRefused(claude(), None))
}

/// At start the keys have loaded but the OS's route has not: `agent.keys_loading` stays, so a
/// launch waits; once the route lands it is lifted, and the launch gets the OS's proxy.
#[test]
fn a_launch_at_start_waits_for_the_route_and_then_gets_the_os_proxy() {
    let rig = rig_unrouted();
    loading(&rig);
    let mut lift = pin!(lift_at_start(&rig.backend, Duration::from_secs(60)));
    let mut context = Context::from_waker(std::task::Waker::noop());
    assert!(matches!(lift.as_mut().poll(&mut context), Poll::Pending));
    assert!(!lifted(&rig), "lifted before the route was known");
    let facts = NetworkFacts {
        api: [(claude(), "https://api.anthropic.com/".to_owned())].into(),
        ..NetworkFacts::default()
    };
    rig.backend.wiring.network.update(facts);
    block_on(lift);
    assert!(lifted(&rig));
    let hook = SetupHook::new(
        Arc::new(std::sync::OnceLock::new()),
        Arc::default(),
        Arc::clone(&rig.network),
    );
    let env = hook.launch_env(&claude());
    assert_eq!(
        env.get(std::ffi::OsStr::new("HTTPS_PROXY")),
        Some(&std::ffi::OsString::from(OS_PROXY))
    );
}

/// Keys that load slowly use up part of the route's wait rather than add to it: a route that
/// never lands lifts the refusal at the bound from the setup's start (when its network follow
/// started), not the bound from the keys' end.
#[test]
fn the_route_wait_counts_from_the_start_not_from_the_keys() {
    let rig = rig_unrouted();
    let started = SystemClock.monotonic();
    loading(&rig);
    // The keys' load.
    block_on(AsyncIoTimer.after(Duration::from_millis(500)));
    let lifting = SystemClock.monotonic();
    block_on(lift_at_start(&rig.backend, Duration::from_millis(600)));
    assert!(lifted(&rig));
    let waited = SystemClock.monotonic().saturating_sub(lifting);
    assert!(
        waited < Duration::from_millis(450),
        "{waited:?}: the wait started again after the keys"
    );
    assert!(SystemClock.monotonic().saturating_sub(started) >= Duration::from_millis(590));
}

/// A route that never lands still lifts the refusal once the wait is over.
#[test]
fn a_route_that_times_out_still_lifts_the_refusal() {
    let rig = rig_unrouted();
    loading(&rig);
    block_on(lift_at_start(&rig.backend, Duration::from_millis(20)));
    assert!(lifted(&rig));
}
