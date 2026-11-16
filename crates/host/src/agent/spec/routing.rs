//! The inherited `CLAUDE_*` and `ANTHROPIC_*` names that pass the scrub
//! (`super::SCRUBBED_PREFIXES`): how a firm routes Claude. Routing is the firm's, set in the
//! machine's environment, and passes as it did before the scrub, so the scrub never changes
//! where a lawyer's matters go. Whether Claude may start under an endpoint no plan set while
//! Claude Code's own sign-in is on is #1152's refusal (`agent.endpoint_key_needed`), not this
//! list's. A name the launch sets itself (the stored key, a firm plan's endpoint) still wins over
//! the inherited one.
//!
//! Exact names only, never a prefix or pattern: a name Claude Code adds later stays dropped
//! until someone reviews it and adds it here. Each comes from the pinned CLI, Claude Code
//! 2.1.293 (`@anthropic-ai/claude-agent-sdk-linux-x64/claude`, decimal byte offsets), read
//! statically (`hostfu/b4l-launchweak-routing.md`); re-check at each bump.
//!
//! Left out on purpose, so they stay dropped:
//! - `CLAUDE_CODE_OAUTH_TOKEN`, `CLAUDE_CODE_OAUTH_REFRESH_TOKEN`: a claude.ai account's token,
//!   not a firm's route; inherited, it would swap the lawyer's own sign-in for whatever account
//!   set it.
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
//! - Feature, tier, timeout, retry and telemetry names (`ANTHROPIC_BETAS`,
//!   `ANTHROPIC_BEDROCK_SERVICE_TIER`, `CLAUDE_CODE_API_KEY_HELPER_TTL_MS`, …).
//!
//! Outside both prefixes, so never scrubbed: `CLOUD_ML_REGION`, `AWS_REGION`,
//! `AWS_BEARER_TOKEN_BEDROCK`, `AWS_ENDPOINT_URL*`, `HTTPS_PROXY`, `NODE_EXTRA_CA_CERTS`.

/// The routing names that pass the scrub, grouped as the CLI groups them.
pub(crate) const ROUTING_KEPT: [&str; 56] = [
    // The provider (the provider list @205034126, its map @204797294).
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "CLAUDE_CODE_USE_ANTHROPIC_AWS",
    "CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD",
    "CLAUDE_CODE_USE_MANTLE",
    "CLAUDE_CODE_USE_GATEWAY",
    // Each provider's endpoint (the base-URL list @205037613, its switch @211334681).
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_BEDROCK_BASE_URL",
    "ANTHROPIC_BEDROCK_MANTLE_BASE_URL",
    "ANTHROPIC_VERTEX_BASE_URL",
    "ANTHROPIC_FOUNDRY_BASE_URL",
    "ANTHROPIC_AWS_BASE_URL",
    "ANTHROPIC_GOOGLE_CLOUD_BASE_URL",
    // The Files API's fallback base URL (@211588126).
    "CLAUDE_CODE_API_BASE_URL",
    // The API over a unix socket (@204890948).
    "ANTHROPIC_UNIX_SOCKET",
    // The resource, project, region or workspace (@211320120, @224642830, @211330348,
    // @225014021, @206793038, @211334977).
    "ANTHROPIC_FOUNDRY_RESOURCE",
    "ANTHROPIC_VERTEX_PROJECT_ID",
    "ANTHROPIC_GOOGLE_CLOUD_PROJECT",
    "ANTHROPIC_GOOGLE_CLOUD_LOCATION",
    "ANTHROPIC_GOOGLE_CLOUD_WORKSPACE_ID",
    "ANTHROPIC_AWS_WORKSPACE_ID",
    "ANTHROPIC_BEDROCK_REGION_PREFIX",
    "ANTHROPIC_SMALL_FAST_MODEL_AWS_REGION",
    // The endpoint's credentials and headers (the credential list @205039200; @205706766,
    // @205707624; headers @205706533).
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_FOUNDRY_API_KEY",
    "ANTHROPIC_FOUNDRY_AUTH_TOKEN",
    "ANTHROPIC_AWS_API_KEY",
    "ANTHROPIC_CUSTOM_HEADERS",
    // A gateway's own cloud auth skipped (@211328516, @206789764).
    "CLAUDE_CODE_SKIP_BEDROCK_AUTH",
    "CLAUDE_CODE_SKIP_VERTEX_AUTH",
    "CLAUDE_CODE_SKIP_FOUNDRY_AUTH",
    "CLAUDE_CODE_SKIP_ANTHROPIC_AWS_AUTH",
    "CLAUDE_CODE_SKIP_ANTHROPIC_GOOGLE_CLOUD_AUTH",
    "CLAUDE_CODE_SKIP_MANTLE_AUTH",
    // A gateway's mutual TLS and certificate store (@204880367, @204879295, @204876206).
    "CLAUDE_CODE_CLIENT_CERT",
    "CLAUDE_CODE_CLIENT_KEY",
    "CLAUDE_CODE_CLIENT_KEY_PASSPHRASE",
    "CLAUDE_CODE_CERT_STORE",
    // Workload identity federation and its profile, which can hold a base URL (the names
    // remote settings may not set unverified, @205331000; @210486664, @206604923).
    "ANTHROPIC_FEDERATION_RULE_ID",
    "ANTHROPIC_ORGANIZATION_ID",
    "ANTHROPIC_WORKSPACE_ID",
    "ANTHROPIC_SERVICE_ACCOUNT_ID",
    "ANTHROPIC_IDENTITY_TOKEN",
    "ANTHROPIC_IDENTITY_TOKEN_FILE",
    "ANTHROPIC_SCOPE",
    "ANTHROPIC_PROFILE",
    "ANTHROPIC_CONFIG_DIR",
    // Model ids, which on Bedrock and Vertex are ARNs or project/location paths naming the
    // account and region (the ARN pattern @102292824; that traffic follows the ARN's region is
    // unverified). A firm on Bedrock sets them with its inference profiles.
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL",
];
