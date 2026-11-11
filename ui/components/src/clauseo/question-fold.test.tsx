// @vitest-environment happy-dom
// A long question in the thread folds after eight lines, with Show all and Show less as a real button that says
// whether it is open; eight lines or fewer, nothing changes. The pinned question is one line, its press brings the
// thread back to the question, gliding unless motion is reduced, and moves the focus there; a folded question stays
// folded. happy-dom lays nothing out: a short question's lines are given by its scrollHeight, and the one line is the
// stylesheet's.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { act, StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { DEFAULT_DISPLAY, showDisplay, type Display } from "../lib/display.ts";
import { FoldedQuestion } from "./question-fold.tsx";
import { FOLD_CHARS, startOf } from "./question-fold-logic.ts";
import { useRoom } from "./room-scope.ts";
import { RoomScopeProvider } from "./room-scope-provider.tsx";
import { Thread } from "./thread.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
if (!("fonts" in document)) {
  Object.defineProperty(document, "fonts", {
    value: { ready: Promise.resolve(), status: "loaded", load: async () => [] },
  });
}

let host: HTMLDivElement | null = null;
let root: ReturnType<typeof createRoot> | null = null;

async function mount(node: ReactNode, display: Partial<Display> = {}) {
  showDisplay({ ...DEFAULT_DISPLAY, ...display });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(
      <StrictMode>
        <RoomScopeProvider>{node}</RoomScopeProvider>
      </StrictMode>,
    ),
  );
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  showDisplay(DEFAULT_DISPLAY);
  vi.restoreAllMocks();
});

const LONG = "The bank's reply denied the debt and alleged short delivery. ".repeat(100);
const asked = () => document.querySelector<HTMLParagraphElement>(".asked");
const toggle = () => document.querySelector<HTMLButtonElement>(".asked-fold");
const press = async (b: HTMLElement | null) => act(async () => b?.click());

/** A short question's height in lines, as the clamp would leave it: scrollHeight is its whole height, clientHeight
 * the clamp's (eight lines of 28 px). */
function lines(n: number) {
  vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockImplementation(function (
    this: HTMLElement,
  ) {
    return this.classList.contains("asked") ? n * 28 : 0;
  });
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockImplementation(function (
    this: HTMLElement,
  ) {
    return this.classList.contains("asked") ? Math.min(n, 8) * 28 : 0;
  });
}

test("a question past eight lines folds, with Show all; pressed, it opens with Show less, and folds again", async () => {
  await mount(<FoldedQuestion text={LONG} />);
  expect(asked()?.hasAttribute("data-folded")).toBe(true);
  // folded, only its start is drawn: a 6 MiB question costs a few lines of layout
  expect(asked()?.textContent).toBe(LONG.slice(0, FOLD_CHARS));
  expect(toggle()?.textContent).toBe("Show all");
  expect(toggle()?.getAttribute("type")).toBe("button");
  expect(toggle()?.getAttribute("aria-expanded")).toBe("false");
  expect(toggle()?.getAttribute("aria-controls")).toBe(asked()?.id);
  await press(toggle());
  expect(asked()?.hasAttribute("data-folded")).toBe(false);
  expect(asked()?.textContent).toBe(LONG);
  expect(toggle()?.textContent).toBe("Show less");
  expect(toggle()?.getAttribute("aria-expanded")).toBe("true");
  await press(toggle());
  expect(asked()?.hasAttribute("data-folded")).toBe(true);
  expect(toggle()?.getAttribute("aria-expanded")).toBe("false");
});

test("a short question the clamp cuts (nine lines) folds; one of eight lines has no toggle and draws whole", async () => {
  lines(9);
  await mount(<FoldedQuestion text="Nine lines of a question." />);
  expect(toggle()?.textContent).toBe("Show all");
  await act(async () => root?.unmount());
  host?.remove();
  vi.restoreAllMocks();
  lines(8);
  await mount(<FoldedQuestion text="Eight lines of a question." />);
  expect(toggle()).toBeNull();
  expect(asked()?.textContent).toBe("Eight lines of a question.");
});

test("Show all and Show less say themselves in Hindi", async () => {
  await mount(<FoldedQuestion text={LONG} />, { lang: "hi" });
  expect(toggle()?.textContent).toBe("पूरा दिखाएँ");
  await press(toggle());
  expect(toggle()?.textContent).toBe("कम दिखाएँ");
});

test("the start of a question never ends inside a surrogate pair", () => {
  expect(startOf("ab\u{1F600}c", 3)).toBe("ab");
  expect(startOf("abc", 5)).toBe("abc");
});

const sheet = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "thread.css"), "utf8");

test("the pinned question is one line, cut with an ellipsis, and the fold is the line clamp at eight", () => {
  const pinLine = /\.room-pin-line \{([^}]*)\}/.exec(sheet)?.[1] ?? "";
  expect(pinLine).toMatch(/white-space: nowrap;/);
  expect(pinLine).toMatch(/overflow: hidden;/);
  expect(pinLine).toMatch(/text-overflow: ellipsis;/);
  const folded = /\.asked\[data-folded\] \{([^}]*)\}/.exec(sheet)?.[1] ?? "";
  expect(folded).toMatch(/-webkit-line-clamp: 8;/);
  expect(folded).toMatch(/overflow: hidden;/);
});

/** The latest question's section, as the room's thread names it (list.tsx). */
function Question({ text }: { text: string }) {
  const { nodes } = useRoom();
  return (
    <section className="step step-question" ref={nodes.slot("questionStep")} tabIndex={-1}>
      <FoldedQuestion text={text} ref={nodes.slot("asked")} />
    </section>
  );
}

async function pressPin(display: Partial<Display>) {
  const glides: (ScrollBehavior | undefined)[] = [];
  vi.spyOn(HTMLElement.prototype, "scrollTo").mockImplementation(function (
    this: HTMLElement,
    a?: ScrollToOptions | number,
  ) {
    if (typeof a === "object") glides.push(a.behavior);
  });
  vi.spyOn(window, "scrollBy").mockImplementation((a?: ScrollToOptions | number) => {
    if (typeof a === "object") glides.push(a.behavior);
  });
  await mount(
    <Thread question={LONG} who="You">
      <Question text={LONG} />
    </Thread>,
    display,
  );
  const pin = document.querySelector<HTMLButtonElement>(".room-pin");
  // its words are the question's start, its line's ellipsis where it is cut
  const words = pin?.querySelector(".room-pin-text")?.textContent ?? "";
  expect(LONG.startsWith(words)).toBe(true);
  expect(words.length).toBeLessThan(300);
  expect(pin?.textContent).toContain("Scroll back to it");
  await press(pin);
  return glides;
}

test("pressing the pinned question glides the thread to the question and moves the focus there; it stays folded", async () => {
  const glides = await pressPin({ motion: "full", render: "full" });
  expect(glides).toContain("smooth");
  expect(document.activeElement).toBe(document.querySelector(".step-question"));
  expect(asked()?.hasAttribute("data-folded")).toBe(true);
  expect(toggle()?.textContent).toBe("Show all");
});

test("under reduced motion the press goes to the question at once", async () => {
  const glides = await pressPin({ motion: "reduced" });
  expect(glides).toContain("instant");
  expect(glides).not.toContain("smooth");
  expect(document.activeElement).toBe(document.querySelector(".step-question"));
});
