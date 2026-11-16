// The composer shows both plans' lines while either downloads or waits (Rohan's call: both download at once, and both
// plans' progress always shows): the conversation's plan's line first, the other plan's compact below it, each with
// its own progress and Cancel. Choosing a plan never hides the other's download, and nothing pressed pauses it.
// @vitest-environment happy-dom
import { expect, test } from "vitest";
import { setupStoreOf, type SetupSource } from "../../store/setup.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { FakeTransport } from "../../transport/fake-transport.ts";
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
} from "./rig.tsx";

const storeOf = (t: FakeTransport) => setupStoreOf(t as unknown as HostTransport & SetupSource);
/** The composer's working lines, top to bottom. */
const lines = () => [...document.querySelectorAll(".room-entry .working")] as HTMLElement[];
const also = () => document.querySelector<HTMLElement>(".room-entry .working-also .working");
const DOWNLOADS = [
  install(CLAUDE, { downloading: { received: 13_000_000, total: 165_000_000 } }),
  signIn(CLAUDE, "queued"),
  install(CODEX, { downloading: { received: 90_000_000, total: 139_000_000 } }),
  signIn(CODEX, "queued"),
];

test("both downloading: the conversation's plan's line first, the other's compact below, each with its own progress", async () => {
  await mountFirstRun({ first: DOWNLOADS });
  expect(lines().map((l) => l.textContent)).toEqual([
    expect.stringContaining("Getting Claude ready… 13 MB of 165 MB"),
    expect.stringContaining("Getting ChatGPT ready… 90 MB of 139 MB"),
  ]);
  // the other plan's line names it and has its own Cancel
  expect(also()?.querySelector(".working-plan")).toBeNull();
  expect(also()?.querySelector("button")?.textContent).toBe("Cancel");
  // both are heard, in the one status region
  expect(document.querySelector(".room-entry [role=status]")?.textContent).toContain(
    "Getting ChatGPT ready…",
  );
});

test("pressing ChatGPT while Claude downloads sends ChatGPT's sign-in, no install over Claude's, and Claude's line stays, now below", async () => {
  const t = await mountFirstRun({
    first: [
      install(CLAUDE, { downloading: { received: 13_000_000, total: 165_000_000 } }),
      signIn(CLAUDE, "queued"),
      install(CODEX, { missing: { download_bytes: 139_000_000 } }),
      signIn(CODEX, { signed_out: {} }),
    ],
  });
  storeOf(t).signInTo(CODEX);
  await play(
    t,
    signIn(CODEX, "queued"),
    install(CODEX, { downloading: { received: 2_000_000, total: 139_000_000 } }),
  );
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CODEX } }]);
  expect(byId("signin")?.dataset["agent"]).toBe("chatgpt");
  expect(lines().map((l) => l.textContent)).toEqual([
    expect.stringContaining("Getting ChatGPT ready… 2 MB of 139 MB"),
    expect.stringContaining("Getting Claude ready… 13 MB of 165 MB"),
  ]);
});

test("the other plan's Cancel cancels its sign-in alone; its download line stays", async () => {
  const t = await mountFirstRun({ first: DOWNLOADS });
  await press(also()?.querySelector("button") ?? null);
  expect(setups(t)).toEqual([{ cancel_sign_in: { agent: CODEX } }]);
  await play(t, signIn(CODEX, { signed_out: {} }));
  expect(lines()).toHaveLength(2);
  expect(lines()[0]?.textContent).toContain("Getting Claude ready… 13 MB of 165 MB");
});

test("ChatGPT ready while Claude signs in: its line says its browser opens after Claude's sign-in, under Claude's gate", async () => {
  const t = await mountFirstRun({
    first: [
      install(CLAUDE, READY),
      signIn(CLAUDE, { waiting: { since: AT, nudge: false } }),
      install(CODEX, READY),
      signIn(CODEX, "queued"),
    ],
  });
  expect(byId("signin")?.dataset["agent"]).toBe("claude");
  expect(lines()).toHaveLength(1);
  expect(lines()[0]?.textContent).toContain(
    "Your browser opens for ChatGPT after Claude's sign-in.",
  );
  // signed in: the other plan's sign-in is desktop's to start; its line follows it
  await play(t, signIn(CLAUDE, SIGNED_IN), signIn(CODEX, { waiting: { since: AT, nudge: false } }));
  expect(lines().some((l) => l.textContent?.includes("Your browser opens for"))).toBe(false);
  expect(setups(t)).toEqual([]);
});

test("ChatGPT's browser sign-in open while Claude still downloads: Claude's line first, ChatGPT's below with its Cancel", async () => {
  const t = await mountFirstRun({
    first: [
      install(CLAUDE, { downloading: { received: 58_000_000, total: 165_000_000 } }),
      signIn(CLAUDE, "queued"),
      install(CODEX, READY),
      signIn(CODEX, { waiting: { since: AT, nudge: false } }),
    ],
  });
  expect(lines().map((l) => l.textContent)).toEqual([
    expect.stringContaining("Getting Claude ready… 58 MB of 165 MB"),
    expect.stringContaining("ChatGPT · Finish signing in with your browser."),
  ]);
  await press(also()?.querySelector("button") ?? null);
  expect(setups(t)).toEqual([{ cancel_sign_in: { agent: CODEX } }]);
});

test("nothing downloading or waiting on the other plan: one line, as before", async () => {
  await mountFirstRun({
    first: [
      install(CLAUDE, { downloading: { received: 13_000_000, total: 165_000_000 } }),
      signIn(CLAUDE, "queued"),
      install(CODEX, { missing: { download_bytes: 139_000_000 } }),
      signIn(CODEX, { signed_out: {} }),
    ],
  });
  expect(lines()).toHaveLength(1);
  expect(also()).toBeNull();
});
