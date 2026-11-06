// The scratch folders on any OS: no app, no Windows.

import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import {
  agentSettings,
  fakeAgentPath,
  launchProfile,
  makeScratch,
  portFile,
  profileLock,
  readTree,
  readyReusedProfile,
  removeScratch,
  retryWarmUp,
  scratchEnv,
  scratchSettings,
  warmProfile,
  webviewFolder,
} from "./scratch.ts";

const PLANS = { "claude-acp": { provider: "anthropic" }, "codex-acp": { provider: "anthropic" } };

let base: string;
let agent: string;

test.beforeEach(() => {
  base = mkdtempSync(join(tmpdir(), "clauseo-scratch-test-"));
  agent = join(base, "fake_agent.exe");
  writeFileSync(agent, "");
});

test.afterEach(() => {
  rmSync(base, { recursive: true, force: true });
});

test("settings start fake_agent on the scenario as agents.claude-acp", () => {
  expect(agentSettings(agent, "prompt-stream")).toEqual({
    agents: { "claude-acp": { command: agent, args: ["--scenario", "prompt-stream"] } },
  });
  // The scoreboard's pace goes after the scenario.
  expect(agentSettings(agent, "scoreboard", ["--pace-ms", "5"])).toEqual({
    agents: {
      "claude-acp": { command: agent, args: ["--scenario", "scoreboard", "--pace-ms", "5"] },
    },
  });
});

test("other agents each start fake_agent on their own scenario, beside agents.claude-acp", () => {
  expect(
    agentSettings(agent, "prompt-stream", ["--pace-ms", "5"], { "codex-acp": "codex-handshake" }),
  ).toEqual({
    agents: {
      "claude-acp": { command: agent, args: ["--scenario", "prompt-stream", "--pace-ms", "5"] },
      "codex-acp": { command: agent, args: ["--scenario", "codex-handshake"] },
    },
  });
  // No others: the same section as before.
  expect(agentSettings(agent, "prompt-stream", [], {})).toEqual(
    agentSettings(agent, "prompt-stream"),
  );
});

test("an other agent may not be claude-acp, the test's own agent", () => {
  expect(() =>
    agentSettings(agent, "prompt-stream", [], { "claude-acp": "codex-handshake" }),
  ).toThrow(/claude-acp plays the test's scenario/);
});

test("settings refuse a relative command", () => {
  expect(() => agentSettings("fake_agent.exe", "prompt-stream")).toThrow(/absolute/);
});

test("fake_agent comes from E2E_FAKE_AGENT, else beside the app", () => {
  const app = join(base, "release", "clauseo-desktop.exe");
  expect(fakeAgentPath({ E2E_FAKE_AGENT: agent }, app, true)).toBe(agent);
  expect(fakeAgentPath({ E2E_FAKE_AGENT: "" }, app, true)).toBe(
    join(base, "release", "fake_agent.exe"),
  );
  expect(fakeAgentPath({}, app, false)).toBe(join(base, "release", "fake_agent"));
});

test("the folders: data with settings, a seeded matter beside it, and the launch variables", async () => {
  const scratch = makeScratch(
    {
      scenario: "files-one-turn",
      matterFiles: { "brief.txt": "Clause 4 draft\n", "notes/b.txt": "B\n" },
    },
    () => agent,
    base,
  );
  const settings: unknown = JSON.parse(
    readFileSync(join(scratch.dataDir, "settings.json"), "utf8"),
  );
  expect(settings).toEqual({
    matters: { default_location: scratch.mattersDir },
    agents: { "claude-acp": { command: agent, args: ["--scenario", "files-one-turn"] } },
    plans: PLANS,
  });
  expect(scratch.mattersDir.startsWith(scratch.root)).toBe(true);
  expect(existsSync(scratch.mattersDir)).toBe(true);
  expect(readTree(scratch.mattersDir)).toEqual({});
  expect(readTree(scratch.matterDir)).toEqual({
    "brief.txt": "Clause 4 draft\n",
    "notes/b.txt": "B\n",
  });
  expect(scratch.matterDir.startsWith(scratch.dataDir)).toBe(false);
  expect(scratchEnv(scratch)).toEqual({
    CLAUSEO_DATA_DIR: scratch.dataDir,
    E2E_MATTER_DIR: scratch.matterDir,
    E2E_CONFIRM_ALWAYS: "accept",
    E2E_AGENTS_MANAGED: "claude-acp,codex-acp",
  });
  const first = webviewFolder(scratch, 1);
  expect(webviewFolder(scratch, 2)).not.toBe(first);
  await removeScratch(scratch);
  expect(existsSync(scratch.root)).toBe(false);
});

test("the settings file holds the other agents, each on its scenario", async () => {
  const scratch = makeScratch(
    { scenario: "prompt-stream", matterFiles: {}, otherAgents: { "codex-acp": "codex-handshake" } },
    () => agent,
    base,
  );
  const settings: unknown = JSON.parse(
    readFileSync(join(scratch.dataDir, "settings.json"), "utf8"),
  );
  expect(settings).toEqual({
    matters: { default_location: scratch.mattersDir },
    agents: {
      "claude-acp": { command: agent, args: ["--scenario", "prompt-stream"] },
      "codex-acp": { command: agent, args: ["--scenario", "codex-handshake"] },
    },
    plans: PLANS,
  });
  await removeScratch(scratch);
});

test("no scenario: settings hold the matters' location and the plans, and fake_agent is never asked for", () => {
  const scratch = makeScratch(
    { scenario: null, matterFiles: {} },
    () => {
      throw new Error("asked");
    },
    base,
  );
  const settings: unknown = JSON.parse(
    readFileSync(join(scratch.dataDir, "settings.json"), "utf8"),
  );
  expect(settings).toEqual({ matters: { default_location: scratch.mattersDir }, plans: PLANS });
  expect(scratch.agentCommand).toBeNull();
});

test("scratchSettings refuses a relative matters location", () => {
  expect(() => scratchSettings("matters", null)).toThrow(/absolute/);
});

test("a missing fake_agent fails and leaves no folder", () => {
  expect(() =>
    makeScratch(
      { scenario: "prompt-stream", matterFiles: {} },
      () => join(base, "missing.exe"),
      base,
    ),
  ).toThrow(/fake_agent is not at/);
  expect(readTree(base)).toEqual({ "fake_agent.exe": "" });
});

test("matter files refuse absolute and .. paths", () => {
  for (const path of ["../x.txt", "/x.txt", "a//b.txt", ""]) {
    expect(() =>
      makeScratch({ scenario: null, matterFiles: { [path]: "x" } }, () => agent, base),
    ).toThrow(/plain relative path/);
  }
});

test("readTree leaves out .clauseo and empty folders", () => {
  const folder = join(base, "m");
  mkdirSync(join(folder, ".clauseo"), { recursive: true });
  mkdirSync(join(folder, "notes"), { recursive: true });
  writeFileSync(join(folder, ".clauseo", "matter.json"), "{}");
  writeFileSync(join(folder, "brief.txt"), "x");
  expect(readTree(folder)).toEqual({ "brief.txt": "x" });
});

test("a fresh profile is a new folder per launch; a warm one is the worker's folder, counted", async () => {
  const one = makeScratch({ scenario: null, matterFiles: {} }, () => agent, base);
  const two = makeScratch({ scenario: null, matterFiles: {} }, () => agent, base);
  const warm = warmProfile(base);

  // Fresh: each launch its own new folder under its test's root, always launch 1 on it.
  const a = launchProfile("fresh", one, 1, warm);
  const b = launchProfile("fresh", one, 2, warm);
  expect(a).toEqual({ profile: "fresh", folder: join(one.root, "webview-1"), launch: 1 });
  expect(b.folder).not.toBe(a.folder);
  expect(b.launch).toBe(1);
  expect(warm.launches).toBe(0);

  // Warm: the warm-up makes it (launch 1), the measured plays reuse it, across tests' scratch.
  const plays = [one, two, two, two].map((scratch, i) =>
    launchProfile("warm", scratch, i + 1, warm),
  );
  expect(plays.map((play) => play.launch)).toEqual([1, 2, 3, 4]);
  expect(new Set(plays.map((play) => play.folder)).size).toBe(1);
  const folder = plays[0]!.folder;
  expect(folder.startsWith(one.root)).toBe(false);
  expect(existsSync(folder)).toBe(true);

  // It outlives a test's scratch, and goes with the worker.
  await removeScratch(one);
  await removeScratch(two);
  expect(existsSync(folder)).toBe(true);
  const warnings: string[] = [];
  await warm.remove((message) => warnings.push(message), 1000);
  expect(existsSync(folder)).toBe(false);
  expect(warnings).toEqual([]);
});

test("a warm profile never asked for makes no folder", async () => {
  const warm = warmProfile(base);
  await warm.remove(() => {
    throw new Error("warned");
  });
  expect(readTree(base)).toEqual({ "fake_agent.exe": "" });
});

test("readying a reused profile deletes the last launch's port file", async () => {
  const folder = join(base, "profile");
  mkdirSync(join(folder, "EBWebView"), { recursive: true });
  writeFileSync(portFile(folder), "51234\n/devtools/browser/old\n");
  writeFileSync(join(folder, "EBWebView", "Local State"), "{}");
  await readyReusedProfile(folder, 1000);
  expect(existsSync(portFile(folder))).toBe(false);
  // The profile itself stays: only the port file goes.
  expect(existsSync(join(folder, "EBWebView", "Local State"))).toBe(true);
  // No port file (the launch before wrote none) is fine too.
  await readyReusedProfile(folder, 1000);
});

test("readying a reused profile waits while a browser holds it, and fails when it never lets go", async () => {
  const folder = join(base, "profile");
  mkdirSync(join(folder, "EBWebView"), { recursive: true });
  writeFileSync(portFile(folder), "51234\n");
  writeFileSync(profileLock(folder), "");
  await expect(readyReusedProfile(folder, 300)).rejects.toThrow(
    /still holds the reused WebView2 profile/,
  );
  // The port file stays until the profile is free, so nothing reads the old port meanwhile either.
  expect(existsSync(portFile(folder))).toBe(true);

  // The browser exits (Chromium's lock goes with it) while the fixture waits.
  const ready = readyReusedProfile(folder, 5000);
  setTimeout(() => rmSync(profileLock(folder)), 250);
  await ready;
  expect(existsSync(portFile(folder))).toBe(false);
});

test("removing the warm profile waits for the last browser to let go of it", async () => {
  const warm = warmProfile(base);
  const folder = warm.folder();
  mkdirSync(join(folder, "EBWebView"));
  writeFileSync(profileLock(folder), "");
  const warnings: string[] = [];
  const removed = warm.remove((message) => warnings.push(message), 5000);
  // Still held: nothing is removed yet.
  await new Promise((resolve) => setTimeout(resolve, 150));
  expect(existsSync(folder)).toBe(true);
  // The browser exits and its lock goes; then the folder goes, without a warning.
  rmSync(profileLock(folder));
  await removed;
  expect(existsSync(folder)).toBe(false);
  expect(warnings).toEqual([]);
});

test("a warm profile still locked at the worker's end warns instead of failing", async () => {
  const warm = warmProfile(base);
  const folder = warm.folder();
  mkdirSync(join(folder, "EBWebView"));
  writeFileSync(profileLock(folder), "");
  const warnings: string[] = [];
  await warm.remove((message) => warnings.push(message), 200);
  expect(warnings).toHaveLength(1);
  expect(warnings[0]).toMatch(
    /still holds the warm WebView2 profile .* 200 ms after the last launch ended/,
  );
});

test("resetting the warm profile removes its folder and starts over at launch 1", async () => {
  const warm = warmProfile(base);
  const one = makeScratch({ scenario: null, matterFiles: {} }, () => agent, base);
  const first = launchProfile("warm", one, 1, warm);
  expect(first.launch).toBe(1);
  mkdirSync(join(first.folder, "EBWebView"));
  // The failed launch's browser still holds the profile for a moment after its tree is killed.
  writeFileSync(profileLock(first.folder), "");
  const warnings: string[] = [];
  const reset = warm.reset((message) => warnings.push(message), 5000);
  await new Promise((resolve) => setTimeout(resolve, 150));
  expect(existsSync(first.folder)).toBe(true);
  rmSync(profileLock(first.folder));
  await reset;
  expect(existsSync(first.folder)).toBe(false);
  expect(warnings).toEqual([]);
  expect(warm.launches).toBe(0);

  // The retry is launch 1 again, on a new folder, and later launches reuse that one.
  const retry = launchProfile("warm", one, 2, warm);
  expect(retry.launch).toBe(1);
  expect(retry.folder).not.toBe(first.folder);
  expect(existsSync(retry.folder)).toBe(true);
  expect(launchProfile("warm", one, 3, warm)).toEqual({
    profile: "warm",
    folder: retry.folder,
    launch: 2,
  });
  await warm.remove(() => {}, 1000);
  expect(existsSync(retry.folder)).toBe(false);
});

test("resetting a warm profile never asked for is a no-op", async () => {
  const warm = warmProfile(base);
  await warm.reset(() => {
    throw new Error("warned");
  });
  expect(warm.launches).toBe(0);
  expect(readTree(base)).toEqual({ "fake_agent.exe": "" });
});

test("a warm-up that passes first time is neither recovered nor reported", async () => {
  const calls: string[] = [];
  const result = await retryWarmUp(
    async () => {
      calls.push("attempt");
      return 7;
    },
    () => true,
    async () => {
      calls.push("recover");
    },
    () => calls.push("report"),
  );
  expect(result).toBe(7);
  expect(calls).toEqual(["attempt"]);
});

test("a warm-up that fails once is recovered, reported on one line, and tried again", async () => {
  const calls: string[] = [];
  let tries = 0;
  const result = await retryWarmUp(
    async () => {
      tries += 1;
      calls.push(`attempt ${tries}`);
      if (tries === 1) throw new Error("connecting over CDP\r\ntimed out\nat port 9222");
      return "app";
    },
    () => true,
    async (error) => {
      calls.push(`recover ${(error as Error).message.split(/\r?\n/)[0]}`);
    },
    (message) => calls.push(`report ${message}`),
  );
  expect(result).toBe("app");
  // Cleaned up before the report, and both before the second try.
  expect(calls).toEqual([
    "attempt 1",
    "recover connecting over CDP",
    "report connecting over CDP timed out at port 9222",
    "attempt 2",
  ]);
});

test("a warm-up that fails twice throws the second error, naming the first", async () => {
  let tries = 0;
  const reports: string[] = [];
  const second = new Error("no page");
  const failed = retryWarmUp(
    async () => {
      tries += 1;
      throw tries === 1 ? new Error("no port") : second;
    },
    () => true,
    async () => {},
    (message) => reports.push(message),
  );
  await expect(failed).rejects.toThrow(/no page.*first try: no port/);
  await failed.catch((error: Error) => expect(error.cause).toBe(second));
  expect(tries).toBe(2);
  expect(reports).toEqual(["no port"]);
});

test("an error that is not a warm-up setup failure is never retried", async () => {
  let tries = 0;
  const failed = retryWarmUp(
    async () => {
      tries += 1;
      throw new Error("a fresh launch failed");
    },
    () => false,
    async () => {
      throw new Error("recovered");
    },
    () => {
      throw new Error("reported");
    },
  );
  await expect(failed).rejects.toThrow("a fresh launch failed");
  expect(tries).toBe(1);
});

test("a warm-up whose clean-up fails throws without a second try, naming the first error", async () => {
  let tries = 0;
  const cleanup = new Error("taskkill failed");
  const failed = retryWarmUp(
    async () => {
      tries += 1;
      throw new Error("no port");
    },
    () => true,
    async () => {
      throw cleanup;
    },
    () => {},
  );
  await expect(failed).rejects.toThrow(/cleaning up .* \(no port\)/);
  await failed.catch((error: Error) => expect(error.cause).toBe(cleanup));
  expect(tries).toBe(1);
});

/** An error as fs gives one for a file still held open. */
function held(code = "EPERM"): NodeJS.ErrnoException {
  return Object.assign(new Error(`${code}, Permission denied`), { code });
}

/**
 * A stand-in for the removal that fails with `error` while `stillHeld()`, then removes the root
 * for real; counts its calls.
 */
function removal(stillHeld: () => boolean, error: () => Error = held) {
  const calls = { count: 0 };
  const remove = (root: string): void => {
    calls.count += 1;
    if (stillHeld()) throw error();
    rmSync(root, { recursive: true, force: true });
  };
  return { calls, remove };
}

const quick = { exitTimeoutMs: 200, killTimeoutMs: 200, pollMs: 10 };

test("a root nobody holds goes at once: the holders are never asked", async () => {
  const scratch = makeScratch({ scenario: null, matterFiles: {} }, () => agent, base);
  let asked = 0;
  await removeScratch(scratch, {
    ...quick,
    holders: {
      find: async () => {
        asked += 1;
        return [];
      },
      kill: async () => {},
    },
  });
  expect(existsSync(scratch.root)).toBe(false);
  expect(asked).toBe(0);
});

test("a held root waits for its WebView2 processes to exit, then goes, and says so", async () => {
  const scratch = makeScratch({ scenario: null, matterFiles: {} }, () => agent, base);
  let finds = 0;
  const killed: number[][] = [];
  const warnings: string[] = [];
  const { calls, remove } = removal(() => finds < 3);
  await removeScratch(scratch, {
    ...quick,
    remove,
    warn: (message) => warnings.push(message),
    holders: {
      find: async (root) => {
        expect(root).toBe(scratch.root);
        finds += 1;
        return finds < 3 ? [41, 42] : [];
      },
      kill: async (pids) => {
        killed.push([...pids]);
      },
    },
  });
  expect(existsSync(scratch.root)).toBe(false);
  expect(finds).toBe(3);
  expect(killed).toEqual([]);
  expect(calls.count).toBe(2);
  expect(warnings).toHaveLength(1);
  expect(warnings[0]).toContain(`${scratch.root} was still held after the app ended`);
});

test("holders still running when the wait ends are killed, then the root goes", async () => {
  const scratch = makeScratch({ scenario: null, matterFiles: {} }, () => agent, base);
  let alive = true;
  const killed: number[][] = [];
  const warnings: string[] = [];
  const { remove } = removal(
    () => alive,
    () => held("EBUSY"),
  );
  await removeScratch(scratch, {
    ...quick,
    remove,
    warn: (message) => warnings.push(message),
    holders: {
      find: async () => (alive ? [41, 42] : []),
      kill: async (pids) => {
        killed.push([...pids]);
        alive = false;
      },
    },
  });
  expect(existsSync(scratch.root)).toBe(false);
  expect(killed).toEqual([[41, 42]]);
  expect(warnings).toEqual([
    `WebView2 processes 41, 42 still held ${scratch.root} 200 ms after the app ended; killing them`,
  ]);
});

test("a holder the kill leaves: the root is removed anyway, and that removal's error stands", async () => {
  const scratch = makeScratch({ scenario: null, matterFiles: {} }, () => agent, base);
  const warnings: string[] = [];
  const { calls, remove } = removal(() => true);
  const removed = removeScratch(scratch, {
    ...quick,
    remove,
    warn: (message) => warnings.push(message),
    holders: {
      find: async () => [41],
      kill: async () => {
        throw new Error("Access is denied");
      },
    },
  });
  await expect(removed).rejects.toMatchObject({ code: "EPERM" });
  expect(calls.count).toBe(2);
  expect(warnings).toEqual([
    `WebView2 processes 41 still held ${scratch.root} 200 ms after the app ended; killing them`,
    "killing 41 failed: Access is denied",
    `processes 41 still held ${scratch.root} after the kill; removing it anyway`,
  ]);
  rmSync(scratch.root, { recursive: true, force: true });
});

test("a failed process list is warned of, and the removal is still tried", async () => {
  const scratch = makeScratch({ scenario: null, matterFiles: {} }, () => agent, base);
  let tries = 0;
  const warnings: string[] = [];
  const { calls, remove } = removal(() => tries++ === 0);
  await removeScratch(scratch, {
    ...quick,
    remove,
    warn: (message) => warnings.push(message),
    holders: {
      find: async () => {
        throw new Error("powershell timed out");
      },
      kill: async () => {},
    },
  });
  expect(existsSync(scratch.root)).toBe(false);
  expect(calls.count).toBe(2);
  expect(warnings).toEqual([
    `listing the processes that hold ${scratch.root} failed: powershell timed out`,
  ]);
});

test("an error that is no held file, or a held root without holders, throws at once", async () => {
  const scratch = makeScratch({ scenario: null, matterFiles: {} }, () => agent, base);
  let asked = 0;
  const holders = {
    find: async () => {
      asked += 1;
      return [];
    },
    kill: async () => {},
  };
  const other = removal(
    () => true,
    () => Object.assign(new Error("EINVAL, invalid argument"), { code: "EINVAL" }),
  );
  await expect(
    removeScratch(scratch, { ...quick, remove: other.remove, holders }),
  ).rejects.toMatchObject({ code: "EINVAL" });
  const unheld = removal(() => true);
  await expect(removeScratch(scratch, { ...quick, remove: unheld.remove })).rejects.toMatchObject({
    code: "EPERM",
  });
  expect(asked).toBe(0);
  expect(other.calls.count + unheld.calls.count).toBe(2);
  await removeScratch(scratch);
  expect(existsSync(scratch.root)).toBe(false);
});
