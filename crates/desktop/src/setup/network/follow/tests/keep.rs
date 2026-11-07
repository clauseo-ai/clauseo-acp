//! A refresh never turns a proxy into direct on a transient failure (review High): a lookup with
//! no answer keeps the last route's answer, only `DIRECT` takes it away, and a first route or
//! new settings with no answer go direct. Late answers apply only to the route that asked for
//! them, and the network thread ends once its `NetworkFollow` is gone.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use clauseo_agents::{Clock as _, SystemClock};
use clauseo_process::{AsyncIoTimer, Timer as _};
use clauseo_sandbox::{Route, UpstreamAnswer};
use url::Url;

use super::super::super::{NetworkFacts, NetworkSlot, OsAnswer, ProxyEnv, os_answer};
use super::super::{Follower, NetworkFollow, follow};
use super::{OS_PROXY, claude, codex, facts, hook_env, since};
use crate::bridge::lock;
use crate::setup::SetupHook;
use crate::test_support::block_on;

const NEW_PROXY: &str = "http://vpn-proxy.firm.example:3128";

/// An OS whose answer, and how long it takes, the test sets.
#[derive(Clone)]
struct Os {
    answer: Arc<Mutex<OsAnswer>>,
    delay: Arc<Mutex<Duration>>,
}

impl Os {
    fn new(answer: OsAnswer) -> Self {
        Self {
            answer: Arc::new(Mutex::new(answer)),
            delay: Arc::default(),
        }
    }

    fn set(&self, answer: OsAnswer, delay: Duration) {
        *lock(&self.answer) = answer;
        *lock(&self.delay) = delay;
    }

    fn lookup(&self) -> impl Fn(&Url) -> OsAnswer + Send + Sync + 'static {
        let os = self.clone();
        move |_target: &Url| {
            let delay = *lock(&os.delay);
            if !delay.is_zero() {
                futures::executor::block_on(AsyncIoTimer.after(delay));
            }
            lock(&os.answer).clone()
        }
    }
}

/// A route resolved on the caller's thread, stale at once (each refresh resolves again), with a
/// 200 ms budget; its launch hook.
fn inline(os: &Os) -> (NetworkFollow, SetupHook) {
    let slot = NetworkSlot::default();
    let follower = Follower::unbound(Arc::clone(&slot), ProxyEnv::default, os.lookup(), true)
        .fresh_for(Duration::ZERO)
        .lookup_budget(Duration::from_millis(200));
    let network = NetworkFollow::inline(follower);
    let hook = SetupHook::new(Arc::new(OnceLock::new()), Arc::default(), slot);
    (network, hook)
}

fn proxy_of(hook: &SetupHook) -> Option<String> {
    hook_env(hook, &claude()).get("HTTPS_PROXY").cloned()
}

/// The same facts but for a CA file: new settings that ask the OS about the same hosts.
fn other_facts() -> NetworkFacts {
    let mut other = facts();
    other
        .ca_certs
        .insert(claude(), PathBuf::from("/firm/other-ca.pem"));
    other
}

/// On a refresh of the same read, a lookup that fails (or finds no slot: the sandbox's
/// `Failed`) or runs past the budget keeps the previous proxy; an explicit `DIRECT` takes it
/// away.
#[test]
fn a_refresh_keeps_the_previous_proxy_when_a_lookup_fails_or_is_late() {
    let os = Os::new(OsAnswer::Proxy(OS_PROXY.to_owned()));
    let (network, hook) = inline(&os);
    let refresh = network.refresher();
    network.update(facts());
    assert_eq!(proxy_of(&hook).as_deref(), Some(OS_PROXY));
    os.set(OsAnswer::Unknown, Duration::ZERO);
    assert_eq!(refresh().and_then(|n| n.proxy).as_deref(), Some(OS_PROXY));
    assert_eq!(
        proxy_of(&hook).as_deref(),
        Some(OS_PROXY),
        "a failed lookup"
    );
    os.set(
        OsAnswer::Proxy(NEW_PROXY.to_owned()),
        Duration::from_millis(700),
    );
    refresh();
    assert_eq!(
        proxy_of(&hook).as_deref(),
        Some(OS_PROXY),
        "a lookup past the budget"
    );
    os.set(OsAnswer::Direct, Duration::ZERO);
    assert_eq!(refresh().and_then(|n| n.proxy), None);
    assert_eq!(proxy_of(&hook), None, "an explicit DIRECT");
    assert_eq!(hook_env(&hook, &codex()).get("HTTPS_PROXY"), None);
}

/// A first route with no answer goes direct, as before; so does one after the settings changed,
/// whatever the last route had.
#[test]
fn a_first_route_or_new_settings_with_no_answer_go_direct() {
    let os = Os::new(OsAnswer::Unknown);
    let (network, hook) = inline(&os);
    network.update(facts());
    assert_eq!(proxy_of(&hook), None, "a first route");
    os.set(OsAnswer::Proxy(OS_PROXY.to_owned()), Duration::ZERO);
    network.update(facts());
    assert_eq!(proxy_of(&hook).as_deref(), Some(OS_PROXY));
    os.set(OsAnswer::Unknown, Duration::ZERO);
    network.update(other_facts());
    assert_eq!(proxy_of(&hook), None, "new settings");
}

/// The sandbox's failed lookup (failed, timed out, no slot) is no answer, apart from `DIRECT`.
#[test]
fn the_sandboxs_failure_is_no_answer_and_its_direct_is_direct() {
    assert_eq!(os_answer(Ok(UpstreamAnswer::Failed)), OsAnswer::Unknown);
    let error = clauseo_sandbox::Error::ProxyConfig("bad".to_owned());
    assert_eq!(os_answer(Err(error)), OsAnswer::Unknown);
    let direct = os_answer(Ok(UpstreamAnswer::Route(Route::Direct)));
    assert_eq!(direct, OsAnswer::Direct);
    let proxy = Route::Http {
        host: "::1".to_owned(),
        port: 3128,
    };
    let proxy = os_answer(Ok(UpstreamAnswer::Route(proxy)));
    assert_eq!(proxy, OsAnswer::Proxy("http://[::1]:3128".to_owned()));
}

/// New settings arrive while a lookup of the old ones is still running past the budget: its
/// late answer is dropped, so it never overwrites the newer route.
#[test]
fn a_late_answer_for_older_settings_is_dropped() {
    const LATE_PROXY: &str = "http://wpad-proxy.firm.example:8080";
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let os = move |target: &Url| {
        if target.host_str() != Some("api.anthropic.com") {
            return OsAnswer::Proxy(OS_PROXY.to_owned());
        }
        if counted.fetch_add(1, Ordering::SeqCst) == 0 {
            futures::executor::block_on(AsyncIoTimer.after(Duration::from_millis(700)));
            OsAnswer::Proxy(LATE_PROXY.to_owned())
        } else {
            OsAnswer::Proxy(NEW_PROXY.to_owned())
        }
    };
    let slot = NetworkSlot::default();
    let follower = Follower::unbound(Arc::clone(&slot), ProxyEnv::default, os, true)
        .lookup_budget(Duration::from_millis(200));
    let network = follow(follower, SystemClock.monotonic());
    let started = SystemClock.monotonic();
    network.update(facts());
    block_on(network.ready());
    let hook = SetupHook::new(Arc::new(OnceLock::new()), Arc::default(), slot);
    assert_eq!(proxy_of(&hook), None, "the first lookup is late");
    network.update(other_facts());
    while proxy_of(&hook).as_deref() != Some(NEW_PROXY) {
        assert!(
            since(started) < Duration::from_secs(5),
            "the new route never came"
        );
        block_on(AsyncIoTimer.after(Duration::from_millis(20)));
    }
    // Past the late lookup's end (700 ms) and its hand-over.
    block_on(AsyncIoTimer.after(Duration::from_millis(1_200)));
    assert_eq!(proxy_of(&hook).as_deref(), Some(NEW_PROXY));
}

/// Dropping the `NetworkFollow` ends the network thread: it lets go of the slot within a bound.
#[test]
fn dropping_the_follow_ends_the_network_thread() {
    let slot = NetworkSlot::default();
    let os = |_target: &Url| Some(OS_PROXY.to_owned());
    let follower = Follower::unbound(Arc::clone(&slot), ProxyEnv::default, os, true);
    let network = follow(follower, SystemClock.monotonic());
    network.update(facts());
    block_on(network.ready());
    assert!(Arc::strong_count(&slot) > 1, "the thread holds the slot");
    drop(network);
    let dropped = SystemClock.monotonic();
    while Arc::strong_count(&slot) > 1 {
        assert!(
            since(dropped) < Duration::from_secs(2),
            "the network thread never ended"
        );
        block_on(AsyncIoTimer.after(Duration::from_millis(20)));
    }
}
