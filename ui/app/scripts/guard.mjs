// The scripts' guard over a tracked file, and the one script allowed to change files:
// ui/app/.oxlintrc.json bans the node:fs calls that write in every other script under scripts/
// (the tests that build scratch trees aside, each named there). Any
// script that edits a tracked file calls it; the caller names the file, where its copy waits,
// itself, and which leftover edits are its own. Its rules are written out here only:
// - The file and the copies' folder are repo paths: plain names, no `..`, no root.
// - Before the first edit, the original bytes are copied to `<backupDir>/<repo path>`. A copy is
//   never replaced: it is written to its temporary file, created only when missing, and then
//   hard-linked into place, which fails when a copy exists, so the check and the create are one
//   step even for two runs at once.
// - Every write to the file goes to a temporary file in the same folder (`.<name>.<tool>.tmp`),
//   gets the file's mode, then is renamed over it, so a kill never leaves half a file.
// - The original goes back only while the file holds exactly the bytes last written. Anything else
//   is the developer's edit: nothing more is written, both files stay, and a ConflictError names
//   them. Any failed restore fails the run and keeps the copy (`restoreFailure`).
// - A killed run leaves its copy. The next run's `recoverFile` puts it back only when the caller's
//   `isOwnEdit` says the file is the copy plus an edit the tool itself makes.
// - `ensureLink` is the guard's one write that is not an edit of a tracked file: it makes an
//   ignored link to a folder (a junction on Windows, which needs no symlink rights) where nothing
//   or only a stale link is, and leaves a link that already reaches the folder alone. A file or
//   folder at the path is the developer's and is never replaced.
// The guard starts no process: .oxlintrc.json bans child processes in this file.

import {
  closeSync,
  existsSync,
  fchmodSync,
  fsyncSync,
  linkSync,
  lstatSync,
  mkdirSync,
  openSync,
  readFileSync,
  realpathSync,
  renameSync,
  rmdirSync,
  statSync,
  symlinkSync,
  unlinkSync,
  writeSync,
} from "node:fs";
import { basename, dirname, isAbsolute, join, resolve, win32 } from "node:path";

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
  for (const path of [rel, backupDir]) {
    if (!isRepoPath(path)) {
      throw new Error(
        `refusing to guard \`${path}\`: it is not a relative path inside the repo; pass a repo path, such as ui/app/src/App.tsx`,
      );
    }
  }
  if (!/^[a-z][a-z0-9-]*$/.test(tool)) {
    throw new Error(
      `refusing the tool name \`${tool}\`: it names temporary files, so use lowercase letters, digits and -`,
    );
  }
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

/**
 * Whether `rel` is a relative path inside the repo: one or more plain names, split on `/` or `\`,
 * none empty, `.` or `..`, and no root or drive. Stricter than xtask's `is_repo_path`, which
 * lets `a//b` and `a/./b` through.
 */
export function isRepoPath(rel) {
  if (
    typeof rel !== "string" ||
    isAbsolute(rel) ||
    win32.isAbsolute(rel) ||
    /^[A-Za-z]:/.test(rel)
  ) {
    return false;
  }
  return rel.split(/[\\/]/).every((part) => part !== "" && part !== "." && part !== "..");
}

/**
 * Makes repo path `rel` under `root` a link to the folder `target`, written relative to `rel`'s
 * folder (`../components`). A junction on Windows, which needs no symlink rights or Developer
 * Mode; a symlink elsewhere. Returns "kept" (a link already reaching the folder), "made", or
 * "remade": a link that is dangling or reaches elsewhere is replaced, since removing a link loses
 * nothing, and a junction keeps an absolute target, so moving or copying a Windows checkout leaves
 * one like that. Throws when `target` is no folder or a file or folder is at `rel`: those are
 * never replaced. It shares nothing with FileGuard; it lives here because .oxlintrc.json lets only
 * this file under scripts/ write. Its one caller is components-link.mjs.
 */
export function ensureLink({ root, rel, target }) {
  if (!isRepoPath(rel)) {
    throw new Error(`refusing to link \`${rel}\`: it is not a relative path inside the repo`);
  }
  const path = join(root, rel);
  const folder = resolve(dirname(path), target);
  if (!statSync(folder, { throwIfNoEntry: false })?.isDirectory()) {
    throw new Error(`cannot link ${rel} to ${target}: ${folder} is not a folder`);
  }
  const there = lstatSync(path, { throwIfNoEntry: false });
  if (there === undefined) {
    symlinkSync(target, path, "junction");
    return "made";
  }
  if (there.isSymbolicLink()) {
    if (realpathOrUndefined(path) === realpathSync(folder)) {
      return "kept";
    }
    unlinkSync(path);
    symlinkSync(target, path, "junction");
    return "remade";
  }
  const what = there.isDirectory()
    ? "a folder"
    : "a plain file (on Windows, likely git's stand-in for the link it once carried)";
  throw new Error(
    `${rel} should be a link to ${target}, but it is ${what}; move it away, then run \`pnpm -C ui install\` to make the link`,
  );
}

function realpathOrUndefined(path) {
  try {
    return realpathSync(path);
  } catch {
    return undefined;
  }
}

/**
 * Writes all of `bytes` to `fd` with `write` (`writeSync`), which may write fewer bytes than
 * asked. Throws when a write makes no progress.
 */
export function writeAll(fd, bytes, write = writeSync) {
  let at = 0;
  while (at < bytes.length) {
    const wrote = write(fd, bytes, at, bytes.length - at);
    if (!(wrote > 0)) {
      throw new Error(`a write of ${bytes.length - at} bytes made no progress`);
    }
    at += wrote;
  }
}

/** The temporary file a write by `tool` to `path` goes through, in the same folder. */
export function tmpOf(path, tool) {
  return join(dirname(path), `.${basename(path)}.${tool}.tmp`);
}

/**
 * Writes `bytes` to a temporary file next to `path`, gives it `path`'s mode, syncs it, then renames
 * it over `path`. `path` exists: the guard only writes over a file it has saved.
 */
function writeThrough(path, tool, bytes) {
  const tmp = tmpOf(path, tool);
  const fd = openSync(tmp, "w");
  try {
    writeAll(fd, bytes);
    fchmodSync(fd, statSync(path).mode & 0o7777);
    fsyncSync(fd);
  } finally {
    closeSync(fd);
  }
  renameSync(tmp, path);
}

/**
 * Writes `bytes` to `paths.backup` without ever replacing a file there: to the backup's temporary
 * file, created only when it is missing, then hard-linked to the backup by `link` (`linkSync`),
 * which fails when the backup exists. Throws a ConflictError when either is in the way; the
 * temporary file is deleted again either way. `link` is a parameter so a test can let another
 * run win between the two steps.
 */
function writeNewCopy(paths, bytes, link) {
  mkdirSync(dirname(paths.backup), { recursive: true });
  const tmp = tmpOf(paths.backup, paths.tool);
  let fd;
  try {
    fd = openSync(tmp, "wx");
  } catch (error) {
    if (error?.code === "EEXIST") {
      const tmpRel = `${dirname(paths.backupRel)}/${basename(tmp)}`;
      throw new ConflictError(
        `could not back up ${paths.rel}: ${tmpRel} is left from an interrupted save`,
        `compare ${tmpRel} with ${paths.rel}, keep what you want, then delete ${tmpRel}`,
      );
    }
    throw error;
  }
  try {
    try {
      writeAll(fd, bytes);
      fsyncSync(fd);
    } finally {
      closeSync(fd);
    }
    link(tmp, paths.backup);
  } catch (error) {
    if (error?.code === "EEXIST") {
      throw copyExists(paths);
    }
    throw error;
  } finally {
    unlinkIfThere(tmp);
  }
}

/** Deletes `path`; one already gone (another run's `recoverFile` took it) is no error. */
function unlinkIfThere(path) {
  try {
    unlinkSync(path);
  } catch (error) {
    if (error?.code !== "ENOENT") {
      throw error;
    }
  }
}

function copyExists(paths) {
  return new ConflictError(
    `a saved copy of ${paths.rel} already exists at ${paths.backupRel}`,
    `compare ${paths.rel} with ${paths.backupRel}, keep what you want, then delete ${paths.backupRel}`,
  );
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
  constructor(paths, { link = linkSync } = {}) {
    if (existsSync(paths.backup)) {
      throw copyExists(paths);
    }
    this.paths = paths;
    this.original = readFileSync(paths.file);
    this.last = this.original;
    writeNewCopy(paths, this.original, link);
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
