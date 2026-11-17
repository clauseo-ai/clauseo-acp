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
  command's own input, except a prompt's text over its limit (`Error::TooLarge` with the typed
  measure `Measure::PromptText`), which is `prompt.too_long`. A value read back from the store
  that does not parse, such as a `StreamId` or a `MatterPath` in a row, is `store.corrupt`, built
  with `HostError::new`.
- A limit is not an error: reaching an account limit has no code.
- Retryable means the same call may succeed if made again later with nothing else changed. A
  retryable failure may be retried by the client; any other needs the lawyer or a code fix first.
  `HostError.retryable` carries the value below.
- `HostError.matter` (optional, left out when empty) names the matter a failure is about, for the
  codes whose meaning says so (`matter.folder_is_matter`, `matter.folder_inside_matter`,
  `matter.folder_holds_matter`), so the UI can name it and offer to
  open it without reading `message`. Added by decision record 0022.
- `HostError.files` (optional, left out when empty) lists the matter's files a failure is about, as
  matter paths: for `matter.file_locked`, the files held open, so the UI can name the document to
  close without reading `message` (an undo has no preview that lists them). For
  `matter.file_locked` the files are in `files`, not in `message`. A peer that predates it reads such an error as before, without the files.
  For `matter.file_refused`, the refused files, sorted; for `matter.file_write_failed`, the file.
  At most 50 (`MAX_ERROR_FILES`); the counts below give the real number.
- `HostError.refusal` (optional, left out when empty), for `matter.file_refused`: `because`, the
  cause every refused file shares (`RefusedBecause`: `permissions`, `second_name`,
  `saved_copy_unconfirmed`; a cause from a newer Clauseo reads as `unknown`), left out when the
  files were refused for different reasons; `refused`, how many files were refused; and `of`, how
  many files the action wrote, deleted or meant to, the refused ones included (1: the action was
  on that file alone, so nothing was done; more: everything else was done). The message gives
  counts only, never a path.
- `HostError.write_failure` (optional, left out when empty), for `matter.file_write_failed`:
  `put_back`, whether the file was written back as it was after its write in place failed
  partway (otherwise it may be damaged), and `of`, counted as for `refusal`. The message gives no
  path.
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
| `thread.interrupted` | The host stopped during the turn; the turn ended without its answer. | no | host (recovery, in `turn_failed`; and a running turn the host stops while it closes, in `cancel::shutdown`) |
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
| `matter.folder_missing` | The matter's folder is gone: moved, deleted, or its drive is not mounted or did not answer in time. | yes | matters |
| `matter.folder_refused` | The folder may not be a matter: a drive root, the home folder or a system folder. | no | matters |
| `matter.codex_folder` | The matter folder holds a `.codex` folder, whose settings Codex would load. | no | host (a Codex agent's launch and each of its prompts; decision record 0025) |
| `matter.file_locked` | A file the operation needs is locked by another program, such as an open document. | yes | matters |
| `matter.turn_running` | A turn is running in the matter, so undo or restore waits. | yes | host, matters |
| `matter.folder_is_drive` | The folder may not be a matter: it is the root of a drive or a mounted volume. | no | host (from matters' refusal; decision record 0022) |
| `matter.folder_is_home` | The folder may not be a matter: it is the home folder, or holds it. | no | host (0022) |
| `matter.folder_is_system` | The folder may not be a matter: it is a system folder or a folder of app data or keys, or holds one. | no | host (0022) |
| `matter.folder_unreadable` | Clauseo may not read the folder (a `.clauseo/matter.json` that does not read is set aside, not refused, since 2026-11-11). | no | host (0022) |
| `matter.folder_is_matter` | The folder is a matter's folder, or sits inside one or holds one; the error's `matter` names that matter when it is on Clauseo's list. | no | host (0022) |
| `matter.folder_in_matters_root` | The folder is or holds Clauseo's data folder, or is, holds or sits inside the matters' history folder in it. | no | host (0022) |
| `matter.folder_network` | The matter folder is on a network drive, where Codex's sandbox can neither read nor write, so Codex is not started there. | no | host (a Codex agent's launch on Windows, `agent/folder_support.rs`) |
| `matter.folder_no_permissions` | The matter folder is on a drive that keeps no file permissions (FAT, exFAT), which Codex's sandbox needs, so Codex is not started there. | no | host (a Codex agent's launch on Windows, `agent/folder_support.rs`) |
| `matter.folder_too_deep` | The matter folder's path is too long for Windows to start programs in it: move the folder closer to the top of its drive. | no | host (matters' refusal on create, import and the folder query; the host on open and at a launch, on Windows) |
| `transport.unauthorized` | The client is not allowed to make this call. | no | transport_ws, desktop |
| `transport.too_large` | The message is larger than the transport accepts. | no | transport_ws, desktop |
| `settings.locked` | The change names a setting that the firm's policy sets, so the lawyer cannot change it; nothing was changed. | no | host (`SetUiSettings`, `SetToolSettings`, `SetPrivacySettings`, `SetSkillPack`; decision records 0017, 0030, 0036 and 0040) |
| `agent.install_offline` | The agent's install cannot reach its hosts: name resolution failed, connect failed with no route, or the connection was reset, aborted or timed out. The install waits and tries again by itself; the app says it is offline only while the window says so, else that the download cannot be reached. | yes | agents (S17 `FailureKind::Offline`), desktop (0048) |
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
| `signin.agents_still_running` | An agent was still running after Clauseo waited for it to stop, so the login was not changed and nothing was removed; trying again once it has stopped can succeed. | yes | desktop (the leave order's stop_agents timed out) |
| `agent.keys_loading` | Clauseo has not finished loading the stored API keys; no agent was started; trying again in a moment can succeed. | yes | host (a launch while desktop still loads the stored API keys, decision record 0048) |
| `agent.key_unreadable` | A stored API key could not be read from the system's key store in time: the read had not answered (often a permission prompt not answered yet); no agent was started. Trying again once it can be read can succeed. | yes | host (a launch for an agent whose stored API key's read had not answered by desktop's bound, decision record 0048) |
| `matter.document_path_too_long` | A Word document's path is too long for Word to open, even with its folders' short names (Windows): move the matter's folder nearer the top of its drive, or shorten its folder names. | no | desktop (`open_file` for a Word document on Windows, decision record 0026) |
| `matter.protected_link` | A protected name in the matter folder (such as a nested CLAUDE.md, AGENTS.md or .claude) is a shortcut (a junction or symbolic link), which the Windows sandbox can't protect, so the agent is not started; the lawyer moves or deletes it. | no | sandbox (`windows::apply` on Windows, `Error::ProtectedLink` with the item's path relative to the matter; S15 PR4a2) |
| `sandbox.setup_refused` | The sandbox's set-up was not done: the lawyer declined the administrator prompt, or Windows refused to raise it; asking again can succeed. | yes | host (the set-up's administrator prompt, decision record 0051) |
| `sandbox.setup_failed` | The sandbox's set-up, or the check that the sandbox works, failed; trying again can succeed. | yes | host (the set-up or its check, decision record 0051) |
| `sandbox.setup_required` | The firm requires the sandbox (sandbox.required) and this computer needs its one-time set-up first (not done, or its administrator prompt declined), so no agent was started; running the set-up fixes it. | no | host (`CreateThread` or a turn's launch under `sandbox.required`, cause `needs_set_up` or `set_up_refused`, decision record 0051) |
| `sandbox.required` | The firm requires the sandbox (sandbox.required) and this computer cannot give it for a reason the set-up does not fix (no sandbox here, a failed set-up, a full pool, protection switched off, or a newer cause), so no agent was started. | no | host (`CreateThread` or a turn's launch under `sandbox.required`, any other cause, decision record 0051) |
| `matter.folder_not_a_matter` | The matter's folder is there, but its Clauseo file (`.clauseo/matter.json`) is missing, unreadable or belongs to another matter: locate the matter's folder. | no | matters (`Error::FolderMissing` with reason `NoMatterJson`, `Unreadable` or `OtherMatter`: open, locate, relink, every checkpoint), desktop (`open_file` and `reveal` for a folder without its marker) |
| `sandbox.blocked` | A Windows setting or security tool blocks the sandbox (its set-up program is missing or was blocked, say), so no agent was started; whoever looks after this computer can fix it. | no | sandbox (`Error::Blocked`: tier B's launch check refused the set-up program's admin copy, `Blocked::AdminCopyMissing`, `AdminCopyChanged` or `AdminCopyUnsafe`, or Windows security blocked or removed it, `ToolMissing`; decision record 0049) |
| `blocklist.override_off` | Allowing a site on Clauseo's unsafe-site list is turned off (network.allow_unsafe_site_override is false, usually by the firm's policy), so nothing was allowed. | no | host (`AllowBlockedSite`, decision record 0053); new area |
| `blocklist.notice_expired` | The blocked-site notice an allow names is one the host no longer keeps (too old, or from before a restart), so nothing was allowed: allow the site from a new notice. | no | host (`AllowBlockedSite` naming a `block_id` it no longer keeps, decision record 0053); not retryable because the same `block_id` stays unknown, and only a new notice can be allowed |
| `blocklist.allow_not_saved` | The allow applies until Clauseo closes, but Clauseo couldn't save it (the OS secret store or the data folder failed), so the site is blocked again after a restart. | no | host (`AllowBlockedSite`, decision record 0053); not retryable because the run keeps changes in memory only from then on |
| `blocklist.revoke_not_saved` | The site is blocked again until Clauseo closes, but Clauseo couldn't save that or void the saved allows, so the site may be allowed again after a restart. | no | host (`RevokeAllowedSite`, decision record 0053); not retryable because the run keeps changes in memory only from then on |
| `sandbox.cleaning_up` | An earlier Safe workspace session's clean-up (its unclean-session marker's stop or sweep) wasn't finished after Clauseo waited for it, so no agent was started; trying again in a moment can succeed. | yes | sandbox (`Error::UncleanSession`: tier B's launch check found an earlier session's unclean-session marker whose stop or sweep didn't complete within 15 s, decision record 0049); retryable because a later launch checks again once the clean-up is done |
| `agent.install_setup_failed` | The agent's files all downloaded and checked, but setting them up on this computer failed after three tries (unpacking, `npm ci`, the move into place or its receipt), often because security software held the fresh files. Trying again may work. | yes | agents (`FailureKind::SetupFailed` on `Failed`), desktop (0048) |
| `blackline.no_word_tool` | The Word blackline needs stetdoc, Clauseo's Word tool, and this Clauseo has none, so nothing was compared. | no | host (`HostQuery::WordBlackline`, the Word blackline); new area |
| `blackline.not_word` | The file is not a Word document (.docx), so it has no Word blackline. | no | host (`HostQuery::WordBlackline`) |
| `blackline.failed` | The Word blackline failed: the compare or its view failed, took too long, or printed what Clauseo could not read; nothing is shown and nothing was written in the matter. Trying again can succeed. | yes | host (`HostQuery::WordBlackline`); retryable because a run that timed out or failed can work the next time |
| `word.stetdoc_missing` | Clauseo has no stetdoc program to read or edit Word files with, or could not start it. | no | host (`ListRevisions`, `DecideRevisions`: no stetdoc program configured, or it could not be started) |
| `word.revisions_stale` | The Word file changed since its tracked changes were listed, or a change named is no longer in it, so nothing was written: list them again. | no | host and matters (`DecideRevisions`: the file's bytes differ from the list's `version`, or stetdoc answers `not_found` or a `stale_*` code) |
| `word.not_docx` | The file is not a Word document (.docx) that stetdoc can read. | no | host (`ListRevisions`, `DecideRevisions`: the path does not end in `.docx`, or stetdoc cannot read the file) |
| `word.edit_failed` | stetdoc failed or did not finish in time while reading or editing the Word file, or the file is not on this computer or is too large to version, so nothing was written. | yes | host (`ListRevisions`, `DecideRevisions`: stetdoc timed out, exited with another error, or printed output that does not read; the file is cloud-only or over the checkpoint size cap); retryable because a passing failure (a slow disk, a busy machine) can clear |
| `matter.folder_permission` | The computer's privacy settings keep Clauseo, or the agent it starts, from opening the matter's folder (on macOS, Files and Folders in Privacy & Security), so no agent works in it: allow Clauseo there, then ask again. | no | host (a launch's folder check on macOS: the host's own open of the matter folder is refused, EPERM or EACCES, or is still unanswered after two minutes, the prompt macOS shows left up or a volume that hangs; and on every OS an agent's `session/new` answered with JSON-RPC -32602 saying its `cwd` does not exist, after the host found the folder there) |
| `agent.offline` | The agent lost its connection while answering and the computer is offline: the window said offline and the host's own probe of the agent's service did not get through either. The answer so far is kept; asking again can work once the computer is back online. | yes | host (a turn whose prompt failed with a connection-class error, or whose agent said its connection failed or went quiet, while the window says offline and the probe misses; `actor/turn/offline/hold.rs`) |
| `matter.folder_in_clauseo_data` | The folder is, holds or sits inside Clauseo's data folder; one that is or sits inside the matters' history folder in it is `matter.folder_in_matters_root`. | no | host (from matters' refusal on create, import, the folder scan and the folder query; folder PR 2) |
| `matter.folder_inside_matter` | The folder sits inside a matter's folder; the error's `matter` names that matter when it is on Clauseo's list. | no | host (the registry check on create, import, the folder scan and the folder query; folder PR 2) |
| `matter.folder_holds_matter` | The folder holds a matter's folder; the error's `matter` names that matter when it is on Clauseo's list. | no | host (the registry check on create, import, the folder scan and the folder query; folder PR 2) |
| `prompt.too_long` | The prompt's text is over `MAX_PROMPT_TEXT_BYTES` (6 MiB, counted as JSON-escaped bytes), in a `SendPrompt` or an `EditQueuedPrompt`, so nothing was sent; the draft is kept. A `ChatGPT` prompt over Codex's own input limit (1,048,576 characters, as the host counts them) gets it too: refused the same way when sent or edited, or, when the undo note added as it goes out takes it over, its turn fails with this code before anything is sent, the queue held. Only the prompt's text gets this code: every other size limit stays `command.invalid`. | no | `HostError::invalid_command` for `Error::TooLarge` of `Measure::PromptText` (desktop, transport_ws, cli); the host's Codex check (`actor/turn/codex_cap.rs`, on `SendPrompt`, `EditQueuedPrompt` and the prompt as it goes out); the composer's own check before sending |
| `mcp.name_empty` | The MCP server has no name. | no | host (`HostCommand::Mcp` `SaveServer`) |
| `mcp.name_taken` | Another of the lawyer's MCP servers already has this name. | no | host (`SaveServer`: an add, or a rename onto another server) |
| `mcp.name_reserved` | The MCP server name is kept for one of Clauseo's own tools (`browser`, `case-law`) or Claude Code's built-ins (`ide`, `claude-in-chrome`, `computer-use`), in any case. | no | host (`SaveServer`) |
| `mcp.name_invalid` | The MCP server name is not 1 to 64 letters, digits, `_` or `-`, or holds `__`, or ends in `_`. | no | host (`SaveServer`) |
| `mcp.command_invalid` | The local MCP server's program is neither a bare name (`npx`) nor an absolute path on this computer: a relative path, a network path, or empty. | no | host (`SaveServer` of a local server) |
| `mcp.command_not_found` | The local MCP server's program is not on this computer: a bare name found on no search path, or an absolute path with no file. The program is the one the command named. | no | host (`SaveServer` of a local server: the program resolved off the actor, as a launch resolves it) |
| `mcp.url_invalid` | The remote MCP server's URL is not an `https` URL with a host (`http` only for `localhost` or `127.0.0.1`), or holds a user name or password. | no | host (`SaveServer` of a remote server) |
| `mcp.header_name_invalid` | A header name is not an HTTP token, or appears twice in any case. | no | host (`SaveServer` of a remote server) |
| `mcp.env_name_invalid` | A variable name is not a letter or `_` followed by letters, digits or `_`, or appears twice. | no | host (`SaveServer` of a local server) |
| `mcp.secret_store_failed` | The OS keychain refused or failed to save a value, so the MCP server was not changed. | yes | host (`SaveServer`: a keychain write or read failed, or no keychain on this computer; the keychain is put back and the settings stay); retryable because a locked keychain or a denied prompt can clear |
| `mcp.server_unknown` | The lawyer has no MCP server of that name. | no | host (`SaveServer` with a `previous_name`, `SetServerEnabled`) |
| `mcp.save_declined` | The lawyer declined the native confirm before an MCP server was added or its program, arguments, variable names or URL changed, so nothing was saved. | no | host (`SaveServer` that adds a server or changes what starts: desktop's confirm answered Cancel, or no confirm is wired; settings and keychain untouched) |
| `agent.service_error` | The agent's service had a problem on its side and the agent gave up on the turn: an AIR `sessionFailure` of severity `error` offering `retry` or `new_session`, in a category no other code reads (codex-acp's `internal_error`, `provider_error`, `overloaded`). The answer so far is kept; asking again can work. | yes | host (a prompt whose `end_turn` carries such a failure, `PromptEnd::failed`; `actor/turn/gave_up.rs`); retryable because the agent itself offers `retry`. A JSON-RPC error to `session/prompt` is `agent.answer_failed` since 2026-11-15 (it was `agent.protocol_error`, which could not tell it from a malformed reply) |
| `agent.quiet` | The agent sent nothing for 5 minutes while answering, with no tool running and no question waiting for the lawyer, so the host ended the turn (D-050's backstop: a blocked network the host's probe could not see, such as a proxy that asks for credentials or one that inspects TLS); or the agent's own request to its service timed out while the host's probe reached that service (a slow service). The answer so far is kept; asking again can work. | yes | host (a turn the agent sent nothing on for 5 minutes, with no tool running and no question waiting; the D-050 backstop in `actor/turn/offline/quiet.rs`; or a prompt that failed on "Request timed out" while the probe got through, `actor/turn/offline/hold.rs`); retryable because the agent may answer once the blocked network clears |
| `mcp.import_too_large` | The servers an import would add, with their programs, arguments, URLs and variable or header names, are too long to show whole in one confirm (over 2,000 characters), so nothing was imported. | no | host (the MCP import, when its servers are too long for one confirm; settings and keychain untouched) |
| `matter.folder_backslash` | The matter folder's path has a backslash (`\`) in a folder's name, on macOS or Linux, where the agent cannot start in such a folder (Claude Code exits with "Can't access working directory"), so the host did not start it. Renaming the folder fixes it. | no | host (a launch's preparation, `agent/folder_name.rs`: the folder the agent would start in has a `\` in its path, for the kinds `REFUSES_BACKSLASH` lists, Claude only; never on Windows, where `\` is the separator). Not retryable: the folder is renamed first |
| `agent.folder_inaccessible` | The agent could not open the matter folder as its working folder when it started (Claude Code's "Can't access working directory" in its answer to a startup request), though the host found the folder there just before. Asking again can work once the folder is back where the matter expects it. | yes | host (a launch whose startup request was answered with an error whose message or `data` says "Can't access working directory", claude-agent-acp's -32603 with `data.details` holding Claude Code's stderr; `agent/folder_access.rs`). Before, it read as `agent.protocol_error` |
| `blackline.nested_table` | stetdoc could not compare the two versions because of a table inside a table cell (its compare refused the redline it made, `validation_failed`, over a cell), so nothing is shown; trying again gives the same answer. | no | host (`HostQuery::WordBlackline`: stetdoc 0.9.3's compare exits 5 when an inner table that ends its cell is removed; `blackline/run.rs` reads the code and its fixed wording) |
| `blackline.cannot_compare` | stetdoc refused to compare the two versions (`validation_failed` or `unsupported`) for a reason other than a table inside a table, so nothing is shown; trying again gives the same answer. | no | host (`HostQuery::WordBlackline`) |
| `blackline.not_readable` | stetdoc could not read one of the two versions as a Word document (`malformed_input`, `corrupt_complement` or `unsupported_version`), so nothing is shown. | no | host (`HostQuery::WordBlackline`) |
| `blackline.too_large` | One of the two versions is past a size limit of stetdoc's (`limit_exceeded`), so nothing is shown. | no | host (`HostQuery::WordBlackline`) |
| `blackline.format_unknown` | stetdoc's view of the blackline is in a format this Clauseo does not know (a header other than `stet 1`, or none), so nothing is shown rather than a guess. | no | host (`HostQuery::WordBlackline`: the view's header names another format, such as a newer stetdoc's `stet 2`) |
| `agent.unreachable` | The agent could not get through to its service while answering, and the computer did not seem offline: the service, or a proxy or firewall in front of it, refused, timed out or blocked the connection, or its name did not resolve. The answer so far is kept; asking again can work. | yes | host (a connection failure while the window says online, or whose probe got through: a watch whose probe missed, a failed prompt naming a connection failure, Codex giving up on its connection; `actor/turn/offline/hold.rs`); the detail carries the probe's `class`, `via` and `status` for firm IT |
| `word.too_large` | The Word file is over the matters' size cap (512,000,000 bytes) or one of stetdoc's limits (its `limit_exceeded`: the package's size, entry count or compression ratio, an XML part's size, depth or node count), so its tracked changes were not read or decided. | no | host (`ListRevisions`, `DecideRevisions`: the file is over `FILE_CAP_BYTES`, or stetdoc answers `limit_exceeded`, exit 6); not retryable because the same file meets the same limit |
| `agent.start_offline` | The agent did not start: it answered a start request with an error naming a connection failure, and the host's probe of its service did not get through. Asking again can work once back online. | yes | host (a launch whose start request failed with an error naming a connection failure, confirmed by the offline probe; `actor/turn/offline/start.rs`); an error to a start request with no such confirmation is `agent.launch_failed`, never `agent.protocol_error` |
| `agent.answer_failed` | The agent answered the question with an error no other code reads, or Codex gave up on a connection the host does not read as offline. The answer so far is kept; asking again can work. | yes | acp (`Error::code` for an `Error::Rpc` to `session/prompt`; an answer that does not decode is `Error::Protocol`, `agent.protocol_error`, since 2026-11-15) and host (`offline/ended.rs::gave_up_code`); before 2026-11-15 these were `agent.protocol_error` |
| `agent.mode_missing` | The session offered no mode Clauseo runs the agent in, so nothing was sent: an unsupported agent version. | no | host (`actor/turn/session.rs` for Codex, `session/claude_mode.rs::no_mode` for Claude); before 2026-11-15 `agent.protocol_error` |
| `agent.setting_refused` | The agent refused the lawyer's own mode or setting change on a running session; nothing changed. | yes | host (`actor/turn/answers.rs::answered`, an `Error::Rpc` to `SetMode`'s `session/set_mode` or `SetConfigOption`'s `session/set_config_option`; the detail names the method and code, never the agent's text); before 2026-11-15 `agent.protocol_error`. A refused switch during a start is `agent.launch_failed` |
| `agent.program_blocked` | The computer refused to run the agent's program (permission, Windows policy or antivirus). | no | process (`Error::code` for an `Error::Spawn` the OS answered permission-denied (EACCES, EPERM; Windows `ERROR_ACCESS_DENIED`), or on Windows `ERROR_ACCESS_DISABLED_BY_POLICY` (1260), `ERROR_VIRUS_INFECTED` (225), `ERROR_VIRUS_DELETED` (226)); before 2026-11-16 `agent.launch_failed`. The host spawns Node (with the adapter's script) or a custom agent's command, so it reads Node's or that command's refusal; a binary the adapter starts is the adapter's to report |
| `agent.program_missing` | The OS found no agent program to run (not found), so the agent did not start. | no | process (`Error::code` for an `Error::Spawn` the OS answered not found: ENOENT; Windows `ERROR_FILE_NOT_FOUND`, `ERROR_PATH_NOT_FOUND`); before 2026-11-16 `agent.launch_failed`. Two codes, not one with a detail, because the app's words and what the lawyer does differ (reopen Clauseo to reinstall, against allow it in the antivirus) and a client reads codes, never details |
| `skill.no_skill_md` | The skill to add has no `SKILL.md`: the path is neither a folder holding one nor a `SKILL.md` file; nothing was added. | no | host (`AddSkill`, decision record 0040) |
| `skill.invalid` | The skill to add could not be made a skill pack: it could not be read, it is over 5,000,000 bytes or 200 files, or it breaks a skill pack rule (no name or description in its `SKILL.md`, a script while scripts are off, and the like); nothing was added. | no | host (`AddSkill`; the log line holds the rule's code) |
| `matter.file_folder_clash` | A restore of chosen files names a file whose name is now a folder, or one of whose folders is now a file, so nothing was restored or written. | no | matters (a `RestoreVersion` or `PreviewRestore` with a `files` scope, naming the entry in the way, then the file, in `HostError::files`; a whole-matter restore keeps such a file instead, and is refused only for one that clashes after its plan) |
| `agent.home_failed` | Clauseo could not prepare the agent's own folder under its data folder (its home, or a folder of its session), so the agent was not started. Asking again can work. | yes | host (a Codex or Claude home under `<data>/agents/<id>/home` that can't be made, made private, or cleared of Codex's `rules`, or whose `config.toml` can't be written, `agent/codex.rs` `agent_home_failed`; a session's `run`, temporary or notes folder that can't be made, or a work or notes folder that can't be readied, `actor/turn/launch_checks/folders.rs` `make` and `ready_own`). Before, it read as `agent.launch_failed`; a process that does not spawn stays that code |
| `matter.file_refused` | Files the operation would change refused it (their permissions, a second name, or a saved copy that could not be checked); everything else was done. | yes | matters, host |
| `matter.file_write_failed` | Writing a file in place failed partway, so the operation stopped there; the file was put back as it was, or may be damaged, and the version before the operation holds it. | yes | matters, host |
| `agent.key_denied` | A stored API key could not be read: the system's key store answered the read with an error (often a permission prompt that was denied or cancelled, or a locked key store); no agent was started. Trying again, and allowing the read, can succeed. | yes | host (a launch for an agent whose stored API key's read failed with the key store's error, decision record 0048; desktop's sign-in state and refusal carry it too) |
| `agent.start_unreachable` | The agent did not start: its start failed on its connection, and the host's probe could not reach its service while the window says the computer is online (a slow network, a proxy or a firewall). Asking again can work. | yes | host (`offline/start.rs::start_code`: a start whose error names a connection failure, or that went silent past its bound, probed while the window says online, and the probe missed; the offline truth rule of `offline/hold.rs`). Before 2026-11-17 such a start was `agent.start_offline` |
| `unknown` | A code from a newer Clauseo that this version does not know. Never produced here. | no | decoding only |

Judgement calls on retryable: `agent.launch_failed` and `agent.exited` are retryable because a new
launch can work; `store.locked` is not, because the lawyer must close the other Clauseo first;
`matter.checkpoint_failed` is, because the usual cause (another program holding the repository lock)
passes; `matter.turn_running` is, as `thread.turn_running` is; `agent.home_failed` is, as
`agent.launch_failed` is, because a folder another program held or scanned at the launch can be
free at the next one.

`matter.folder_missing` became retryable, and its meaning gained "or did not answer in time", on
2026-11-07 for #617, at the orchestrator's request. The widened meaning clarifies "its drive is
not mounted": a folder on a drive or network share that is not there, or does not answer within
the host's 10 s bound, comes back once the drive or share does, and the same call then works. It
changes a code's meaning, which the rule above forbids, but no peer of another version decodes
this code: the host and the app ship in one bundle.

`matter.folder_not_a_matter` was added on 2026-11-07, at the orchestrator's request. Until then a
folder that is there but whose `.clauseo/matter.json` is missing, unreadable or names another
matter (a relink that could not repair it included) came back as `matter.folder_missing`, whose
meaning ("the matter's folder is gone") never covered it. Those cases now have their own code, not
retryable: the lawyer has to locate the right folder. `matter.folder_missing` keeps its meaning and
now comes only for a folder that is not there or does not answer.

`agent.install_setup_failed` was added on 2026-11-07, at the orchestrator's request (the install
resume PR, after the Windows VM pass of #683 on #664): a Claude install with Defender on downloaded
every file, then failed all three tries at the rename into place with `Access is denied`, and said
the agent "couldn't be downloaded". A failure after every file is fetched and checked now has its own
code, so the words can say the download worked. It is retryable, as `agent.install_failed` is: the
antivirus that holds the fresh files lets go, and the next try's set-up may work. `agent.install_failed`
keeps its meaning, now only for failures before the files are all fetched.

`matter.folder_permission` was added on 2026-11-10, at the orchestrator's request (a launch
blocker on a Mac: a matter under `~/Documents`, where every Claude conversation failed as
`agent.protocol_error`). macOS privacy protection (TCC) keeps an app without the lawyer's consent
out of Documents, Desktop, Downloads, removable and network volumes and iCloud Drive, and the
agents Clauseo starts inherit Clauseo's consent. Claude's adapter answers `session/new` in such a
folder with -32602 "`cwd` does not exist", which said nothing the lawyer could act on. The host
now opens the folder itself before a launch on macOS and refuses with this code, and maps that one
agent answer to it too. The open waits while macOS asks the lawyer, for up to two minutes; one
unanswered by then is this code too, never `matter.folder_missing`. Not retryable: the lawyer changes a privacy setting first, then asks again.

`matter.folder_backslash` and `agent.folder_inaccessible` were added on 2026-11-15, at the
orchestrator's request (a launch blocker on a Mac: a matter whose folder's name held literal
backslashes, `Clauseo\ Test\ Matter`, where every Claude conversation failed as
`agent.protocol_error`, "Claude sent something Clauseo can't read"). Claude Code 2.1.293 exits at
start in any working folder whose path has a `\` ("Can't access working directory …: Path …
does not exist"), and claude-agent-acp answers `session/new` with -32603 "Internal error",
Claude Code's stderr in `data.details`. Codex works in such a folder (RC2, tested by hand), so
only Claude is refused; listing Codex in `REFUSES_BACKSLASH` refuses it too. The host now
refuses a Claude launch in such a folder before it starts, and maps any startup answer saying
"Can't access working directory" to `agent.folder_inaccessible`. New matter and Locate warn
before such a folder becomes a matter's.

`matter.folder_in_clauseo_data`, `matter.folder_inside_matter` and `matter.folder_holds_matter`
were added on 2026-11-11 (folder PR 2), so the app can say a refused folder's reason in plain
words. `matter.folder_in_matters_root` and `matter.folder_is_matter` keep their meanings; the host
now answers `matter.folder_in_clauseo_data` for the data folder outside the matters' history
folder, and `matter.folder_inside_matter` or `matter.folder_holds_matter` for a folder nested with
another matter's, keeping `matter.folder_is_matter` for the same folder. The folder scan answers
the same codes as the import (before, it answered `matter.folder_refused`). An older app reads the
new codes as `unknown`, which it shows as a generic failure.

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
