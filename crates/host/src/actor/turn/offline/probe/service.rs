//! What the host's probe asks for an agent, recorded once at its launch (`agents/launch.rs`)
//! from the launch's final environment, the one the agent itself gets (`AgentSpec::env`), so the
//! probe stands where the agent stands and looks nothing up when it runs:
//! - **The service:** the base URL the environment sets (`ANTHROPIC_BASE_URL` for Claude, a firm
//!   plan's endpoint included; `OPENAI_BASE_URL` for Codex); else Codex's `api.openai.com` when
//!   its launch passes a key (`CODEX_API_KEY`, the variable `key_retry.rs` reads),
//!   `chatgpt.com` for Clauseo's own login (the sign-in check's `Own`, as the key retry reads
//!   it), and both for any other (the lawyer's reused login, whose kind the host never reads:
//!   either answering counts); Claude's `api.anthropic.com`. A Claude
//!   on a cloud provider's login (`profile::CLOUD_LOGIN_VARS`: Bedrock, Vertex, Foundry) or a
//!   base URL the host cannot map to one `http`/`https` host is [`ReachClass::UnknownService`].
//! - **The route:** the proxy the environment hands the agent, with its `NO_PROXY`, through
//!   the sandbox's [`launch_route`]: desktop's launch puts the route it resolved there (a firm's
//!   `network.upstream_proxy`, else the environment's, else the OS's, `setup/network.rs`), and
//!   the variables a plan, the agent's entry or the inherited environment set stay as the agent
//!   reads them. Each agent reads its own names: Claude Code `https_proxy`, `HTTPS_PROXY`,
//!   `http_proxy`, `HTTP_PROXY` (the first set) and both `no_proxy` lists; Codex `HTTPS_PROXY`,
//!   `https_proxy`, `ALL_PROXY`, `all_proxy` and `NO_PROXY` else `no_proxy`. A proxy a `CONNECT`
//!   cannot follow (`https://`, `socks5://`) is [`ReachClass::UnknownRoute`].
//!
//! Either unknown means no probe: it counts as reached, so a watch waits and the quiet backstop
//! ends a silent turn at 5 min.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;

use clauseo_protocol::AgentKind;
use clauseo_sandbox::proxy::{Route, launch_route};
use url::Url;

use super::ReachClass;
use crate::agent::loads::profile::Login;

/// The service Codex uses with its own sign-in (`chatgpt.com`).
const CHATGPT: &str = "https://chatgpt.com";

/// The service Codex uses with an API key.
const OPENAI_API: &str = "https://api.openai.com";

/// Claude's service.
const ANTHROPIC_API: &str = "https://api.anthropic.com";

/// The proxy variables Claude Code reads, the first set winning (its `https_proxy` reading).
const CLAUDE_PROXY: [&str; 4] = ["https_proxy", "HTTPS_PROXY", "http_proxy", "HTTP_PROXY"];

/// The proxy variables Codex reads for its HTTPS, the first set winning.
const CODEX_PROXY: [&str; 4] = ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"];

/// A launch's environment, as the agent gets it.
pub(crate) type LaunchEnv = BTreeMap<OsString, OsString>;

/// A service the probe tries, with the route the agent's launch gives it.
pub(crate) type Service = (Url, Route);

/// What the probe asks for one agent launch. Its `Debug` names no host, URL or proxy.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Target {
    /// The agent's service (or, for a login of unknown kind, each it may use: either answering
    /// counts), never empty.
    Service(Vec<Service>),
    /// No probe: [`ReachClass::UnknownService`] or [`ReachClass::UnknownRoute`].
    Unknown(ReachClass),
}

impl fmt::Debug for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Service(services) => {
                let via = services.iter().map(|(_, route)| match route {
                    Route::Direct => "direct",
                    Route::Http { .. } => "proxy",
                });
                f.debug_tuple("Service")
                    .field(&via.collect::<Vec<_>>())
                    .finish()
            }
            Self::Unknown(class) => write!(f, "Unknown({})", class.as_str()),
        }
    }
}

impl Target {
    /// The target of an agent of `kind` launched with `env`; `own_login`: a Codex home's login
    /// is Clauseo's own (`key_retry::own_login`).
    pub(crate) fn at_launch(kind: AgentKind, env: &LaunchEnv, own_login: bool) -> Self {
        let urls = services(kind, env, own_login);
        if urls.is_empty() {
            return Self::Unknown(ReachClass::UnknownService);
        }
        let (proxy, no_proxy) = match kind {
            AgentKind::Claude => {
                let lists = [var(env, "no_proxy"), var(env, "NO_PROXY")];
                let lists: Vec<String> = lists.into_iter().flatten().collect();
                (first(env, &CLAUDE_PROXY), Some(lists.join(",")))
            }
            AgentKind::Codex => (
                first(env, &CODEX_PROXY),
                first(env, &["NO_PROXY", "no_proxy"]),
            ),
        };
        let routed: Result<Vec<Service>, _> = urls
            .into_iter()
            .map(|url| launch_route(proxy.as_deref(), no_proxy.as_deref(), &url).map(|r| (url, r)))
            .collect();
        routed.map_or(Self::Unknown(ReachClass::UnknownRoute), Self::Service)
    }
}

/// The services an agent of `kind` launched with `env` may talk to (more than one only for a
/// Codex login of unknown kind); none when it cannot be known.
fn services(kind: AgentKind, env: &LaunchEnv, own_login: bool) -> Vec<Url> {
    let (var_name, defaults): (&str, &[&str]) = match kind {
        AgentKind::Codex if key_passed(env) => ("OPENAI_BASE_URL", &[OPENAI_API]),
        AgentKind::Codex if own_login => ("OPENAI_BASE_URL", &[CHATGPT]),
        AgentKind::Codex => ("OPENAI_BASE_URL", &[CHATGPT, OPENAI_API]),
        AgentKind::Claude => {
            let login = Login::from_env(|name| var(env, name).map(OsString::from));
            if login != Login::FirstParty {
                return Vec::new();
            }
            ("ANTHROPIC_BASE_URL", &[ANTHROPIC_API])
        }
    };
    let Some(set) = var(env, var_name) else {
        return defaults
            .iter()
            .filter_map(|url| Url::parse(url).ok())
            .collect();
    };
    Url::parse(set.trim())
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
        .into_iter()
        .collect()
}

/// Whether a Codex launch's environment passes a key: `CODEX_API_KEY` set and not empty (the
/// variable the profile hook launches a key in, `key_retry.rs`).
fn key_passed(env: &LaunchEnv) -> bool {
    var(env, "CODEX_API_KEY").is_some()
}

/// The first of `names` that `env` sets.
fn first(env: &LaunchEnv, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| var(env, name))
}

/// `name`'s value in `env` when set and not blank: the exact name, and on Windows, where names
/// have no case, any case.
fn var(env: &LaunchEnv, name: &str) -> Option<String> {
    let same = |key: &OsStr| {
        key == name || (cfg!(windows) && key.to_string_lossy().eq_ignore_ascii_case(name))
    };
    env.iter()
        .find(|(key, _)| same(key))
        .and_then(|(_, value)| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests;
