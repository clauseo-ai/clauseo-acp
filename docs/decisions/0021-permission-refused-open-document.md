# 0021: A permission request refused because a program holds the file open

## Context

Word for Mac keeps its own copy of a document it has open. When an agent writes that file
mid-turn, Word's next save puts its copy back over the agent's edit, and nobody is told
(private issue #162). Claude's adapter asks `session/request_permission` before Edit and Write
in its default and plan modes (and in acceptEdits outside the working folder), naming the file
in `toolCall.locations` and `rawInput.file_path`; Codex's adapter names a file change's paths in
`toolCall.locations`. So the host can see the file before the write and refuse it.

The lawyer's view must say why: "Word has Brief.docx open. Close it in Word or save a copy, then
ask again." `PermissionResolved` says what was chosen and by whom, not why, so the log needs one
more fact: the file.

## Decision

- When a permission request arrives in the live turn and a path it names (its locations, its
  diffs, Claude's `file_path` or `notebook_path`; naming none, the paths its tool call named),
  inside the matter folder, is a file a program holds open, the host answers it without asking
  the lawyer: the request's reject-once option, else `cancelled`. A reject-always option is
  never taken: it may leave a lasting rule in the agent the lawyer never chose. One commit logs
  `PermissionRequested` and `PermissionResolved`; the agent hears the answer once it is durable.
- The check is `clauseo_matters::held_open_at`, on the matter thread: the path is resolved into
  the folder by `matter_path_in` (both canonicalised with `dunce`, a missing file through its
  parent, compared by components ignoring case on macOS and Windows, the text compared only when
  that fails), then `held_open` looks for an Office owner file beside it (Word, Excel or
  PowerPoint), or on Windows a sharing violation from any program. Claude Code works in the
  resolved folder, so its paths are physical even when the matter was picked through a symlink.
  The pre-write guard calls the same function.
- `PermissionResolved` gains `held_open: Option<MatterPath>`, left out when `None`, set only
  on such a refusal. It is not named for Word: the same check catches Excel's and PowerPoint's
  owner files and, on Windows, any program's sharing violation, so a Word name would be untrue
  on the wire for good. It matches `clauseo_matters::held_open`. `PermissionState::Resolved`
  in the view carries the same field. `by` stays `Principal::LocalDesktop`: the host on this
  computer answered.
- This is additive under 0002's Freeze rules: a new `Option` field left out when `None`, so
  `schema_version` stays 1, rows written before decode with `None`, and an older reader skips
  the field. No new enum, so no row in 0002's enum table and no frozen corpus entry.
- Rejected: an open enum `reason` field (`{"reason": {"held_open": {"path": ..}}}`). It reads
  better once a second reason exists, but none is planned, and a new open enum brings a table row,
  corpus files and coverage tests for one variant. A second reason later adds its own optional
  field, or this record is superseded by one that introduces the enum.
- Rejected: a new `Principal` or `PermissionChoice` variant. The choice really is the reject-once
  option the agent offered, and the answer really came from this computer.

## Consequences

- The UI can say which file is held open and that the lawyer was not asked; Word is by far the
  likeliest holder in a matter, so its copy may name Word.
- ACP gives the agent no text with a rejection: the model sees only that the edit was rejected.
  The pre-write guard through Claude's PreToolUse hook (the second part of #162) gives the model
  the reason; it calls the same `held_open_at`.
- Writes the agent makes without a permission request (Claude's acceptEdits inside the folder,
  Codex's workspace-write and agent modes, any shell command) are not covered here.

## The stale owner-file rule (R9 follow-up, 2026-11-05)

After a Word crash its owner file stays (research 38, point 1: 3 of 3 crashes), and Word's next
open and close of that document does not remove it. So an owner-file-only check would refuse
for good: undo, restore, this permission check and the pre-write guard alike
(clauseo-ai/clauseo-acp-private#224). `held_open` now passes over an owner file the stale rule
finds stale (`crates/matters/src/locks/stale.rs`), checked for each owner file found:

- macOS: stale only when no `Microsoft Word` process runs at all, `F_GETLK` sees no lock on the
  document, and the folder is on a local APFS or HFS+ volume (`statfs`). On a network volume a
  colleague's Word elsewhere leaves an owner file and no local lock, so it stays held there.
- Windows: stale only when a no-share open of the document succeeds and no `WINWORD.EXE` runs.
- Linux: no Word, so an owner file is always held.
- Any query that fails (the process listing, `F_GETLK`, `statfs`, the open) counts as held. A
  failed process listing counts as "a Word may be running".

Research 38's own rule (point 3) also let an owner file through when it was older than every
running Word. That is not taken, because of its point 5: after a crash, a Word that reopens the
document reuses the stale owner file. So a reopened document's owner file predates the new
Word, and during a save (Word swaps in a new file and locks it a moment later) only the lock
would guard the document. No path to a silent overwrite matters more than an occasional extra
"quit Word" step. So a crashed Word's owner file blocks only while some Word runs, and quitting
Word clears it.

## Status

proposed

## Date

2026-11-01
