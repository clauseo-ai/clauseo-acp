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
  - `HEAD`: `ref: refs/clauseo/timeline` and a newline.
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
    was moved and located again. Clauseo never reads it back for anything but that comparison.
  - `objects/` and `refs/`, empty.

  No hooks, no `description`, no `info/`, no reflogs (`logs/`), no packs, no attributes, no filters,
  no LFS.
- The matter folder holds one thing of Clauseo's: `.clauseo/matter.json`, version 1:
  `{"version": 1, "matter_id": "<uuid>", "created_at": <milliseconds since the Unix epoch>}`, written
  by temp file and rename. Readers accept any `version` (or none) and ignore keys they do not know;
  they need `matter_id` and `created_at`. A later format that drops either uses a new file name. Opening a
  matter whose folder is gone, has no `matter.json`, has one that does not read, or has one naming
  another matter fails with `matter.folder_missing` (the "Locate folder" answer).
- Repositories are opened with gix's isolated options: no system, global or user git config and no
  `GIT_*` environment variable applies. Clauseo never sets an environment variable, so agents never
  see `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE` or `GIT_CONFIG*` from it.
- A `.git` in the matter folder (a folder or a `gitdir:` file) belongs to the lawyer. Clauseo never
  reads or writes it and leaves it out of every checkpoint.

### The timeline

- One ref: `refs/clauseo/timeline`. No branches, no tags, no other refs. It always points to a
  commit.
- Each checkpoint is one commit whose single parent is the previous checkpoint, whichever thread made
  it; the first (`import`) checkpoint has no parent. Nothing rewrites the timeline: undo and restore
  add commits.
- No empty commits. A checkpoint whose tree equals the last checkpoint's reuses that checkpoint's id
  instead of committing.
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
  `undo`, `restore`. `Clauseo-Thread` and `Clauseo-Turn` are present when the checkpoint has them
  (turn checkpoints, and an undo's when the host passes them). Writers emit them in that order.
  Readers take the message's last paragraph, accept the trailers in any order, ignore keys they do
  not know, read a kind they do not know as `Unknown`, and need only `Clauseo-Checkpoint`; a commit
  without one is not a checkpoint. Writers never reuse an id; a reader that finds one id on two
  commits takes the later. The summary line is for people and is never parsed.
- There is no `Clauseo-Seq` trailer: the commit is written before the host logs
  `CheckpointRecorded`, so the `seq` is not known yet, and the parent chain already orders the
  timeline. Opening a matter walks the first-parent chain from the ref and rebuilds the map from
  checkpoint ids to commits from the trailers, so `matters` never needs `store`, and `store`'s
  checkpoints table can be rebuilt from the repository.

### What is versioned

- Never versioned and never listed, by name, ASCII-case-insensitively on every OS (the same folder may
  sync between Windows and macOS): files matching `~$*`, `*.tmp` (which covers `~WRL*.tmp` and
  Clauseo's own `.clauseo-<n>.tmp` temp files), `Thumbs.db`, `desktop.ini`, `.DS_Store`,
  `.~lock.*#`; and any file or folder named `.git` or `.clauseo`, with everything under it.
- A file over the per-file size cap (512 MB by default; the stat cache keeps 32-bit sizes, so a cap of
  4 GiB or more acts as `u32::MAX` bytes) is never read and is not in the tree. A file that grows over
  the cap leaves the tree and is reported as skipped, never as deleted.
- A cloud-only placeholder is never read (macOS: the `SF_DATALESS` flag; Windows:
  `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` or `FILE_ATTRIBUTE_OFFLINE`; checked behind the
  `PlaceholderProbe` trait). A versioned file that becomes cloud-only keeps its previous tree entry,
  since dehydration does not change content. One that was never versioned is not in the tree.
- A name that is not valid UTF-8, or whose path fails `MatterPath`'s check, is skipped with a warning.

### The stat cache

- `history.git/index` is a git index file (gix-index) describing the last checkpoint's tree plus the
  skipped files: one entry per path, sorted by path bytes, mode `100644`, uid and gid 0, and git's
  32-bit stat fields: ctime and mtime (seconds and nanoseconds), device, inode, size. On Windows
  `ctime` holds the creation time and device and inode are 0.
- The entry's state is written with git's own flags, so `git status` with this index reads it
  sensibly:
  - no flag: the file was read; the entry's id is its blob, in the tree;
  - `skip-worktree`: in the tree with this earlier blob, but the file is cloud-only now and was not
    read; it is checked again on every walk;
  - `intent-to-add` with the empty blob's id: not in the tree (over the cap, or cloud-only and never
    versioned), kept so an unchanged skipped file is not listed on every checkpoint.

  Either extended flag makes the file index version 3.
- It is a cache. A file that is missing, does not decode, or whose in-tree entries do not match the
  last checkpoint's tree is rebuilt from that tree, at the cost of reading every file once. Later
  versions may discard it the same way.
- A file is read again when its stat differs from its entry, or when its mtime is within 2 s of (or
  after) the index file's own modification time ("racily clean", as git does), so a same-size rewrite
  in the same second is seen. Nothing else is read. Clauseo's own walk (`read_dir` plus
  `symlink_metadata`), not gix's status or dirwalk, decides this, so a cloud-only file is never
  opened.

### Objects

- Loose objects only. A blob whose first bytes say it is already compressed is written at zlib
  level 0: `PK\x03\x04` (`.docx`, `.xlsx`, `.pptx`, `.zip`), `%PDF`, JPEG `FF D8 FF`, PNG
  `89 50 4E 47`, GIF `GIF8`, WebP (`RIFF` and `WEBP` at byte 8), HEIC (`ftyp` at byte 4). Every other
  blob, and every tree and commit, is written at level 1, gix's default. A level-0 object is ordinary
  zlib that every git reads, so the rule can change later without rewriting history.
- Hashing is gix's own collision-detecting SHA-1.
- An object already present is never written again once its header decodes with the expected kind
  and size; one whose file is newer than the ref file (left by a run that never published) is
  inflated and hashed in full first. A torn one is written again.
- Retention: every version is kept. No pruning, squashing, gc or repack. `Retention::KeepAll` is
  passed at open, so a later policy is a new variant.

### The sync order

gix 0.89 syncs no loose object (it writes a temp file and renames it), so Clauseo syncs, with one full
flush per checkpoint:

- macOS: a plain `fsync` on each object file the checkpoint wrote or found already present (an
  object a killed run left behind may never have been synced), and on each `objects/xx` folder it
  touched (and `objects/` when a new `xx` folder was made); one `F_BARRIERFSYNC`; then the ref moves (checked
  against the expected previous commit) and the index is written, each by temp file and rename, each
  plain-`fsync`ed with its folder; then one `F_FULLFSYNC`. Objects reach the disk before the ref that
  names them, and the last flush makes everything durable before the checkpoint returns.
  `File::sync_all` is `F_FULLFSYNC` on macOS, so it is never called per file.
- Linux: `File::sync_all` on each written file and each changed folder, in the same order.
- Windows: `File::sync_all` on each written file, in the same order; folders are not synced.

Files Clauseo writes into the matter folder (undo) go to a temp file in the same folder, are synced
the same way, and are renamed over the target. A file is never truncated in place.

### `ARCHITECTURE.md`

The `matters` row becomes: Owns "Shadow repos via gix: create and import, checkpoints, undo, restore,
compare, file watching; retention after G9"; May depend on "protocol, gix, notify, serde, serde_json,
tracing, dunce; nix (macOS only)"; Never depends on "tauri"; Status `exists`. `dunce` is the
workspace's canonicalize (the root `clippy.toml` bans std's), used by the refused-folder check.

## Consequences

- The history is a plain git repository: `git --git-dir=<matters root>/<id>/history.git log
  refs/clauseo/timeline` reads it, and any git can repack or verify it.
- A later version must keep reading this layout, `matter.json` version 1, the single ref, the
  trailers and level-0 and level-1 loose objects. It may change the summary line, the compression
  rule and the stat cache freely; it may add trailers and kinds (older readers ignore them).
- Rebuilding the timeline reads every commit on open, which grows with the number of checkpoints.
- Loose objects only, kept for ever, so `history.git` grows with every changed document until a
  retention policy exists.
- A checkpoint costs at most one full drive flush, plus a barrier, plus plain syncs of new files.
- Not tested by pulling the power: durability rests on Apple's documented `F_FULLFSYNC` behaviour.
  The placeholder attributes are unverified per OS until a measurement checks them.

## Status

accepted

## Date

2026-10-26
