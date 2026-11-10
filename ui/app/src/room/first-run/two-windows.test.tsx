// D-012: a plan signed in from one window (Settings' Plans) is shown the same in the other (the main window), from the
// same setup state. Desktop sends each setup event to every window's feed; each window's store hears the same events,
// and the main window, which pressed nothing, shows ChatGPT ready and signed in, its draft runs ChatGPT, and no gate
// asks for Claude. Neither window sends a turn for it.
// @vitest-environment happy-dom
import { expect, test } from "vitest";
import { createSetupStore, setupStoreOf, type SetupSource } from "../../store/setup.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { SetupEvent } from "../../transport/protocol.ts";
import {
  AT,
  CLAUDE,
  CODEX,
  READY,
  SIGNED_IN,
  allById,
  byId,
  install,
  mountFirstRun,
  play,
  sent,
  signIn,
  text,
} from "./rig.tsx";

const BEFORE: SetupEvent[] = [
  install(CLAUDE, READY),
  signIn(CLAUDE, { failed: { code: "signin.cancelled", at: AT } }),
  install(CODEX, { missing: { download_bytes: 139_000_000 } }),
  signIn(CODEX, { signed_out: {} }),
];
/** What desktop sends every window while Settings installs ChatGPT and signs it in. */
const SETTINGS_SIGN_IN: SetupEvent[] = [
  signIn(CODEX, "queued"),
  install(CODEX, { downloading: { received: 20_000_000, total: 139_000_000 } }),
  install(CODEX, READY),
  signIn(CODEX, { waiting: { since: AT, nudge: false } }),
  signIn(CODEX, SIGNED_IN),
];

type Wired = HostTransport & SetupSource;

/** The Settings window: its own transport and store, over the same events. */
async function settingsWindow() {
  const transport = new FakeTransport({ setup: { first: () => BEFORE } });
  const store = createSetupStore(transport as unknown as Wired);
  store.start();
  // the replay of the first events comes after the subscribe
  await new Promise((resolve) => setTimeout(resolve, 0));
  return { transport, store };
}

const row = (agent: string) =>
  (allById("plan-row") as HTMLElement[]).find((r) => r.dataset["agent"] === agent) ?? null;

/** Both windows' sign-in and install state, as each store holds it. */
const picture = (store: ReturnType<typeof createSetupStore>) =>
  Object.fromEntries(
    Object.entries(store.state().agents).map(([id, a]) => [
      id,
      { install: a.install, signIn: a.signIn },
    ]),
  );

test("first screen: ChatGPT signed in from Settings shows in the main window, from the same state", async () => {
  const main = await mountFirstRun({ matters: [], first: BEFORE });
  const settings = await settingsWindow();
  await play(main, ...SETTINGS_SIGN_IN);
  for (const e of SETTINGS_SIGN_IN) settings.transport.setupEvent(e);
  expect(picture(setupStoreOf(main as unknown as Wired))).toEqual(picture(settings.store));
  // the main window says ChatGPT is ready, and Claude's cancelled sign-in as Settings does
  expect(row(CODEX)?.textContent).toContain("ChatGPT is ready");
  expect(row(CLAUDE)?.textContent).toContain("Sign-in was cancelled.");
  expect(text()).not.toContain("Claude is ready");
  expect(sent(main, "send_prompt")).toEqual([]);
});

test("a matter: the main window's new conversation follows ChatGPT, signed in elsewhere, and no gate asks for Claude", async () => {
  const main = await mountFirstRun({ first: BEFORE });
  expect(byId("signin")?.dataset["agent"]).toBe("claude");
  await play(main, ...SETTINGS_SIGN_IN);
  await play(main);
  expect(byId("signin")).toBeNull();
  expect(sent(main, "send_prompt")).toEqual([]);
  expect(sent(main, "create_thread")).toEqual([]);
});
