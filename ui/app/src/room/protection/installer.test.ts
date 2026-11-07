// Design B's repair page: stage F's per-version release page, built in one place, and no page for a
// test build or a version that isn't a plain X.Y.Z. And the cause-to-words table: each reason R17
// PR-E names, other_account, and an unknown reason's "ask IT" line.
import { pw } from "@clauseo/components/lib/words-protection.ts";
import { expect, test } from "vitest";
import { causeOf } from "./causes.ts";
import { blockLine, blockOf, firmInstall, needsInstaller } from "./blocked.ts";
import { NO_PROMPT, PROT, view } from "./fixtures.ts";
import { installerPage, isTestBuild, repairPage } from "./installer.ts";
import type { NotProtectedCause } from "../../transport/protocol.ts";

test("the installer's page is the installed version's release page, never latest", () => {
  expect(installerPage("0.1.0")).toBe(
    "https://github.com/clauseo-ai/clauseo-acp/releases/tag/v0.1.0",
  );
  expect(installerPage("3.12.4")).toBe(
    "https://github.com/clauseo-ai/clauseo-acp/releases/tag/v3.12.4",
  );
});

test.each(["0.1.0-dev", "v0.1.0", "", "0.1"])(
  "a version %j that isn't plain X.Y.Z has no page",
  (v) => {
    expect(installerPage(v)).toBeNull();
  },
);

test("a test build offers no page; any other display does", () => {
  expect(isTestBuild({ testBuild: true })).toBe(true);
  expect(isTestBuild({ lang: "en" })).toBe(false);
  expect(repairPage({ testBuild: true }, "0.1.0")).toBeNull();
  expect(repairPage({ testBuild: false }, "0.1.0")).toBe(installerPage("0.1.0"));
});

const cause = (c: unknown) => causeOf(c as NotProtectedCause);

test.each([
  [
    "logon_right",
    "Windows policy on this computer doesn't let the Safe workspace's accounts sign in. Ask your IT team to add them to the Allow log on locally policy.",
  ],
  [
    "policy",
    "A Windows policy on this computer blocks the program that sets up the Safe workspace. Ask your IT team to allow it.",
  ],
  [
    "tool_missing",
    "The program that sets up the Safe workspace is missing, or security software blocked it. Ask your IT team to look into it.",
  ],
  ["admin_copy_missing", "The Safe workspace wasn't set up when Clauseo was installed."],
  [
    "admin_copy_changed",
    "The program that sets up the Safe workspace has changed since Clauseo was installed. Ask your IT team, or install Clauseo again.",
  ],
  [
    "admin_copy_unsafe",
    "The folder that holds the Safe workspace's set-up program isn't limited to administrators, or holds files it shouldn't. Ask your IT team to look into it.",
  ],
] as const)("blocked { reason: %s } has its own line", (reason, line) => {
  const c = cause({ blocked: { reason } });
  expect(c.group).toBe("cant");
  expect(blockOf(c)).toBe(reason);
  expect(blockLine("en", c)).toBe(pw("en", line));
});

test("other_account has its own line; an unknown reason, or none, asks IT", () => {
  expect(blockLine("en", cause("other_account"))).toBe(
    pw(
      "en",
      "The Safe workspace on this computer belongs to another Windows account. Ask your IT team.",
    ),
  );
  for (const c of [{ blocked: { reason: "something_new" } }, { blocked: {} }, "blocked"])
    expect(blockLine("en", cause(c))).toBe(pw("en", "Ask your IT team to look into it."));
  expect(blockLine("en", cause("needs_set_up"))).toBeNull();
});

test("the repair is the installer for a missing admin copy, and for a set-up behind Windows' prompt", () => {
  const missing = cause({ blocked: { reason: "admin_copy_missing" } });
  expect(needsInstaller(view(PROT.missing), missing)).toBe(true);
  expect(needsInstaller(view(PROT.needs), cause("needs_set_up"))).toBe(true);
  expect(needsInstaller(view(PROT.needs, NO_PROMPT), cause("needs_set_up"))).toBe(false);
  expect(needsInstaller(view(PROT.policy), cause({ blocked: { reason: "policy" } }))).toBe(false);
  expect(firmInstall(view(PROT.missing))).toBe(false);
  expect(firmInstall(view(PROT.missing, { required: true }))).toBe(true);
  expect(firmInstall(view(PROT.missing, { unprotected_auto_allowed: false }))).toBe(true);
});
