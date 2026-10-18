import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, describe, expect, it } from "vitest";
import { DRAIN_MS, KILL_GRACE_MS, groupAlive, runTool } from "./buildtime-proc.mjs";

const dir = mkdtempSync(join(tmpdir(), "buildtime-proc-"));

afterAll(() => {
  rmSync(dir, { recursive: true, force: true });
});

describe.skipIf(process.platform === "win32")("runTool", () => {
  it("stops a tool whose own child holds the pipes, and that child, at the timeout", async () => {
    // tsc's pattern: a node wrapper that runs the real binary with inherited stdio.
    const tool = join(dir, "wrapper.mjs");
    writeFileSync(
      tool,
      'import { execFileSync } from "node:child_process";\nexecFileSync("sleep", ["30"], { stdio: "inherit" });\n',
    );
    const started = performance.now();
    const result = await runTool([tool], { cwd: dir, timeoutMs: 300 });
    const ms = performance.now() - started;
    expect(result.timedOut).toBe(true);
    expect(ms).toBeLessThan(300 + KILL_GRACE_MS + DRAIN_MS);
    expect(groupAlive(result.pid)).toBe(false);
  }, 15_000);

  it("returns the code and output of a tool that exits", async () => {
    const tool = join(dir, "ok.mjs");
    writeFileSync(tool, 'console.log("hi"); process.exitCode = 3;\n');
    const result = await runTool([tool], { cwd: dir, timeoutMs: 10_000 });
    expect(result).toMatchObject({ code: 3, output: "hi\n", timedOut: false });
  });
});
