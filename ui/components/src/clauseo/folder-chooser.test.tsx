// @vitest-environment happy-dom
// Clauseo's own folder chooser, the part: its roles and words, the list's keys (arrows, Enter, Backspace, Escape),
// refused folders that don't open, the line for folders not shown, the views of a folder that didn't answer, isn't
// there or can't be used, the typed location, Choose this folder and its wait, and focus kept inside.
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import {
  FolderChooser,
  type ChooserGo,
  type ChooserListing,
  type ChooserStartPoint,
  type FolderChooserProps,
} from "./folder-chooser.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

async function mount(node: ReactNode) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(node));
  await act(async () => new Promise((done) => setTimeout(done, 0)));
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  for (const n of document.body.querySelectorAll("[data-base-ui-portal], [role=dialog]"))
    n.remove();
});

const STARTS: ChooserStartPoint[] = [
  { key: 0, kind: "drive", display: "C:" },
  { key: 1, kind: "home", display: "" },
  { key: 2, kind: "desktop", display: "Desktop" },
  { key: 3, kind: "last_pick", display: "Documents\\Matters" },
];

const MATTERS: ChooserListing = {
  display: "Documents\\Matters",
  crumbs: ["", "Documents", "Matters"],
  state: {
    kind: "folders",
    items: [
      { name: "Clauseo", refused: true },
      { name: "Iyer v. Iyer", refused: false },
      { name: "Mehta v. Mehta", refused: false },
    ],
    more: false,
  },
};

const ROOT: ChooserListing = {
  display: "C:\\",
  crumbs: ["C:"],
  state: { kind: "folders", items: [{ name: "Users", refused: false }], more: true },
};

function chooser(props: Partial<FolderChooserProps> = {}) {
  const calls = { go: [] as ChooserGo[], choose: 0, cancel: 0 };
  const node = (
    <FolderChooser
      open
      starts={STARTS}
      listing={MATTERS}
      onGo={(go) => calls.go.push(go)}
      onChoose={() => (calls.choose += 1)}
      onCancel={() => (calls.cancel += 1)}
      {...props}
    />
  );
  return { node, calls };
}

const q = <T extends Element = HTMLElement>(id: string) =>
  document.querySelector<T & HTMLElement>(`[data-testid="${id}"]`);
const all = (id: string) => [...document.querySelectorAll<HTMLElement>(`[data-testid="${id}"]`)];
const key = async (el: Element | null, k: string, init: KeyboardEventInit = {}) => {
  await act(async () => {
    el?.dispatchEvent(
      new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...init }),
    );
  });
};
const click = async (el: Element | null) => {
  await act(async () => {
    el?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, cancelable: true }));
    (el as HTMLElement | null)?.click();
  });
};
const selected = () => q("chooser-list")?.getAttribute("aria-activedescendant");
const option = (i: number) => all("chooser-item")[i]?.id;

test("a dialog with its title, why it's here, the start points, where it is, and the folders as a listbox", async () => {
  const { node } = chooser();
  await mount(node);
  const dialog = document.querySelector('[role="dialog"]');
  expect(dialog?.textContent).toContain("Choose the matter's folder");
  expect(dialog?.textContent).toContain(
    "Windows' folder window is slow to open on this computer, so Clauseo shows its own.",
  );
  expect(all("chooser-start").map((b) => b.textContent)).toEqual([
    "C:",
    "Home",
    "Desktop",
    "Last place",
  ]);
  expect(q("chooser-where")?.textContent).toBe("Home›Documents›Matters");
  const list = q("chooser-list");
  expect(list?.getAttribute("role")).toBe("listbox");
  expect(all("chooser-item").map((o) => o.getAttribute("role"))).toEqual([
    "option",
    "option",
    "option",
  ]);
  // focus starts on the list, the first folder highlighted
  expect(document.activeElement).toBe(list);
  expect(selected()).toBe(option(0));
  expect(q("chooser-heard")?.getAttribute("role")).toBe("status");
  expect(q("chooser-heard")?.textContent).toBe("Home, Documents, Matters");
});

test("before the desktop has read anything it says Looking… for the places and the folders, quietly, then fills in", async () => {
  const { node, calls } = chooser({ starts: null, listing: null, busy: true });
  await mount(node);
  expect(document.querySelector('[role="dialog"]')).not.toBeNull();
  expect(q("chooser-starts-looking")?.textContent).toBe("Looking…");
  expect(q("chooser-looking")?.textContent).toBe("Looking…");
  expect(all("chooser-start")).toHaveLength(0);
  expect(all("chooser-item")).toHaveLength(0);
  expect(q("chooser-state")).toBeNull();
  expect(q<HTMLButtonElement>("chooser-up")?.disabled).toBe(true);
  expect(q<HTMLButtonElement>("chooser-choose")?.disabled).toBe(true);
  expect(q("chooser-heard")?.textContent).toBe("Looking…");
  // the places land first; the folders still say Looking…
  await act(async () => root?.render(chooser({ listing: null, busy: true }).node));
  expect(q("chooser-starts-looking")).toBeNull();
  expect(all("chooser-start")).toHaveLength(4);
  expect(q("chooser-looking")?.textContent).toBe("Looking…");
  // then the folders
  await act(async () => root?.render(chooser().node));
  expect(q("chooser-looking")).toBeNull();
  expect(all("chooser-item")).toHaveLength(3);
  expect(q("chooser-heard")?.textContent).toBe("Home, Documents, Matters");
  expect(calls.go).toEqual([]);
});

test("arrows move, Enter opens the folder by its index, Backspace goes up", async () => {
  const { node, calls } = chooser();
  await mount(node);
  const list = q("chooser-list");
  await key(list, "ArrowDown");
  expect(selected()).toBe(option(1));
  await key(list, "ArrowDown");
  await key(list, "ArrowDown");
  expect(selected()).toBe(option(2));
  await key(list, "ArrowUp");
  expect(selected()).toBe(option(1));
  await key(list, "Enter");
  expect(calls.go).toEqual([{ kind: "into", index: 1 }]);
  await key(list, "Backspace");
  expect(calls.go.at(-1)).toEqual({ kind: "up" });
  await key(list, "End");
  expect(selected()).toBe(option(2));
  await key(list, "Home");
  expect(selected()).toBe(option(0));
});

test("a refused folder shows greyed with why, and neither Enter nor a press opens it", async () => {
  const { node, calls } = chooser();
  await mount(node);
  const refused = all("chooser-item")[0];
  expect(refused?.getAttribute("aria-disabled")).toBe("true");
  expect(refused?.textContent).toContain("Can't be a matter");
  expect(all("chooser-item")[1]?.getAttribute("aria-disabled")).toBeNull();
  await key(q("chooser-list"), "Enter");
  await click(refused ?? null);
  expect(calls.go).toEqual([]);
  // a press on another opens it
  await click(all("chooser-item")[2] ?? null);
  expect(calls.go).toEqual([{ kind: "into", index: 2 }]);
});

test("Up and the start points go where they say; at a drive's root Up and Backspace stay", async () => {
  const { node, calls } = chooser();
  await mount(node);
  await click(q("chooser-up"));
  await click(all("chooser-start")[3] ?? null);
  expect(calls.go).toEqual([{ kind: "up" }, { kind: "start", key: 3 }]);
  await act(async () => root?.render(chooser({ listing: ROOT }).node));
  expect(q<HTMLButtonElement>("chooser-up")?.disabled).toBe(true);
});

test("a typed location goes with Go or Enter; Backspace in the field edits it", async () => {
  const { node, calls } = chooser();
  await mount(node);
  const field = q<HTMLInputElement>("chooser-typed");
  expect(field?.getAttribute("placeholder")).toBe("C:\\Matters\\Iyer");
  expect(document.querySelector(`label[for="${field?.id}"]`)?.textContent).toBe(
    "Or type a folder's location",
  );
  expect(q<HTMLButtonElement>("chooser-go")?.disabled).toBe(true);
  await act(async () => {
    if (field) Reflect.set(HTMLInputElement.prototype, "value", " D:\\Cases ", field);
    field?.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await key(field, "Backspace");
  await click(q("chooser-go"));
  expect(calls.go).toEqual([{ kind: "typed", path: "D:\\Cases" }]);
  await act(async () => {
    field?.form?.requestSubmit();
  });
  expect(calls.go.at(-1)).toEqual({ kind: "typed", path: "D:\\Cases" });
});

test("more shows its line at the list's end", async () => {
  await mount(chooser({ listing: ROOT }).node);
  expect(q("chooser-more")?.textContent).toBe("Some folders aren't shown.");
  await act(async () => root?.render(chooser().node));
  expect(q("chooser-more")).toBeNull();
});

test("no answer says so with Try again, which reads the folder again; not found and not allowed each say a line", async () => {
  const shown = (state: ChooserListing["state"]): ChooserListing => ({ ...MATTERS, state });
  const { node, calls } = chooser({ listing: shown({ kind: "no_answer" }) });
  await mount(node);
  expect(q("chooser-state")?.textContent).toBe("This folder didn't answer in time.");
  expect(q("chooser-heard")?.textContent).toBe("This folder didn't answer in time.");
  expect(q<HTMLButtonElement>("chooser-choose")?.disabled).toBe(true);
  await click(q("chooser-again"));
  expect(calls.go).toEqual([{ kind: "again" }]);
  await act(async () => root?.render(chooser({ listing: shown({ kind: "not_found" }) }).node));
  expect(q("chooser-state")?.textContent).toBe("This folder can't be found.");
  expect(q("chooser-again")).toBeNull();
  await act(async () => root?.render(chooser({ listing: shown({ kind: "not_allowed" }) }).node));
  expect(q("chooser-state")?.textContent).toBe("Clauseo can't use this folder.");
});

test("Choose this folder chooses; while the desktop asks, it waits and nothing moves", async () => {
  const { node, calls } = chooser();
  await mount(node);
  await click(q("chooser-choose"));
  expect(calls.choose).toBe(1);
  const waiting = chooser({ waiting: true });
  await act(async () => root?.render(waiting.node));
  expect(q("chooser-waiting")?.textContent).toBe("Waiting for you to confirm…");
  expect(q("chooser-heard")?.textContent).toBe("Waiting for you to confirm…");
  expect(q<HTMLButtonElement>("chooser-choose")?.disabled).toBe(true);
  await key(q("chooser-list"), "Enter");
  expect(waiting.calls.go).toEqual([]);
});

test("Escape and Cancel cancel, and Escape goes no further than the chooser", async () => {
  const beneath = vi.fn();
  document.addEventListener("keydown", beneath);
  const { node, calls } = chooser();
  await mount(node);
  await key(q("chooser-list"), "Escape");
  expect(calls.cancel).toBe(1);
  expect(beneath).not.toHaveBeenCalled();
  await click(q("chooser-cancel"));
  expect(calls.cancel).toBe(2);
  document.removeEventListener("keydown", beneath);
});

test("focus stays inside: what lies outside is inert, and the dialog's focus guards lead back in", async () => {
  const outside = document.createElement("button");
  outside.textContent = "outside";
  document.body.append(outside);
  await mount(chooser().node);
  const dialog = document.querySelector('[role="dialog"]');
  // what lies outside is hidden from the keyboard and screen readers while the chooser is open
  expect(outside.getAttribute("aria-hidden")).toBe("true");
  expect(outside.hasAttribute("data-base-ui-inert")).toBe(true);
  const guards = [...document.querySelectorAll<HTMLElement>("[data-base-ui-focus-guard]")];
  expect(guards.length).toBeGreaterThan(0);
  for (const guard of guards) {
    await act(async () => guard.focus());
    expect(dialog?.contains(document.activeElement)).toBe(true);
  }
  outside.remove();
});
