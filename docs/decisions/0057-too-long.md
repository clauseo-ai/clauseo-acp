# 0057: A question too long for the model, or for Clauseo

## Context

Clauseo capped a prompt's text at 256 KiB of raw UTF-8 bytes, far below what Claude and ChatGPT
accept, and refused anything above it as `command.invalid` ("Clauseo couldn't take that request as
it was."). The research behind this record is the private `measurements/long-question/cap.md`.

- The binding limit is the ACP client's own: 8 MiB per line the agent sends
  (`crates/acp/src/line.rs` `MAX_LINE_BYTES`). A longer line closes the connection. When a
  conversation is reopened, the agent replays each question as one `user_message_chunk` line, so
  a question must fit that line once JSON-escaped, with room for the envelope.
- JSON escaping can make text up to six times longer (`\u0001`), so a raw-byte cap does not bound
  the line.
- Codex refuses input over 1,048,576 characters; a 1M-token Claude model takes roughly 4 to 6 MB.
- When the model refuses a question as too long, the lawyer saw nothing or a protocol error:
  - codex-acp sends an AIR client a full context as `end_turn` with a `sessionFailure` of
    category `limit`, actions `["new_session"]`, which the ACP crate only logged as a warning;
  - claude-agent-acp, recorded live (0.89.1, a 6,000,000-character question), sends
    `end_turn` with a `sessionFailure` of category `service`, severity `error`, titled "Prompt is
    too long", actions `["retry"]`, after a failed compaction and with no answer text; it was only
    logged;
  - codex-acp, recorded live (2.2.2 on Codex 0.162.1, a 6 MiB question), refuses input over
    Codex's 1,048,576 characters as `end_turn` with a `sessionFailure` of category `service`,
    titled "Codex encountered an internal error.", actions `["retry", "new_session"]`; the length
    reason is only on its stderr.
- Both adapters accepted a single 6 MB stdin line without error.

## Decision

- **The cap.** `MAX_PROMPT_TEXT_BYTES` is 6 MiB, counted as JSON-escaped bytes
  (`escaped_len`) summed over the prompt's `Text` blocks, for `SendPrompt` and for
  `EditQueuedPrompt.text`. 6 MiB leaves 2 MiB of the 8 MiB line for the replay's envelope, and
  is above what either model accepts, so the model refuses first. The 8 MiB line cap stays.
- **Its code.** Over the cap is `prompt.too_long` (not retryable), read from the typed measure
  of `Error::TooLarge` (`Measure::PromptText`), never from its text, in one place
  (`HostError::invalid_command_code`, which the host's own `Error::code` uses too). Every other
  size limit stays `command.invalid`. The code has its frozen corpus pair
  (`corpus/added/turn_failed-PromptTooLong`). The measure stops at the first text block that
  crosses the cap. The constant is exported to the UI, which ports `escaped_len` and
  checks the draft when Ask is pressed (never per keystroke); one case table
  (`crates/protocol/tests/support/escaped-len-cases.json`) is read by the Rust and the TypeScript
  tests, so the two measures cannot drift. The draft is kept and nothing is sent.
- **The model refused: `StopReason::TooLong`.** A new stop reason, `too_long`, additive to the
  open enum under 0002 (frozen pair `corpus/added/turn_ended-TooLong`, no schema bump). It is a
  stop, not a failure: no `TurnFailed` before it, the answer so far is kept, and the queue holds,
  as for every stop but `EndTurn`. Counts report it `agent_stopped` (0036). Not `AtLimit`,
  which is the account's usage limit, and not `MaxTokens`, which is the answer's length.
- **Where it is read.** In `crates/acp`, `map/too_long.rs` is the one home of every shape that
  says "too long", each a `TooLong` kind: a `sessionFailure` of category `limit`, severity
  `error`, actions exactly `["new_session"]` (`ContextExhausted`; codex-acp's full context, from
  its source); one of category `service` or `request`, severity `error`, titled "Prompt is too
  long" (`PromptTooLong`; prefix, ASCII case ignored; Claude, recorded); and a JSON-RPC error to
  `session/prompt` whose allowlisted `data` fields say `prompt_too_long` or a context-window
  kind. No free text is matched anywhere else: Codex's "Input exceeds the maximum length of"
  never reaches the wire (recorded), so its prefix match was removed. Codex's recorded
  "internal error" is not read as too long: the wire cannot tell it from a real internal error.
  It is a failure the agent gave up on (amended, codex-service-error): any `sessionFailure` of
  severity `error` with `retry` or `new_session` in its actions, in a category no earlier rule
  reads (not `limit` nor `connection`, and not too long), is `PromptEnd::failed`, read after
  limit, too long and connection. The host ends that turn `Failed { agent.service_error }` (0003):
  a failure line and Try again (never Restart), the queue held, the agent's title in no event and
  no log line. A turn that ends `TooLong` never ends so. A JSON-RPC error keeps
  `agent.protocol_error`: it cannot be told from a malformed reply.
- **Which agent.** The host ends a turn `TooLong` for `PromptTooLong` from any agent, and for
  `ContextExhausted` only from codex-acp: claude-agent-acp sends that shape for its configured
  turn or output limit, which shortening the question does not fix; there it is logged by kind.
  It is never a `LimitSignal`: it reaches the host as `PromptEnd::too_long` or
  `Error::prompt_too_long`, so it never reaches the limits store nor the Codex limits read. A
  too-long `sessionFailure` on a `session_info_update` is its own event, `AcpEvent::TooLong`,
  which the host ORs into the turn's end, so a failure sent only on the update is not lost; it is
  never logged as a warning.
- **Codex's own cap, checked by the host.** Codex refuses a turn whose text items hold more than
  `MAX_USER_INPUT_TEXT_CHARS` (`1 << 20`) Unicode scalar values together, and codex-acp 2.2.2
  passes that refusal on only as "Codex encountered an internal error.". So the host refuses a
  question to a Codex thread over that cap with `prompt.too_long`, the same words as Clauseo's own
  cap, before anything is queued or sent: on `SendPrompt` (start now, queue, send now, steer) and
  `EditQueuedPrompt`. A thread's agent is fixed when it is created, but the undo note (the files
  put back since the last turn, no length cap) is added only when the turn's prompt goes out, so
  the host checks the text again there, note in: over the cap, the turn fails with
  `prompt.too_long` before anything is sent, the queue holds, and the note waits for the next
  prompt. A steer carries no note. Claude is not checked; its own refusal ends `TooLong`.
  - How the limit was found: in codex-rs at `rust-v0.162.1` (the Codex codex-acp 2.2.2 bundles),
    `protocol/src/user_input.rs:9-10` sets the cap; `app-server/src/request_processors/
    turn_processor.rs:552-556` (`turn/start`) and `:515-516` (`turn/steer`) sum
    `text_char_count` over the input items; `app-server-protocol/src/protocol/v2/turn.rs:532-541`
    counts a text item's `chars().count()` and every other item 0. codex-acp 2.2.2's
    `buildPromptItems` (`dist/index.js:35754`) passes a text block as it is and turns a
    `resource_link` into the text `[@name](uri)` (`formatUriAsLink`, `:35808`), which counts too.
  - Measured live with codex-acp 2.2.2 on Codex 0.162.1, no request leaving the machine (private
    `measurements/long-question/recordings/codex-boundary/README.md`): 1,048,576 characters are
    accepted and 1,048,577 refused; 1,048,576 emoji and 1,048,576 Devanagari letters are accepted;
    a link adds exactly `5 + chars(name) + chars(uri)` (58 for `paper.txt` with a 44-character
    URI), and with it the boundary holds to one character either way. Codex's own answer to
    codex-acp then carries `input_error_code: "input_too_large"`, which codex-acp drops.
  - Clauseo pins both: codex-acp 2.2.2, and `@openai/codex` 0.162.1 through an `overrides` entry
    (codex-acp itself asks for `^0.160.1`), in `crates/limits/tests/live/package.json` and its
    lock, which the agents install runs `npm ci` on.
  - `crates/host/src/codex_input.rs` counts exactly that, with no margin; its constants name
    codex-acp 2.2.2 and Codex 0.162.1, and a test fails when the pinned versions move. The limit
    must be re-checked on every codex-acp bump. `codex-input-too-long` keeps the recorded refusal
    as evidence of what the lawyer would otherwise get.
  - The UI does not check Codex's cap: the composer would need the thread's agent and the file
    links' text, which only the host has. A host refusal shows the same words, the draft kept;
    one when the prompt goes out shows them on the failed turn, the question kept in the thread.
- **Error data.** The driver keeps only a few allowlisted fields of a JSON-RPC error's `data`
  (`errorKind`, `terminal_reason`, `codex_error_info`), each only when it is a short token of
  at most 64 ASCII letters, digits, `_`, `-` and `.`; the raw data, which may echo the
  lawyer's question, is not kept there.

## Consequences

- `claude-prompt-too-long` and `codex-input-too-long` are built from the live recordings, and
  the replay-line fixtures are the recorded lines; `codex-context-full` is still from source (not
  recorded, to avoid billing).
- Codex's refusal of an over-long input, should one still reach Codex (a Codex whose count
  differs), reaches the lawyer as nothing: the turn ends `EndTurn` with no answer and the queue
  moves on. The host's check keeps such questions back.
- Claude's `context_exhausted` failure (its configured turn or output limit) has Codex's shape;
  the host reads it as too long only for codex-acp, so it stays logged, not worded, for Claude.
- A question near 6 MiB is about 1,500 view changes and several copies in memory while it is
  sent; the thread's drawing of it is not measured.
- A later change that raises the cap must keep the replayed line under `MAX_LINE_BYTES`
  (`crates/acp/src/line/tests/replay.rs`).

## Status

proposed

## Date

2026-11-11
