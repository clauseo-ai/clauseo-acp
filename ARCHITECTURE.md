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
3. The host checks it against current state, assigns the next gapless `seq` in the event's stream
   (a thread's or a matter's), and commits the event to `store` before acting or publishing. Text
   deltas are the exception: they reach the screen in 16 ms batches
   and disk in batches of at most 250 ms.
4. `reducer` folds events into thread and matter state and view patches (`Upsert`, `Remove`,
   `AppendText`, `Reset`), each with a per-stream `cursor`.
5. The host sends patches to each subscribed window over its Channel. The webview applies only the
   next cursor of each stream, so replays and duplicates are no-ops, and acknowledges what it applied
   (`Ack { stream, epoch, cursor }`).
6. Agent traffic goes the other way: ACP over stdio into `acp`, then to the host as events.
   `matters` checkpoints the matter folder before and after every turn.
7. A reload, renderer crash or stall recovers per stream through `Subscribe { stream, after }`
   (`after` is the last epoch and cursor), which returns the tail or a `Reset` carrying a
   `ViewWindow`.
   `SubscribeEvents { stream, after_seq }` serves envelopes by `seq` instead.

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

A "Never depends on" entry that is a row's name bans that crate; any other name bans the
crates.io package of that name. The I/O words (below) win over a row of the same name, so
`clauseo-process` is how a row bans the process crate. One name bans one package: `tauri` does
not cover `tauri-runtime`, `tauri-utils` or `wry`, so banning a family means listing each one.
The check rejects an entry it could misread, such as a likely typo of one of ours, and says what
to write instead.

| Crate | Owns | May depend on | Never depends on | Status |
|---|---|---|---|---|
| `xtask` | check, ci, buildtime, budgets, doc checks | none of ours | tauri | exists |
| `desktop` | Tauri shell: fixed plugin set, AppManifest, capabilities; later commands and the Channel bridge | host, settings, tauri | | exists |
| `protocol` | Event envelope, domain events, commands, queries, view patches, account limits, ids, error codes | serde, serde_json, uuid, compact_str, thiserror, specta, specta-serde | tauri, tokio, fs, process | exists |
| `reducer` | Pure fold from events to thread and matter state and view patches; the snapshot format (`MessagePack`, `REDUCER_SCHEMA_VERSION`) | protocol, serde, rmp-serde | any I/O, clock, tauri | exists |
| `reducer_bench` | The gungraun instruction-count bench of `reducer`'s fold (Linux, CI) | none (dev: protocol, reducer, gungraun) | tauri | exists |
| `store` | SQLite log, snapshots, blobs, receipts, writer thread | protocol, rusqlite, serde, serde_json, rmp-serde, blake3, zstd, async-channel, futures | tauri | exists |
| `matters` | Shadow repos via gix: create and import, checkpoints, undo, restore, compare, file watching; retention after G9 | protocol, gix, notify, serde, serde_json, tracing, dunce; nix (macOS only) | tauri | exists |
| `settings` | Settings schema and locations, the policy layer | serde, serde_json, serde_json_lenient, schemars; windows (Windows only); objc2-core-foundation (macOS only) | tauri | exists |
| `process` | Spawning, graceful shutdown, process-tree kill, restart supervisor, the `Launcher` trait | protocol, async-process, async-io, futures, memchr; windows (Windows only); nix (Unix only) | tauri | exists |
| `acp` | Wrapper over `agent-client-protocol` | protocol, agent-client-protocol, futures, async-channel, serde, serde_json, memchr | tauri, tokio | exists |
| `host` | Own thread and executor: sessions, commands, permissions, subscriptions, checkpoints per turn | protocol, reducer, store, matters, process, acp, settings | tauri, tokio, agents | planned |
| `fake_agent` | Scripted ACP agent for tests | agent-client-protocol, serde, serde_json | | exists |
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
`ui/components` (exists: the design system's tokens, icons, words and controls, which take data as
props and hold no host logic) and `ui/bindings` (planned). Nothing in `ui/` imports Tauri APIs outside
`ui/app/src/transport/`. `cargo xtask check` runs only ui/app's scripts, so ui/app checks
ui/components too: its tsconfig includes `../components/src`, its vitest config has a `components`
project, and oxlint, which refuses paths with `..`, reaches it through a symlink in ui/app named
components.
A version both packages need lives once, in the `catalog` of `ui/pnpm-workspace.yaml`.

"fs", "process", "any I/O" and "clock" in the "Never depends on" column mean the standard
library's APIs, not the `process` crate, and cover the crate's own code. "fs", "process" or "any
I/O" bans the file, network, process and environment APIs, the standard streams, and the
functions that read the disk or the clock, such as `Path::exists` and `Instant::elapsed`.
"clock" alone bans only the two clock `now` functions, which the root `clippy.toml` bans in every
crate. clippy enforces the bans after macro expansion. A crate whose row lists "fs", "process",
"any I/O" or "clock" starts every target root with
`#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]` and has no build script, so it
gets no exception to the bans. Its cfgs use only `test`, `doc` and positive features, it never
names `cfg_select`, and it has no `include!`. Its only `#[path]` attributes sit on a top-level
`mod name;` and name, as a string literal, a `.rs` file in the crate's folder. The rest of these
rules, the macro rules among them, are in `xtask/src/checks/core_io/cfgs.rs`. A crate whose row
lists "fs", "process" or "any I/O" also has its own `clippy.toml`, which `cargo xtask check`
prints when it is missing. Lint configuration lives only in the root `clippy.toml`, those crates'
own files and the root `[workspace.lints]`. The other rules `cargo xtask check` applies are in
`xtask/src/checks/core_io.rs`.

## Seams left open on purpose

1. The host serves subscribers by `seq`. The webview is one subscriber; a phone client can be another.
2. `session/new` always sends an `mcpServers` list (empty until `clauseo-mcp` exists).
3. Tool calls render by tool name, with a generic card as the fallback.
4. Every call has a `principal` (only `LocalDesktop` today). The surface that takes the call
   (desktop bridge, transport, cli) passes it to the host from its connection; it is never read
   from the client's message.
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
`docs/threat-model.md` first. Anyone who changes a type in `protocol` reads
`docs/decisions/0002-event-schema.md` first: it says which changes keep the log readable.
