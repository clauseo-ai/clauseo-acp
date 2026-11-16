// @vitest-environment happy-dom
// R23's Updates in the settings window (settings.html#updates), over FakeTransport with the updater scripted
// (fake-update.ts): the version and the last check; each state's words; Check now from idle and failed only, sending
// check_for_update; the mode choice sending set_update_mode, locked by policy (the locked key, or the host's
// settings.locked) with "Managed by firm"; every managed kind's line with no choice and no Check now; no section
// without the updater; and the window never sends install_update or cancel_scheduled_update.
import { settled } from "@clauseo/components/lib/late.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { st } from "@clauseo/components/lib/words-settings.ts";
import { uw } from "@clauseo/components/lib/words-update.ts";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { connectDisplay } from "../../display/connect.ts";
import { DEFAULT_UI, type DisplayStore } from "../../room/display-store.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import {
  fakeUpdate,
  isUpdateCommand,
  type FakeUpdateRefusals,
} from "../../transport/fake-update.ts";
import type { ManagedBy, UpdateState, UpdateView } from "../../transport/protocol.ts";
import { SettingsApp } from "../settings-app.tsx";
import { MANAGED_WORDS } from "./updates-words.ts";

let host: HTMLDivElement | null = null;
let root: Root | null = null;
let store: DisplayStore | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  store?.dispose();
  host = null;
  root = null;
  store = null;
  document.body.replaceChildren();
});

async function settle() {
  for (let i = 0; i < 4; i += 1) {
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
    await act(async () => settled());
  }
}

const CHECKED = new Date(2026, 10, 8, 16, 31).getTime();

const view = (state: UpdateState, over: Partial<UpdateView> = {}): UpdateView => ({
  current: "0.0.1",
  state,
  mode: "auto",
  ...over,
});

/** The settings window at Updates over an updater at `shown` (none: no updater), with `locked` locked. */
async function mount(
  shown: UpdateView | undefined,
  { locked = [], refuse }: { locked?: string[]; refuse?: FakeUpdateRefusals } = {},
) {
  const settings = { ui: { ...DEFAULT_UI }, locked };
  const transport = new FakeTransport({
    query: (query) => {
      if (query === "get_settings") return { settings: settings as never };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    next: { query: () => ({ settings }) as never },
    ...(shown === undefined
      ? {}
      : { update: fakeUpdate({ view: shown, ...(refuse === undefined ? {} : { refuse }) }) }),
  });
  history.replaceState(null, "", "/settings.html#updates");
  store = connectDisplay(transport).store;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  const made = store;
  await act(async () =>
    root?.render(<SettingsApp store={made} transport={transport} windowControls={null} />),
  );
  await settle();
  return transport;
}

const one = (id: string) => document.querySelector<HTMLElement>(`[data-testid="${id}"]`);
const check = () => one("update-check") as HTMLButtonElement | null;
const standing = () => one("update-standing")?.textContent ?? "";
const radios = () => [...document.querySelectorAll<HTMLElement>('[role="radio"]')];
/** The update commands sent, as their JSON (the window also checks the connectors as it opens). */
const sent = (transport: FakeTransport) =>
  transport
    .only("dispatch")
    .filter((c) => isUpdateCommand(c.command.command))
    .map((c) => JSON.stringify(c.command.command));
async function press(el: HTMLElement | null | undefined) {
  if (el == null) throw new Error("nothing to press");
  await act(async () => el.click());
  await settle();
}

test("the section is named Updates, and shows only where the transport carries the updater", async () => {
  await mount(view("idle"));
  expect(document.body.textContent).toContain(uw("en", "Updates"));
  expect(one("update-version")?.textContent).toBe("Clauseo 0.0.1");
  await act(async () => root?.unmount());
  document.body.replaceChildren();
  await mount(undefined);
  expect(one("update-version")).toBeNull();
  expect(document.body.textContent).not.toContain(uw("en", "Check now"));
});

test("up to date after a check: the last check's time, and Check now sends check_for_update", async () => {
  const transport = await mount(view("idle", { checked_at: CHECKED }));
  expect(one("update-checked")?.textContent).toMatch(/^Last checked .*4:31 pm$/);
  expect(standing()).toBe(uw("en", "Clauseo is up to date."));
  expect(check()?.disabled).toBe(false);
  await press(check());
  expect(sent(transport)).toEqual([JSON.stringify({ check_for_update: {} })]);
});

test("idle before any check: no last-checked line and no up-to-date line", async () => {
  await mount(view("idle"));
  expect(one("update-checked")).toBeNull();
  expect(standing()).toBe("");
});

test.each<[string, UpdateState, string, boolean]>([
  ["checking", "checking", "Checking for updates…", false],
  [
    "downloading",
    { downloading: { version: "0.0.2", received: 5 } },
    "Downloading the update…",
    false,
  ],
  [
    "ready",
    { ready: { version: "0.0.2" } },
    "An update is ready. Restart to update from the main window.",
    false,
  ],
  [
    "scheduled",
    { scheduled: { version: "0.0.2" } },
    "An update is ready. Restart to update from the main window.",
    false,
  ],
  [
    "a failed check",
    { failed: { at: 1, stage: "check" } },
    "Clauseo couldn't check for updates. Try again.",
    true,
  ],
  [
    "a failed download",
    { failed: { at: 1, stage: "download" } },
    "Clauseo couldn't check for updates. Try again.",
    true,
  ],
])(
  "%s: its words, and Check now enabled only from idle or failed",
  async (_, state, words, enabled) => {
    await mount(view(state));
    expect(standing()).toBe(uw("en", words as never));
    expect(check()?.disabled).toBe(!enabled);
  },
);

test("a failed install says update.failed's words", async () => {
  await mount(view({ failed: { at: 1, stage: "install", version: "0.0.2" } }));
  expect(standing()).toBe(errorWords("en", "update.failed"));
  expect(check()?.disabled).toBe(false);
});

test("the mode: both choices, the one in force checked, and a choice sends set_update_mode", async () => {
  const transport = await mount(view("idle"));
  const [auto, notify] = radios();
  expect(auto?.parentElement?.parentElement?.textContent).toContain(
    uw("en", "Install updates when Clauseo next opens"),
  );
  expect(notify?.parentElement?.parentElement?.textContent).toContain(uw("en", "Tell me first"));
  expect(auto?.getAttribute("aria-checked")).toBe("true");
  await press(notify);
  expect(sent(transport)).toEqual([JSON.stringify({ set_update_mode: { mode: "notify" } })]);
  expect(radios()[1]?.getAttribute("aria-checked")).toBe("true");
});

test("a mode locked by policy is greyed with Managed by firm, and sends nothing", async () => {
  const transport = await mount(view("idle", { mode: "notify" }), { locked: ["update.mode"] });
  expect(one("managed")?.textContent).toBe(st("en", "Managed by firm"));
  expect(radios()[1]?.getAttribute("aria-checked")).toBe("true");
  await press(radios()[0]);
  expect(sent(transport)).toEqual([]);
});

test("the host's settings.locked greys the choice with Managed by firm", async () => {
  const transport = await mount(view("idle"), { refuse: { set_update_mode: "settings.locked" } });
  expect(one("managed")).toBeNull();
  await press(radios()[1]);
  expect(sent(transport)).toHaveLength(1);
  expect(one("managed")?.textContent).toBe(st("en", "Managed by firm"));
  expect(radios()[0]?.getAttribute("aria-checked")).toBe("true");
  expect(one("update-refused")).toBeNull();
});

test("a refused check says its code's words", async () => {
  await mount(view("idle"), { refuse: { check_for_update: "command.invalid" } });
  await press(check());
  expect(one("update-refused")?.textContent).toBe(errorWords("en", "command.invalid"));
});

test.each(Object.keys(MANAGED_WORDS) as ManagedBy[])(
  "managed by %s: who manages updates, with no mode choice and no Check now",
  async (by) => {
    await mount(view({ managed: { by } }, { mode: by === "policy" ? "none" : "auto" }));
    expect(standing()).toBe(uw("en", MANAGED_WORDS[by]));
    expect(check()).toBeNull();
    expect(radios()).toEqual([]);
  },
);

test("the settings window never sends install_update or cancel_scheduled_update", async () => {
  const transport = await mount(view({ ready: { version: "0.0.2" } }));
  for (const b of document.querySelectorAll<HTMLButtonElement>("button")) {
    if (b.disabled) continue;
    if (b.closest('[data-testid="settings-updates"], .set-group') === null) continue;
    await press(b);
  }
  const names = transport.only("dispatch").map((c) => Object.keys(c.command.command)[0]);
  expect(names).not.toContain("install_update");
  expect(names).not.toContain("cancel_scheduled_update");
  expect([...document.querySelectorAll("button")].map((b) => b.textContent)).not.toContain(
    uw("en", "Restart to update"),
  );
});
