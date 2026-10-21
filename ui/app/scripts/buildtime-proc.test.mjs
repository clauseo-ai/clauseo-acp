import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { afterAll, describe, expect, it } from "vitest";
import { DRAIN_MS, KILL_GRACE_MS, groupAlive, runTool, start, stop } from "./buildtime-proc.mjs";

const dir = mkdtempSync(join(tmpdir(), "buildtime-proc-"));

afterAll(() => {
  rmSync(dir, { recursive: true, force: true });
});

const LEADER = join(import.meta.dirname, "buildtime-leader.mjs");

/**
 * tsc's pattern: a node wrapper that runs the real binary with inherited stdio. The binary is a
 * node process that prints `ready` and waits 30 s. Every process of the tool, and its leader, has
 * the scratch folder in its argv, so `leftovers()` finds them without knowing their pids.
 */
function wrapper() {
  const tool = join(dir, "wrapper.mjs");
  const binary = 'console.log("ready"); setTimeout(() => {}, 30_000);';
  writeFileSync(
    tool,
    [
      'import { execFileSync } from "node:child_process";',
      `execFileSync(process.execPath, ["-e", ${JSON.stringify(binary)}, ${JSON.stringify(dir)}], {`,
      '  stdio: "inherit",',
      "});",
      "",
    ].join("\n"),
  );
  return tool;
}

/** Whether any process with the scratch folder in its argv (the tool, its binary) is left. */
function leftovers() {
  return spawnSync("pgrep", ["-f", dir]).status === 0;
}

/** Resolves once `stream` has printed a line `ready`. */
function ready(stream) {
  return new Promise((done) => {
    let out = "";
    stream.on("data", (chunk) => {
      out += chunk;
      if (out.split("\n").includes("ready")) {
        done();
      }
    });
  });
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
  // The timeout can fire before the leader has started the tool (a loaded machine starts node
  // slowly), so `toolPid` may be unknown; either way nothing of the tool may be left.
  it("leaves nothing of a tool whose own child holds the pipes at the timeout", async () => {
    const started = performance.now();
    const result = await runTool([wrapper()], { cwd: dir, timeoutMs: 300 });
    const ms = performance.now() - started;
    expect(result.timedOut).toBe(true);
    expect(ms).toBeLessThan(300 + KILL_GRACE_MS + DRAIN_MS);
    expect(groupAlive(result.pid)).toBe(false);
    if (result.toolPid !== undefined) {
      expect(groupAlive(result.toolPid)).toBe(false);
    }
    expect(await until(() => !leftovers(), 3_000)).toBe(true);
  }, 15_000);

  it("stops a started tool's group, and the child of it that holds the pipes", async () => {
    const child = start([wrapper()], { cwd: dir });
    const reported = new Promise((done) => child.once("message", done));
    await Promise.all([ready(child.stdout), reported]);
    expect(groupAlive(child.toolPid)).toBe(true);
    expect(await stop(child)).toBe(true);
    expect(groupAlive(child.toolPid)).toBe(false);
    expect(groupAlive(child.pid)).toBe(false);
    expect(await until(() => !leftovers(), 3_000)).toBe(true);
  }, 15_000);

  // What `stop` sends while the leader has not reported the tool's pid: SIGTERM to the leader's
  // group alone. The leader passes it on, and the tool and its child go with it.
  it("stops the tool through its leader when the stop comes before the pid", async () => {
    const leader = spawn(process.execPath, [LEADER, wrapper()], {
      cwd: dir,
      stdio: ["ignore", "pipe", "inherit", "ipc"],
      detached: true,
    });
    const exited = new Promise((done) => leader.once("exit", (code) => done(code)));
    await ready(leader.stdout);
    process.kill(-leader.pid, "SIGTERM");
    expect(await exited).toBe(128 + 15);
    expect(await until(() => !leftovers(), 3_000)).toBe(true);
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
