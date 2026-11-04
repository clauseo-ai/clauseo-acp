// The composer's `@` menu tests' fixture (composer-mention.test.tsx, composer-smart-quotes.test.tsx): a room's papers,
// the composer mounted in its providers and unmounted after each test, and the field's events as an engine sends them.
import { act, StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, vi } from "vitest";
import { settled } from "../lib/late.ts";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { TooltipProvider } from "../ui/tooltip.tsx";
import {
  Composer,
  type ComposerProps,
  type MentionItem,
  type MentionList,
  type Mentions,
} from "./composer.tsx";
import { RoomScopeProvider } from "./room-scope-provider.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
if (!("fonts" in document)) {
  Object.defineProperty(document, "fonts", {
    value: { ready: Promise.resolve(), status: "loaded", load: async () => [] },
  });
}

export const paper = (id: string, path: string): MentionItem => {
  const cut = path.lastIndexOf("/");
  return {
    id,
    label: cut === -1 ? path : path.slice(cut + 1),
    detail: cut === -1 ? undefined : path.slice(0, cut),
    text: path,
  };
};
export const PAPERS: MentionItem[] = [
  paper("f-1", "Lease deed.pdf"),
  paper("f-2", "Pleadings/Board minutes.pdf"),
  paper("f-3", "Notice.pdf"),
];

/** A room's papers: answers `list` (or what `answer` gives), counting the loads. */
export function source(list: MentionList | (() => Promise<MentionList>) = { items: PAPERS }) {
  const load = vi.fn(typeof list === "function" ? list : async () => list);
  const mentions: Mentions = { load };
  return { mentions, load };
}

let host: HTMLDivElement | null = null;
let root: ReturnType<typeof createRoot> | null = null;
export const wrap = (node: ReactNode) => (
  <StrictMode>
    <TooltipProvider>
      <RoomScopeProvider>{node}</RoomScopeProvider>
    </TooltipProvider>
  </StrictMode>
);
export const composer = (p: Partial<ComposerProps>) => (
  <Composer
    took={null}
    plan=""
    hint={null}
    testIds={{ field: "composer", menu: "composer-menu", menuItem: "composer-menu-item" }}
    {...p}
  />
);
export async function mount(node: ReactNode) {
  showDisplay({ ...DEFAULT_DISPLAY, motion: "reduced" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(wrap(node)));
}

/** Unmounts the composer mounted last, before a test mounts another. */
export async function unmount() {
  await act(async () => root?.unmount());
  host?.remove();
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  showDisplay(DEFAULT_DISPLAY);
  Reflect.deleteProperty(document, "execCommand");
  vi.useRealTimers();
  vi.restoreAllMocks();
});

export const field = () => document.querySelector<HTMLTextAreaElement>('[data-testid="composer"]')!;
export const menu = () => document.querySelector<HTMLElement>('[data-testid="composer-menu"]');
export const rows = () => [
  ...document.querySelectorAll<HTMLElement>('[data-testid="composer-menu-item"]'),
];
export const names = () => rows().map((r) => r.querySelector("bdi")?.textContent);
/** The plane's lines under the rows: loading, failed, none, the count past 50, a list that stops short. */
export const notes = () =>
  [...document.querySelectorAll(".fixed.z-20 p")].map((p) => p.textContent);
export const said = () => document.querySelector('[aria-live="polite"]')?.textContent;

export const focus = async () => {
  await act(async () => field().focus());
  await act(settled);
};
export const edit = (value: string, init: { inputType?: string; isComposing?: boolean } = {}) =>
  act(async () => {
    const a = field();
    Reflect.set(HTMLTextAreaElement.prototype, "value", value, a);
    a.setSelectionRange(value.length, value.length);
    a.dispatchEvent(
      new InputEvent("input", {
        bubbles: true,
        inputType: init.inputType ?? "insertText",
        isComposing: init.isComposing ?? false,
      }),
    );
  });
export const key = (k: string, init: KeyboardEventInit = {}) =>
  act(async () => {
    const e = new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...init });
    field().dispatchEvent(e);
    return e.defaultPrevented;
  });
export const put = (at: number) =>
  act(async () => {
    field().setSelectionRange(at, at);
    document.dispatchEvent(new Event("selectionchange"));
  });
export const compose = (kind: "compositionstart" | "compositionend") =>
  act(async () => {
    field().dispatchEvent(new CompositionEvent(kind, { bubbles: true, data: "" }));
  });

/** A Range's rects as a page would give them (composer-tokens.test.tsx's): one line, 10 px a character. */
export function lay() {
  vi.spyOn(Range.prototype, "getClientRects").mockImplementation(function (this: Range) {
    const r = new DOMRect(this.startOffset * 10, 0, (this.endOffset - this.startOffset) * 10, 26);
    return [r] as unknown as DOMRectList;
  });
}
export const boxes = () => [...document.querySelectorAll(".room-field-tokens .room-token")];

/** The engine's own editing with its undo stack (composer-tokens.test.tsx's). */
export function engine() {
  const back: string[] = [];
  const set = (a: HTMLTextAreaElement, v: string, at: number, inputType: string) => {
    Reflect.set(HTMLTextAreaElement.prototype, "value", v, a);
    a.setSelectionRange(at, at);
    a.dispatchEvent(new InputEvent("input", { bubbles: true, inputType }));
  };
  const exec = (command: string, _ui?: boolean, value?: string) => {
    const a = document.activeElement;
    if (!(a instanceof HTMLTextAreaElement)) return false;
    const insert = command === "delete" ? "" : (value ?? "");
    const s = a.selectionStart;
    back.push(a.value);
    const kind = command === "delete" ? "deleteContentBackward" : "insertText";
    set(a, a.value.slice(0, s) + insert + a.value.slice(a.selectionEnd), s + insert.length, kind);
    return true;
  };
  Object.defineProperty(document, "execCommand", { value: exec, configurable: true });
  return {
    undo: () =>
      act(async () => {
        const v = back.pop();
        if (v !== undefined) set(field(), v, v.length, "historyUndo");
      }),
  };
}
