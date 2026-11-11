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

**Agent-started turns** (2026-11-11, `actor/turn/own_turn.rs`). claude-agent-acp starts a turn of
its own, with no `session/prompt`, when a command Claude Code put in the background finishes (a
steer during a Bash call backgrounds the command and ends the prompt `end_turn`). Its updates
come with no prompt in flight (`Origin::Idle`), and agent output outside a turn is never
dropped: answer or thought text, a tool call the host does not have, or a permission request or
elicitation (`acp` now tags those with their `Origin` too) opens a turn on the same thread; a
request is asked once the turn is live, never cancelled as outside a turn. It has no `UserMessage` and is otherwise a turn: step 3 (the writer) at once, step 4
(the before checkpoint, with the outside edits since the last turn as for a prompt, then
`TurnStarted`), its updates logged live, steps 6 to 8 with the after checkpoint, so the view shows
an exchange with no question and Undo has its step. No protocol change: `TurnStarted` needs no
prompt, the reducer's step needs only the turn, and the UI already draws an exchange with no
question. Until it is live, and while another turn of the thread starts or ends, the updates
wait in memory in order; they are logged in the agent-started turn once it starts, or in a turn
a prompt starts meanwhile, which they then come first in. It ends `EndTurn` on the agent's
`usage_update` with a cost (claude-agent-acp sends it from the SDK's `result` at the end of every
turn, its own turns included, with `_meta._claude/origin.kind: "task-notification"` there, read
from 0.89.1's source, not recorded), or after 60 s with no update, unless a tool call or request
of the turn is open, which it waits for up to 10 minutes of silence. Queued prompts wait behind
it as behind any turn. A cancel (Stop, Send now), in every phase (waiting for the writer,
checkpointing, starting or live), sends `session/cancel`, drops the output waiting for the turn
and ends it `Cancelled` (at once once live, as no prompt answers it), and opens the turn's late
window, the own-turn counterpart of a prompt's `Origin::Late`: no prompt response tells the host
when the agent has stopped, so until its stop signal (the cost-bearing `usage_update`) or 3 s with
no answer, thought, tool or plan update with no prompt in flight, that output is dropped (a
lifecycle line counts it, with no text), no agent-started turn opens, and a prompt sent now (or
sent meanwhile) waits, at most 3 s after the cancel, so the agent's late edits come before the new
turn's before checkpoint and its Undo takes back only its own. The window closes at the latest
30 s after the cancel. Agent output while a prompted turn is cancelled, waiting when it ends, or
within 5 s of its end is that turn's tail and is dropped too. `acp`'s `Origin::Late` now ends at
the cancelled prompt's response, so output after it can open such a turn. A host restart repairs
an agent-started turn left open as it repairs any (`Failed`, `thread.interrupted`), keeping what
it logged. codex-acp
does not start turns of its own for a client without AIR's async tasks (its background terminals
are AIR-only), so this applies to Claude; were it to, the same path would take it, ended by the
quiet period, as Codex sends no cost-bearing usage at a turn's end.

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
  (send now, `SendQueuedNow`, a steer the lawyer sent now), else only after `EndTurn`, which
  also starts a steer the host held (below). Any other stop holds
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
- Steering (amended 2026-11-11: a steer counts as delivered only once confirmed). A real steer
  goes only to an agent with both `SteeringCapability` flags (claude-agent-acp; `acp` reads
  `_meta.steering.supported` and `agentCapabilities._meta.claudeCode.promptQueueing`), and only
  while Claude can take it now: the turn is prompting, not cancelled, and no permission or
  question is open (claude-agent-acp defers a steer while one is, and a cancel then drops it).
  Its `UserMessage` names the running turn and is durable before `_session/steering` goes out
  with `idleBehavior: promptRequired`. Every other steer is a send now (`SentNow`), Codex's
  included, or is held by the host.
- Host-held steers. A steer Claude could take later but not now waits as the next turn: its
  `UserMessage` with a new turn, `TurnQueued` behind the running turn marked `at_head`, and
  `SteerResolved { Requeued }`; it is never sent as a steer. It starts once the running turn ends
  `EndTurn` (a permission allowed or rejected, the turn carrying on to its end). After any other
  stop (Stop, Send now of another message, a failure) it is not sent: one commit of its turn's
  `TurnEnded { Cancelled }`, as `RemoveQueuedPrompt` logs it, and `SteerResolved { NotSent }`
  naming the turn that ended. A steer requeued before Claude took it is held the same way.
- Confirmation. claude-agent-acp answers `injected` once the steer is in its own queue, not when
  the model reads it, sends nothing when it does, and drops a queued steer unread on
  `session/cancel`. So for an agent with a Claude home `injected` only accepts the steer; nothing
  is logged until Claude's transcript (`<home>/projects/<project key>/<session>.jsonl`) shows it
  taken in: a line stamped no earlier than 2 s before the send whose words hold one of its text
  blocks exactly (both trimmed). The words are a `user` line's message content (a string, or its
  `text` blocks; never a `tool_result` block nor anything beside the message, which hold a tool's
  output), a `queued_command` attachment's `prompt`, or the `content` of a queue operation
  `remove` with `absorbed_mid_turn`. Just before the steer goes out the host marks each
  transcript file at its length (one `stat` each, in the host's data folder; a file not there yet
  is marked at its start), and each check reads on from where the last stopped, off the actor, at
  most 2 MiB per file, up to the last whole line: a line cut by that cap or still being written is
  read again from its start, one longer than the cap is skipped (never a steer's own: eight times
  the longest prompt text), and a file shorter than its mark is read from its start. A read that
  stopped at the cap is followed by the next check at once. Nothing before the mark is read and
  nothing is ever read whole; a missing or unreadable file counts as not taken in. Checks run every
  2 s on the host clock while the steer's turn runs, at once when it ends, then every 2 s for 10 s
  more: Claude Code may write the line late, or run the queued steer as a turn of its own once the
  turn has ended (an agent-started turn, `own_turn.rs`), which then shows it taken in. Confirmed,
  it logs `SteerResolved { Injected }` naming the steered turn, with `aside`, the turn's tool
  calls not finished when the agent answered (the view marks them as carried on in the
  background). An agent with no Claude home (Codex never steers for real; the tests' fake agent)
  has no better signal, so its `injected` counts as confirmed at once; a Claude agent whose
  transcript cannot be placed (no home or no session known) has nothing to read, so its steer gets
  no check and no timer and is `NotSent` as soon as its turn ends.
- The steered turn's end, whatever its stop, with a steer unconfirmed: the checks go on for 10 s
  after the end (after a cancel, which drops the steer, the check at the end is the last), then
  `Injected` or `SteerResolved { NotSent }` with that turn; never sent again on its own. Nothing in
  the queue waits for those checks: the next queued turn starts as it would (`move_on` waits only
  for steers not answered yet), and Send now naming the ended turn within the 10 s requeues the
  steer as for a running one. A steer still unanswered at the end: after `EndTurn` the queue
  waits for its answer within 30 s of the send (`promptRequired` requeues it and it runs next;
  `injected` starts its checks with 10 s from the answer; no answer is `NotSent`); after any
  other stop it is `NotSent` at once and a later answer is ignored. `promptRequired`, `failed`, an
  error or `startedNewTurn` (cancelled at once) log `SteerResolved { Requeued }` and a
  `TurnQueued` behind the steered turn marked `at_head`; no answer within 30 s of the send while
  the turn still runs does too, so an agent that never answers cannot freeze the queue.
- Send now on a steer. `SendQueuedNow` naming the turn a steer not yet settled was sent into is
  the lawyer sending it: its steers unanswered or unconfirmed move at once to new turns at the
  head (`Requeued`), which start once that turn has ended whatever its stop, and a running turn
  is cancelled.
- Logs. Each steer is logged at info on the lifecycle target, with no text: sent, answered (with
  the answer's kind), requeued (why), confirmed, and not sent (why: `turn_stopped`,
  `no_confirmation`, `restart`).
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
  steer saved but never settled before the stop (no `SteerResolved`: unanswered or unconfirmed)
  gets `SteerResolved { NotSent }` with the turn it was sent into, which has ended or which the
  repair ends `Failed`; nothing is queued for it.
  A restart restores the log's order, except for the turns put back at the head: a requeued
  steer and a turn held for its agent's sign-in keep that head place as they have it live (their
  `TurnQueued` is `at_head`), the latest put back first. A send now's place at the head is not
  kept. A send now or a `SendQueuedNow` move not yet started when the host stops is lost; the
  turn stays queued, held.

The undo note (QR11.1) is not logged. Its words: "Note from Clauseo: since your last turn, the
lawyer put these files back to an earlier version, and removed those it did not have: notes/a.txt,
brief.txt. Read them again before relying on what you remember about them." The removed files are
those an undo deleted (its turn made them). The paths are the undo checkpoints' files since the
thread's last `TurnStarted`. A Versions restore has a line of its own, after what came before it,
so a file an undo removed and a restore brought back reads as back: "The lawyer restored
brief.txt from Versions; it's back as of 2026-11-11 14:05 UTC." (the time in UTC, to the minute;
"they're back" for several files), and for the files it removed, "That restore also removed
draft.docx, which that version did not have." Undos after a restore get a line starting "Then
the lawyer put these files back". An undo keeps the writer and leaves the note unchanged
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
- `RestoreVersion { undoes }` takes back an earlier restore: the files that restore added (in
  its tree, not the checkpoint before it) are deleted too, each only while it holds what the
  restore wrote; `PreviewRestore { undoes }` lists them in `removes`. `RestoreVersion { thread }`
  names the conversation it was asked from (one of the matter's, else `command.invalid`): the
  same commit logs `VersionRestored` in that thread's stream, where it folds to the same marker.
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
  `RestoreVersion` would refuse with `matter.file_locked`, its `files` naming them (0003).
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
- The agent's launch spec is the child's whole environment, so the caller's launcher must clear
  the environment (#384).
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
