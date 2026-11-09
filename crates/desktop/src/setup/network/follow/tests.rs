//! The network thread's step: each change reaches `Agents` and the launch env and is logged
//! once, by source, scheme, host and port, never with a password.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use clauseo_agents::{Clock as _, SystemClock};
use clauseo_host::ProfileHook as _;
use clauseo_process::{AsyncIoTimer, Timer as _};
use clauseo_protocol::AgentId;
use url::Url;

use super::super::{
    NetworkFacts, NetworkRoute, NetworkSlot, OsAnswer, OsLookup, Probe, ProxyEnv, RealTime, ask_all,
};
use super::{FIRST_ROUTE_WAIT, Follower, NetworkFollow, OS_LOOKUP_BUDGET, follow};
use crate::bridge::lock;
use crate::setup::SetupHook;
use crate::setup::backend::SetupAgents;
use crate::test_support::Scratch;

const FIRM: &str = "http://proxy.firm.example:3128";
const SECRET: &str = "hunter2-secret";

fn claude() -> AgentId {
    AgentId::new("claude-acp").unwrap()
}

fn codex() -> AgentId {
    AgentId::new("codex-acp").unwrap()
}

/// The facts the embedded snapshot gives both agents, with no setting.
fn facts() -> NetworkFacts {
    let scratch = Scratch::folder("network-follow-facts", &[]);
    let effective = clauseo_settings::Effective::load(
        &clauseo_settings::Locations::under_root(scratch.path()),
        &clauseo_settings::SystemFiles,
        &[],
    );
    NetworkFacts::read(&effective, [&claude(), &codex()])
}

fn hook_env(hook: &SetupHook, agent: &AgentId) -> BTreeMap<String, String> {
    hook.launch_env(agent)
        .into_iter()
        .map(|(name, value)| (name.into_string().unwrap(), value.into_string().unwrap()))
        .collect()
}

/// Records each `set_network`; nothing else is called.
#[derive(Default)]
struct Networks(Mutex<Vec<clauseo_agents::Network>>);

impl SetupAgents for Networks {
    fn install(&self, _agent: &AgentId) {}
    fn retry(&self, _agent: &AgentId, _use_system_proxy: bool) {}
    fn state(&self, _agent: &AgentId) -> Option<clauseo_agents::InstallState> {
        None
    }
    fn installed(&self, _agent: &AgentId) -> Option<clauseo_agents::Installed> {
        None
    }
    fn sign_in(
        &self,
        _agent: &AgentId,
        _method: clauseo_agents::Method,
    ) -> Result<(), clauseo_agents::SigninError> {
        Ok(())
    }
    fn cancel_sign_in(&self, _agent: &AgentId) -> Result<(), clauseo_agents::SigninError> {
        Ok(())
    }
    fn paste_code(
        &self,
        _agent: &AgentId,
        _code: clauseo_agents::PasteCode,
    ) -> Result<(), clauseo_agents::SigninError> {
        Ok(())
    }
    fn sign_out(&self, _agent: &AgentId) -> Result<(), clauseo_agents::SigninError> {
        Ok(())
    }
    fn check_status(&self, _agent: &AgentId) -> Result<(), clauseo_agents::SigninError> {
        Ok(())
    }
    fn find_login(&self, _agent: &AgentId) -> Result<(), clauseo_agents::SigninError> {
        Ok(())
    }
    fn use_found_login(&self, _agent: &AgentId) -> Result<(), clauseo_agents::SigninError> {
        Ok(())
    }
    fn use_different_account(&self, _agent: &AgentId) -> Result<(), clauseo_agents::SigninError> {
        Ok(())
    }
    fn signin_state(&self, _agent: &AgentId) -> Option<clauseo_agents::SigninState> {
        None
    }
    fn sign_in_going(&self, _agent: &AgentId) -> bool {
        false
    }
    fn stop_using_login(
        &self,
        _agent: &AgentId,
        _why: Option<clauseo_agents::Found>,
    ) -> Result<(), clauseo_agents::SigninError> {
        Ok(())
    }
    fn install_tries(&self, _agent: &AgentId) -> Option<u32> {
        None
    }
    fn read_account(&self, _agent: &AgentId) -> clauseo_agents::AccountFuture {
        Box::pin(std::future::ready(clauseo_agents::AccountRead::Failed))
    }
    fn read_account_refreshed(&self, _agent: &AgentId) -> clauseo_agents::AccountFuture {
        Box::pin(std::future::ready(clauseo_agents::AccountRead::Failed))
    }
    fn shutdown(&self, _wait: std::time::Duration) -> bool {
        true
    }
    fn set_network(&self, network: clauseo_agents::Network) {
        lock(&self.0).push(network);
    }
}

/// Log lines, as the subscriber writes them.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Lines {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        std::io::Write::write(&mut *lock(&self.0), buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Lines {
    fn text(&self) -> String {
        String::from_utf8_lossy(&lock(&self.0)).into_owned()
    }
}

#[test]
fn each_change_reaches_agents_and_the_launch_env_and_is_logged_once() {
    let lines = Lines::default();
    let writer = lines.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    let slot = NetworkSlot::default();
    let agents = Arc::new(Networks::default());
    let mut follower = Follower::new(
        Arc::clone(&slot),
        Arc::clone(&agents) as Arc<dyn SetupAgents>,
        ProxyEnv::default,
        |_target: &Url| None,
        true,
    );
    let hook = SetupHook::new(Arc::new(OnceLock::new()), Arc::default(), Arc::clone(&slot));
    let with_secret = format!("http://user:{SECRET}@proxy.firm.example:3128/path?token={SECRET}");
    tracing::subscriber::with_default(subscriber, || {
        follower.take(&facts());
        assert!(!hook_env(&hook, &claude()).contains_key("HTTPS_PROXY"));
        let mut set = facts();
        set.upstream = Some(with_secret.clone());
        follower.take(&set);
        // The same read again: nothing new to log or to give `Agents`.
        follower.take(&set);
        set.upstream = Some("https://tls-proxy.firm.example".to_owned());
        follower.take(&set);
    });
    let pushed = lock(&agents.0).clone();
    assert_eq!(
        pushed.len(),
        3,
        "the first, the secret one and the https one"
    );
    assert_eq!(pushed[0].proxy, None);
    // The consumers get the URL as written: userinfo is theirs to use.
    assert_eq!(pushed[1].proxy.as_deref(), Some(with_secret.as_str()));
    assert_eq!(
        pushed[1].signin_proxy.get(&codex()),
        Some(&Some(with_secret.clone()))
    );
    assert_eq!(
        hook_env(&hook, &claude())["HTTPS_PROXY"],
        "https://tls-proxy.firm.example"
    );
    let text = lines.text();
    assert_eq!(text.matches("the agents' proxy").count(), 3, "{text}");
    assert!(text.contains("route=none"), "{text}");
    assert!(
        text.contains("setting http://proxy.firm.example:3128"),
        "{text}"
    );
    assert!(
        text.contains("setting https://tls-proxy.firm.example:443"),
        "{text}"
    );
    assert_eq!(
        text.matches("chains only to http:// proxies").count(),
        1,
        "{text}"
    );
    for printed in [
        text,
        format!("{:?}", current_of(&slot)),
        format!("{:?}", pushed[1]),
        format!("{set:?}", set = with_setting(&with_secret)),
    ] {
        assert!(!printed.contains(SECRET), "{printed}");
        assert!(!printed.contains("user:"), "{printed}");
        assert!(!printed.contains("/path"), "{printed}");
    }
}

fn current_of(slot: &NetworkSlot) -> NetworkRoute {
    slot.read().unwrap().clone()
}

fn with_setting(setting: &str) -> NetworkFacts {
    let mut set = facts();
    set.upstream = Some(setting.to_owned());
    set
}

#[test]
fn a_refresh_through_the_settings_changes_the_next_install_sign_in_and_launch() {
    // The setup backend's own follow: `apply_settings` hands the read's facts to the
    // follower, which gives `Agents` and the launch hook the new route.
    let slot = NetworkSlot::default();
    let agents = Arc::new(Networks::default());
    let follow = NetworkFollow::inline(Follower::new(
        Arc::clone(&slot),
        Arc::clone(&agents) as Arc<dyn SetupAgents>,
        ProxyEnv::default,
        |_target: &Url| None,
        true,
    ));
    let hook = SetupHook::new(Arc::new(OnceLock::new()), Arc::default(), Arc::clone(&slot));
    follow.update(with_setting(FIRM));
    assert_eq!(lock(&agents.0).last().unwrap().proxy.as_deref(), Some(FIRM));
    assert_eq!(hook_env(&hook, &codex())["HTTPS_PROXY"], FIRM);
    follow.update(facts());
    assert_eq!(lock(&agents.0).last().unwrap().proxy, None);
    assert!(!hook_env(&hook, &codex()).contains_key("HTTPS_PROXY"));
}

const OS_PROXY: &str = "http://os-proxy.firm.example:8080";

/// The time since `start`, a monotonic reading.
fn since(start: Duration) -> Duration {
    SystemClock.monotonic().saturating_sub(start)
}

/// An OS lookup that takes `slow` for a host whose name starts `slow`, `each` for others.
fn slow_os(each: Duration, slow: Duration) -> Arc<OsLookup> {
    Arc::new(move |target: &Url| {
        let slow_host = target
            .host_str()
            .is_some_and(|host| host.starts_with("slow"));
        futures::executor::block_on(AsyncIoTimer.after(if slow_host { slow } else { each }));
        OsAnswer::Proxy(OS_PROXY.to_owned())
    })
}

fn targets(hosts: &[&str]) -> BTreeSet<Url> {
    hosts
        .iter()
        .map(|host| Url::parse(&format!("https://{host}/")).unwrap())
        .collect()
}

/// The OS is asked about every target at once: five slow lookups take about one's time.
#[test]
fn five_slow_lookups_take_about_one_lookups_time() {
    let os = slow_os(Duration::from_millis(400), Duration::ZERO);
    let asked = SystemClock.monotonic();
    let hosts = [
        "a.example",
        "b.example",
        "c.example",
        "d.example",
        "e.example",
    ];
    let answers = ask_all(
        targets(&hosts),
        &os,
        Duration::from_secs(5),
        Probe::none(),
        &RealTime::shared(),
    )
    .got;
    let took = since(asked);
    assert_eq!(answers.len(), 5);
    assert!(
        answers
            .values()
            .all(|answer| answer.proxy().as_deref() == Some(OS_PROXY))
    );
    assert!(
        took < Duration::from_millis(1_200),
        "{took:?}: one after another"
    );
}

/// A lookup past the budget has no answer in the route; the answers in time are kept, and the
/// route is made by the budget, not by the slow lookup.
#[test]
fn a_lookup_past_the_budget_has_no_answer_and_the_rest_are_kept() {
    let os = slow_os(Duration::ZERO, Duration::from_secs(3));
    let asked = SystemClock.monotonic();
    let budget = Duration::from_millis(300);
    let answers = ask_all(
        targets(&["slow.example", "fast.example"]),
        &os,
        budget,
        Probe::none(),
        &RealTime::shared(),
    )
    .got;
    let took = since(asked);
    assert!(took >= budget && took < Duration::from_secs(2), "{took:?}");
    let fast = Url::parse("https://fast.example/").unwrap();
    assert_eq!(
        answers.get(&fast).and_then(OsAnswer::proxy).as_deref(),
        Some(OS_PROXY)
    );
    let slow = Url::parse("https://slow.example/").unwrap();
    assert_eq!(answers.get(&slow), Some(&OsAnswer::Unknown));
    // The whole resolution fits the 5 s budget, a margin left to hand the route on.
    assert!(OS_LOOKUP_BUDGET < FIRST_ROUTE_WAIT);
    assert_eq!(FIRST_ROUTE_WAIT, Duration::from_secs(5));
}

/// The start hands the read to the network thread and goes on at once, however slow the OS
/// lookups (so neither the window nor the keys' load waits on them); the route lands within the
/// budget, and `Agents`, bound after it, gets it then.
#[test]
fn the_start_never_waits_on_the_lookups_and_agents_bound_later_get_the_route() {
    let slot = NetworkSlot::default();
    let started = SystemClock.monotonic();
    let os = slow_os(Duration::from_millis(500), Duration::ZERO);
    let follower = Follower::unbound(
        Arc::clone(&slot),
        ProxyEnv::default,
        move |target: &Url| os(target),
        true,
    )
    .reach(|_proxy| true);
    let network = follow(follower, started);
    network.update(facts());
    assert!(
        since(started) < Duration::from_millis(300),
        "{:?}: the start waited on the lookups",
        since(started)
    );
    crate::test_support::block_on(network.ready());
    assert!(since(started) < FIRST_ROUTE_WAIT, "{:?}", since(started));
    let route = current_of(&slot);
    assert_eq!(route.agents[&claude()].launch.as_deref(), Some(OS_PROXY));
    let agents = Arc::new(Networks::default());
    network.bind(Arc::clone(&agents) as Arc<dyn SetupAgents>);
    let pushed = lock(&agents.0).clone();
    assert_eq!(pushed.len(), 1, "the route resolved before the bind");
    assert_eq!(pushed[0].proxy.as_deref(), Some(OS_PROXY));
}

/// An OS whose answer the test switches, counting its lookups.
struct Switchable {
    answer: Arc<Mutex<String>>,
    lookups: Arc<std::sync::atomic::AtomicUsize>,
}

impl Switchable {
    fn new(answer: &str) -> Self {
        Self {
            answer: Arc::new(Mutex::new(answer.to_owned())),
            lookups: Arc::default(),
        }
    }

    fn lookup(&self) -> impl Fn(&Url) -> Option<String> + Send + Sync + 'static {
        let (answer, lookups) = (Arc::clone(&self.answer), Arc::clone(&self.lookups));
        move |_target: &Url| {
            lookups.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Some(lock(&answer).clone())
        }
    }

    fn switch(&self, answer: &str) {
        answer.clone_into(&mut lock(&self.answer));
    }

    fn lookups(&self) -> usize {
        self.lookups.load(std::sync::atomic::Ordering::SeqCst)
    }
}

const NEW_PROXY: &str = "http://vpn-proxy.firm.example:3128";

/// A route followed on its own thread, fresh for `fresh_for`, from `os`; its slot and the
/// launch hook over it, once the first route has landed.
fn followed(os: &Switchable, fresh_for: Duration) -> (NetworkFollow, SetupHook) {
    let slot = NetworkSlot::default();
    let follower = Follower::unbound(Arc::clone(&slot), ProxyEnv::default, os.lookup(), true)
        .reach(|_proxy| true)
        .fresh_for(fresh_for);
    let network = follow(follower, SystemClock.monotonic());
    network.update(facts());
    crate::test_support::block_on(network.ready());
    let hook = SetupHook::new(Arc::new(OnceLock::new()), Arc::default(), slot);
    (network, hook)
}

/// The network changes: within the freshness an install try's refresh and a launch take the
/// route as it is, with no OS lookup; past it the try's refresh resolves again and gets the new
/// proxy, and the next launch takes it too.
#[test]
fn past_the_freshness_an_install_try_and_the_next_launch_get_the_new_proxy() {
    let os = Switchable::new(OS_PROXY);
    let fresh_for = Duration::from_secs(1);
    let (network, hook) = followed(&os, fresh_for);
    let refresh = network.refresher();
    assert_eq!(hook_env(&hook, &claude())["HTTPS_PROXY"], OS_PROXY);
    let asked = os.lookups();
    os.switch(NEW_PROXY);
    assert_eq!(refresh().and_then(|n| n.proxy).as_deref(), Some(OS_PROXY));
    assert_eq!(hook_env(&hook, &claude())["HTTPS_PROXY"], OS_PROXY);
    assert_eq!(os.lookups(), asked, "within the freshness: no lookup");
    crate::test_support::block_on(AsyncIoTimer.after(fresh_for));
    assert_eq!(refresh().and_then(|n| n.proxy).as_deref(), Some(NEW_PROXY));
    assert_eq!(hook_env(&hook, &claude())["HTTPS_PROXY"], NEW_PROXY);
    assert!(os.lookups() > asked);
}

/// With nothing asking, a stale route is resolved again by the network thread itself, so a
/// launch, which never waits, finds the new proxy.
#[test]
fn a_stale_route_is_resolved_again_by_itself_for_the_next_launch() {
    let os = Switchable::new(OS_PROXY);
    let fresh_for = Duration::from_millis(300);
    let (_network, hook) = followed(&os, fresh_for);
    os.switch(NEW_PROXY);
    let switched = SystemClock.monotonic();
    while hook_env(&hook, &claude())["HTTPS_PROXY"] != NEW_PROXY {
        assert!(
            since(switched) < Duration::from_secs(10),
            "never resolved again"
        );
        crate::test_support::block_on(AsyncIoTimer.after(Duration::from_millis(50)));
    }
}

/// An OS answer past the budget is not lost: the route is made again once it lands, with the
/// answers already got and no new lookup, so the launch takes the slow host's proxy.
#[test]
fn a_lookup_past_the_budget_reaches_the_route_once_it_answers() {
    const LATE_PROXY: &str = "http://wpad-proxy.firm.example:8080";
    let lookups = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = Arc::clone(&lookups);
    let os = move |target: &Url| {
        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if target.host_str() == Some("api.anthropic.com") {
            futures::executor::block_on(AsyncIoTimer.after(Duration::from_millis(700)));
            Some(LATE_PROXY.to_owned())
        } else {
            Some(OS_PROXY.to_owned())
        }
    };
    let slot = NetworkSlot::default();
    let follower = Follower::unbound(Arc::clone(&slot), ProxyEnv::default, os, true)
        .reach(|_proxy| true)
        .lookup_budget(Duration::from_millis(200));
    let network = follow(follower, SystemClock.monotonic());
    network.update(facts());
    crate::test_support::block_on(network.ready());
    let hook = SetupHook::new(Arc::new(OnceLock::new()), Arc::default(), slot);
    assert!(
        !hook_env(&hook, &claude()).contains_key("HTTPS_PROXY"),
        "a first route: a lookup with no answer yet goes direct"
    );
    let asked = lookups.load(std::sync::atomic::Ordering::SeqCst);
    let started = SystemClock.monotonic();
    while hook_env(&hook, &claude())
        .get("HTTPS_PROXY")
        .map(String::as_str)
        != Some(LATE_PROXY)
    {
        assert!(
            since(started) < Duration::from_secs(10),
            "the late answer never landed"
        );
        crate::test_support::block_on(AsyncIoTimer.after(Duration::from_millis(50)));
    }
    assert_eq!(
        lookups.load(std::sync::atomic::Ordering::SeqCst),
        asked,
        "made again with no new lookup"
    );
    assert_eq!(hook_env(&hook, &codex())["HTTPS_PROXY"], OS_PROXY);
}

mod keep;
mod reach;
mod virtual_time;
