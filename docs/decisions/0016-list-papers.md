# 0016: A matter's papers: the `ListPapers` query

## Context

The first thing a matter shows is its papers: one tile per document in the folder. Nothing on
the wire listed a folder's files: `ImportSummary` holds counts only, and a checkpoint's tree is
the folder as it was at the last turn, not after the lawyer saved a document in Finder or
Explorer. The list must not download a cloud-only file, must not run on the host's actor
thread, and must stay bounded however large the folder. A prompt names a file by `FileRefId`,
so the list must give each file one.

## Decision

**Query.** `HostQuery::ListPapers { matter }` returns `QueryResult::Papers { papers, skipped,
unread, truncated }`; each paper is a `PaperSummary { file: FileRefId, path: MatterPath, size:
Count, modified: Timestamp }`, `skipped` reuses `SkippedFile`, and `unread` is a list of
`MatterPath` folders. JSON: `{"list_papers":{"matter":"<id>"}}` answered by
`{"papers":{"papers":[{"file","path","size","modified"}],"skipped":[{"path","reason"}],"unread":["<folder>"],"truncated":false}}`.
Errors: `command.unknown_matter` for a matter the host does not know, `matter.folder_missing`
when the folder is gone or its `matter.json` names another matter.

**The folder as it is now, by the checkpoint's rules.** `Matter::papers` (crate `matters`) walks
the folder with the checkpoint walk's own code (`walk.rs`): the same excluded names (`.git`,
`.clauseo`, `~$*`, `*.tmp` and the rest), never a symlink, the same size cap and cloud-only probe,
under the same no-download guard. `papers` are the files a checkpoint versions; a file over the
cap or cloud-only goes in `skipped` with the reason a checkpoint gives. The listing only stats:
no file is opened, so an unreadable file is a paper (a checkpoint tries to version it). A
subfolder that does not read (permission denied, or a cloud folder not downloaded) cannot be
listed, but a checkpoint keeps the files it held as they were (`keep_unread`), so the folder
goes in `unread`: the window can say part of the matter is not on this computer instead of
showing those papers as gone.

**Bounded.** At most `MAX_PAPERS` (2,000) entries, papers, skipped files and unread folders
together, the first by path bytes (a folder counted where `<folder>/` sorts); `truncated` says
the folder holds more. The walk takes each folder's entries in
path-byte order (a folder `a` sorts as `a/`) and stops at the entry past the cap, so it never
reads a folder that sorts after it, and takes the stat of no file that does but each folder's
first file. That first stat tells whether the folder can be entered: a folder whose every stat
is denied (mode `0o444`) is unread, as a checkpoint has it, even when the cut falls inside it.

**Where it runs.** On the matter's worker thread, as checkpoints do; the actor only adds the
ids. No push event: the window asks again after `checkpoint_recorded` or `outside_edits`.

**File ids.** The host keeps one registry of file refs, both ways (`FileRefId` to path and path
to `FileRefId`), for its run. `ListPapers` and `RegisterFile` both use it, so a path (as written)
gets the same id from either, every time; it was one fresh id per `RegisterFile` call before.
A prompt resolves the id to a path when it is sent, and the log stores the resulting link, never
the id, so nothing depended on a fresh id.

**Measured** (iris-vm, 8 vCPU Debian VM, criterion medians under the shared build lock,
`crates/matters/benches/papers.rs`, 2,000-file cap). The plain walk (the whole folder, sorted,
then the first 2,000) and the ordered walk that stops at the cap give the same answer (checked
on each fixture before measuring, and by `walk_first`'s unit test at every limit):

| Folder | Plain walk | Ordered walk, kept |
|---|---|---|
| 1,000 files in 100 folders | 3.49 ms | 3.45 ms |
| 10,000 files in 100 folders | 27.4 ms | 5.14 ms |
| 10,000 files in the folder itself | 24.9 ms | 10.6 ms |

The stat cache cannot make it cheaper: it vouches for a file's content, and the listing reads no
content, only the stat the cache would be compared against.

## Consequences

- The window gets the folder as the lawyer left it, and the same rule for what is versioned in
  the list and in the history: a file it can see is on the wall exactly when the next checkpoint
  would version it. A folder it cannot read is named in `unread`, while the history keeps its
  files as they were.
- A folder of more than 2,000 files shows its first 2,000 by path; paging is a later query.
- The registry grows by one entry per distinct path listed or registered during a run (at most
  2,000 per listing); ids do not survive a restart, as before.
- Another spelling of the same file (case, `..`, a symlinked parent) gets another id.
- A later change to what a checkpoint versions changes the list with it, since both use
  `walk.rs`.

## Status

accepted

## Date

2026-11-01
