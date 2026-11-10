// @vitest-environment happy-dom
// D-058: while an answer runs, Escape and ⌘. (Ctrl+. off the Mac) stop it through the composer's own Stop; never while
// nothing runs, never for a key a menu, dialog or popover hears itself, never with another modifier, and never for an
// input method's composing key.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { WorkComposer } from "./work-composer.tsx";
import { isStopKey } from "./stop-key.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
if (!("fonts" in document)) {
  Object.defineProperty(document, "fonts", {
    value: { ready: Promise.resolve(), status: "loaded", load: async () => [] },
  });
}

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  document.body.replaceChildren();
});

async function composer(busy: boolean) {
  const onStop = vi.fn();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root!.render(
      <WorkComposer
        drafts={new Map()}
        draftKey={null}
        commands={[]}
        disabled={false}
        busy={busy}
        refused={null}
        failed={null}
        onAsk={async () => false}
        onStop={onStop}
      />,
    ),
  );
  return onStop;
}

const field = () => host!.querySelector<HTMLTextAreaElement>('[data-testid="composer"]')!;

const press = (target: Element, init: KeyboardEventInit) =>
  act(async () => {
    target.dispatchEvent(
      new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }),
    );
  });

test("Escape in the field and ⌘. anywhere stop a running answer, as Stop does", async () => {
  const onStop = await composer(true);
  // Stop stands where Ask stood: the press and the keys call the same stop
  host!.querySelector<HTMLButtonElement>('[aria-label="Stop the answer"]')?.click();
  expect(onStop).toHaveBeenCalledTimes(1);
  await press(field(), { key: "Escape" });
  expect(onStop).toHaveBeenCalledTimes(2);
  await press(document.body, { key: ".", code: "Period", metaKey: true });
  expect(onStop).toHaveBeenCalledTimes(3);
});

test("nothing runs: the keys stop nothing", async () => {
  const onStop = await composer(false);
  await press(field(), { key: "Escape" });
  await press(document.body, { key: ".", metaKey: true });
  expect(onStop).not.toHaveBeenCalled();
});

test("a menu, dialog or popover keeps its Escape, and a key already taken stops nothing", async () => {
  const onStop = await composer(true);
  // the focus inside a menu
  const menu = document.createElement("div");
  menu.setAttribute("role", "menu");
  const item = document.createElement("button");
  menu.append(item);
  document.body.append(menu);
  await press(item, { key: "Escape" });
  expect(onStop).not.toHaveBeenCalled();
  menu.remove();
  // a popover open elsewhere, the focus in the field
  const popover = document.createElement("div");
  popover.setAttribute("role", "dialog");
  popover.setAttribute("data-open", "");
  document.body.append(popover);
  await press(field(), { key: "Escape" });
  expect(onStop).not.toHaveBeenCalled();
  popover.remove();
  // the composer's `/` menu took it first
  field().addEventListener("keydown", (e) => e.preventDefault(), { once: true });
  await press(field(), { key: "Escape" });
  expect(onStop).not.toHaveBeenCalled();
  // and with nothing in the way, it stops
  await press(field(), { key: "Escape" });
  expect(onStop).toHaveBeenCalledTimes(1);
});

test("a prompt holding Escape keeps it, ⌘. still stops; a disabled or inert one holds nothing", async () => {
  const onStop = await composer(true);
  const deny = document.createElement("button");
  deny.setAttribute("aria-keyshortcuts", "Escape");
  document.body.append(deny);
  await press(field(), { key: "Escape" });
  expect(onStop).not.toHaveBeenCalled();
  await press(field(), { key: ".", metaKey: true });
  expect(onStop).toHaveBeenCalledTimes(1);
  deny.disabled = true;
  await press(field(), { key: "Escape" });
  expect(onStop).toHaveBeenCalledTimes(2);
  deny.disabled = false;
  const held = document.createElement("div");
  held.setAttribute("inert", "");
  held.append(deny);
  document.body.append(held);
  await press(field(), { key: "Escape" });
  expect(onStop).toHaveBeenCalledTimes(3);
});

test("Stop's keys: Escape alone, ⌘. on the Mac, Ctrl+. elsewhere; not composing, not with another modifier", () => {
  const key = (init: KeyboardEventInit) => new KeyboardEvent("keydown", init);
  expect(isStopKey(key({ key: "Escape" }), true)).toBe(true);
  expect(isStopKey(key({ key: "Escape" }), false)).toBe(true);
  expect(isStopKey(key({ key: ".", metaKey: true }), true)).toBe(true);
  expect(isStopKey(key({ key: ".", ctrlKey: true }), false)).toBe(true);
  // the period key's place where the layout gives another character there
  expect(isStopKey(key({ key: "।", code: "Period", ctrlKey: true }), false)).toBe(true);
  expect(isStopKey(key({ key: ".", ctrlKey: true }), true)).toBe(false);
  expect(isStopKey(key({ key: ".", metaKey: true }), false)).toBe(false);
  expect(isStopKey(key({ key: "." }), true)).toBe(false);
  expect(isStopKey(key({ key: "Escape", shiftKey: true }), true)).toBe(false);
  expect(isStopKey(key({ key: "Escape", metaKey: true }), true)).toBe(false);
  expect(isStopKey(key({ key: ".", metaKey: true, altKey: true }), true)).toBe(false);
  expect(isStopKey(key({ key: "Escape", isComposing: true }), true)).toBe(false);
});
