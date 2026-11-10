# 0055: The open matter's papers follow its folder

## Context

The wall's tiles were the folder as `ListPapers` found it when the matter opened. A file added,
renamed, moved or deleted in Finder or Explorer while the matter was open left the tiles as they
were (stage C, D-055 and D-056), and a folder deleted while open went on showing its tiles until
the lawyer tried something (D-057). Lawyers save, rename and file papers in Word and Finder all
day, beside Clauseo.

## Decision

- The host watches the folder of the matter the lawyer last opened (`OpenMatter`), one matter
  at a time, through the `notify` crate: FSEvents on macOS, `ReadDirectoryChangesW` on Windows,
  inotify on Linux. The whole tree is watched, as the listing walks it, symlinks not followed.
  No polling.
- A change to a file the listing never shows (`clauseo_matters::is_excluded_path`: `~$` owner
  files, `*.tmp`, `.clauseo`, `.git`) and a read are passed over. The rest wait until the folder
  has been quiet for 300 ms (a Word save is temporary files and a rename), or 256 changes have
  come, on a `clauseo-papers` thread, which then stats the folder.
- The host then sends the app event `HostEvent::PapersChanged { matter }` (never logged, in no
  stream, no catch-up), and the app lists the papers again. The event carries no list: the
  listing stays the one place the folder's rules live. A folder that went reads `missing` in
  the matter's next row, so the room shows the moved folder's line with Locate folder, and the
  watch stops. A relocate watches the new folder and sends `PapersChanged`.
- Opening another matter, forgetting the matter, and the host's end drop the watch. A folder the
  OS will not watch is logged by kind and left unwatched. The app also lists the papers again when
  its window comes back to the front, which covers that case and any change a feed missed; a
  listing that finds the folder there again starts the watch.
- A file's `FileRefId` is per path, so a rename is the old tile gone and a new one come. The
  `tracked` marks follow, since each listing scans them. The Find index keeps the names of the
  last checkpoint, as before.

## Consequences

- `host` depends on `notify` (and, through it, inotify on Linux, objc2-core-services on macOS,
  windows-sys on Windows). `matters` exports `is_excluded_path`.
- No file is opened. On Windows the watch holds one handle, on the folder itself, opened with
  every share mode: Word saves, renames and deletes inside it as before, and the folder itself
  can be deleted or moved. While the watch lives, Windows refuses to rename or move the folder
  that holds the matter's folder; it lives only while the matter is open.
- inotify uses one watch per subfolder; past the user's limit the watch fails and the focus
  listing is all that is left.

## Status

accepted

## Date

2026-11-10
