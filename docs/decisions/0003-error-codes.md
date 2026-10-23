# 0003: Error codes

## Context

Every host call returns `Result<T, HostError { code, message, retryable }>`. The UI, `cli` and a later
phone client choose what to show or do by `code`, never by `message`, which is for logs. The codes
must therefore stay stable across versions.

## Decision

- `ErrorCode` is a fieldless enum in `crates/protocol/src/error_code.rs`. Each code has a stable
  dotted string, `<area>.<reason>`, which is its form on the wire. It is not `#[non_exhaustive]`:
  `unknown` covers newer codes on the wire, so a crate that matches codes lists every one and a
  new code flags each match that should handle it. `Unknown` stays last in `ErrorCode::ALL`; the
  `unknown_is_the_last_code` test checks it.
- Codes are append-only, and a code's meaning never changes. A new code needs a row here and the
  code owner's approval.
- A code string this version does not know decodes to `unknown`, and `HostError.message` becomes
  `<original code>: <original message>`. The same holds for a logged `TurnFailed`: its `detail`
  becomes `<original code>: <original detail>`, or `<original code>` when it had none, so a re-sent
  copy still names the code. Clauseo never produces `unknown` itself.
- `protocol`'s own `Error` (a value that did not parse or broke a rule) maps to no code by
  itself; each caller picks one. `HostError::invalid_command(error)` builds `command.invalid` for a
  command's own input. A value read back from the store that does not parse, such as a
  `StreamId` or a `MatterPath` in a row, is `store.corrupt`, built with `HostError::new`.
- A limit is not an error: reaching an account limit has no code.
- Retryable means the same call may succeed if made again later with nothing else changed. A
  retryable failure may be retried by the client; any other needs the lawyer or a code fix first.
  `HostError.retryable` carries the value below.

One row per code. The `error_codes_match_record` test reads this table and compares it with
`ErrorCode::ALL`: the code strings, the meaning column with `ErrorCode::meaning()`, and retryable
as `yes` or `no`. The meaning is also each variant's doc comment in `error_code.rs`.

| code | meaning | retryable | produced by |
|---|---|---|---|
| `internal` | A bug or an unexpected failure in the host; Tauri's own string rejections map here too. | no | host, desktop bridge |
| `command.invalid` | The command failed shape validation: a length, count or format. A query that fails to decode (a `PageLimit` or `ReadLen` out of range) gets this code too. | no | `CommandEnvelope::validate` and `HostError::invalid_command` (desktop, transport_ws, cli) |
| `command.conflict` | The command conflicts with what the host holds, such as a reused `command_id` with a different body. | no | host |
| `command.unknown_matter` | The command names a matter the host does not know. | no | host |
| `command.unknown_thread` | The command names a thread the host does not know. | no | host |
| `thread.turn_running` | A turn is running, so the thread refuses this command (archive, restart) for now. | yes | host; acp (guard against a second `session/prompt`) |
| `thread.interrupted` | The host stopped during the turn; the turn ended without its answer. | no | host (recovery, in `turn_failed`) |
| `thread.turn_not_queued` | The queued turn an edit, remove or send-now names has already started or ended. | no | host |
| `thread.queue_full` | The thread already has `MAX_QUEUED_TURNS` queued turns. | yes | host |
| `permission.not_pending` | The permission request was already answered, expired or never existed. | no | host |
| `permission.unknown_option` | The answer names an option the permission request did not offer. | no | host |
| `elicitation.not_pending` | The elicitation was already answered, expired or never existed. | no | host |
| `elicitation.invalid_values` | The values do not match the elicitation's schema. | no | host |
| `agent.launch_failed` | The agent's process did not start or did not finish the ACP handshake. | yes | process supervisor, acp |
| `agent.exited` | The agent's process exited; a new launch may work. | yes | acp, host |
| `agent.protocol_error` | The agent sent something that breaks ACP. | no | acp |
| `agent.auth_required` | The agent needs the lawyer to sign in first. | no | acp |
| `agent.unsupported_version` | The agent speaks an ACP version Clauseo does not support. | no | acp |
| `store.disk_full` | The disk is full; space must be freed first. | no | store |
| `store.corrupt` | The data folder holds data that does not read back, a stored value that does not parse included. | no | store |
| `store.io` | A read or write in the data folder failed, possibly for a passing reason. | yes | store |
| `store.locked` | Another Clauseo process holds the data folder. | no | store |
| `store.newer_version` | The data came from a newer Clauseo. | no | store |
| `matter.checkpoint_failed` | Taking a checkpoint of the matter's files failed. | yes | matters |
| `matter.folder_missing` | The matter's folder is gone: moved, deleted, or its drive is not mounted. | no | matters |
| `matter.folder_refused` | The folder may not be a matter: a drive root, the home folder or a system folder. | no | matters |
| `matter.file_locked` | A file the operation needs is locked by another program, such as an open document. | yes | matters |
| `matter.turn_running` | A turn is running in the matter, so undo or restore waits. | yes | host, matters |
| `transport.unauthorized` | The client is not allowed to make this call. | no | transport_ws, desktop |
| `transport.too_large` | The message is larger than the transport accepts. | no | transport_ws, desktop |
| `unknown` | A code from a newer Clauseo that this version does not know. Never produced here. | no | decoding only |

Judgement calls on retryable: `agent.launch_failed` and `agent.exited` are retryable because a new
launch can work; `store.locked` is not, because the lawyer must close the other Clauseo first;
`matter.checkpoint_failed` is, because the usual cause (another program holding the repository lock)
passes; `matter.turn_running` is, as `thread.turn_running` is.

## Consequences

- The UI maps each code to copy and an action once; `unknown` needs a generic treatment of its own,
  since a newer host can send any code.
- An older Clauseo talking to a newer host still decodes every failure, as `unknown`, and keeps the
  original code in the message for logs.
- Renaming a code or changing its meaning is not allowed; a replacement is a new code, and the old
  one stays.

## Status

Proposed. Accepted once Rohan approves the pull request that adds this record.

## Date

2026-10-19
