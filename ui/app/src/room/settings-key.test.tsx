// @vitest-environment happy-dom
// Settings from the room over FakeTransport: the rail's foot draws Settings only where the transport can open the
// settings window, its press and Ctrl+, (⌘, on the Mac) each ask the host once, never while an input method composes,
// and a refusal says nothing on screen.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { frameBatch } from "../store/batch.ts";
import { StoreContext } from "../store/hooks.ts";
import { createViewStore } from "../store/store.ts";
import { FakeTransport, type FakeHost } from "../transport/fake-transport.ts";
import { Room } from "./room.tsx";
import { isSettingsKey } from "./settings-key.ts";
import { connectRoom } from "./start.ts";
import { loadThread } from "./thread/chunk.ts";
import { stubFonts } from "./thread/testing.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

let host: HTMLDivElement | null = null;
let root: Root | null = null;

/** A host with no matters, on `platform`, with the settings window when `openSettings` is given. */
function fake(
  platform: NonNullable<FakeHost["platform"]>,
  openSettings?: FakeHost["openSettings"],
) {
  return new FakeTransport({
    platform,
    ...(openSettings === undefined ? {} : { openSettings }),
    query: (query) => {
      if (query === "list_matters") return { matters: [] };
      throw new Error("not scripted");
    },
  });
}

async function mount(transport: FakeTransport) {
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

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  vi.restoreAllMocks();
});

const settings = () =>
  host?.querySelector<HTMLButtonElement>('[data-testid="open-settings"]') ?? null;

/** Presses a key on the page as the webview would deliver it. */
async function press(init: KeyboardEventInit & { keyCode?: number }) {
  const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
  if (init.keyCode !== undefined) Object.defineProperty(event, "keyCode", { value: init.keyCode });
  await act(async () => void document.body.dispatchEvent(event));
  return event;
}

const settle = () => act(async () => new Promise((resolve) => setTimeout(resolve, 0)));

test("without the settings window (no openSettings) the rail draws no Settings and Ctrl+, does nothing", async () => {
  const transport = fake("windows");
  await mount(transport);
  expect(transport.openSettings).toBeUndefined();
  expect(settings()).toBeNull();
  expect(host?.querySelector(".room-settings")).toBeNull();
  const event = await press({ key: ",", code: "Comma", ctrlKey: true });
  expect(event.defaultPrevented).toBe(false);
});

test("the rail's Settings is a button that asks the host to open the settings window, once per press", async () => {
  const transport = fake("windows", () => {});
  await mount(transport);
  const button = settings();
  expect(button?.tagName).toBe("BUTTON");
  expect(button?.title).toBe("Ctrl+,");
  await act(async () => button?.click());
  await act(async () => button?.click());
  await settle();
  // the second press asks again: the host's open-or-focus brings the one window forward
  expect(transport.only("open_settings")).toHaveLength(2);
});

test("Ctrl+, off the Mac opens Settings; Ctrl+Shift+, and a composing key do not", async () => {
  const transport = fake("windows", () => {});
  await mount(transport);
  expect((await press({ key: ",", code: "Comma", ctrlKey: true })).defaultPrevented).toBe(true);
  await settle();
  expect(transport.only("open_settings")).toHaveLength(1);
  await press({ key: ",", code: "Comma", ctrlKey: true, isComposing: true });
  await press({ key: "Process", code: "Comma", ctrlKey: true, keyCode: 229 });
  await press({ key: "<", code: "Comma", ctrlKey: true, shiftKey: true });
  await press({ key: ",", code: "Comma", metaKey: true });
  await settle();
  expect(transport.only("open_settings")).toHaveLength(1);
});

test("on the Mac ⌘, opens Settings and Ctrl+, does not", async () => {
  const transport = fake("mac", () => {});
  await mount(transport);
  expect(settings()?.title).toBe("⌘,");
  await press({ key: ",", code: "Comma", ctrlKey: true });
  await settle();
  expect(transport.only("open_settings")).toHaveLength(0);
  await press({ key: ",", code: "Comma", metaKey: true });
  await settle();
  expect(transport.only("open_settings")).toHaveLength(1);
});

test("a refused open says nothing on screen and warns with the code, never as copy", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const transport = fake("linux", () => {
    throw { code: "internal", message: "window failed to build", retryable: false };
  });
  await mount(transport);
  const before = host?.textContent;
  await act(async () => settings()?.click());
  await settle();
  expect(transport.only("open_settings")).toHaveLength(1);
  expect(host?.textContent).toBe(before);
  expect(host?.textContent).not.toContain("window failed to build");
  expect(warn.mock.calls.some(([m]) => String(m).startsWith("open_settings failed:"))).toBe(true);
});

test("isSettingsKey takes the comma key's place where the layout gives no Latin character", () => {
  const key = (init: KeyboardEventInit) => new KeyboardEvent("keydown", init);
  expect(isSettingsKey(key({ key: "ب", code: "Comma", ctrlKey: true }), false)).toBe(true);
  expect(isSettingsKey(key({ key: "w", code: "Comma", ctrlKey: true }), false)).toBe(false);
  expect(isSettingsKey(key({ key: ",", code: "KeyM", ctrlKey: true }), false)).toBe(true);
  expect(isSettingsKey(key({ key: ",", code: "Comma", ctrlKey: true, altKey: true }), false)).toBe(
    false,
  );
});
