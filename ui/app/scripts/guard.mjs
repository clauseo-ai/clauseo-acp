// The scripts' guard over a tracked file, and the one script allowed to change files:
// ui/app/.oxlintrc.json bans the node:fs calls that write in every other script under scripts/
// (the tests that build scratch trees aside, each named there). Any
// script that edits a tracked file calls it; the caller names the file, where its copy waits,
// itself, and which leftover edits are its own. Its rules are written out here only:
// - Before the first edit, the original bytes are copied to `<backupDir>/<repo path>`. An existing
//   copy is never replaced.
// - Every write, to the file or to the copy, goes to a temporary file in the same folder
//   (`.<name>.<tool>.tmp`), then is renamed over the target, so a kill never leaves half a file.
// - The original goes back only while the file holds exactly the bytes last written. Anything else
//   is the developer's edit: nothing more is written, both files stay, and a ConflictError names
//   them. Any failed restore fails the run and keeps the copy (`restoreFailure`).
// - A killed run leaves its copy. The next run's `recoverFile` puts it back only when the caller's
//   `isOwnEdit` says the file is the copy plus an edit the tool itself makes.
// The guard starts no process: .oxlintrc.json bans child processes in this file.

import {
  closeSync,
  existsSync,
  fsyncSync,
  mkdirSync,
  openSync,
  readFileSync,
  renameSync,
  rmdirSync,
  unlinkSync,
  writeSync,
} from "node:fs";
import { basename, dirname, join } from "node:path";

/** A conflict over a guarded file: both files are kept, and `fix` says what to do with them. */
export class ConflictError extends Error {
  constructor(message, fix) {
    super(message);
    this.name = "ConflictError";
    this.fix = fix;
  }
}

/**
 * The file at repo path `rel` under `root`, its copy under `backupDir` (a repo path), and the
 * folder copies live in. `tool` names the run in messages and temporary files; `rerun` is the
 * command whose next run restores the file from its copy.
 */
export function guardPaths({ root, rel, backupDir, tool, rerun }) {
  return {
    rel,
    file: join(root, rel),
    base: join(root, backupDir),
    backup: join(root, backupDir, rel),
    backupRel: `${backupDir}/${rel}`,
    tool,
    rerun,
  };
}

/** The temporary file a write by `tool` to `path` goes through, in the same folder. */
export function tmpOf(path, tool) {
  return join(dirname(path), `.${basename(path)}.${tool}.tmp`);
}

/** Writes `bytes` to a temporary file next to `path`, syncs it, then renames it over `path`. */
function writeThrough(path, tool, bytes) {
  const tmp = tmpOf(path, tool);
  const fd = openSync(tmp, "w");
  try {
    writeSync(fd, typeof bytes === "string" ? Buffer.from(bytes) : bytes);
    fsyncSync(fd);
  } finally {
    closeSync(fd);
  }
  renameSync(tmp, path);
}

function readOrUndefined(path) {
  return existsSync(path) ? readFileSync(path) : undefined;
}

function conflict(paths, what) {
  return new ConflictError(
    `${paths.rel} ${what}; its saved copy is ${paths.backupRel}`,
    `compare the two, keep what you want, then delete ${paths.backupRel}`,
  );
}

/** Removes the copy, then the folders between it and the backup folder that are now empty. */
function dropCopy(paths) {
  unlinkSync(paths.backup);
  let dir = dirname(paths.backup);
  while (dir.startsWith(paths.base) && dir !== paths.base) {
    try {
      rmdirSync(dir);
    } catch {
      break;
    }
    dir = dirname(dir);
  }
}

/**
 * The run failure for an error from `FileGuard.restore`: a ConflictError as it is; any other error
 * (EPERM from the rename while a scanner holds the file, say) named with the copy, which stays
 * for the next run's `recoverFile`.
 */
export function restoreFailure(paths, error) {
  if (error instanceof ConflictError) {
    return { message: error.message, fix: error.fix };
  }
  return {
    message: `could not put back ${paths.rel}: ${error?.message ?? String(error)}; its saved copy is ${paths.backupRel}`,
    fix: `the next \`${paths.rerun}\` restores ${paths.rel} from ${paths.backupRel}; or copy it back by hand`,
  };
}

/**
 * What an interrupted run's copy means for the file as it is now (`current`, `undefined` when the
 * file is gone): `clean` when they are equal, `restore` when `isOwnEdit(current, copy)` (both
 * strings) says the difference is the tool's own edit, `missing`, or `conflict` for any other
 * difference.
 */
export function leftover(current, copy, isOwnEdit) {
  if (current === undefined) {
    return "missing";
  }
  if (current.equals(copy)) {
    return "clean";
  }
  return isOwnEdit(current.toString("utf8"), copy.toString("utf8")) ? "restore" : "conflict";
}

/**
 * Settles what an interrupted run left, before anything is written: deletes the temporary files
 * of an interrupted write, then puts the copy back when `leftover` allows it. Returns the line to
 * print, or `undefined` when there was no copy. Throws a ConflictError, keeping both files, when
 * the file changed in any other way or is gone.
 */
export function recoverFile(paths, isOwnEdit) {
  for (const tmp of [tmpOf(paths.file, paths.tool), tmpOf(paths.backup, paths.tool)]) {
    if (existsSync(tmp)) {
      unlinkSync(tmp);
    }
  }
  const copy = readOrUndefined(paths.backup);
  if (copy === undefined) {
    return undefined;
  }
  const run = `an interrupted ${paths.tool} run`;
  switch (leftover(readOrUndefined(paths.file), copy, isOwnEdit)) {
    case "clean":
      dropCopy(paths);
      return `deleted the copy of ${paths.rel} ${run} left; the file was unchanged`;
    case "restore":
      writeThrough(paths.file, paths.tool, copy);
      dropCopy(paths);
      return `restored ${paths.rel} from ${run}`;
    case "missing":
      throw conflict(paths, `is missing after ${run}`);
    default:
      throw conflict(paths, `changed after ${run}`);
  }
}

/** The guard over one file for one run: its original, its copy, and the bytes last written. */
export class FileGuard {
  /** Saves the original of the file as its copy. Nothing is edited when the save fails. */
  constructor(paths) {
    if (existsSync(paths.backup)) {
      throw new ConflictError(
        `a saved copy of ${paths.rel} already exists at ${paths.backupRel}`,
        `compare ${paths.rel} with ${paths.backupRel}, keep what you want, then delete ${paths.backupRel}`,
      );
    }
    this.paths = paths;
    this.original = readFileSync(paths.file);
    this.last = this.original;
    mkdirSync(dirname(paths.backup), { recursive: true });
    writeThrough(paths.backup, paths.tool, this.original);
    this.pending = true;
  }

  /** Throws a ConflictError, and stops writing, unless the file holds the bytes last written. */
  check() {
    const current = readOrUndefined(this.paths.file);
    if (current !== undefined && current.equals(this.last)) {
      return;
    }
    this.pending = false;
    const run = `this ${this.paths.tool} run`;
    throw conflict(
      this.paths,
      current === undefined ? `went missing during ${run}` : `changed during ${run}`,
    );
  }

  /** Writes `text` to the file, once it still holds the bytes last written. */
  write(text) {
    if (!this.pending) {
      throw new Error(`${this.paths.rel} is no longer guarded`);
    }
    this.check();
    const bytes = Buffer.from(text);
    writeThrough(this.paths.file, this.paths.tool, bytes);
    this.last = bytes;
  }

  /**
   * Puts the original back and deletes the copy, once the file still holds the bytes last written.
   * Safe to repeat. After a conflict, both files stay as they are.
   */
  restore() {
    if (!this.pending) {
      return;
    }
    this.check();
    this.pending = false;
    if (!this.last.equals(this.original)) {
      writeThrough(this.paths.file, this.paths.tool, this.original);
    }
    dropCopy(this.paths);
  }
}
