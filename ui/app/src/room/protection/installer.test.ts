// Design B's repair page: stage F's per-version release page for the build's release mode, built
// in one place, and no page for a dry run or a version that isn't a plain X.Y.Z. And the
// cause-to-words table: each reason R17 PR-E names, other_account, and an unknown reason's "ask
// IT" line.
import { INSTALLER_CLOSE, pw } from "@clauseo/components/lib/words-protection.ts";
import { afterEach, expect, test, vi } from "vitest";
import { causeOf } from "./causes.ts";
import { blockLine, blockOf, firmInstall, needsInstaller } from "./blocked.ts";
import { PROT, view } from "./fixtures.ts";
import { installerPage, openInstallerPage, repairPage } from "./installer.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import type { NotProtectedCause } from "../../transport/protocol.ts";

afterEach(() => vi.restoreAllMocks());

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

test("a release opens its v tag's page", () => {
  expect(repairPage({ releaseMode: "release" }, "0.1.0")).toBe(
    "https://github.com/clauseo-ai/clauseo-acp/releases/tag/v0.1.0",
  );
});

test("a test build opens its test-v tag's page", () => {
  expect(repairPage({ releaseMode: "test" }, "0.1.0")).toBe(
    "https://github.com/clauseo-ai/clauseo-acp/releases/tag/test-v0.1.0",
  );
});

test("a dry run has no page, so no button", () => {
  expect(repairPage({ releaseMode: "dry" }, "0.1.0")).toBeNull();
});

test.each(["release", "test", "dry"] as const)(
  "in %s mode, a version that isn't plain X.Y.Z has no page",
  (releaseMode) => {
    expect(repairPage({ releaseMode }, "0.1.0-dev")).toBeNull();
  },
);

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

test("the repair is the installer only for a missing admin copy; set-up causes keep the app's button", () => {
  expect(needsInstaller(cause({ blocked: { reason: "admin_copy_missing" } }))).toBe(true);
  for (const c of [
    "needs_set_up",
    "set_up_refused",
    "set_up_failed",
    { blocked: { reason: "policy" } },
  ])
    expect(needsInstaller(cause(c))).toBe(false);
});

test("a firm install: per machine, or the firm's policy as a hint; a per-user install isn't", () => {
  expect(firmInstall(view(PROT.missing), null)).toBe(false);
  expect(firmInstall(view(PROT.missing), "package")).toBe(false);
  expect(firmInstall(view(PROT.missing), "per_machine")).toBe(true);
  expect(firmInstall(view(PROT.missing), "policy")).toBe(true);
  expect(firmInstall(view(PROT.missing, { required: true }), null)).toBe(true);
  expect(firmInstall(view(PROT.missing, { unprotected_auto_allowed: false }), null)).toBe(true);
});

// Get the installer: the page opens through open_external, and only then Clauseo quits through
// quit_app (the app's normal quit); a decline or a refusal leaves Clauseo open.
const RELEASE = "https://github.com/clauseo-ai/clauseo-acp/releases/tag/v0.1.0";
const DECLINED = { code: "transport.unauthorized", message: "declined", retryable: false };
const REFUSED = { code: "command.invalid", message: "bad link", retryable: false };

test("accepted: the release page opens, and Clauseo quits once, after it", async () => {
  let accept: () => void = () => {};
  const t = new FakeTransport({
    openExternal: () => new Promise<void>((resolve) => (accept = resolve)),
  });
  const done = openInstallerPage(t, RELEASE);
  expect(t.only("open_external")).toEqual([{ call: "open_external", url: RELEASE }]);
  expect(t.only("quit_app")).toEqual([]);
  accept();
  await done;
  expect(t.calls.filter((c) => c.call === "open_external" || c.call === "quit_app")).toEqual([
    { call: "open_external", url: RELEASE },
    { call: "quit_app" },
  ]);
});

test("declined: nothing quits, and Clauseo stays open", async () => {
  const t = new FakeTransport({ openExternal: () => Promise.reject(DECLINED) });
  await openInstallerPage(t, RELEASE);
  expect(t.only("open_external")).toHaveLength(1);
  expect(t.only("quit_app")).toEqual([]);
});

test("a refused open says so to the console and quits nothing", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const t = new FakeTransport({ openExternal: () => Promise.reject(REFUSED) });
  await openInstallerPage(t, RELEASE);
  expect(t.only("quit_app")).toEqual([]);
  expect(warn).toHaveBeenCalledOnce();
});

test("a refused quit is said to the console; the page stays open", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const t = new FakeTransport({ quitApp: () => Promise.reject(REFUSED) });
  await openInstallerPage(t, RELEASE);
  expect(t.only("open_external")).toHaveLength(1);
  expect(t.only("quit_app")).toHaveLength(1);
  expect(warn).toHaveBeenCalledOnce();
});

test("a second click while the first waits for the confirm opens nothing more and quits once", async () => {
  let accept: () => void = () => {};
  const t = new FakeTransport({
    openExternal: () => new Promise<void>((resolve) => (accept = resolve)),
  });
  const first = openInstallerPage(t, RELEASE);
  const second = openInstallerPage(t, RELEASE);
  accept();
  await Promise.all([first, second]);
  expect(t.only("open_external")).toHaveLength(1);
  expect(t.only("quit_app")).toHaveLength(1);
});

test("after a decline, the next click asks again", async () => {
  let answer: () => void = () => {};
  const t = new FakeTransport({
    openExternal: () =>
      t.only("open_external").length === 1
        ? Promise.reject(DECLINED)
        : new Promise<void>((resolve) => (answer = resolve)),
  });
  await openInstallerPage(t, RELEASE);
  const again = openInstallerPage(t, RELEASE);
  answer();
  await again;
  expect(t.only("open_external")).toHaveLength(2);
  expect(t.only("quit_app")).toHaveLength(1);
});

test("the close line says what happens, in English and in Hindi", () => {
  expect(pw("en", INSTALLER_CLOSE)).toBe(
    "Clauseo will close so the installer can set up the Safe workspace. Run the installer you download, then open Clauseo again.",
  );
  expect(pw("hi", INSTALLER_CLOSE)).toBe(
    "Clauseo बंद हो जाएगा, ताकि इंस्टॉलर सुरक्षित कार्यक्षेत्र सेट अप कर सके। जो इंस्टॉलर आप डाउनलोड करें, उसे चलाएँ, फिर Clauseo दोबारा खोलें।",
  );
});
