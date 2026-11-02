# 0030: The launch profile

R21, in three pull requests: `crates/profile` alone (this record's first part), one host pull
request with the settings and protocol pieces, and the desktop adapter. Each later pull request
fills its own heading below.

## Context

Clauseo gives Claude Code and Codex the same launch tools: each agent's own web search, the
`browser` MCP server (S27), and stetdoc on the `PATH` with its skill. The host already calls a
`ProfileHook` at each session start (`crates/host/src/config.rs`), with only the agent's id, and
forces the keys that keep MCP to the lawyer's servers (`crates/host/src/agent/loads.rs`). No
production hook exists. R22 (the legal instructions and three skills) builds on the same crate.

The host thread set the conditions the profile must keep: Claude keeps `settingSources:
["user"]`, strict MCP config, `ENABLE_CLAUDEAI_MCP_SERVERS=false`, the pre-write guard's
`PreToolUse` entry and `settings.disableAllHooks: false`; Codex keeps the hooks seam (one
`PreToolUse` group, the guard's) and `DISABLE_MCP_CONFIG_FILTERING=true`, and its home's
`config.toml` never carries hooks or MCP entries; file work stays off the host thread.

## Decision

### The crate (`crates/profile`, PR-A)

- **Plain values, no host types.** `profile` depends on `protocol` (for `AgentKind`),
  `settings` (for `Locations`), `process` (to start stetdoc), and `serde_json`, `sha2`,
  `futures`, `thiserror`, `tracing`; never `host`, `acp` or `tauri`. It is not a core crate
  (`CONTRIBUTING.md`'s list). `desktop` adapts it to `ProfileHook`.
- **`ClauseoProfile::new(paths, author: Option<String>)`.** `ProfilePaths::new(&Locations,
  stetdoc, BrowserSidecars { clauseo_mcp, agent_browser })` names Claude's plugin
  (`<data>/profile/claude-plugin`), Codex's skills folder (`Locations::agent_home("codex-acp")`
  then `skills`), stetdoc, the two browser sidecars and agent-browser's config
  (`<data>/browser/agent-browser.json`).
- **What the hook's output looks like, so the host's merge keeps its conditions.**
  - `session_meta(kind)`: Claude only. `systemPrompt.append`, and under
    `claudeCode.options` only `allowedTools`, `disallowedTools` (when not empty), `plugins`
    (one local plugin) and `settings`. It never holds `strictMcpConfig`, `settingSources`,
    `mcpServers`, `env`, `extraArgs`, `settings.hooks` or `settings.disableAllHooks` (a test
    checks the key set), so the host's forced keys and the guard's entry meet no profile value.
  - `codex_keys(kind)`: Codex only. `developer_instructions`. `CodexConfig::new` drops the
    reserved top keys `hooks`, `mcp_servers`, `features`, `cloud`, `skills` and `web_search`,
    so the hooks seam, R15's overrides and the host's `web_search` cannot be touched. The host
    merges the keys after the overrides and before `hooks`, adding only where no key is.
  - `servers(kind)`: `browser` for every kind, an agent with no kind included. `env()`:
    `STETDOC_AUTHOR`. `path_prepend()`: stetdoc's parent folder.
  - An agent with no kind (`fake-agent`) gets the servers and nothing kind-specific.
- **The `browser` entry.** Command `clauseo-mcp`; arguments `browser-guard --agent-browser
  <agent-browser> --cwd {session_dir}/browser -- mcp --tools core`; exactly four variables:
  `AGENT_BROWSER_CONFIG` (static), `AGENT_BROWSER_PROXY=http://127.0.0.1:{proxy_port}`,
  `AGENT_BROWSER_SESSION={session_id}` and `AGENT_BROWSER_EXECUTABLE_PATH={browser_executable}`.
  S27's guard derives the download, socket, screenshot, `HOME`, `TMPDIR` or `TEMP` and proxy
  variables from `--cwd` and the proxy value, so the entry carries none of them. The guard's
  `--cwd` is the session's browser state folder, never the matter folder.
- **Placeholders, and the host's contract** (`placeholders.rs`, the one home of the token
  strings; `fill` is the reference way to fill them).
  - Instructions: `{work_folder}` and `{matter_notes}` (R22). An unfillable one is a host bug:
    the host fails the launch with `agent.launch_failed`, naming the token.
  - `browser`: `{session_dir}`, `{proxy_port}`, `{session_id}`, `{browser_executable}`. The
    slot limit is checked first (`TooManyBrowsers`); then an unfillable `{proxy_port}` drops
    the entry with `NoProxy`, and an unfillable `{browser_executable}` with
    `BrowserNotInstalled`. The Chrome is the host's per-session choice, not the profile's.
  - One pass, left to right: a value put in is never scanned again, so a folder literally named
    `{session_id}` stays literal (tested).
  - The host fills decoded strings (a JSON string's content, one argument, one variable's
    value) and then encodes them for their destination: `serde_json` escapes the instructions
    in `_meta` and `CODEX_CONFIG`; MCP arguments go as an argument list with no shell; nothing
    is filled into TOML. Filling never happens inside text already encoded.
- **`{author}`** is filled by the profile, never the host. The named source is a settings key
  for the lawyer's name; there is none yet, so desktop passes `None` and tracked changes carry
  `Clauseo`. Never the OS account name. A name loses `"`, `\`, `` ` ``, `$`, `!`, braces and
  control characters, since it sits inside `--author "…"` in a shell command.
- **Claude's allow list:** `mcp__browser`, then `Bash(stetdoc <sub>:*)` for `view`, `inspect`,
  `check`, `lookup` and `capabilities`. The host adds `WebSearch` and the seeded
  `WebFetch(domain:…)` rules, or the off values, from `tools.web_search`.
- **`disableSkillShellExecution: true`** goes in every Claude session's `options.settings`
  from R21, not R22: R21 ships the first skill, whose text is stetdoc's own. The host merges
  `skipWebFetchPreflight` into the same object on a cloud login, as it already merges the
  guard's keys (so "settings only on cloud logins" in the brief no longer holds).
- **Skills, rendered per agent start with the hook actor-owned.**
  - `ClauseoProfile::with_skills(&[SkillSource])` renders once, when the profile is built:
    `skills::render` is pure (names, paths, checks, SHA-256 per file; no file read, no
    process). `skill_files(kind)` hands out a clone: cheap, safe on the host thread.
  - The launch-prep thread (`crates/host/src/actor/turn/launch_checks.rs`) calls
    `write_skills`, which runs `skills::write_rendered`: each file atomically, the manifest
    `.clauseo-manifest.json` updated, a drifted file (on disk not as the manifest recorded)
    warned about by name and rewritten, files the skill dropped removed, and every other folder
    and manifest entry left byte for byte.
  - stetdoc's skill: `skills::stetdoc::load_skill` runs `stetdoc --version` once when the app
    builds the profile, off the host thread; it runs `stetdoc skills get core` only when the
    cached copy is not that version's (or drifted). This is the only place the profile starts a
    process, through `clauseo-process` (absolute program, no shell, a tree of its own, a
    10-second limit).
  - The renderer refuses a file holding `` !` ``, a bad name or a path outside the skill's
    folder. `SkillSource::builtin` parses R22's fixed frontmatter (no YAML crate) and renders
    `name` and `description` alone.
- **Codex gets no skills (fallback (b)).** `CODEX_SKILLS` is `false`; `skill_files(Codex)` is
  none, and the stetdoc rules reach Codex through `developer_instructions`. R15 runs Codex with
  `skills.include_instructions=false`, and codex 0.162.1 has no key that keeps the lawyer's
  skills out while letting a root under the agent home in. Read at tag `rust-v0.162.1`:
  - `SkillsConfig` has only `bundled`, `include_instructions`, `max_context_tokens` and
    `config` (`codex-rs/config/src/skills_config.rs:41-56`).
  - The user layer's roots are `$CODEX_HOME/skills`, then `~/.agents/skills` from
    `dirs::home_dir()` (so `HOME`), then the system cache
    (`codex-rs/ext/skills/src/host_roots.rs:35-36`, `:95-114`); `/etc/codex/skills` comes
    from the system layer (`:115-120`) and repository `.agents/skills` from the working folder
    up (`:137`). Only `skills.bundled.enabled=false` removes a root, the system one
    (`skill_roots_for_config`, `codex-rs/ext/skills/src/host_service.rs:217` onwards).
  - `[[skills.config]]` rules match one `SKILL.md` path exactly or one name; no folder, prefix
    or glob (`codex-rs/config/src/skills_config.rs:103-130`), read from the user and
    session-flags layers only (`:159-164`). So they cannot keep out a skill not known in
    advance.
  - With `include_instructions=false` no catalog is rendered
    (`codex-rs/ext/skills/src/world_state_catalogs.rs:400`,
    `codex-rs/ext/skills/src/extension.rs:196`).

  Stage B owns `clauseo_limits::CODEX_OVERRIDES`; R21 does not change it. Turning Codex skills
  on needs a key that passes stage B's test: a skill placed in `~/.agents/skills` and in each
  system root never reaches the model's instructions.
- **Reviewer:** the owners' approval (the host thread for host, settings and protocol; the app
  thread for desktop) plus a high-effort review helper; not Rohan in person.

### The host, settings and protocol (PR-B)

Added by a later R21 PR.

### The desktop adapter (PR-C)

Added by a later R21 PR.

## Consequences

- R22 adds its text with `Instructions::with_part` and its skills with `SkillSource::builtin`
  and `ClauseoProfile::with_skills`, beside stetdoc's, and reuses `test_support::tree_text`
  for its snapshots. R22's Codex skills wait on the same key as R21's.
- The host either depends on `profile` for `placeholders::fill` or keeps its own copy with the
  single-pass test; desktop's tests then check both lists agree.
- U13 ships stetdoc in a `stetdoc/` folder of its own, and only that folder goes on the
  agents' `PATH`: it must never be the folder holding the other sidecars, or each of them
  becomes a bare command for the agent.
- The Claude plugin's shape (`plugins: [{type: "local", path}]` and
  `.claude-plugin/plugin.json`) is inferred, not run; G11 runs it.

## Status

proposed

## Date

2026-11-02
