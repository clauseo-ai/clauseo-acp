// Clauseo's own browser under the browser row with none, read purely: the store's reading of the host's
// `browser_install` events and the two commands it sends, and the row's words for every state, on a Mac, on Linux,
// and never on Windows. The failures with a line of their own (UC-586 to UC-590: TLS, a refused address, the server's
// error, a full disk, no network) are the offer with that line above it; the firm's policy (UC-584) and the rest are as
// before.
import { expect, test } from "vitest";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { BrowserInstallState } from "../transport/protocol.ts";
import {
  BrowserInstallStore,
  installRunning,
  progressOf,
  type BrowserInstall,
} from "./browser-install.ts";
import { toolRow, type ToolRowRoom } from "./tool-rows.ts";
import type { KnownTool } from "./tools.ts";

const MB = 1_000_000;
const MISSING: KnownTool = { tool: "browser", state: "browser_not_installed" };
const IDLE: BrowserInstall = { progress: null, pressed: false, total: 0, behind: false };
const INSTALL_MAC =
  "Clauseo didn't find Google Chrome, Microsoft Edge, Brave or Chromium on this computer. Install one, then start a new conversation.";
const TLS =
  "Clauseo couldn't connect securely to get the browser. Ask your firm's IT team for help.";
const HOST_REFUSED =
  "The download address was refused, so Clauseo couldn't get the browser. Try again later.";
const SERVER_REFUSED =
  "Google's download server answered with an error, so Clauseo couldn't get the browser. Try again later.";
const DISK_SPACE =
  "There isn't enough free space on this computer to get the browser. Free up about 1\u00a0GB and try again.";
const NETWORK =
  "Clauseo couldn't reach the internet to get the browser. Check your connection and try again.";

const room = (install: BrowserInstall | null, platform: ToolRowRoom["platform"] = "mac") =>
  ({ settings: true, platform, install }) satisfies ToolRowRoom;
const words = (install: BrowserInstall | null, platform?: ToolRowRoom["platform"]) =>
  toolRow("en", MISSING, room(install, platform));
const at = (progress: BrowserInstall["progress"], more: Partial<BrowserInstall> = {}) => ({
  ...IDLE,
  progress,
  ...more,
});

test("the host's state as the row reads it: the firm's policy and the failures with a line; every other failure is the offer again", () => {
  expect(progressOf({ downloading: { bytes: 5, total: 10, behind: true } })).toEqual({
    downloading: { bytes: 5, total: 10, behind: true },
  });
  expect(progressOf("unpacking")).toBe("unpacking");
  expect(progressOf("offline")).toBe("offline");
  expect(progressOf("done")).toBe("done");
  expect(progressOf({ failed: { code: "firm_forbids", message: "policy" } })).toBe("forbidden");
  expect(progressOf({ failed: { code: "tls", message: "" } })).toBe("tls");
  expect(progressOf({ failed: { code: "server_refused", message: "" } })).toBe("server_refused");
  expect(progressOf({ failed: { code: "disk_space", message: "" } })).toBe("disk_space");
  expect(progressOf({ failed: { code: "network", message: "" } })).toBe("network");
  // a refused address and an HTTP error are different lines
  expect(progressOf({ failed: { code: "host_refused", message: "" } })).toBe("host_refused");
  for (const code of [
    "cancelled",
    "size_mismatch",
    "unpack",
    "signature",
    "unsupported",
    "unknown",
  ] as const)
    expect(progressOf({ failed: { code, message: "" } })).toBeNull();
  // a state from a newer host
  expect(progressOf("something_newer" as BrowserInstallState)).toBeNull();
});

test("the offer: Get a browser for Clauseo, under what to install, waiting from the press", () => {
  expect(words(IDLE)).toMatchObject({
    reason: "Install Chrome",
    note: INSTALL_MAC,
    install: { kind: "offer", line: null, button: "Get a browser for Clauseo", waiting: false },
  });
  expect(words(at(null, { pressed: true })).install).toEqual({
    kind: "offer",
    line: null,
    button: "Get a browser for Clauseo",
    waiting: true,
  });
  // Linux, for development, as on a Mac
  expect(words(IDLE, "linux").install?.kind).toBe("offer");
});

test("downloading: its size and percent, its bar, and Cancel, in place of what to install", () => {
  const row = words(at({ downloading: { bytes: 84 * MB, total: 162 * MB, behind: false } }));
  expect(row.note).toBeNull();
  expect(row.reason).toBe("Install Chrome");
  expect(row.install).toEqual({
    kind: "getting",
    behind: null,
    text: "Getting Clauseo's browser: 84\u00a0MB of 162\u00a0MB (52%)",
    value: 84 * MB,
    max: 162 * MB,
    percent: 52,
    cancel: "Cancel",
  });
  // before the size is known, the bar alone
  expect(words(at({ downloading: { bytes: 0, total: 0, behind: false } })).install).toMatchObject({
    text: "",
    percent: 0,
  });
  // unpacking: the same line at 100%
  expect(words(at("unpacking", { total: 162 * MB })).install).toMatchObject({
    kind: "getting",
    text: "Getting Clauseo's browser: 162\u00a0MB of 162\u00a0MB (100%)",
    percent: 100,
  });
});

test("behind on security updates: the update's words over its progress", () => {
  const behind = "Clauseo's browser is behind on security updates. Updating it now.";
  expect(
    words(at({ downloading: { bytes: 81 * MB, total: 162 * MB, behind: true } })).install,
  ).toMatchObject({ kind: "getting", behind, percent: 50, cancel: "Cancel" });
  expect(words(at("unpacking", { total: 162 * MB, behind: true })).install).toMatchObject({
    behind,
  });
});

test("offline: the wait, with Cancel", () => {
  const row = words(at("offline"));
  expect(row.note).toBeNull();
  expect(row.install).toEqual({
    kind: "offline",
    line: "Waiting for an internet connection to get the browser.",
    cancel: "Cancel",
  });
});

test("each failure with a line: the offer with its line above it, what to install still said; a refused address and an HTTP error differ", () => {
  const offer = (line: string) => ({
    kind: "offer",
    line,
    button: "Get a browser for Clauseo",
    waiting: false,
  });
  for (const [progress, line] of [
    ["tls", TLS],
    ["host_refused", HOST_REFUSED],
    ["server_refused", SERVER_REFUSED],
    ["disk_space", DISK_SPACE],
    ["network", NETWORK],
  ] as const) {
    const row = words(at(progress));
    expect(row.note).toBe(INSTALL_MAC);
    expect(row.reason).toBe("Install Chrome");
    expect(row.install).toEqual(offer(line));
  }
  // the plain offer has no line
  expect(words(IDLE).install).toEqual({
    kind: "offer",
    line: null,
    button: "Get a browser for Clauseo",
    waiting: false,
  });
  // a press waits as the plain offer does
  expect(words(at("tls", { pressed: true })).install).toMatchObject({ waiting: true });
});

test("the firm forbids it: the line and no offer, what to install still said", () => {
  const row = words(at("forbidden"));
  expect(row.note).toBe(INSTALL_MAC);
  expect(row.install).toEqual({
    kind: "forbidden",
    line: "Your firm doesn't allow Clauseo to download a browser.",
  });
});

test("done: ready for new conversations, and a new conversation uses it", () => {
  expect(words(at("done"))).toMatchObject({
    reason: "Ready for new conversations",
    reasonTip: "The browser comes with your next conversation.",
    note: null,
    install: {
      kind: "done",
      line: "Clauseo's browser is ready. Start a new conversation to use it.",
    },
  });
});

test("never on Windows, without an app feed, or under any other state", () => {
  for (const progress of [null, "offline", "done", "forbidden", "host_refused"] as const)
    expect(words(at(progress), "windows").install).toBeNull();
  expect(words(null).install).toBeNull();
  for (const state of ["available", "off", "locked_off", "browser_policy_unreadable"] as const)
    expect(toolRow("en", { tool: "browser", state }, room(IDLE)).install).toBeNull();
});

test("in Hindi", () => {
  const hi = (install: BrowserInstall) => toolRow("hi", MISSING, room(install)).install;
  expect(hi(IDLE)).toMatchObject({ button: "Clauseo के लिए ब्राउज़र डाउनलोड करें" });
  expect(hi(at("tls"))).toMatchObject({
    line: "Clauseo ब्राउज़र डाउनलोड करने के लिए सुरक्षित रूप से कनेक्ट नहीं कर सका। सहायता के लिए अपनी फ़र्म की IT टीम से पूछें।",
    button: "Clauseo के लिए ब्राउज़र डाउनलोड करें",
  });
  expect(hi(at("disk_space"))).toMatchObject({
    line: "ब्राउज़र डाउनलोड करने के लिए इस कंप्यूटर पर पर्याप्त खाली जगह नहीं है। लगभग 1\u00a0GB जगह खाली करें और फिर से प्रयास करें।",
  });
  expect(hi(at({ downloading: { bytes: 84 * MB, total: 162 * MB, behind: false } }))).toMatchObject(
    {
      text: "Clauseo का ब्राउज़र डाउनलोड हो रहा है: 162\u00a0MB में से 84\u00a0MB (52%)",
      cancel: "रद्द करें",
    },
  );
});

/** A store over a fake host with the app feed. */
function store() {
  const transport = new FakeTransport({
    next: { parts: ["app"], query: () => Promise.reject(new Error("not scripted")) },
  });
  return { transport, store: new BrowserInstallStore(transport) };
}
const sent = (t: FakeTransport) => t.only("dispatch").map((c) => Object.keys(c.command.command)[0]);

test("the offer sends install_browser once and waits; Cancel sends cancel_browser_install", async () => {
  const { transport, store: s } = store();
  s.cancel(); // nothing runs: nothing to cancel
  s.install();
  s.install();
  await Promise.resolve();
  expect(sent(transport)).toEqual(["install_browser"]);
  expect(s.get().pressed).toBe(true);
  transport.appEvent({
    browser_install: { state: { downloading: { bytes: 1, total: 9, behind: false } } },
  });
  expect(s.get()).toMatchObject({ pressed: false, total: 9 });
  s.install(); // under way: no second start
  s.cancel();
  await Promise.resolve();
  expect(sent(transport)).toEqual(["install_browser", "cancel_browser_install"]);
  // the host's answer to the cancel: the offer again
  transport.appEvent({
    browser_install: { state: { failed: { code: "cancelled", message: "" } } },
  });
  expect(s.get()).toEqual(IDLE);
  // offline can be cancelled too
  transport.appEvent({ browser_install: { state: "offline" } });
  s.cancel();
  await Promise.resolve();
  expect(sent(transport)).toEqual([
    "install_browser",
    "cancel_browser_install",
    "cancel_browser_install",
  ]);
});

test("a line's offer starts a download, and the firm's policy as the first event offers nothing", () => {
  for (const state of [
    { failed: { code: "tls", message: "" } },
    { failed: { code: "server_refused", message: "" } },
    { failed: { code: "host_refused", message: "" } },
    { failed: { code: "disk_space", message: "" } },
    { failed: { code: "network", message: "" } },
  ] as const) {
    const { transport, store: s } = store();
    transport.appEvent({ browser_install: { state } });
    expect(s.get().progress).not.toBeNull();
    s.install();
    expect(s.get().pressed).toBe(true);
    expect(sent(transport)).toEqual(["install_browser"]);
    expect(installRunning(s.get())).toBe(false);
  }
  // a catch-up that says the firm forbids it, with no press before: forbidden, and nothing to start
  const { transport, store: s } = store();
  transport.appEvent({
    browser_install: { state: { failed: { code: "firm_forbids", message: "" } } },
  });
  expect(s.get().progress).toBe("forbidden");
  s.install();
  expect(s.get()).toMatchObject({ progress: "forbidden", pressed: false });
  expect(sent(transport)).toEqual([]);
});

test("unpacking keeps the download's size and update; the firm's policy offers nothing to start", () => {
  const { transport, store: s } = store();
  transport.appEvent({
    browser_install: { state: { downloading: { bytes: 1, total: 9, behind: true } } },
  });
  transport.appEvent({ browser_install: { state: "unpacking" } });
  expect(s.get()).toEqual({ progress: "unpacking", pressed: false, total: 9, behind: true });
  transport.appEvent({
    browser_install: { state: { failed: { code: "firm_forbids", message: "" } } },
  });
  s.install();
  expect(sent(transport)).toEqual([]);
});
