//! The inherited `CLAUDE_*` and `ANTHROPIC_*` names ([`CLAUDE_PREFIXES`]) that a Claude child
//! Clauseo starts may keep: how a firm routes Claude. One rule for sign-in's sign-in, status and
//! logout runs (`crates/agents/src/signin/env.rs`) and the plan readers
//! (`crates/limits/src/readers/env.rs`), which host's agent launch will import (#1167), so
//! Settings > Plans never says signed in on a credential the agent would not use. Every other
//! name under the prefixes is dropped, and so are [`ALSO_DROPPED`]'s names outside them. A name a
//! run sets itself (sign-in's `ANTHROPIC_API_KEY=""`, the stored key, a firm plan's endpoint, the
//! settings entry's `ANTHROPIC_API_KEY: ""`) still wins over the inherited one.
//!
//! Why a Claude-specific list lives in the spawn crate: it is one list that host's launch,
//! sign-in's runs (`agents`) and the plan readers (`limits`) must all share, and `process` is
//! the one crate all three depend on (`host` never depends on `agents`, ARCHITECTURE.md).
//!
//! This is meant to be the single list for every Claude child Clauseo starts: host's launch scrub
//! (#1167) will import it and drop its own copy. Its 65 names were reconciled with host's table at
//! 4ea6afd34 (`lw-review-fixes`).
//!
//! The rule. The 13 [`ROUTES`] names (7 endpoints, 6 provider switches) pass on their own,
//! whatever their value. The rest of [`ROUTING_KEPT`] (credentials, headers, regions, resources,
//! model ids, mutual TLS, skipped cloud auth) and [`COMPANIONS_OUTSIDE_PREFIXES`] are companions:
//! they pass only when the inherited environment holds a route ([`route_present`], worked out
//! once per run). A route is what makes Claude Code send its traffic somewhere other than
//! Anthropic's own API, as Claude Code itself decides it:
//! - `ANTHROPIC_BASE_URL` with a value that is not blank once trimmed of JavaScript whitespace
//!   ([`crate::js_trim`]): Claude Code trims it and reads a blank one as unset;
//! - or a provider switch that is on by Claude Code's truth test (lower-cased, trimmed, then `1`,
//!   `true`, `yes` or `on`).
//!
//! The six provider addresses (`ANTHROPIC_BEDROCK_BASE_URL` and the rest) are never a route on
//! their own: Claude Code picks its provider from the switches alone and reads a provider's
//! address only inside that provider's client, so with its switch off it goes to Anthropic's own
//! API. A lone `ANTHROPIC_AUTH_TOKEN` or model id with no route swaps the account or the model,
//! not the route, and drops. A name that only swaps the account, or points Claude at something a
//! local program can serve, is not a route and stays dropped: Clauseo has its own sign-in, shown
//! on Settings > Plans, an inherited account no screen shows would contradict it, and any program
//! can set one. [`ALWAYS_KEPT`]'s names pass route or not, because each can only reduce what
//! leaves the computer.
//!
//! Names and the OS. Claude Code reads exact upper-case names, and on macOS and Linux its
//! environment is case-sensitive, so a listed name matches only as spelled there: a lower-case
//! `anthropic_base_url` is no route, and it drops (the prefix test ignores ASCII case there, so a
//! spelling Claude Code would not read never reaches it either). On Windows the environment
//! ignores case, so listed names match in any ASCII case, and spellings of one name that differ
//! only in case are folded first ([`kept_inherited`]): if they all hold one value one spelling
//! is kept, and if they differ none is kept and none counts as a route. Rust's Windows `Command`
//! would otherwise pass one variable with the first spelling it was given and the last value, so
//! which value the child got would depend on the order of the map. The Windows prefix test
//! upper-cases by Unicode (`ı`, U+0131, becomes `I`), so more names fall under the rule and drop.
//!
//! Exact names only, never a prefix or pattern: a name Claude Code adds later stays dropped
//! until someone reviews it and adds it here. Each comes from the pinned CLI, Claude Code
//! 2.1.293 (`@anthropic-ai/claude-agent-sdk-linux-x64/claude`, decimal byte offsets from
//! `grep -aob`), read statically; re-check at each bump. The CLI groups them the same way: its
//! table of endpoints, each with its provider switch and companions, @205038092 (`O4e`).
//!
//! Left out on purpose, so they stay dropped:
//! - `ANTHROPIC_UNIX_SOCKET` (@204890948): the API over a local socket, which any program on the
//!   computer can serve.
//! - `CLAUDE_CODE_API_BASE_URL` (@211588126): the Files API's address when `ANTHROPIC_BASE_URL`
//!   is unset, so it never routes a firm's endpoint, and alone it points file uploads elsewhere.
//! - `CLAUDE_CODE_OAUTH_TOKEN`, `CLAUDE_CODE_OAUTH_REFRESH_TOKEN`,
//!   `CLAUDE_CODE_SESSION_ACCESS_TOKEN` and the OAuth client's names: they swap the account.
//! - Workload identity federation (`ANTHROPIC_IDENTITY_TOKEN`, `ANTHROPIC_IDENTITY_TOKEN_FILE`,
//!   `ANTHROPIC_FEDERATION_RULE_ID`, `ANTHROPIC_ORGANIZATION_ID`, `ANTHROPIC_WORKSPACE_ID`,
//!   `ANTHROPIC_SERVICE_ACCOUNT_ID`, `ANTHROPIC_SCOPE`, `ANTHROPIC_PROFILE`,
//!   `ANTHROPIC_CONFIG_DIR`; @210486664, @206604923): an account, not a route. A stated limit: a
//!   firm that signs in this way is not supported until Plans supports firm accounts (0030).
//! - `CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR`, `CLAUDE_CODE_API_KEY_FILE_DESCRIPTOR`,
//!   `CLAUDE_CODE_GATEWAY_TOKEN_FILE_DESCRIPTOR`: a descriptor number, and the agent gets only
//!   its three standard streams from Clauseo, so the descriptor never reaches it.
//! - `CLAUDE_CODE_HOST_AUTH_ENV_VAR`, `CLAUDE_CODE_HOST_CREDS_FILE`,
//!   `CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST`: a host app's own wiring (@207485763, @205043690);
//!   Clauseo is the host and sets none.
//! - `CLAUDE_CODE_CUSTOM_OAUTH_URL`: the sign-in's OAuth address, not traffic's route (stage D
//!   relies on it being dropped).
//! - `CLAUDE_CODE_HTTP_PROXY`, `CLAUDE_CODE_HTTPS_PROXY`: fed only to the proxy settings Claude
//!   Code builds for child processes (@206611630); `CLAUDE_CODE_PROXY_URL`: written, not read.
//! - `CLAUDE_CODE_3P_PROBE_WROTE_*_DEFAULT`, `CLAUDE_CODE_3P_SEEDED_*_DEFAULT`: on the CLI's
//!   model list (@205039601) but its own bookkeeping, not a firm's choice.
//! - Feature, timeout, retry and telemetry names (`ANTHROPIC_BETAS`,
//!   `CLAUDE_CODE_API_KEY_HELPER_TTL_MS`, …).
//!
//! Outside both prefixes, so never scrubbed: `CLOUD_ML_REGION`, `AWS_REGION`,
//! `AWS_BEARER_TOKEN_BEDROCK`, `AWS_ENDPOINT_URL*`, and the firm's proxy and CA (`HTTPS_PROXY`,
//! `NODE_EXTRA_CA_CERTS`, …). `_CLAUDE_CODE_ASSUME_FIRST_PARTY_BASE_URL` starts with `_` but is a
//! companion of `ANTHROPIC_BASE_URL` ([`COMPANIONS_OUTSIDE_PREFIXES`]).

use std::ffi::{OsStr, OsString};

/// The prefixes the rule governs: host's `SCRUBBED_PREFIXES`. A name outside them passes, but for
/// [`ALSO_DROPPED`] and [`COMPANIONS_OUTSIDE_PREFIXES`].
pub const CLAUDE_PREFIXES: [&str; 2] = ["CLAUDE_", "ANTHROPIC_"];

/// The routing names that pass the scrub: the [`ROUTES`] first, then their companions, grouped as
/// the CLI groups them.
///
/// One list for every Claude child Clauseo starts: sign-in's runs
/// (`crates/agents/src/signin/env.rs`), the plan readers (`crates/limits/src/readers/env.rs`) and,
/// once host's launch scrub (#1167) imports it, the launch. There is to be no second list.
pub const ROUTING_KEPT: [&str; 65] = [
    // The endpoints (the CLI's endpoint table @205038092, its switch @211334681). Only the first
    // makes a route; the six provider addresses pass but are read only beside their switch.
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_BEDROCK_BASE_URL",
    "ANTHROPIC_VERTEX_BASE_URL",
    "ANTHROPIC_FOUNDRY_BASE_URL",
    "ANTHROPIC_AWS_BASE_URL",
    "ANTHROPIC_GOOGLE_CLOUD_BASE_URL",
    "ANTHROPIC_BEDROCK_MANTLE_BASE_URL",
    // The provider switches (their map @204797295, read with the truth test @204576068).
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "CLAUDE_CODE_USE_ANTHROPIC_AWS",
    "CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD",
    "CLAUDE_CODE_USE_MANTLE",
    // Companions from here on. The gateway, ignored without `ANTHROPIC_BASE_URL` (@207404260),
    // so not a route of its own; its token and hint headers (@222471285, @211322209).
    "CLAUDE_CODE_USE_GATEWAY",
    "CLAUDE_CODE_GATEWAY_TOKEN",
    "CLAUDE_CODE_GATEWAY_HINT_HEADERS",
    // The endpoint's credentials and headers (the credential list @205039200; @205706766,
    // @205707624; headers in the endpoint table @205038092 and @205706531).
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_FOUNDRY_API_KEY",
    "ANTHROPIC_FOUNDRY_AUTH_TOKEN",
    "ANTHROPIC_AWS_API_KEY",
    "ANTHROPIC_CUSTOM_HEADERS",
    // A provider's own cloud auth skipped (the list @205039384).
    "CLAUDE_CODE_SKIP_BEDROCK_AUTH",
    "CLAUDE_CODE_SKIP_VERTEX_AUTH",
    "CLAUDE_CODE_SKIP_FOUNDRY_AUTH",
    "CLAUDE_CODE_SKIP_ANTHROPIC_AWS_AUTH",
    "CLAUDE_CODE_SKIP_ANTHROPIC_GOOGLE_CLOUD_AUTH",
    "CLAUDE_CODE_SKIP_MANTLE_AUTH",
    // The resource, project, region, workspace or tier (the provider list @205033949;
    // @211320120, @211330348, @225014021, @206793036, the routing names @204647027).
    "ANTHROPIC_FOUNDRY_RESOURCE",
    "ANTHROPIC_VERTEX_PROJECT_ID",
    "ANTHROPIC_AWS_WORKSPACE_ID",
    "ANTHROPIC_GOOGLE_CLOUD_PROJECT",
    "ANTHROPIC_GOOGLE_CLOUD_LOCATION",
    "ANTHROPIC_GOOGLE_CLOUD_WORKSPACE_ID",
    "ANTHROPIC_BEDROCK_REGION_PREFIX",
    "ANTHROPIC_BEDROCK_SERVICE_TIER",
    // Model ids, which on Bedrock and Vertex are ARNs or project/location paths naming the
    // account and region (the ARN pattern @102292824), with the names and capabilities a firm
    // gives its own: the CLI's model list @205039601 and custom-option list @205040626.
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL_DESCRIPTION",
    "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
    "ANTHROPIC_DEFAULT_FABLE_MODEL_SUPPORTED_CAPABILITIES",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL_DESCRIPTION",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL_SUPPORTED_CAPABILITIES",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL_DESCRIPTION",
    "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
    "ANTHROPIC_DEFAULT_OPUS_MODEL_SUPPORTED_CAPABILITIES",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL_DESCRIPTION",
    "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
    "ANTHROPIC_DEFAULT_SONNET_MODEL_SUPPORTED_CAPABILITIES",
    "ANTHROPIC_SMALL_FAST_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL_AWS_REGION",
    "CLAUDE_CODE_SUBAGENT_MODEL",
    "ANTHROPIC_CUSTOM_MODEL_OPTION",
    "ANTHROPIC_CUSTOM_MODEL_OPTION_DESCRIPTION",
    "ANTHROPIC_CUSTOM_MODEL_OPTION_NAME",
    "ANTHROPIC_CUSTOM_MODEL_OPTION_SUPPORTED_CAPABILITIES",
    // A gateway's mutual TLS and certificate store (@204880367, @204879295, @204876204).
    "CLAUDE_CODE_CLIENT_CERT",
    "CLAUDE_CODE_CLIENT_KEY",
    "CLAUDE_CODE_CLIENT_KEY_PASSPHRASE",
    "CLAUDE_CODE_CERT_STORE",
];

/// The names kept whether or not a route is present, each because it can only reduce what leaves
/// the computer: a firm that sets one machine-wide keeps it. Exact names only, each with its
/// reason; nothing joins by pattern.
pub const ALWAYS_KEPT: [&str; 1] = [
    // Any value turns off Claude Code's nonessential traffic (its traffic mode @204573922, `dIt`).
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
];

/// Companions outside [`CLAUDE_PREFIXES`]: passed only beside a route, like the companions in
/// [`ROUTING_KEPT`]. `_CLAUDE_CODE_ASSUME_FIRST_PARTY_BASE_URL` makes Claude Code treat its
/// `ANTHROPIC_BASE_URL` as Anthropic's own API (the endpoint table pairs it with that address), so
/// with no route it has nothing to act on.
pub const COMPANIONS_OUTSIDE_PREFIXES: [&str; 1] = ["_CLAUDE_CODE_ASSUME_FIRST_PARTY_BASE_URL"];

/// Names that sign Claude Code in to an account, or reach one, that the rule always drops, route
/// or not (host's "left out on purpose"). The rule needs no list for them, since it keeps only
/// [`ROUTING_KEPT`]; they are named so the tests of every run can check each one. Offsets: the
/// OAuth token is read @207457297, the refresh token by `auth login` itself @229847062, the
/// session token @206642972; the federation sign-in is built from the workload identity names
/// @210486379 (switched on by `_t` @206603569).
pub const CLAUDE_ACCOUNT_ONLY: [&str; 26] = [
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_REFRESH_TOKEN",
    "CLAUDE_CODE_OAUTH_CLIENT_ID",
    "CLAUDE_CODE_OAUTH_SCOPES",
    "CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR",
    "CLAUDE_CODE_SESSION_ACCESS_TOKEN",
    "CLAUDE_CODE_API_KEY_FILE_DESCRIPTOR",
    "CLAUDE_CODE_GATEWAY_TOKEN_FILE_DESCRIPTOR",
    "CLAUDE_CODE_WEBSOCKET_AUTH_FILE_DESCRIPTOR",
    "CLAUDE_CODE_CUSTOM_OAUTH_URL",
    "CLAUDE_CODE_API_BASE_URL",
    "ANTHROPIC_UNIX_SOCKET",
    "CLAUDE_CODE_HOST_CREDS_FILE",
    "CLAUDE_CODE_HOST_AUTH_ENV_VAR",
    "CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST",
    "CLAUDE_BRIDGE_OAUTH_TOKEN",
    "ANTHROPIC_IDENTITY_TOKEN",
    "ANTHROPIC_IDENTITY_TOKEN_FILE",
    "ANTHROPIC_FEDERATION_RULE_ID",
    "ANTHROPIC_ORGANIZATION_ID",
    "ANTHROPIC_WORKSPACE_ID",
    "ANTHROPIC_SERVICE_ACCOUNT_ID",
    "ANTHROPIC_SCOPE",
    "ANTHROPIC_PROFILE",
    "ANTHROPIC_CONFIG_DIR",
    "CLAUDE_CODE_FEDERATION_CACHE_DIR",
];

/// How many of [`ROUTING_KEPT`]'s names, from the start, pass on their own: [`ENDPOINT_NAMES`]
/// endpoints, then six provider switches.
const ROUTE_NAMES: usize = 13;

/// How many of [`ROUTES`], from the start, are endpoints.
const ENDPOINT_NAMES: usize = 7;

/// The one endpoint that is a route on its own (when not blank): the others are a provider's.
const BASE_URL: &str = "ANTHROPIC_BASE_URL";

/// [`ROUTING_KEPT`]'s endpoints and provider switches, each passing on its own whatever its value.
/// Only `ANTHROPIC_BASE_URL` with a value and a switch that is on make a route ([`route_present`]).
pub const ROUTES: &[&str] = ROUTING_KEPT.split_at(ROUTE_NAMES).0;

/// The values a provider switch is on with, as Claude Code reads one (its truth test
/// @204302936: lower-cased, trimmed of JavaScript whitespace).
const ON: [&str; 4] = ["1", "true", "yes", "on"];

/// Whether `inherited` holds a route: `ANTHROPIC_BASE_URL` not blank once trimmed of JavaScript
/// whitespace, or a provider switch that is on. Names match as the OS reads them (exactly on
/// macOS and Linux, in any ASCII case on Windows, after folding: see the module doc). A provider
/// address alone, a switch set to `0` or a blank `ANTHROPIC_BASE_URL` routes nothing, so the
/// companions drop.
pub fn route_present<'a>(inherited: impl IntoIterator<Item = (&'a OsStr, &'a OsStr)>) -> bool {
    let all: Vec<(&OsStr, &OsStr)> = inherited.into_iter().collect();
    routed(&all, &folded(&all))
}

/// Whether the inherited `name` passes the scrub: it is one of [`ROUTES`] or [`ALWAYS_KEPT`],
/// or a companion and `route` ([`route_present`] of the whole inherited environment) is true.
/// Exact names on macOS and Linux, any ASCII case on Windows.
pub fn kept(name: &OsStr, route: bool) -> bool {
    let (routes, companions) = ROUTING_KEPT.split_at(ROUTE_NAMES);
    listed(name, routes)
        || listed(name, &ALWAYS_KEPT)
        || (route && (listed(name, companions) || listed(name, &COMPANIONS_OUTSIDE_PREFIXES)))
}

/// The inherited variables outside [`CLAUDE_PREFIXES`] that every Claude child drops too (host's
/// `SCRUBBED`, exact names, ignoring ASCII case), because they loosen Claude Code 2.1.293
/// (decimal byte offsets in its binary, `grep -aob`): `IS_SANDBOX=1` lets it run as root in
/// bypass and cancels a policy's `disableNoSandbox`; `NODE_TLS_REJECT_UNAUTHORIZED=0` accepts any
/// certificate (on Claude Code's own list of sensitive names, `Sl`, @205331198);
/// `SESSION_INGRESS_URL` is read ahead of `ANTHROPIC_BASE_URL` (@93264844); `AGENT_PROXY_URL`
/// and `AGENT_PROXY_AUTH_TOKEN` send its traffic through another proxy (@94103596, @94103620);
/// and the self-hosted runner's `SELF_HOSTED_RUNNER_TRUST_WORKSPACE` (@102682348),
/// `SELF_HOSTED_RUNNER_HOOKS_DIR` (@93241659) and `SELF_HOSTED_RUNNER_CONFINE_REPO_SETTINGS`
/// (@102681396) trust the folder, add hooks, or change which settings it reads. Sign-in's runs
/// and the plan readers drop them as the launch does: a sign-in that works where the launch
/// cannot is worse than both failing. The firm's proxy and CA (`HTTPS_PROXY`, `HTTP_PROXY`,
/// `ALL_PROXY`, `NO_PROXY`, `NODE_EXTRA_CA_CERTS`, `SSL_CERT_FILE`) stay.
pub const ALSO_DROPPED: [&str; 8] = [
    "IS_SANDBOX",
    "NODE_TLS_REJECT_UNAUTHORIZED",
    "SESSION_INGRESS_URL",
    "AGENT_PROXY_URL",
    "AGENT_PROXY_AUTH_TOKEN",
    "SELF_HOSTED_RUNNER_TRUST_WORKSPACE",
    "SELF_HOSTED_RUNNER_HOOKS_DIR",
    "SELF_HOSTED_RUNNER_CONFINE_REPO_SETTINGS",
];

/// Whether `name` starts with one of [`CLAUDE_PREFIXES`]: ignoring ASCII case on macOS and Linux,
/// after Unicode upper-casing on Windows (so `anthropıc_x`, with U+0131, falls under it there).
pub fn under_prefixes(name: &OsStr) -> bool {
    let upper = upper(name);
    CLAUDE_PREFIXES
        .iter()
        .any(|prefix| upper.starts_with(prefix))
}

/// `inherited` with the rule applied, in order. On Windows, spellings of one governed name that
/// differ only in case are folded first: one kept (the first) when they hold one value, none
/// when they differ. Then [`route_present`] once over what is left, and every name outside the
/// rule but [`ALSO_DROPPED`]'s is kept, and inside it every name [`kept`] keeps.
pub fn kept_inherited(
    inherited: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    let all: Vec<(OsString, OsString)> = inherited.into_iter().collect();
    let pairs: Vec<(&OsStr, &OsStr)> = all
        .iter()
        .map(|(name, value)| (name.as_os_str(), value.as_os_str()))
        .collect();
    let folded = folded(&pairs);
    let route = routed(&pairs, &folded);
    let keep: Vec<bool> = pairs
        .iter()
        .zip(&folded)
        .map(|(&(name, _), &left)| {
            left && if governed(name) {
                kept(name, route)
            } else {
                !ALSO_DROPPED
                    .iter()
                    .any(|drop| name.eq_ignore_ascii_case(drop))
            }
        })
        .collect();
    all.into_iter()
        .zip(keep)
        .filter_map(|(pair, keep)| keep.then_some(pair))
        .collect()
}

/// Whether `all`'s pairs left by [`folded`] hold a route.
fn routed(all: &[(&OsStr, &OsStr)], folded: &[bool]) -> bool {
    let switches = &ROUTES[ENDPOINT_NAMES..];
    all.iter().zip(folded).any(|(&(name, value), &left)| {
        let address = listed(name, &[BASE_URL]) && !blank(value);
        let switch = listed(name, switches) && value.to_str().is_some_and(on);
        left && (address || switch)
    })
}

/// Whether Claude Code reads `value` as unset: nothing left once trimmed of JavaScript
/// whitespace. A value that is not UTF-8 is read as the replacement characters it decodes to.
fn blank(value: &OsStr) -> bool {
    crate::js_trim(&value.to_string_lossy()).is_empty()
}

/// Claude Code's truth test: `String(v).toLowerCase().trim()` is one of [`ON`].
fn on(value: &str) -> bool {
    let lower = value.to_lowercase();
    ON.contains(&crate::js_trim(&lower))
}

/// Which of `all`'s pairs are left once case-only spellings are folded: on Windows, of the
/// governed names that are one name in ASCII case, only the first, and only when every spelling
/// holds the same value; elsewhere, every pair.
fn folded(all: &[(&OsStr, &OsStr)]) -> Vec<bool> {
    all.iter()
        .enumerate()
        .map(|(at, &(name, value))| {
            if !cfg!(windows) || !governed(name) {
                return true;
            }
            let mut same = all
                .iter()
                .enumerate()
                .filter(|(_, (other, _))| other.eq_ignore_ascii_case(name));
            let first = same.next().map(|(first, _)| first);
            first == Some(at) && same.all(|(_, &(_, other))| other == value)
        })
        .collect()
}

/// Whether `name` is one of `list`: exactly on macOS and Linux, ignoring ASCII case on Windows.
fn listed(name: &OsStr, list: &[&str]) -> bool {
    list.iter().any(|listed| {
        if cfg!(windows) {
            name.eq_ignore_ascii_case(listed)
        } else {
            name == *listed
        }
    })
}

/// Whether the rule decides `name`: it is [`under_prefixes`] or, in any case, one of
/// [`COMPANIONS_OUTSIDE_PREFIXES`].
fn governed(name: &OsStr) -> bool {
    under_prefixes(name) || COMPANIONS_OUTSIDE_PREFIXES.contains(&upper(name).as_str())
}

/// `name` upper-cased as the prefix test reads it: by Unicode on Windows, ASCII elsewhere.
fn upper(name: &OsStr) -> String {
    let name = name.to_string_lossy();
    if cfg!(windows) {
        name.to_uppercase()
    } else {
        name.to_ascii_uppercase()
    }
}

#[cfg(test)]
mod tests;
