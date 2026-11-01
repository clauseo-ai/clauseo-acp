# 0014: limits, the plan usage readers and the limits model

## Context

The lawyer's agent plan has usage windows (Claude's 5 hours and week, Codex's primary and
secondary windows), and a turn can stop at one. Clauseo shows each signed-in agent's usage and
what happens at the limit, and it never reads a credential (`ARCHITECTURE.md`, Invariants). ACP
has no request for account state, so a snapshot outside a turn needs a separate reader, which runs
each vendor's own tool. During a turn, claude-agent-acp already forwards `_claude/rateLimit` on
`usage_update`. The new crate `limits` holds the readers and a pure model that merges both sources
and decides when to read. `host` wires it in later.

## Decision

**The crate row.** `limits` owns plan usage readers (the Claude usage helper and the Codex
reader), event-driven refresh, the merge of live `_claude/rateLimit` signals, and the limits model
and its view. May depend on: `protocol, process, settings, codex_appserver, serde, serde_json,
futures, tracing`. Never depends on: `tauri, tokio, keyring`. It has no `acp`, `store`, `host` or
`sandbox` dependency. The host turns `acp::LimitSignal::RateLimit` into `LiveSignal`, keeps
`AccountLimits` in `store`, and hands the cached values back at start. Readers launch through the
`process::Launcher` they are given. `protocol` gains `AgentKind::of(&AgentId)`: `claude-acp` is
Claude, `codex-acp` is Codex, and any other id has no kind and no limits. That is the one place
an id becomes a kind.

**The model.** `Limits` is pure: no I/O and no clock. `new(cached, agents, now)`, then
`input(Input, now) -> Vec<Action>`, `view(now)`, `next_deadline()`, `next_read_at(agent)` and
`at_limit(agent, now)`. Four shapes differ from the task brief's sketch. `host` (R16) passes the
first three through and calls the fourth:
- `ReadOutcome::Usage { limits, reached: Option<LimitStopReason> }`. `reached` carries Codex's
  `rateLimitReachedType` and `spendControlReached`, which `AccountLimits` has no field for. It is
  model state until the next good read, so a restart forgets it, as it forgets the rejected flag.
  Claude always gives `None`.
- `ReadOutcome::SignedOut`: the vendor's tool says no account is signed in (Codex's
  `account/read` with no account, in a home with no login). The model gives that agent a row with
  no account and no windows, whose view shows `MissingScope`'s freshness and so the sign-in-again
  action. It sends no `Action::Changed`, so nothing is stored. An earlier account's stored entry
  stays as it is, and the next good read with an account replaces the row. A fixed `signed-out`
  account would have been stored, and would come back at start beside the real one.
- `ProcessReaders::new(effective, codex_home, launcher, timer)`, from #160: Clauseo owns the
  Codex home (below).
- G16 adds one hidden, additive hook: `ProcessReaders::with_stdout_tap(StdoutTap)` copies each
  read's raw stdout to a callback, so its live tests can record the vendors' answers, redacted,
  under `target/g16/`. Without it the stdout passes as before; R16 never sets it. It exists only with
  the `live-tap` feature, which only `limits`' own dev-dependency turns on, so a crate that
  wants it must add the feature in a reviewed `Cargo.toml`. That holds per package: once the
  host depends on `limits`, workspace runs (`clippy --workspace --all-targets`,
  `nextest --workspace`) build `limits` with the tap on, while ci.yml's per-package
  `cargo clippy -p clauseo-desktop --all-targets` and release builds still catch a production
  caller.
- `at_limit(agent, now)`: a row whose `resets_at` is at or before `now` never counts, even when
  no deadline swept it because nobody watched. R16 asks at a turn stop, outside watching. A
  Codex `reached` plan window (any reason but credits and the spend cap) ends with its rows: once
  every shown row is past its reset at `now`, it gives `None` (#204). With no row it stands until
  the next read. Credits and the spend cap do not reset with time, so they stand.

The host keeps two contracts. It sends exactly one `ReadDone` per `Action::Read`, as
`Failed(TimedOut)` if it drops the read; a `ReadDone` the model did not ask for is ignored. And it
calls `next_deadline()` again after every `new` and `input`, and re-arms or cancels its one timer.
`Asking { asking: true }` means any permission or elicitation is pending in any session of that
agent; the host sends `false` only when none is left.

**Merge** (T3's rules). Windows upsert by id. A missing `resets_at` keeps the old one. A no-op is
ignored. A live signal never revives `NotApplicable`. A rejected live signal leaves the figure as
the agent sent it, and marks the window rejected. The flag drives the `AtLimit` tone and
`at_limit`. It clears at the next good read, at a later live `allowed` or `allowed_warning` for
that window, or when the window's `resets_at` passes. This changes the task brief's "until the next
good read" (review of #114, round 1), for Rohan to confirm: after `MethodMissing` no good read ever
comes, so the window would stay at the limit for good. A failed read keeps the last good numbers as
`Stale { ReadFailed }`, or gives `Unavailable { ReadFailed }` when there are none.
`NotApplicable` and `Unavailable { MissingScope }` survive a failed read. A good read
replaces everything, live updates that arrived during it included. A live signal for a window
no read has named is dropped, except Claude's `five_hour` and `seven_day`, so an `overage` event
before any snapshot is dropped. A rejected `five_hour` or `seven_day` signal with no figure adds
its row at 100%. That 100% is stored with the account, on purpose, although the agent never sent
it: it is what "rejected" means for a row no read has named, and the next good read replaces it. A missing usage method (`MethodMissing`, Claude only) gives
`Stale { ReadFailed }` or `Unavailable { ReadFailed }`, then `LiveOnly` once a live signal
arrives. So does a failed read with no earlier numbers (a timeout, a crash, a refused install):
live signals still reach the view, so a real limit shows. Only while the first read is in flight,
and for a signed-out agent, are live signals dropped. While no read has named the account, a live
signal changes the view only (`ViewChanged`), since `AccountLimits` needs an `AccountRef`. At
start, the cached entry with the newest checked time is each agent's, whatever order the store
gives.

**Refresh.** One read per agent at start, and cached values become `Stale { Aged }`
(`NotApplicable` stays). A focus, or a meter or the settings page opening, reads when the last read
started 5 minutes ago or more. A turn end reads after 10 minutes for Claude and 60 seconds for
Codex. `TurnEnded { read_now: true }` and `TryAgain` read at once. Every age counts from the start
of the agent's last read, whatever its outcome, so a failed read is not retried on every focus.
At most one read is in flight per account. None starts while that agent has a permission or
elicitation pending; one asked for then starts when the question is answered. While anyone
watches, `next_deadline()` is the earliest 15-minute stale mark or shown `resets_at`. A passed
`resets_at` marks its row `ResetPassed` (no percentage) and asks one read, but only when the last
read started before that reset, so a vendor that answers with a past reset time starts no loop.
No deadline exists while nobody watches. The model starts no retry of its own. Timeouts: 25 s for
Claude, 10 s for Codex.

**The view.** Tones: neutral below 75% used, near from 75% or on the agent's warning, at the
limit at 100% or when rejected. Never red. A row is stale 15 minutes after its last update. The
meter is the tightest window. The at-limit words are in `src/view/at_limit_words.rs`.

**The Claude helper.** `limits` embeds `helper/claude-usage.mjs` and writes it into
claude-agent-acp's install folder as `clauseo-claude-usage.mjs`. It writes only when the bytes
differ, through a temp file and a rename in `write.rs`. A bare
`import "@anthropic-ai/claude-agent-sdk"` then resolves to the adapter's own SDK (0.3.293 for
0.89.1). The helper calls `query()` with a prompt that never yields, `persistSession: false`,
`settingSources: []`, `allowedTools: []`, `mcpServers: {}`, `strictMcpConfig: true`, `hooks: {}`
and `settings: { disableAllHooks: true }`. It then calls `initializationResult()` and
`usage_EXPERIMENTAL_MAY_CHANGE_DO_NOT_RELY_ON_THIS_API_YET({ skipBehaviors: true })`, prints one
line, and aborts. It passes `pathToClaudeCodeExecutable` only when `CLAUDE_CODE_EXECUTABLE` is set
and not empty. That is the variable claude-agent-acp 0.89.1 reads for its own override
(`@agentclientprotocol/claude-agent-acp` 0.89.1, `dist/acp-agent.js:646-647` and `:7359`:
`process.env.CLAUDE_CODE_EXECUTABLE ?? await claudeCliPath()`). Without it, the SDK starts its own
native binary, as the adapter does by default.

**The helper's line.** Exactly one stdout line, exit 0:
`{"ok":true,"account":A,"usage":U}` or
`{"ok":false,"stage":"init"|"usage","missing":<bool>,"message":<at most 200 chars>}`. `A` keeps
only `email`, `subscriptionType` and `apiProvider`. `U` keeps only
`subscription_type`, `rate_limits_available` and `rate_limits`. Rust reads at most 1 MiB and
ignores unknown fields. `missing: true`, or an `ok:true` line whose usage does not fit the schema,
is `MethodMissing`: the source is unavailable, which is not an error. `missing: false` is a failed
read with the process's redacted stderr tail. The line's own `message` is not redacted, so it is
never shown or logged. Anything else is `Malformed`. A read ends when the reader exits
and its stdout and stderr close. Once the answer is in, the exit and the pipes get 1 s more, then the
tree is killed and the answer kept, so a reader that lingers, or a descendant that left the
process tree, cannot turn a good read into a timeout. A complete Codex answer counts whatever
the exit code.

**Mapping.** No subscription and no rate limits is pay as you go:
`PayAsYouGo { via }` by `apiProvider` (Anthropic, Bedrock, Vertex, Other), with `NotApplicable`
and no windows. A subscription without rate limits is `Unavailable { MissingScope }`. Otherwise
the windows are `five_hour` (session), `seven_day` (weekly), `seven_day_opus` and
`seven_day_sonnet` (model), `seven_day_oauth_apps` (other), and `model_scoped[]` (model). A single
`model_scoped` entry is named `seven_day_overage_included`, where the live signal of that
`rateLimitType` lands. Two or more are `model_scoped:<n>`. `extra_usage` gives
`ExtraUsage { enabled }`. The plan names are "Claude Pro", "Claude Max", "Claude Team" and
"Claude Enterprise", and an unknown plan gives none. `AccountRef` is the reported email when it
looks like one. Otherwise it is a fixed ref: `api-key`, `bedrock`, `vertex`, or `plan`. It is
never a key or a token.

**Codex.** The reader runs `node <codex.js> app-server` and talks through `codex_appserver`'s
client: `initialize` (then `initialized`), `account/read`, and, for a ChatGPT account only,
`account/rateLimits/read { excludeResetCreditDetails: true }`. Then it drops the client, which
closes stdin. It runs `codex.js` with `-c` overrides before `app-server`, one per category #160
turns off: `features.apps=false`, `features.plugins=false`,
`features.skill_mcp_dependency_install=false` and `cloud.skills.enabled=false` (#203,
`CODEX_OVERRIDES`). #160 names categories, not keys, and the host sends none yet: these keys
come from the `rust-v0.162.1` source, and R15's `CODEX_CONFIG` adopts them from the constant. G16 probed
`@openai/codex` 0.162.1 with a scratch home and no login: before any thread, nothing the home's
config names ran (an `[mcp_servers]` entry, a plugin's MCP server and hook, config hooks, a
skill's MCP dependency), but with plugins on by default app-server ran `git ls-remote` and
`git fetch` of `github.com/openai/plugins` into the home, even with an empty home.
`features.plugins=false` stops that; with all four there was no child process and no socket.
App-server still writes its own state files into the home and unpacks its bundled skills into
`skills/.system`. `codex.js` is codex-acp's own `node_modules/@openai/codex/bin/codex.js`, else
the install's. No account is `SignedOut`. An API key is `PayAsYouGo { via: OpenAi }` with ref
`api-key`, and Bedrock is `PayAsYouGo { via: Bedrock }` with ref `bedrock`, both
`NotApplicable`. For ChatGPT, only the snapshot whose `limitId` is `codex` (from
`rateLimitsByLimitId`, else `rateLimits` when its `limitId` is `codex` or absent) sets the
windows, so a model bucket never overwrites them; other buckets are ignored for now. `primary`
and `secondary` are positions and keep those ids. `windowDurationMins` gives the kind: 300 is the
session, 10080 the week, 28 to 31 days "Monthly limit", and Free or Go with no duration one
monthly window. Credits give `Credits { has_credits }`. `rateLimitReachedType` gives `reached`:
`workspace_*_credits_depleted` is `WorkspaceCredits`; `workspace_*_usage_limit_reached`, or
`spendControlReached: true` with any type that does not name credits, is `WorkspaceSpendCap`;
any other value is `PlanWindow`. The plan names are "ChatGPT Plus", "ChatGPT Pro" and so on, made
from `planType`, and an unknown type gives none.

**Paths and environment.** The paths come from settings `agents.claude-acp` and
`agents.codex-acp`. `command` is the absolute path of `node`. `args[0]` must be an absolute path
to a regular file, with no `.` or `..` part. The install folder is the parent of the nearest
`node_modules` above it. No component from the install folder down to the file may be a symlink,
and the helper is never written through one. Before the helper is written or run,
`<install>/node_modules/@anthropic-ai/claude-agent-sdk/package.json` must be a regular file on the
same walk. Otherwise Node would climb to a parent `node_modules` and run some other SDK. A global
npm install, where the SDK sits nested under the adapter, is refused for the same reason. The install folder and its parents may be symlinks.
In practice an npm-installed adapter passes, since npm puts real files under `node_modules`. A
pnpm or linked install puts symlinks on that path, so it reads as "no limits" until stage D's
`agents` owns the install. A reader's environment is the inherited one, minus every name the
agent's settings `env` names (ignoring ASCII case), then the settings variables, as
`crates/host/src/agent/spec.rs` builds an agent's. The inherited secrets are in the spec, so
`process` redacts them from the stderr tail. The Codex reader then sets `CODEX_HOME` to the
Clauseo-owned home, after removing every other spelling of that name. codex-acp merges the user's
`~/.codex/config.toml` (MCP servers, plugins, hooks, rules) unless `CODEX_HOME` points elsewhere,
so Clauseo owns one and Codex signs in separately inside Clauseo (#160). The reader never reads
inside that home, never creates it and never defaults to `~/.codex`. A missing home is
`SignedOut` without a launch, since Codex refuses to start without one and no login can be
there. A home that is a symlink or not a folder is refused. The working folder is `<data_dir>/limits/run`, made if missing and refused when it
or `limits` is a symlink. Nothing in Clauseo writes there. An agent with no settings entry has no limits.

## Consequences

The model is a pure function of its inputs and `now`, tested offline. Every rule above has a test
in `crates/limits/tests/model/`. The readers are tested against `fake_agent` and synthetic
fixtures. G16 records real responses and replaces the fixtures. Where a recording differs, G16's
code PR fixes this crate's schema. The helper's first real run, and whether the live signal for a
per-model window really uses `seven_day_overage_included`, wait for G16. Stage D's `agents` takes
over the helper's write and owns the install, which lifts the symlink limit. The sandbox's account
probe policy joins in stage C. A change to the line contract changes the helper, the Rust schema
and this record together.

## Status

proposed

## Date

2026-11-02
