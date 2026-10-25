# Scenario format

A scenario is a `.jsonl` file that `fake_agent` plays over stdio, one line at a time. It stands
in for a real ACP v1 agent in tests: it waits for the client lines the scenario expects, and
sends the lines the scenario writes. It never waits for anything but the next client line: no
timers, no sleeps.

This file is the full reference. The code behind a rule is sometimes named in parentheses, such
as (`body.rs`), for whoever wants to check it; nobody needs to read the code to write a scenario.
The code lives in `crates/fake_agent/src/`.

## Contents

- [The file](#the-file)
- [Steps](#steps)
- [Patterns](#patterns)
- [Labels and ids](#labels-and-ids)
- [Includes](#includes)
- [`raw` and SDK validation](#raw-and-sdk-validation)
- [Running](#running)
- [Exit codes](#exit-codes)
- [Add a scenario](#add-a-scenario)

## The file

- Line 1 is the header. Every other non-blank line is one step. Blank lines are skipped but
  still count for line numbers.
- Every line is one JSON object. A line that is not JSON fails at load.
- Steps run in file order.
- Every load error names the file and line: `fake_agent: tool-calls.jsonl:7: <what>`. A line
  that came in through `include` names the included file and adds
  `; included from <file>:<line>`.

### Header

```json
{"scenario":"tool-calls","description":"Ten tool kinds, each with name and locations: ...","on_stdin_close":"exit"}
```

| Field | Required | Meaning | Checked at load |
|---|---|---|---|
| `scenario` | yes | The scenario name. | Must equal the file stem (`tool-calls` for `tool-calls.jsonl`). |
| `description` | yes | One line saying what the scenario does and what the test must do. | Must not be empty or only spaces. |
| `on_stdin_close` | no, default `"exit"` | What happens when stdin closes: `"exit"` or `"hang"`. See [Running](#running). | Must be one of the two strings. |

No other keys are allowed in the header (`body.rs`, `Header`).

## Steps

A step is a JSON object with exactly one step key, plus an optional `note`. The value of the
step key is the step's body. Bodies refuse unknown fields, so a typo fails at load with the list
of valid fields. A line with two step keys, no step key, or an unknown key fails at load and
lists every valid step key (`body.rs`, `StepBody`).

`note` is a string the agent ignores. Use it to explain a step to the next reader:

```json
{"note":"Either order: the cancel notification and the cancelled answer to perm-1.","all":[...]}
```

A `note` that is not a string fails at load. A line with only a `note` fails too: it has no
step key.

"Session cwd" below means the `cwd` of the last `session/new` or `session/load` request an
`expect` step matched. "Current session" means the `sessionId` of the last `session/new` result
a `respond` step sent, or of the last `session/load` request an `expect` matched (`run.rs`,
`received` and `respond`).

| Step | Fields (required unless marked) | When run | Checked at load |
|---|---|---|---|
| `expect` | `method`: string. `as`: string, the label (see [Labels](#labels-and-ids)). `params`: optional pattern. `off`: optional list of JSON pointers, default `[]`. | Reads the next client line. It must be a request with this `method` when `as` is set, or a notification with this `method` when it is not. Its `params` must match the pattern and every `off` pointer must be off (see [Patterns](#patterns)). Without `params`, any params match. A matched request is remembered under its label. | `as` is present for every method except `session/cancel` and `$/cancel_request`, and absent for those two. The label is not used twice. Every `off` pointer starts with `/`. |
| `expect_response` | `id`: string. One of `result` or `error`: a pattern. | Reads the next client line. It must be a response to the agent's request `id`, of the same kind (result or error), and match the pattern. | `id` names an earlier `request` step. Exactly one of `result` and `error`. |
| `all` | A non-empty list. Each item is `{"expect": {...}}` or `{"expect_response": {...}}`. | Reads client lines until every item has matched once, in any order. Each line must match one item that has not matched yet. | Each item as for its own step. The list is not empty. Labels from the items count only after the whole `all` step. |
| `respond` | `to`: label. One of `result` or `error`. `raw`: optional bool, default `false`. | Sends a response to the request labelled `to`, with that request's `id` (string or number, as the client sent it). `{{cwd}}` is replaced in the value. A result for `session/new` with a `sessionId` sets the current session. | `to` names an earlier `expect` label and is not answered twice. Exactly one of `result` and `error`. Unless `raw`: the value survives the SDK type of the labelled method's response, or `Error` for `error`. |
| `update` | `update`: object, the `update` of a `session/update`. `session`: optional session id, default the current session. `raw`: optional bool, default `false`. | Sends the notification `session/update` with `params` `{"sessionId": ..., "update": ...}`. `{{cwd}}` is replaced. Fails (exit 3) when there is no `session` and no current session. | Unless `raw`: the update survives the SDK's `SessionUpdate`. |
| `notify` | `method`: string. `params`: optional. `raw`: optional bool, default `false`. | Sends a notification. Leaves `params` out when it is not given. `{{cwd}}` is replaced. | Unless `raw`: `method` is one of `session/update`, `elicitation/complete` and `$/cancel_request`, and `params` survive its SDK type. |
| `request` | `id`: string. `method`: string. `params`: optional. `raw`: optional bool, default `false`. | Sends a request with this `id` as a JSON string. Does not wait: an `expect_response` waits for the answer. `{{cwd}}` is replaced. | `id` is not used by another `request`. Unless `raw`: `method` is a stable agent-to-client request (`session/request_permission`, `fs/read_text_file`, `fs/write_text_file`, `terminal/create`, `terminal/output`, `terminal/release`, `terminal/wait_for_exit`, `terminal/kill`, `elicitation/create`), and `params` survive its SDK type. |
| `send_line` | A string. | Writes the string and `\n` to stdout as is, valid JSON or not. Use it for a malformed line. | It does not hold `{{cwd}}`. It is not validated. |
| `write_file` | `path`: relative path. `content`: string. | Creates the parent folders and writes `content` to `path` under the session cwd, replacing any file there. Fails (exit 3) before any session. | `path` is not empty, not absolute (`/x`, `\x`, `C:...`), and has no `..` part. |
| `delete_file` | `path`: relative path. | Deletes the file at `path` under the session cwd. Fails (exit 3) when it is not a file, or before any session. | As `write_file`. |
| `move_file` | `from`, `to`: relative paths. | Moves `from` to `to` under the session cwd, creating `to`'s parent folders. Fails (exit 3) when `from` is not a file, or before any session. | Both paths as `write_file`. |
| `stderr` | `line`: string. `repeat`: optional integer, default `1`. | Writes `line` and `\n` to stderr `repeat` times, in one write. | `repeat` is 0 to 100,000 (`MAX_REPEAT`). |
| `spawn_child` | `pid_file`: relative path. | Starts `fake_agent --hold` with null stdio and never waits for it, writes its PID and `\n` to `pid_file`, then writes `fake_agent: child <pid>` to stderr. `pid_file` is relative to the agent's working directory (the process folder), not the session cwd, so it needs no session. | `pid_file` as `write_file`'s `path`. |
| `exit` | `code`: integer. | Flushes stdout and exits with `code`. Later steps never run. | `code` is 0 to 255 and not 2 or 3, which `fake_agent` keeps for itself (see [Exit codes](#exit-codes)). |
| `include` | A file name such as `"handshake.jsonl"`. | Never runs itself: at load, the included file's steps take its place. | See [Includes](#includes). |

Examples from the bundled scenarios:

```json
{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
{"respond":{"to":"new","error":{"code":-32000,"message":"Authentication required"}}}
{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-answer-1","content":{"type":"text","text":"The clause "}}}}
{"expect":{"method":"session/cancel","params":{"sessionId":"sess-1"}}}
{"notify":{"method":"elicitation/complete","params":{"elicitationId":"el-1"}}}
{"expect_response":{"id":"perm-1","result":{"outcome":{"outcome":"selected","optionId":"allow-once"}}}}
{"write_file":{"path":"notes/a.txt","content":"Alpha\n"}}
{"move_file":{"from":"notes/b.txt","to":"archive/b.txt"}}
{"stderr":{"line":"fake_agent exit-mid-turn stderr filler.","repeat":200}}
{"spawn_child":{"pid_file":"grandchild.pid"}}
{"exit":{"code":1}}
{"include":"handshake.jsonl"}
```

## Patterns

`expect.params`, `expect_response.result` and `expect_response.error` are patterns. A client
value matches a pattern by these rules (`pattern.rs`, `matches`):

| Pattern | Matches |
|---|---|
| An object | An object that has every key the pattern names, each matching. Extra keys are fine (a subset match). `{}` matches any object, and only an object. |
| An array | An array of the same length whose items match one by one, in order. |
| `"{{any}}"` | Any value that is present, `null` included. Only the whole string counts; `"x {{any}}"` is a plain string. |
| A string holding `{{cwd}}` | The string with every `{{cwd}}` replaced by the session cwd. |
| A number | An equal number. When either side is a float, both compare as 64-bit floats, so `1` matches `1.0`. Two integers compare exactly, so `9007199254740993` does not match `9007199254740992`. |
| Any other value | An equal value: strings exactly, `true`, `false` and `null` only themselves. |

A key the pattern names that the client left out never matches, even when the pattern is
`null` or `"{{any}}"`. The mismatch message names the first JSON pointer that differs, such as
`initialize params: /protocolVersion: expected 1, got "1"`.

### `{{cwd}}`

`{{cwd}}` is replaced inside string values, never in keys, in patterns and in everything the
agent sends (`respond`, `update`, `notify`, `request`). A scenario that uses `{{cwd}}` before
any `session/new` or `session/load` request has matched fails at that step with exit 3.
`send_line` may not hold it, since a Windows cwd's backslashes would make the raw line invalid
JSON. At load, `{{cwd}}` becomes `/fake_agent/cwd` for SDK validation.

### `off`

`off` lists JSON pointers (RFC 6901, starting with `/`) into the client's `params`. Each one
must be off, checked after the `params` pattern matched (`pattern.rs`, `check_off`). A value is
off when it is:

- missing (also when the client sent no `params` at all),
- `null`,
- `false`, or
- a non-empty object whose values are all off.

An empty object `{}` is **on**: in schema 1.11.0, `"subagents": {}` is how a client advertises
subagents. Anything else (`true`, a number, a string, an array) is on too.

This example from `initialize-caps.jsonl` (shortened) requires the client to leave file
system, terminal and subagent capabilities out, or turned off:

```json
{"expect":{"method":"initialize","as":"init","params":{"protocolVersion":1,"clientCapabilities":{"plan":{}}},"off":["/clientCapabilities/fs","/clientCapabilities/terminal","/clientCapabilities/subagents","/clientCapabilities/session/subagents"]}}
```

`expect_response` has no `off`.

## Labels and ids

- **`as`** names a client request so a later `respond` can answer it. It is required on every
  `expect` except for the two notifications the client sends, `session/cancel` and
  `$/cancel_request`, which must not have one. Without `as`, an `expect` matches only a
  notification; with it, only a request. Labels are unique in the scenario, includes counted:
  a scenario that includes `handshake.jsonl` cannot reuse `init` or `new`.
- A label need not be answered. Leaving a request unanswered is how `cancel-unanswered` tests
  the client's fallback.
- **`request.id`** is the id the agent sends, always as a JSON string. Ids are unique in the
  scenario. `expect_response.id` must name an earlier `request` step; the client's response
  must carry that id as the same string.

## Includes

`{"include":"handshake.jsonl"}` replaces itself with the steps of a shared file:

- The value is a bare file name ending in `.jsonl`, with no `/` or `\`.
- The file comes from `scenarios/common/`. A bundled scenario played by name (or read by
  `Scenario::parse`) uses the copies compiled into the binary. A scenario read from a path
  (`--scenario path/x.jsonl`, `--check`, `Scenario::load`) uses the `common/` folder beside it.
- An included file has no header: every non-blank line is a step.
- Includes cannot nest: an `include` inside an included file fails at load.
- Included steps keep their own file and line in messages. Load errors add
  `; included from x.jsonl:2`; mismatches while running do not.
- All load checks run over the scenario after includes are expanded, so labels and request ids
  from the included file count.

The one shared file today, `common/handshake.jsonl`, expects `initialize` (label `init`) and
`session/new` (label `new`, any `cwd`) and answers `session/new` with `sessionId` `sess-1`.

## `raw` and SDK validation

At load, every message that `respond`, `update`, `notify` and `request` send is checked against
the SDK's typed v1 schema for its method (`agent-client-protocol`, no unstable features)
(`validate.rs`):

1. The value is deserialised into the SDK type for its method (for `update`, `SessionUpdate`;
   for an error, `Error`).
2. That typed value is serialised again.
3. Every key and value the scenario wrote must still be there and equal. Arrays must keep their
   length. Extra keys the SDK adds (defaults) are fine.

Plain deserialising is not enough, because some SDK types fall back on unknown input: for
example `"kind":"edit_file"` would deserialise as `other`. The round trip catches it.

A key the SDK leaves out when it holds its default or is empty passes only at the places where
the SDK really does leave it out:

| Method | Place | Passes when |
|---|---|---|
| `session/update` | `kind` of a `tool_call` update | it is `"other"` |
| `session/update` | `status` of a `tool_call` update | it is `"pending"` |
| `session/update` | `content`, `locations` of a `tool_call` update | it is `null`, `[]` or `{}` |
| `initialize` result | `args`, `env` of an `authMethods` item with `"type":"terminal"` | it is `null`, `[]` or `{}` |
| `terminal/create` params | `args`, `env` | it is `null`, `[]` or `{}` |

Anywhere else, a key that does not survive fails, `null` included. For example
`"status":"pending"` on an `agent_message_chunk` fails:
`session/update: /update/status: "pending" does not survive the SDK schema (dropped)`.

A method the SDK has no stable type for fails at load and asks for `raw`. **`"raw": true`**
goes inside the body of a `respond`, `update`, `notify` or `request` step, and only there; any
other step refuses it as an unknown field. It skips only the SDK validation: label, id and
`{{cwd}}` checks still apply. From `unknown-update.jsonl`:

```json
{"update":{"update":{"sessionUpdate":"_clauseo_test_unknown","messageId":"m-answer-1","content":{"type":"text","text":"Never shown."}},"raw":true}}
```

Nothing checks `expect` and `expect_response` patterns against the SDK, and nothing checks
`send_line`. A pattern with a typo loads fine and fails only when a real client line does not
match it.

## Running

```sh
fake_agent --scenario <name or path>
```

A bare name plays a bundled scenario; anything with `/`, `\` or a `.jsonl` suffix is a file.

- Each client line must end with `\n` (a `\r` before it is dropped) and be a JSON-RPC 2.0
  request, notification or response, with a string or number `id`. Anything else is a mismatch.
- A client line that does not match the step waiting for it is a mismatch: `fake_agent` writes
  one stderr line `fake_agent: <file>:<line>: <what>` and exits 3.
- Every line the agent sends is one line on stdout, flushed at once. Stdout carries nothing
  else; diagnostics go to stderr.
- After the last step the agent keeps reading. Any client line then is a mismatch
  (`unexpected <message> after the last step`).
- When stdin closes, at any point, `on_stdin_close` decides:

| `on_stdin_close` | When stdin closes |
|---|---|
| `"exit"` (default) | Exit 0 at once, even if steps are left. Exit 0 does not prove the scenario ran to the end. |
| `"hang"` | Write `fake_agent: stdin closed, holding` to stderr and block forever, until killed. Use it to test that the client escalates to a signal. |

Stdin closing is only seen while a step reads. Steps that send run before the agent reads
again.

## Exit codes

The constants are in the library (`clauseo_fake_agent::EXIT_*`). Assert on them, not on the
numbers.

| Code | Constant | Meaning |
|---|---|---|
| 0 | `EXIT_OK` | Stdin closed under `"exit"`, or `--list` or `--check` succeeded. |
| 1 | `EXIT_WRITE` | `--list` or `--check` could not write its result to stdout. |
| 2 | `EXIT_LOAD` | Bad arguments, or a scenario that fails to load (`--scenario` or `--check`). |
| 3 | `EXIT_MISMATCH` | A client line did not match, or a step failed while running (a file step, a missing session, a failed write). |
| other 0 to 255 | none | An `exit` step's code. |

An `exit` step may not use 2 or 3, so those codes always mean `fake_agent` itself. It may use 0
and 1 (`exit-mid-turn` uses 1); 1 means `EXIT_WRITE` only for `--list` and `--check`, which play
no scenario.

## Add a scenario

1. Pick a name in kebab case. Create `crates/fake_agent/scenarios/<name>.jsonl`.
2. Write the header on line 1: `scenario` equal to `<name>`, a `description` that says what the
   client must do, and `on_stdin_close`.
3. Start with `{"include":"handshake.jsonl"}` unless the scenario tests the opening itself
   (as `initialize-caps` and `load-replay` do) or needs no handshake (`grandchild`). Do not
   reuse its labels `init` and `new`.
4. Write the steps. Give every client request an `as` label and answer it with `respond`. Give
   every agent `request` a unique `id` and wait for the answer with `expect_response`. Use
   `{{cwd}}` for paths in the session folder, and `note` for anything a reader could misread.
5. Mark `"raw": true` only on a message the SDK has no stable type for, and say why in a `note`.
6. To bundle it, add the name to the `scenarios` list of the `bundled!` macro in
   `crates/fake_agent/src/bundled.rs`, keeping the list sorted. A new shared include goes in
   `scenarios/common/` and in the macro's `common` list.
7. Write the test that plays it, in the crate that needs it.
8. Run, from the repo root:

   ```sh
   cargo run -q -p clauseo-fake_agent -- --check crates/fake_agent/scenarios
   ```

   It prints `ok <n>`, with `<n>` the number of scenario files, and exits 0. Any error names the
   file and line. Then run `cargo nextest run -p clauseo-fake_agent`: it checks that the
   `bundled!` list and the folder agree.
