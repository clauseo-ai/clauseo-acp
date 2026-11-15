// installs-cancel: Cancel on a plan's waiting sign-in while it isn't installed yet stops its download too. The store
// sends cancel_agent_install (never cancel_sign_in), desktop drops the waiting sign-in and answers `missing`, and the
// row reads not installed again, offering its size and Sign in; the other plan's row is untouched. On the first screen,
// in the matter's composer and in the matter's gate alike.
// @vitest-environment happy-dom
import { expect, test } from "vitest";
import { setupStoreOf, type SetupSource } from "../../store/setup.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { FakeTransport } from "../../transport/fake-transport.ts";
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

const storeOf = (t: FakeTransport) => setupStoreOf(t as unknown as HostTransport & SetupSource);
const DOWNLOADS = [
  install(CLAUDE, { downloading: { received: 13_000_000, total: 165_000_000 } }),
  signIn(CLAUDE, "queued"),
  install(CODEX, { downloading: { received: 90_000_000, total: 139_000_000 } }),
  signIn(CODEX, "queued"),
];
/** What desktop says once it has stopped ChatGPT's download. */
const CODEX_GONE = [
  install(CODEX, { missing: { download_bytes: 139_000_000 } }),
  signIn(CODEX, { signed_out: {} }),
];
/** The first screen's plan row for `agent`. */
const row = (agent: string) =>
  (allById("plan-row") as HTMLElement[]).find((r) => r.dataset["agent"] === agent) ?? null;
const cancelIn = (el: Element | null) =>
  el?.querySelector<HTMLElement>('[data-testid="signin-foot-cancel"]') ?? null;
/** The composer's other line, compact under the first. */
const also = () => document.querySelector<HTMLElement>(".room-entry .working-also");

test("first screen: Cancel while ChatGPT downloads sends cancel_agent_install; on missing its row reads not installed with its Sign in; Claude's row is untouched", async () => {
  const t = await mountFirstRun({ matters: [], first: DOWNLOADS });
  await press(cancelIn(row(CODEX)));
  await play(t);
  expect(setups(t)).toEqual([{ cancel_agent_install: { agent: CODEX } }]);
  await play(t, ...CODEX_GONE);
  expect(row(CODEX)?.textContent).toContain("needs a download of about 140 MB");
  expect(row(CODEX)?.textContent).not.toContain("Getting ChatGPT ready");
  expect(row(CODEX)?.querySelector('[data-testid="install-sign-in"]')).not.toBeNull();
  expect(cancelIn(row(CODEX))).toBeNull();
  expect(row(CLAUDE)?.textContent).toContain("Getting Claude ready… 13 MB of 165 MB");
  expect(cancelIn(row(CLAUDE))).not.toBeNull();
});

test("Sign in again after the cancel: its sign-in goes, and desktop downloads it again", async () => {
  const t = await mountFirstRun({ matters: [], first: DOWNLOADS });
  await press(cancelIn(row(CODEX)));
  await play(t, ...CODEX_GONE);
  storeOf(t).signInTo(CODEX);
  await play(t);
  expect(setups(t).at(-1)).toEqual({ start_sign_in: { agent: CODEX } });
});

test("the store's own wait, cancelled while the plan downloads: the wait clears and cancel_agent_install goes", async () => {
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
  expect(storeOf(t).state().queued).toBeNull();
  expect(setups(t)).toEqual([{ cancel_agent_install: { agent: CODEX } }]);
  await play(t, install(CODEX, { missing: { download_bytes: 139_000_000 } }));
  expect(row(CODEX)?.textContent).toContain("needs a download of about 140 MB");
  // Claude's sign-in ending starts nothing for ChatGPT
  await play(t, signIn(CLAUDE, { signed_out: {} }));
  expect(setups(t)).toEqual([{ cancel_agent_install: { agent: CODEX } }]);
});

test("the matter's composer: the other plan's Cancel sends cancel_agent_install; on missing its compact line goes, and the conversation's plan's line is untouched", async () => {
  const t = await mountFirstRun({ first: DOWNLOADS });
  await press(also()?.querySelector<HTMLElement>("button") ?? null);
  await play(t);
  expect(setups(t)).toEqual([{ cancel_agent_install: { agent: CODEX } }]);
  await play(t, ...CODEX_GONE);
  expect(also()).toBeNull();
  const first = document.querySelector(".room-entry .working");
  expect(first?.textContent).toContain("Getting Claude ready… 13 MB of 165 MB");
});

test("the matter's gate: the conversation's plan's Cancel while it downloads sends cancel_agent_install, and on missing the gate offers its sign-in again", async () => {
  const t = await mountFirstRun({
    first: [
      install(CLAUDE, { downloading: { received: 13_000_000, total: 165_000_000 } }),
      signIn(CLAUDE, "queued"),
    ],
  });
  await press(byId("signin-cancel"));
  await play(t);
  expect(setups(t)).toEqual([{ cancel_agent_install: { agent: CLAUDE } }]);
  await play(
    t,
    install(CLAUDE, { missing: { download_bytes: 165_000_000 } }),
    signIn(CLAUDE, { signed_out: {} }),
  );
  expect(byId("signin-cancel")).toBeNull();
  expect(byId("signin-state")?.dataset["state"]).not.toBe("queued");
});
