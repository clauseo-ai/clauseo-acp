# codex-launch-probe

Runs real codex-acp and Codex against a local stand-in for the Responses API and checks four rows
of the Codex launch. Each row is one scripted model step. It needs no login and no network, and
nothing in CI runs it. It exists to check the two `[features]` keys the host sets for Codex
(`LAUNCH_FEATURES` in `crates/host/src/agent/codex.rs`) against the pinned codex-acp and Codex.

```sh
# A node_modules folder holding the pinned @agentclientprotocol/codex-acp and @openai/codex.
node scripts/codex-launch-probe/probe.mjs <node_modules>
```

The probe makes a scratch folder under the temp folder with a Codex home and a work folder. At the
end it stops codex-acp and the Codex processes it started, and removes the folder only after they
have exited (SIGTERM, then SIGKILL after 10 s). The same happens on the timeout and on an error.
`KEEP=1` keeps the folder. The home has no `auth.json`, and `HOME`
points into the scratch folder too. The home's `config.toml` holds the host's keys as dotted
keys (`clauseo_limits::CODEX_OVERRIDES`, then `LAUNCH_FEATURES`), plus a `mock` model provider on
127.0.0.1 with `requires_openai_auth = false`. `CODEX_CONFIG` holds the same keys as nested
objects. The main thread runs `gpt-5.5`, which has direct tools. Codex names the conversation on
a second thread (`gpt-5.6-luna`), which gets the tool declarations as code-mode text in its
input, so each request is searched whole.

The rows:

- **a.** No request, on the main thread or the title thread, names `create_goal`, `get_goal` or
  `update_goal`, or offers a `tty` parameter.
- **b.** `exec_command` with `tty: true` gets "TTY execution is disabled by config".
- **c.** `write_stdin` with input into a running command gets "stdin is closed for this session".
- **d.** Ordinary work still runs: a shell command writes a file, `rg` finds a line, `apply_patch`
  adds a file, and a long command (`sleep 4; echo long-done`) started with a short yield ends on
  an empty `write_stdin` poll with its output and exit code 0. The turn ends after the last step.

Each row prints `PASS` or `FAIL`. The exit code is 1 when any row fails.

Two variables change the launch so you can see the rows tell the cases apart:

- `BASELINE=1` leaves both keys out. Rows a and b fail. Row c still passes, because a command
  started without a terminal never had stdin to write to.
- `KEYS_IN=config` puts the keys only in `CODEX_CONFIG`. Row a fails on the title thread, which
  reads only the home's `config.toml`. `KEYS_IN=home` puts them only in the home's file.
