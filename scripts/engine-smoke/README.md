# engine-smoke

Starts the pinned claude-agent-acp and codex-acp with the machine's own sign-in, reads the
models each one lists and sends each **one** prompt. It is run by hand before a pull request that
changes `crates/limits/tests/live/package.json` and `package-lock.json` (the engine pins) is
merged, on the machines that have real signed-in accounts. Nothing in CI runs it: each run is real
usage on a real account, so it sends exactly one prompt per agent and no more.

```sh
# The pair from the pull request, installed as the app installs it: npm runs no package scripts,
# and still installs each platform's optional package (the Claude and Codex programs).
cp crates/limits/tests/live/package.json crates/limits/tests/live/package-lock.json "$DIR"/
(cd "$DIR" && npm ci --ignore-scripts --no-audit --no-fund)
node scripts/engine-smoke/smoke.mjs "$DIR/node_modules" [--agent claude|codex|both] \
  [--expect-model [claude:|codex:]<id>]... [--timeout 180]
```

Node 24 or later, no npm dependencies. For each agent it:

1. Starts the adapter's `bin` (else `main`) from its `package.json` with the current Node.
2. Uses the machine's own sign-in: the environment passes through unchanged. `SMOKE_CLAUDE_CONFIG_DIR`
   sets `CLAUDE_CONFIG_DIR` and `SMOKE_CODEX_HOME` sets `CODEX_HOME`, for an account that lives
   somewhere other than the default (`~/.claude`, `~/.codex`).
3. Makes a scratch folder under the temp folder holding one small text file, the session's cwd.
4. Sends `initialize` (the `adapter_version` is its `agentInfo` name and version), then
   `session/new` with `mcpServers: []`, and reads the models, the current model and the current
   effort. Both adapters report them in the `session/new` result's `configOptions`: the option with
   `category: "model"` (id `model`) holds the model ids and `currentValue`; the option with
   `category: "thought_level"` holds the effort (id `effort` in claude-agent-acp, `reasoning_effort`
   in codex-acp). codex-acp also sends `models` (`availableModels[].modelId` such as
   `gpt-5.5[high]`, and `currentModelId` in the same form); that is read when `configOptions` are
   missing, and its ids also count for `--expect-model`. A `config_option_update` that arrives
   before the prompt replaces the options.
5. Checks that every `--expect-model` id is listed. `claude:<id>` or `codex:<id>` checks the id
   for that agent only; an id without the prefix is checked for both. claude-agent-acp lists
   aliases such as `default`, `opus`, `sonnet`; codex-acp lists ids such as `gpt-5.5`, so the
   usual form is `--expect-model claude:opus --expect-model codex:gpt-5.5`.
6. Sends one `session/prompt`: `Reply with the single word OK.` The text of the
   `agent_message_chunk` updates is the reply. Every permission request is refused (`reject_once`,
   else `reject_always`, else `cancelled`); other requests from the adapter get "not supported".
   The prompt passes when the stop reason is `end_turn` and the trimmed reply contains `OK`.
   Then, where the adapter offers `session/delete`, the session is deleted (best effort, so the
   smoke's conversation does not stay in the account's history; its failure fails nothing).
7. Stops the adapter and the processes it started and removes the scratch folder, only after they
   have exited: SIGTERM, then SIGKILL after 10 s, then the same for leftover children (the process
   tree is read with `ps` before anything is signalled). On Windows, where a signal is a hard kill,
   stdin is closed first and the adapter gets the same 10 s; the tree comes from `Win32_Process`
   through PowerShell. The same happens on a failure, on the timeout and on Ctrl-C. `KEEP=1` keeps
   the folder. A process still alive after all that fails the agent.

`--timeout` (seconds, default 180) is each agent's whole budget; every request gets what is left
of it, so nothing hangs.

## Output

One JSON line per agent on stdout, then `PASS` or `FAIL`. The exit code is 0 only when every agent
passed (1 otherwise, 2 for a usage error, 130 when interrupted).

```json
{"agent":"codex","adapter_version":"@agentclientprotocol/codex-acp 2.2.2","models":["gpt-5.5"],
 "current_model":"gpt-5.5","current_effort":"medium","expected_missing":[],"prompt_ok":true,
 "stop_reason":"end_turn","seconds":6.1,"ok":true,"reason":null,"reply":"OK"}
```

The fields after `seconds` are extra: `ok`, `reason` (why it failed), `reply` (the first 200
characters), `auth_status` (the kind in the adapter's last `_auth/status_update`),
`session_deleted`, `permissions_refused`, `mock`. The reasons start with a code:

- `auth_required`: the adapter answered with ACP's auth error (-32000). Sign in on that machine, or
  point `SMOKE_CLAUDE_CONFIG_DIR` / `SMOKE_CODEX_HOME` at the account. codex-acp refuses
  `session/new` without a sign-in; claude-agent-acp opens the session (so its models are listed,
  with `auth_status` `none`) and refuses the prompt.
- `timeout`, `adapter_exited`, `adapter_failed`, `adapter_missing`, `acp_error`, `no_models`,
  `prompt` (wrong stop reason or no `OK`), `expected models missing`, `processes left running`.

On a failure other than `auth_required`, the adapter's last stderr goes to stderr.
`SMOKE_TRACE=1` prints the whole ACP traffic to stderr.

## `--mock-codex`

Runs only codex-acp, against a local stand-in for the Responses API that answers "OK", the way
`scripts/codex-launch-probe/` does: a Codex home in the scratch folder with a `mock` model provider
on 127.0.0.1 and no `auth.json`, and `HOME` in the scratch folder too. It needs no account and no
network, and proves the whole path (models, the prompt, the reply, the cleanup) on any machine.
`SMOKE_CODEX_HOME` is ignored in this mode.
