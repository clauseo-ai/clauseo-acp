// The first screen for a plan that is installed but not signed in: the plan's Sign in on the working line at the foot,
// where a missing plan's install line stands, sending the room gate's start_sign_in; a sign-in under way says the
// gate's words with its Cancel and no second Sign in; a status check under way says it is checking, with Sign in busy;
// a signed-in or locked plan shows none.
// @vitest-environment happy-dom
import { expect, test } from "vitest";
import {
  CLAUDE,
  READY,
  SIGNED_IN,
  allById,
  byId,
  install,
  mountFirstRun,
  play,
  press,
  settingsView,
  setups,
  signIn,
  text,
} from "./rig.tsx";
import type { SignIn } from "../../transport/protocol.ts";

const line = () => byId("signin-foot");
const signInButtons = () => [...allById("signin-foot-sign-in"), ...allById("install-sign-in")];

test("installed and signed out: the foot says Claude is ready with Sign in to Claude, which sends the room gate's start_sign_in", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [install(CLAUDE, READY), signIn(CLAUDE, { signed_out: {} })],
  });
  expect(line()?.dataset["state"]).toBe("signed-out");
  expect(byId("install-said")?.textContent).toBe("Claude is ready");
  const sign = byId("signin-foot-sign-in");
  expect(sign?.textContent).toBe("Sign in to Claude");
  expect(byId("signin")).toBeNull();
  expect(setups(t)).toEqual([]);
  await press(sign);
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
  // the sign-in under way: the gate's words and its Cancel, never a second Sign in
  await play(t, signIn(CLAUDE, { waiting: { since: 0, nudge: false } }));
  expect(line()?.dataset["state"]).toBe("waiting");
  expect(text()).toContain("Finish signing in with your browser.");
  expect(signInButtons()).toEqual([]);
  await press(byId("signin-foot-cancel"));
  expect(setups(t)).toEqual([
    { start_sign_in: { agent: CLAUDE } },
    { cancel_sign_in: { agent: CLAUDE } },
  ]);
  // signed in: no line and no Sign in
  await play(t, signIn(CLAUDE, SIGNED_IN));
  expect(line()).toBeNull();
  expect(signInButtons()).toEqual([]);
});

test("installed, the sign-in status check still running: the foot is the checking line, not Claude is ready with Sign in", async () => {
  await mountFirstRun({ matters: [], first: [install(CLAUDE, READY), signIn(CLAUDE, "checking")] });
  expect(line()).toBeNull();
  expect(byId("install-foot")?.dataset["state"]).toBe("checking");
  expect(text()).toContain("Checking whether you're signed in to Claude…");
  expect(byId("install-foot")?.textContent).not.toContain("is ready");
  expect(byId("signin-foot-sign-in")).toBeNull();
  expect(signInButtons()).toHaveLength(1);
  expect(byId<HTMLButtonElement>("install-sign-in")?.disabled).toBe(true);
});

test("installed by the firm and signed out: the same Sign in", async () => {
  await mountFirstRun({
    matters: [],
    first: [install(CLAUDE, "managed"), signIn(CLAUDE, { signed_out: {} })],
  });
  expect(line()?.dataset["state"]).toBe("signed-out");
  expect(byId("signin-foot-sign-in")).not.toBeNull();
});

test("a sign-in that stopped (not a cancel): Claude is ready with Sign in on the foot", async () => {
  await mountFirstRun({
    matters: [],
    first: [
      install(CLAUDE, READY),
      signIn(CLAUDE, { failed: { code: "signin.link_expired", at: 0 } }),
    ],
  });
  expect(line()?.dataset["state"]).toBe("failed");
  expect(byId("signin-foot-sign-in")?.textContent).toBe("Sign in to Claude");
});

test("D-007: a cancelled sign-in says so with Sign in again, never Claude is ready", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [install(CLAUDE, READY), signIn(CLAUDE, { signed_out: {} })],
  });
  await press(byId("signin-foot-sign-in"));
  await play(t, signIn(CLAUDE, { waiting: { since: 0, nudge: false } }));
  await press(byId("signin-foot-cancel"));
  // the browser's Cancel, as desktop says it
  await play(t, signIn(CLAUDE, { failed: { code: "signin.cancelled", at: 0 } }));
  expect(line()?.dataset["state"]).toBe("cancelled");
  const row = byId("install-foot")?.closest("[data-testid=plan-row]")?.textContent ?? text();
  expect(row).toContain("Sign-in was cancelled.");
  expect(row).not.toContain("is ready");
  expect(byId("install-said")?.textContent).toContain("Sign-in was cancelled.");
  expect(byId("signin-foot-sign-in")?.textContent).toBe("Sign in again");
  await press(byId("signin-foot-sign-in"));
  expect(setups(t).at(-1)).toEqual({ start_sign_in: { agent: CLAUDE } });
});

test("signed in, with a plan, a key or the firm's account: no Sign in on the foot", async () => {
  await mountFirstRun({
    matters: [],
    first: [install(CLAUDE, READY), signIn(CLAUDE, SIGNED_IN)],
  });
  expect(line()).toBeNull();
  expect(signInButtons()).toEqual([]);
});

const UNDER_WAY: [string, SignIn][] = [
  ["queued", "queued"],
  ["waiting", { waiting: { since: 0, nudge: true } }],
  ["switching", { switching: { since: 0, stuck: false } }],
  ["a login found", { login_found: { from: "claude_code", email: "a@example.invalid" } }],
];

test.each(UNDER_WAY)("a sign-in in progress (%s) shows no second Sign in", async (_, state) => {
  await mountFirstRun({ matters: [], first: [install(CLAUDE, READY), signIn(CLAUDE, state)] });
  expect(signInButtons()).toEqual([]);
});

test.each([["plans.claude-acp"], ["plans."]])(
  "a plan locked by %s shows no control on the foot",
  async (lock) => {
    await mountFirstRun({
      matters: [],
      first: [install(CLAUDE, READY), signIn(CLAUDE, { signed_out: {} })],
      settings: settingsView({ locked: [lock] }),
    });
    expect(line()).toBeNull();
    expect(signInButtons()).toEqual([]);
  },
);

test("in a matter the gate has Sign in, and no foot line offers another", async () => {
  await mountFirstRun({ first: [install(CLAUDE, READY), signIn(CLAUDE, { signed_out: {} })] });
  expect(byId("signin")).not.toBeNull();
  expect(line()).toBeNull();
});
