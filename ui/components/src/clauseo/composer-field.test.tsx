// @vitest-environment happy-dom
// The composer's field at its interface: uncontrolled (React never writes the words, so the engine's undo stack stays
// whole), Enter its own and never an input method's, the handle setting and clearing the words through the engine's
// editing, and the field where the permission prompt finds it at capture time.
import { act, createRef, StrictMode, useEffect, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { TooltipProvider } from "../ui/tooltip.tsx";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { Composer, type ComposerHandle, type ComposerProps } from "./composer.tsx";
import { EdgePrompt } from "./edge.tsx";
import { fromComposer } from "./composer-keys.ts";
import { RoomScopeProvider } from "./room-scope-provider.tsx";
import { useRoom } from "./room-scope.ts";
import type { RoomScope } from "./room-scope.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
if (!("fonts" in document)) {
  Object.defineProperty(document, "fonts", {
    value: { ready: Promise.resolve(), status: "loaded", load: async () => [] },
  });
}

let host: HTMLDivElement | null = null;
let root: ReturnType<typeof createRoot> | null = null;
let scope: RoomScope | null = null;

function Scope() {
  const room = useRoom();
  useEffect(() => {
    scope = room;
  }, [room]);
  return null;
}

const wrap = (node: ReactNode) => (
  <StrictMode>
    <TooltipProvider>
      <RoomScopeProvider>
        <Scope />
        {node}
      </RoomScopeProvider>
    </TooltipProvider>
  </StrictMode>
);

async function mount(node: ReactNode) {
  showDisplay({ ...DEFAULT_DISPLAY, motion: "reduced" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(wrap(node)));
}

const rerender = (node: ReactNode) => act(async () => root?.render(wrap(node)));

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  scope = null;
  showDisplay(DEFAULT_DISPLAY);
  Reflect.deleteProperty(document, "execCommand");
  vi.unstubAllGlobals();
});

const composer = (p: Partial<ComposerProps>) => (
  <Composer took={null} plan="" hint={null} testIds={{ field: "composer", ask: "send" }} {...p} />
);
const field = () => document.querySelector<HTMLTextAreaElement>('[data-testid="composer"]')!;
const ask = () => document.querySelector<HTMLButtonElement>('[data-testid="send"]')!;
const type = (text: string) =>
  act(async () => {
    Reflect.set(HTMLTextAreaElement.prototype, "value", text, field());
    field().dispatchEvent(new Event("input", { bubbles: true }));
  });
/** A key in the field; true when its default was prevented. */
const key = (init: KeyboardEventInit) =>
  act(async () => {
    const e = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
    field().dispatchEvent(e);
    return e.defaultPrevented;
  });
const compose = (kind: "compositionstart" | "compositionend") =>
  act(async () => {
    field().dispatchEvent(new CompositionEvent(kind, { bubbles: true, data: "" }));
  });

/** The engine's editing, as WebKit and Chromium do it: the selection replaced, then an input event. */
function engine(result = true) {
  const calls: [string, string | undefined][] = [];
  const exec = (command: string, _ui?: boolean, value?: string) => {
    calls.push([command, value]);
    if (!result) return false;
    const a = document.activeElement;
    if (!(a instanceof HTMLTextAreaElement)) return false;
    const insert = command === "delete" ? "" : (value ?? "");
    const next = a.value.slice(0, a.selectionStart) + insert + a.value.slice(a.selectionEnd);
    Reflect.set(HTMLTextAreaElement.prototype, "value", next, a);
    a.dispatchEvent(new Event("input", { bubbles: true }));
    return true;
  };
  Object.defineProperty(document, "execCommand", { value: exec, configurable: true });
  return calls;
}

test("the field starts with its words, keeps what is typed through a render, and React never writes it", async () => {
  const onInput = vi.fn();
  await mount(composer({ initialText: "Draft", onInput }));
  expect(field().value).toBe("Draft");
  expect(ask().disabled).toBe(false);
  await type("What did the bank know?");
  expect(onInput).toHaveBeenLastCalledWith("What did the bank know?");
  // other words given later are not the field's: it was read once, as the field mounted
  await rerender(composer({ initialText: "Other words", onInput, busy: false }));
  expect(field().value).toBe("What did the bank know?");
  // nor is it written as the field's defaultValue (its text child), which React would rewrite on each commit
  expect(field().defaultValue).toBe("Draft");
  await type("   ");
  expect(ask().disabled).toBe(true);
});

test("Enter asks with the field's words and never breaks the line, empty or not; Shift+Enter breaks it", async () => {
  const onAsk = vi.fn();
  await mount(composer({ onAsk }));
  expect(await key({ key: "Enter", keyCode: 13 })).toBe(true);
  expect(onAsk).toHaveBeenLastCalledWith("");
  await type("First line");
  expect(await key({ key: "Enter", keyCode: 13, shiftKey: true })).toBe(false);
  expect(onAsk).toHaveBeenCalledOnce();
  expect(await key({ key: "Enter", keyCode: 13 })).toBe(true);
  expect(onAsk).toHaveBeenLastCalledWith("First line");
});

test("an input method's Enter is its own: composing, keyCode 229, or any Enter between compositionstart and end", async () => {
  const onAsk = vi.fn();
  await mount(composer({ onAsk, initialText: "नमस्ते" }));
  expect(await key({ key: "Enter", keyCode: 229, isComposing: true })).toBe(false);
  expect(await key({ key: "Enter", keyCode: 229 })).toBe(false);
  await compose("compositionstart");
  // a plain Enter inside the composition, with neither flag set: the field's own record of it holds the send
  expect(await key({ key: "Enter", keyCode: 13 })).toBe(false);
  await compose("compositionend");
  expect(onAsk).not.toHaveBeenCalled();
  expect(await key({ key: "Enter", keyCode: 13 })).toBe(true);
  expect(onAsk).toHaveBeenCalledExactlyOnceWith("नमस्ते");
});

test("a composition the engine never ended doesn't hold Enter past the field's blur", async () => {
  const onAsk = vi.fn();
  await mount(composer({ onAsk, initialText: "Words" }));
  await compose("compositionstart");
  await act(async () => {
    field().focus();
    field().blur();
  });
  expect(await key({ key: "Enter", keyCode: 13 })).toBe(true);
  expect(onAsk).toHaveBeenCalledOnce();
});

test("while an input method composes, onKeyCommand hears no key: each is the input method's", async () => {
  const onKeyCommand = vi.fn(() => true);
  await mount(composer({ onKeyCommand }));
  expect(await key({ key: "Escape", isComposing: true })).toBe(false);
  expect(await key({ key: "Escape", keyCode: 229 })).toBe(false);
  await compose("compositionstart");
  expect(await key({ key: "Escape", keyCode: 27 })).toBe(false);
  await compose("compositionend");
  expect(onKeyCommand).not.toHaveBeenCalled();
  expect(await key({ key: "Escape", keyCode: 27 })).toBe(true);
  expect(onKeyCommand).toHaveBeenCalledOnce();
});

test("onKeyCommand hears keys first: a key it takes is prevented, and an Enter it takes asks nothing", async () => {
  const onAsk = vi.fn();
  const onKeyCommand = vi.fn((e: { key: string }) => e.key === "Escape" || e.key === "Enter");
  await mount(composer({ onAsk, onKeyCommand, initialText: "Words" }));
  expect(await key({ key: "Escape" })).toBe(true);
  expect(await key({ key: "Enter", keyCode: 13 })).toBe(true);
  expect(onAsk).not.toHaveBeenCalled();
  expect(onKeyCommand).toHaveBeenCalledTimes(2);
});

test("Ask sends the field's words", async () => {
  const onAsk = vi.fn();
  await mount(composer({ onAsk }));
  await type("Summarise it");
  await act(async () => ask().click());
  expect(onAsk).toHaveBeenCalledExactlyOnceWith("Summarise it");
});

test("the handle sets and clears the words through the engine's editing, so undo takes each back", async () => {
  const calls = engine();
  const onInput = vi.fn();
  const ref = createRef<ComposerHandle>();
  await mount(composer({ ref, onInput }));
  await act(async () => ref.current?.setText("When was the notice sent?"));
  expect(calls).toEqual([["insertText", "When was the notice sent?"]]);
  expect(field().value).toBe("When was the notice sent?");
  expect(document.activeElement).toBe(field());
  expect(ref.current?.text()).toBe("When was the notice sent?");
  expect(onInput).toHaveBeenCalledExactlyOnceWith("When was the notice sent?");
  expect(ask().disabled).toBe(false);
  await act(async () => ref.current?.clear());
  expect(calls.at(-1)).toEqual(["delete", undefined]);
  expect(field().value).toBe("");
  expect(onInput).toHaveBeenLastCalledWith("");
  expect(ask().disabled).toBe(true);
});

test("clear never takes the focus from where the lawyer is; the words go in place", async () => {
  const calls = engine();
  const onInput = vi.fn();
  const ref = createRef<ComposerHandle>();
  await mount(composer({ ref, onInput, initialText: "Asked" }));
  const elsewhere = document.createElement("input");
  document.body.append(elsewhere);
  elsewhere.focus();
  await act(async () => ref.current?.clear());
  expect(calls).toEqual([]);
  expect(field().value).toBe("");
  expect(document.activeElement).toBe(elsewhere);
  expect(onInput).toHaveBeenCalledExactlyOnceWith("");
  elsewhere.remove();
});

test("after a press on Ask, clear takes the focus back to the field and empties it as one undoable step", async () => {
  const calls = engine();
  const ref = createRef<ComposerHandle>();
  await mount(composer({ ref, initialText: "Asked" }));
  ask().focus();
  await act(async () => ref.current?.clear("Asked"));
  expect(calls).toEqual([["delete", undefined]]);
  expect(field().value).toBe("");
  expect(document.activeElement).toBe(field());
  // WebKit on macOS leaves a clicked button unfocused: focus on nothing is taken back too
  await type("Asked again");
  field().blur();
  expect(document.activeElement).toBe(document.body);
  await act(async () => ref.current?.clear("Asked again"));
  expect(calls.at(-1)).toEqual(["delete", undefined]);
  expect(document.activeElement).toBe(field());
});

test("fill puts words into an empty field without taking the focus from elsewhere, and leaves words alone", async () => {
  const calls = engine();
  const onInput = vi.fn();
  const ref = createRef<ComposerHandle>();
  await mount(composer({ ref, onInput }));
  const elsewhere = document.createElement("input");
  document.body.append(elsewhere);
  elsewhere.focus();
  await act(async () => ref.current?.fill("Summarise the notice"));
  expect(calls).toEqual([]);
  expect(field().value).toBe("Summarise the notice");
  expect(document.activeElement).toBe(elsewhere);
  expect(onInput).toHaveBeenCalledExactlyOnceWith("Summarise the notice");
  // words in the field keep their place
  await act(async () => ref.current?.fill("Something else"));
  expect(field().value).toBe("Summarise the notice");
  elsewhere.remove();
});

test("fill with focus on nothing goes through the engine as one undoable step", async () => {
  const calls = engine();
  const ref = createRef<ComposerHandle>();
  await mount(composer({ ref }));
  await act(async () => ref.current?.fill("Summarise the notice"));
  expect(calls).toEqual([["insertText", "Summarise the notice"]]);
  expect(field().value).toBe("Summarise the notice");
});

test("clear(only) clears only while the field holds exactly that", async () => {
  const ref = createRef<ComposerHandle>();
  await mount(composer({ ref, initialText: "Asked" }));
  await type("Asked, and more");
  await act(async () => ref.current?.clear("Asked"));
  expect(field().value).toBe("Asked, and more");
});

test("a clear asked for while an input method composes waits for its end, and then only if nothing was composed", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout"] });
  try {
    const calls = engine();
    const onInput = vi.fn();
    const ref = createRef<ComposerHandle>();
    await mount(composer({ ref, onInput, initialText: "Asked" }));
    field().focus();
    await compose("compositionstart");
    await act(async () => ref.current?.clear("Asked"));
    // nothing touched inside the composition
    expect(calls).toEqual([]);
    expect(field().value).toBe("Asked");
    await compose("compositionend");
    await act(async () => vi.runAllTimers());
    expect(calls).toEqual([["delete", undefined]]);
    expect(field().value).toBe("");
    // a composition that put words in: the field no longer holds the question, so it stays
    await type("Asked");
    await compose("compositionstart");
    await act(async () => ref.current?.clear("Asked"));
    await type("Asked न");
    await compose("compositionend");
    await act(async () => vi.runAllTimers());
    expect(field().value).toBe("Asked न");
  } finally {
    vi.useRealTimers();
  }
});

test("a fill asked for while an input method composes waits for its end, and then only into an empty field", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout"] });
  try {
    const calls = engine();
    const ref = createRef<ComposerHandle>();
    await mount(composer({ ref }));
    field().focus();
    await compose("compositionstart");
    await act(async () => ref.current?.fill("Summarise the notice"));
    expect(calls).toEqual([]);
    expect(field().value).toBe("");
    await compose("compositionend");
    await act(async () => vi.runAllTimers());
    expect(field().value).toBe("Summarise the notice");
    // a composition that put words in keeps them: the fill finds the field taken
    await act(async () => ref.current?.clear());
    await compose("compositionstart");
    await act(async () => ref.current?.fill("Summarise the notice"));
    await type("न");
    await compose("compositionend");
    await act(async () => vi.runAllTimers());
    expect(field().value).toBe("न");
  } finally {
    vi.useRealTimers();
  }
});

test("where the engine refuses its editing, setText still puts the words in and tells the room", async () => {
  const calls = engine(false);
  const onInput = vi.fn();
  const ref = createRef<ComposerHandle>();
  await mount(composer({ ref, onInput }));
  await act(async () => ref.current?.setText("Words"));
  expect(calls).toEqual([["insertText", "Words"]]);
  expect(field().value).toBe("Words");
  expect(onInput).toHaveBeenCalledExactlyOnceWith("Words");
});

test("the field stands in the room's field slot, and a capture listener reads its typed words", async () => {
  await mount(composer({}));
  expect(scope?.nodes.get("field")).toBe(field());
  await type("Typed words");
  let seen: { mine: boolean; value: string } | null = null;
  const listen = (e: KeyboardEvent) => {
    const t = e.target;
    seen = {
      mine: scope !== null && fromComposer(scope, t),
      value: t instanceof HTMLTextAreaElement ? t.value : "",
    };
  };
  document.addEventListener("keydown", listen, true);
  try {
    await key({ key: "Enter", keyCode: 13 });
  } finally {
    document.removeEventListener("keydown", listen, true);
  }
  expect(seen).toEqual({ mine: true, value: "Typed words" });
});

test("a gate keeps the words: typed, gated, ungated, the same field is back with them and its undo, Ask with it", async () => {
  // the fit's size observer: it must keep watching the one field, never let go of it across the gate
  const watched: { targets: Element[]; gone: boolean }[] = [];
  vi.stubGlobal(
    "ResizeObserver",
    class {
      seen = { targets: [] as Element[], gone: false };
      constructor() {
        watched.push(this.seen);
      }
      observe(t: Element) {
        this.seen.targets.push(t);
      }
      unobserve() {}
      disconnect() {
        this.seen.gone = true;
      }
    },
  );
  const onInput = vi.fn();
  const gate = <button type="button">Sign in</button>;
  await mount(composer({ onInput }));
  const before = field();
  await type("What did the bank know?");
  onInput.mockClear();
  await rerender(composer({ onInput, gate }));
  // the gate stands in the row; the field is kept, hidden, with Ask and Add papers gone
  expect(document.querySelector(".room-field")?.textContent).toContain("Sign in");
  expect(field().hidden).toBe(true);
  expect(document.querySelector('[data-testid="send"]')).toBeNull();
  await rerender(composer({ onInput }));
  // the very node, so the engine's undo stack for what was typed is the one the lawyer left
  expect(field()).toBe(before);
  expect(field().hidden).toBe(false);
  expect(field().value).toBe("What did the bank know?");
  expect(ask().disabled).toBe(false);
  // nothing changed the words, so the room's draft heard nothing
  expect(onInput).not.toHaveBeenCalled();
  expect(scope?.nodes.get("field")).toBe(before);
  expect(watched.some((o) => o.targets.includes(before) && !o.gone)).toBe(true);
});

test("an edge prompt answers Enter from the empty composer ahead of it, and never over words, whose Enter asks", async () => {
  const onAsk = vi.fn();
  const onAllow = vi.fn();
  await mount(
    <>
      <EdgePrompt
        kind="folder"
        place="Documents/Precedents"
        state={{ kind: "pending" }}
        keys
        onAllow={onAllow}
        onRefuse={() => {}}
      />
      {composer({ onAsk })}
    </>,
  );
  expect(await key({ key: "Enter", keyCode: 13 })).toBe(true);
  expect(onAllow).toHaveBeenCalledOnce();
  expect(onAsk).not.toHaveBeenCalled();
  await type("And clause ten?");
  expect(await key({ key: "Enter", keyCode: 13 })).toBe(true);
  expect(onAllow).toHaveBeenCalledOnce();
  expect(onAsk).toHaveBeenCalledExactlyOnceWith("And clause ten?");
});
