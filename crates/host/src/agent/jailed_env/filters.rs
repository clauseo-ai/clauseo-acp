//! The names a jailed launch drops whatever their source (round 3b, reviewer M2 and lows 1-2):
//! credentials ([`is_credential`]), names that run code or move a tool's config ([`BLOCKED`]),
//! and names that are not ASCII ([`ascii`]). The setup hook keeps only the credentials its
//! selected route needs ([`ROUTES`], [`route_names`]).

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

/// One entry of [`CREDENTIALS`] or [`BLOCKED`]: a whole name, or every name with a prefix,
/// ignoring ASCII case.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Pattern {
    Name(&'static str),
    Prefix(&'static str),
}

impl Pattern {
    fn matches(self, name: &[u8]) -> bool {
        match self {
            Pattern::Name(known) => name.eq_ignore_ascii_case(known.as_bytes()),
            Pattern::Prefix(prefix) => name
                .get(..prefix.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(prefix.as_bytes())),
        }
    }
}

/// The names that carry or choose an agent's credentials, beside every secret-shaped name
/// (`clauseo_settings::secret_shaped`, the firm plan's own rule: a `_`-separated part ending in
/// `KEY`, `TOKEN` …, a part `AUTH`, `PAT` …). No jailed agent takes one from the lawyer's
/// environment (none passes `JAILED_ENV`) nor from its settings `env`; the setup hook passes
/// only its route's ([`ROUTES`]). Clauseo's own sign-in, the hook's stored key and the firm plan
/// set them instead.
pub(crate) const CREDENTIALS: [Pattern; 35] = [
    // Anthropic's keys, tokens and endpoint.
    Pattern::Name("ANTHROPIC_API_KEY"),
    Pattern::Name("ANTHROPIC_AUTH_TOKEN"),
    Pattern::Name("CLAUDE_CODE_OAUTH_TOKEN"),
    Pattern::Name("ANTHROPIC_BASE_URL"),
    Pattern::Name("ANTHROPIC_CUSTOM_HEADERS"),
    // Claude Code's client certificate (mTLS to a gateway): the key and the certificate.
    Pattern::Name("CLAUDE_CODE_CLIENT_KEY"),
    Pattern::Name("CLAUDE_CODE_CLIENT_CERT"),
    // Bedrock.
    Pattern::Name("CLAUDE_CODE_USE_BEDROCK"),
    Pattern::Name("CLAUDE_CODE_SKIP_BEDROCK_AUTH"),
    Pattern::Name("ANTHROPIC_BEDROCK_BASE_URL"),
    Pattern::Prefix("AWS_"),
    // Vertex and Google's own keys.
    Pattern::Name("CLAUDE_CODE_USE_VERTEX"),
    Pattern::Name("CLAUDE_CODE_SKIP_VERTEX_AUTH"),
    Pattern::Name("ANTHROPIC_VERTEX_PROJECT_ID"),
    Pattern::Name("ANTHROPIC_VERTEX_BASE_URL"),
    Pattern::Name("GOOGLE_APPLICATION_CREDENTIALS"),
    Pattern::Name("GOOGLE_CLOUD_PROJECT"),
    Pattern::Name("CLOUD_ML_REGION"),
    Pattern::Prefix("VERTEX_REGION_"),
    Pattern::Name("GEMINI_API_KEY"),
    Pattern::Name("GOOGLE_API_KEY"),
    // Microsoft Foundry.
    Pattern::Name("CLAUDE_CODE_USE_FOUNDRY"),
    Pattern::Name("CLAUDE_CODE_SKIP_FOUNDRY_AUTH"),
    Pattern::Prefix("ANTHROPIC_FOUNDRY_"),
    // OpenAI and Codex.
    Pattern::Name("OPENAI_API_KEY"),
    Pattern::Name("OPENAI_BASE_URL"),
    Pattern::Name("OPENAI_ORG_ID"),
    Pattern::Name("OPENAI_ORGANIZATION"),
    Pattern::Name("OPENAI_PROJECT"),
    Pattern::Name("CODEX_API_KEY"),
    // Azure, OpenAI's and the rest.
    Pattern::Prefix("AZURE_"),
    // Code hosts' and registries' tokens.
    Pattern::Name("GH_TOKEN"),
    Pattern::Name("GITHUB_TOKEN"),
    Pattern::Name("NPM_TOKEN"),
    Pattern::Name("HF_TOKEN"),
];

/// Whether `name` is a credential: in [`CREDENTIALS`], or secret-shaped by the firm plan's rule
/// (`clauseo_settings::secret_shaped`, the one function both use), ignoring ASCII case.
pub(crate) fn is_credential(name: &OsStr) -> bool {
    let bytes = name.as_encoded_bytes();
    CREDENTIALS.iter().any(|pattern| pattern.matches(bytes))
        || name.to_str().is_some_and(clauseo_settings::secret_shaped)
}

/// The names that run code in, or move the config of, the agent's shells and tools: dropped
/// from every source of a jailed launch. The host sets none of them itself; the Windows runner
/// sets `GIT_CONFIG_COUNT`, `GIT_CONFIG_KEY_0` and `GIT_CONFIG_VALUE_0` (git over OpenSSL,
/// `runner::env::GIT_OPENSSL`) after this filter, and those stay.
pub(crate) const BLOCKED: [Pattern; 16] = [
    // Run by bash and sh at start.
    Pattern::Name("BASH_ENV"),
    Pattern::Name("ENV"),
    // Moves zsh's start-up files.
    Pattern::Name("ZDOTDIR"),
    // Run before each prompt.
    Pattern::Name("PROMPT_COMMAND"),
    // Python's start-up file, module search and installation.
    Pattern::Name("PYTHONSTARTUP"),
    Pattern::Name("PYTHONPATH"),
    Pattern::Name("PYTHONHOME"),
    // Perl's and Ruby's extra switches (`-M` loads a module).
    Pattern::Name("PERL5OPT"),
    Pattern::Name("RUBYOPT"),
    // git's config, hooks and programs (`GIT_CONFIG_*`, `GIT_SSH_COMMAND` …).
    Pattern::Prefix("GIT_"),
    // macOS's library injection.
    Pattern::Prefix("DYLD_"),
    // Linux's.
    Pattern::Name("LD_PRELOAD"),
    Pattern::Name("LD_LIBRARY_PATH"),
    // npm's config, which names scripts' shell and registries.
    Pattern::Prefix("npm_config_"),
    // Node's switches (`--require`) and module search (`clauseo_process::NODE_ENV`).
    Pattern::Name("NODE_OPTIONS"),
    Pattern::Name("NODE_PATH"),
];

/// Whether `name` is in [`BLOCKED`], ignoring ASCII case.
pub(crate) fn is_blocked(name: &OsStr) -> bool {
    let bytes = name.as_encoded_bytes();
    BLOCKED.iter().any(|pattern| pattern.matches(bytes))
}

/// Whether `name` is all ASCII: Windows folds Unicode case in names, so a non-ASCII name could
/// match a listed one there and not here; such a name is dropped.
pub(crate) fn ascii(name: &OsStr) -> bool {
    name.as_encoded_bytes().is_ascii()
}

/// One sign-in route and the credentials it needs from the setup hook (desktop's
/// `setup::hook::setup_parts`, by exact name).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Route {
    /// The variable whose presence in the hook selects it; `None` for the routes that always
    /// apply.
    pub(crate) switch: Option<&'static str>,
    /// The names it needs, which pass the hook's credential filter.
    pub(crate) needs: &'static [&'static str],
}

/// The routes, which `pr-d2a.md` tabulates:
///
/// | Route | Selected by | The hook's credentials it keeps |
/// |---|---|---|
/// | Clauseo's stored key | always | `ANTHROPIC_API_KEY` (Claude), `CODEX_API_KEY` (Codex) |
/// | Firm plan or gateway (Anthropic) | always | `ANTHROPIC_BASE_URL`, the plan's endpoint |
/// | Bedrock | `CLAUDE_CODE_USE_BEDROCK` | it, `AWS_REGION`, `AWS_PROFILE` |
/// | Vertex | `CLAUDE_CODE_USE_VERTEX` | it, `CLOUD_ML_REGION`, `ANTHROPIC_VERTEX_PROJECT_ID` |
///
/// A plan never sets `GOOGLE_APPLICATION_CREDENTIALS` or `AWS_*` keys (`PLAN_ENV_NAMES`): Vertex
/// and Bedrock find their own sign-in files. The switches choose a route and carry no secret.
pub(crate) const ROUTES: [Route; 4] = [
    Route {
        switch: None,
        needs: &["ANTHROPIC_API_KEY", "CODEX_API_KEY"],
    },
    Route {
        switch: None,
        needs: &["ANTHROPIC_BASE_URL"],
    },
    Route {
        switch: Some("CLAUDE_CODE_USE_BEDROCK"),
        needs: &["CLAUDE_CODE_USE_BEDROCK", "AWS_REGION", "AWS_PROFILE"],
    },
    Route {
        switch: Some("CLAUDE_CODE_USE_VERTEX"),
        needs: &[
            "CLAUDE_CODE_USE_VERTEX",
            "CLOUD_ML_REGION",
            "ANTHROPIC_VERTEX_PROJECT_ID",
        ],
    },
];

/// The credential names `hook` may pass: the needs of every route it selects ([`ROUTES`]).
pub(crate) fn route_names(hook: &BTreeMap<OsString, OsString>) -> Vec<&'static str> {
    let selected = |switch: &str| {
        hook.iter().any(|(name, value)| {
            name.as_encoded_bytes()
                .eq_ignore_ascii_case(switch.as_bytes())
                && !value.is_empty()
        })
    };
    ROUTES
        .iter()
        .filter(|route| route.switch.is_none_or(selected))
        .flat_map(|route| route.needs.iter().copied())
        .collect()
}

/// Whether a variable's `value` is secret whatever its name: a web address with a password
/// (`clauseo_settings::secret_value`, the settings' own rule).
pub(crate) fn is_secret_value(value: &OsStr) -> bool {
    value.to_str().is_some_and(clauseo_settings::secret_value)
}

/// Whether the hook's `name` passes: not a credential, or one of its route's `names`.
pub(crate) fn hook_keeps(name: &OsStr, names: &[&str]) -> bool {
    !is_credential(name)
        || names.iter().any(|known| {
            name.as_encoded_bytes()
                .eq_ignore_ascii_case(known.as_bytes())
        })
}
