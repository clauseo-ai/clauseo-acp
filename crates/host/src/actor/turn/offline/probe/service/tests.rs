//! What a launch records for the probe (`service.rs`): the host each login kind uses, a base
//! URL, a cloud provider, and the route the launch's own proxy variables give, `NO_PROXY`
//! included. Pure: nothing here touches the network.

use std::ffi::OsString;

use clauseo_protocol::AgentKind;
use clauseo_sandbox::proxy::Route;

use super::{LaunchEnv, Target};
use crate::actor::turn::offline::probe::ReachClass;

const FIRM: &str = "http://proxy.firm.example:3128";

fn env(vars: &[(&str, &str)]) -> LaunchEnv {
    vars.iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

fn firm() -> Route {
    Route::Http {
        host: "proxy.firm.example".to_owned(),
        port: 3128,
    }
}

/// The hosts and routes of a known target; panics on an unknown one.
fn services(target: &Target) -> Vec<(String, Route)> {
    let Target::Service(services) = target else {
        panic!("not probed: {target:?}");
    };
    services
        .iter()
        .map(|(url, route)| (url.host_str().unwrap().to_owned(), route.clone()))
        .collect()
}

fn hosts(target: &Target) -> Vec<String> {
    services(target).into_iter().map(|(host, _)| host).collect()
}

#[test]
fn claude_asks_anthropic_with_no_base_url_and_no_proxy() {
    let target = Target::at_launch(AgentKind::Claude, &env(&[]), false);
    let want = vec![("api.anthropic.com".to_owned(), Route::Direct)];
    assert_eq!(services(&target), want);
}

#[test]
fn a_plan_endpoint_in_the_launch_env_is_the_host_claude_is_probed_at() {
    let launch = env(&[("ANTHROPIC_BASE_URL", "https://gateway.firm.example/claude")]);
    let target = Target::at_launch(AgentKind::Claude, &launch, false);
    assert_eq!(hosts(&target), ["gateway.firm.example"]);
}

#[test]
fn a_cloud_providers_login_is_never_probed() {
    for var in [
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
    ] {
        let launch = env(&[(var, "1"), ("HTTPS_PROXY", FIRM)]);
        let target = Target::at_launch(AgentKind::Claude, &launch, false);
        assert_eq!(target, Target::Unknown(ReachClass::UnknownService), "{var}");
    }
    // `0` or empty is no provider.
    for value in ["0", ""] {
        let launch = env(&[("CLAUDE_CODE_USE_BEDROCK", value)]);
        let target = Target::at_launch(AgentKind::Claude, &launch, false);
        assert_eq!(hosts(&target), ["api.anthropic.com"], "{value:?}");
    }
}

#[test]
fn a_base_url_naming_no_one_host_is_never_probed() {
    for url in [
        "not a url",
        "ftp://gateway.firm.example",
        "unix:/run/gateway.sock",
    ] {
        let launch = env(&[("ANTHROPIC_BASE_URL", url)]);
        let target = Target::at_launch(AgentKind::Claude, &launch, false);
        assert_eq!(target, Target::Unknown(ReachClass::UnknownService), "{url}");
    }
}

#[test]
fn codex_follows_its_login_its_key_and_its_base_url() {
    let key = env(&[("CODEX_API_KEY", "sk-test")]);
    // A key passed: the API's host alone, whatever the login.
    for own in [true, false] {
        let target = Target::at_launch(AgentKind::Codex, &key, own);
        assert_eq!(hosts(&target), ["api.openai.com"]);
    }
    // Clauseo's own ChatGPT login: chatgpt.com alone.
    let no_key = env(&[("CODEX_API_KEY", "")]);
    let target = Target::at_launch(AgentKind::Codex, &no_key, true);
    assert_eq!(hosts(&target), ["chatgpt.com"]);
    // The lawyer's reused login, of unknown kind: both, either answering counts.
    let target = Target::at_launch(AgentKind::Codex, &no_key, false);
    assert_eq!(hosts(&target), ["chatgpt.com", "api.openai.com"]);
    // A base URL wins over all of them.
    let base = env(&[
        ("CODEX_API_KEY", "sk-test"),
        ("OPENAI_BASE_URL", "https://llm.firm.example/v1"),
    ]);
    let target = Target::at_launch(AgentKind::Codex, &base, false);
    assert_eq!(hosts(&target), ["llm.firm.example"]);
}

#[test]
fn the_proxy_the_launch_hands_the_agent_is_the_route_its_credentials_dropped() {
    // What desktop's launch gives for `network.upstream_proxy` alone (`launch_vars`).
    let launch = env(&[
        ("HTTPS_PROXY", FIRM),
        ("HTTP_PROXY", FIRM),
        ("NO_PROXY", "localhost,127.0.0.1,::1"),
    ]);
    for kind in [AgentKind::Claude, AgentKind::Codex] {
        let target = Target::at_launch(kind, &launch, true);
        assert!(services(&target).iter().all(|(_, route)| *route == firm()));
    }
    let with_login = env(&[("https_proxy", "http://me:secret@proxy.firm.example:3128")]);
    let target = Target::at_launch(AgentKind::Claude, &with_login, false);
    assert_eq!(services(&target)[0].1, firm());
}

#[test]
fn each_agent_reads_its_own_proxy_names() {
    let other = "http://other.firm.example:8080";
    // Claude Code: `https_proxy` before `HTTPS_PROXY`, then `HTTP_PROXY`; never `ALL_PROXY`.
    let both = env(&[("https_proxy", FIRM), ("HTTPS_PROXY", other)]);
    let target = Target::at_launch(AgentKind::Claude, &both, false);
    if cfg!(not(windows)) {
        assert_eq!(services(&target)[0].1, firm());
    }
    let http_only = env(&[("HTTP_PROXY", FIRM)]);
    let target = Target::at_launch(AgentKind::Claude, &http_only, false);
    assert_eq!(services(&target)[0].1, firm());
    let all_only = env(&[("ALL_PROXY", FIRM)]);
    let target = Target::at_launch(AgentKind::Claude, &all_only, false);
    assert_eq!(services(&target)[0].1, Route::Direct);
    // Codex: `HTTPS_PROXY`, then `ALL_PROXY`; never `HTTP_PROXY` for its HTTPS.
    let target = Target::at_launch(AgentKind::Codex, &all_only, true);
    assert_eq!(services(&target)[0].1, firm());
    let target = Target::at_launch(AgentKind::Codex, &http_only, true);
    assert_eq!(services(&target)[0].1, Route::Direct);
}

#[test]
fn the_agents_no_proxy_holding_its_service_sends_the_probe_straight() {
    let launch = env(&[
        ("HTTPS_PROXY", FIRM),
        ("https_proxy", FIRM),
        ("NO_PROXY", "localhost,127.0.0.1,::1,.anthropic.com"),
    ]);
    let target = Target::at_launch(AgentKind::Claude, &launch, false);
    assert_eq!(services(&target)[0].1, Route::Direct);
    // Claude Code reads both lists, the lower-case one too.
    let lower = env(&[("https_proxy", FIRM), ("no_proxy", "api.anthropic.com")]);
    let target = Target::at_launch(AgentKind::Claude, &lower, false);
    assert_eq!(services(&target)[0].1, Route::Direct);
    // Codex's list holding one of its two hosts: that one straight, the other by the proxy.
    let codex = env(&[("HTTPS_PROXY", FIRM), ("NO_PROXY", "chatgpt.com")]);
    let target = Target::at_launch(AgentKind::Codex, &codex, false);
    let routes: Vec<Route> = services(&target).into_iter().map(|(_, r)| r).collect();
    assert_eq!(routes, [Route::Direct, firm()]);
}

#[test]
fn a_proxy_a_connect_cannot_follow_is_never_probed() {
    for proxy in [
        "socks5://proxy.firm.example:1080",
        "https://proxy.firm.example",
    ] {
        let launch = env(&[("HTTPS_PROXY", proxy), ("https_proxy", proxy)]);
        let target = Target::at_launch(AgentKind::Claude, &launch, false);
        assert_eq!(target, Target::Unknown(ReachClass::UnknownRoute), "{proxy}");
    }
}

#[test]
fn unknown_targets_count_as_reached_and_debug_names_no_host() {
    for class in [ReachClass::UnknownService, ReachClass::UnknownRoute] {
        assert!(crate::actor::turn::offline::probe::Reach::of(class).reached());
    }
    let launch = env(&[
        ("ANTHROPIC_BASE_URL", "https://gateway.firm.example"),
        ("HTTPS_PROXY", FIRM),
        ("https_proxy", FIRM),
    ]);
    let shown = format!("{:?}", Target::at_launch(AgentKind::Claude, &launch, false));
    assert_eq!(shown, "Service([\"proxy\"])");
}
