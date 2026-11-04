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

1. `UserMessage` and the receipt; `dispatch` answers `PromptAccepted`. A prompt sent while the
   thread has a current turn or queued turns joins its queue instead (below).
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
   `TurnEnded`. If a turn's last commit fails, it is retried once when the stream drains; a
   prompt sent meanwhile queues behind it, and once the retry has failed too, `SendPrompt` on the
   thread is refused with `thread.turn_running` until the next start ends the turn.
8. Thread snapshot; the writer passes to the next turn waiting at step 4; once `TurnEnded` is
   durable the thread's queue moves on or holds.

When `matters` answers a checkpoint with `Reused` and the matter's log does not hold that id, the
host records it first (`CheckpointRecorded` with the kind, thread and turn from `matters`'s
timeline), at step 4, at step 7 and in start-up repair, so no logged event names an unrecorded
checkpoint.

Agent exit: expiries with `AgentExited`, step 7 with `TurnFailed { agent.exited }`, then
`AgentSessionDetached { Exited }` last.

### Queue, send now and steer

Amended by H9 (2026-11-02), the brief's "Later" list. The code is `actor/turn/queue.rs` and its
`queue/` modules.

- A thread's queued turns have a `UserMessage` and a `TurnQueued` behind their own thread
  (`behind_turn`: the turn ahead), and no `TurnStarted` or `TurnEnded`. `SendPrompt` (delivery
  queue) on a thread with a current turn, an end not yet durable or queued turns logs those two
  events and the receipt in one commit; past `MAX_QUEUED_TURNS` (10) it is refused
  `thread.queue_full` and nothing is logged. A thread whose own undo holds the writer still
  refuses with `thread.turn_running`, as does one whose last end failed twice.
- The queue moves on when the current turn's `TurnEnded` is durable: always to a head sent now
  (send now, `SendQueuedNow`, a requeued steer), else only after `EndTurn`. Any other stop holds
  it, `AtLimit` included (taken from `TurnEnded`, never from a later `TurnStoppedAtLimit`), until
  `ResumeQueue`, `SendQueuedNow` or a new `SendPrompt`. While a steer request is out the queue
  waits for its answer, and while the thread's own undo holds the matter's writer nothing
  starts: the undo's end moves the queue on, so a prompt never goes out before the undo's note.
  A head goes to step 2 as any turn does; a wait for another thread's
  writer logs its second `TurnQueued` there. The held flag is in memory only: the view shows it
  as `queue_held` (set by the host through `clauseo_reducer::set_queue_held`, written `false` in
  snapshots), true only while turns wait.
- Send now, and a steer that cannot be real, log the new turn's `UserMessage`, `TurnQueued`
  behind the current turn, the receipt and, for a steer, `SteerResolved { SentNow }`; the new turn
  takes the head and the current turn takes the Cancel path. With no current turn it starts at
  once, ahead of a held queue, which moves on after it. Send now is not counted against the limit.
- A real steer needs the running turn's prompt in flight and both of the agent's
  `SteeringCapability` flags (claude-agent-acp; `acp` reads `_meta.steering.supported` and
  `agentCapabilities._meta.claudeCode.promptQueueing`). Its `UserMessage` names the running turn
  and is durable before `_session/steering` goes out with `idleBehavior: promptRequired`.
  `Injected` logs `SteerResolved { Injected }` with `aside`, the running turn's tool calls not yet
  finished (the view marks them as carried on in the background); `promptRequired`, a failure or
  `startedNewTurn` (cancelled at once) log `SteerResolved { Requeued }` and a `TurnQueued` behind
  the steered turn, and the new turn takes the head with the steered content. An answer that
  has not come 30 s after the steered turn's end counts as a failure, so an agent that never
  answers cannot freeze the queue (a later answer is ignored). The wait starts at the end, not
  at the send: an agent may answer only once it takes the message in, and a steer requeued
  while its turn runs would reach the agent twice.
- `EditQueuedPrompt { thread, turn, text }` replaces the prompt's first text block, drops its other
  text blocks and keeps its file blocks (`QueuedPromptEdited`); empty text for a prompt without
  files is `command.invalid`. `RemoveQueuedPrompt` logs `TurnEnded { Cancelled }` without holding
  or moving the queue; the turn leaves the queue when that commit is durable and goes back to
  its place if the commit fails. `SendQueuedNow` moves the turn to the head, logs only the receipt and
  cancels the current turn. These three also take the current turn while it is still launching
  or waiting for another thread's writer, since the view shows it queued until `TurnStarted`;
  anything else is `thread.turn_not_queued`. `ResumeQueue` logs only the receipt. `ArchiveThread`
  is refused `thread.turn_running` while turns wait.
- The queue is in the log, so it survives a restart: start-up reconciliation rebuilds it from the
  whole log (edits applied, requeued steers in their new turns) and holds it; the started open
  turn is the one the fold shows running, no longer the last prompt, and a held queue is found
  behind a later turn that ended (the newest prompt without a step counts, not the last one). A
  steer saved but never answered before the stop moves to a new held queued turn
  (`SteerResolved { Requeued }` and its `TurnQueued`), as a failed answer would have moved it.
  A restart restores the log's order: a requeued steer keeps its message's place, behind prompts
  queued before it, and a send now or a requeue's place at the head is not kept. A send now or a
  `SendQueuedNow` move not yet started when the host stops is lost; the turn stays queued, held.

The undo note (QR11.1) is not logged. Its words: "Note from Clauseo: since your last turn, the
lawyer put these files back to an earlier version: notes/a.txt, brief.txt. Read them again before
relying on what you remember about them." The paths are the undo and restore checkpoints' files
since the thread's last `TurnStarted`. An undo keeps the writer and leaves the note unchanged
until `StepUndone` is durable; a failed undo commit is retried once, as a turn's end is.

### Versions

- Every checkpoint gets its `CheckpointRow` (id, matter, git commit, the `seq` of its
  `CheckpointRecorded`) in the same commit as that event: the import, before-turn, after-turn,
  outside-edits, undo and restore checkpoints. A `Reused` checkpoint the host records late
  (at step 4, step 7, start-up repair, including the turn's own after-turn checkpoint the repair
  takes from the timeline, or a restore that writes nothing) gets its row too, with the commit
  `Matter::commit_of` gives, except
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
  checkpoints, or, with `to` missing or `null`, of `from` with the matter's folder as it is now
  (`CompareSide::Folder`, on the matter's thread; nothing is stored, and only `from` is checked).
- `HostQuery::PreviewRestore { matter, checkpoint, scope }` is new and additive (0002): it
  answers `QueryResult::RestorePreview { changes, added_since, kept, locked }` from
  `Matter::preview_restore` and writes nothing. `locked` lists the files a program holds, which a
  `RestoreVersion` would refuse with `matter.file_locked`, its message naming them.
- `HostQuery::PreviewUndo { thread, turn }` is new and additive (0002): it reads the thread's
  log for the step as `UndoStep` does (refused as `command.invalid` when the turn has no step to
  undo), then answers `QueryResult::UndoPreview { edited_since }` from `Matter::edited_since` on
  the matter's thread: the step's files whose content differs from the after-turn checkpoint,
  by the stat cache and else a blob hash. It writes nothing, logs nothing and takes no writer.
  It is what Undo's "changed after Clauseo's edit" ask reads; a `FileChange`'s
  `may_include_lawyer_edits` (no agent tool call named the path during the turn) is not.

### Cancel, reconciliation, shutdown

- `Cancel` stops the current turn only. A turn not yet started ends `TurnEnded { Cancelled }`. A
  running turn: one commit resolving every pending permission request and elicitation as
  cancelled and every unfinished tool call as `Cancelled`, then `session/cancel` and a 5 s wait.
  If the prompt does not return, the agent is stopped by `process`'s shutdown ladder
  (`AgentSessionDetached { Closed }`) and steps 6 to 8 run with `Cancelled`. An agent request that
  arrives with no running turn, or while it is cancelling, is answered cancelled and not logged.
  Everything else the agent sends after `session/cancel` (text, tool calls) is dropped too: R10
  tags it `Late` and the host drops `Late` updates, keeping only what the cancel commit needs.
- Stop ends what the turn started (clauseo-acp-private#359). An agent can keep a command running
  past `session/cancel`: codex app-server keeps each background terminal on `turn/interrupt` by
  design, and starts each command in a session of its own, outside the agent's process group. So
  as the prompt goes out the host lists the agent's descendants (`process`'s `Descendants`,
  each with its start time; off the actor, on a `clauseo-sweep` thread), and when the turn ends
  `Cancelled` with the agent still attached it reads a cutoff on the actor (the clock process
  starts are stamped with) before the turn lets go of the writer, then ends every descendant new
  since the listing that started before the cutoff: on Unix SIGTERM (to the process's group when
  it leads one), 2 s, then SIGKILL to those and to what they started meanwhile; on Windows
  `TerminateProcess` within the agent's job, and again after 2 s. A queued prompt sent now
  (`SendQueuedNow`), a Codex steer (a cancel and a new prompt) and any prompt sent right after a
  Stop all start their turn after that cutoff, so nothing they start is ended; a process started
  in the cutoff's own clock unit (10 ms on Linux, a timer tick on Windows) is spared too. The
  agent and everything that ran before the prompt stay, and so does a new process running one of
  the session's MCP server programs. No sweep runs while the host stops. The agent's stop
  (restart, fallback, quit) lists the descendants off the actor first, and after the group or job
  kill also kills those still running. Known gaps: a process that double-forks away from its
  parent (Unix); an MCP server started or restarted mid-turn whose program is not an absolute path
  the OS reports as such (`npx` runs as `node`), or any server under a firm's managed MCP
  (`McpControl::Managed`, where the session gives the agent no servers and so nothing is kept);
  a helper an agent starts only at its first prompt, which a first-turn cancel ends; and a wall
  clock set back between the cutoff and a start (macOS, Windows).
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
- Coalescing per thread on the host's clock: a delta after a quiet 16 ms reaches the screen at once,
  and deltas inside the window reach it in one trailing display flush at the window's end;
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
publishing `HostEvent::AccountLimitsChanged`. The restart supervisor comes with a follow-up;
the queue and steer came with H9 (above). The profile hook is called at each session start
(clauseo-acp-private#123), and
a session gets only the MCP servers the lawyer turned on (clauseo-acp-private#160):
`crates/host/src/agent/loads.rs` holds the switches and their reasons. A Claude session runs in
the `default` permission mode whatever the user's own `permissions.defaultMode` says: every
session's `_meta` sends `allowDangerouslySkipPermissions: false`, and a session that starts in
another mode gets `session/set_mode default` before its first prompt, or its launch fails
(clauseo-acp-private#162). Known gap: the lawyer's personal Claude Code allow rules and user
hooks still apply to Clauseo sessions until stage D's R20 gives each session Clauseo's own
`CLAUDE_CONFIG_DIR` (clauseo-acp-private#285). Flag-tier ask rules could cover the allow rules
until then, but were dropped: every `Bash` call would prompt.

## Status

proposed

## Date

2026-10-31
