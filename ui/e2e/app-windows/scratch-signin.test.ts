// The scratch folders' scripted sign-ins (`E2E_FAKE_SIGNIN`) on any OS: no app, no Windows.

import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { fakeSignInVar, makeScratch, removeScratch, scratchEnv } from "./scratch.ts";

let base: string;

test.beforeEach(() => {
  base = mkdtempSync(join(tmpdir(), "clauseo-scratch-signin-"));
  writeFileSync(join(base, "fake_agent.exe"), "");
});

test.afterEach(() => {
  rmSync(base, { recursive: true, force: true });
});

test("scripted sign-ins reach the app as E2E_FAKE_SIGNIN, and none sets no variable", async () => {
  const opts = { scenario: null, matterFiles: {}, fakeSignIn: { "claude-acp": "ok" as const } };
  const scratch = makeScratch(opts, () => join(base, "fake_agent.exe"), base);
  expect(scratchEnv(scratch)["E2E_FAKE_SIGNIN"]).toBe("claude-acp=ok");
  // installed by the seeding, but signed out: out of `plans`, so its sign-in card shows
  expect(scratchEnv(scratch)["E2E_AGENTS_MANAGED"]).toBe("claude-acp,codex-acp");
  const settings = JSON.parse(readFileSync(join(scratch.dataDir, "settings.json"), "utf8"));
  expect(Object.keys(settings.plans)).toEqual(["codex-acp"]);
  expect(fakeSignInVar({ "claude-acp": "ok", "codex-acp": "fail" })).toBe(
    "claude-acp=ok,codex-acp=fail",
  );
  await removeScratch(scratch);
});

test("no scripted sign-in sets no variable", async () => {
  const scratch = makeScratch({ scenario: null, matterFiles: {} }, () => "", base);
  expect(scratchEnv(scratch)).not.toHaveProperty("E2E_FAKE_SIGNIN");
  await removeScratch(scratch);
});
