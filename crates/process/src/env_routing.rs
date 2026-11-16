//! The inherited `CLAUDE_*` and `ANTHROPIC_*` names ([`CLAUDE_PREFIXES`]) that a Claude child
//! Clauseo starts may keep: how a firm routes Claude. One rule for the agent launch (`host`,
//! #1167), sign-in's sign-in, status and logout runs (`crates/agents/src/signin/env.rs`) and
//! the plan readers (`crates/limits/src/readers/env.rs`), so Settings > Plans never says
//! signed in on a credential the agent would not use. Every other name under the prefixes is
//! dropped, and so are [`ALSO_DROPPED`]'s names outside them. A name a run sets
//! itself (sign-in's `ANTHROPIC_API_KEY=""`, the stored key, a firm plan's endpoint, the settings
//! entry's `ANTHROPIC_API_KEY: ""`) still wins over the inherited one.
//!
//! Why a Claude-specific list lives in the spawn crate: it is one list that host's launch,
//! sign-in's runs (`agents`) and the plan readers (`limits`) must all share, and `process` is
//! the one crate all three depend on (`host` never depends on `agents`, ARCHITECTURE.md).
//!
//! This is the single list for every Claude child Clauseo starts: host's launch scrub (#1167)
//! imports it, and there is no second copy. Its 65 names were reconciled with host's table at
//! 4ea6afd34 (`lw-review-fixes`).
//!
//! The rule (the orchestrator, after #1167's security review): a route is an inherited endpoint
//! or provider switch ([`ROUTES`]), plus the names that route needs (the rest of
//! [`ROUTING_KEPT`]: credentials, headers, regions, resources, model ids, mutual TLS, skipped
//! cloud auth). Those companions pass only beside a route ([`route_present`], worked out once per
//! run): a lone `ANTHROPIC_AUTH_TOKEN` or model id with no endpoint swaps the account or the
//! model, not the route, and drops. A name that only swaps the account, or points Claude at
//! something a local program can serve, is not a route and stays dropped: Clauseo has its own
//! sign-in, shown on Settings > Plans, an inherited account no screen shows would contradict it,
//! and any program can set one.
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
//! `AWS_BEARER_TOKEN_BEDROCK`, `AWS_ENDPOINT_URL*`, `_CLAUDE_CODE_ASSUME_FIRST_PARTY_BASE_URL`
//! (it starts with `_`), and the firm's proxy and CA (`HTTPS_PROXY`, `NODE_EXTRA_CA_CERTS`, …).

use std::ffi::{OsStr, OsString};

/// The prefixes the rule governs: host's `SCRUBBED_PREFIXES`. A name outside them passes.
pub const CLAUDE_PREFIXES: [&str; 2] = ["CLAUDE_", "ANTHROPIC_"];

/// The routing names that pass the scrub: the [`ROUTES`] first, then their companions, grouped as
/// the CLI groups them.
///
/// One list for every Claude child Clauseo starts: the launch (`host`), sign-in's runs
/// (`crates/agents/src/signin/env.rs`) and the plan readers (`crates/limits/src/readers/env.rs`).
/// Host's launch scrub (#1167) imports it; there is no second list.
pub const ROUTING_KEPT: [&str; 65] = [
    // The endpoints (the CLI's endpoint table @205038092, its switch @211334681).
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

/// How many of [`ROUTING_KEPT`]'s names, from the start, are routes: [`ENDPOINT_NAMES`]
/// endpoints, then six provider switches.
const ROUTE_NAMES: usize = 13;

/// How many of [`ROUTES`], from the start, are endpoints.
const ENDPOINT_NAMES: usize = 7;

/// The routes: [`ROUTING_KEPT`]'s endpoints and provider switches, each passing on its own.
pub const ROUTES: &[&str] = ROUTING_KEPT.split_at(ROUTE_NAMES).0;

/// The values a provider switch is on with, as Claude Code reads one (its truth test
/// @204302936: trimmed, in any case).
const ON: [&str; 4] = ["1", "true", "yes", "on"];

/// Whether `inherited` holds a route: an endpoint set to a value, or a provider switch that is
/// on (`1`, `true`, `yes` or `on`), names ignoring ASCII case. A switch set to `0` routes
/// nothing, so its companions drop.
pub fn route_present<'a>(inherited: impl IntoIterator<Item = (&'a OsStr, &'a OsStr)>) -> bool {
    let (endpoints, switches) = ROUTES.split_at(ENDPOINT_NAMES);
    inherited.into_iter().any(|(name, value)| {
        let named = |list: &[&str]| list.iter().any(|listed| name.eq_ignore_ascii_case(listed));
        (named(endpoints) && !value.is_empty())
            || (named(switches)
                && value
                    .to_str()
                    .is_some_and(|value| ON.iter().any(|on| value.trim().eq_ignore_ascii_case(on))))
    })
}

/// Whether the inherited `name` passes the scrub: it is a route, or a companion and `route`
/// ([`route_present`] of the whole inherited environment) is true. Ignores ASCII case.
pub fn kept(name: &OsStr, route: bool) -> bool {
    let (routes, companions) = ROUTING_KEPT.split_at(ROUTE_NAMES);
    let named = |list: &[&str]| list.iter().any(|listed| name.eq_ignore_ascii_case(listed));
    named(routes) || (route && named(companions))
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

/// Whether `name` starts with one of [`CLAUDE_PREFIXES`], ignoring ASCII case.
pub fn under_prefixes(name: &OsStr) -> bool {
    let bytes = name.as_encoded_bytes();
    CLAUDE_PREFIXES.iter().any(|prefix| {
        bytes
            .get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix.as_bytes()))
    })
}

/// `inherited` with the rule applied, in order: [`route_present`] once over all of it, then every
/// name outside [`CLAUDE_PREFIXES`] but [`ALSO_DROPPED`]'s, and inside them every name [`kept`]
/// keeps.
pub fn kept_inherited(
    inherited: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    let all: Vec<(OsString, OsString)> = inherited.into_iter().collect();
    let route = route_present(
        all.iter()
            .map(|(name, value)| (name.as_os_str(), value.as_os_str())),
    );
    all.into_iter()
        .filter(|(name, _)| {
            if under_prefixes(name) {
                kept(name, route)
            } else {
                !ALSO_DROPPED
                    .iter()
                    .any(|drop| name.eq_ignore_ascii_case(drop))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
