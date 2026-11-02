# 0023: Seen marks, last stops, and the matter and thread lists over `subscribe_app`

## Context

The room's rail shows every matter's status and, inside a matter, every thread's, while the
lawyer looks elsewhere (task U2; app-thread shapes H6 and H4). Two gaps remained after 0022.
First, "an answer is ready" and "stopped" had to clear once the lawyer had looked, and 0022
used the matter's `last_opened` as a stand-in: opening a matter cleared every thread's ending,
read or not, and a turn that ended while the matter was open showed as ready once they left.
Second, a window had to ask `ListMatters` and `ListThreads` again to see any change, because
`subscribe_app` (0017) carried only `SettingsChanged`. 0017 also set a condition for partial rows
on the app stream: a subscriber that lagged misses rows it cannot rebuild, so a whole-list form
must land before or with the first partial row.

## Decision

- **Seen mark.** `HostCommand::MarkSeen { thread }` (`"mark_seen"`) answers `Done` and stores
  the thread stream's last `seq` sent (so a `TurnEnded` already on screen is covered) as the
  thread row's `seen_seq`, a column added by migration `0003_thread_seen.sql` (schema 3). It
  writes the row and the receipt in one commit and logs no event, as 0022's row commands do;
  an unknown thread or one in a forgotten matter is `command.unknown_thread`. The migration fills
  `seen_seq` of every existing thread with its stream's `last_seq`, so threads from before the
  upgrade start seen rather than all showing as ready at once; a thread made later starts with
  none.
- **`ThreadSummary`** gains `seen: bool`, `last_stop: Option<LastStop { reason: StopReason, at:
  Timestamp }>` and `last_activity: Option<Timestamp>`. `last_stop` is the thread's newest
  `TurnEnded`: its `stop` and its envelope's `ts`. A turn stopped at the limit
  (`turn_stopped_at_limit`) or failed (`turn_failed`) ends with a `TurnEnded` too (`at_limit`,
  `failed`), so one event covers every ending. The host keeps it per thread from every durable
  `TurnEnded` and, when a stream unloads, from its last step (whose `order` is that `TurnEnded`'s
  `seq`), so nothing loads a stream for it; start-up loads every thread not archived in a listed
  matter, so only archived threads and those of forgotten matters can lack one. `seen` is true
  when there is no last stop or its `seq` is at most `seen_seq`. `last_activity` is the thread
  stream's newest event time, from the map 0022 keeps. A missing `seen` (an older host) reads
  as `true`.
- **Status.** 0022's rule stands with one change: a thread that runs nothing is `ready`
  (`end_turn`) or `stopped { at }` (any other ending) while it is not seen, and `idle` once seen.
  `OpenMatter` keeps setting `last_opened`, for "Recent", and no longer clears anything. This
  replaces 0022's `last_opened` stand-in.
- **App events.** `HostEvent::MattersChanged(MattersChanged { matters, removed, all })`
  (`"matters_changed"`) and `HostEvent::ThreadsChanged(ThreadsChanged { matter, threads,
  removed, all })` (`"threads_changed"`), both through `subscribe_app` only. With `all: false`
  they hold only the rows that changed or are new, each whole; `removed` names matters that left
  the list. With `all: true`, `matters` is the whole list in `ListMatters`' order (or `threads` the
  matter's whole list, as `ListThreads` answers) and `removed` is empty; the window replaces
  what it holds. Every app subscriber gets every thread's row; a window drops those of threads it
  has open, as 0017 decided.
- **Removals.** A forgotten matter goes out once in `MattersChanged::removed`, its threads with
  it and with no `ThreadsChanged`. A thread never leaves its matter's list (an archived thread
  stays with the `archived` status), so `ThreadsChanged::removed` is always empty today; the
  field is there so a later removal needs no new shape. A matter brought back goes out as a
  changed row, then its threads as one `ThreadsChanged` of new rows.
- **First frames and catch-up.** Every app subscription starts with the whole state: one
  `SettingsChanged`, one `MattersChanged { all: true }`, and one `ThreadsChanged { all: true }`
  per matter in the list's order (one with no thread included). So a window needs no separate
  `GetSettings`, `ListMatters` or `ListThreads` read to start, and no change can fall between
  the read and the stream. A subscriber whose channel filled gets the same frames once it has
  drained, instead of what it missed, then partial rows resume. That meets 0017's condition: the
  whole form is the first thing on every stream and comes again before any partial row after a
  gap.
- **The whole state goes outside the channel.** The first frames and every catch-up are 2 + one
  per matter, which can exceed the subscriber's channel (1,024 places; a smaller one in tests).
  Sent through the channel, an oversized catch-up would fill it, mark the subscriber lagged, and
  be resent whole after every drain, so it would never finish. So the actor puts the whole state
  in the subscription's front queue (`handle::Front`), which the subscription reads before its
  channel; the state is already in memory, and the queue holds one catch-up at a time (it is
  filled only when the channel is empty: at subscribe, and after a drain, when nothing was sent
  meanwhile). Events sent while part of it is unread queue behind it in the same queue, under
  the same lock that decides whether it is empty, so none overtakes it through the channel. Once
  the queue holds the catch-up's length plus a channel's worth, the subscriber lags as on a full
  channel and is caught up once it has read everything. Live partial rows still go through the `bounded` channel, filled with
  `try_send`: a slow window never holds up the actor. The other option, sizing each channel
  for the frames plus its capacity, was not taken: the matter count grows after the channel is
  made, and `async_channel` cannot grow a bounded channel.
- **When a row goes out.** A row goes out when a field the rail shows changes: status (a
  permission waiting counts, though the thread row does not show it), `seen`, `last_stop`,
  title, `pinned`, `client`, `next_date`, `last_opened`, name, `folder_display`, `queue_held`,
  and a create, forget or bring-back. The host keeps the rows it last sent while anyone is
  subscribed and compares after each input it handles. What may have changed is marked as it
  happens: each durable envelope of a thread's stream and each unload marks that thread, and
  each answered command marks everything (matter rows change only by commands, which are clicks).
  A marked thread's row is built and compared; only when it changed is its matter's row built
  and compared. `folder_state` (H3) is not on this branch; once it is, it is one more field of
  the comparison, which is written out field by field so a new field fails to compile there.
- **`last_activity` is coalesced** by riding along: it is never a reason to send a row, and every
  row sent carries it current. It changes on every event; a time-based throttle (at most once
  every 5 s) would need a timer and would not be deterministic in tests without one, while the
  ride-along rule is, and the rows that matter for ordering by activity (a turn starting and
  ending) go out anyway. Measured: a turn of the `tool-calls` scenario logs 36 envelopes and
  sends 5 app events (matter working with thread starting, thread running, matter ready with
  thread idle); sending each touched row unconditionally sent 37.

## Consequences

- The rail and the thread list stay current from one stream; `ListMatters` and `ListThreads`
  remain for a window that wants a fresh read.
- The window must call `MarkSeen` when the lawyer has seen a thread's ending (opening the
  thread, or reading its last turn); until then the matter shows ready or stopped.
- A lagging window gets whole lists, which grow with the number of matters and threads: one
  `ThreadsChanged` per matter, held in the front queue whole whatever the channel's capacity,
  so memory per lagging window grows with the lists.
- A row change by a command lands at the command's answer, after its commit is durable; a
  failed commit sends nothing, since the comparison sees the row as it was.
- A rollback of a thread row command (a failed `MarkSeen`, rename or archive commit) puts the
  whole row back as it was when that command ran, as before.
- Each durable thread envelope costs one thread row built and compared while an app subscriber
  is open; with none, nothing is marked or kept.

## Status

proposed (replaces 0022's `last_opened` rule for `ready` and `stopped`)

## Date

2026-11-02
