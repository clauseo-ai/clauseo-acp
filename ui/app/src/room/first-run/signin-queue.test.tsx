// UD1 with both plans: one sign-in at a time. A press while the other plan's sign-in is under way starts its download
// and holds its sign-in, which goes by itself when the other's ends (the parallel installs; apart from signin.test.tsx
// to keep that file under the size limit).
// @vitest-environment happy-dom
import { expect, test } from "vitest";
import { setupStoreOf, type SetupSource } from "../../store/setup.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import {
  AT,
  CLAUDE,
  CODEX,
  READY,
  SIGNED_IN,
  byId,
  install,
  mountFirstRun,
  play,
  press,
  setups,
  signIn,
  text,
} from "./rig.tsx";

const gateState = () => byId("signin-state")?.dataset["state"];

test("plan-chatgpt while Claude downloads: ChatGPT's sign-in goes at once and desktop downloads it beside Claude; while Claude signs in, ChatGPT's download starts and its sign-in waits, then goes by itself, and its Cancel sends nothing", async () => {
  const t = await mountFirstRun({
    first: [
      install(CLAUDE, { downloading: { received: 1, total: 2 } }),
      signIn(CLAUDE, "queued"),
      install(CODEX, { missing: { download_bytes: 139_000_000 } }),
      signIn(CODEX, { signed_out: {} }),
    ],
  });
  // Claude's gate is the queued one: no plan offered, so the matter's line is reached through ChatGPT's own press
  // in the store, as the first screen would send it
  const store = setupStoreOf(t as unknown as HostTransport & SetupSource);
  store.signInTo(CODEX);
  await play(t);
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CODEX } }]);
  expect(byId("signin")?.dataset["agent"]).toBe("chatgpt");
  await play(t, signIn(CODEX, "queued"));
  expect(gateState()).toBe("queued");
  expect(text()).toContain("Your browser opens when ChatGPT is ready.");
  // a second round, with Claude's sign-in under way: ChatGPT waits for it, its download started
  await play(
    t,
    install(CLAUDE, READY),
    signIn(CLAUDE, { waiting: { since: AT, nudge: false } }),
    install(CODEX, { missing: { download_bytes: 139_000_000 } }),
    signIn(CODEX, { signed_out: {} }),
  );
  store.signInTo(CODEX);
  await play(t);
  expect(store.state().queued).toBe(CODEX);
  expect(setups(t).at(-1)).toEqual({ install_agent: { agent: CODEX } });
  await play(t, install(CODEX, READY));
  expect(text()).toContain("Your browser opens for ChatGPT after Claude's sign-in.");
  await press(byId("signin-cancel"));
  expect(store.state().queued).toBeNull();
  await play(t, signIn(CLAUDE, SIGNED_IN));
  expect(setups(t)).toEqual([
    { start_sign_in: { agent: CODEX } },
    { install_agent: { agent: CODEX } },
  ]);
});

test("plan-gate-queue: the gate's Sign in and Try again wait for the other plan's sign-in, and go by themselves once it ends", async () => {
  const t = await mountFirstRun({
    first: [
      install(CLAUDE, READY),
      signIn(CLAUDE, { failed: { code: "signin.link_expired", at: AT } }),
      install(CODEX, READY),
      signIn(CODEX, { waiting: { since: AT, nudge: false } }),
    ],
  });
  await press("Try again");
  expect(setups(t)).toEqual([]);
  expect(gateState()).toBe("queued");
  expect(text()).toContain("Your browser opens for Claude after ChatGPT's sign-in.");
  await play(t, signIn(CODEX, SIGNED_IN));
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
});
