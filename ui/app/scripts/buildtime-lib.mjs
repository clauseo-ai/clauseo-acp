// The parts of buildtime.mjs that can be tested without a browser or a dev server, kept apart so
// buildtime-lib.test.mjs can test them: argument parsing, the commands from package.json, sizing
// the initial JS of a build, and the guard over src/App.tsx.

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
import { brotliCompressSync, constants as zlib } from "node:zlib";

/**
 * The text the HMR probe edits in src/App.tsx. App.tsx names this constant in a comment; change
 * the two together.
 */
export const ROOT_TAG = '<div id="app-root"';

/**
 * `original` with `data-buildtime="<n>"` added right after ROOT_TAG. Throws unless ROOT_TAG
 * appears exactly once, so the edit can only land on the element's opening tag.
 */
export function marked(original, n) {
  const at = original.indexOf(ROOT_TAG);
  if (at === -1 || original.indexOf(ROOT_TAG, at + 1) !== -1) {
    throw new Error(`ROOT_TAG must appear exactly once in src/App.tsx (ROOT_TAG is ${ROOT_TAG})`);
  }
  const end = at + ROOT_TAG.length;
  return `${original.slice(0, end)} data-buildtime="${n}"${original.slice(end)}`;
}

/** The `--runs N` of `argv`, 5 when absent. Throws on anything but a whole number of 1 or more. */
export function parseRuns(argv) {
  const at = argv.indexOf("--runs");
  if (at === -1) {
    return 5;
  }
  const value = argv[at + 1];
  const runs = /^\d+$/.test(value ?? "") ? Number(value) : Number.NaN;
  if (!Number.isSafeInteger(runs) || runs < 1) {
    throw new Error(`--runs takes a whole number of 1 or more, got ${value}`);
  }
  return runs;
}

/** Characters that make a package.json script more than a plain command and its arguments. */
const SHELL_SYNTAX = /[&|;<>()$`'"\\*?{}[\]~#]/;

/**
 * The words of the package.json script `text`, split on spaces. Throws on shell syntax (`&&`,
 * `|`, quotes, `$`, globs, `VAR=value`): the script runs it without a shell, so it must be one
 * plain command.
 */
export function splitCommand(name, text) {
  if (typeof text !== "string" || text.trim() === "") {
    throw new Error(`ui/app/package.json has no script "${name}"`);
  }
  if (SHELL_SYNTAX.test(text) || text.trim().split(/ +/)[0].includes("=")) {
    throw new Error(
      `ui/app/package.json script "${name}" is \`${text}\`; buildtime runs scripts without a shell, so keep it one plain command`,
    );
  }
  return text.trim().split(/ +/);
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

/**
 * The JS entry of the bin `name`: the first direct dependency, in `dirs` order, whose
 * package.json `bin` provides it, read from `<dir>/node_modules/<dependency>`. A string `bin`
 * provides the package's own name without its scope. Throws when no dependency provides it.
 */
export function resolveBin(name, dirs) {
  for (const dir of dirs) {
    const manifest = readJson(join(dir, "package.json"));
    const deps = Object.keys({ ...manifest.dependencies, ...manifest.devDependencies });
    for (const dep of deps) {
      const pkgDir = join(dir, "node_modules", dep);
      if (!existsSync(join(pkgDir, "package.json"))) {
        continue;
      }
      const pkg = readJson(join(pkgDir, "package.json"));
      const own = String(pkg.name ?? dep).replace(/^@[^/]+\//, "");
      const entry =
        typeof pkg.bin === "string" ? (own === name ? pkg.bin : undefined) : pkg.bin?.[name];
      if (typeof entry === "string") {
        return join(pkgDir, entry);
      }
    }
  }
  throw new Error(
    `no dependency in ${dirs.map((dir) => join(dir, "package.json")).join(" or ")} provides the bin "${name}"; run \`pnpm -C ui install --frozen-lockfile\``,
  );
}

/**
 * The package.json script `script` of the app in `appDir`, ready to run without a shell or pnpm:
 * `argv` is the bin's JS entry and the script's arguments, for `process.execPath`. Bins are looked
 * up in the app, then in the ui workspace root (`rootDir`).
 */
export function scriptCommand(appDir, rootDir, script) {
  const scripts = readJson(join(appDir, "package.json")).scripts ?? {};
  const [bin, ...args] = splitCommand(script, scripts[script]);
  return { argv: [resolveBin(bin, [appDir, rootDir]), ...args], text: scripts[script] };
}

/**
 * The initial JS of the build in `dist`: the `<script type="module" src>` entry plus every
 * `<link rel="modulepreload" href>` of `dist/index.html`, each brotli-compressed at quality 11.
 * Returns the total in bytes and the files, in the order index.html names them. xtask reports it
 * in KB of 1000 bytes, as Vite's own `kB` output does. Throws when there is no module script.
 */
export function initialJs(dist) {
  const html = readFileSync(join(dist, "index.html"), "utf8");
  const files = [];
  let entries = 0;
  for (const tag of html.match(/<script\b[^>]*>|<link\b[^>]*>/g) ?? []) {
    const isEntry = tag.startsWith("<script") && /\btype="module"/.test(tag);
    const isPreload = tag.startsWith("<link") && /\brel="modulepreload"/.test(tag);
    const url = /\b(?:src|href)="([^"]+)"/.exec(tag)?.[1];
    if ((isEntry || isPreload) && url !== undefined) {
      files.push(url.replace(/^\//, ""));
      entries += isEntry ? 1 : 0;
    }
  }
  if (entries === 0) {
    throw new Error("dist/index.html has no module script");
  }
  let bytes = 0;
  for (const file of files) {
    const compressed = brotliCompressSync(readFileSync(join(dist, file)), {
      params: { [zlib.BROTLI_PARAM_QUALITY]: 11 },
    });
    bytes += compressed.length;
  }
  return { bytes, files };
}

// The guard over src/App.tsx. It follows xtask's guard over a marker file (probe.rs, marker.rs):
// - Before the first edit, the original bytes are copied to `target/xtask/buildtime/<repo path>`.
//   An existing copy is never replaced.
// - Every write, to App.tsx or to the copy, goes to a temporary file in the same folder, then is
//   renamed over the target, so a kill never leaves half a file.
// - The original goes back only while App.tsx holds exactly the bytes last written. Anything else
//   is the developer's edit: nothing more is written, both files stay, and a ConflictError names
//   them.
// - A killed run leaves its copy. The next run's `recoverApp` puts it back only when App.tsx is
//   the copy plus one `data-buildtime="<n>"` at ROOT_TAG. xtask's own leftover restore leaves
//   `target/xtask/buildtime/ui/` to this script.

/** Where the copies wait, under the repo root. xtask's buildtime lock covers it. */
export const BACKUP_DIR = "target/xtask/buildtime";

/** A conflict over App.tsx: both files are kept, and `fix` says what to do with them. */
export class ConflictError extends Error {
  constructor(message, fix) {
    super(message);
    this.name = "ConflictError";
    this.fix = fix;
  }
}

/**
 * The run failure for an error from `AppGuard.restore`: a ConflictError as it is; any other error
 * (EPERM from the rename while a scanner holds the file, say) named with the copy, which stays
 * for the next run's `recoverApp`.
 */
export function restoreFailure(paths, error) {
  if (error instanceof ConflictError) {
    return { message: error.message, fix: error.fix };
  }
  return {
    message: `could not put back ${paths.rel}: ${error?.message ?? String(error)}; its saved copy is ${paths.backupRel}`,
    fix: `the next \`cargo xtask buildtime web\` restores ${paths.rel} from ${paths.backupRel}; or copy it back by hand`,
  };
}

/** The file at repo path `rel` under `root`, its copy, and the folder copies live in. */
export function appPaths(root, rel) {
  return {
    rel,
    app: join(root, rel),
    base: join(root, BACKUP_DIR),
    backup: join(root, BACKUP_DIR, rel),
    backupRel: `${BACKUP_DIR}/${rel}`,
  };
}

/** The temporary file a write to `path` goes through, in the same folder. */
export function tmpOf(path) {
  return join(dirname(path), `.${basename(path)}.buildtime.tmp`);
}

/** Writes `bytes` to a temporary file next to `path`, syncs it, then renames it over `path`. */
export function writeThrough(path, bytes) {
  const tmp = tmpOf(path);
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

const MARK_AT_ROOT = new RegExp(
  `${ROOT_TAG.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")} data-buildtime="\\d+"`,
  "g",
);

/**
 * `text` with the one `data-buildtime="<n>"` that `marked` adds at ROOT_TAG taken out, or
 * `undefined` unless there is exactly one.
 */
export function unmarked(text) {
  const found = text.match(MARK_AT_ROOT) ?? [];
  return found.length === 1 ? text.replace(MARK_AT_ROOT, ROOT_TAG) : undefined;
}

/**
 * What an interrupted run's copy means for App.tsx as it is now (`current`, `undefined` when the
 * file is gone): `clean` when they are equal, `restore` when App.tsx is the copy plus one mark at
 * ROOT_TAG, `missing`, or `conflict` for any other difference.
 */
export function leftover(current, copy) {
  if (current === undefined) {
    return "missing";
  }
  if (current.equals(copy)) {
    return "clean";
  }
  return unmarked(current.toString("utf8")) === copy.toString("utf8") ? "restore" : "conflict";
}

/**
 * Settles what an interrupted run left, before anything is written: deletes the temporary files
 * of an interrupted write, then puts the copy back when `leftover` allows it. Returns the line to
 * print, or `undefined` when there was no copy. Throws a ConflictError, keeping both files, when
 * App.tsx changed in any other way or is gone.
 */
export function recoverApp(paths) {
  for (const tmp of [tmpOf(paths.app), tmpOf(paths.backup)]) {
    if (existsSync(tmp)) {
      unlinkSync(tmp);
    }
  }
  const copy = readOrUndefined(paths.backup);
  if (copy === undefined) {
    return undefined;
  }
  switch (leftover(readOrUndefined(paths.app), copy)) {
    case "clean":
      dropCopy(paths);
      return `deleted the copy of ${paths.rel} an interrupted buildtime run left; the file was unchanged`;
    case "restore":
      writeThrough(paths.app, copy);
      dropCopy(paths);
      return `restored ${paths.rel} from an interrupted buildtime run`;
    case "missing":
      throw conflict(paths, "is missing after an interrupted buildtime run");
    default:
      throw conflict(paths, "changed after an interrupted buildtime run");
  }
}

/** The guard over App.tsx for one run: its original, its copy, and the bytes last written. */
export class AppGuard {
  /** Saves the original of App.tsx as its copy. Nothing is edited when the save fails. */
  constructor(paths) {
    if (existsSync(paths.backup)) {
      throw new ConflictError(
        `a saved copy of ${paths.rel} already exists at ${paths.backupRel}`,
        `compare ${paths.rel} with ${paths.backupRel}, keep what you want, then delete ${paths.backupRel}`,
      );
    }
    this.paths = paths;
    this.original = readFileSync(paths.app);
    this.last = this.original;
    mkdirSync(dirname(paths.backup), { recursive: true });
    writeThrough(paths.backup, this.original);
    this.pending = true;
  }

  /** Throws a ConflictError, and stops writing, unless App.tsx holds the bytes last written. */
  check() {
    const current = readOrUndefined(this.paths.app);
    if (current !== undefined && current.equals(this.last)) {
      return;
    }
    this.pending = false;
    throw conflict(
      this.paths,
      current === undefined
        ? "went missing during this buildtime run"
        : "changed during this buildtime run",
    );
  }

  /** Writes `text` to App.tsx, once it still holds the bytes last written. */
  write(text) {
    if (!this.pending) {
      throw new Error(`${this.paths.rel} is no longer guarded`);
    }
    this.check();
    const bytes = Buffer.from(text);
    writeThrough(this.paths.app, bytes);
    this.last = bytes;
  }

  /**
   * Puts the original back and deletes the copy, once App.tsx still holds the bytes last written.
   * Safe to repeat. After a conflict, both files stay as they are.
   */
  restore() {
    if (!this.pending) {
      return;
    }
    this.check();
    this.pending = false;
    if (!this.last.equals(this.original)) {
      writeThrough(this.paths.app, this.original);
    }
    dropCopy(this.paths);
  }
}
