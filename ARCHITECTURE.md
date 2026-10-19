# Architecture

Clauseo runs coding agents (Claude Code, Codex) for lawyers, on the lawyer's machine, inside an
OS sandbox, with every matter versioned so any step can be undone. This file is the map. Detail lives
in each crate's `//!` contract. Keep this file to at most 300 lines.

## Bird's-eye view

```
Clauseo app process (Rust)
  Tauri main thread: window, native dialogs (never blocked)
  desktop crate (tokio): async commands --> async-channel --> host thread
       ^ one ipc::Channel per window (16 ms batches, messages under 8 KB, acknowledged)
  host thread: seq, commands, permissions, subscriptions, supervisor
    store writer thread (SQLite)        matters: shadow git repos (gix)
    egress proxy (allowlist)            sign-in runs (outside the sandbox)
        |                                   |
        | system webview                    | ACP over stdio
        v                                   v
  React UI (thin patch store)     sandbox (Seatbelt / bubblewrap / MXC or sandbox user)
                                    Node adapter (claude-agent-acp / codex-acp)
                                      Claude Code binary / Codex app-server
                                    reads and writes only the matter folder; network only via the proxy
                                    clauseo-mcp sidecar, reaching the host's broker over a local socket
```

## Event flow

1. The UI calls one async command (send, subscribe, acknowledge, answer a permission, pick a folder,
   open a file or link). Files are named by opaque ids, never paths.
2. `desktop` deserialises and validates the command in Rust (types, opaque ids, allowed values), then
   forwards it over `async-channel` to the host thread. Rust-side validation is the security boundary.
3. The host checks it against current state, assigns the next gapless per-thread `seq`, and commits the event to `store`
   before acting or publishing. Text deltas are the exception: they reach the screen in 16 ms batches
   and disk in batches of at most 250 ms.
4. `reducer` folds events into thread state and view patches (`Upsert`, `Remove`, `AppendText`,
   `Reset`), each with a per-thread `cursor`.
5. The host sends patches to each subscribed window over its Channel. The webview applies only the
   next cursor, so replays and duplicates are no-ops, and acknowledges what it applied.
6. Agent traffic goes the other way: ACP over stdio into `acp`, then to the host as events.
   `matters` checkpoints the matter folder before and after every turn.
7. A reload, renderer crash or stall recovers through `subscribe(thread_id, after_cursor)`, which
   returns the tail or a `Reset` with a view snapshot.

## Crates

Rust crates live in `crates/<name>`, package `clauseo-<name>`; `xtask` keeps its own folder and
name. The frontend is a pnpm workspace under `ui/`. `cargo xtask check` reads the table below and
fails if a crate depends on a crate in its "Never depends on" column, directly or through another
crate. Rows marked `planned` are checked once the crate exists. Every crate that exists
must have a row.

A dependency counts when it is a normal or build dependency, on any target, with every feature of
the crate itself turned on; dev dependencies never count. Each crate's own graph is resolved, so a
feature that another workspace crate turns on does not count. A dependency's own build
dependencies count too: a crate whose dependency uses tokio only in its `build.rs` fails a tokio
ban.

A "Never depends on" entry that is a row's name bans that crate; any other name means the
crates.io package of that name. A name that starts with `clauseo-`, holds a `_` or is one edit
from a row's name (ignoring case; a swap of two adjacent letters is one edit) is rejected as a
likely typo, except a row's own package name: "process" in the column means the standard
library's API (below), so `clauseo-process` is how a row bans the process crate. To ban a
crates.io package that such a name would reject, mark it: `rmcp (crates.io)`. A ban names exactly
one package. Banning a family means listing each one: `tauri` does not cover `tauri-runtime`,
`tauri-utils` or `wry`.

| Crate | Owns | May depend on | Never depends on | Status |
|---|---|---|---|---|
| `xtask` | check, ci, buildtime, budgets, doc checks | none of ours | tauri | exists |
| `desktop` | Tauri shell: fixed plugin set, AppManifest, capabilities; later commands and the Channel bridge | host, settings, tauri | | exists |
| `protocol` | Event envelope, domain events, commands, view patches, ids, error codes | serde, uuid, specta | tauri, tokio, fs, process | planned |
| `reducer` | Pure fold from events to thread state and view patches | protocol | any I/O, clock, tauri | planned |
| `store` | SQLite log, snapshots, blobs, receipts, writer thread | protocol, rusqlite | tauri | planned |
| `matters` | Shadow repos via gix: checkpoints, undo, restore, compare, retention, file watching | protocol, gix, notify | tauri | planned |
| `settings` | Settings schema and locations, the policy layer | serde, schemars | tauri | planned |
| `process` | Spawning, graceful shutdown, process-tree kill, restart supervisor, the `Launcher` trait | protocol, async-process | tauri | planned |
| `acp` | Wrapper over `agent-client-protocol` | protocol, agent-client-protocol | tauri, tokio | planned |
| `host` | Own thread and executor: sessions, commands, permissions, subscriptions, checkpoints per turn | protocol, reducer, store, matters, process, acp, settings | tauri, tokio, agents | planned |
| `fake_agent` | Scripted ACP agent for tests | agent-client-protocol | | planned |
| `cli` | Headless client for agents and CI | host, settings | tauri | planned |
| `transport_ws` | Dev and test only WebSocket transport, random token | host | tauri | planned |
| `sandbox` | Policy model, tier detection, per-OS launchers, per-matter agent state folders, egress proxy, denial events | protocol, process | tauri | planned |
| `agents` | Agent registry, Node runtime, silent installs, sign-in runs, idle stop and resume | protocol, process, sandbox, settings | tauri | planned |
| `profile` | Clauseo profile translated into each agent's native options and files | protocol, settings | tauri | planned |
| `legal` | Case-law and regulator data clients and parsers | protocol | tauri | planned |
| `citations` | Deterministic citation parsing and checks | protocol, legal | tauri | planned |
| `skills` | Skill format, built-in skills, validation, install through `profile` | protocol, profile | tauri | planned |
| `mcp` | The `clauseo-mcp` sidecar: research tools, Word tracked changes, citation checks, views | legal, citations, stetdoc-core, rmcp | tauri | planned |

The broker (keys, cache and limits for outside services, plus the socket the sandboxed sidecar
reaches) is a module of `host`, not a crate. Frontend packages: `ui/app` (exists),
`ui/components` (planned) and `ui/bindings` (planned). Nothing in `ui/` imports Tauri APIs outside
`ui/app/src/transport/`.

"fs", "process", "any I/O" and "clock" in the "Never depends on" column mean the standard
library's APIs, not the `process` crate, and cover the crate's own code. `cargo xtask check`
parses every `.rs` file under the crate's `src/` with `syn`. "fs", "process" or "any I/O" bans
the file, network, process and environment APIs and the functions that read the clock; "clock"
alone bans only the functions that read the clock. The banned paths are the paths in
`xtask/src/checks/core_io/bans.rs`. Exceptions, one path per line with a reason, live in `xtask/core-io-exceptions.txt`.

## Seams left open on purpose

1. The host serves subscribers by `seq`. The webview is one subscriber; a phone client can be another.
2. `session/new` always sends an `mcpServers` list (empty until `clauseo-mcp` exists).
3. Tool calls render by tool name, with a generic card as the fallback.
4. Every call carries a `principal` (only `LocalDesktop` today).
5. Large payloads go to the content-addressed blob store.
6. The policy layer is read at start-up even before any policy exists.
7. All frontend access to the host goes through one `HostTransport` interface.
8. Agent endpoints are configurable.
9. `host` has a hook at `session/new` where `profile` injects per-agent options.
10. `process` launches agents through a `Launcher` trait: a passthrough launcher first, sandbox
    launchers later.

## Invariants

- The append-only event log is the source of truth. Ids are stable; positions are never ids.
- `reducer` is pure: no I/O, no clock, no randomness. Time comes in as data.
- Every Tauri command is async and returns `Result<T, HostError { code, message, retryable }>`.
  The UI shows copy chosen by `code`, never a raw Rust string.
- The webview never navigates. External links go through one `open_external`.
- Clauseo never reads an agent's credentials. Sign-in runs outside the sandbox; the agent runs inside.
- Clauseo's version store is outside the matter folder and invisible to agents.

## Decision records

Changes to the event schema, error codes, a crate's public interface, the dependency table above or
the Rust-to-webview boundary need a record in `docs/decisions/`. Anyone who adds a Tauri command, or
anything else the webview can reach, reads `docs/decisions/0001-rust-webview-boundary.md` and
`docs/threat-model.md` first.
