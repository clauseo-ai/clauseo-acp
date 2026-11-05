// The hold script and the holder's lifetime, on any OS: node stands in for PowerShell. Holding a
// real document with no sharing needs Windows (word-locked.spec.ts, versions-locked.spec.ts).

import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import {
  HELD_LINE,
  holdScript,
  holdWord,
  psQuote,
  startHolder,
  withWordLock,
  type WordHoldOptions,
} from "./word-hold.ts";

/** A node holder that writes its PID to `pidFile`, then runs `then`. */
function node(pidFile: string, then: string): [string, string[]] {
  const script = `require("node:fs").writeFileSync(${JSON.stringify(pidFile)}, String(process.pid)); ${then}`;
  return [process.execPath, ["-e", script]];
}

/** Whether the process `pid` is still running. */
function running(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

let dir: string;
test.beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), "clauseo-word-hold-"));
});
test.afterEach(() => {
  rmSync(dir, { recursive: true, force: true });
});

test("psQuote doubles single quotes, so a path cannot end the string", () => {
  expect(psQuote(String.raw`C:\a\brief.txt`)).toBe(String.raw`'C:\a\brief.txt'`);
  expect(psQuote("O'Brien's $x `n.docx")).toBe("'O''Brien''s $x `n.docx'");
});

test("the script opens the document with no sharing, then says so, then waits", () => {
  const script = holdScript(String.raw`C:\m\O'Neil.docx`, 5);
  expect(script).toContain("$ErrorActionPreference = 'Stop'");
  expect(script).toContain(
    String.raw`[IO.File]::Open('C:\m\O''Neil.docx', 'Open', 'ReadWrite', 'None')`,
  );
  expect(script).toContain(`WriteLine('${HELD_LINE}')`);
  expect(script).toContain("Start-Sleep -Seconds 5");
  // The line comes only after the open.
  expect(script.indexOf("[IO.File]::Open")).toBeLessThan(script.indexOf(HELD_LINE));
});

test("startHolder waits for the held line, and end kills the holder", async () => {
  const pidFile = join(dir, "pid");
  const [command, args] = node(
    pidFile,
    `setTimeout(() => console.log("${HELD_LINE}"), 200); setInterval(() => {}, 1000);`,
  );
  const holder = await startHolder(command, args, 10_000);
  const pid = Number(readFileSync(pidFile, "utf8"));
  expect(running(pid)).toBe(true);
  await holder.end();
  expect(running(pid)).toBe(false);
  // A second end, as the fixture's teardown after a spec's own release, is fine.
  await holder.end();
});

test("startHolder rejects with stderr when the holder exits before it holds the file", async () => {
  const [command, args] = node(
    join(dir, "pid"),
    `console.error("sharing violation"); process.exit(3);`,
  );
  await expect(startHolder(command, args, 10_000)).rejects.toThrow(
    /exited with 3 before it held the file; stderr: sharing violation/,
  );
});

test("startHolder kills a holder that never says it holds the file", async () => {
  const pidFile = join(dir, "pid");
  const [command, args] = node(pidFile, `console.log("not yet"); setInterval(() => {}, 1000);`);
  await expect(startHolder(command, args, 500)).rejects.toThrow(/did not print "held" within/);
  expect(running(Number(readFileSync(pidFile, "utf8")))).toBe(false);
});

test("startHolder rejects when the holder cannot start", async () => {
  await expect(startHolder(join(dir, "no-such-program"), [], 10_000)).rejects.toThrow(
    /did not start/,
  );
});

test("holdWord writes no owner file when the document cannot be held", async () => {
  // On Windows PowerShell runs, and the open fails: the document does not exist. Elsewhere
  // `powershell` is missing or fails the same way. Either way it rejects and leaves nothing.
  await expect(
    holdWord({ folder: dir, document: "missing.docx", owner: "~$ssing.docx", ownerBytes: "x" }),
  ).rejects.toThrow();
  expect(() => readFileSync(join(dir, "~$ssing.docx"))).toThrow();
});

test("withWordLock releases every hold after a spec fails, and one it released once more", async () => {
  const released: string[] = [];
  const asked: WordHoldOptions[] = [];
  const fake = async (options: WordHoldOptions) => {
    asked.push(options);
    return {
      release: async () => {
        released.push(options.document);
      },
    };
  };
  await expect(
    withWordLock(
      dir,
      async (lock) => {
        const first = await lock.hold({ document: "a.docx", owner: "~$a.docx", ownerBytes: "" });
        await lock.hold({ document: "b.docx", owner: "~$b.docx", ownerBytes: "" });
        await first.release();
        throw new Error("the spec failed");
      },
      fake,
    ),
  ).rejects.toThrow("the spec failed");
  expect(asked.map((options) => options.folder)).toEqual([dir, dir]);
  expect(released).toEqual(["a.docx", "a.docx", "b.docx"]);
});
