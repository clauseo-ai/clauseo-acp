# 0007: Matter history format

## Context

Clauseo saves a checkpoint of the matter folder before and after every turn, so that any step can be
undone and every version is kept. The history lives in a shadow git repository that `clauseo-matters`
writes in process with gix: no git binary, no libgit2. Later versions of Clauseo must read what this
version writes, so this record fixes the format: where things live, what each file holds, how
checkpoints are named, which files are left out, how objects are written and in what order they are
synced. It also records the change to the `matters` row of `ARCHITECTURE.md`.

The constraints: nothing of the history may sit in the matter folder, where the agent works and could
read or damage it; a `.git` the lawyer already has is theirs; a checkpoint must never download a
cloud-only file; checkpointing a turn is a hot path (the `checkpoint_per_turn` budget: 200 ms for 500
documents with 3 changed).

## Decision

### Layout

- The matters root is a folder the host passes (on Windows under `%LOCALAPPDATA%`, never `Roaming`).
  Each matter has `<matters root>/<matter id>/history.git`, where the matter id is the hyphenated
  lowercase UUID.
- `history.git` is a bare-style git dir. At creation it holds exactly:
  - `HEAD`: `ref: refs/heads/clauseo` and a newline: a branch that is never born (no commit is ever
    made on it). `git fsck` reports a `HEAD` naming a ref outside `refs/heads/` as `badHeadTarget`;
    an unborn branch it accepts. Opening a matter rewrites `HEAD` when it holds anything else.
    Clauseo never reads it otherwise.
  - `config`:
    ```
    [core]
    	repositoryformatversion = 0
    	filemode = false
    	bare = false
    	logallrefupdates = false
    	worktree = "<absolute path of the matter folder>"
    ```
    `core.worktree` is quoted, with `\`, `"`, newline and tab escaped (`\\`, `\"`, `\n`, `\t`);
    it must be absolute and valid UTF-8. Opening the matter rewrites it when the folder
    was moved and located again, so it names the folder the matter was last opened from.
    Clauseo reads it back for that comparison and for one more: repairing a deleted or
    unreadable `matter.json` (`Matter::relink`) only in the folder it names.
  - `objects/` and `refs/`, empty.

  No hooks, no `description`, no `info/`, no reflogs (`logs/`), no packs, no attributes, no filters,
  no LFS.
- The matter folder holds one thing of Clauseo's: `.clauseo/matter.json`, version 1:
  `{"version": 1, "matter_id": "<uuid>", "created_at": <milliseconds since the Unix epoch>}`, written
  by temp file and rename. Readers accept any `version` (or none) and ignore keys they do not know;
  they need `matter_id` and `created_at`. A later format that drops either uses a new file name. An
  import keeps a `matter.json` naming the matter it imports, byte for byte, and renames one that
  does not read to `.clauseo/matter.old-<UTC stamp>.json` before writing its own (decision record
  0022). Opening a
  matter whose folder is gone fails with `matter.folder_missing`; one whose folder has no
  `matter.json`, has one that does not read, or has one naming another matter fails with
  `matter.folder_not_a_matter` (`matter.folder_missing` before 2026-11-07, decision record 0003).
  Both are the "Locate folder" answer. On Windows
  `.clauseo` gets `FILE_ATTRIBUTE_HIDDEN` (keeping its other attributes) when `matter.json` is
  written and again on open if a zip or a sync dropped it; a failure to set it is logged only.
- `Marker`, chosen per create or import, is what else goes in the folder: `None` (nothing) or
  `EmptyRepository`. `EmptyRepository` runs a plain non-bare `git init` only when the folder has no
  `.git`: no commits, no remote, never a `gitdir:` file, unrelated to the shadow repository, and
  that repository is then excluded like any lawyer's `.git`. `DEFAULT_MARKER` is `None` until the
  Codex-without-git verdict; that verdict changes only that constant. This version refuses `EmptyRepository` with `command.invalid` until it is built.
- Repositories are opened with gix's isolated options: no system, global or user git config and no
  `GIT_*` environment variable applies. Clauseo never sets an environment variable, so agents never
  see `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE` or `GIT_CONFIG*` from it.
- A `.git` in the matter folder (a folder or a `gitdir:` file) belongs to the lawyer. Clauseo never
  reads or writes it and leaves it out of every checkpoint.

### The timeline

- One ref: `refs/clauseo/timeline`. No branches (`HEAD`'s `refs/heads/clauseo` never exists), no
  tags, no other refs. It always points to a commit.
- Each checkpoint is one commit whose single parent is the commit the ref names (its tip), whichever
  thread made it; the first (`import`) checkpoint has no parent. This version always writes a
  trailer, so the tip is the previous checkpoint; a later version may put a commit without one
  there, and a writer still builds on the tip. Nothing rewrites the timeline: undo and restore add
  commits.
- No empty commits. A checkpoint whose tree equals the tip's tree, and whose skip record (below)
  equals the tip's, reuses the last checkpoint's id instead of committing, when the tip's tree is
  that checkpoint's tree; otherwise it commits.
- Author and committer are `Clauseo <>` (name `Clauseo`, empty email), at the checkpoint's time in
  whole seconds, offset `+0000`. The host passes the time; Clauseo reads no clock.
- Every file is stored as mode `100644`. Symlinks and other non-regular files are not versioned.
- The message is a short summary line, a blank line, and trailers:

  ```
  After turn: 3 files changed

  Clauseo-Checkpoint: <checkpoint uuid>
  Clauseo-Kind: after-turn
  Clauseo-Thread: <thread uuid>
  Clauseo-Turn: <turn uuid>
  ```

  `Clauseo-Checkpoint` is the `CheckpointId` the host minted; the commit id never leaves `matters`
  and `store`. `Clauseo-Kind` is one of `import`, `before-turn`, `after-turn`, `outside-edits`,
  `undo`, `restore`, `revisions` (tracked changes of a Word file accepted or rejected, no thread or
  turn). `Clauseo-Thread` and `Clauseo-Turn` are present when the checkpoint has them
  (turn checkpoints, and an undo's when the host passes them). Writers emit them in that order.
  Readers take the message's last paragraph, accept the trailers in any order, ignore keys they do
  not know, read a kind they do not know as `Unknown`, and need only `Clauseo-Checkpoint`; a commit
  without one is not a checkpoint. Writers never reuse an id; a reader that finds one id on two
  commits takes the later. The summary line is for people and is never parsed.
- What was in the folder at a turn's start (orchestrator, 2026-11-11, stage D; found on a
  Windows laptop, where an agent's read of a OneDrive placeholder downloaded it, the room called it
  a new paper, and Undo would have deleted it). Undo deletes a file only when the history proves it
  was absent when the step began; the stat cache cannot prove that (it is rebuilt without skipped
  files, or lost), so each checkpoint commit records it.
  - **The skip record.** Every checkpoint commit has `Clauseo-Skips`, after the trailers above:
    `none` when its walk skipped no file and listed every folder, otherwise the id of a blob
    holding the record. No tree names the blob; it is stored and made durable like the commit's
    other objects, and kept, since Clauseo never prunes. The blob is UTF-8 text, a header line
    `clauseo-skips 1`, then one line per entry sorted by path bytes:

    ```
    clauseo-skips 1
    skipped cloud-only 18 1700000000.000000000 notes.txt
    unread Sub
    ```

    `skipped <reason> <size> <mtime seconds>.<nanoseconds> <path>` is a file not versioned (over
    the cap, or cloud-only and never versioned), with its size and mtime as the stat cache keeps
    them; `unread <folder>` is a folder that did not list (not downloaded, permission denied), of
    which nothing is known. The path runs to the end of the line (no control character). A header,
    or any line, a reader cannot read makes the whole record unreadable (a path left out would look
    absent); a reason it does not know reads as unknown. A commit
    without `Clauseo-Skips`, or whose record does not read, has no record: nothing about its
    walk's skips is known.
  - **Reuse.** A checkpoint reuses the last one only when the tree **and** the record are
    unchanged (a new placeholder, or a skipped file's new size or mtime, makes a checkpoint with no
    file changed), so the checkpoint a turn starts at records the folder as the turn found it.
  - **Files read for the first time.** A file the last commit's record (for a tip with none, the
    stat cache) has as skipped that a walk reads was there before: never an added file. A file new
    to the folder with the size and mtime of a skipped file the walk no longer finds is that file
    moved or renamed, and counts the same; so does one with the size and mtime of a `stale` file
    the walk no longer finds. Writers add one line per such file after `Clauseo-Skips`, each list
    sorted by path bytes:

    ```
    Clauseo-Baseline: cloud-only notes.txt
    Clauseo-Was-Skipped: over-size-cap Exhibits/scan 2.pdf
    ```

    The value is the reason (`cloud-only`, `over-size-cap`; one a reader does not know reads as
    unknown), one space, then the path exactly as in the tree (spaces kept, a last one included;
    readers trim only line breaks). A walk knows each reason; one kept from the cache under a
    folder that did not list, or a tip with no record, is told by its size against the cap.
    `Clauseo-Baseline` is read with the size and mtime it was skipped with, so unchanged: in the
    commit's tree, but not a change (no `FilesChanged` names it). That a sync client keeps both
    when it downloads a file is assumed, not verified (OneDrive included); one that does not makes
    the file `Clauseo-Was-Skipped` instead, listed but still kept by undo. A versioned file that
    was cloud-only and not read (`stale` in the record), read with the size and mtime it had then,
    is a baseline too: its content is what the folder held (a colleague's edit in the cloud, say).
    `Clauseo-Was-Skipped` is read otherwise, or moved: a change (listed as added, never a rename's
    new name).
  - **Diffs.** Any diff of two checkpoints (a step's, undo's, "edited since", a compare either
    way round) reads the first-read trailers of the checkpoints after the earlier up to the later:
    each baseline is compared from its content at the checkpoint that read it, so one unchanged
    since is no change. A compare with the folder does the same, and takes a file the folder holds
    unchanged since the last commit's record skipped it as its baseline too.
  - **Undo.** Undo deletes a path (an added file, a rename's new name) only when the step's
    first checkpoint has a record and the path is neither a skipped file in it nor under one of
    its unread folders, nor named by the step's first-read trailers, and the start record has no
    unread folder at all: a file the step added may have come out of one (it may list for a while
    and stop again), so then every added file is kept, as `unread_folder`. It never writes a path those trailers name, nor one the start record has as
    `stale` (its content then unknown). Any other file is kept, no delete and no write, and
    reported (`Undone::unversioned`, `StepUndone.unversioned`) with why: the skip reason,
    `unread_folder`, or `unknown` (no record: a step from before this, whose added files are all
    kept, the agent's own included). Modified and deleted files have earlier blobs and are put
    back as ever.
  - **Where a delete goes** (orchestrator, 2026-11-13, after the Windows dry run lost a lawyer's
    file to Undo). Undo's deletes, and those of a restore that takes back a restore, go to the
    OS's trash first, so the lawyer can take the file back from there: on macOS the Trash,
    through `NSFileManager`'s `trashItemAtURL` only (never Finder, which asks for Automation
    permission); on Windows the Recycle Bin, through `IFileOperation` with no UI, tried only on
    a fixed drive and refused when the shell would delete for good. A file the trash cannot take
    (a share, a removable drive, a failed move) is deleted, but only when the history holds it
    as the folder has it: the save before the write-back (the `outside-edits` checkpoint, whose
    failure fails the undo or restore before any delete) committed it, and the tip's tree has
    that blob. Otherwise it stays and the undo fails. One warning per write-back gives the
    count and the failure's kind, never a path. Linux keeps the plain delete. Tests set a folder
    as the trash (`Settings::trash`), so no run fills the machine's Trash.
  - An older Clauseo reading a newer history ignores these keys and undoes as it did.
- There is no `Clauseo-Seq` trailer: the commit is written before the host logs
  `CheckpointRecorded`, so the `seq` is not known yet, and the parent chain already orders the
  timeline. Opening a matter walks the first-parent chain from the ref and rebuilds the map from
  checkpoint ids to commits from the trailers, so `matters` never needs `store`, and `store`'s
  checkpoints table can be rebuilt from the repository.

### What is versioned

- Never versioned and never listed, by name, ASCII-case-insensitively on every OS (the same folder may
  sync between Windows and macOS): files matching `~$*`, `*.tmp` (which covers `~WRL*.tmp` and
  Clauseo's own `.clauseo-<n>.tmp` temp files), `Thumbs.db`, `desktop.ini`, `.DS_Store`,
  `.~lock.*#`, macOS's AppleDouble files `._*` (FAT, exFAT, SMB and NAS shares); a file named
  `.git` or `.clauseo`; and any folder whose name starts with `.`
  (`.git`, `.clauseo`, `.claude`, `.codex`, `.agents`, `.github`), with everything under it, at any
  depth. Other dot-files (`.gitignore`) are papers. The rule is
  `clauseo_protocol::is_excluded_path`.
  A history written before a folder was covered keeps its files in old trees, but every reader
  (changes, versions, compare, undo, restore, a logged turn's files) leaves them out, so they are
  never reported deleted and never written or deleted by undo. A skip record never lists such a
  path (one read from an older record is dropped), and a paper an old step moved out of such a
  folder (same blob) is kept by undo, not deleted.
- A file over the per-file size cap (512 MB by default; the stat cache keeps 32-bit sizes, so a cap of
  4 GiB or more acts as `u32::MAX` bytes) is never read and is not in the tree. A file that grows over
  the cap leaves the tree and is reported as skipped, never as deleted; so does one found over it
  while it is read (still copying in when the turn ended, say).
- A cloud-only placeholder is never read (macOS: the `SF_DATALESS` flag; Windows:
  `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` or `FILE_ATTRIBUTE_OFFLINE`; checked behind the
  `PlaceholderProbe` trait). A versioned file that becomes cloud-only keeps its previous tree entry,
  since dehydration does not change content. One that was never versioned is not in the tree.
- A name that is not valid UTF-8, or whose path fails `MatterPath`'s check, is skipped with a warning.
- A subfolder that does not read (permission denied, say) is skipped with a warning, and the files
  the last checkpoint had under it keep their tree entries, as a cloud-only file does. The matter
  folder itself not reading fails the checkpoint.

### The stat cache

- `history.git/index` is a git index file (gix-index) describing the tip's tree plus the skipped
  files: one entry per path, sorted by path bytes, mode `100644`, uid and gid 0, and git's
  32-bit stat fields: ctime and mtime (seconds and nanoseconds), device, inode, size. On Windows
  `ctime` holds the creation time and device and inode are 0.
- The entry's state is written with git's own flags, so `git status` with this index reads it
  sensibly:
  - no flag: the file was read; the entry's id is its blob, in the tree;
  - `skip-worktree`: in the tree with this earlier blob, but the file is cloud-only now and was not
    read; it is checked again on every walk;
  - `intent-to-add` with the empty blob's id: not in the tree (over the cap, or cloud-only and never
    versioned), kept so an unchanged skipped file is not listed on every checkpoint. Unchanged
    here means the same size and modification time: a sync client touches a placeholder's other
    attributes without changing it.

  Either extended flag makes the file index version 3.
- It is a cache. A file that is missing, does not decode, or whose in-tree entries do not match the
  tip's tree is rebuilt from that tree, at the cost of reading every file once. Later
  versions may discard it the same way.
- A file is read again when its stat differs from its entry, or when its mtime is within 2 s of (or
  after) the index file's own modification time ("racily clean", as git does), so a same-size rewrite
  in the same second is seen. Nothing else is read. A checkpoint that read a file again only for
  being racily clean writes the cache even when no entry changed, so its time moves past the file,
  as git rewrites its index. Clauseo's own walk (`read_dir` plus
  `symlink_metadata`), not gix's status or dirwalk, decides this, so a cloud-only file is never
  opened.

### Objects

- Loose objects only. A blob whose first bytes say it is already compressed is written at zlib
  level 0: `PK\x03\x04` (`.docx`, `.xlsx`, `.pptx`, `.zip`), `%PDF`, JPEG `FF D8 FF`, PNG
  `89 50 4E 47`, GIF `GIF8`, WebP (`RIFF` and `WEBP` at byte 8), HEIC (`ftyp` at byte 4). Every other
  blob, and every tree and commit, is written at level 1, gix's default. A level-0 object is ordinary
  zlib that every git reads, so the rule can change later without rewriting history.
- Hashing is gix's own collision-detecting SHA-1.
- An object already present is inflated in full however old its file is (one left by a run that
  never published, or one from before the last publish that a revert names again), and never
  written again once it checks out: for a file of up to 8 MiB, a tree or a commit, whose bytes the
  checkpoint holds in memory, it must inflate to exactly those bytes (compared, not hashed); for a
  larger file, which is streamed, it must hash to its id. A torn or wrong one is written again:
  from the bytes in memory, or for a streamed file from a second read of the file, itself hashed
  as it is stored. The cost falls on a revert: on the 2026 reference VM (AMD EPYC), about 5.9 ms
  for an 8 MiB text file and 1.5 ms for an 8 MiB `.docx`, and for a file over 8 MiB about 1.4 ms
  per MiB of text (about 57 ms for 40 MiB), on top of reading the file.
- Retention: every version is kept. No pruning, squashing, gc or repack. `Retention::KeepAll` is
  passed at open, so a later policy is a new variant.
- No automatic repack either (orchestrator, 2026-11-04, after the R9 spike,
  clauseo-ai/clauseo-acp-private#224). Loose growth was about 131 MB per 100 turns (20 Word
  documents of 30 KB to 2 MB, 3 changed per turn). `git repack -a -d` saves 25 to 38 % on whole
  `.docx` blobs, but gix 0.89 writes no delta pack (0 % on whole blobs). A loose-to-packed repack
  of 300 such turns took 17.5 s wall on Windows (2 vCPUs, Defender on) and 5.0 s on the Mac mini;
  one every 50 turns took 4.9 s and 1.7 s, and 60 checkpoints taken during repacks hit no lock or
  missing-object error. A clean Windows has no `git`, and a Mac without the Command Line Tools
  shows Apple's install dialog when `/usr/bin/git` runs, so a repack means bundling git (installer
  size, GPL-2 source offer) or trusting a `git` on `PATH` that few lawyers have. Performance comes
  before size, and the growth is acceptable for release. Revisit when gix writes delta packs or
  users run short of disk. Numbers: `measurements/g9-versions.md` and
  `measurements/g9-versions/repack-timing.md` in the private repository.

### The sync order

gix 0.89 syncs no loose object (it writes a temp file and renames it), so Clauseo syncs, with one full
flush per checkpoint:

- macOS: a plain `fsync` on each object file the checkpoint wrote or found already present and
  sound by that full check (an object a killed run left behind may never have been synced), and
  on each `objects/xx` folder it touched (and `objects/` when a new `xx` folder was made); one
  `F_BARRIERFSYNC`; then the ref moves (checked against the expected previous commit) and the
  index is written, each by temp file and rename, each plain-`fsync`ed with its folder; then one
  `F_FULLFSYNC`. Objects reach the disk before the ref that names them, and the last flush makes
  everything durable before the checkpoint returns. `File::sync_all` is `F_FULLFSYNC` on macOS,
  so it is never called per file.
- Linux: `File::sync_all` on each written file and each changed folder, in the same order.
- Windows: `File::sync_all` on each written file, in the same order; folders are not synced.

Files Clauseo writes into the matter folder (undo and restore) go to a temp file in the same
folder, are synced the same way, and are renamed over the target. A file is never truncated in
place. Undo writes every temp file first, then makes its deletions, then renames the temp files
into place, so on a case- or normalisation-insensitive file system (APFS by default, NTFS) a
deletion never removes a file just put back. Neither writes a name the folder holds only spelled
otherwise (another case or normalisation), which would replace that other file; a restore never
deletes.

### Unicode names

Added 2026-11-03, when Hindi content became a requirement. A Devanagari name can be written in
more than one Unicode form: `ऱ` is U+0931 composed (NFC) or `र` + nukta U+093C decomposed (NFD),
and the nukta letters U+0958-095F (`क़`) are composition exclusions, so NFC writes them
decomposed while a keyboard may type the precomposed letter. APFS (macOS) finds a name in any
form; ext4 (Linux) and NTFS (Windows) keep each form a separate file.

- `MatterPath` stays byte for byte (`crates/protocol/src/ids/strings.rs`): ids carry exactly
  the name the file system gave. Listing papers, checkpoints, `files_at`, comparisons, undo and
  restore store and return names as stored, never normalised. The crate walks the folder itself
  and writes its own trees and config (no `core.precomposeunicode`, no git worktree code), so
  the name a checkpoint records is the one `read_dir` returned, and undo and restore write those
  bytes back: on macOS an NFD name comes back NFD.
- Two names that differ only by form are two files on Linux and Windows and one on macOS; tests
  assert each OS's answer (`crates/matters/tests/matters/devanagari.rs`).
- A match that refuses compares names composed (NFC) on every OS, with case ignored on macOS and
  Windows, so a name in either form is caught: Word's owner file (`~$…`) beside a document,
  in `held_open` and `held_open_at` (a non-ASCII name the exact stats miss is looked for in the
  folder's listing, read at most once per folder by the guard and capped at 20 000 entries; a
  listing that fails, or an owner file past the cap, is not found: it fails open, as `held_open`
  does for any other error), the refused-folder check, `same_or_nested` ("already a matter", a nested
  matter, the agents' data folder) and the host's pre-write guard. On Linux and Windows that may
  refuse a sibling file that only shares the name's letters: the safe side.
- A match that allows compares as the OS's file system does: byte for byte on Linux, ignoring
  case on Windows, ignoring case and form on macOS. That is `matter_path_in` (a path counted as
  the matter's, which joins a turn's paths and frees a folder for the guard's "outside" reads)
  and the folder `Matter::relink` writes `matter.json` into; folding there would cover a sibling
  spelled in the other form.

### Security

- Clauseo never opens a matter's own `.git`. It is only ever a name, refused by the sandbox and the
  fence (decision record 0038).
- Clauseo never spawns `git`, or any tool a repository configures, as the lawyer.
- The shadow `history.git` lives outside the matter folder, in Clauseo's data area, and is opened
  only with gix's isolated options: no system, global, user or `GIT_*` environment config, and no
  hooks, filters, diff drivers or fsmonitor. The test
  `recorded_worktree_ignores_the_global_system_and_user_config` (in
  `crates/matters/src/history/tests/repo.rs`) pins that.

### `ARCHITECTURE.md`

The `matters` row becomes: Owns "Shadow repos via gix: create and import, checkpoints, undo, restore,
compare, file watching; retention after G9"; May depend on "protocol, gix, notify, serde, serde_json,
tracing, dunce; nix (macOS only)"; Never depends on "tauri"; Status `exists`. `dunce` is the
workspace's canonicalize (the root `clippy.toml` bans std's), used by the refused-folder check.
Since the Unicode names section, also `unicode-normalization`, for the NFC of the refusing
matches.

## Consequences

- The history is a plain git repository: `git --git-dir=<matters root>/<id>/history.git log
  refs/clauseo/timeline` reads it, and `git fsck --strict` accepts it (git 2.54.0, on a repository
  this version made: an import and one checkpoint).
- A later version must keep reading this layout, `matter.json` version 1, the single ref, the
  trailers and level-0 and level-1 loose objects. It may change the summary line, the compression
  rule and the stat cache freely; it may add trailers and kinds (older readers ignore them).
- Rebuilding the timeline reads every commit on open, which grows with the number of checkpoints.
- Loose objects only, kept for ever, so `history.git` grows with every changed document (about
  1.3 MB per turn in R9's corpus) until a retention policy or a delta-pack writer exists.
- A checkpoint costs at most one full drive flush, plus a barrier, plus plain syncs of new files.
- Not tested by pulling the power: durability rests on Apple's documented `F_FULLFSYNC` behaviour.
  The placeholder attributes are unverified per OS until a measurement checks them.

## Status

accepted

## Date

2026-10-26
