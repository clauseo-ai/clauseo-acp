//! The agents' network route: the resolver's order (setting, environment, OS, and the OS
//! fallback off), policy over the lawyer's setting, empty values as unset, the route `Agents`
//! and the launch hook take, never over a plan's or an entry's variables, and no password in a
//! log line or a `Debug`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use clauseo_protocol::AgentId;
use clauseo_sandbox::Upstream;
use clauseo_settings::{Effective, FilePolicy, Locations, PolicySource, SystemFiles};
use url::Url;

use super::{NetworkFacts, OS_FALLBACK, ProxyEnv, ProxySource, resolve, shown};
use crate::bridge::lock;
use crate::test_support::Scratch;

const FIRM: &str = "http://proxy.firm.example:3128";
const ENV: &str = "http://env-proxy.example:8080";
const SECRET: &str = "hunter2-secret";

fn claude() -> AgentId {
    AgentId::new("claude-acp").unwrap()
}

fn codex() -> AgentId {
    AgentId::new("codex-acp").unwrap()
}

/// The facts the embedded snapshot gives both agents, with no setting.
fn facts() -> NetworkFacts {
    let scratch = Scratch::folder("network-facts", &[]);
    let effective = Effective::load(&Locations::under_root(scratch.path()), &SystemFiles, &[]);
    NetworkFacts::read(&effective, [&claude(), &codex()])
}

fn env(https: Option<&str>) -> ProxyEnv {
    ProxyEnv {
        https: https.map(str::to_owned),
        ..ProxyEnv::default()
    }
}

/// An OS that answers a proxy per target host, recording each target it is asked about.
fn os_per_host(asked: &Arc<Mutex<Vec<String>>>) -> impl Fn(&Url) -> Option<String> + use<> {
    let asked = Arc::clone(asked);
    move |target: &Url| {
        let host = target.host_str().unwrap_or_default().to_owned();
        lock(&asked).push(host.clone());
        (host != "auth.openai.com").then(|| format!("http://os-{host}:8080"))
    }
}

fn no_os(_target: &Url) -> Option<String> {
    panic!("the OS is not asked")
}

#[test]
fn the_snapshot_and_vendor_hosts_are_the_targets() {
    let facts = facts();
    assert_eq!(facts.signin_hosts[&claude()], "claude.ai");
    assert_eq!(facts.signin_hosts[&codex()], "auth.openai.com");
    assert_eq!(facts.api[&claude()], "https://api.anthropic.com/");
    assert_eq!(facts.api[&codex()], "https://api.openai.com/");
}

#[test]
fn the_setting_beats_the_environment_which_beats_the_os() {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let os = os_per_host(&asked);
    let mut set = facts();
    set.upstream = Some(FIRM.to_owned());

    let route = resolve(&set, &env(Some(ENV)), true, &os);
    assert_eq!(route.source, ProxySource::Setting);
    assert_eq!(route.install.as_deref(), Some(FIRM));
    for agent in [claude(), codex()] {
        assert_eq!(route.agents[&agent].signin.as_deref(), Some(FIRM));
        assert_eq!(route.agents[&agent].launch.as_deref(), Some(FIRM));
    }

    let route = resolve(&facts(), &env(Some(ENV)), true, &os);
    assert_eq!(route.source, ProxySource::Env);
    assert_eq!(route.install.as_deref(), Some(ENV));
    assert_eq!(route.agents[&codex()].launch.as_deref(), Some(ENV));
    assert!(
        lock(&asked).is_empty(),
        "the OS is never asked over a setting or the env"
    );

    let route = resolve(&facts(), &env(None), true, &os);
    assert_eq!(route.source, ProxySource::Os);
    assert_eq!(
        route.install.as_deref(),
        Some("http://os-registry.npmjs.org:8080")
    );
    let claude_route = &route.agents[&claude()];
    assert_eq!(
        claude_route.signin.as_deref(),
        Some("http://os-claude.ai:8080")
    );
    assert_eq!(
        claude_route.launch.as_deref(),
        Some("http://os-api.anthropic.com:8080")
    );
    // A PAC `DIRECT` for Codex's sign-in host is no proxy there, whatever its API host gets.
    assert_eq!(route.agents[&codex()].signin, None);
    assert_eq!(
        route.agents[&codex()].launch.as_deref(),
        Some("http://os-api.openai.com:8080")
    );
}

#[test]
fn a_firm_mirror_and_a_plan_endpoint_are_what_the_os_is_asked_about() {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut facts = facts();
    facts.registry = Some("https://npm.firm.example/".to_owned());
    facts
        .api
        .insert(claude(), "https://gateway.firm.example/v1".to_owned());
    let _route = resolve(&facts, &env(None), true, &os_per_host(&asked));
    let asked = lock(&asked).clone();
    assert!(asked.contains(&"npm.firm.example".to_owned()), "{asked:?}");
    assert!(
        asked.contains(&"gateway.firm.example".to_owned()),
        "{asked:?}"
    );
    assert!(
        !asked.contains(&"registry.npmjs.org".to_owned()),
        "{asked:?}"
    );
}

#[test]
fn with_the_os_fallback_off_no_proxy_and_the_os_is_not_asked() {
    let route = resolve(&facts(), &env(None), false, &no_os);
    assert_eq!(route.source, ProxySource::None);
    assert_eq!(route.install, None);
    assert!(
        route
            .agents
            .values()
            .all(|agent| agent.signin.is_none() && agent.launch.is_none())
    );
}

/// Decided for launch: the OS's proxy is asked.
const _OS_FALLBACK_IS_ON: () = assert!(OS_FALLBACK);

#[test]
fn an_os_that_answers_direct_everywhere_is_source_none() {
    let route = resolve(&facts(), &env(None), true, &|_target: &Url| None);
    assert_eq!(route.source, ProxySource::None);
    assert_eq!(route.summary(), "none");
}

#[test]
fn empty_values_count_as_unset() {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut blank = facts();
    blank.upstream = Some(String::new());
    let route = resolve(&blank, &env(Some("  ")), true, &os_per_host(&asked));
    assert_eq!(route.source, ProxySource::Os);
    blank.upstream = Some(" ".to_owned());
    let route = resolve(&blank, &env(Some("")), true, &|_target: &Url| {
        Some(String::new())
    });
    assert_eq!(route.source, ProxySource::None);
    assert_eq!(route.install, None);
}

/// The effective settings under `scratch`: `settings.json` holding `user`, and a policy file
/// holding `policy` when given.
fn effective(scratch: &Scratch, user: &str, policy: Option<&str>) -> Effective {
    let locations = Locations::under_root(scratch.path());
    let rel = locations
        .settings_file()
        .strip_prefix(scratch.path())
        .unwrap();
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
fn a_firm_policy_beats_the_lawyers_setting() {
    let scratch = Scratch::folder("network-policy", &[]);
    let user = r#"{"network": {"upstream_proxy": "http://lawyer.example:3128"}}"#;
    let mine = effective(&scratch, user, None);
    let read = NetworkFacts::read(&mine, [&claude()]);
    let route = resolve(&read, &env(Some(ENV)), true, &no_os);
    assert_eq!(route.source, ProxySource::Setting);
    assert_eq!(route.install.as_deref(), Some("http://lawyer.example:3128"));

    let policy = format!(r#"{{"network": {{"upstream_proxy": "{FIRM}"}}}}"#);
    let firm = effective(&scratch, user, Some(&policy));
    assert!(firm.policy().errors().is_empty(), "{:?}", firm.policy());
    let read = NetworkFacts::read(&firm, [&claude()]);
    let route = resolve(&read, &env(Some(ENV)), true, &no_os);
    assert_eq!(route.source, ProxySource::Policy);
    assert_eq!(route.install.as_deref(), Some(FIRM));
    assert_eq!(route.summary(), format!("policy {FIRM}"));
}

#[test]
fn the_ca_files_come_from_the_settings_and_reach_agents() {
    let scratch = Scratch::folder("network-ca", &[]);
    let ca = scratch.file("firm-ca.pem", b"not checked here");
    let user = serde_json::json!({ "network": { "ca_certs": { "claude-acp": ca } } }).to_string();
    let read = NetworkFacts::read(&effective(&scratch, &user, None), [&claude(), &codex()]);
    let route = resolve(&read, &env(None), false, &no_os);
    let network = route.agents_network();
    assert_eq!(network.ca_certs, BTreeMap::from([(claude(), ca.clone())]));
    assert_eq!(route.ca_cert(&claude()), Some(ca.as_path()));
    assert_eq!(route.ca_cert(&codex()), None);
}

#[test]
fn the_start_route_takes_the_setting_and_never_asks_the_os() {
    let scratch = Scratch::folder("network-start", &[]);
    let user = format!(r#"{{"network": {{"upstream_proxy": "{FIRM}"}}}}"#);
    let effective = effective(&scratch, &user, None);
    let start = crate::setup::SetupStart::read(&effective, &Locations::under_root(scratch.path()));
    let network = start.network_at_start().agents_network();
    assert_eq!(network.proxy.as_deref(), Some(FIRM));
    assert_eq!(
        network.signin_proxy.get(&claude()),
        Some(&Some(FIRM.to_owned()))
    );
}

#[test]
fn the_sandbox_upstream_is_fixed_for_a_setting_and_system_otherwise() {
    let mut set = facts();
    set.upstream = Some(FIRM.to_owned());
    let fixed = resolve(&set, &env(None), true, &no_os).upstream();
    assert!(
        matches!(&fixed, Upstream::Fixed(url) if url.as_str() == "http://proxy.firm.example:3128/"),
        "{fixed:?}"
    );
    set.policy = true;
    assert!(matches!(
        resolve(&set, &env(None), true, &no_os).upstream(),
        Upstream::Fixed(_)
    ));
    for route in [
        resolve(&facts(), &env(Some(ENV)), true, &no_os),
        resolve(&facts(), &env(None), true, &|_target: &Url| {
            Some(ENV.to_owned())
        }),
        resolve(&facts(), &env(None), false, &no_os),
    ] {
        assert!(matches!(route.upstream(), Upstream::System), "{route:?}");
    }
}

fn with_setting(setting: &str) -> NetworkFacts {
    let mut set = facts();
    set.upstream = Some(setting.to_owned());
    set
}

#[test]
fn a_shown_proxy_is_its_scheme_host_and_port_only() {
    assert_eq!(
        shown(&format!(
            "http://ann:{SECRET}@proxy.firm.example:3128/x?y=z"
        )),
        "http://proxy.firm.example:3128"
    );
    assert_eq!(
        shown("proxy.firm.example:8080"),
        "http://proxy.firm.example:8080"
    );
    assert_eq!(shown("http://[::1]:3128"), "http://[::1]:3128");
    assert_eq!(
        shown("https://proxy.firm.example"),
        "https://proxy.firm.example:443"
    );
    assert_eq!(shown("http://"), "unreadable");
    let route = format!(
        "{:?}",
        resolve(
            &with_setting(&format!("http://u:{SECRET}@p:1")),
            &env(None),
            true,
            &no_os
        )
    );
    assert!(!route.contains(SECRET), "{route}");
}

/// The environment's proxy: `HTTPS_PROXY`, `https_proxy`, then `ALL_PROXY`, `all_proxy`;
/// never `HTTP_PROXY` for the agents' HTTPS.
#[test]
fn the_env_reads_all_proxy_after_the_https_ones_and_never_http_proxy() {
    let from = |pairs: &[(&str, &str)]| {
        let vars: BTreeMap<String, String> = pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        ProxyEnv::from_vars(|name| vars.get(name).cloned()).https
    };
    let all = ("ALL_PROXY", "http://all.example:1");
    let https = ("https_proxy", "http://https.example:2");
    let http = ("HTTP_PROXY", "http://http.example:3");
    assert_eq!(
        from(&[all, https, http]).as_deref(),
        Some("http://https.example:2")
    );
    assert_eq!(from(&[all, http]).as_deref(), Some("http://all.example:1"));
    assert_eq!(
        from(&[("all_proxy", "http://a.example:4")]).as_deref(),
        Some("http://a.example:4")
    );
    assert_eq!(from(&[http]), None);
    assert_eq!(from(&[("ALL_PROXY", " ")]), None);
}

mod launch;
