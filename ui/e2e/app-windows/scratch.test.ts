// The scratch folders on any OS: no app, no Windows.

import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import {
  agentSettings,
  fakeAgentPath,
  makeScratch,
  readTree,
  removeScratch,
  scratchEnv,
  scratchSettings,
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

test("settings start fake_agent on the scenario as agents.claude", () => {
  expect(agentSettings(agent, "prompt-stream")).toEqual({
    agents: { claude: { command: agent, args: ["--scenario", "prompt-stream"] } },
  });
  // The scoreboard's pace goes after the scenario.
  expect(agentSettings(agent, "scoreboard", ["--pace-ms", "5"])).toEqual({
    agents: {
      claude: { command: agent, args: ["--scenario", "scoreboard", "--pace-ms", "5"] },
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
    agents: { claude: { command: agent, args: ["--scenario", "files-one-turn"] } },
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
