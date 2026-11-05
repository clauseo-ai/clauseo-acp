// Word holding a document open, for the lock specs (word-locked.spec.ts, versions-locked.spec.ts).
// Since #409 an owner file `~$…` alone is stale on Windows: it counts only while WINWORD.EXE runs
// or while the document cannot be opened with no sharing (crates/matters/src/locks.rs,
// `held_open`). So a hold does what Word does: a background PowerShell keeps the document open
// with no sharing, and the owner file sits beside it. Releasing ends that process and removes the
// owner file. The fixture's `wordLock` releases every hold still on in teardown, so a failed spec
// leaves no locked file and no stray process behind.
//
// While a hold is on, the document cannot be read either (no sharing), so a spec reads the folder
// only once it has released the hold.
//
// The script and the process handling run on any OS (word-hold.test.ts, with node standing in for
// PowerShell); holding a real document needs Windows.

import { spawn, type ChildProcess } from "node:child_process";
import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { createInterface } from "node:readline";

/** The line the holder prints once the document is open. */
export const HELD_LINE = "held";
/** How long the holder may take to open the document and say so. */
const HELD_TIMEOUT_MS = 30_000;
/** How long the holder may take to exit once killed. */
const EXIT_TIMEOUT_MS = 30_000;
/**
 * How long the holder keeps the document if nobody ends it: past any one spec, so only a lost
 * worker leaves it, and then not for long.
 */
const HOLD_SECONDS = 600;

/** `text` as a PowerShell single-quoted string: each `'` doubled, nothing else expands in it. */
export function psQuote(text: string): string {
  return `'${text.replaceAll("'", "''")}'`;
}

/**
 * The PowerShell that opens `path` for read and write with no sharing, prints {@link HELD_LINE},
 * and keeps it open for `seconds`. A failed open stops it before the line, with an error.
 */
export function holdScript(path: string, seconds = HOLD_SECONDS): string {
  return `$ErrorActionPreference = 'Stop'
$f = [IO.File]::Open(${psQuote(path)}, 'Open', 'ReadWrite', 'None')
[Console]::Out.WriteLine('${HELD_LINE}')
[Console]::Out.Flush()
Start-Sleep -Seconds ${seconds}
`;
}

/** A process that holds something until it is ended. */
export interface Holder {
  /** Ends the process and waits for it to exit. Safe to call more than once. */
  end(): Promise<void>;
}

/** Whether `child` exits within `ms`. */
async function exited(child: ChildProcess, ms: number): Promise<boolean> {
  if (child.exitCode !== null || child.signalCode !== null) return true;
  return await new Promise<boolean>((resolve) => {
    const timer = setTimeout(() => {
      child.off("exit", onExit);
      resolve(false);
    }, ms);
    const onExit = (): void => {
      clearTimeout(timer);
      resolve(true);
    };
    child.once("exit", onExit);
  });
}

/**
 * Starts `command` and resolves once it prints {@link HELD_LINE}. Rejects, with the process
 * ended, if it fails to start, exits first or says nothing within `timeoutMs`; the rejection
 * carries what it wrote to stderr.
 */
export async function startHolder(
  command: string,
  args: readonly string[],
  timeoutMs = HELD_TIMEOUT_MS,
): Promise<Holder> {
  const child = spawn(command, args, { stdio: ["ignore", "pipe", "pipe"], windowsHide: true });
  let stderr = "";
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk: string) => {
    if (stderr.length < 4_000) stderr += chunk;
  });
  const end = async (): Promise<void> => {
    if (child.pid === undefined || (await exited(child, 0))) return;
    child.kill();
    if (!(await exited(child, EXIT_TIMEOUT_MS))) {
      throw new Error(`the holder (PID ${String(child.pid)}) did not exit once killed`);
    }
  };
  const lines = createInterface({ input: child.stdout });
  try {
    await new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => {
        reject(new Error(`the holder did not print "${HELD_LINE}" within ${timeoutMs} ms`));
      }, timeoutMs);
      const settle = (error?: Error): void => {
        clearTimeout(timer);
        lines.off("line", onLine);
        child.off("exit", onExit);
        if (error === undefined) resolve();
        else reject(error);
      };
      const onLine = (line: string): void => {
        if (line.trim() === HELD_LINE) settle();
      };
      const onExit = (code: number | null): void => {
        settle(new Error(`the holder exited with ${String(code)} before it held the file`));
      };
      // The handler stays: a ChildProcess "error" with no handler would end the worker.
      child.on("error", (error) => {
        settle(new Error(`the holder did not start: ${command}`, { cause: error }));
      });
      lines.on("line", onLine);
      child.once("exit", onExit);
    });
  } catch (error) {
    await end().catch(() => {});
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(stderr === "" ? message : `${message}; stderr: ${stderr.trim()}`, {
      cause: error,
    });
  }
  // The pipes are drained from here on, so the holder never blocks writing to them.
  child.stdout.resume();
  return { end };
}

/** Word holding `document` open, as a spec sees it. */
export interface WordHold {
  /** Word closes the document: the holder ends, then the owner file goes. Safe to call twice. */
  release(): Promise<void>;
}

/** What Word holds: the document, and the owner file it writes beside it. */
export interface WordHoldOptions {
  /** The folder both files sit in. */
  folder: string;
  /** The document's name (`brief.txt`). */
  document: string;
  /** The owner file's name (`~$brief.txt`; locks.rs `owner_names`). */
  owner: string;
  /** The owner file's content. */
  ownerBytes: string;
}

/**
 * Word opens `document`: a PowerShell holds it with no sharing (waited on until it says so), and
 * the owner file appears beside it. If writing the owner file fails, the holder is ended.
 */
export async function holdWord(options: WordHoldOptions): Promise<WordHold> {
  const holder = await startHolder("powershell", [
    "-NoProfile",
    "-NonInteractive",
    "-EncodedCommand",
    Buffer.from(holdScript(join(options.folder, options.document)), "utf16le").toString("base64"),
  ]);
  const ownerPath = join(options.folder, options.owner);
  try {
    writeFileSync(ownerPath, options.ownerBytes);
  } catch (error) {
    await holder.end().catch(() => {});
    throw error;
  }
  return {
    release: async () => {
      await holder.end();
      rmSync(ownerPath, { force: true });
    },
  };
}

/** Word holding documents in one folder, for a spec (the fixture's `wordLock`). */
export interface WordLock {
  /**
   * Word opens `document`: it is held open with no sharing and `owner` appears beside it.
   * Resolves once the document is held.
   */
  hold(options: Omit<WordHoldOptions, "folder">): Promise<WordHold>;
}

/**
 * Hands `use` a {@link WordLock} on `folder`, then releases every hold still on, even when `use`
 * failed; releasing one the spec released already is safe. Throws what failed to release.
 * `hold` is {@link holdWord} but in word-hold.test.ts.
 */
export async function withWordLock(
  folder: string,
  use: (lock: WordLock) => Promise<void>,
  hold: (options: WordHoldOptions) => Promise<WordHold> = holdWord,
): Promise<void> {
  const holds: WordHold[] = [];
  try {
    await use({
      hold: async (options) => {
        const held = await hold({ ...options, folder });
        holds.push(held);
        return held;
      },
    });
  } finally {
    const errors: unknown[] = [];
    for (const hold of holds) {
      await hold.release().catch((error: unknown) => errors.push(error));
    }
    if (errors.length > 0) {
      throw new AggregateError(errors, "releasing Word's holds failed");
    }
  }
}
