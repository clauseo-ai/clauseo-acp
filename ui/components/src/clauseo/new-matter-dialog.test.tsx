// @vitest-environment happy-dom
// D-075, New matter's dialog: no button in it goes without a name, Base UI's focus guards included (on WebKit under
// macOS they are role="button" for VoiceOver), and focus going back to New matter draws its ring only when a key, not a
// press, closed the dialog.
import { act, useRef, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { NewMatterDialog } from "./new-matter-dialog.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
});

const frames = async () => {
  for (let i = 0; i < 3; i += 1)
    await act(async () => new Promise<void>((r) => requestAnimationFrame(() => r())));
};

/** New matter's button and its dialog, as the room has them. */
function Rail() {
  const [open, setOpen] = useState(false);
  const back = useRef<HTMLButtonElement>(null);
  return (
    <>
      <button ref={back} type="button" data-testid="new-matter" onClick={() => setOpen(true)}>
        New matter
      </button>
      <NewMatterDialog
        open={open}
        onOpenChange={setOpen}
        folder="Documents/Acme"
        pickFolder={() => null}
        onCreate={() => {}}
        finalFocus={back}
      />
    </>
  );
}

async function show() {
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<Rail />));
}

const newMatter = () => document.querySelector<HTMLButtonElement>("[data-testid=new-matter]")!;
const cancel = () =>
  [...document.querySelectorAll<HTMLButtonElement>("[role=dialog] button")].find(
    (b) => b.textContent?.trim() === "Cancel",
  )!;

/** A pointer's press on `el`: its pointerdown, then its click. */
async function press(el: HTMLElement) {
  await act(async () => {
    el.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    el.click();
  });
  await frames();
}

test("every button in the dialog has a name, the focus guards too, which take the dialog's title", async () => {
  await show();
  await press(newMatter());
  const guards = [...document.querySelectorAll<HTMLElement>("[data-base-ui-focus-guard]")];
  expect(guards.length).toBeGreaterThan(0);
  for (const guard of guards) {
    // as WebKit under macOS has them
    guard.setAttribute("role", "button");
    expect(guard.getAttribute("aria-label")).toBe("A new matter");
  }
  const plane = document.querySelector("[data-dialog-plane]")!.parentElement!;
  const unnamed = [...plane.querySelectorAll<HTMLElement>("button, [role=button]")].filter(
    (b) =>
      (b.getAttribute("aria-label") ?? "").trim() === "" &&
      !b.hasAttribute("aria-labelledby") &&
      (b.textContent ?? "").trim() === "",
  );
  expect(unnamed).toEqual([]);
});

test("closed by a press, focus goes back to New matter with no ring until a key is pressed", async () => {
  await show();
  await press(newMatter());
  expect(document.querySelector("[role=dialog]")).not.toBeNull();
  await press(cancel());
  expect(document.activeElement).toBe(newMatter());
  expect(newMatter().hasAttribute("data-quiet-focus")).toBe(true);
  await act(async () =>
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true })),
  );
  expect(newMatter().hasAttribute("data-quiet-focus")).toBe(false);
});

test("closed by Escape, focus goes back to New matter with its ring", async () => {
  await show();
  await press(newMatter());
  await act(async () =>
    document.activeElement?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    ),
  );
  await frames();
  expect(document.querySelector("[role=dialog]")).toBeNull();
  expect(document.activeElement).toBe(newMatter());
  expect(newMatter().hasAttribute("data-quiet-focus")).toBe(false);
});
