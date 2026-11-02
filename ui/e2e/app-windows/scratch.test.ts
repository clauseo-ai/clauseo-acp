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
  scratchEnv,
  scratchSettings,
  warmProfile,
  webviewFolder,
} from "./scratch.ts";

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

test("the folders: data with settings, a seeded matter beside it, and the launch variables", () => {
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
  });
  const first = webviewFolder(scratch, 1);
  expect(webviewFolder(scratch, 2)).not.toBe(first);
  removeScratch(scratch);
  expect(existsSync(scratch.root)).toBe(false);
});

test("no scenario: settings hold only the matters' location, and fake_agent is never asked for", () => {
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
  expect(settings).toEqual({ matters: { default_location: scratch.mattersDir } });
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

test("a fresh profile is a new folder per launch; a warm one is the worker's folder, counted", () => {
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
  removeScratch(one);
  removeScratch(two);
  expect(existsSync(folder)).toBe(true);
  warm.remove();
  expect(existsSync(folder)).toBe(false);
});

test("a warm profile never asked for makes no folder", () => {
  const warm = warmProfile(base);
  warm.remove();
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
