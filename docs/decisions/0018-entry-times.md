# 0018: Clock times on thread entries

## Context

The app shows when things happened in a thread: when the lawyer sent a prompt, when a step ended,
when a question was answered. Until now only `VersionEntry` had a time (`ts`, the envelope's).
The reducer may not read the clock (`ARCHITECTURE.md`), so a time can only come from the envelope
it folds. Its snapshots hold the view entries as they are, so adding a field changes the
snapshot shape.

## Decision

**Fields.** `ts: Timestamp` on `UserMessageEntry`, `StepEntry`, `LimitStopEntry`, `NoticeEntry`
and `MarkerEntry`; `answered_at: Option<Timestamp>` on `PermissionEntry` and `ElicitationEntry`.
Milliseconds since the Unix epoch, display only, like every `Timestamp`: order stays by `seq`.

**Where each comes from.** The `ts` of the envelope of the event that made or settled the entry,
as `VersionEntry.ts` already was: `user_message`, `turn_ended`, `turn_stopped_at_limit`,
`notice`, the marker's event (`compaction_changed`, `step_undone`, `version_restored`), and
`permission_resolved` / `elicitation_resolved`. `apply` already holds the envelope, so the fold
passes `envelope.ts` to the functions that build or settle these entries; nothing in `host`
changes. Later events that update an entry (`files_changed` or `turn_failed` after the end, a
`step_undone` on its step) leave its time alone.

**A turn that never ended.** It has no step, so there is no step time. A turn cut off by a crash
gets its `turn_ended` from start-up repair (0011), and its step shows the time of that repair.

**Expired questions.** `answered_at` stays `None` when a request or an elicitation expires, and
while it waits. A cancel resolves pending questions as cancelled (0011), which is a
`permission_resolved` or `elicitation_resolved`, so it does get a time.

**Snapshots.** A snapshot written before this change has no times, and the times are not in the
snapshot to recover. `REDUCER_SCHEMA_VERSION` goes from 1 to 2. `store`'s `reopen` asks for
schema 2 and passes older snapshots over (0006), so `host` folds the whole log once and every
time comes from its event. Nothing is invented: no time is taken from `seq`, from a neighbour,
or from the epoch.

## Consequences

The first reopen of each thread after the upgrade folds its whole log. On iris-vm (Debian, 8
vCPU), `cargo bench -p clauseo-host --bench reopen` took 6.76 ms with a schema-2 snapshot and
31.1 ms with only a schema-1 snapshot (20,000 events). When the fold covers 2,000 events or more
(`SNAPSHOT_EVERY_EVENTS`), `host` writes a schema-2 snapshot at once; a shorter thread gets one
at its next `turn_ended`. Old snapshot rows stay on disk until two newer snapshots of their
stream push them out.

Each entry is 8 bytes larger (16 for the `Option`s); the reducer bench moved by at most +1.0%
(`permission_requested`, +26 instructions).

A later field the snapshot cannot fill from itself bumps the schema the same way.

## Status

proposed

## Date

2026-11-02
