//! The inherited `CLAUDE_*` and `ANTHROPIC_*` names that pass the scrub
//! (`super::SCRUBBED_PREFIXES`): how a firm routes Claude. Routing is the firm's, set in the
//! machine's environment, and passes as it did before the scrub, so the scrub never breaks a
//! route. Whether Claude may start under an endpoint no plan set while Claude Code's own sign-in
//! is on is #1152's refusal (`agent.endpoint_key_needed`), not this list's. A name the launch sets
//! itself (the stored key, a firm plan's endpoint) still wins over the inherited one, and so does
//! the settings entry's `ANTHROPIC_API_KEY: ""`.
//!
//! The rule: a route is an inherited endpoint or provider switch ([`ROUTES`]), plus the names
//! that route needs (the rest of [`ROUTING_KEPT`]: credentials, headers, regions, resources,
//! model ids, mutual TLS, skipped cloud auth). Those companions pass only beside a route
//! ([`holds_route`], worked out once per launch): a lone `ANTHROPIC_AUTH_TOKEN` or model id
//! with no endpoint swaps the account or the model, not the route, and drops. A name that only
//! swaps the account, or points Claude at something a local program can serve, is not a route
//! and stays dropped: Clauseo has its own sign-in, shown on Settings > Plans, an inherited
//! account no screen shows would contradict it, and any program can set one.
//!
//! Exact names only, never a prefix or pattern: a name Claude Code adds later stays dropped
//! until someone reviews it and adds it here. Each comes from the pinned CLI, Claude Code
//! 2.1.293 (`@anthropic-ai/claude-agent-sdk-linux-x64/claude`, decimal byte offsets from
//! `grep -aob`), read statically; re-check at each bump. The CLI groups them the same way: its
//! table of endpoints, each with its provider switch and companions, @205038092 (`O4e`).
//!
//! Left out on purpose, so they stay dropped:
//! - `ANTHROPIC_UNIX_SOCKET` (@204890948): the API over a local socket, not an endpoint.
//! - `CLAUDE_CODE_API_BASE_URL` (@211588126): the Files API's address when `ANTHROPIC_BASE_URL`
//!   is unset, so it never routes a firm's endpoint.
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

use std::ffi::OsStr;

/// The routing names that pass the scrub: the [`ROUTES`] first, then their companions, grouped as
/// the CLI groups them.
///
/// One list for every Claude child Clauseo starts. The sign-in and status child
/// (`crates/agents/src/signin/env.rs`) and the plan readers (`crates/limits/src/readers/env.rs`)
/// will use the same names with the same companion rule, from stage D's shared module
/// `clauseo_process::env_routing` (`crates/process/src/env_routing.rs`, its own PR; the host
/// never depends on `clauseo-agents`). This list must match that one name for name, and a
/// change to one goes in the other; once stage D's PR lands, this copy goes and the host will
/// import `clauseo_process::env_routing`.
pub(crate) const ROUTING_KEPT: [&str; 65] = [
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

/// How many of [`ROUTING_KEPT`]'s names, from the start, are routes: [`ENDPOINT_NAMES`]
/// endpoints, then six provider switches.
const ROUTE_NAMES: usize = 13;

/// How many of [`ROUTES`], from the start, are endpoints.
const ENDPOINT_NAMES: usize = 7;

/// The routes: [`ROUTING_KEPT`]'s endpoints and provider switches, each passing on its own.
pub(crate) const ROUTES: &[&str] = ROUTING_KEPT.split_at(ROUTE_NAMES).0;

/// The values a provider switch is on with, as Claude Code reads one (its truth test
/// @204302936: trimmed, in any case).
const ON: [&str; 4] = ["1", "true", "yes", "on"];

/// Whether `inherited` holds a route: an endpoint set to a value, or a provider switch that is
/// on ([`ON`]), names ignoring ASCII case. A switch set to `0` routes nothing, so its
/// companions drop.
pub(crate) fn holds_route<'a>(inherited: impl IntoIterator<Item = (&'a OsStr, &'a OsStr)>) -> bool {
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
/// ([`holds_route`] of the whole inherited environment) is true. Ignores ASCII case.
pub(crate) fn kept(name: &OsStr, route: bool) -> bool {
    let (routes, companions) = ROUTING_KEPT.split_at(ROUTE_NAMES);
    let named = |list: &[&str]| list.iter().any(|listed| name.eq_ignore_ascii_case(listed));
    named(routes) || (route && named(companions))
}
