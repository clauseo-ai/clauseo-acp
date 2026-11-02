# 0025: Codex in `acp` and `host`

## Context

Stage A ran Claude through claude-agent-acp. Stage B adds Codex through codex-acp 2.2.2 (with
`@openai/codex` 0.162.1), run as `node <install>/node_modules/@agentclientprotocol/codex-acp/dist/index.js`.
With no Clauseo sandbox every machine is tier C, so Codex must run in its ask mode,
`workspace-write`. Its login lives in a home Clauseo owns, never the user's `~/.codex` (#160).
codex-acp marks every session root `trusted` (`dist/index.js:35450-35455`), so a matter folder's
`.codex/` would load as project config (#184). The window needs to offer Claude or Codex for a
new thread.

What codex-acp 2.2.2 does, read from its source (`dist/index.js`): `initialize` (`:42416-42501`)
answers `loadSession: true` and `sessionCapabilities` `resume`, `list`, `close`, `delete`, `fork`,
`additionalDirectories` and `subagents`; `promptCapabilities` `embeddedContext` and `image`. It
reads `CODEX_CONFIG` with `JSON.parse` (`:45657-45660`) and sends it, less `hooks`, as the
`config` of each thread start, resume and fork; `hooks` becomes one `-c hooks=…` at app-server
start (`:45590-45601`). `INITIAL_AGENT_MODE` sets the first mode (`:34362-34367`). It sends
`plan` and markdown `plan_update`, never item plans or `plan_removed`.

## Decision

- **`AgentKind::of(&AgentId)`** (S13 added it in `protocol`'s `limits.rs`) is the one place an id
  becomes a kind: `claude-acp` is Claude, `codex-acp` is Codex, any other id has none.
- **`HostQuery::ListAgents`** answers `QueryResult::Agents(Vec<AgentSummary>)`, one
  `AgentSummary { id, kind }` per settings agent whose id is a valid `AgentId`, sorted by id.
  Additive (0002, "Types outside the log"); the bridge allows it.
- **The host owns the mode.** A Codex agent is launched with `INITIAL_AGENT_MODE=workspace-write`
  over any settings value. After `session/new`, `session/load` or `session/resume` the host reads
  the session's modes: none, or none named `workspace-write`, fails the turn with
  `agent.protocol_error` and no prompt is sent; a current mode other than `workspace-write` gets
  `session/set_mode` before the first prompt. Doing both is safe whichever route G8a shows to
  win. Updates the agent sends meanwhile are held until the attach commit.
- **`AcpClient::resume_session`** sends ACP v1's stable `session/resume`; nothing is replayed.
  The host loads when `loadSession` is set, else resumes when `sessionCapabilities.resume` is
  set, else starts a new session. codex-acp 2.2.2 offers both load and resume, so a Codex thread
  reopens with `session/load`, which replays.
- **Plans.** `acp` keeps, per session, the id of the item plan updated last (`state.rs`); its
  `plan_update` shows as `AgentUpdate::Plan(entries)` and a `plan_removed` of it as an empty
  `Plan`. A removal of any other plan shows nothing, so no other plan's entries are kept. A whole
  `plan` replaces the shown item plan. Markdown and file plans stay
  `ProtocolWarning::UnknownUpdate` until a protocol type exists.
- **`Locations::agent_home(agent)`** is `<data folder>/agents/<agent>/home`, never under the temp
  folder. The host makes it at launch (0700 on Unix) and passes it as `CODEX_HOME`; the limits
  reader reads the same home. It takes the id's text: `settings` does not depend on `protocol`.
- **A Codex agent's launch env**, set over any settings or profile value of the same name:
  `INITIAL_AGENT_MODE=workspace-write`, `CODEX_HOME=<agent_home>`,
  `DISABLE_MCP_CONFIG_FILTERING=true`, and `CODEX_CONFIG`, a JSON object holding every one of
  `clauseo_limits::CODEX_OVERRIDES` as nested keys (apps, plugins, skill MCP installs, cloud
  skills and bundled skills off; bundled skills joined the list on #160's reasoning). The only
  keys a settings `CODEX_CONFIG` contributes are `model` and `model_reasoning_effort`, so a
  by-hand check can pick the cheapest model while no settings value can undo a switch.
- **Every agent** is launched without the host's inherited `GIT_*` variables.
- **The hooks seam.** `actor/turn/agents.rs`'s `codex_hooks` gives the `hooks` object that goes
  into `CODEX_CONFIG`, from `agent::loads::codex_hook_config` once #162 part 2 lands; until then
  none. With an object, the host calls codex-acp's `_codex/hooks/list` once after the session
  starts and requires every listed session-flags hook to be `trustStatus: "trusted"`: Codex skips
  an untrusted hook silently, so a drifted hash would leave the guard off. Otherwise it logs an
  error and fails the session start with `agent.launch_failed`, and no prompt is sent.
- **#184.** A Codex agent is refused, before it is launched, in a matter folder (as resolved
  through links) holding a `.codex` entry of any kind, with `matter.folder_refused` and a plain
  message to rename or remove that folder. This covers new, loaded and resumed sessions alike.
  It is lifted once codex-acp gets an option not to trust the session root
  (agentclientprotocol/codex-acp#690) or the direct Codex driver exists.

## Consequences

- R16 passes the same `agent_home` to the limits reader. G8a's recordings replace the Codex
  scenarios written from source.
- A new key in `CODEX_OVERRIDES` reaches both the reader and the host; the host's test of the
  nested object fails until it is checked.
- Wiring #162's hooks object is one line in `codex_hooks`; its trust check is already in place.
- Clauseo advertises the `plan` capability, so codex-acp sends its markdown plans as
  `plan_update`, which shows nothing until markdown plans get a protocol type.
- Stage C moves Codex to its `agent` mode inside the sandbox; the mode rule changes then.

## Status

proposed

## Date

2026-11-02
