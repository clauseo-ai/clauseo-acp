//! A refresh never turns a proxy into direct on a transient failure (review High): a lookup with
//! no answer keeps the last route's answer when the same hosts are asked (whatever else in the
//! settings changed), only `DIRECT` takes it away, and a first route or other hosts with no
//! answer go direct. Late answers apply only to the route that asked for
//! them, and the network thread ends once its `NetworkFollow` is gone.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use clauseo_agents::{Clock as _, SystemClock};
use clauseo_process::{AsyncIoTimer, Timer as _};
use clauseo_sandbox::{Route, UpstreamAnswer};
use url::Url;

use super::super::super::{
    Answered, KEEP_FOR, Known, NetworkFacts, NetworkSlot, OsAnswer, Probed, ProxyEnv, os_answer,
};
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
        .reach(|_proxy| true)
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

/// The same facts but for Claude's API host (a firm plan's endpoint): the OS is asked about
/// another host.
fn other_hosts() -> NetworkFacts {
    let mut other = facts();
    other
        .api
        .insert(claude(), "https://llm.firm.example/".to_owned());
    other
}

/// On a refresh of the same read, a lookup that fails (or finds no slot: the sandbox's
/// `Failed`) or runs past the budget keeps the previous proxy while it answers; two explicit
/// `DIRECT`s in a row take it away.
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
    refresh();
    assert_eq!(
        proxy_of(&hook).as_deref(),
        Some(OS_PROXY),
        "one DIRECT, the proxy still answering"
    );
    assert_eq!(refresh().and_then(|n| n.proxy), None);
    assert_eq!(proxy_of(&hook), None, "a second DIRECT");
    assert_eq!(hook_env(&hook, &codex()).get("HTTPS_PROXY"), None);
}

/// A failed lookup after a cached `DIRECT` or proxy keeps that answer for its target when the
/// same hosts are asked; an explicit answer replaces it (a `DIRECT` over a proxy found dead); with
/// no earlier answers it stays unknown.
#[test]
fn a_failure_keeps_a_cached_direct_or_proxy_for_the_same_hosts() {
    let (direct, proxied) = (
        Url::parse("https://direct.example/").unwrap(),
        Url::parse("https://proxied.example/").unwrap(),
    );
    let last: Answered = [
        (direct.clone(), OsAnswer::Direct),
        (proxied.clone(), OsAnswer::Proxy(OS_PROXY.to_owned())),
    ]
    .into();
    let failed: Answered = [
        (direct.clone(), OsAnswer::Unknown),
        (proxied.clone(), OsAnswer::Unknown),
    ]
    .into();
    let at = Duration::from_secs(100);
    let known = Known::default().merge(last.clone(), at, KEEP_FOR, &Probed::new());
    let later = at + Duration::from_secs(60);
    assert_eq!(
        known
            .merge(failed.clone(), later, KEEP_FOR, &Probed::new())
            .answers,
        last
    );
    let nothing = Known::default().merge(failed.clone(), later, KEEP_FOR, &Probed::new());
    assert_eq!(nothing.answers, failed, "nothing to keep");
    let answered: Answered = [
        (direct, OsAnswer::Proxy(NEW_PROXY.to_owned())),
        (proxied, OsAnswer::Direct),
    ]
    .into();
    let dead: Probed = [(OS_PROXY.to_owned(), false)].into();
    assert_eq!(
        known
            .merge(answered.clone(), later, KEEP_FOR, &dead)
            .answers,
        answered
    );
    // Through the follower: a cached DIRECT, then a failure on a refresh, stays direct.
    let os = Os::new(OsAnswer::Direct);
    let (network, hook) = inline(&os);
    let refresh = network.refresher();
    network.update(facts());
    assert_eq!(proxy_of(&hook), None);
    os.set(OsAnswer::Unknown, Duration::ZERO);
    refresh();
    assert_eq!(proxy_of(&hook), None, "a failure after DIRECT");
    os.set(OsAnswer::Proxy(OS_PROXY.to_owned()), Duration::ZERO);
    refresh();
    assert_eq!(proxy_of(&hook).as_deref(), Some(OS_PROXY));
}

/// A first route with no answer goes direct, as before; so does one that asks the OS about
/// other hosts, whatever the last route had.
#[test]
fn a_first_route_or_other_hosts_with_no_answer_go_direct() {
    let os = Os::new(OsAnswer::Unknown);
    let (network, hook) = inline(&os);
    network.update(facts());
    assert_eq!(proxy_of(&hook), None, "a first route");
    os.set(OsAnswer::Proxy(OS_PROXY.to_owned()), Duration::ZERO);
    network.update(facts());
    assert_eq!(proxy_of(&hook).as_deref(), Some(OS_PROXY));
    os.set(OsAnswer::Unknown, Duration::ZERO);
    network.update(other_hosts());
    assert_eq!(proxy_of(&hook), None, "another API host");
}

/// Settings that change only what the OS is not asked about (an agent entry's own variables, a
/// CA file) keep the proxy over a failed lookup (delta review Low).
#[test]
fn an_entry_env_or_ca_changed_with_the_same_hosts_keeps_the_proxy_over_a_failure() {
    let os = Os::new(OsAnswer::Proxy(OS_PROXY.to_owned()));
    let (network, hook) = inline(&os);
    network.update(facts());
    assert_eq!(proxy_of(&hook).as_deref(), Some(OS_PROXY));
    os.set(OsAnswer::Unknown, Duration::ZERO);
    let mut entry_env = facts();
    entry_env
        .own_vars
        .insert(claude(), ["ANTHROPIC_LOG".to_owned()].into());
    network.update(entry_env);
    assert_eq!(
        proxy_of(&hook).as_deref(),
        Some(OS_PROXY),
        "an entry's own env"
    );
    network.update(other_facts());
    assert_eq!(proxy_of(&hook).as_deref(), Some(OS_PROXY), "a CA file");
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
        .reach(|_proxy| true)
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
    let follower =
        Follower::unbound(Arc::clone(&slot), ProxyEnv::default, os, true).reach(|_proxy| true);
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

/// A monotonic clock the test moves.
#[derive(Clone, Default)]
struct FakeClock(Arc<Mutex<Duration>>);

impl FakeClock {
    fn advance(&self, by: Duration) {
        *lock(&self.0) += by;
    }
}

impl clauseo_agents::Clock for FakeClock {
    fn now(&self) -> std::time::SystemTime {
        std::time::UNIX_EPOCH + *lock(&self.0)
    }
    fn monotonic(&self) -> Duration {
        *lock(&self.0)
    }
}

/// A proxy is kept over failed lookups for at most 10 minutes from when a real lookup last gave
/// it; a real answer meanwhile confirms it again; past the 10 minutes a failure goes direct, as
/// in a first route (stage C's backstop: a new network that fails every lookup).
#[test]
fn a_kept_proxy_expires_ten_minutes_after_a_real_lookup_last_gave_it() {
    assert_eq!(KEEP_FOR, Duration::from_mins(10));
    let clock = FakeClock::default();
    let os = Os::new(OsAnswer::Proxy(OS_PROXY.to_owned()));
    let slot = NetworkSlot::default();
    let follower = Follower::unbound(Arc::clone(&slot), ProxyEnv::default, os.lookup(), true)
        .reach(|_proxy| true)
        .fresh_for(Duration::ZERO)
        .clock(Arc::new(clock.clone()));
    let network = NetworkFollow::inline(follower);
    let hook = SetupHook::new(Arc::new(OnceLock::new()), Arc::default(), slot);
    let refresh = network.refresher();
    network.update(facts());
    os.set(OsAnswer::Unknown, Duration::ZERO);
    clock.advance(Duration::from_mins(9));
    refresh();
    assert_eq!(proxy_of(&hook).as_deref(), Some(OS_PROXY), "at 9 min: kept");
    os.set(OsAnswer::Proxy(OS_PROXY.to_owned()), Duration::ZERO);
    refresh();
    os.set(OsAnswer::Unknown, Duration::ZERO);
    clock.advance(Duration::from_mins(9));
    refresh();
    assert_eq!(
        proxy_of(&hook).as_deref(),
        Some(OS_PROXY),
        "confirmed at 9 min, kept at 18"
    );
    clock.advance(Duration::from_secs(60));
    refresh();
    assert_eq!(
        proxy_of(&hook),
        None,
        "10 min after the last real answer: direct"
    );
}
