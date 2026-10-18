# 0001: The Rust-to-webview boundary

## Context

The React UI runs in the system webview and renders text Clauseo does not control: agent output,
document contents, tool results. An XSS there gives an attacker script in the page. The host behind
the page has file and process access. Tauri's access control alone does not stop a local page: with
no `AppManifest`, every app command is callable with no check. A confirm drawn in the webview stops
nobody who already has script in the page. Tauri's `Channel::send` is unbounded, and one large
message that fails stalls its ordered queue.

## Decision

- **Rust-side validation of every command is the boundary.** `desktop` deserialises each command
  and checks its types, opaque ids and allowed values before it reaches the host.
- `build.rs` declares an `AppManifest`, so app commands go through the access check, and each
  capability lists its permissions explicitly (never `core:default`).
- The command surface stays minimal: send a command to the host, subscribe, acknowledge, answer a
  permission, pick a folder, open a file or link.
- Files are named by opaque ids the host issues, never by paths. Paths reach the UI for display
  only.
- The host sends view patches (`Upsert`, `Remove`, `AppendText`, `Reset`), each with a per-thread
  cursor. The webview applies only the next cursor, so replays and duplicates are no-ops.
- One `ipc::Channel` per window carries all its threads. `subscribe(thread_id, after_cursor)`
  returns the tail or a `Reset`, and is the one recovery path after a reload, crash or stall.
- Every message is split below 8 KB.
- Backpressure: the webview acknowledges the cursors it applied. The host caps unacknowledged bytes
  per window at 256 KB and coalesces harder past it. A 5 s watchdog with no progress forces a
  resubscribe.
- Every command returns `Result<T, HostError { code, message, retryable }>`. The UI shows copy
  chosen by `code`, never a raw Rust string.
- Escalations go only through native dialogs drawn by Rust: sandbox off, full access, "always
  allow", paths outside the matter, new domains, adding agents or MCP servers, external links.
- The webview never navigates. External links go through one `open_external`.
- A strict CSP: no remote images or connections, no `unsafe-eval`, Trusted Types.
- Release builds have no devtools, no global Tauri object and no asset protocol.

## Consequences

- An XSS reaches only a handful of narrow, typed commands, never file or process access.
- A new command is denied until a capability grants it, so every widening shows up in review.
- A new command, a capability, a CSP change or a change to the patch protocol needs a new record.
- Large views reach the UI in pieces, and the host must track acknowledgements per window.
- See [the threat model](../threat-model.md) for which parts are built.

## Status

accepted

## Date

2026-10-19
