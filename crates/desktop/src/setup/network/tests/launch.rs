//! The launch env the route gives: the proxy, `NO_PROXY`, the CA file and `NODE_USE_SYSTEM_CA`,
//! never over what a plan or the agent's own entry sets.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};

use clauseo_host::ProfileHook as _;
use clauseo_protocol::AgentId;
use clauseo_settings::Plan;

use super::super::{NetworkSlot, ProxyEnv, resolve};
use super::{FIRM, claude, codex, env, facts, no_os};
use crate::setup::SetupHook;

/// A slot holding the firm's proxy for every agent and a CA file for Claude.
fn proxied(no_proxy: Option<&str>) -> (NetworkSlot, PathBuf) {
    let ca = PathBuf::from("/firm/ca.pem");
    let mut set = facts();
    set.upstream = Some(FIRM.to_owned());
    set.ca_certs.insert(claude(), ca.clone());
    let env = ProxyEnv {
        no_proxy: no_proxy.map(str::to_owned),
        ..ProxyEnv::default()
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

/// An entry that sets only `NO_PROXY` still gets the firm's proxy (review M3), and keeps its own
/// `NO_PROXY`, with the loopback names it lacks added (Clauseo's MCP relay is on 127.0.0.1).
#[test]
fn an_entry_with_only_no_proxy_keeps_it_with_loopback_and_still_gets_the_proxy() {
    let (network, _ca) = proxied(None);
    {
        let mut route = network.write().unwrap();
        route
            .own_vars
            .insert(claude(), ["NO_PROXY".to_owned()].into());
        route
            .own_no_proxy
            .insert(claude(), ("NO_PROXY".to_owned(), "corp.example".to_owned()));
    }
    let hook = SetupHook::new(Arc::new(OnceLock::new()), Arc::default(), network);
    let env = hook_env(&hook, &claude());
    assert_eq!(env["HTTPS_PROXY"], FIRM);
    assert_eq!(env["HTTP_PROXY"], FIRM);
    if cfg!(not(windows)) {
        assert_eq!(env["https_proxy"], FIRM);
    }
    assert_eq!(env["NO_PROXY"], "corp.example,localhost,127.0.0.1,::1");
    assert!(!env.contains_key("no_proxy"), "only the entry's own name");
}

/// A plan's `NO_PROXY` stays too, with the loopback names it lacks added; one that holds them
/// is left as it is.
#[test]
fn a_plans_no_proxy_gets_the_loopback_names_it_lacks() {
    let (network, _ca) = proxied(None);
    let plan = |list: &str| Plan {
        env: [("no_proxy".to_owned(), list.to_owned())].into(),
        ..Plan::default()
    };
    let plans = Arc::new(RwLock::new(BTreeMap::from([(
        claude(),
        plan("corp.example, 127.0.0.1"),
    )])));
    let hook = SetupHook::new(Arc::new(OnceLock::new()), Arc::clone(&plans), network);
    let env = hook_env(&hook, &claude());
    assert_eq!(env["no_proxy"], "corp.example, 127.0.0.1,localhost,::1");
    assert!(!env.contains_key("NO_PROXY"));
    assert_eq!(env["HTTPS_PROXY"], FIRM);
    plans
        .write()
        .unwrap()
        .insert(claude(), plan("localhost,127.0.0.1,::1,corp.example"));
    let env = hook_env(&hook, &claude());
    assert_eq!(env["no_proxy"], "localhost,127.0.0.1,::1,corp.example");
}

/// `NODE_USE_SYSTEM_CA` set in this process's environment (here `0`) is the launch's: the hook
/// adds none, so the inherited one survives.
#[test]
fn an_inherited_node_use_system_ca_survives_a_launch() {
    let env = ProxyEnv::from_vars(|name| (name == "NODE_USE_SYSTEM_CA").then(|| "0".to_owned()));
    assert!(env.system_ca);
    let route = resolve(&facts(), &env, false, &no_os);
    let hook = SetupHook::new(
        Arc::new(OnceLock::new()),
        Arc::default(),
        Arc::new(RwLock::new(route)),
    );
    assert!(!hook_env(&hook, &claude()).contains_key("NODE_USE_SYSTEM_CA"));
    assert!(
        !ProxyEnv::from_vars(|_name| Some(String::new())).system_ca,
        "empty is unset"
    );
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
