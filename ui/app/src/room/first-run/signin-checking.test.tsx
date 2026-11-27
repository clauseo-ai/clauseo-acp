// @vitest-environment happy-dom
// A3 (Windows RC): after an update, a plan installed whose sign-in desktop hasn't said yet showed Sign in beside
// "Checking whether you're signed in to Claude…" for 13 s, until the status check found the login. Now absent (installed,
// no state heard) and desktop's own `checking` both read checking with nothing to press, in the composer and on the
// first screen; signed out brings Sign in back; signed in shows neither. A desktop that never answers leaves the
// absent state checking for CHECK_MS at most (sign-in-check.ts), then Sign in.
import { act } from "react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { CHECK_MS } from "../../store/sign-in-check.ts";
import type { SignIn } from "../../transport/protocol.ts";
import {
  A,
  CLAUDE,
  READY,
  SIGNED_IN,
  allById,
  byId,
  install,
  mountFirstRun,
  play,
  setups,
  signIn,
  text,
} from "./rig.tsx";

// the room's lazy chunks, loaded once under the hook's own limit (as signin.test.tsx does)
beforeAll(async () => {
  await Promise.all([
    import("../first-read/first-read-offer.tsx"),
    import("../new-matter.tsx"),
    import("../locate-warning.tsx"),
    import("../thread/list.tsx"),
    import("../protection/note-load.ts").then((m) => m.loadProtectionNote()),
    import("../signin-expired/expired-card.tsx"),
  ]);
}, 30_000);

afterEach(() => {
  vi.useRealTimers();
});

const CHECKING = "Checking whether you're signed in to Claude…";

/** Every Sign in on the page: the gate's, the first screen's foot and rows. */
const signIns = () =>
  [...document.querySelectorAll("button")].filter((b) =>
    (b.textContent ?? "").startsWith("Sign in"),
  );

const SURFACES = [
  ["the composer", [A]],
  ["the first screen", []],
] as const;

test.each(SURFACES)("absent after Ready on %s: checking, and no Sign in", async (_, m) => {
  const t = await mountFirstRun({ matters: [...m], first: [install(CLAUDE, READY)] });
  expect(text()).toContain(CHECKING);
  expect(signIns()).toEqual([]);
  expect(setups(t)).toEqual([]);
});

test.each(SURFACES)("desktop's checking on %s: still checking, no Sign in", async (_, m) => {
  const t = await mountFirstRun({ matters: [...m], first: [install(CLAUDE, READY)] });
  await play(t, signIn(CLAUDE, "checking"));
  expect(text()).toContain(CHECKING);
  expect(signIns()).toEqual([]);
});

test.each(SURFACES)("the answer on %s: signed out brings Sign in", async (_, m) => {
  const t = await mountFirstRun({ matters: [...m], first: [install(CLAUDE, READY)] });
  await play(t, signIn(CLAUDE, "checking"), signIn(CLAUDE, { signed_out: {} }));
  expect(text()).not.toContain(CHECKING);
  expect(signIns().length).toBeGreaterThan(0);
});

test.each(SURFACES)("the answer on %s: signed in shows neither", async (_, m) => {
  const t = await mountFirstRun({ matters: [...m], first: [install(CLAUDE, READY)] });
  await play(t, signIn(CLAUDE, "checking"), signIn(CLAUDE, SIGNED_IN as SignIn));
  expect(text()).not.toContain(CHECKING);
  expect(signIns()).toEqual([]);
  expect(byId("signin")).toBeNull();
});

test.each(SURFACES)(
  "no answer on %s: checking until CHECK_MS, then Sign in; a late answer still counts",
  async (_, m) => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const t = await mountFirstRun({ matters: [...m], first: [install(CLAUDE, READY)] });
    await act(async () => {
      vi.advanceTimersByTime(CHECK_MS - 2_000);
    });
    await play(t);
    expect(text()).toContain(CHECKING);
    expect(signIns()).toEqual([]);
    await act(async () => {
      vi.advanceTimersByTime(3_000);
    });
    await play(t);
    expect(text()).not.toContain(CHECKING);
    expect(signIns().length).toBeGreaterThan(0);
    // nothing was sent by itself
    expect(setups(t)).toEqual([]);
    await play(t, signIn(CLAUDE, SIGNED_IN as SignIn));
    expect(signIns()).toEqual([]);
  },
);

test("a plan not installed keeps its own line: no checking for a plan with no install yet", async () => {
  await mountFirstRun({
    matters: [],
    first: [install(CLAUDE, { missing: { download_bytes: 160_000_000 } })],
  });
  expect(text()).not.toContain(CHECKING);
  expect(allById("install-sign-in")).toHaveLength(1);
});
