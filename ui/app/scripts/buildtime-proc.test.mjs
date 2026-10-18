import { spawn } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { afterAll, describe, expect, it } from "vitest";
import { DRAIN_MS, KILL_GRACE_MS, groupAlive, runTool } from "./buildtime-proc.mjs";

const dir = mkdtempSync(join(tmpdir(), "buildtime-proc-"));

afterAll(() => {
  rmSync(dir, { recursive: true, force: true });
});

/** tsc's pattern: a node wrapper that runs the real binary (`sleep 30`) with inherited stdio. */
function wrapper() {
  const tool = join(dir, "wrapper.mjs");
  writeFileSync(
    tool,
    [
      'import { execFileSync } from "node:child_process";',
      'execFileSync("sleep", ["30"], { stdio: "inherit" });',
      "",
    ].join("\n"),
  );
  return tool;
}

/** Resolves once `test()` is true, or with false after `ms`. */
async function until(test, ms) {
  const end = performance.now() + ms;
  while (performance.now() < end) {
    if (test()) {
      return true;
    }
    await new Promise((done) => setTimeout(done, 20));
  }
  return test();
}

// POSIX only: process groups and the leader exist only there. On Windows, buildtime-proc.mjs
// stops a tree with taskkill, which these tests do not cover.
describe.skipIf(process.platform === "win32")("runTool", () => {
  it("stops a tool whose own child holds the pipes, and that child, at the timeout", async () => {
    const started = performance.now();
    const result = await runTool([wrapper()], { cwd: dir, timeoutMs: 300 });
    const ms = performance.now() - started;
    expect(result.timedOut).toBe(true);
    expect(ms).toBeLessThan(300 + KILL_GRACE_MS + DRAIN_MS);
    expect(result.toolPid).toBeTypeOf("number");
    expect(groupAlive(result.toolPid)).toBe(false);
    expect(groupAlive(result.pid)).toBe(false);
  }, 15_000);

  it("returns the code and output of a tool that exits", async () => {
    const tool = join(dir, "ok.mjs");
    writeFileSync(tool, 'console.log("hi"); process.exitCode = 3;\n');
    const result = await runTool([tool], { cwd: dir, timeoutMs: 10_000 });
    expect(result).toMatchObject({ code: 3, output: "hi\n", timedOut: false });
  });

  it("kills the tool's group through its leader when the parent gets SIGKILL", async () => {
    // A parent that starts the wrapper through buildtime-proc.mjs, prints the tool's pid and
    // waits; SIGKILL runs none of its handlers, so only the leader can stop the tool.
    const parent = join(dir, "parent.mjs");
    const proc = pathToFileURL(join(import.meta.dirname, "buildtime-proc.mjs")).href;
    writeFileSync(
      parent,
      [
        `import { start } from ${JSON.stringify(proc)};`,
        `const child = start([${JSON.stringify(wrapper())}], { cwd: ${JSON.stringify(dir)} });`,
        'child.on("message", () => console.log(JSON.stringify({ toolPid: child.toolPid })));',
        "",
      ].join("\n"),
    );
    const runner = spawn(process.execPath, [parent], { stdio: ["ignore", "pipe", "inherit"] });
    const toolPid = await new Promise((done, fail) => {
      let out = "";
      runner.stdout.on("data", (chunk) => {
        out += chunk;
        const line = out.split("\n").find((text) => text.startsWith("{"));
        if (line !== undefined && out.includes("\n")) {
          done(JSON.parse(line).toolPid);
        }
      });
      runner.on("exit", (code) => fail(new Error(`the parent exited with ${code}`)));
    });
    expect(groupAlive(toolPid)).toBe(true);
    runner.kill("SIGKILL");
    const gone = await until(() => !groupAlive(toolPid), 3_000);
    if (!gone) {
      process.kill(-toolPid, "SIGKILL");
    }
    expect(gone).toBe(true);
  }, 15_000);
});
