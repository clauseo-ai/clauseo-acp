//! A proxy is kept over a refresh's failure, or over one `DIRECT`, only while it still takes a
//! connection (app review of #656): a laptop whose VPN dropped, its PAC on an internal server
//! failing with it, goes direct at once; a flaky lookup, or a WPAD that read once as "no proxy
//! here", on the firm's network keeps the proxy. The probe ends within the lookup budget, and
//! none runs while the OS answers with proxies.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use clauseo_agents::{Clock as _, SystemClock};
use clauseo_process::{AsyncIoTimer, Timer as _};
use url::Url;

use super::super::super::{NetworkSlot, OsAnswer, ProxyEnv};
use super::super::{Follower, NetworkFollow};
use super::{OS_PROXY, claude, facts, hook_env, since};
use crate::bridge::lock;
use crate::setup::SetupHook;
use crate::test_support::block_on;

/// An OS whose answer, and how long it takes, the test sets.
#[derive(Clone)]
struct Os {
    answer: Arc<Mutex<(OsAnswer, Duration)>>,
}

impl Os {
    fn new(answer: OsAnswer) -> Self {
        Self {
            answer: Arc::new(Mutex::new((answer, Duration::ZERO))),
        }
    }

    fn set(&self, answer: OsAnswer, delay: Duration) {
        *lock(&self.answer) = (answer, delay);
    }

    fn lookup(&self) -> impl Fn(&Url) -> OsAnswer + Send + Sync + 'static {
        let os = self.clone();
        move |_target: &Url| {
            let (answer, delay) = lock(&os.answer).clone();
            if !delay.is_zero() {
                futures::executor::block_on(AsyncIoTimer.after(delay));
            }
            answer
        }
    }
}

/// A proxy probe the test sets: whether the proxy answers, how long the probe takes, and the
/// proxies it was asked about.
#[derive(Clone, Default)]
struct Probe {
    reachable: Arc<AtomicBool>,
    takes: Arc<Mutex<Duration>>,
    asked: Arc<Mutex<Vec<String>>>,
}

impl Probe {
    fn reachable(reachable: bool) -> Self {
        let probe = Self::default();
        probe.reachable.store(reachable, Ordering::SeqCst);
        probe
    }

    fn reach(&self) -> impl Fn(&str) -> bool + Send + Sync + 'static {
        let probe = self.clone();
        move |proxy: &str| {
            lock(&probe.asked).push(proxy.to_owned());
            let takes = *lock(&probe.takes);
            if !takes.is_zero() {
                futures::executor::block_on(AsyncIoTimer.after(takes));
            }
            probe.reachable.load(Ordering::SeqCst)
        }
    }

    fn asked(&self) -> Vec<String> {
        lock(&self.asked).clone()
    }
}

/// A route resolved on the caller's thread, stale at once (each refresh resolves again), within
/// `budget`, over `os` and `probe`; its launch hook. The first route is made: `OS_PROXY`.
fn inline(os: &Os, probe: &Probe, budget: Duration) -> (NetworkFollow, SetupHook) {
    let slot = NetworkSlot::default();
    let follower = Follower::unbound(Arc::clone(&slot), ProxyEnv::default, os.lookup(), true)
        .reach(probe.reach())
        .fresh_for(Duration::ZERO)
        .lookup_budget(budget);
    let network = NetworkFollow::inline(follower);
    let hook = SetupHook::new(Arc::new(OnceLock::new()), Arc::default(), slot);
    network.update(facts());
    assert_eq!(proxy_of(&hook).as_deref(), Some(OS_PROXY));
    (network, hook)
}

fn proxy_of(hook: &SetupHook) -> Option<String> {
    hook_env(hook, &claude()).get("HTTPS_PROXY").cloned()
}

/// Over `PROBE_CAP` (1 s), so a probe starts only when an answer asks for one, or a lookup is
/// still running 1 s before the budget ends, as with the real 4.75 s.
const BUDGET: Duration = Duration::from_millis(1_500);

/// VPN off: the PAC URL on the firm's server fails (no answer), and the kept proxy does not take
/// a connection: the refresh goes direct at once, not after 10 minutes.
#[test]
fn vpn_off_a_failed_pac_with_the_proxy_unreachable_goes_direct() {
    let os = Os::new(OsAnswer::Proxy(OS_PROXY.to_owned()));
    let probe = Probe::reachable(false);
    let (network, hook) = inline(&os, &probe, BUDGET);
    assert!(
        probe.asked().is_empty(),
        "no probe while the OS gives proxies"
    );
    os.set(OsAnswer::Unknown, Duration::ZERO);
    assert_eq!(network.refresher()().and_then(|n| n.proxy), None);
    assert_eq!(proxy_of(&hook), None);
    assert_eq!(probe.asked(), [OS_PROXY], "one probe per distinct proxy");
}

/// A flaky lookup on the firm's network (the proxy still answers) keeps the proxy.
#[test]
fn a_failed_lookup_with_the_proxy_reachable_keeps_it() {
    let os = Os::new(OsAnswer::Proxy(OS_PROXY.to_owned()));
    let probe = Probe::reachable(true);
    let (network, hook) = inline(&os, &probe, BUDGET);
    os.set(OsAnswer::Unknown, Duration::ZERO);
    let refresh = network.refresher();
    assert_eq!(refresh().and_then(|n| n.proxy).as_deref(), Some(OS_PROXY));
    assert_eq!(proxy_of(&hook).as_deref(), Some(OS_PROXY));
    assert_eq!(probe.asked(), [OS_PROXY]);
    os.set(OsAnswer::Proxy(OS_PROXY.to_owned()), Duration::ZERO);
    refresh();
    assert_eq!(probe.asked().len(), 1, "a proxy answer needs no probe");
}

/// A lookup that hangs past the budget (a PAC host that drops packets): the proxy is probed
/// while it runs, and the route made within the budget goes direct when it does not answer.
#[test]
fn a_hanging_lookup_with_the_proxy_unreachable_goes_direct_within_the_budget() {
    let os = Os::new(OsAnswer::Proxy(OS_PROXY.to_owned()));
    let probe = Probe::reachable(false);
    let budget = BUDGET;
    let (network, hook) = inline(&os, &probe, budget);
    *lock(&probe.takes) = Duration::from_millis(900);
    os.set(OsAnswer::Proxy(OS_PROXY.to_owned()), Duration::from_secs(3));
    let started = SystemClock.monotonic();
    assert_eq!(network.refresher()().and_then(|n| n.proxy), None);
    assert!(
        since(started) < budget + Duration::from_millis(300),
        "the probe ended within the budget"
    );
    assert_eq!(proxy_of(&hook), None);
    assert_eq!(probe.asked(), [OS_PROXY]);
}

/// One `DIRECT` (a WPAD 12167 once on the firm's network) with the proxy still answering keeps
/// it for that refresh; a second `DIRECT` goes direct.
#[test]
fn one_direct_with_the_proxy_reachable_keeps_it_and_a_second_goes_direct() {
    let os = Os::new(OsAnswer::Proxy(OS_PROXY.to_owned()));
    let probe = Probe::reachable(true);
    let (network, hook) = inline(&os, &probe, BUDGET);
    let refresh = network.refresher();
    os.set(OsAnswer::Direct, Duration::ZERO);
    assert_eq!(refresh().and_then(|n| n.proxy).as_deref(), Some(OS_PROXY));
    assert_eq!(proxy_of(&hook).as_deref(), Some(OS_PROXY), "one DIRECT");
    refresh();
    assert_eq!(proxy_of(&hook), None, "a second DIRECT");
    // A proxy answer between two DIRECTs starts the count again.
    os.set(OsAnswer::Proxy(OS_PROXY.to_owned()), Duration::ZERO);
    refresh();
    os.set(OsAnswer::Direct, Duration::ZERO);
    refresh();
    os.set(OsAnswer::Proxy(OS_PROXY.to_owned()), Duration::ZERO);
    refresh();
    os.set(OsAnswer::Direct, Duration::ZERO);
    refresh();
    assert_eq!(
        proxy_of(&hook).as_deref(),
        Some(OS_PROXY),
        "DIRECT, proxy, DIRECT: kept"
    );
}

/// `DIRECT` with the proxy no longer answering (office to home) goes direct at once.
#[test]
fn a_direct_with_the_proxy_unreachable_goes_direct_at_once() {
    let os = Os::new(OsAnswer::Proxy(OS_PROXY.to_owned()));
    let probe = Probe::reachable(false);
    let (network, hook) = inline(&os, &probe, BUDGET);
    os.set(OsAnswer::Direct, Duration::ZERO);
    assert_eq!(network.refresher()().and_then(|n| n.proxy), None);
    assert_eq!(proxy_of(&hook), None);
    assert_eq!(probe.asked(), [OS_PROXY]);
}

/// A late `DIRECT` (past the budget) over a proxy that still answers is kept once too, on the
/// network thread; the next late `DIRECT` goes direct.
#[test]
fn a_late_direct_over_a_reachable_proxy_is_kept_once() {
    let os = Os::new(OsAnswer::Proxy(OS_PROXY.to_owned()));
    let probe = Probe::reachable(true);
    let slot = NetworkSlot::default();
    let follower = Follower::unbound(Arc::clone(&slot), ProxyEnv::default, os.lookup(), true)
        .reach(probe.reach())
        .lookup_budget(BUDGET);
    let network = super::super::follow(follower, SystemClock.monotonic());
    let hook = SetupHook::new(Arc::new(OnceLock::new()), Arc::default(), slot);
    network.update(facts());
    block_on(network.ready());
    assert_eq!(proxy_of(&hook).as_deref(), Some(OS_PROXY));
    os.set(OsAnswer::Direct, BUDGET + Duration::from_millis(300));
    // The same read again: a resolve of the same hosts, whose lookups land late.
    network.update(facts());
    // Past the late answers and their hand-over.
    block_on(AsyncIoTimer.after(BUDGET + Duration::from_millis(900)));
    assert_eq!(
        proxy_of(&hook).as_deref(),
        Some(OS_PROXY),
        "one late DIRECT, the proxy answering"
    );
    network.update(facts());
    let started = SystemClock.monotonic();
    while proxy_of(&hook).is_some() {
        assert!(
            since(started) < Duration::from_secs(5),
            "the second late DIRECT never landed"
        );
        block_on(AsyncIoTimer.after(Duration::from_millis(20)));
    }
}
