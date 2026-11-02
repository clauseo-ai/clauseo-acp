# 0017: Locate a moved matter folder, and each matter's folder state

## Context

A matter whose folder moved answers `matter.folder_missing` (reason `NoFolder`) until the lawyer
says where it went. There is no automatic move tracking (decision of 2026-10-31): the lawyer picks
the folder, and the folder's `.clauseo/matter.json` must name the matter. `matters` already has
`Matter::locate` (import's refusals for the place, then the marker check, then an open that points
the history's `core.worktree` at the folder) and `Matter::relink` (writes a deleted or unreadable
`matter.json` again, only in the folder the history records). Both leave one check to the host:
the folder must not be, hold or sit inside another registered matter's folder
(`matters::same_or_nested`). The screens need a command, a way to show which matters lost their
folder, and a line in the matter's history.

## Decision

- `HostCommand::RelocateMatter { matter, folder }`, the folder a `FolderRefId` the desktop
  registered from its picker, as `CreateMatter`'s `ExistingFolder` takes it. It answers `Done`.
  Order: shape, principal, receipt, then the matter's writer must be free and no thread of the
  matter may have a turn, an unlogged undo or a start-up repair in progress
  (`matter.turn_running`). The relocate then holds the writer (`Holder::Relocate`, beside a
  thread's turn or undo and a restore) until its commit returns or it is refused, so a turn
  sent meanwhile queues behind it and an undo, a restore or a second relocate is refused. On the matter's worker thread
  come the registry check (`matter.folder_refused`) and `Matter::locate`. The registry is every
  other registered matter's folder plus the folders other commands are making matters of (a
  create, an import or a relocate in progress, held on the actor until each is answered). Only
  after `Ok` does one commit carry the new event, the matter's row with the new path and the
  receipt. The host's row moves when that commit is durable; a failed commit sends the worker
  back to the recorded folder before the writer passes on. `Matter::locate` has already pointed
  `core.worktree` at the new folder, and only an open writes it back: when the old folder is gone
  it stays on the new one. That is safe (the matter reads `missing`, relink refuses a folder the
  history does not record, and the next relocate sets both); writing it directly needs a
  `matters` call that does not exist. Every refusal keeps the codes
  `matters` gives and stores nothing, receipt included, so the same command id can be sent again.
- `DomainEvent::MatterRelocated { folder }` on the matter's stream, `folder` being the path as
  the host stores it. The reducer folds it into a marker of the Versions timeline,
  `MarkerKind::MatterRelocated { folder }` ("Found at ..."). Paths stay out of commands; the path
  in the log and the view is for display.
- `MatterSummary.folder_state`: `found` or `missing`, an open unit enum (`FolderState`), missing
  from an older host reading as `found`. `ListMatters` takes it from one `std::fs::metadata` per
  matter, following a symlink as `matters` does for `NoFolder`: a path that is missing or not a
  folder is `missing`; a stat that fails for another reason is `found`, and opening the matter
  gives the reason. The stats run off the host thread (0011: the actor never blocks), on a
  short-lived `clauseo-stat` thread per list. The whole list has one deadline, 2 s on the host
  clock: a folder not answered by then reads `found`. A folder whose stat is still running from
  an earlier list (a share that hangs) reads `found` without a new stat, so a hung folder strands
  one thread at most. 100 bare stats cost about 0.17 ms; the threaded read of 100 folders about
  0.35 ms in a release build and 0.5 ms in a debug one (Linux VM, ext4). There is no stored
  state: each list looks again, so a folder that comes back reads `found` at once.
- Relink: the first open of a matter on its worker calls `Matter::relink` with the registry's
  `created_at` when it fails with `NoMatterJson` or `Unreadable`, after the same registry check,
  and logs a warning. That first open spends the relink whatever it finds, so a later open never
  relinks with a registry taken before. When another matter claims the folder the open's own
  `matter.folder_missing` stands and nothing is written.
- Each agent remembers the folder it was started in. When a turn takes the writer (step 4) and
  its agent's folder is not the one the matter's row names (the matter was relocated since), the
  agent is stopped (`AgentSessionDetached { restarting }`) and started again in the new folder
  for that turn, which keeps the writer. Idle agents are left alone until their next turn.

## Consequences

- Import does not run the registry check (it never did): an import of a folder holding one a
  relocate has claimed is not refused by the host.
- Two refusals the app thread asked for ("already the matter {name}", "inside Clauseo's matters
  folder") still share `matter.folder_refused`; separate codes are H5's.
- A stat that hangs leaves its `clauseo-stat` thread blocked until the system gives up on it; the
  list answers without it, and the folders after it in that list read `found` too.

## Status

proposed

## Date

2026-11-02
