# 0055: The open matter's papers follow its folder

## Context

The wall's tiles were the folder as `ListPapers` found it when the matter opened. A file added,
renamed, moved or deleted in Finder or Explorer while the matter was open left the tiles as they
were (stage C, D-055 and D-056), and a folder deleted while open went on showing its tiles until
the lawyer tried something (D-057). Lawyers save, rename and file papers in Word and Finder all
day, beside Clauseo.

## Decision

- On macOS and Linux the host watches the folder of the matter the lawyer last opened
  (`OpenMatter`) or the app last listed (`ListPapers`: the matter the app reopens at launch is
  listed, never opened), one matter at a time, through the `notify` crate: FSEvents on macOS,
  inotify on Linux. The whole tree is watched, as the listing walks it, symlinks not followed.
  No polling. Windows has no watch (below).
- The watch starts on a short-lived thread, never on the actor: resolving the folder and
  inotify's walk of its tree would freeze the host on a share that does not answer. Each start
  has a generation; what a replaced watch posts is dropped. A start the OS refused (inotify's
  watch limit, an error from the share) is not tried again until another folder or matter is
  followed.
- A change to a file the listing never shows (`clauseo_matters::is_excluded_path`: `~$` owner
  files, `*.tmp`, `.clauseo`, `.git`) and a read are passed over; a path the event does not say
  is a file or a folder counts unless the listing passes it over as both. The rest wait on a
  `clauseo-papers` thread until the folder has been quiet for 300 ms (a Word save is temporary
  files and a rename), or 2 s after the burst's first change, whichever comes first. The thread
  then looks at the folder: only one not found or not a folder, twice 2 s apart, is missing (a
  share that blinks is not), and another folder swapped in at the same path gets a new watch.
- The app lists at most one at a time: changes while a listing runs make one more after it.
- The host then sends the app event `HostEvent::PapersChanged { matter }` (never logged, in no
  stream, no catch-up), and the app lists the papers again. The event carries no list: the
  listing stays the one place the folder's rules live. A folder that went reads `missing` in
  the matter's next row, so the room shows the moved folder's line with Locate folder, and the
  watch stops. A relocate watches the new folder and sends `PapersChanged`.
- Opening another matter, forgetting the matter, and the host's end drop the watch. A folder the
  OS will not watch is logged by kind and left unwatched. The app also lists the papers again when
  its window comes back to the front, which covers that case and any change a feed missed; a
  listing that finds the folder there again starts the watch.
- **Windows: no watch.** `ReadDirectoryChangesW` needs a handle on the matter's folder, and while
  that handle is open Windows refuses to rename or move the folder that holds it. A lawyer who
  cannot rename or move their own matter folder while Clauseo is open reads that as Clauseo
  breaking their files, which is worse than tiles that lag a moment. So the watch is compiled out
  on Windows (`notify` is a `cfg(not(windows))` dependency; `papers/no_watch.rs` stands in
  behind the same follow code, and `follow::live_watch` is the switch). Instead, the app lists
  the papers again when its window comes back to the front (as on every OS), and the host sends
  `PapersChanged` for the matter when each turn there ends, done, cancelled or failed
  (`follow::on_turn_ended`), since an answer is the one change Clauseo itself makes while the
  lawyer looks at the wall. A change made in Explorer while Clauseo stays in front shows at the
  next return to the window or the next answer's end. macOS and Linux send nothing at a turn's
  end: the watch tells of each change.
- A file's `FileRefId` is per path, so a rename is the old tile gone and a new one come. The
  `tracked` marks follow: the watch knows only that something changed, so it publishes
  `PapersChanged` and calls no `papers::scan::rescan` (#924); the listing that follows hands
  each new or changed `.docx` to the background scan, which publishes again if it finds marks.
  The Find index keeps the names of the last checkpoint, as before.

## Consequences

- `host` depends on `notify` on macOS and Linux only (and, through it, inotify on Linux and
  objc2-core-services on macOS); Windows does not build it. `matters` exports
  `is_excluded_path`.
- No file is opened, and on Windows no handle is held: the lawyer renames, moves and deletes the
  matter's folder and the folders around it as before. The cost is that Windows tiles follow
  Explorer only at the window's return to the front and at each answer's end, one listing each.
- inotify uses one watch per subfolder; past the user's limit the watch fails and the focus
  listing is all that is left.
- A listing that finds the folder gone empties the wall and shows the moved folder's line; one
  that finds a folder that is not the matter's leaves the wall as it was, and opening the matter
  says why.

## Status

accepted

## Date

2026-11-10
