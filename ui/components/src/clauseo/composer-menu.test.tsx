// @vitest-environment happy-dom
// The composer's `/` menu at its interface, through the composer: it loads as the field is first focused, opens on a
// `/` at a run's start (typed, replaced by an input method that never composes, or after a composition ends, never
// during one), picks with Enter, Tab or a press and never sends while open, closes with Escape for its trigger, reads
// a replaced list with the highlight kept by name, and keeps the room's prompts off its Enter while open.
import { act, StrictMode, useEffect, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { settled } from "../lib/late.ts";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { TooltipProvider } from "../ui/tooltip.tsx";
import { Composer, type ComposerProps, type SlashCommand } from "./composer.tsx";
import { fromComposer } from "./composer-keys.ts";
import { RoomScopeProvider } from "./room-scope-provider.tsx";
import { useRoom, type RoomScope } from "./room-scope.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
if (!("fonts" in document)) {
  Object.defineProperty(document, "fonts", {
    value: { ready: Promise.resolve(), status: "loaded", load: async () => [] },
  });
}

const COMMANDS: SlashCommand[] = [
  { name: "review", description: "Review the changes" },
  { name: "restart", description: "Start the session again" },
  { name: "compact", description: "Compact the conversation", input_hint: "what to keep" },
];

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

const IDS = {
  field: "composer",
  ask: "send",
  menu: "composer-menu",
  menuItem: "composer-menu-item",
};
const composer = (p: Partial<ComposerProps>) => (
  <Composer took={null} plan="" hint={null} testIds={IDS} commands={COMMANDS} {...p} />
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
  vi.useRealTimers();
});

const field = () => document.querySelector<HTMLTextAreaElement>('[data-testid="composer"]')!;
const menu = () => document.querySelector<HTMLElement>('[data-testid="composer-menu"]');
const rows = () => [
  ...document.querySelectorAll<HTMLElement>('[data-testid="composer-menu-item"]'),
];
const lit = () => rows().find((r) => r.getAttribute("aria-selected") === "true");
const names = () => rows().map((r) => r.querySelector("bdi")?.textContent);

/** The field focused, and the menu's chunk in. */
const focus = async () => {
  await act(async () => field().focus());
  await act(settled);
};

/** An edit as an engine makes it: the new value with the caret at its end (or `caret`), then the input event. */
const edit = (
  value: string,
  init: { inputType?: string; isComposing?: boolean; caret?: number } = {},
) =>
  act(async () => {
    const a = field();
    Reflect.set(HTMLTextAreaElement.prototype, "value", value, a);
    const at = init.caret ?? value.length;
    a.setSelectionRange(at, at);
    a.dispatchEvent(
      new InputEvent("input", {
        bubbles: true,
        inputType: init.inputType ?? "insertText",
        isComposing: init.isComposing ?? false,
      }),
    );
  });

/** A key in the field; true when its default was prevented. */
const key = (k: string, init: KeyboardEventInit = {}) =>
  act(async () => {
    const e = new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...init });
    field().dispatchEvent(e);
    return e.defaultPrevented;
  });

const compose = (kind: "compositionstart" | "compositionend") =>
  act(async () => {
    field().dispatchEvent(new CompositionEvent(kind, { bubbles: true, data: "" }));
  });

/** The engine's own editing, as WebKit and Chromium do it: the selection replaced, then an input event. */
function engine() {
  const calls: [string, string | undefined][] = [];
  const exec = (command: string, _ui?: boolean, value?: string) => {
    calls.push([command, value]);
    const a = document.activeElement;
    if (!(a instanceof HTMLTextAreaElement)) return false;
    const insert = value ?? "";
    const start = a.selectionStart;
    const next = a.value.slice(0, start) + insert + a.value.slice(a.selectionEnd);
    Reflect.set(HTMLTextAreaElement.prototype, "value", next, a);
    a.setSelectionRange(start + insert.length, start + insert.length);
    a.dispatchEvent(
      new InputEvent("input", { bubbles: true, inputType: "insertText", data: insert }),
    );
    return true;
  };
  Object.defineProperty(document, "execCommand", { value: exec, configurable: true });
  return calls;
}

// first in the file, while the menu's chunk has never loaded: the race it covers needs the import still on its way
test("a / typed while the chunk is on its way opens the menu as it lands; leaving the field closes it", async () => {
  await mount(composer({}));
  await act(async () => {
    field().focus();
    // typed before the chunk is in: no listener of its yet
    Reflect.set(HTMLTextAreaElement.prototype, "value", "/", field());
    field().setSelectionRange(1, 1);
    field().dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText" }));
  });
  await act(settled);
  expect(menu()).not.toBeNull();
  await act(async () => field().blur());
  expect(menu()).toBeNull();
  expect(field().hasAttribute("data-menu")).toBe(false);
});

test("a / at the field's start opens the agent's commands at the caret, named, its rows and the lit one on the field", async () => {
  await mount(composer({}));
  const a = field();
  expect(a.getAttribute("aria-autocomplete")).toBe("list");
  expect(a.getAttribute("aria-haspopup")).toBe("listbox");
  expect(a.hasAttribute("aria-controls")).toBe(false);
  await focus();
  await edit("/");
  expect(menu()?.getAttribute("role")).toBe("listbox");
  expect(menu()?.getAttribute("aria-label")).toBe("Commands");
  expect(names()).toEqual(["/review", "/restart", "/compact"]);
  expect(rows()[2]?.textContent).toContain("what to keep");
  expect(rows()[2]?.textContent).toContain("Compact the conversation");
  expect(a.getAttribute("aria-controls")).toBe(menu()?.id);
  expect(a.getAttribute("aria-activedescendant")).toBe(lit()?.id);
  expect(lit()?.textContent).toContain("/review");
  expect(a.hasAttribute("data-menu")).toBe(true);
  // the lit row moves with the arrows, round the ends, and the field says so
  expect(await key("ArrowUp")).toBe(true);
  expect(lit()?.textContent).toContain("/compact");
  expect(await key("ArrowDown")).toBe(true);
  expect(await key("ArrowDown")).toBe(true);
  expect(lit()?.textContent).toContain("/restart");
  expect(a.getAttribute("aria-activedescendant")).toBe(lit()?.id);
});

test("M6: / opens at the start, not after a letter, and after a space; the filter; Escape; Enter picks, then sends once", async () => {
  const calls = engine();
  const onAsk = vi.fn();
  await mount(composer({ onAsk }));
  await focus();
  await edit("/");
  expect(menu()).not.toBeNull();
  // Escape closes it, and it stays closed for that slash
  expect(await key("Escape")).toBe(true);
  expect(menu()).toBeNull();
  await edit("/r");
  expect(menu()).toBeNull();
  await edit("", { inputType: "deleteContentBackward" });
  await edit("a/");
  expect(menu()).toBeNull();
  await edit("", { inputType: "deleteContentBackward" });
  await edit("x /re");
  expect(names()).toEqual(["/review", "/restart"]);
  expect(await key("Enter", { keyCode: 13 })).toBe(true);
  expect(onAsk).not.toHaveBeenCalled();
  expect(calls).toEqual([["insertText", "/review "]]);
  expect(field().value).toBe("x /review ");
  expect(menu()).toBeNull();
  expect(field().hasAttribute("data-menu")).toBe(false);
  expect(field().hasAttribute("aria-activedescendant")).toBe(false);
  // the pick's own insertion opened nothing; Enter with no menu sends, once
  expect(await key("Enter", { keyCode: 13 })).toBe(true);
  expect(onAsk).toHaveBeenCalledExactlyOnceWith("x /review ");
});

test("Tab picks as Enter does, and a press on a row picks that row, the focus staying in the field", async () => {
  await mount(composer({}));
  await focus();
  await edit("/c");
  expect(await key("Tab")).toBe(true);
  expect(field().value).toBe("/compact ");
  await edit("/compact /");
  const second = rows()[1]!;
  await act(async () => {
    second.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, cancelable: true }));
  });
  expect(field().value).toBe("/compact /restart ");
  expect(document.activeElement).toBe(field());
  expect(menu()).toBeNull();
});

test("no query is read while an input method composes; a / typed in the composition opens the menu at its end", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout"] });
  await mount(composer({}));
  await focus();
  await compose("compositionstart");
  await edit("/", { inputType: "insertCompositionText", isComposing: true });
  expect(menu()).toBeNull();
  // even an input event that forgot its flag: the composer's own record holds the query
  await edit("/r", { inputType: "insertCompositionText" });
  expect(menu()).toBeNull();
  await compose("compositionend");
  expect(menu()).toBeNull();
  await act(async () => vi.runAllTimers());
  expect(names()).toEqual(["/review", "/restart"]);
});

test("an input method that edits by replacement and never composes (Tamil99, Anjal) opens the menu as typing does", async () => {
  await mount(composer({}));
  await focus();
  await edit("நீதி /", { inputType: "insertReplacementText" });
  expect(menu()).not.toBeNull();
  expect(names()).toHaveLength(3);
});

test("a list replaced while the menu is open is read again, the lit row kept by its name, or the first once it's gone", async () => {
  await mount(composer({}));
  await focus();
  await edit("/");
  await key("ArrowDown");
  expect(lit()?.textContent).toContain("/restart");
  const added = [{ name: "init", description: "Start a project" }, ...COMMANDS];
  await rerender(composer({ commands: added }));
  expect(names()).toEqual(["/init", "/review", "/restart", "/compact"]);
  expect(lit()?.textContent).toContain("/restart");
  expect(field().getAttribute("aria-activedescendant")).toBe(lit()?.id);
  await rerender(composer({ commands: added.filter((c) => c.name !== "restart") }));
  expect(lit()?.textContent).toContain("/init");
  // a list with nothing the query matches closes it
  await rerender(composer({ commands: [{ name: "zap", description: "" }] }));
  await edit("/r");
  expect(menu()).toBeNull();
});

test("while the menu is open the room's prompts keep off the field's keys; closed, they have them again", async () => {
  await mount(composer({}));
  await focus();
  await edit("/");
  expect(scope !== null && fromComposer(scope, field())).toBe(false);
  await key("Escape");
  expect(scope !== null && fromComposer(scope, field())).toBe(true);
});

test("without the agent's commands, / is a character: no list, no menu; a list that arrives later is read", async () => {
  await mount(<Composer took={null} plan="" hint={null} testIds={IDS} />);
  expect(field().hasAttribute("aria-autocomplete")).toBe(false);
  await focus();
  await edit("/");
  expect(menu()).toBeNull();
  // an empty list, as before the agent sends one: no menu either, and the field claims no list
  await rerender(composer({ commands: [] }));
  expect(field().hasAttribute("aria-autocomplete")).toBe(false);
  expect(field().hasAttribute("aria-haspopup")).toBe(false);
  await edit("/r");
  expect(menu()).toBeNull();
  // the agent's list arriving while the field has the focus: the next / opens it
  await rerender(composer({}));
  await act(settled);
  expect(field().getAttribute("aria-haspopup")).toBe("listbox");
  await edit("/r");
  expect(names()).toEqual(["/review", "/restart"]);
});

test("a gate closes the menu and hides the field from the room", async () => {
  await mount(composer({}));
  await focus();
  await edit("/");
  expect(menu()).not.toBeNull();
  await rerender(composer({ gate: <button type="button">Sign in</button> }));
  expect(menu()).toBeNull();
  expect(scope?.nodes.get("field")).toBeNull();
  await rerender(composer({}));
  expect(scope?.nodes.get("field")).toBe(field());
});

test("a list that arrives under an old /re run, the field focused, opens nothing; the next letter typed opens it", async () => {
  await mount(composer({ commands: [] }));
  await focus();
  await edit("/re");
  expect(menu()).toBeNull();
  // the agent's list lands while the caret still stands in the run typed before it: old words, no menu
  await rerender(composer({}));
  await act(settled);
  expect(menu()).toBeNull();
  expect(field().hasAttribute("data-menu")).toBe(false);
  // a fresh insertion is the lawyer's: it opens
  await edit("/res");
  expect(names()).toEqual(["/restart"]);
});

test("while an input method composes, Enter and the arrows are its own: no pick, no send, the lit row stays", async () => {
  const onAsk = vi.fn();
  await mount(composer({ onAsk }));
  await focus();
  await edit("/");
  expect(lit()?.textContent).toContain("/review");
  await compose("compositionstart");
  expect(await key("ArrowDown", { isComposing: true })).toBe(false);
  expect(await key("Enter", { isComposing: true })).toBe(false);
  await compose("compositionend");
  // a key the engine holds (keyCode 229), its composition flag aside
  expect(await key("ArrowDown", { keyCode: 229 })).toBe(false);
  expect(await key("Enter", { keyCode: 229 })).toBe(false);
  expect(lit()?.textContent).toContain("/review");
  expect(field().value).toBe("/");
  expect(onAsk).not.toHaveBeenCalled();
  expect(menu()).not.toBeNull();
});

test("a pick replaces the slash's whole run; a modified Enter is no pick, but the composer's own key", async () => {
  engine();
  const onAsk = vi.fn();
  await mount(composer({ onAsk }));
  await focus();
  await edit("/review the lease", { caret: 3 });
  expect(names()).toEqual(["/review", "/restart"]);
  for (const mod of ["ctrlKey", "metaKey", "altKey"]) await key("Enter", { [mod]: true });
  expect(field().value).toBe("/review the lease");
  expect(onAsk.mock.calls).toEqual([
    ["/review the lease"],
    ["/review the lease"],
    ["/review the lease"],
  ]);
  expect(await key("ArrowDown")).toBe(true);
  expect(await key("Enter")).toBe(true);
  expect(field().value).toBe("/restart the lease");
});

test("rows are named by their command: a list that puts another row under the highlight says the new row", async () => {
  await mount(composer({}));
  await focus();
  await edit("/re");
  const first = field().getAttribute("aria-activedescendant");
  expect(first).toBe(lit()?.id);
  await rerender(composer({ commands: [COMMANDS[1]!, COMMANDS[0]!] }));
  // the highlight is kept by name, on the row now second, and the field names that row
  expect(lit()?.textContent).toContain("/review");
  expect(field().getAttribute("aria-activedescendant")).toBe(first);
  await rerender(composer({ commands: [COMMANDS[1]!] }));
  expect(lit()?.textContent).toContain("/restart");
  expect(field().getAttribute("aria-activedescendant")).not.toBe(first);
  expect(field().getAttribute("aria-activedescendant")).toBe(lit()?.id);
});

test("a command row's icon is hidden from assistive technology and adds no words; its text is the name and the note", async () => {
  await mount(composer({}));
  await focus();
  await edit("/");
  const icons = rows().map((r) => r.querySelector("svg"));
  expect(icons).toHaveLength(3);
  for (const svg of icons) {
    expect(svg?.getAttribute("aria-hidden")).toBe("true");
    expect(svg?.hasAttribute("aria-label")).toBe(false);
    expect(svg?.getAttribute("width")).toBe("16");
    // muted, and the row's text colour once the row is highlighted (the row is the group)
    expect(svg?.getAttribute("class")).toContain("group-data-highlighted:text-primary");
  }
  expect(rows().every((r) => r.classList.contains("group"))).toBe(true);
  expect(rows().map((r) => r.textContent)).toEqual([
    "/reviewReview the changes",
    "/restartStart the session again",
    "/compact what to keepCompact the conversation",
  ]);
  expect(names()).toEqual(["/review", "/restart", "/compact"]);
});

test("a polite live region says how many commands the menu opened with, then the lit one's name as the arrows move", async () => {
  await mount(composer({}));
  await focus();
  const live = () => document.querySelector<HTMLElement>('[aria-live="polite"]');
  expect(live()?.textContent).toBe("");
  const region = live();
  await edit("/");
  expect(live()?.textContent).toBe("3 commands");
  // the same node, there before the menu opened: a region added with its words in may be heard saying nothing
  expect(live()).toBe(region);
  await key("ArrowDown");
  expect(live()?.textContent).toBe("/restart");
  await edit("/c");
  expect(live()?.textContent).toBe("/compact");
  await key("Escape");
  expect(live()?.textContent).toBe("");
  await edit("/c /comp");
  expect(live()?.textContent).toBe("1 command");
});
