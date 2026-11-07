//! The agents' network route: the resolver's order (setting, environment, OS, and the OS
//! fallback off), policy over the lawyer's setting, empty values as unset, the route `Agents`
//! and the launch hook take, never over a plan's or an entry's variables, and no password in a
//! log line or a `Debug`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use clauseo_host::ProfileHook as _;
use clauseo_protocol::AgentId;
use clauseo_sandbox::Upstream;
use clauseo_settings::{Effective, FilePolicy, Locations, Plan, PolicySource, SystemFiles};
use url::Url;

use super::{NetworkFacts, NetworkSlot, OS_FALLBACK, ProxyEnv, ProxySource, resolve, shown};
use crate::bridge::lock;
use crate::setup::SetupHook;
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
        no_proxy: None,
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

/// A slot holding the firm's proxy for every agent and a CA file for Claude.
fn proxied(no_proxy: Option<&str>) -> (NetworkSlot, PathBuf) {
    let ca = PathBuf::from("/firm/ca.pem");
    let mut set = facts();
    set.upstream = Some(FIRM.to_owned());
    set.ca_certs.insert(claude(), ca.clone());
    let env = ProxyEnv {
        https: None,
        no_proxy: no_proxy.map(str::to_owned),
    };
    let route = resolve(&set, &env, true, &no_os);
    (Arc::new(RwLock::new(route)), ca)
}

fn hook_env(hook: &SetupHook, agent: &AgentId) -> BTreeMap<String, String> {
    hook.launch_env(agent)
        .into_iter()
        .map(|(name, value)| (name.into_string().unwrap(), value.into_string().unwrap()))
        .collect()
}

#[test]
fn a_launch_gets_the_proxy_no_proxy_and_its_ca_variable() {
    let (network, ca) = proxied(Some("*.firm.local, localhost"));
    let hook = SetupHook::new(Arc::new(OnceLock::new()), Arc::default(), network);
    let claude_env = hook_env(&hook, &claude());
    assert_eq!(claude_env["HTTPS_PROXY"], FIRM);
    assert_eq!(claude_env["HTTP_PROXY"], FIRM);
    assert_eq!(
        claude_env["NO_PROXY"],
        "localhost,127.0.0.1,::1,*.firm.local"
    );
    assert_eq!(claude_env["NODE_EXTRA_CA_CERTS"], ca.to_string_lossy());
    if cfg!(not(windows)) {
        assert_eq!(claude_env["https_proxy"], FIRM);
        assert_eq!(claude_env["http_proxy"], FIRM);
        assert_eq!(claude_env["no_proxy"], claude_env["NO_PROXY"]);
    }
    let codex_env = hook_env(&hook, &codex());
    assert_eq!(codex_env["HTTPS_PROXY"], FIRM);
    assert!(
        !codex_env.contains_key("CODEX_CA_CERTIFICATE"),
        "Codex has no CA file"
    );
}

#[test]
fn codex_takes_its_ca_file_as_codex_ca_certificate_never_ssl_cert_file() {
    let mut set = facts();
    set.ca_certs
        .insert(codex(), PathBuf::from("/firm/codex.pem"));
    let route = resolve(&set, &env(None), false, &no_os);
    let hook = SetupHook::new(
        Arc::new(OnceLock::new()),
        Arc::default(),
        Arc::new(RwLock::new(route)),
    );
    let codex_env = hook_env(&hook, &codex());
    assert_eq!(
        codex_env.get("CODEX_CA_CERTIFICATE").map(String::as_str),
        Some("/firm/codex.pem")
    );
    assert!(
        !codex_env.contains_key("HTTPS_PROXY"),
        "no proxy, no proxy variables"
    );
    assert!(!codex_env.contains_key("NO_PROXY"));
}

#[test]
fn a_plans_proxy_or_ca_variable_is_never_overridden() {
    let (network, _ca) = proxied(None);
    let plan = Plan {
        env: [
            (
                "https_proxy".to_owned(),
                "http://plan-proxy.example:1".to_owned(),
            ),
            ("NODE_EXTRA_CA_CERTS".to_owned(), "/plan/ca.pem".to_owned()),
        ]
        .into(),
        ..Plan::default()
    };
    let plans = Arc::new(RwLock::new(BTreeMap::from([(claude(), plan)])));
    let hook = SetupHook::new(Arc::new(OnceLock::new()), plans, network);
    let claude_env = hook_env(&hook, &claude());
    assert_eq!(claude_env["https_proxy"], "http://plan-proxy.example:1");
    assert_eq!(claude_env["NODE_EXTRA_CA_CERTS"], "/plan/ca.pem");
    assert!(
        !claude_env.contains_key("HTTPS_PROXY"),
        "HTTPS_PROXY beside the plan's https_proxy"
    );
    // Each variable on its own: the ones the plan leaves free still come.
    assert_eq!(claude_env["HTTP_PROXY"], FIRM);
    assert!(claude_env["NO_PROXY"].starts_with("localhost,127.0.0.1,::1"));
}

#[test]
fn an_agents_own_entry_variables_are_never_overridden() {
    let (network, _ca) = proxied(None);
    {
        let mut route = network.write().unwrap();
        route.own_vars.insert(
            claude(),
            ["no_proxy".to_owned(), "NODE_EXTRA_CA_CERTS".to_owned()].into(),
        );
    }
    let hook = SetupHook::new(Arc::new(OnceLock::new()), Arc::default(), network);
    let claude_env = hook_env(&hook, &claude());
    for name in ["NO_PROXY", "no_proxy", "NODE_EXTRA_CA_CERTS"] {
        assert!(!claude_env.contains_key(name), "{name} over the entry's");
    }
}

/// The entry's own variables, for Claude, with the firm's proxy routed.
fn entry_env(own: &[&str]) -> BTreeMap<String, String> {
    let (network, _ca) = proxied(None);
    network.write().unwrap().own_vars.insert(
        claude(),
        own.iter().map(|name| (*name).to_owned()).collect(),
    );
    let hook = SetupHook::new(Arc::new(OnceLock::new()), Arc::default(), network);
    hook_env(&hook, &claude())
}

/// An entry that sets only `NO_PROXY` keeps its own and still gets the firm's proxy (review M3).
#[test]
fn an_entry_with_only_no_proxy_keeps_it_and_still_gets_the_proxy() {
    let env = entry_env(&["NO_PROXY"]);
    assert_eq!(env["HTTPS_PROXY"], FIRM);
    assert_eq!(env["HTTP_PROXY"], FIRM);
    if cfg!(not(windows)) {
        assert_eq!(env["https_proxy"], FIRM);
    }
    assert!(!env.contains_key("NO_PROXY") && !env.contains_key("no_proxy"));
}

/// An entry's own `HTTPS_PROXY` stays (in neither case added); `HTTP_PROXY` and `NO_PROXY`,
/// which it leaves free, come. An entry's `ALL_PROXY` keeps both proxies out.
#[test]
fn an_entry_with_its_own_https_proxy_keeps_it() {
    let env = entry_env(&["HTTPS_PROXY"]);
    assert!(!env.contains_key("HTTPS_PROXY") && !env.contains_key("https_proxy"));
    assert_eq!(env["HTTP_PROXY"], FIRM);
    assert!(env.contains_key("NO_PROXY"));
    let env = entry_env(&["all_proxy"]);
    assert!(!env.contains_key("HTTPS_PROXY") && !env.contains_key("HTTP_PROXY"));
    assert!(env.contains_key("NO_PROXY"));
}

#[test]
fn the_limits_readers_take_the_same_network_variables() {
    let (network, _ca) = proxied(None);
    let keys = Arc::new(OnceLock::new());
    let plans: crate::setup::hook::Plans = Arc::default();
    let hook = SetupHook::new(Arc::clone(&keys), Arc::clone(&plans), Arc::clone(&network));
    let read: BTreeMap<String, String> = crate::setup::readers::read_vars(
        &keys,
        &plans,
        &network,
        clauseo_protocol::AgentKind::Claude,
    )
    .into_iter()
    .map(|(name, value)| (name, String::clone(&value)))
    .collect();
    assert_eq!(read, hook_env(&hook, &claude()));
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

/// A CA path that is not UTF-8 reaches the launch env byte for byte.
#[cfg(unix)]
#[test]
fn a_ca_path_that_is_not_utf8_reaches_the_launch_as_it_is() {
    use std::os::unix::ffi::OsStrExt as _;
    let ca = PathBuf::from(std::ffi::OsStr::from_bytes(b"/firm/ca-\xff.pem"));
    let mut set = facts();
    set.ca_certs.insert(claude(), ca.clone());
    let route = resolve(&set, &env(None), false, &no_os);
    let hook = SetupHook::new(
        Arc::new(OnceLock::new()),
        Arc::default(),
        Arc::new(RwLock::new(route)),
    );
    let env = hook.launch_env(&claude());
    assert_eq!(
        env.get(std::ffi::OsStr::new("NODE_EXTRA_CA_CERTS")),
        Some(&ca.into_os_string())
    );
}

/// Every launch trusts the OS's store too (`NODE_USE_SYSTEM_CA=1`), proxy or not, unless the
/// plan or the entry sets it; and the hook never names `SSL_CERT_FILE`, in any case, so one the
/// launcher sets (the sandbox's whole System root set: the host's forced variables come after
/// the hook's, `clauseo_host`'s `launch_spec_drops_inherited_git_and_forced_wins`) or the agent
/// inherits survives beside a firm CA file.
#[test]
fn launches_use_the_system_ca_and_never_touch_a_launchers_ssl_cert_file() {
    let mut set = facts();
    set.ca_certs
        .insert(codex(), PathBuf::from("/firm/codex.pem"));
    let route = resolve(&set, &env(None), false, &no_os);
    let network = Arc::new(RwLock::new(route));
    let hook = SetupHook::new(
        Arc::new(OnceLock::new()),
        Arc::default(),
        Arc::clone(&network),
    );
    for agent in [claude(), codex()] {
        let env = hook_env(&hook, &agent);
        assert_eq!(env["NODE_USE_SYSTEM_CA"], "1", "{agent}");
        assert!(
            !env.keys()
                .any(|name| name.eq_ignore_ascii_case("SSL_CERT_FILE")),
            "{agent}: {env:?}"
        );
    }
    network
        .write()
        .unwrap()
        .own_vars
        .insert(claude(), ["node_use_system_ca".to_owned()].into());
    let env = hook_env(&hook, &claude());
    assert!(!env.contains_key("NODE_USE_SYSTEM_CA"), "over the entry's");
}
