// UC-230: Cancel on a plan's waiting sign-in while it downloads. Desktop drops only the sign-in and the download carries
// on, so the row says so at once, loses its Cancel, and keeps the note until the download ends or a new Sign in; the
// other plan's row is untouched. On the first screen and in the matter's composer alike.
// @vitest-environment happy-dom
import { expect, test } from "vitest";
import { setupStoreOf, type SetupSource } from "../../store/setup.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { FakeTransport } from "../../transport/fake-transport.ts";
import type { InstallProgress } from "../../transport/protocol.ts";
import {
  CLAUDE,
  CODEX,
  READY,
  allById,
  byId,
  install,
  mountFirstRun,
  play,
  press,
  setups,
  signIn,
} from "./rig.tsx";

const NOTE = "Sign-in cancelled. ChatGPT is still downloading; sign in when it's ready.";
const storeOf = (t: FakeTransport) => setupStoreOf(t as unknown as HostTransport & SetupSource);
const DOWNLOADS = [
  install(CLAUDE, { downloading: { received: 13_000_000, total: 165_000_000 } }),
  signIn(CLAUDE, "queued"),
  install(CODEX, { downloading: { received: 90_000_000, total: 139_000_000 } }),
  signIn(CODEX, "queued"),
];
/** The first screen's plan row for `agent`. */
const row = (agent: string) =>
  (allById("plan-row") as HTMLElement[]).find((r) => r.dataset["agent"] === agent) ?? null;
const cancelIn = (el: Element | null) =>
  el?.querySelector<HTMLElement>('[data-testid="signin-foot-cancel"]') ?? null;
/** The composer's other line, compact under the first, with its note. */
const also = () => document.querySelector<HTMLElement>(".room-entry .working-also");

test("first screen: Cancel while ChatGPT downloads shows the note under its row and drops its Cancel; Claude's row is untouched", async () => {
  const t = await mountFirstRun({ matters: [], first: DOWNLOADS });
  await press(cancelIn(row(CODEX)));
  await play(t);
  expect(setups(t)).toEqual([{ cancel_sign_in: { agent: CODEX } }]);
  expect(row(CODEX)?.textContent).toContain("Getting ChatGPT ready… 90 MB of 139 MB");
  expect(row(CODEX)?.textContent).toContain(NOTE);
  expect(cancelIn(row(CODEX))).toBeNull();
  // Claude's row: its download, its Cancel, no note
  expect(row(CLAUDE)?.textContent).toContain("Getting Claude ready… 13 MB of 165 MB");
  expect(cancelIn(row(CLAUDE))).not.toBeNull();
  expect(row(CLAUDE)?.textContent).not.toContain("Sign-in cancelled");
  // desktop's own signed-out word, after: nothing changes
  await play(t, signIn(CODEX, { signed_out: {} }));
  expect(row(CODEX)?.textContent).toContain(NOTE);
});

test("the note stays while the download goes and clears at ready: ChatGPT is ready, with its Sign in", async () => {
  const t = await mountFirstRun({ matters: [], first: DOWNLOADS });
  await press(cancelIn(row(CODEX)));
  await play(t, install(CODEX, { downloading: { received: 120_000_000, total: 139_000_000 } }));
  expect(row(CODEX)?.textContent).toContain(NOTE);
  await play(t, install(CODEX, READY));
  expect(row(CODEX)?.textContent).not.toContain("Sign-in cancelled");
  expect(row(CODEX)?.textContent).toContain("ChatGPT is ready");
  expect(row(CODEX)?.querySelector('[data-testid="signin-foot-sign-in"]')).not.toBeNull();
});

test("the note clears when the download fails or pauses", async () => {
  const ends: InstallProgress[] = [
    { failed: { code: "agent.install_failed", hosts: [], log: [] } },
    "paused",
  ];
  for (const ended of ends) {
    const t = await mountFirstRun({ matters: [], first: DOWNLOADS });
    await press(cancelIn(row(CODEX)));
    await play(t);
    expect(row(CODEX)?.textContent).toContain(NOTE);
    await play(t, install(CODEX, ended));
    expect(row(CODEX)?.textContent).not.toContain("Sign-in cancelled");
    document.body.innerHTML = "";
  }
});

test("a new Sign in press clears the note, and its sign-in goes", async () => {
  const t = await mountFirstRun({ matters: [], first: DOWNLOADS });
  await press(cancelIn(row(CODEX)));
  await play(t);
  expect(row(CODEX)?.textContent).toContain(NOTE);
  storeOf(t).signInTo(CODEX);
  await play(t);
  expect(row(CODEX)?.textContent).not.toContain("Sign-in cancelled");
  expect(setups(t).at(-1)).toEqual({ start_sign_in: { agent: CODEX } });
});

test("the store's own wait, cancelled while the plan downloads: nothing sent, and the same note", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [
      install(CLAUDE, READY),
      signIn(CLAUDE, { waiting: { since: 1, nudge: false } }),
      install(CODEX, { downloading: { received: 90_000_000, total: 139_000_000 } }),
      signIn(CODEX, { signed_out: {} }),
    ],
  });
  storeOf(t).signInTo(CODEX);
  await play(t);
  expect(storeOf(t).state().queued).toBe(CODEX);
  await press(cancelIn(row(CODEX)));
  await play(t);
  expect(setups(t)).toEqual([]);
  expect(row(CODEX)?.textContent).toContain(NOTE);
  expect(cancelIn(row(CODEX))).toBeNull();
});

test("the matter's composer: the other plan's Cancel shows the note under its compact line, and drops its Cancel; the conversation's plan's line is untouched", async () => {
  const t = await mountFirstRun({ first: DOWNLOADS });
  await press(also()?.querySelector<HTMLElement>("button") ?? null);
  await play(t);
  expect(setups(t)).toEqual([{ cancel_sign_in: { agent: CODEX } }]);
  expect(also()?.textContent).toContain("Getting ChatGPT ready… 90 MB of 139 MB");
  expect(also()?.querySelector('[data-testid="working-note"]')?.textContent).toBe(NOTE);
  expect(also()?.querySelector("button")).toBeNull();
  const first = document.querySelector(".room-entry .working");
  expect(first?.textContent).toContain("Getting Claude ready… 13 MB of 165 MB");
  expect(document.querySelectorAll('.room-entry [data-testid="working-note"]')).toHaveLength(1);
  await play(t, install(CODEX, READY));
  expect(document.querySelector('.room-entry [data-testid="working-note"]')).toBeNull();
});

test("the matter's gate: the conversation's plan's Cancel while it downloads puts the note under its line, and the gate offers Sign in again", async () => {
  const t = await mountFirstRun({
    first: [
      install(CLAUDE, { downloading: { received: 13_000_000, total: 165_000_000 } }),
      signIn(CLAUDE, "queued"),
    ],
  });
  await press(byId("signin-cancel"));
  await play(t);
  expect(setups(t)).toEqual([{ cancel_sign_in: { agent: CLAUDE } }]);
  expect(document.querySelector('.room-entry [data-testid="working-note"]')?.textContent).toBe(
    "Sign-in cancelled. Claude is still downloading; sign in when it's ready.",
  );
  expect(byId("signin-cancel")).toBeNull();
});
