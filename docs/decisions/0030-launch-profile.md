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
  app_exe, stetdoc, BrowserSidecars { clauseo_mcp, agent_browser })` returns a `Result`: every
  program path must be absolute. It names Claude's plugin (`<data>/profile/claude-plugin`),
  Codex's skills folder (`Locations::agent_home("codex-acp")` then `skills`), stetdoc, the two
  browser sidecars and agent-browser's config (`<data>/browser/agent-browser.json`). Its
  fields are private, read through getters.
- **What the hook's output looks like, so the host's merge keeps its conditions.**
  - `session_meta(kind)`: Claude only. `systemPrompt.append`, and under
    `claudeCode.options` only `allowedTools`, `disallowedTools` (when not empty), `plugins`
    (one local plugin) and `settings`, which holds only `disableSkillShellExecution`.
    `ClaudeOptions`' fields are private and `meta_fragment` emits only `OPTION_KEYS` and
    `SETTINGS_KEYS`, dropping any other settings key with a warning. It never holds
    `strictMcpConfig`, `settingSources`, `mcpServers`, `env`, `extraArgs`, `settings.hooks`
    or `settings.disableAllHooks` (tests check the key set, and that a `hooks` setting is
    dropped), so the host's forced keys and the guard's entry meet no profile value.
  - `codex_keys(kind)`: Codex only. `developer_instructions`. `CodexConfig::new` drops any
    key holding `.`, and any whose first part is reserved: `hooks`, `mcp_servers`, `features`,
    `cloud`, `skills`, `web_search` and `bypass_hook_trust`. So the hooks seam, R15's
    overrides and the host's `web_search` cannot be touched. codex reads each request key as a
    dotted path: at tag `rust-v0.162.1`, `codex-rs/app-server/src/config_manager.rs:446-454`
    chains the request's keys into the CLI overrides, `codex-rs/config/src/overrides.rs:18-22`
    splits each on `.`, and `config_manager.rs:438` reads `bypass_hook_trust` straight from
    the request. The host merges the keys after the overrides and before `hooks`, adding only
    where no key is, and treats a dotted key as a clash with its first part.
  - `servers(kind)`: `browser` for every kind, an agent with no kind included. `env()`:
    `STETDOC_AUTHOR`. `path_prepend()`: `Option`, stetdoc's parent folder, or none (with a
    warning) when that folder also holds a browser sidecar or the app's own executable, or is a
    root. Folders compare without regard to case or separator.
  - An agent with no kind (`fake-agent`) gets the servers and nothing kind-specific.
- **The `browser` entry.** Command `clauseo-mcp`; arguments `browser-guard --agent-browser
  <agent-browser> --cwd {browser_dir} --cdp-port {cdp_port} -- mcp --tools core`, with
  `--daemon-port {daemon_port}` after `--cdp-port` on Windows; five variables:
  `AGENT_BROWSER_CONFIG` (static), `AGENT_BROWSER_PROXY=http://127.0.0.1:{proxy_port}`,
  `AGENT_BROWSER_SESSION={session_id}`, `AGENT_BROWSER_EXECUTABLE_PATH={browser_executable}` and
  `AGENT_BROWSER_STREAM_PORT={stream_port}`, and on macOS and Linux a sixth,
  `AGENT_BROWSER_SOCKET_DIR={socket_dir}` (S27's ports-and-socket PR, 0029). S27's guard derives
  the download, screenshot, `HOME`, `TMPDIR` or `TEMP`, Windows socket and proxy variables from
  `--cwd` and the proxy value, so the entry carries none of them. The guard's
  `--cwd` is the session's browser state folder, never the matter folder.
- **Placeholders, and the host's contract** (`placeholders.rs`, the one home of the token
  strings and of `fill`, both public, documented API). The host depends on `profile` and calls
  `placeholders::fill` with these constants; it keeps no second copy. PR-B adds `profile` to
  the host's row in `ARCHITECTURE.md`.
  - Instructions: `{work_folder}` and `{matter_notes}` (R22). An unfillable one is a host bug:
    the host fails the launch with `agent.launch_failed`, naming the token.
  - `browser`: `{browser_dir}`, `{proxy_port}`, `{session_id}`, `{browser_executable}`,
    `{socket_dir}` (macOS and Linux), `{stream_port}`, `{cdp_port}` and `{daemon_port}`
    (Windows), the last four from the session's claims (0029); an unfillable one of those
    drops the entry with `BrowserNotInstalled`.
    `{browser_dir}` is one whole token, `<data>/run/<session id>/browser`, which the host joins
    with the platform's separators and fills with a `dunce`-simplified path (no `\\?\` prefix),
    so a Windows path never mixes separators. The
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
  `Clauseo`. Never the OS account name. A name keeps only letters, combining marks, digits,
  spaces and `.,'-` (an allowlist), since it sits inside `--author "…"` in a shell command:
  curly quotes (PowerShell), `%VAR%` (cmd), bidi controls and every other sign go.
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
    and manifest entry left byte for byte. A file the manifest lists that is gone counts as
    drifted. Stale files are removed before any write. The manifest carries `"format": 1` and
    keeps top-level keys it does not know.
  - The write never follows a symlink under the skills folder (each level is checked with
    `symlink_metadata`), re-checks every name and path it is handed, sweeps its own leftover
    `.<name>.<pid>-<n>.tmp` files, and holds one process-wide lock. `Rendered`,
    `RenderedSkill` and `SkillWrite` have private fields, so they come only from `render` and
    `skill_files`. `skill_files` shares the rendered bytes through an `Arc`.
  - **A skill-write failure never fails a launch.** The host logs an error from `write_skills`
    as a warning and starts the agent with what the folder already holds.
  - stetdoc's skill: `skills::stetdoc::load_skill` runs `stetdoc --version` once when the app
    builds the profile, off the host thread; it runs `stetdoc skills get core` only when the
    cached copy is not that version's (or drifted). This is the only place the profile starts a
    process, through `clauseo-process` (absolute program, no shell, a tree of its own, a
    10-second limit, killed as soon as its stdout passes 1 MiB, with the cap named in the error).
  - stetdoc's text is not Clauseo's, so its frontmatter is cut to `name: stetdoc` and its
    `description` (read leniently: bare, quoted, folded or literal); `allowed-tools`, `hooks`
    and every other key go. `with_skills` renders skill by skill (`skills::render_each`): a
    skill the renderer refuses is left out with a warning and returned, and the others go on.
  - The renderer refuses a file holding `` !` ``, a bad name, a `SKILL.md` whose frontmatter is
    not `name` and `description` alone, two paths that differ only in case, or a path part
    outside `[A-Za-z0-9._-]`, starting or ending with `.`, or naming a Windows device.
    `SkillSource::builtin` parses R22's fixed frontmatter (no YAML crate) and renders `name`
    and `description` alone.
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

- **The hook.** `ProfileHook` gains three methods, each defaulting to none, so `NoProfile`
  is unchanged: `mcp_servers(agent)`, `codex_config(agent, written)` and
  `launch_writes(agent) -> Vec<SkillWrite>`. The host re-exports `clauseo_acp::McpServer`. `host` now depends on
  `profile`, for `placeholders::fill`, the token constants and `write_skills`. `profile`
  brings no external crate that `host` did not already have (`dunce`, `futures`,
  `serde_json`, `sha2`, `thiserror`, `tracing`), so ADR 0024's list does not change.
- **No `_meta` servers.** `claudeCode.options.mcpServers` from the hook is removed in every
  control mode (`agent/loads.rs`). Servers come only from `mcp_servers`, after the lawyer's
  enabled servers.
- **The browser entry** (`agent/loads/browser.rs`, beside `mcp_servers`):
  - The host fills `IN_SERVERS` into each argument and each variable's value, one pass per
    string. `{browser_dir}` is `dunce`-simplified.
  - The entry is dropped, with the header's reason, in this order:
    1. `tools.browser` off (`Off`, or `LockedOff`).
    2. `managed-mcp.json` (`ManagedByFirm`): under Managed no hook server goes at all, the
       browser included, and `settingSources` stays `["user"]` and strict stays off.
    3. No slot (`TooManyBrowsers`).
    4. No proxy port (`NoProxy`).
    5. No Chrome (`BrowserNotInstalled`).
  - S27's host PR (record 0029, "The host part") replaced the stand-ins: the launch prep finds
    the Chrome (`with_browser_executable` stays as a test-support override), and the slots are
    counted per session (`with_browser_slots` is gone; `with_machine_memory` sets the limit).
  - `HostConfig::with_egress_proxy(Arc<dyn EgressProxy>)`, with `port_for(SessionKey)`, and
    `SessionKey { matter, thread, launch }`, are added here as stage C's R17 names them. Tests
    use `TestProxy`. Production passes none, so production has no browser until stage C.
- **Per-session folders** (`actor/turn/launch_checks.rs`). The launch-prep thread makes them
  after the data-folder and `.codex` refusals, and before any `session/*` call:
  - `run/<session id>/{work,browser}`. The session id is `<thread>-<launch>`, two opaque ids
    and never user text. `launch` is the launch prep's id, from the agent ids' counter (a
    deviation: R17 names `Agent::id`).
  - `agent-state/<matter id>/notes`.

  Each part below the data folder must be a real folder or be made as one
  (`write::create_private_dirs_under`, by `symlink_metadata`): nothing is made through a link
  at `run` or `agent-state`, and a folder that cannot be made fails the launch and takes its
  run folder back. The notes folder stays when a session ends.
- **The run folder's life.** Each launch removes only its own run folder, never a prefix of
  folders, so a late prep cannot touch a newer launch's. It goes, off the actor, on every way
  out:
  - a prep the actor stopped waiting for (past `PREPARE_WAIT`) undoes its own folders on its
    thread (`codex::prepare::off_actor_or_undo`: the actor closes the answer channel when it
    gives up, so each answer is either taken or undone);
  - an answer for an earlier launch, a stopping host, failed profile keys, or a failed
    `launch` (`launch_checks::discard`);
  - the agent's end (`remove_run`), once its process has exited.

  The removal takes only the path the host built, `<data>/run/<session id>`, never
  canonicalized, with `remove_dir_all`, which removes a link inside and never follows it.
- **The start-up sweep.** The first launch prep, under a lock every prep takes before making
  its folders, renames `run` to `run.old-<n>` in one call, and a `clauseo-run-sweep` thread
  deletes every real `run.old-*` folder. No launch waits on the delete. A `run` or
  `run.old-*` that is a link is never swept through.
- **Instructions.** `{work_folder}` and `{matter_notes}` are filled after `session_meta`, in
  `_meta.systemPrompt.append`, and at the launch in Codex's `developer_instructions`. An
  unfillable token, or a folder that is not UTF-8, fails the launch with
  `agent.launch_failed`, naming the token.
- **Claude** (`agent/loads/profile.rs`, a `claude-acp` agent only). These are added after the
  hook and before the guard's entry, so `hooks` and `disableAllHooks: false` still go last:
  - `disallowedTools` gains `WebSearch` and `WebFetch` while `tools.web_search` is off, and
    `allowedTools` loses every `WebSearch` and `WebFetch(...)` rule.
  - While it is on, `allowedTools` gains `WebSearch` (not on Bedrock) and one
    `WebFetch(domain:<host>)` per host in `host/src/browser/seed_sites.rs`. That file is R21's
    copy of the hosts S27 names; S27 adds the High Courts.
  - `additionalDirectories` gains the work folder and the matter's notes folder, after the
    hook's entries.
  - `settings` gains `disableSkillShellExecution: true`, and `skipWebFetchPreflight: true` when
    the merged launch env sets `CLAUDE_CODE_USE_BEDROCK`, `_VERTEX` or `_FOUNDRY` to a value
    that is not empty and not `0`.
- **Codex** (stage B's conditions). Of the hook's keys only an allowlist goes in,
  `agent::codex::PROFILE_KEYS`: `developer_instructions`, the one key `clauseo_profile` emits.
  Every other key is refused with a warning, `hooks`, `mcp_servers`, `sandbox_mode`,
  `sandbox_workspace_write`, `approval_policy`, `skills`, `features`, `projects`,
  `model_providers`, `profiles` and `notify` among them, and `agent::codex::config` refuses
  any key outside `developer_instructions`, `web_search` and `sandbox_workspace_write` again.
  The host then adds its own `web_search` (`live` or `disabled`) and
  `sandbox_workspace_write.writable_roots` (exactly the work folder and the matter's notes
  folder, never a parent). `config` builds, in order:
  1. the settings' model keys;
  2. those keys, merged add-only;
  3. R15's `CODEX_OVERRIDES` as nested keys over everything, so R15's pins win and the
     writable roots merge beside them;
  4. `hooks`, last.

  So `hooks` stays one `PreToolUse` group, and nothing from the profile reaches the home's
  `config.toml`, which takes no profile input.
- **`_meta` layering.** The hook's `_meta`, then the host's switches, then the guard. Each key
  the host sets replaces the hook's whole: `strictMcpConfig`, `settingSources`, the connector
  variable, `disableSkillShellExecution`, `disableAllHooks`. The one append is the guard's
  `PreToolUse` entry, which goes after the hook's own entries, as the host thread pinned it. The guard's entry sitting after the hook's is safe while Claude Code applies deny over allow
  across hooks, and the hook is Clauseo's own profile, not the lawyer's.
- **Substitution scope.** `{work_folder}`, `{matter_notes}` and the browser's tokens are
  filled only in what the profile supplied: its instructions, its `systemPrompt.append` and
  its `browser` entry. The lawyer's own MCP servers and a settings agent's `env` reach the
  agent as written (`tests/host/profile.rs`, `tokens_are_filled_only_in_the_profiles_values`).
- **The notes folder** is made at 0700 on Unix (an earlier one is set to 0700), and refused,
  failing the launch with `agent.launch_failed` naming the link, when `agent-state`, the matter
  folder or `notes` below the data folder is a link: the rule `agent_home` follows.
- **`DISABLE_MCP_CONFIG_FILTERING=true`** does not change the browser entry (codex-acp 2.2.2,
  `dist/index.js`). The variable is read once, at `:35820`, into
  `shouldDeduplicateMcpConflicts()` (`:35821`), which `createSessionConfig` (`:35437`) uses at
  `:35459-35460`:
  - With filtering on, a requested server whose name is already in any Codex config layer is
    dropped (`:35465-35467`, names from `getConfigMcpServerNames`, `:35489-35498`).
  - With it off, the requested entry wins instead.

  Clauseo's home `config.toml` defines no `mcp_servers`, so `browser` is sent either way, as
  `mcp_servers.browser` with `command`, `args` and `env` only (`createMcpSeverConfig`,
  `:35519-35534`). codex-acp sends no per-server approval key, so Codex asks before each
  browser tool call that is not read-only.
- **Skills.** At the prep, for an agent with a kind, the host writes `launch_writes(agent)`, in
  order, with `write_skills`. The desktop adapter (PR-C) hands Claude's plugin first, to Codex
  agents too, for R22's (b′) index. A write failure is a warning, not a failed launch. The
  first write's `SkillsWritten` rides in that launch's `Prepared` and goes to its
  `codex_config(agent, Some(token))`, which the adapter hands to `ClauseoProfile::codex_keys`;
  a failed write gives `None`, and the adapter calls `codex_keys_without_index` (0031). The
  token is taken out when the keys are built, so no later launch can reuse it.
- **PATH.** `HostConfig::with_agent_path_prepend` puts stetdoc's folder first on the merged
  `PATH` (`agent/spec.rs`, `prepend_path`), under the name the merge kept. It shadows nothing
  the host relies on: the guard's hooks run the desktop executable by absolute path.
- **Settings** (`crates/settings`). The `tools` section holds `web_search` (default `true`)
  and `browser` (`true` on macOS only; off on Windows until G17 passes). Both are security
  keys:
  - `SECURITY_KEYS` holds `mcp.servers`, `tools.web_search` and `tools.browser`.
  - An unreadable policy source, or a policy value of the wrong type, locks each through
    `refuse`, separate from `reset`: `false` for the tools keys, empty for `mcp.servers`. The
    warning says "locked off". This departs from 0009's "locks at its default" for these two
    keys.
  - A policy name under `tools` that this version cannot read refuses only the keys it could
    stand for: `tools.websearch` refuses `tools.web_search` alone. A name that could stand for
    either (`"tools": "off"`, `tools.brwser`, a `Tools` section) refuses both, except a
    sibling the same source sets validly by its exact name, which keeps its value
    (`leaf::could_stand_for`). An unreadable source still locks every security key off.
  - `mcp.servers.browser` is a reserved name, dropped with a warning from the user's file and
    from policy; `clauseo` loads like any other.
- **Header tools** (`protocol`: `HeaderEntry::tools`, `ToolState`, `ClauseoTool`,
  `ToolAvailability`, `ToolsUnavailableReason`, open enums, left out when empty):
  - The host composes them at each session start, and never logs them. Per the host thread,
    they are not in the reducer's state: `actor/header_tools.rs` keeps each thread's last
    value in memory (as `folder_state` is kept for app rows). The thread's `StreamView` lays
    it over every header upsert and every `Reset` window, so a new subscriber's first frame
    carries it, and a relaunch replaces it.
  - Each session start works out the tools and holds them (`hold`); they show only once
    `session/new`, `session/load` or `session/resume` answers (`show_held`, one line in
    `session::on_started`), so a session that never starts shows none. Archiving a thread
    drops its entries (the host deletes no thread). 0002 now says a header may carry
    host-composed, unlogged fields.
  - Web search appears for an agent with a kind; the browser appears when the hook offers one.
- **Busy files touched.** `actor/turn/agents.rs`, three lines: the Codex keys into
  `codex::launch_env`, the `PATH` folder into `launch_spec`, and `remove_run` in `on_ended`.
  `actor/startup.rs`, three fields. `actor/turn/session.rs`: `session_loads` now calls
  `turn/profile_loads.rs`, and `on_started` shows the held tools (one line).
  `actor/commands.rs`: archiving forgets the header's tools (one line).
- **No I/O in the hook.** `ProfileHook`'s methods run on the host thread and must do no I/O;
  its doc says so. Known follow-up, older than R21: `agent::loads::mcp_control` still stats
  `managed-mcp.json` on the actor at each session start, and should move off it.

### Deviations from the brief (PR-B)

- **`SessionKey.launch`** is the launch prep's id, not `Agent::id`.
- **The start-up sweep** runs at the first launch prep, under a lock, not at host start; it is
  one rename there, and the delete runs on its own thread.
- **The reserved server name `browser`** is matched ignoring case.
- **Header tools** are shown when the session call answers, not when it is sent.
- **`protocol/src/wire.rs`** needs no change here: main's `bdf5c2b8` keeps a skipped field
  that is not an `Option`, so `HeaderEntry`'s `tools?: ToolState[]` is in `bindings.ts`, and
  `specta_export.rs` asserts it.
- **One line in `xtask/expect-allowlist.txt`** (`prepend_path`) needs code-owner approval.
  `StreamHeader` needs none: main boxes its thread header.

### The desktop adapter (PR-C)

`crates/desktop/src/profile.rs` holds `DesktopProfile`, the app's `ProfileHook`. Desktop gains
two dependencies, `profile` and `tracing`.

- **What is mapped.** The agent's kind is `AgentKind::of(agent)`. An agent with no kind
  (neither `claude-acp` nor `codex-acp`) gets nothing from any method.
  - `session_meta` is `ClauseoProfile::session_meta`, Claude's `_meta` fragment (the
    instructions and the plugin folder).
  - `launch_env` is `env()` (`STETDOC_AUTHOR`).
  - `mcp_servers` maps each `ProfileServer` to `McpServer::Stdio`.
  - `launch_writes` is `ClauseoProfile::launch_writes`, Claude's plugin first.
  - `codex_config(agent, written)` calls `codex_keys(kind, token)` with a token, and
    `codex_keys_without_index(kind)` without one. The adapter keeps no token and no state
    between calls.
- **Start-up.** The profile is built once, on the host-start thread (`host_start.rs`), before
  `Host::start`. The window never waits on it, but the host starts after it, and every bridge
  command waits for the host. So loading stetdoc's skill is bounded: both of stetdoc's runs
  together get `STETDOC_START_LIMIT`, 3 s. Past it, the load is dropped, which kills
  stetdoc's process tree, and stetdoc is treated as absent (no skill, no prepend, a warning).
  The rest of the build is file reads and rendering. The build finds the
  app's executable, runs stetdoc for its skill (`load_skill`, cached in the plugin's
  `skills/` folder), and renders R22's built-ins. The hook's methods then only map what it
  holds. The browser's sidecars are named beside the executable, where Tauri's `externalBin`
  puts them (S27 bundles them). The author is `None`, so tracked changes carry `Clauseo`.
- **Failure.** A stetdoc skill that fails, or a refused built-in, is left out with a warning,
  and the profile is built without it. If the profile itself cannot be built (no executable
  path, or `ProfilePaths::new` refuses a path), the app logs a warning and starts with no
  hook (`NoProfile`), as before PR-C. The desktop installs no tracing subscriber yet, so
  these warnings go nowhere until it has a log.
- **stetdoc.** U13's sidecar is not bundled on this base. A debug build takes the first
  `stetdoc` (with the OS's suffix) in an absolute folder of `PATH`. A release build treats
  stetdoc as absent. The lookup matches `which`: a file that is not a program (no execute bit
  on Unix) is skipped and the walk goes on, and on Windows only `stetdoc.exe` counts, since
  stetdoc starts with no shell. When stetdoc is absent, `ProfilePaths` names U13's future path
  (`<exe folder>/stetdoc/stetdoc`), which is neither run nor put on `PATH`. When stetdoc is
  found and its skill loaded, `with_agent_path_prepend` takes `ClauseoProfile::path_prepend()`,
  but only when stetdoc is the only program in its folder. A folder found on `PATH` that holds
  others (`~/.cargo/bin`, `/usr/local/bin`) is not put first (a warning says so), since it would
  shadow the system's programs; the agent still finds stetdoc on the `PATH` it inherits.
  The folder must be absolute, since the host takes it as given: `with_profile` checks it
  (`absolute_folder`), with a `debug_assert!` and, in a release build, a warning and no
  prepend. `ProfilePaths::new` already refuses a relative stetdoc, so this guards a bug only.
  The `PATH` walk is one `#[expect(clippy::disallowed_methods)]` (`first_on_path`), with
  its line in `xtask/expect-allowlist.txt`.
- **No proxy yet.** Production calls no `with_egress_proxy`: stage C's proxy (R17) is not
  wired. The host drops the `browser` entry, and the header shows the browser unavailable
  with `NoProxy`. Wiring the proxy is one builder call once stage C has it.
- **Known limit (follow-up issue).** The host's `Folders::of` fails at every session start
  when the data folder's path is not UTF-8, so such a data folder starts no agent session.
  The fix is in the host, not here.
- **Test seam.** Under `test-support`, `HostConfig` gains `profile_hook()` and
  `agent_path_prepend()`, so the desktop's wiring (`with_profile`) is tested on a real
  config.

### Deviations from the brief (PR-C)

- **`tracing`** is a second new desktop dependency, beside `clauseo-profile` (the brief says
  only one): the adapter's start-up warnings use it, as `profile` and `host` do.
- **No tracing subscriber** exists in the desktop yet, so those warnings (a stetdoc skill left
  out or stopped, a shared or relative `PATH` folder, no profile) are not visible anywhere
  until one is installed.
- **The stetdoc lookup matches `which`**, not only "a file named stetdoc": a non-program file
  earlier on `PATH` is skipped. The brief's "found by `which`" left this open.
- **A slow stetdoc is bounded at 3 s** at start-up (`STETDOC_START_LIMIT`), within stetdoc's own
  10 s per run (`RUN_LIMIT`).

### Tool switches over the wire

Added by the R21 follow-up. The lawyer's two switches, `tools.web_search` and `tools.browser`,
change from the settings window as `ui` does.

- **Command.** `HostCommand::SetToolSettings { patch: ToolsPatch }` sets the fields the patch
  names, each optional, and leaves the rest. Unknown fields are refused. It answers `Done`, and
  every app subscriber gets `SettingsChanged`. Only the settings window may send it: the main
  window refuses it (`command.invalid`), since the switches live in Settings and the main window
  only offers to open Settings (least privilege; the app thread's call).
- **View.** `SettingsView.tools: ToolsSettingsView { web_search, browser }` carries the effective
  values in `GetSettings` and `SettingsChanged`.
- **Lock.** `Effective::apply_tools_patch` refuses the whole patch with `settings.locked` when it
  names a tool key policy sets, and then writes nothing. Otherwise it writes the `tools` section
  of `settings.json` (write first, then memory), as `apply_ui_patch` writes `ui`.
- **Next session start.** A session reads the switches once, at its start (`switches` in
  `actor/turn/profile_loads.rs`). A change reaches the next session started. A session already
  running keeps its tools: its header keeps what it held (`actor/header_tools.rs`), and nothing
  re-reads the switches mid-session.

### Forced ask in an unprotected matter

- **2026-11-04 (Rohan; private `folder-rights.md`, decision 3).** A Claude session in a matter
  whose folder is on a network drive or a drive without file permissions (the Windows folder
  check, `agent/folder_support.rs`, answers `network` or `no_permissions` at that launch,
  `session/load` included) runs in forced ask until the sandbox can fence such a folder. Codex
  is refused there. A check that fails (an io error) is unknown and fails closed for Claude:
  forced ask, `check_failed` on the matter's row (2026-11-05, orchestrator; nothing before),
  and Codex not refused (its own sandbox applies). The host enforces four layers
  (`agent/forced_ask.rs`):
  - **Modes:** `SetMode` takes only `default` and `plan` (any other id is `command.invalid`),
    and so does `SetConfigOption` for claude-agent-acp's `mode` option, which sets the mode
    too; a mode the agent moves to itself is switched back to `default`.
  - **Options:** a permission request loses its `allow_always` options and the plan exit's
    options that switch to a skipping mode (`exit-plan-auto`, `exit-plan-accept-edits` and the
    rest, claude-agent-acp 0.89.1 `permissions/options/shared.js`); an answer naming one is
    refused.
  - **No `allowedTools`, and ask rules:** not the host's web rules, not the profile hook's. The
    flag settings carry `permissions.ask` rules for `Bash`, `PowerShell`, `Edit`, `Write`,
    `MultiEdit`, `NotebookEdit`, `WebFetch`, `WebSearch` and `mcp__*`, and
    `sandbox.autoAllowBashIfSandboxed: false`: if the hook fails, times out or is blocked, Claude
    Code 2.1.293 checks deny rules, then ask rules, then allow rules over every source, so the
    lawyer's own `allow` rules no longer approve unasked (read from the CLI binary; the
    detail is in `agent/forced_ask.rs` `ASK_RULES`).
  - **The hook:** the guard's entry matches every tool (`*`, MCP tools included) and passes
    `--ask-all`, so the guard answers `ask` for every tool but `Read`, `Glob`, `Grep`, `LS`,
    `NotebookRead`, `TodoWrite`, `Task` and `ToolSearch` (which only loads deferred tools'
    schemas; added 2026-11-05 after the live run, orchestrator); `WebSearch` and `WebFetch` ask
    too. The flag is an argument, not a variable, so it never reaches the agent's shell.
  A forced-ask Claude launch with no guard hook to give (no guard program, as when
  `current_exe` failed, or a path that is not UTF-8) is refused with `agent.guard_missing`
  (#402; it was `agent.launch_failed` before).
- **2026-11-05 (orchestrator; accepted risk).** A `PermissionRequest` hook already in the
  lawyer's own settings or plugins can still approve in forced ask: Claude Code runs such hooks
  alongside the SDK's permission callback, and whichever decides first applies. The risk is
  accepted because only something the lawyer or their IT already installed can do it; the agent
  cannot install one without a yes. Keeping user hooks out was not possible in Claude Code
  2.1.293 with claude-agent-acp 0.89.1:
  - `settingSources: []` breaks the login (a live run on the Mac mini, 2026-11-02; commit
    `45b2a5a4`, `agent/loads.rs` `CLAUDE_SETTING_SOURCES`);
  - `disableAllHooks` drops Clauseo's guard hook too: when the merged settings set it, the CLI
    keeps only managed (policy) hooks, and the guard is in the flag settings;
  - the adapter refuses `hooks` in `_meta` (`acp-agent.js`, its option table marks `hooks:
    false`), so the guard cannot become a programmatic hook.
- **2026-11-05 (orchestrator; the row and the New matter dialog).** A failed check shows on the
  matter's row as `check_failed` (`FolderSupportKind::CheckFailed`) at the create, the import
  and each launch whose check failed; a missing folder is a failed check too, and the app shows
  the missing folder first. Only the row's value changes: Codex's refusal and forced ask still
  decide from the check's own answer. `HostQuery::FolderSupport { source }` takes the
  `MatterSource` the New matter dialog will send in `CreateMatter` and runs the same check of
  the place that create would use, before the matter exists: an existing folder; a new
  folder's parent, canonicalised as create does; the default location, or its nearest folder
  that exists before the first matter makes it. It answers `QueryResult::FolderSupport` with
  the row's values (`PickedFolderSupport::support`, left out for a fine folder: the wire has no
  `null`). First it refuses, with create's codes, what create refuses before its matter is
  made (`actor/matters/picked.rs`): for an existing folder the import's place check
  (`check_place`: drive root, home, system and data folders) and its registry check (another
  matter's folder), then its protected-tree check (`clauseo_matters::check_protected`: inside
  the data folder or the matters root), all run before anything is made; for a new folder
  or the default location nothing, since create's other checks need the new folder's name. A
  refused folder never reaches the check. An unknown ref is `command.invalid`, and the bridge
  takes only refs this run's picker issued, as for `CreateMatter`. All of it runs off the actor
  and waits 5 s (`PREPARE_WAIT`); a check that doesn't answer by then, or whose thread fails,
  answers `check_failed`, never an error: `agent.launch_failed` reads as a broken thread in the
  app. No log line names the folder (`actor/query/folder_support.rs`).

### The model across mode changes

- **2026-11-05 (measured by stage C on win-01, claude-agent-acp 0.89.1; private
  `measurements/folder-rights/live-checks/mode-model.md`, PR #385).** With Haiku chosen,
  entering plan mode moves the session to Sonnet, and leaving plan moves it back to Haiku. This
  happens whichever way the mode is set: `session/set_mode plan` or `default`,
  `SetConfigOption` for the `mode` option, and forced ask's switch back to `default` after a
  plan exit. The answer to the mode call doesn't show the move: `session/set_mode` answers
  `{}`, and `set_config_option` still lists `model: haiku`. The `config_option_update`
  notifications do show it, and the next prompt runs on the model they name.
- **Decision (2026-11-05, orchestrator).** The host lets Claude Code pick its plan-mode model
  and doesn't set the lawyer's model back after a mode change. The thread records the model
  in use, because the host logs each `config_option_update` as `ConfigOptionsReplaced`
  (`actor/turn/updates.rs`). The app's composer has no model picker yet. The model option is
  received but not shown, so a lawyer can't choose Haiku in Clauseo today. When a model picker
  is built, a provisional line goes beside it for Rohan's batch: "Claude may use a stronger
  model while it plans."
- **Rejected:** holding the chosen model in every mode by watching `config_option_update` and
  setting the model back. Plans would run on the weaker model, and setting it back could fight
  the adapter if the adapter re-applies its plan model.

### Deviations from the brief (PR-A)

- **The skills API.** The brief's `write_skills(stetdoc, claude_dir, codex_home)` method is
  split, as the host thread asked: `ClauseoProfile::skill_files(kind)` (pure, on the actor) and
  the free function `write_skills(&SkillWrite)` (off the actor). `with_skills` returns the
  profile and the skills it refused, not a `Result`.
- **`ClaudeOptions` gained a `settings` field** (private, with `with_setting`), for
  `disableSkillShellExecution`.
- **`disableSkillShellExecution` comes from R21**, not R22, and goes in every Claude session.
- **Dependencies beyond the brief's list:** `process` (to start stetdoc), `futures`,
  `thiserror` and `tracing`, and `sha2` (R22's). `serde` is not used.
- **`ProfilePaths::new`** takes the app's own executable and returns a `Result`, and
  `path_prepend()` returns `Option<&Path>`.
- **`{browser_dir}`** replaces `{session_dir}` and the `/browser` the profile joined to it.
- **The tools text changed, as a fix:** `{work_folder}` is in backticks, and the agent is
  told to quote that path in shell commands, since it may hold spaces ("Application Support"
  on macOS). The brief's text had it bare.
- **`bypass_hook_trust`** is reserved in `CODEX_CONFIG` beside the brief's list.
- **Codex gets no skills** (fallback (b), above).
- **JSON key order:** the fragments and the manifest are written in sorted key order, since
  the workspace build turns on serde_json's `preserve_order`.
- **`AGENTS.md` has no map line** for `crates/profile`: the file is at its enforced limit.

### Codex's data-folder deny

Every Codex session's `CODEX_CONFIG` carries a permission profile, `clauseo`. It is chosen with
`default_permissions` and extends the built-in `:workspace`. Its `filesystem` table denies what
of the data folder only Clauseo uses (`agent::codex_fence::rules`), scoped under the data folder
as a literal path. That covers the logs, settings, updates, the store's and the search index's
databases (`search.sqlite*` holds every matter's prompts and answers), skill pack sources, Claude's
whole folder, and in Codex's home its login, `sessions/`, `history.jsonl` and its databases
(`state_5.sqlite*` and its siblings). It denies `run`, `agent-state` and `run.old-*` whole, so
a session or matter made after the launch is denied too. The paths come from `Locations` where it
names them, and a guard test checks each `Locations` path is denied or allowed for a stated reason.

The session's own run folder and the matter's notes are reopened two ways, both deeper than the
`run` and `agent-state` denies. The profile names them `write`: once a profile is chosen, Codex
no longer takes `CODEX_CONFIG`'s `sandbox_workspace_write.writable_roots` (stage B, live, macOS).
And the host sends them as ACP `additionalDirectories` on `session/new`, `session/load` and
`session/resume` (`profile_loads::codex_directories`). codex-acp 2.2.2 makes each turn's
`workspaceWrite` roots from those (`dist/index.js:35598-35612, 35878-35885`). It also marks them
trusted projects (`:35452`), which is inert: Codex looks trust up only for the cwd, its project
root and its repo root (`codex-rs/config/src/config_toml.rs:907-921`,
`codex-rs/config/src/loader/mod.rs:1048-1090`). And it adds `<root>/.agents/skills` of each as a
skill root (`:35506-35514`); Clauseo writes no `.agents` in either folder. The run folder is
writable whole, so the agent can write this session's pack plugin (`<run>/skill-packs`), which
0040 accepts. It relies on these codex `rust-v0.162.1` files and lines:

- the profile keys: `codex-rs/config/src/permissions_toml.rs:113-118, 224-244`; a scoped base read
  literally: `codex-rs/core/src/config/permissions.rs:572-623, 674-705`;
- a session-flags `default_permissions` choosing profiles: `codex-rs/core/src/config/mod.rs:2594-2615`;
- codex-acp's per-turn policy passed through (`codex-rs/app-server/src/request_processors/turn_processor.rs:846, 893, 918`)
  and rebuilt keeping the profile's deny entries: `codex-rs/core/src/session/session.rs:491-507`,
  `codex-rs/protocol/src/permissions.rs:709-731`;
- the deepest rule winning, deny winning a tie: `codex-rs/protocol/src/permissions.rs:1043-1048`;
- Seatbelt and bubblewrap enforcing it on every sandboxed command:
  `codex-rs/sandboxing/src/seatbelt.rs:899-900, 1045-1046`, `codex-rs/linux-sandbox/src/bwrap.rs:490-511, 1353-1362`;
  bubblewrap binding a writable root over a masked denied folder (`bwrap.rs:615-659, 1370-1398`);
  Seatbelt reading every writable root (`permissions.rs:1466-1476`) with a deny as an excluded
  subpath (`seatbelt.rs:966-981, 1007-1020`).

Why not deny the whole data folder. Bubblewrap reopens only a writable root under a denied folder
(`bwrap.rs:608-660`); a read-only root is bound before the masks and stays hidden
(`bwrap.rs:536-584`). The shell must read, but never write, Codex's install
(`codex-rs/arg0/src/lib.rs:329`: `apply_patch` re-runs the Codex binary in the sandbox), Codex's
home, and the built-in skills Codex's index names under `profile/claude-plugin/skills`. So
`profile` and `agents/codex-acp` stay readable: `profile` is the same for every matter. Codex
itself keeps cross-matter records: one Codex home per agent holds every matter's transcripts and
databases. The deny binds only sandboxed processes, not Codex's own process
(`codex-rs/state/src/sqlite.rs:34-39, 61-63`). A home per matter is a later design question.

Gaps: a data folder whose path holds a glob character loses the `run.old-*` deny, since a scoped
glob reads its base as glob text. The other rules are literal. A data folder that isn't UTF-8
already fails the launch (`Folders::of`). Windows gets no profile: Clauseo runs Codex there in the
unelevated sandbox, which refuses to run any sandboxed command when a profile denies reads
(`codex-rs/sandboxing/src/windows.rs:111-131`), until the elevated sandbox (S15). For what the
fence is and isn't (tier C has no wall between matters), see the data-folder fence section of
this record and firm-it §8.

## Consequences

- R22 adds its text with `Instructions::with_part` and its skills with `SkillSource::builtin`
  and `ClauseoProfile::with_skills`, beside stetdoc's, and reuses `test_support::tree_text`
  for its snapshots. R22's Codex skills wait on the same key as R21's.
- The host depends on `profile` for `placeholders::fill` and the token constants (PR-B adds
  `profile` to the host's row in `ARCHITECTURE.md`); there is no second copy.
- U13 ships stetdoc in a `stetdoc/` folder of its own, and only that folder goes on the
  agents' `PATH`: it must never be the folder holding the other sidecars, or each of them
  becomes a bare command for the agent.
- The Claude plugin's shape (`plugins: [{type: "local", path}]` and
  `.claude-plugin/plugin.json`) is inferred, not run; G11 runs it.

## Status

proposed

## Date

2026-11-02
