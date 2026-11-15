//! The pass list: for each tool the guard allows, the only argument keys that reach
//! agent-browser. Every other key is refused as not supported, so a key agent-browser adds in a
//! later version is refused until it is read and put here; nothing passes by being unknown.
//!
//! The keys come from agent-browser v0.41.1's own input schemas for its `core` tools (`tool`,
//! `cli/src/mcp.rs:2044-2170` at the tag, which adds the keys every tool shares, and each
//! tool's definition in `tools`, `mcp.rs:832-1058`); the test
//! `every_key_of_agent_browsers_schemas_is_classified` reads that `tools/list` reply, kept in
//! `tests/agent_browser_core_tools.json`, and fails on a key that is neither passed here nor
//! refused by name in `filter.rs`. A key on this list still goes through every check in
//! `filter.rs` (a `url` is still judged, `newTab` and `webgpu` still pass only as `false`).
//!
//! Left off, beside the keys `filter.rs` refuses by name: `allowedDomains` (Clauseo controls the
//! sites, and agent-browser cannot apply it to the guard's Chrome, [`super::domains`]),
//! `webmcp` on `agent_browser_open` (an experimental Chrome feature for tools a page offers,
//! which Clauseo does not offer) and `all` on `agent_browser_close` (it closes every session in
//! the socket folder, where Clauseo has only the one the guard runs).

use super::MUST_BE_FALSE;

/// The keys every tool shares that pass, each still checked (`timeoutMs` up to
/// [`super::MAX_TIMEOUT_MS`], as every key of [`super::TIME_KEYS`] is; `extraArgs` from
/// [`super::EXTRA_ARGS_ALLOWED`] only).
pub(crate) const COMMON_KEYS: &[&str] = &["extraArgs", "timeoutMs"];

/// Each allowed tool's own keys that pass, in [`super::ALLOWED_TOOLS`]' order.
pub(crate) const TOOL_KEYS: &[(&str, &[&str])] = &[
    ("agent_browser_tools_profiles", &[]),
    ("agent_browser_open", &["url", "webgpu"]),
    (
        "agent_browser_read",
        &["filter", "outline", "raw", "readTimeoutMs"],
    ),
    (
        "agent_browser_snapshot",
        &[
            "compact",
            "delta",
            "depth",
            "full",
            "includeUrls",
            "interactive",
            "selector",
        ],
    ),
    ("agent_browser_back", &[]),
    ("agent_browser_forward", &[]),
    ("agent_browser_reload", &[]),
    ("agent_browser_click", &["selector", "human", "newTab"]),
    ("agent_browser_fill", &["selector", "text"]),
    (
        "agent_browser_type",
        &["selector", "text", "clear", "delayMs"],
    ),
    ("agent_browser_press", &["key"]),
    ("agent_browser_check", &["selector"]),
    ("agent_browser_uncheck", &["selector"]),
    ("agent_browser_select", &["selector", "values"]),
    ("agent_browser_scroll", &["selector", "direction", "amount"]),
    ("agent_browser_wait_ms", &["ms"]),
    (
        "agent_browser_wait_for_selector",
        &["selector", "waitTimeoutMs"],
    ),
    ("agent_browser_wait_for_text", &["text", "waitTimeoutMs"]),
    ("agent_browser_wait_for_load", &["state", "waitTimeoutMs"]),
    (
        "agent_browser_screenshot",
        &[
            "selector",
            "annotate",
            "format",
            "fullPage",
            "ifChanged",
            "quality",
            "threshold",
        ],
    ),
    ("agent_browser_get_text", &["selector"]),
    ("agent_browser_get_url", &[]),
    ("agent_browser_get_title", &[]),
    ("agent_browser_tab_new", &["url", "label"]),
    ("agent_browser_tab_list", &[]),
    ("agent_browser_tab_switch", &["tab"]),
    ("agent_browser_tab_close", &["tab"]),
    ("agent_browser_close", &[]),
];

/// Whether `key` passes on `tool`: one of [`COMMON_KEYS`] or of the tool's [`TOOL_KEYS`],
/// matched exactly, as agent-browser matches its keys (`reject_unknown_arguments`,
/// `mcp.rs:2264-2308`). A tool with no entry passes no key at all.
pub(crate) fn passes(tool: &str, key: &str) -> bool {
    COMMON_KEYS.contains(&key)
        || TOOL_KEYS
            .iter()
            .find(|(name, _)| *name == tool)
            .is_some_and(|(_, keys)| keys.contains(&key))
}

/// Whether `key` stays in `tool`'s advertised input schema (`schema.rs`): it passes, and is
/// not one of [`MUST_BE_FALSE`], which pass only as their default, `false`, so offering them
/// would only invite a refusal.
pub(crate) fn advertised(tool: &str, key: &str) -> bool {
    passes(tool, key) && !MUST_BE_FALSE.contains(&key)
}

/// The refusal of a key off the pass list. Fixed: a refusal never repeats text the agent
/// chose.
pub(crate) const UNSUPPORTED: &str = "This option is not supported in Clauseo.";
