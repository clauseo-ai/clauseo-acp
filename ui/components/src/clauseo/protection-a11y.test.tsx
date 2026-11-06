// @vitest-environment happy-dom
// UC's keyboard and screen-reader pass: the "Unknown publisher" confirm takes focus on Cancel, Escape closes it
// without sending and gives focus back to the press that opened it, also from inside the Not protected note; the note's
// pool counts are a list, its waiting line a status; the pressable mark says it opens a dialog.
import { act, StrictMode, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { NotProtectedMark } from "./not-protected.tsx";
import { NotProtectedNote } from "./protection.tsx";
import { ProtectionConfirm, type ProtectionConfirmAct } from "./protection-confirm.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: ReturnType<typeof createRoot> | null = null;
const tick = () => act(async () => new Promise<void>((r) => setTimeout(r, 80)));

async function show(node: React.ReactNode) {
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<StrictMode>{node}</StrictMode>));
  await tick();
}

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
});

const byId = (id: string) => document.querySelector<HTMLElement>(`[data-testid="${id}"]`);
const press = async (el: HTMLElement | null) => {
  await act(async () => el?.click());
  await tick();
};
const escape = async () => {
  await act(async () => {
    document.activeElement?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    );
  });
  await tick();
};

function Opener({ act: which, onConfirm }: { act: ProtectionConfirmAct; onConfirm: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" data-testid="opener" onClick={() => setOpen(true)}>
        Open
      </button>
      <ProtectionConfirm act={which} open={open} onOpenChange={setOpen} onConfirm={onConfirm} />
    </>
  );
}

for (const which of ["setup", "remove"] as const)
  test(`the ${which} confirm starts on Cancel, and Escape sends nothing and gives focus back`, async () => {
    const onConfirm = vi.fn();
    await show(<Opener act={which} onConfirm={onConfirm} />);
    byId("opener")?.focus();
    await press(byId("opener"));
    expect(byId("protection-confirm")).not.toBeNull();
    expect(document.activeElement).toBe(byId("protection-confirm-cancel"));
    await escape();
    expect(byId("protection-confirm")).toBeNull();
    expect(onConfirm).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(byId("opener"));
  });

test("Cancel sends nothing and gives focus back", async () => {
  const onConfirm = vi.fn();
  await show(<Opener act="setup" onConfirm={onConfirm} />);
  byId("opener")?.focus();
  await press(byId("opener"));
  await press(byId("protection-confirm-cancel"));
  expect(byId("protection-confirm")).toBeNull();
  expect(onConfirm).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(byId("opener"));
});

function NoteWithConfirm({ onConfirm }: { onConfirm: () => void }) {
  const [confirming, setConfirming] = useState(false);
  return (
    <>
      <NotProtectedNote
        open
        matter="Mehta v. Rao"
        why="needs_set_up"
        onClose={() => {}}
        onProtect={() => setConfirming(true)}
      />
      <ProtectionConfirm
        act="setup"
        open={confirming}
        onOpenChange={setConfirming}
        onConfirm={onConfirm}
      />
    </>
  );
}

test("from the note, Escape closes the confirm alone and focus goes back to Set up protection", async () => {
  const onConfirm = vi.fn();
  await show(<NoteWithConfirm onConfirm={onConfirm} />);
  byId("protection-setup")?.focus();
  await press(byId("protection-setup"));
  expect(document.activeElement).toBe(byId("protection-confirm-cancel"));
  await escape();
  expect(byId("protection-confirm")).toBeNull();
  expect(byId("np-note")).not.toBeNull();
  expect(onConfirm).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(byId("protection-setup"));
});

test("the note's Escape closes it", async () => {
  const onClose = vi.fn();
  await show(<NotProtectedNote open matter="Mehta v. Rao" why="off" onClose={onClose} />);
  expect(byId("np-note")?.contains(document.activeElement)).toBe(true);
  await escape();
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("a full pool's counts are a list, and the waiting line is a status", async () => {
  await show(
    <NotProtectedNote
      open
      matter="Mehta v. Rao"
      why="pool"
      pool={{ size: 8, open: 3, held: 2, owed: 0, set_aside: 1 }}
      addRoom
      waiting="Choose Yes in the window Windows shows."
      onClose={() => {}}
      onProtect={() => {}}
    />,
  );
  const counts = byId("np-pool");
  expect(counts?.getAttribute("role")).toBe("list");
  const items = [...(counts?.children ?? [])];
  expect(items).toHaveLength(3);
  expect(items.every((item) => item.getAttribute("role") === "listitem")).toBe(true);
  expect(byId("protection-wait")?.getAttribute("role")).toBe("status");
  // the lock in the title is drawn, not said
  expect(byId("np-note")?.querySelector("svg")?.getAttribute("aria-hidden")).toBe("true");
});

test("the pressable mark says it opens a dialog; the mark alone is no control", async () => {
  await show(
    <>
      <NotProtectedMark short onPress={() => {}} />
      <NotProtectedMark />
    </>,
  );
  const marks = document.querySelectorAll(".np-mark");
  expect(marks[0]?.tagName).toBe("BUTTON");
  expect(marks[0]?.getAttribute("aria-haspopup")).toBe("dialog");
  expect(marks[0]?.textContent).toBe("Not protected");
  expect(marks[1]?.tagName).toBe("SPAN");
  expect(marks[1]?.hasAttribute("aria-haspopup")).toBe(false);
  for (const mark of marks)
    expect(mark.querySelector("svg")?.getAttribute("aria-hidden")).toBe("true");
});
