//! The network thread's step: each change reaches `Agents` and the launch env and is logged
//! once, by source, scheme, host and port, never with a password.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

use clauseo_host::ProfileHook as _;
use clauseo_protocol::AgentId;
use url::Url;

use super::super::{NetworkFacts, NetworkRoute, NetworkSlot, ProxyEnv};
use super::{Follower, NetworkFollow};
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
