# 0045: Thread triage: snooze, done and the morning sweep

## Context

The rail is reorganised into Needs you, Done, snoozed and Recent (approved by Rohan). The app
needs per-thread triage state from the host: whether the lawyer snoozed a thread and until when,
and whether it is done. The state must survive a restart, read the same in every window, and
change by rules the app does not have to repeat. The host has no time-zone support, and adding a
time-zone crate is not wanted; a snooze must still wake at its time with no client timer.

## Decision

- **Events, in the thread's stream** (rows in 0002's kind table):
  - `thread_snoozed { until?, by }`: `until` is when the snooze ends, absent when it is cleared;
    `by` is `SnoozeBy`, an open enum: `lawyer` (`SnoozeThread`) or `expired` (the host, when the
    time came, always with no `until`).
  - `thread_done { done, by }`: `by` is `DoneBy`, an open enum: `lawyer` (`MarkThreadDone`) or
    `morning_sweep` (`SweepDone`).
  Both carry no thread or time: the envelope's stream and `ts` hold them.
- **Reducer.** `HeaderEntry` gains `snoozed_until` and `done`, and `ThreadSummary` (the rail's
  row) carries both from the header. The wake rule: `user_message`, `permission_requested`,
  `elicitation_requested`, `turn_stopped_at_limit`, `turn_failed` and `turn_ended` clear both.
  A snooze or done event after them stands. The rule reads no clock.
  `REDUCER_SCHEMA_VERSION` goes from 6 to 7 (0002's rebuild note).
- **Wake at the time.** The host arms one timer on its clock for the earliest `snoozed_until`
  among the headers it holds (threads of listed matters). When it fires, the host logs
  `thread_snoozed { by: expired }` for every thread whose time has come, then arms for the next.
  It re-arms after any returned commit holding a snooze or a waking event and after each thread
  stream is loaded, so a snooze that ended while the host was down, or while its matter was off
  the list, wakes at start-up or at import. Since the wake is logged, the view needs no clock and
  the app sees a real change.
- **Commands** (each answers `Done`; archived threads are allowed, since no command refuses an
  archived thread by code):
  - `snooze_thread { thread, until? }`: an `until` not after the host's now, or more than 366
    days ahead (`MAX_SNOOZE_AHEAD_MS`), is `command.invalid`.
  - `mark_thread_done { thread, done }`.
  - `sweep_done { day_start }`: the morning sweep. The app sends it at its first moment after
    05:00 local each day, and at launch when after 05:00, with `day_start` the local midnight
    that began today. A `day_start` more than 26 hours from the host's now
    (`MAX_DAY_START_SKEW_MS`) is `command.invalid`. Every thread of a listed matter that is not
    archived, not done, not snoozed past now, has nothing pending (no permission, question or
    limit choice), no turn running or queued, its last ending seen (`MarkSeen`), no commit in
    flight and its newest event before `day_start` gets `thread_done { done: true, by:
    morning_sweep }`, all in one commit with the receipt. A second sweep the same day finds
    every such thread done and logs nothing.
- Snooze and done are independent: the reducer keeps both, and the app decides how a thread that
  is both shows.
- Every snooze and done command is logged, a repeat too: the host decides on the durable state,
  which cannot tell a repeat from an undo of a change still in flight.

## Consequences

- A commit of an unloaded stream is not folded, so the sweep and the wake load each thread they
  write first, hold it loaded until all are, and decide again on the loaded state.
- The triage events count as activity (`ThreadSummary::last_activity`), so a thread marked not
  done or snoozed today is not swept until a later day.
- Search (H-search) indexes these events as it does any thread's; nothing here changes search.
- A thread whose stream is blocked by a failed commit is not woken or swept until it is written
  again.
- A new `DoneBy` or `SnoozeBy` value needs its 0002 row and its corpus entry, as any open enum.

## Status

accepted

## Date

2026-11-03
