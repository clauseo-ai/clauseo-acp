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
  - claude-agent-acp sends an API refusal as `end_turn` with a `sessionFailure` of category
    `request`, titled "Prompt is too long", also only logged; without a session failure it
    answers -32603 with `data.errorKind`;
  - Codex's over-1M-character refusal most likely comes as a JSON-RPC error to `session/prompt`,
    which ended the turn `agent.protocol_error`.

## Decision

- **The cap.** `MAX_PROMPT_TEXT_BYTES` is 6 MiB, counted as JSON-escaped bytes
  (`escaped_len`) summed over the prompt's `Text` blocks, for `SendPrompt` and for
  `EditQueuedPrompt.text`. 6 MiB leaves 2 MiB of the 8 MiB line for the replay's envelope, and
  is above what either model accepts, so the model refuses first. The 8 MiB line cap stays.
- **Its code.** Over the cap is `prompt.too_long` (not retryable), read from the typed measure
  of `Error::TooLarge` (`Measure::PromptText`), never from its text. Every other size limit
  stays `command.invalid`. The constant is exported to the UI, which ports `escaped_len` and
  checks the draft when Ask is pressed (never per keystroke); one case table
  (`crates/protocol/tests/support/escaped-len-cases.json`) is read by the Rust and the TypeScript
  tests, so the two measures cannot drift. The draft is kept and nothing is sent.
- **The model refused: `StopReason::TooLong`.** A new stop reason, `too_long`, additive to the
  open enum under 0002 (frozen pair `corpus/added/turn_ended-TooLong`, no schema bump). It is a
  stop, not a failure: no `TurnFailed` before it, the answer so far is kept, and the queue holds,
  as for every stop but `EndTurn`. Counts report it `agent_stopped` (0036). Not `AtLimit`,
  which is the account's usage limit, and not `MaxTokens`, which is the answer's length.
- **Where it is read.** In `crates/acp`, `map/too_long.rs` is the one home of every shape that
  says "too long": a `sessionFailure` of category `limit`, severity `error`, actions exactly
  `["new_session"]`; one of category `request`, severity `error`, titled "Prompt is too long"
  (prefix, ASCII case ignored); and a JSON-RPC error whose allowlisted `data` fields say
  `prompt_too_long` or a context-window kind, or whose text begins with Codex's exact
  "Input exceeds the maximum length of". It is never a `LimitSignal`: it reaches the host as
  `PromptEnd::too_long` or `Error::prompt_too_long`, so it never reaches the limits store nor the
  Codex limits read. A too-long `sessionFailure` on a `session_info_update` makes no event; the
  prompt's answer carries the same record.
- **Error data.** The driver keeps only a few allowlisted fields of a JSON-RPC error's `data`
  (`errorKind`, `terminal_reason`, `codex_error_info`), each only when it is a short token of
  at most 64 ASCII letters, digits, `_`, `-` and `.`; the raw data, which may echo the
  lawyer's question, is not kept there.

## Consequences

- The shapes are written from the adapters' source; live recordings will replace the scenarios
  (`codex-context-full`, `claude-prompt-too-long`, `codex-input-too-long`) and may move a
  rule inside `map/too_long.rs`, nothing else.
- Claude's `context_exhausted` failure (its configured turn or output limit) has Codex's shape
  and reads as too long too.
- A question near 6 MiB is about 1,500 view changes and several copies in memory while it is
  sent; the thread's drawing of it is not measured.
- A later change that raises the cap must keep the replayed line under `MAX_LINE_BYTES`
  (`crates/acp/src/line/tests/replay.rs`).

## Status

proposed

## Date

2026-11-11
