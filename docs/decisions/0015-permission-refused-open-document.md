# 0015: A permission request refused because a program holds the file open

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

- When a permission request arrives in the live turn and a path it names, inside the matter
  folder, is a file a program holds open (`clauseo_matters::held_open`: an Office owner file
  beside it, from Word, Excel or PowerPoint, or on Windows a sharing violation from any
  program), the host answers it without asking the lawyer: the request's reject-once option,
  else its first reject option, else `cancelled`. One commit logs `PermissionRequested` and
  `PermissionResolved`; the agent hears the answer once it is durable.
- `PermissionResolved` gains `held_open: Option<MatterPath>`, left out when `None`, set only
  on such a refusal. It is not named for Word: the same check catches Excel's and PowerPoint's
  owner files and, on Windows, any program's sharing violation, so a Word name would be untrue
  on the wire for good. It matches `clauseo_matters::held_open`. `PermissionState::Resolved`
  in the view carries the same field. `by` stays `Principal::LocalDesktop`: the host on this
  computer answered.
- This is additive under 0002's Freeze rules: a new `Option` field left out when `None`, so
  `schema_version` stays 1, rows written before decode with `None`, and an older reader skips
  the field. No new enum, so no row in 0002's enum table and no frozen corpus entry.
- Rejected: an open enum `reason` field (`{"reason": {"held_open": {"path": ..}}}`). It reads better
  once a second reason exists, but none is planned, and a new open enum brings a table row,
  corpus files and coverage tests for one variant. A second reason later adds its own optional
  field, or this record is superseded by one that introduces the enum.
- Rejected: a new `Principal` or `PermissionChoice` variant. The choice really is the reject
  option the agent offered, and the answer really came from this computer.

## Consequences

- The UI can say which file is held open and that the lawyer was not asked; Word is by far the
  likeliest holder in a matter, so its copy may name Word.
- ACP gives the agent no text with a rejection: the model sees only that the edit was rejected.
  The pre-write guard through Claude's PreToolUse hook (the second part of #162) gives the model
  the reason; it calls the same `held_open`.
- Writes the agent makes without a permission request (Claude's acceptEdits inside the folder,
  Codex's workspace-write and agent modes, any shell command) are not covered here.

## Status

proposed

## Date

2026-11-01
