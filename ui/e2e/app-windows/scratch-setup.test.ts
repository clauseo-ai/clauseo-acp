// How the scratch folders start desktop's agents: set up (installed and signed in) by default, as on
// a lawyer's first run on request. No app, no Windows.

import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { makeScratch, removeScratch, SETUP_AGENTS, scratchEnv, signedInPlans } from "./scratch.ts";

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

test("by default both of desktop's agents start set up", async () => {
  const scratch = makeScratch({ scenario: "prompt-stream", matterFiles: {} }, () => agent, base);
  expect(SETUP_AGENTS).toEqual(["claude-acp", "codex-acp"]);
  expect(scratch.setUp).toEqual(SETUP_AGENTS);
  expect(scratchEnv(scratch)["E2E_AGENTS_MANAGED"]).toBe("claude-acp,codex-acp");
  await removeScratch(scratch);
});

test("a first run starts no agent set up: no plans, and no install taken as the firm's", async () => {
  const scratch = makeScratch(
    { scenario: "prompt-stream", matterFiles: {}, firstRun: true },
    () => agent,
    base,
  );
  const settings: unknown = JSON.parse(
    readFileSync(join(scratch.dataDir, "settings.json"), "utf8"),
  );
  expect(settings).toEqual({
    matters: { default_location: scratch.mattersDir },
    agents: { "claude-acp": { command: agent, args: ["--scenario", "prompt-stream"] } },
  });
  expect(scratch.setUp).toEqual([]);
  expect(scratchEnv(scratch)["E2E_AGENTS_MANAGED"]).toBe("");
  await removeScratch(scratch);
});

test("signedInPlans names the provider only, so the agent's launch gets no variable", () => {
  expect(signedInPlans([])).toEqual({});
  expect(signedInPlans(["claude-acp"])).toEqual({
    plans: { "claude-acp": { provider: "anthropic" } },
  });
});
