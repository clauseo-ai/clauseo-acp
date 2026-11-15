// The install's view from the host's words (views.ts): the download's size as "about" (the
// nearest 10 MB from 100 MB up, whole MB below), and a download's done MB never above its total.
import { expect, test } from "vitest";
import { CLAUDE, type AgentSetup, type SetupState } from "../../store/setup.ts";
import { aboutMb, installView as viewOf, queuedView, toolsView } from "./views.ts";

const setup = (install: NonNullable<AgentSetup["install"]>, mb?: number): AgentSetup =>
  mb === undefined ? { install } : { install, mb };
/** The install's view with the window offline: the network's words change only a waiting install's. */
const installView = (s: AgentSetup) => viewOf(s, false);

test("about: 160 MB stays 160, 182 MB reads about 180, 165 MB about 170, under 100 MB whole", () => {
  expect(aboutMb(160)).toBe(160);
  expect(aboutMb(182)).toBe(180);
  expect(aboutMb(165)).toBe(170);
  expect(aboutMb(99)).toBe(99);
  expect(aboutMb(0)).toBe(0);
});

test("missing says the download's size, about, from the bytes the host gives", () => {
  expect(installView(setup({ missing: { download_bytes: 160_000_000 } }))).toEqual({
    kind: "missing",
    mb: 160,
  });
  expect(installView(setup({ missing: { download_bytes: 182_400_000 } }))).toEqual({
    kind: "missing",
    mb: 180,
  });
  expect(installView(setup({ missing: { download_bytes: 0 } }))).toEqual({
    kind: "missing",
    mb: 0,
  });
});

test("missing after Clauseo installed it here (#859) says reinstall; never installed says nothing of it", () => {
  const gone = { missing: { download_bytes: 160_000_000, installed_before: true } };
  expect(installView(setup(gone))).toStrictEqual({ kind: "missing", mb: 160, reinstall: true });
  const never = { missing: { download_bytes: 160_000_000, installed_before: false } };
  expect(installView(setup(never))).toStrictEqual({ kind: "missing", mb: 160 });
});

test("downloading carries the bytes for the bar and whole MB for the words: 45 MB of 160 MB", () => {
  expect(installView(setup({ downloading: { received: 45_000_000, total: 160_000_000 } }))).toEqual(
    {
      kind: "downloading",
      received: 45_000_000,
      total: 160_000_000,
      doneMb: 45,
      totalMb: 160,
    },
  );
});

test("downloading's done never runs above its total", () => {
  const view = installView(setup({ downloading: { received: 161_000_000, total: 160_000_000 } }));
  expect(view).toMatchObject({ kind: "downloading", doneMb: 160, totalMb: 160 });
});

test("a failed set-up is a failed view marked setup; a failed download is not", () => {
  const failed = (code: "agent.install_setup_failed" | "agent.install_failed") =>
    installView(setup({ failed: { code, hosts: [], log: ["result: failed after 3 tries"] } }));
  expect(failed("agent.install_setup_failed")).toEqual({
    kind: "failed",
    log: ["result: failed after 3 tries"],
    setup: true,
  });
  expect(failed("agent.install_failed")).toEqual({
    kind: "failed",
    log: ["result: failed after 3 tries"],
  });
});

test("installing has no bytes; paused says the size it had, about", () => {
  expect(installView(setup("installing"))).toEqual({ kind: "installing" });
  expect(installView(setup("paused", 182))).toEqual({ kind: "missing", mb: 180 });
});

test("a waiting install, its queued sign-in and the waiting document tools carry online only with the window online", () => {
  const waiting = setup({ waiting: { code: "agent.install_offline", hosts: [], log: [] } });
  expect(viewOf(waiting, false)).toStrictEqual({ kind: "offline" });
  expect(viewOf(waiting, true)).toStrictEqual({ kind: "offline", online: true });
  const state: SetupState = {
    keys: true,
    python: null,
    agents: {},
    chosen: CLAUDE,
    picked: false,
    queued: null,
    pending: null,
    keyOpen: false,
    pasting: false,
    copied: null,
    counts: { on: true, locked: false, started: false },
    locked: [],
  };
  expect(queuedView(waiting, state, undefined, false)).toStrictEqual({
    kind: "queued",
    offline: true,
  });
  expect(queuedView(waiting, state, undefined, true)).toStrictEqual({
    kind: "queued",
    offline: true,
    online: true,
  });
  const tools: SetupState = {
    ...state,
    python: { waiting: { code: "agent.install_offline", hosts: [], log: [] } },
  };
  expect(toolsView(tools, false)).toStrictEqual({ kind: "waiting" });
  expect(toolsView(tools, true)).toStrictEqual({ kind: "waiting", online: true });
});
