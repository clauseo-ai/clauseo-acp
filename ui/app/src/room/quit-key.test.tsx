// @vitest-environment happy-dom
// Quit's key over FakeTransport: plain Ctrl+Q on Windows asks the host to quit once, in the room and in the settings
// window; Ctrl+Alt+Q (AltGr+Q, "@" on German layouts), AltGraph, Ctrl+Shift+Q, a composing key and a held key's
// repeats do not; on the Mac and on Linux the page leaves the key alone.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { Display } from "@clauseo/components/lib/display.ts";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, expect, test, vi } from "vitest";
import { connectDisplay } from "../display/connect.ts";
import type { DisplayStore } from "./display-store.ts";
import { SettingsApp } from "../settings/settings-app.tsx";
import { frameBatch } from "../store/batch.ts";
import { StoreContext } from "../store/hooks.ts";
import { createViewStore } from "../store/store.ts";
import { FakeTransport, type FakeHost } from "../transport/fake-transport.ts";
import { isQuitKey } from "./quit-key.ts";
import { Room } from "./room.tsx";
import { connectRoom } from "./start.ts";
import { loadThread } from "./thread/chunk.ts";
import { stubFonts } from "./thread/testing.ts";

vi.mock("../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

/** The page's localStorage: Node's own global shadows happy-dom's, so the test brings one. */
const items = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (key: string) => items.get(key) ?? null,
  setItem: (key: string, value: string) => void items.set(key, value),
  removeItem: (key: string) => void items.delete(key),
  clear: () => items.clear(),
});

let host: HTMLDivElement | null = null;
let root: Root | null = null;
let store: DisplayStore | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  store?.dispose();
  root = null;
  host = null;
  store = null;
  items.clear();
  vi.restoreAllMocks();
});

type Init = KeyboardEventInit & { keyCode?: number; altGraph?: boolean };

/** A keydown as the webview would deliver it; `altGraph` sets the AltGraph modifier state. */
function keydown(init: Init): KeyboardEvent {
  const { keyCode, altGraph, ...rest } = init;
  const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...rest });
  if (keyCode !== undefined) Object.defineProperty(event, "keyCode", { value: keyCode });
  const own = event.getModifierState.bind(event);
  Object.defineProperty(event, "getModifierState", {
    value: (key: string) => (key === "AltGraph" ? altGraph === true : own(key)),
  });
  return event;
}

async function press(init: Init) {
  const event = keydown(init);
  await act(async () => void document.body.dispatchEvent(event));
  return event;
}

const settle = () => act(async () => new Promise((resolve) => setTimeout(resolve, 0)));

const CTRL_Q = { key: "q", code: "KeyQ", ctrlKey: true } as const;

/** The keys that must never quit. */
const NOT_QUIT: Init[] = [
  { key: "@", code: "KeyQ", ctrlKey: true, altKey: true },
  { key: "@", code: "KeyQ", ctrlKey: true, altGraph: true },
  { key: "Q", code: "KeyQ", ctrlKey: true, shiftKey: true },
  { key: "q", code: "KeyQ", ctrlKey: true, metaKey: true },
  { key: "q", code: "KeyQ", ctrlKey: true, isComposing: true },
  { key: "Process", code: "KeyQ", ctrlKey: true, keyCode: 229 },
  { key: "q", code: "KeyQ", ctrlKey: true, repeat: true },
  { key: "q", code: "KeyQ" },
  // AZERTY's Ctrl+A: the key at the Q place types "a", so it is not a quit
  { key: "a", code: "KeyQ", ctrlKey: true },
];

test("isQuitKey takes plain Ctrl+Q on Windows only", () => {
  expect(isQuitKey(keydown(CTRL_Q), "windows")).toBe(true);
  // a layout with no Latin Q on the key still quits
  expect(isQuitKey(keydown({ key: "ौ", code: "KeyQ", ctrlKey: true }), "windows")).toBe(true);
  for (const init of NOT_QUIT) expect(isQuitKey(keydown(init), "windows")).toBe(false);
  expect(isQuitKey(keydown(CTRL_Q), "mac")).toBe(false);
  expect(isQuitKey(keydown({ key: "q", code: "KeyQ", metaKey: true }), "mac")).toBe(false);
  expect(isQuitKey(keydown(CTRL_Q), "linux")).toBe(false);
});

/** Each case: the layout, what Ctrl+the key types, and whether it quits on Windows. */
const LAYOUT_CASES: [string, Init, Display["platform"], boolean][] = [
  ["QWERTY Ctrl+Q", { key: "q", code: "KeyQ", ctrlKey: true }, "windows", true],
  ["Caps Lock Ctrl+Q", { key: "Q", code: "KeyQ", ctrlKey: true }, "windows", true],
  ["AZERTY Ctrl+Q", { key: "q", code: "KeyA", ctrlKey: true }, "windows", true],
  ["AZERTY Ctrl+A", { key: "a", code: "KeyQ", ctrlKey: true }, "windows", false],
  ["QWERTZ Ctrl+Q", { key: "q", code: "KeyQ", ctrlKey: true }, "windows", true],
  ["Dvorak Ctrl+' at the Q place", { key: "'", code: "KeyQ", ctrlKey: true }, "windows", false],
  ["Russian Ctrl+й at the Q place", { key: "й", code: "KeyQ", ctrlKey: true }, "windows", true],
  ["Hindi Ctrl+ौ at the Q place", { key: "ौ", code: "KeyQ", ctrlKey: true }, "windows", true],
  ["no scan code, Ctrl+q", { key: "q", code: "", ctrlKey: true }, "windows", true],
  ["Ctrl+Alt+Q", { key: "q", code: "KeyQ", ctrlKey: true, altKey: true }, "windows", false],
  [
    "AltGr+Q (types @)",
    { key: "@", code: "KeyQ", ctrlKey: true, altGraph: true },
    "windows",
    false,
  ],
  ["macOS Ctrl+Q", { key: "q", code: "KeyQ", ctrlKey: true }, "mac", false],
];

test.each(LAYOUT_CASES)("isQuitKey by the letter typed: %s", (_name, init, platform, quits) => {
  expect(isQuitKey(keydown(init), platform)).toBe(quits);
});

/** A host with no matters on `platform`, counting its quits. */
function fake(platform: NonNullable<FakeHost["platform"]>) {
  return new FakeTransport({
    platform,
    query: (query) => {
      if (query === "list_matters") return { matters: [] };
      throw new Error("not scripted");
    },
  });
}

async function mountRoom(transport: FakeTransport) {
  connectRoom(transport);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(
      <StoreContext value={createViewStore(transport, { batch: () => frameBatch })}>
        <Room transport={transport} windowControls={null} />
      </StoreContext>,
    ),
  );
  // list_matters, and display_info's platform
  await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
  await act(async () => void (await loadThread()));
}

async function mountSettings(transport: FakeTransport, platform: "mac" | "windows" | "linux") {
  const made = connectDisplay(transport).store;
  store = made;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(<SettingsApp store={made} transport={transport} windowControls={null} />),
  );
  await act(async () => settled());
  await act(async () => made.setPlatform(platform));
}

const pages = [
  ["the room", (t: FakeTransport) => mountRoom(t)],
  ["the settings window", (t: FakeTransport) => mountSettings(t, t.host.platform ?? "linux")],
] as const;

for (const [page, mount] of pages) {
  test(`Ctrl+Q on Windows quits once from ${page}; AltGr, Ctrl+Alt+Q and Ctrl+Shift+Q do not`, async () => {
    const transport = fake("windows");
    await mount(transport);
    for (const init of NOT_QUIT) await press(init);
    await settle();
    expect(transport.only("quit_app")).toHaveLength(0);
    expect((await press(CTRL_Q)).defaultPrevented).toBe(true);
    await settle();
    expect(transport.only("quit_app")).toHaveLength(1);
  });

  test(`on the Mac and on Linux ${page} leaves Ctrl+Q and ⌘Q alone`, async () => {
    for (const platform of ["mac", "linux"] as const) {
      const transport = fake(platform);
      await mount(transport);
      expect((await press(CTRL_Q)).defaultPrevented).toBe(false);
      expect((await press({ key: "q", code: "KeyQ", metaKey: true })).defaultPrevented).toBe(false);
      await settle();
      expect(transport.only("quit_app")).toHaveLength(0);
      await act(async () => root?.unmount());
      host?.remove();
      store?.dispose();
      store = null;
    }
  });
}
