# 0025: Codex in `acp` and `host`

## Context

Stage A ran Claude through claude-agent-acp. Stage B adds Codex through codex-acp 2.2.2 (with
`@openai/codex` 0.162.1), run as `node <install>/node_modules/@agentclientprotocol/codex-acp/dist/index.js`.
With no Clauseo sandbox every machine is tier C, so Codex must run in its ask mode,
`workspace-write`. Its login lives in a home Clauseo owns, never the user's `~/.codex` (#160).
codex-acp marks every session root `trusted` (`dist/index.js:35450-35455`), so a matter folder's
`.codex/` would load as project config (#184). Choosing Claude or Codex for a new thread is
the window's part; after U5 the window does not offer that choice in R15.

What codex-acp 2.2.2 does, read from its source (`dist/index.js`): `initialize` (`:42416-42501`)
answers `loadSession: true` and `sessionCapabilities` `resume`, `list`, `close`, `delete`, `fork`,
`additionalDirectories` and `subagents`; `promptCapabilities` `embeddedContext` and `image`. It
reads `CODEX_CONFIG` with `JSON.parse` (`:45657-45660`) and sends it, less `hooks`, as the
`config` of each thread start, resume and fork; `hooks` becomes one `-c hooks=…` at app-server
start (`:45590-45601`). `INITIAL_AGENT_MODE` sets the first mode (`:34362-34367`). Its
checklist plan (`turn/plan/updated`) goes as a standard `plan` update with every step
(`:31712-31723`), whatever the client advertises. A proposed plan (a plan item's text) goes as a
markdown `plan_update` to a client that advertises `plan` (`:29738-29748`, `:39794`), and as
agent message text to one that does not (`:29655-29658`, `:44519-44523`). It never sends item
`plan_update`s or `plan_removed`.

## Decision

- **No window change in R15.** U5 replaced R12's window, so R15 adds no UI: the window offering
  Claude or Codex for a new thread waits for U6, and the brief's done-when 3 stays open until
  then. The agent id `claude` becomes `claude-acp`, the registry id `AgentKind::of` knows,
  across the room, its testing hosts and e2e.
- **`AgentKind::of(&AgentId)`** (S13 added it in `protocol`'s `limits.rs`) is the one place an id
  becomes a kind: `claude-acp` is Claude, `codex-acp` is Codex, any other id has none.
- **`HostQuery::ListAgents`** answers `QueryResult::Agents(Vec<AgentSummary>)`, one
  `AgentSummary { id, kind }` per settings agent whose id is a valid `AgentId`, sorted by id.
  Additive (0002, "Types outside the log"); the bridge allows it.
- **What the mode allows.** Rohan decided to keep `workspace-write`. codex-acp describes it as
  "edit workspace files; ask before writing outside the workspace" (`dist/index.js:34277-34281`),
  and G8a (clauseo-acp-private #255) saw it create, change and delete matter files with no
  permission request. So Codex edits files inside the matter folder without asking, and asks
  before writing outside it or escalating; Undo per turn is the lawyer's way back. No screen may
  promise per-edit approval for Codex. The Word-open guard still applies to Codex's writes,
  through the hooks object in `CODEX_CONFIG` (clauseo-acp #152).
- **The host owns the mode.** A Codex agent is launched with `INITIAL_AGENT_MODE=workspace-write`
  over any settings value. After `session/new`, `session/load` or `session/resume` the host reads
  the session's modes: none, or none named `workspace-write`, fails the turn with
  `agent.protocol_error` and no prompt is sent; a current mode other than `workspace-write` gets
  `session/set_mode` before the first prompt. Doing both is safe whichever route G8a shows to
  win. Updates the agent sends meanwhile are held until the attach commit.
- **`AcpClient::resume_session`** sends ACP v1's stable `session/resume`; nothing is replayed.
  The host loads when `loadSession` is set, else resumes when `sessionCapabilities.resume` is
  set, else starts a new session. Found in codex-acp 2.2.2's source: it answers
  `loadSession: true` as well as `sessionCapabilities.resume` (`dist/index.js:42416-42501`), so a
  real Codex thread reopens with `session/load`, which replays (`:43068`); the resume path
  serves an agent that offers resume alone.
- **Plans.** `acp` keeps, per session, the id of the item plan updated last (`state.rs`); its
  `plan_update` shows as `AgentUpdate::Plan(entries)` and a `plan_removed` of it as an empty
  `Plan`. A removal of any other plan shows nothing, so no other plan's entries are kept. A whole
  `plan` replaces the shown item plan. Markdown and file plans stay
  `ProtocolWarning::UnknownUpdate` until a protocol type exists. Codex's checklist plans show:
  they come as standard `plan` updates (scenario `codex-plan`). Clauseo keeps advertising
  `plan`, so Codex's markdown proposed plans arrive as `plan_update` and show nothing yet; not
  advertising it would turn them into answer text instead.
- **`Locations::agent_home(agent)`** is `<data folder>/agents/<agent>/home`, never under the temp
  folder. The host makes it at launch (0700 on Unix) and passes it as `CODEX_HOME`; the limits
  reader reads the same home. It takes the id's text: `settings` does not depend on `protocol`,
  so it debug-asserts that the text is `[a-z0-9-]+` and the home cannot land outside
  `<data folder>/agents`.
- **The home is made without following links.** `make_home` walks `agents`, `agents/<id>` and
  `home` below the data folder one level at a time: each that exists must be a real folder (a
  link or junction is refused, never followed, so nothing is made where it points), and each
  missing one is made, 0700 on Unix. The data folder itself may be a link: `CLAUSEO_DATA_DIR` may
  name one, and the store lives there too.
- **The matter folder may not overlap the home.** At each Codex launch, once the home exists, the
  host compares the matter folder and the home, both resolved through links (without case on
  macOS and Windows): a matter folder that is the home, holds it or sits inside it is refused with
  `matter.folder_refused`. An agent working there could read the login, write rule files (`.rules`)
  that Codex loads from the home, or edit `config.toml` before app-server reads it. Import
  refuses only folders that overlap the matters root, so the data folder or its `agents` folder
  can still be a matter; the launch check closes that.
- **Dropped variables.** A Codex agent never gets `CODEX_PATH` (it swaps the app-server binary),
  `APP_SERVER_LOGS` (codex-acp would log the whole `CODEX_CONFIG`, hooks included, to a file),
  `DEFAULT_AUTH_REQUEST` or `MODEL_PROVIDER`, whether inherited, set in the settings or given by
  the profile hook, in any case. `MODEL_PROVIDER` goes too: the `config.toml` Clauseo writes in
  the home defines no provider, and sign-in is Codex's ChatGPT login.
- **A Codex agent's launch env**, set over any settings or profile value of the same name:
  `INITIAL_AGENT_MODE=workspace-write`, `CODEX_HOME=<agent_home>`,
  `DISABLE_MCP_CONFIG_FILTERING=true`, and `CODEX_CONFIG`, a JSON object holding every one of
  `clauseo_limits::CODEX_OVERRIDES` as nested keys: apps, plugins, skill MCP installs, cloud
  skills and bundled skills off, and `skills.include_instructions=false`. The last two joined the
  list on #160's reasoning, that Clauseo, not Codex, decides what a session loads. G8a found that
  app-server lists the user's own skills from `~/.agents/skills`, which it finds through `HOME`,
  not `CODEX_HOME`, and 5 system skills even with bundled skills off; with
  `skills.include_instructions=false` the model gets no skill text (openai/codex
  `rust-v0.162.1`, `ext/skills/src/extension.rs:196-198` and `:417`), though app-server still
  lists the skills. The only
  keys a settings `CODEX_CONFIG` contributes are `model` and `model_reasoning_effort`, so a
  by-hand check can pick the cheapest model while no settings value can undo a switch.
- **The home's `config.toml`.** codex-acp gives `CODEX_CONFIG` only to the session and starts
  the app-server with `-c` for hooks alone (`dist/index.js:45590-45601`), so app-server starts
  from the home's own `config.toml`, where plugins are on by default. G8a saw every start sync a
  plugin cache into `CODEX_HOME/plugins` (176 folders, 57 skills; openai/codex `rust-v0.162.1`
  `core-plugins/src/manager.rs:2814-2824`) whose skills reached the command list, and #203 saw
  that sync fetch `github.com/openai/plugins`. So at each launch, after making the home, the host
  writes `<agent_home>/config.toml` with every `CODEX_OVERRIDES` switch as a TOML dotted key,
  under a header saying Clauseo manages it and edits are replaced: atomically (temp file and
  rename, the temp name holding the process id and a count, so two hosts sharing a data folder
  never collide), 0600 on Unix, over whatever is there; a `config.toml` that already holds
  exactly those bytes, as a plain 0600 file, is left alone. It reads and touches nothing else in
  the home
  (`auth.json` is Codex's). `CODEX_CONFIG` stays: it reaches the session, the file reaches
  start-up. The hooks table never goes in the file: the guard's trust entry is keyed by the
  session-flags source, so a hook read from the file would be keyed by its path and silently
  not run.
- **Every agent** is launched without the host's inherited `GIT_*` variables.
- **The hooks seam.** `actor/turn/agents.rs`'s `codex_hooks` gives the `hooks` object that goes
  into `CODEX_CONFIG`, from the guard PR's `codex_hook_config()` (clauseo-acp #152, which adds it
  to the host's agent loads) once it merges; until then none. It is read once per launch, and the
  agent records whether an object went in; with one, the host calls codex-acp's
  `_codex/hooks/list` once after the session starts and requires every listed session-flags hook
  to be `trustStatus: "trusted"`: Codex skips an untrusted hook silently, so a drifted hash would
  leave the guard off. Otherwise it logs each hook's trust status and the hook count (not the
  answer, which holds paths) and fails the session start with `agent.launch_failed`, and no
  prompt is sent. `agent.launch_failed` keeps one host code for every launch failure; the app
  makes its words kind-aware from the thread's agent.
- **#184.** A Codex agent is refused, before it is launched, in a matter folder (as resolved
  through links) holding a `.codex` entry of any kind, with its own code `matter.codex_folder`
  (additive; its words tell the lawyer to rename or remove the matter's `.codex` folder to use
  Codex there). The check runs again before each prompt and before each `session/new`,
  `session/resume` and `session/load`: codex-acp restarts a crashed app-server and resumes the
  session itself (`dist/index.js:37755-37772`), so a `.codex` that appears mid-session would
  load unchecked. On a hit the call is not sent, the agent is stopped and the turn ends failed
  with `matter.codex_folder`. A stat that fails for any reason but not-found counts as present.
  It is lifted once codex-acp gets an option not to trust the session root
  (agentclientprotocol/codex-acp#690) or the direct Codex driver exists.
- **Off the actor.** The folder resolution, the `.codex` stats, the home's checks, `make_home`
  and the `config.toml` write never run on the actor thread. As for matter folders' states
  (`crates/host/src/actor/folder_state.rs`), each runs on a named thread (`clauseo-codex-fs`,
  `crates/host/src/agent/codex/prepare.rs`) whose answer comes back over a channel, with a 5 s
  deadline on the host clock; a stat that hangs strands at most that thread, and the launch or
  the turn fails closed with `agent.launch_failed`. A Codex launch has two phases: its
  preparation runs while the work that waits for the agent is held on the thread's state, then
  the launch runs on the actor with what it found. Other agents launch as before. Every error
  the lawyer may see says what failed in words; paths and io errors go to the log only.
- **`agent::codex::session_folder`** resolves the matter folder through links. It is a private
  stand-in until #231's `agent::session_folder` lands, which replaces it.

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
