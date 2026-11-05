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
- `HostError.matter` (optional, left out when empty) names the matter a failure is about, for the
  codes whose meaning says so (`matter.folder_is_matter`), so the UI can name it and offer to
  open it without reading `message`. Added by decision record 0022.
- `HostError.files` (optional, left out when empty) lists the matter's files a failure is about, as
  matter paths: for `matter.file_locked`, the files held open, so the UI can name the document to
  close without reading `message` (an undo has no preview that lists them). For
  `matter.file_locked` the files are in `files`, not in `message`. A peer that predates it reads such an error as before, without the files.
- `matter.folder_refused` stays, with its meaning: since 0022 the host answers `CreateMatter` with
  one of the six `matter.folder_*` codes after it instead, one per reason.

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
| `store.io` | A read or write in the data folder failed, possibly for a passing reason. | yes | store; host (`SetPrivacySettings` and `FirstAct`, when the usage counts folder or the first-act marker cannot be changed; 0036) |
| `store.locked` | Another Clauseo process holds the data folder. | no | store |
| `store.newer_version` | The data came from a newer Clauseo. | no | store |
| `matter.checkpoint_failed` | Taking a checkpoint of the matter's files failed. | yes | matters |
| `matter.folder_missing` | The matter's folder is gone: moved, deleted, or its drive is not mounted. | no | matters |
| `matter.folder_refused` | The folder may not be a matter: a drive root, the home folder or a system folder. | no | matters |
| `matter.codex_folder` | The matter folder holds a `.codex` folder, whose settings Codex would load. | no | host (a Codex agent's launch and each of its prompts; decision record 0025) |
| `matter.file_locked` | A file the operation needs is locked by another program, such as an open document. | yes | matters |
| `matter.turn_running` | A turn is running in the matter, so undo or restore waits. | yes | host, matters |
| `matter.folder_is_drive` | The folder may not be a matter: it is the root of a drive or a mounted volume. | no | host (from matters' refusal; decision record 0022) |
| `matter.folder_is_home` | The folder may not be a matter: it is the home folder, or holds it. | no | host (0022) |
| `matter.folder_is_system` | The folder may not be a matter: it is a system folder or a folder of app data or keys, or holds one. | no | host (0022) |
| `matter.folder_unreadable` | Clauseo may not read the folder, or its `.clauseo/matter.json` does not read, so it may belong to a matter. | no | host (0022) |
| `matter.folder_is_matter` | The folder is a matter's folder, or sits inside one or holds one; the error's `matter` names that matter when it is on Clauseo's list. | no | host (0022) |
| `matter.folder_in_matters_root` | The folder is or holds Clauseo's data folder, or is, holds or sits inside the matters' history folder in it. | no | host (0022) |
| `matter.folder_network` | The matter folder is on a network drive, where Codex's sandbox can neither read nor write, so Codex is not started there. | no | host (a Codex agent's launch on Windows, `agent/folder_support.rs`) |
| `matter.folder_no_permissions` | The matter folder is on a drive that keeps no file permissions (FAT, exFAT), which Codex's sandbox needs, so Codex is not started there. | no | host (a Codex agent's launch on Windows, `agent/folder_support.rs`) |
| `matter.folder_too_deep` | The matter folder's path is too long for Windows to start programs in it: move the folder closer to the top of its drive, or choose a higher folder as the matter. | no | host (matters' refusal on create, import and the folder query; the host on open and at a launch, on Windows) |
| `transport.unauthorized` | The client is not allowed to make this call. | no | transport_ws, desktop |
| `transport.too_large` | The message is larger than the transport accepts. | no | transport_ws, desktop |
| `settings.locked` | The change names a setting that the firm's policy sets, so the lawyer cannot change it; nothing was changed. | no | host (`SetUiSettings`, `SetToolSettings`, `SetPrivacySettings`, `SetSkillPack`; decision records 0017, 0030, 0036 and 0040) |
| `agent.install_offline` | The agent's install cannot reach its hosts: name resolution or connect failed with no route. The install waits and tries again by itself. | yes | agents (S17 `FailureKind::Offline`), desktop (0048) |
| `agent.install_blocked` | A proxy or firewall blocked the agent's download: a 403 or 407 from a proxy, a TLS failure, or a web page where a file should be. | no | agents (S17 `FailureKind::Blocked`), desktop (0048) |
| `agent.install_disk_full` | Too little free space for the agent's install; a retry succeeds once space is freed. | yes | agents (S17 `FailureKind::DiskFull`), desktop (0048) |
| `agent.install_failed` | The agent's install failed for another reason, after three tries. | yes | agents (S17 `FailureKind::Failed`), desktop (0048) |
| `signin.cancelled` | The lawyer cancelled the sign-in, in Clauseo or on the vendor's page. | no | agents (S18 `SigninCode`), desktop (0048); new area for R20 |
| `signin.link_expired` | The sign-in link or device code expired before the sign-in finished; a new sign-in gets a new one. | yes | agents (S18 `SigninCode`), desktop (0048); new area for R20 |
| `signin.no_plan` | The account has no plan the agent accepts. | no | agents (S18 `SigninCode`), desktop (0048); new area for R20 |
| `signin.device_code_off` | Device-code sign-in is turned off for the account. | no | agents (S18 `SigninCode`), desktop (0048); new area for R20 |
| `signin.port_busy` | Another program holds the port the agent's sign-in listens on (Codex's 1455). | yes | agents (S18 `SigninCode`), desktop (0048); new area for R20 |
| `signin.failed` | The sign-in ended before signing in, for any other reason. | yes | agents (S18 `SigninCode`), desktop (0048); new area for R20 |
| `signin.policy_unreadable` | The firm's policy cannot be read and once set this agent's firm plan, so sign-in and launch are refused rather than sent to a plan login. | no | desktop (0048); new area for R20 |
| `agent.guard_missing` | A Claude launch that must ask before every change could not set up the guard that makes it ask, so it was not started. | no | host (a forced-ask Claude launch with no guard hook, `actor/turn/profile_loads.rs`) |
| `update.failed` | The update's check, download, signature check or install failed; the version running keeps running, and a later check tries again. | yes | host (the updater, decision record 0037; produced from R23 PR 4) |
| `settings.refused` | The change breaks a settings rule, such as an agent entry that is not Clauseo's own install, a secret-shaped variable or an address that is not https; the message names the key, never the value, and nothing was changed. | no | host (`HostHandle::set_agent_launch`, `HostHandle::set_plan`, `SetSkillPack`; decision records 0048 and 0040) |
| `settings.key_stored` | A key is already stored for this agent, so this window may not replace it; it is changed or removed in Settings. No key text is in the message. | no | desktop (`setup`: a `SetAgentKey` from `main` when the agent already has a key, decision record 0048) |
| `signin.login_shared` | A Codex login, logout or authenticate was refused while Clauseo's Codex home shares the lawyer's ChatGPT login file; the remedy is in the app, "Stop using it here". | no | host (Codex login, logout and authenticate; decision record 0048) |
| `signin.firm_managed` | Sign-in is refused: a firm endpoint is set by policy. | no | desktop (`setup`: a sign-in start while a firm policy sets the agent's endpoint, decision record 0048) |
| `signin.key_in_use` | Sign-in is refused: an API key is stored. | no | desktop (`setup`: a sign-in start while an API key is stored for the agent, decision record 0048) |
| `signin.endpoint_in_use` | Sign-in is refused: an endpoint is set in Settings. | no | desktop (`setup`: a sign-in start while an endpoint set in Settings is in force, decision record 0048) |
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

accepted

## Date

2026-10-22
