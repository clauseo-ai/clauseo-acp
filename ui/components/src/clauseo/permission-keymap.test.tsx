// @vitest-environment happy-dom
// D-065: the permission prompt by keyboard never grants more than the focused control says. The whole key map: Enter
// with nothing focused, the default (toned) button, Enter, Space and Cmd/Ctrl+Enter on each button, Escape, and focus
// when the prompt opens. The OS's own confirm for Always for this matter (its buttons, Cancel first and the default,
// Escape declining) is pinned in crates/desktop/src/dialogs/tests.rs.
import { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { TooltipProvider } from "../ui/tooltip.tsx";
import { PermissionPrompt } from "./permission.tsx";
import type { PermissionAnswer } from "./permission-keys.ts";
import { RoomScopeProvider } from "./room-scope-provider.tsx";
import { useRoom } from "./room-scope.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | undefined;
let root: ReturnType<typeof createRoot> | undefined;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = undefined;
  showDisplay(DEFAULT_DISPLAY);
  vi.restoreAllMocks();
});

type Props = Partial<ComponentProps<typeof PermissionPrompt>>;

function Composer() {
  const room = useRoom();
  return (
    <form data-composer="">
      <textarea ref={room.nodes.slot<HTMLTextAreaElement>("field")} />
    </form>
  );
}

const ALL: readonly PermissionAnswer[] = ["once", "always", "deny", "never"];

async function show(props: Props = {}) {
  showDisplay({ ...DEFAULT_DISPLAY, motion: "reduced", platform: "windows" });
  const onAnswer = vi.fn<(a: PermissionAnswer) => void>();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(
      <TooltipProvider>
        <RoomScopeProvider>
          <PermissionPrompt
            paper="Notice to the bank"
            state="pending"
            onAnswer={onAnswer}
            keys
            offered={ALL}
            {...props}
          />
          <Composer />
        </RoomScopeProvider>
      </TooltipProvider>,
    ),
  );
  return onAnswer;
}

const button = (a: PermissionAnswer) => {
  const b = host?.querySelector<HTMLButtonElement>(`.perm button[data-perm-answer="${a}"]`);
  if (b == null) throw new Error(`no ${a}`);
  return b;
};

/** A keydown on `on`; whether something prevented it. */
async function key(on: Element, k: string, init: KeyboardEventInit = {}): Promise<boolean> {
  const e = new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...init });
  await act(async () => {
    on.dispatchEvent(e);
  });
  return e.defaultPrevented;
}

/** How broad each answer is: what it allows, and for how long. */
const BREADTH: Record<PermissionAnswer, number> = { never: 0, deny: 0, once: 1, always: 2 };

test("on open, focus stays where it was: the prompt takes none, so no key lands on an answer unasked", async () => {
  await show();
  expect(host?.querySelector(".perm")?.contains(document.activeElement)).toBe(false);
});

test("Enter with nothing focused answers Allow once, never Always for this matter", async () => {
  const onAnswer = await show();
  expect(await key(document.body, "Enter")).toBe(true);
  expect(onAnswer.mock.calls).toEqual([["once"]]);
});

test("the default (toned) button is Allow once, the narrowest allow, and plain Enter's answer", async () => {
  await show();
  // the tone is drawn apart from the quiet three, which are drawn alike
  const quiet = button("deny").className;
  expect(button("once").className).not.toBe(quiet);
  expect([button("always").className, button("never").className]).toEqual([quiet, quiet]);
});

test("with no Allow once offered, Always is toned but plain Enter answers nothing", async () => {
  const onAnswer = await show({ offered: ["always", "never"] });
  expect(button("always").className).not.toBe(button("never").className);
  expect(await key(document.body, "Enter")).toBe(false);
  expect(onAnswer).not.toHaveBeenCalled();
});

test.each(ALL)(
  "on %s, plain Enter and Space are the button's own: its click answers it",
  async (a) => {
    const onAnswer = await show();
    button(a).focus();
    expect(await key(button(a), "Enter")).toBe(false);
    expect(await key(button(a), " ")).toBe(false);
    expect(onAnswer).not.toHaveBeenCalled();
    // the browser turns either into the button's click
    await act(async () => button(a).click());
    expect(onAnswer.mock.calls).toEqual([[a]]);
  },
);

test.each(ALL)(
  "on %s, Cmd or Ctrl with Enter gives that button's answer, once, and never a broader one",
  async (a) => {
    for (const mod of [{ ctrlKey: true }, { metaKey: true }]) {
      const onAnswer = await show();
      button(a).focus();
      // prevented: the key is handled here, so nothing else also answers it
      expect(await key(button(a), "Enter", mod)).toBe(true);
      for (const [given] of onAnswer.mock.calls)
        expect(BREADTH[given]).toBeLessThanOrEqual(BREADTH[a]);
      expect(onAnswer.mock.calls).toEqual([[a]]);
      await act(async () => root?.unmount());
      host?.remove();
    }
  },
);

test("on Allow once, Deny and Don't allow for this matter, Cmd or Ctrl with Enter never answers Always", async () => {
  for (const a of ["once", "deny", "never"] as const) {
    for (const mod of [{ ctrlKey: true }, { metaKey: true }]) {
      const onAnswer = await show();
      button(a).focus();
      await key(button(a), "Enter", mod);
      expect(onAnswer.mock.calls.map(([given]) => given)).not.toContain("always");
      await act(async () => root?.unmount());
      host?.remove();
    }
  }
});

test.each(ALL)("on %s, Escape answers Don't allow: it grants nothing", async (a) => {
  const onAnswer = await show();
  button(a).focus();
  expect(await key(button(a), "Escape")).toBe(true);
  expect(onAnswer.mock.calls).toEqual([["deny"]]);
});

test("Escape with nothing focused answers Don't allow", async () => {
  const onAnswer = await show();
  expect(await key(document.body, "Escape")).toBe(true);
  expect(onAnswer.mock.calls).toEqual([["deny"]]);
});

test("Cmd or Ctrl with Enter from the empty composer is Always's own shortcut, shown on its key cap", async () => {
  const onAnswer = await show();
  const field = host?.querySelector("textarea");
  if (field == null) throw new Error("no field");
  expect(button("always").getAttribute("aria-keyshortcuts")).toBe("Control+Enter");
  expect(await key(field, "Enter", { ctrlKey: true })).toBe(true);
  expect(onAnswer.mock.calls).toEqual([["always"]]);
});
