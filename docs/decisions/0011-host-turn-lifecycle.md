# 0011: host turn lifecycle

## Context

`host` is the one thread that runs a lawyer's turns: it launches the agent, checkpoints the matter
around each turn, saves every event before anyone sees it, and serves the desktop window and later
the `cli` and `transport_ws`. Those callers build on its handle, so its shape must hold. Streaming
text to the screen, saving each event, versioning the matter after each turn and reopening a long
thread all run through it, so it must add nothing a lawyer can feel.

## Decision

### Handle and thread

- `Host::start(HostConfig)` returns a `HostHandle` (`Clone`, `Send`) and a `HostJoin`. The handle
  has `dispatch`, `query`, `subscribe`, `subscribe_events`, `ack`, `register_folder`,
  `register_file` and `shutdown`. Each call is one request over `async_channel::bounded(256)` with
  a `futures` oneshot for the reply (2,102 ns round trip against 3,897 ns with a `bounded(1)`
  reply, Apple M6). One host per data folder: a second start gets `store.locked`.
- One actor on `futures::executor::LocalPool` (QR11.2; `futures` 0.3.34 in `Cargo.lock`). Its loop
  awaits only its input channel; calls that wait run as spawned tasks and post results back.
  Against `async-executor` 1.14.0 it measured 26% faster per message and 17% faster per command
  round trip, at the cost of timers on async-io's own thread and worse tails, all well under the
  16 ms display batch.
- One `clauseo-matter` thread per open matter, started on first use. One `clauseo-snapshot`
  thread compresses snapshots; the actor encodes them from a borrow and never runs zstd.
- `subscribe` yields `EncodedEvent`: each `HostEvent` serialised once into a reused buffer, copied
  into an exact-size `Arc<str>` and shared by every subscriber of the stream (41% less time per
  16-patch batch than encoding per viewer). A batch is cut before it would reach 8 KB, the size
  under which Tauri's Channel takes its fast path. `subscribe_events` stays typed: the replay in
  pages of 500, one `CaughtUp`, then live envelopes.

### The turn

Each step is one commit on the thread stream unless it names the matter stream.

1. `UserMessage` and the receipt; `dispatch` answers `PromptAccepted`.
2. No live session: launch through `process::Launcher`, `initialize`, then `session/load` with the
   stored session id when the agent offers it, else `session/new` (with a "context not restored"
   `Notice` when the thread has turns). `AgentSessionAttached`. A failure ends the turn with
   `TurnFailed` and `TurnEnded { Failed }`, no `TurnStarted`.
3. The matter's writer held by another thread: `TurnQueued`, first in first out per matter.
4. Before-turn checkpoint, then `TurnStarted`. A checkpoint failure ends the turn with
   `TurnFailed` carrying the matter error's code, and nothing goes to the agent.
5. Once `TurnStarted` is durable, `session/prompt`, the undo note first. It ends on a stop
   reason, the cancel fallback or agent exit. A failed commit cancels the turn as `Cancel` does,
   and step 7 adds `TurnFailed` with the store error's code.
6. Flush display, then durable text; expire pending permission requests and elicitations.
7. After-turn checkpoint with the paths the turn's tool calls named. One commit: the matter's
   `CheckpointRecorded { AfterTurn }`, `FilesChanged` when not empty, `TurnFailed` on failure,
   `TurnEnded`. If a turn's last commit fails, it is retried once when the stream drains; until
   it is durable, `SendPrompt` on the thread is refused with `thread.turn_running`.
8. Thread snapshot; the writer passes to the next turn waiting at step 4.

When `matters` answers a checkpoint with `Reused` and the matter's log does not hold that id, the
host records it first (`CheckpointRecorded` with the kind, thread and turn from `matters`'s
timeline), at step 4, at step 7 and in start-up repair, so no logged event names an unrecorded
checkpoint.

Agent exit: expiries with `AgentExited`, step 7 with `TurnFailed { agent.exited }`, then
`AgentSessionDetached { Exited }` last.

The per-thread queue, send now and steer are decided in the brief and come with a follow-up. Until
then `SendPrompt` on a thread with a current turn is refused with `thread.turn_running`
(retryable), and the window disables Send while a turn runs.

The undo note (QR11.1) is not logged. Its words: "Note from Clauseo: since your last turn, the
lawyer put these files back to an earlier version: notes/a.txt, brief.txt. Read them again before
relying on what you remember about them." The paths are the undo and restore checkpoints' files
since the thread's last `TurnStarted`. An undo keeps the writer and leaves the note unchanged
until `StepUndone` is durable; a failed undo commit is retried once, as a turn's end is.

### Versions

- Every checkpoint gets its `CheckpointRow` (id, matter, git commit, the `seq` of its
  `CheckpointRecorded`) in the same commit as that event: the import, before-turn, after-turn,
  outside-edits, undo and restore checkpoints. A `Reused` checkpoint the host records late
  (at step 4, step 7, start-up repair or a restore that writes nothing) gets its row too, except
  when the matter's log could not be read: the host then cannot tell whether a row exists, and a
  second row of one id would fail the whole commit.
- `RestoreVersion` holds the matter's writer for the whole restore, and is refused with
  `matter.turn_running` while a turn or an undo holds it or turns wait. A turn reaching step 3
  meanwhile logs `TurnQueued` behind its own thread, the form start-up reconciliation uses; the
  reducer folds that to `Idle` while no turn of the thread runs, so the thread shows Idle, not
  Queued, for the few moments the restore holds the writer. The
  version must be the matter's (`command.invalid`). One commit of the matter's stream logs the
  outside-edits checkpoint, `CheckpointRecorded { Restore }` and `VersionRestored`; a failed one
  is retried once when the stream drains; if the retry fails too, the files stay restored on disk,
  the log does not say so, the threads are still told, and the writer passes on. The
  outside-edits checkpoint that commit held is lost with it: the lawyer's edit stays in the
  shadow repository but is never recorded, so it cannot be shown or restored as a version. A restore with
  nothing to write logs only the outside edits (and the reused id, if the log lacks it) and
  answers `VersionRestored` with the reused id, as `UndoStep` does: the missing
  `VersionRestored` event, not the outcome, tells the client nothing was written.
- The files a restore wrote join the note of every thread of the matter once its commit is
  durable. `VersionRestored` does not list them, so this part of the note is held in memory and
  lost if the host restarts before that thread's next prompt (clauseo-acp-private#183 makes it
  durable).
- `CompareVersions` answers `QueryResult::FileChanges` from `matters`'s compare of two
  checkpoints. A compare with the folder now is not reachable through this query.
- `HostQuery::PreviewRestore { matter, checkpoint, scope }` is new and additive (0002): it
  answers `QueryResult::RestorePreview { changes, added_since, kept, locked }` from
  `Matter::preview_restore` and writes nothing. `locked` lists the files a program holds, which a
  `RestoreVersion` would refuse with `matter.file_locked`, its message naming them.

### Cancel, reconciliation, shutdown

- `Cancel` stops the current turn only. A turn not yet started ends `TurnEnded { Cancelled }`. A
  running turn: one commit resolving every pending permission request and elicitation as
  cancelled and every unfinished tool call as `Cancelled`, then `session/cancel` and a 5 s wait.
  If the prompt does not return, the agent is stopped by `process`'s shutdown ladder
  (`AgentSessionDetached { Closed }`) and steps 6 to 8 run with `Cancelled`. An agent request that
  arrives with no running turn, or while it is cancelling, is answered cancelled and not logged.
  Everything else the agent sends after `session/cancel` (text, tool calls) is dropped too: R10
  tags it `Late` and the host drops `Late` updates, keeping only what the cancel commit needs.
- Start-up reconciliation, before serving, once per thread with an open turn or pending request:
  expire with `HostRestarted`; a started turn gets an after-turn checkpoint and step 7 with
  `TurnFailed { thread.interrupted }`; `AgentSessionDetached { Closed }` when a session was
  attached. When the after-turn checkpoint was made but never logged (a crash, or an end that
  failed twice), the repair records it, logs `FilesChanged` from `Matter::changes_between`
  (from `checkpoint_before` to it) with every path flagged as possibly holding lawyer edits,
  and keeps `checkpoint_after`, so the turn stays undoable (Rohan, 2026-10-31). The repair takes
  that checkpoint from `matters`'s timeline (matched by thread and turn), not from the folder:
  once an end fails twice the writer is free, and another thread's turn may have run since
  (clauseo-ai/clauseo-acp-private#133). A started turn with no after-turn checkpoint of its own
  whose matter has a later open turn, or a checkpoint after its `checkpoint_before`, gets an
  empty step (`checkpoint_after` = `checkpoint_before`, no `FilesChanged`): a running turn holds
  the writer, so its end must have run, and R8 reused `checkpoint_before` or the checkpoint
  failed. The cost: in that second case the turn's changes are not its own but stay on record as
  the next writer's `OutsideEdits`. The matter's started turns are repaired once every thread of
  the matter that is not archived has been looked at, and only the one that started last (by its
  `TurnStarted`) may take a new after-turn checkpoint. When one of those threads fails to load
  or its log fails to read, none may: a lone crashed turn there gets an empty step, and its
  changes are logged as outside edits by the next turn. This holds within one start, not across
  starts: if a thread fails to load and other turns then run in the matter, a later start that
  repairs that thread's turn as the only open one takes a new checkpoint holding their changes
  (follow-up: refuse a new checkpoint when the matter has a `TurnStarted` newer than the turn's).
  A turn that never started (its `UserMessage` logged, no `TurnStarted`) gets
  `TurnQueued { behind: <its own thread>, behind_turn: <the thread's previous turn, or None> }`,
  so the reducer shows the thread `Idle`. A turn with `TurnEnded` is never touched, so each
  repair happens once.
- Shutdown cancels running turns, stops each agent by the ladder, flushes, takes due snapshots,
  stops the matter threads and closes `store`.

### Saving and showing

- `seq` is gapless per stream. A commit takes `pending + 1` and is sent at once; one feeder task
  hands commits to `Store::send_commit` in order, so group commit puts waiting ones in one sync.
  Waiting for each commit (about 4 ms under fullfsync) would cap a stream near 250 commits a
  second. Results are applied and published in `seq` order, only after `Ok`. After a failure the
  stream sends nothing until its in-flight commits return; no `seq` is reused.
- Coalescing per thread on the host's clock: deltas reach the screen in one 16 ms display batch;
  they are committed as one `TextAppended` per message every 250 ms, at 8 KiB buffered, before any
  other event, at turn end and at shutdown. Per session, past 2,000 updates in one second, tool
  call patches merge per tool call and replace-style updates keep the last value.
- Each loaded stream has an epoch and a patch window of at most 256 KiB of serialised patches,
  counted by their `serde_json` length. A subscriber with the current epoch and a cursor still in
  the window gets the tail, else one `Reset`. Replaying a 4 MiB window took 16 to 28 times longer
  in the webview than one `Reset`; a 256 KiB tail costs about one `Reset`.

### Other choices

- `PromptBlock` holds only text and file links, so step 5 sends inline values only.
- Agent updates without their own event map to `PlanReplaced`, `CommandsAvailable`, `ModeChanged`,
  `ConfigOptionsReplaced`, `UsageUpdated`, `Notice`, `ThreadRenamed { by: Agent }` and
  `CompactionChanged`.
- An undo keeps every path of the step the command leaves out.
- The agent's launch spec carries the inherited environment, so `process` redacts every
  inherited secret from stderr tails. An inherited name an agent or hook variable also names,
  ignoring case, is left out: on Windows `Path` and `PATH` are one variable, and the inherited
  one must not win.
- A prompt's file must resolve to a path inside the thread's matter folder, else
  `command.invalid`; it goes to the agent as a percent-encoded `file://` URI.

In `ARCHITECTURE.md`, the `host` row's "May depend on" adds `async-channel, async-io, futures,
uuid, serde_json, tracing, dunce` (dunce resolves a prompt's file to check it is in the matter
folder); "Never depends on" stays `tauri, tokio, agents`; status `exists`.

## Consequences

The actor never blocks on the store, git or an agent, and text reaches the screen within 16 ms.
A command's answer means its events are durable. The desktop window, the `cli` and
`transport_ws` build on the handle's shape. A later change must keep commit before publish, the
gapless `seq`, and zstd and git off the actor. Stage B's `limits` arrives as a new actor input
publishing `HostEvent::AccountLimitsChanged`. The queue, steer and the restart supervisor come
with follow-ups. The profile hook is called at each session start (clauseo-acp-private#123), and
a session gets only the MCP servers the lawyer turned on (clauseo-acp-private#160):
`crates/host/src/agent/loads.rs` holds the switches and their reasons.

## Status

proposed

## Date

2026-10-31
