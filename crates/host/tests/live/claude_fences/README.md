# The fence proof: Clauseo's fences under Claude Code `auto` mode (R17 PR-D2d)

Runs the pinned Claude Code (claude-agent-acp 0.89.1, Agent SDK 0.3.293, CLI 2.1.293) through
ACP with the `_meta` Clauseo really generates, against a local fake Anthropic API, and records
whether each fence holds in `auto`, `acceptEdits` and `default`. Nothing leaves 127.0.0.1.

## Files

- `run.mjs`: the one command. Dumps the meta (unless `FENCE_META` is given), runs every group,
  writes `$FENCE_WORK/out/`.
- `fake-api.mjs`: fake Messages API (SSE when `stream: true`). Scripted main-loop turns, scripted
  auto-mode classifier verdicts, optional HTTP 400 on the `afk-mode-2026-01-31` beta.
- `proxy.mjs`: recording HTTP proxy standing in for Clauseo's egress proxy. Forwards only to the
  fake API, answers 403 to everything else.
- `acp-driver.mjs`: ACP client (the installed `@agentclientprotocol/sdk`). Advertises Clauseo's
  client capabilities and answers permission requests by a per-scenario policy.
- `guard-wrapper.sh` is generated into `$FENCE_WORK`. It logs each pre-write guard run, then runs
  the real guard.
- The meta comes from the ignored Rust test `fence_meta_dump` in
  `crates/host/src/agent/loads/fence_meta_dump.rs`.

Run by hand, not in CI: it needs node and the pinned npm packages, which CI does not have. Run
it again whenever the adapter pins change (`crates/limits/tests/live/package.json`, the one home
of the pins) or a fence changes, and record the result in decision record 0051.

## Environment

| variable | meaning |
|---|---|
| `FENCE_PKG` | `node_modules` folder holding the pinned packages (`@agentclientprotocol/claude-agent-acp`, `@agentclientprotocol/sdk`, `@anthropic-ai/claude-agent-sdk*`) |
| `FENCE_GUARD_BIN` | Clauseo's guard program: the `clauseo-desktop` binary (`cargo build -p clauseo-desktop --bin clauseo-desktop`), run as `--pre-write-guard --matter <folder>` |
| `FENCE_WORK` | scratch folder. Gets `fixture/` (data and matter folders), `guard-wrapper.sh`, `meta.json`, `config/` and `out/`. Wiped piece by piece on each run |
| `FENCE_REPO` | the Clauseo checkout, used to run `cargo test -p clauseo-host --lib fence_meta_dump -- --ignored` (not needed with `FENCE_META`) |
| `FENCE_META` | optional: an already dumped meta. Its `inputs.guard` must be `$FENCE_WORK/guard-wrapper.sh` |
| `FENCE_ONLY` | optional: comma list of groups, from `auto,acceptEdits,default,F5,B` (default all) |
| `FENCE_SUDO` | optional: `1` also tries B2 through `/etc/claude-code/managed-settings.json` (sudo), removed right after |

The node that runs `run.mjs` also runs the adapter (node 24 tested).

```sh
# the pinned adapter, in a scratch folder (or `npm ci` of crates/limits/tests/live)
(mkdir -p /tmp/fence-pkg && cd /tmp/fence-pkg && echo '{"private":true}' > package.json \
  && npm install --no-audit --no-fund @agentclientprotocol/claude-agent-acp@0.89.1)
cargo build -p clauseo-desktop --bin clauseo-desktop
export FENCE_PKG=/tmp/fence-pkg/node_modules
export FENCE_GUARD_BIN=$PWD/target/debug/clauseo-desktop
export FENCE_WORK=/tmp/fence-work
export FENCE_REPO=$PWD
nice -n 10 node crates/host/tests/live/claude_fences/run.mjs
```

The adapter gets a clean environment: `PATH`, `HOME`, `ANTHROPIC_BASE_URL` (the fake),
`ANTHROPIC_API_KEY` (dummy), `CLAUDE_CONFIG_DIR` (the matter's Claude state folder in the
fixture, as Clauseo sets it), `CLAUDE_CODE_AUTO_MODE_SERVER=0` (local classifier only),
`CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` and `DISABLE_AUTOUPDATER=1` (nothing to Anthropic),
and Clauseo's proxy variables (`HTTPS_PROXY`/`HTTP_PROXY`/lowercase forms set to
`http://clauseo:<token>@127.0.0.1:<proxy>`, `NO_PROXY` empty).

## Groups

- `auto`, `acceptEdits`, `default`: one session each, set to the mode with `session/set_mode`.
  Scenarios F1 (data-fence deny), F1x (shell forms outside the deny rules), F2 (pre-write
  guard), F3 (disallowed web tools), F4 (proxy env) and C (control Write), one prompt each.
- `F5`: auto's denial limits. 5 Bash calls the classifier denies (3 in a row), then 33 calls in a
  deny, deny, allow pattern (20 denials in all, never 3 in a row).
- `B`: refusal causes of `session/set_mode auto`: `disableAutoMode` in user settings (top level
  and under `permissions`), the `managedSettings` option, the afk beta refused by the API, and a
  model without auto (`ANTHROPIC_MODEL=claude-opus-4-5`).

## Output (`$FENCE_WORK/out/`)

- `summary.json`: per scenario, the tool calls (ACP status, output, what the model saw), the
  classifier calls, the permission requests (with the answer), mode updates, proxy hits, file
  hashes before and after, guard runs, and secrets found in tool results.
- `api.jsonl`, `proxy.jsonl`, `acp.jsonl`: every request or message, labelled by scenario.
- `guard/<n>/`: each guard run's input, arguments, exit code and stderr.
- `adapter-stderr.log`.
