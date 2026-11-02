# 0032: The folder scan before a matter

## Context

Before an existing folder becomes a matter, the window shows what is in it and where it lives:
file and byte counts as the walk goes, the file being read, whether the folder is on a network
drive or syncs with a cloud service, and how many files are cloud-only (HANDOFF item 6). Nothing
is stored: the window holds "Got it" before it sends `CreateMatter`. The walk can take minutes on
a large or remote folder, so it must not run on the actor, must stop when asked, and must not
flood the window. S21 (stage F) will need the same "where does it live" answer for an open
matter. Each new command needs a ruling in `Bridge::admit` (0019).

## Decision

- **Commands.** `HostCommand::ScanFolder { folder }` scans the folder a `FolderRefId` names and
  answers `Done` when the walk ends; an unknown ref is `command.invalid`, a folder import would
  refuse is import's refusal code (`matter.folder_refused` and the finer codes of 0022).
  `HostCommand::CancelScan { folder }` sets the cancel flag of a running scan of that folder
  and answers `Done` at once, also when none runs. One scan at a time: a `ScanFolder` sets the
  running scan's flag before it starts its own. Neither stores anything, not even a receipt,
  so a repeated `command_id` scans again; a duplicate sent while the scan runs joins it.
  Both ride on `Access::Dispatch` like every other command.
- **Event.** `HostEvent::FolderScanned(FolderScanned { folder, files, bytes, now_reading,
  place, cloud_only, done, cancelled })`, app-wide (`subscribe_app`), never logged. `files` and
  `bytes` count what an import would version (not cloud-only or over-cap files); at `done`
  without `cancelled` they equal the import's `ImportSummary`. `place` is `"local"`,
  `"network"` or `{"synced": {"service": ".."}}`. The last event has `done: true` and no
  `now_reading`; a cancelled scan's last event has `cancelled: true`. A failed walk still sends
  its last event, then the command fails. The last event is part of the app stream's catch-up,
  so a lagged window does not miss the end.
- **The walk.** `clauseo_matters::scan_folder` refuses what import refuses, then runs import's
  walk with its exclusion list and size cap under the no-download guard: a cloud-only file is
  counted from its metadata and never read. The scan checks the cancel flag before each folder,
  entry and stat.
- **Where a folder lives.** `clauseo_matters::place(folder)` gives `Local`, `Network` or
  `Synced { service }`. macOS: under `~/Library/CloudStorage/<Provider>-*` is synced with
  `<Provider>`, under `~/Library/Mobile Documents` with `iCloud Drive`; else no `MNT_LOCAL` in
  `statfs` is network. Windows: under `%OneDrive%`, `%OneDriveCommercial%` or
  `%OneDriveConsumer%` (no case) is synced, the service from that folder's name before its first
  `-` (`OneDrive - Contoso` is `OneDrive`); else `GetDriveTypeW` of the folder's volume saying
  remote, or a UNC path, is network. Synced wins over network. Anything else is local; so is a
  failed system call. **Linux is always local**: nothing reads a mount there, so `matters` needs
  no new dependency. The rules are plain functions of their inputs, tested for every OS on every
  OS. Other services (Dropbox or Google Drive outside CloudStorage) are later.
- **Threads and throttle.** The host runs each scan on a short-lived `clauseo-scan` thread, as
  `clauseo-stat` reads folder states: the thread computes the place, walks, and posts to the
  actor. Progress goes through one slot the thread overwrites, so at most one progress input
  per scan waits in the actor's channel. The actor publishes at most one event per 250 ms on
  the host clock (a timer sends what it held), and always the last. Only the current scan's
  progress goes out; a cancelled scan only says it ended. On shutdown every flag is set and
  nothing waits for the thread; one stuck on a hung network stat ends when the stat returns.
- **The bridge.** `ScanFolder` passes `admit` only for a folder ref this run's `pick_folder`
  issued, as `CreateMatter` and `RelocateMatter`; `CancelScan` always passes, since it starts
  nothing. Both come from `main` only; the Settings window's allow-list refuses them.

## Consequences

- The window can draw the scan, network, synced and cloud-only notices without touching the
  file system, and can drop events of a folder it no longer shows by `folder`.
- `FolderScanned` reaches `main` only: the room is its only reader, and the desktop's allow-list
  (`AppEvents`, 0028) keeps it from the Settings window. The throttle bounds what `main` receives.
- A scan's counts can differ from the import that follows when the folder changes in between,
  or when a file goes cloud-only after its stat (import then counts it as cloud-only).
- S21 reuses `matters::place` for `matter_opened.place`. A new service or OS rule is a new pure
  rule with its tests, not a change to the wire: `service` is free text.

## Status

proposed

## Date

2026-11-02
