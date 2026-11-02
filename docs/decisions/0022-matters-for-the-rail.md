# 0022: Matters for the rail: details, status, forget and a code per refusal

## Context

The room's rail (task U2, design HANDOFF item 8) lists every matter with what the lawyer needs
to find and order it: the client, the next date, a pin, when it was last opened and last
active, where its folder is, and what it is doing while the lawyer looks elsewhere. Until now
`ListMatters` gave only `{matter, name, kind}`, so the rail could not group by date, show Pinned
or Recent, find by client, or say "Needs your permission" without subscribing to every thread of
every matter. The new-matter form also needs to make a matter in the default folder without a
picker, to take it off the list without touching the folder ("The folder and its papers stay
where they are."), and to say why a folder was refused: one code, `matter.folder_refused`,
covered a drive root, the home folder, a system folder, a folder already a matter's, an
unreadable `matter.json` and Clauseo's own data folder alike.

## Decision

- **Details on the row.** `client` (`ClientName`: trimmed, then the title rules, 1 to 200
  characters), `next_date` (`Date`: ISO `YYYY-MM-DD`, a day that exists, years 0001 to 9999),
  `pinned`, `last_opened` and `forgotten` are columns of the store's `matters` row, added by
  migration `0002_matter_details.sql` (schema 2): nullable, `pinned` `NOT NULL DEFAULT 0`. A
  schema 1 file opens and lists its matters with no details. A command writes the row and its
  receipt in one commit and logs no event: `ListMatters` reads rows, and the reducer's matter
  view does not show details.
- **`MatterSummary`** gains `last_activity`, `client`, `next_date`, `pinned`, `last_opened`,
  `status` and `folder_display`. `status` and `folder_display` are always written; missing (an
  older host) reads as `idle` and empty.
- **Commands.** `CreateMatter` takes optional `client` and `next_date`, and a third source,
  `MatterSource::DefaultLocation` (`"default_location"`): the host makes
  `<default location>/<name>`, the folder `Effective::default_matters_dir` gives, which is
  `matters.default_location` or else `Documents/Clauseo` (the Scratch matter's parent), so there
  is always one and no "no default location" refusal. The name follows the new-folder rules, and
  an existing folder there is `command.invalid`. `UpdateMatter { matter, client?, next_date?,
  pinned? }`: missing keeps a field, `null` clears `client` or `next_date`, a value sets it
  (`FieldChange`). `OpenMatter { matter }` sets `last_opened` to now. All three answer `Done`.
- **Forget.** `ForgetMatter { matter }` sets `forgotten` on the row, moves the matter off the
  host's list at once (so nothing new starts in it), and once durable stops its threads' agents.
  It deletes nothing: the row, threads, log, checkpoints, the history under the matters root, and
  the folder with its `.clauseo/matter.json` all stay. A forgotten matter answers no command or
  query (`command.unknown_matter`; its threads `command.unknown_thread`), and start-up neither
  lists nor repairs it. Refused for the Scratch matter (`command.invalid`) and with
  `matter.turn_running` while a turn, undo or restore holds the writer, turns wait, or a thread
  has a turn not ended or an undo not logged. Importing the folder again finds its marker naming
  the forgotten matter and brings it back under its own id with its threads, versions, name and
  details (a client or date the import gives wins), answering `MatterCreated` with that id; the
  folder path becomes the one picked. The matter's worker is dropped when it is forgotten and a
  new one opens on first use after it comes back, so a folder moved meanwhile, even in the same
  run, is read where it is now. A forgotten matter's folder still counts against other folders
  (one inside it or holding it is `matter.folder_is_matter`); only that exact folder passes, to
  bring the matter back.
- **Status**, computed on each `ListMatters` from memory, never stored. Per thread: `NeedsYou`
  with a pending permission is `needs_permission`, other `NeedsYou` `needs_answer`;
  `PausedAtLimit` is `paused_at_limit`; `Starting`, `Running`, `Queued` are `working`; a thread
  that runs nothing (`Idle`, `AgentExited`, `IdleStopped`) says how its last turn ended if that
  was after the matter's `last_opened` (or it was never opened; 0023 replaces this with the
  thread's seen mark): `end_turn` is `ready`, every
  other ending (`cancelled`, `failed`, `max_tokens`, `max_turn_requests`, `refusal`, `at_limit`,
  an unknown one) `stopped { at }`; otherwise `idle`. Archived threads say nothing. Across
  threads the first of this order wins: `needs_permission`, `needs_answer`, `paused_at_limit`
  (the earliest known reset), `ready`, `working`, `stopped` (the latest stop), `idle`. The rule is
  `actor/rail.rs`'s `matter_status`, with a table test.
- **Last activity.** The newest event time over the matter's stream and its threads' streams.
  `Store::tail_times` reads it once at start-up, one key lookup per stream decoding only the
  envelope's `ts`; every returned commit keeps it current. `ListMatters` is then one walk over
  the host's threads with one map lookup per stream.
- **Folder display.** The folder path below the host's home folder (`HostConfig::with_home_folder`,
  default the OS's), with no `~` and the system's separators; the whole path elsewhere. Home is
  tried as configured and as canonicalised once at start-up, compared by components, ignoring
  case on macOS and Windows as the refused-folder check does (macOS's `/System/Volumes/Data`
  spelling is not tried).
- **Where new matters go.** `HostQuery::GetDefaultLocation` (`"get_default_location"`) answers
  `QueryResult::DefaultLocation { display }` (`{"default_location": {"display": "..."}}`): the
  folder a `DefaultLocation` create makes `<this>/<name>` in, shown by the same rule, for the
  form's folder line before a pick (H20). Display only; no path crosses the wire.
- **A code per refusal.** `CreateMatter` answers `matter.folder_is_drive`, `matter.folder_is_home`,
  `matter.folder_is_system`, `matter.folder_unreadable` (a folder the host may not read, or a
  `matter.json` that does not read), `matter.folder_is_matter` and
  `matter.folder_in_matters_root` instead of `matter.folder_refused`, which keeps its meaning for
  older peers. `matters` already returns the reason (`Refusal`); the host tells the home folder
  and its data folder from the other refused folders by the entry the refusal names.
  `matter.folder_is_matter` covers a folder that is, holds or sits inside a listed, forgotten or
  being-made matter's folder, and a marker naming a matter this install does not list (copied
  from another computer or data folder). `HostError.matter` names the matter only when it is on
  the list, so the form never offers to open a matter it cannot: for a forgotten one, one being
  made or an unknown marker it is left out, and the code stays `folder_is_matter` (the folder
  does belong to a matter, just not one this list holds). For an import the registry check runs on the matter's
  worker after `matters::check_place` (drive root, refused list), so a refused place is named for
  what it is even when a matter sits below it; for a new folder it runs on the host, since only
  the parent exists.

## Consequences

- The rail needs no subscription per thread for a status. Decision record 0023 pushes changed
  rows over `subscribe_app`.
- `ready` and `stopped` cleared when the matter was opened, not per thread: a turn that ended
  while the lawyer had the matter open showed as ready once they left. Decision record 0023
  replaces `last_opened` in that rule with each thread's seen mark (`MarkSeen`).
- Precedence picks recorded here because U2 and HANDOFF leave them open: a permission outranks
  everything, a pause at the limit outranks an answer ready, an answer ready outranks work in
  progress, and every ending but `end_turn` is a stop.
- A forgotten matter's threads are not repaired at start-up. They were idle when it was
  forgotten, so there is nothing to repair; when it comes back, its threads show their row's
  status until loaded.
- A folder that holds a listed matter's folder and is also a refused place is named as the
  refused place. A new folder's parent is not checked against the refused list, as before.
- With H3's `RelocateMatter`: forget is refused (`matter.turn_running`) while a relocate holds
  the matter's writer; a forgotten matter's folder counts in relocate's registry; a folder a
  relocate in flight is moving a matter to counts against new matters (named as being made, so
  no `matter`); and `MatterSummary` keeps H3's `folder_state` beside these fields. Relocate's
  own refusals still answer `matter.folder_refused`; giving them the per-reason codes is a
  follow-up.
- A later detail of the rail (case number, court, note) is another column and another migration.

## Status

proposed; the `last_opened` rule for `ready` and `stopped` is superseded by 0023

## Date

2026-11-02
