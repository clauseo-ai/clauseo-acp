// @vitest-environment happy-dom
// D-062: the room's commands from the Mac's native menu (crates/desktop/src/menu.rs sends `clauseo-menu` events) and
// their keys: New conversation (⌘N, as before), New matter (⇧⌘N, Ctrl+Shift+N elsewhere) and Find (⌘K's).
import { act } from "react";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, expect, test, vi } from "vitest";
import { Conversations } from "./conversations.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import { isCommandKey, MENU_EVENT } from "./room-commands.ts";
import { mountRoom, THE_MATTER, type Harness } from "./thread/harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  vi.restoreAllMocks();
});

const mount = async () => {
  h = await mountRoom({
    platform: "mac",
    next: fakeNext({ matters: () => [detailed(THE_MATTER)] }),
  });
  return h;
};

/** The native menu's command, as menu.rs sends it. */
const menu = async (command: string) => {
  await act(
    async () => void window.dispatchEvent(new CustomEvent(MENU_EVENT, { detail: command })),
  );
  await act(async () => settled());
  await h!.settle();
};

const key = async (init: KeyboardEventInit) => {
  const e = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
  await act(async () => void document.body.dispatchEvent(e));
  await act(async () => settled());
  await h!.settle();
  return e.defaultPrevented;
};

const dialog = () => document.querySelector('[role="dialog"]');

test("the menu's New conversation starts one, as ⌘N does", async () => {
  const start = vi.spyOn(Conversations.prototype, "startNew");
  await mount();
  await menu("new_conversation");
  expect(start).toHaveBeenCalledTimes(1);
  expect(await key({ key: "n", code: "KeyN", metaKey: true })).toBe(true);
  expect(start).toHaveBeenCalledTimes(2);
  expect(dialog()).toBeNull();
});

test("the menu's New matter and ⇧⌘N open New matter's dialog", async () => {
  await mount();
  await menu("new_matter");
  expect(dialog()).not.toBeNull();
  await h!.unmount();
  await mount();
  expect(await key({ key: "N", code: "KeyN", metaKey: true, shiftKey: true })).toBe(true);
  expect(dialog()).not.toBeNull();
});

test("the menu's Find puts the focus in the rail's Find", async () => {
  await mount();
  const find = document.querySelector('input[type="search"]');
  expect(find).not.toBeNull();
  await menu("find");
  expect(document.activeElement).toBe(find);
});

test("a command the room doesn't know does nothing", async () => {
  const start = vi.spyOn(Conversations.prototype, "startNew");
  await mount();
  await menu("quit");
  await menu("");
  expect(start).not.toHaveBeenCalled();
  expect(dialog()).toBeNull();
});

test("the keys: the platform's command key, Shift for New matter, the letter's place off the Latin layouts", () => {
  const k = (init: KeyboardEventInit) => new KeyboardEvent("keydown", init);
  expect(isCommandKey(k({ key: "n", metaKey: true }), "n", false, true)).toBe(true);
  expect(isCommandKey(k({ key: "N", metaKey: true, shiftKey: true }), "n", true, true)).toBe(true);
  expect(isCommandKey(k({ key: "N", ctrlKey: true, shiftKey: true }), "n", true, false)).toBe(true);
  expect(isCommandKey(k({ key: "न", code: "KeyN", ctrlKey: true }), "n", false, false)).toBe(true);
  expect(isCommandKey(k({ key: "n", metaKey: true, shiftKey: true }), "n", false, true)).toBe(
    false,
  );
  expect(isCommandKey(k({ key: "n", ctrlKey: true }), "n", false, true)).toBe(false);
  expect(isCommandKey(k({ key: "n", metaKey: true }), "n", false, false)).toBe(false);
  expect(isCommandKey(k({ key: "n", metaKey: true, altKey: true }), "n", false, true)).toBe(false);
  expect(isCommandKey(k({ key: "n", metaKey: true, isComposing: true }), "n", false, true)).toBe(
    false,
  );
});
