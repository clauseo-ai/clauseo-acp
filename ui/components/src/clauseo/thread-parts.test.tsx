// @vitest-environment happy-dom
// The thread's parts with their props: the composer keeps Ask open under a note and holds only Add papers and Ask,
// or Stop, in its field's row; it takes the room's own placeholder and line where given, and offers questions only
// over an empty field. The working line says each state and its error's way on. The thread names itself and keeps
// the question for its pin.
import { act, StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { TooltipProvider } from "../ui/tooltip.tsx";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { Composer, type ComposerProps } from "./composer.tsx";
import { RoomScopeProvider } from "./room-scope-provider.tsx";
import { Thread } from "./thread.tsx";
import { WorkingLine } from "./working-line.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
// happy-dom has no font loading: the faces are there from the start
if (!("fonts" in document)) {
  Object.defineProperty(document, "fonts", {
    value: { ready: Promise.resolve(), status: "loaded", load: async () => [] },
  });
}

let host: HTMLDivElement | null = null;
let root: ReturnType<typeof createRoot> | null = null;

async function mount(node: ReactNode) {
  showDisplay({ ...DEFAULT_DISPLAY, motion: "reduced" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(
      <StrictMode>
        <TooltipProvider>
          <RoomScopeProvider>{node}</RoomScopeProvider>
        </TooltipProvider>
      </StrictMode>,
    ),
  );
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  showDisplay(DEFAULT_DISPLAY);
});

const txt = (n: Element | null | undefined) => (n?.textContent ?? "").replace(/\u00a0/g, " ");
const composer = (p: Partial<ComposerProps>) => (
  <Composer took={null} plan="Uses your Claude plan" hint={null} {...p} />
);

test("the field's size observer leaves its height alone while its width holds", async () => {
  // a second delivery after the fit, as the browser makes when the fit has resized the form, must settle
  const seen: { cb: ResizeObserverCallback; targets: Element[] }[] = [];
  vi.stubGlobal(
    "ResizeObserver",
    class {
      targets: Element[] = [];
      constructor(cb: ResizeObserverCallback) {
        seen.push({ cb, targets: this.targets });
      }
      observe(t: Element) {
        this.targets.push(t);
      }
      unobserve() {}
      disconnect() {}
    },
  );
  try {
    await mount(composer({ text: "What did the bank know?" }));
    const area = host?.querySelector("textarea");
    expect(area).not.toBeNull();
    if (area === null || area === undefined) return;
    const own = seen.filter((o) => o.targets.includes(area));
    expect(own.length).toBeGreaterThan(0);
    area.style.height = "130px";
    for (const o of own) o.cb([], {} as ResizeObserver);
    expect(area.style.height).toBe("130px");
  } finally {
    vi.unstubAllGlobals();
  }
});

test("a note under the field never blocks Ask", async () => {
  await mount(
    composer({
      text: "What did the bank know?",
      notice: <p className="test-note">Your plan is running low.</p>,
    }),
  );
  expect(txt(document.querySelector(".test-note"))).toBe("Your plan is running low.");
  expect(document.querySelector<HTMLButtonElement>(".room-field-ask")?.disabled).toBe(false);
});

test("Ask waits for words in the field", async () => {
  await mount(composer({ text: "  " }));
  expect(document.querySelector<HTMLButtonElement>(".room-field-ask")?.disabled).toBe(true);
});

test("while Clauseo answers, the field's row holds Add papers and Stop alone; its other acts stand above it", async () => {
  const onStop = vi.fn();
  await mount(
    composer({
      busy: true,
      onStop,
      acts: (
        <div className="test-acts">
          <button type="button">Queue</button>
        </div>
      ),
    }),
  );
  const row = document.querySelector(".room-field");
  expect(row?.querySelectorAll("button")).toHaveLength(2);
  const stop = row?.querySelector<HTMLButtonElement>("[aria-label='Stop the answer']");
  expect(stop).not.toBeNull();
  expect(row?.querySelector(".test-acts")).toBeNull();
  await act(async () => stop?.click());
  expect(onStop).toHaveBeenCalledOnce();
});

test("the room's own placeholder and line replace the matter's", async () => {
  await mount(
    composer({ placeholder: "Ask a quick question", line: "The scratch matter's own line." }),
  );
  expect(document.querySelector<HTMLTextAreaElement>(".room-field textarea")?.placeholder).toBe(
    "Ask a quick question",
  );
  expect(txt(document.querySelector(".room-line"))).toBe("The scratch matter's own line.");
});

test("without its own line the composer says what the Safe workspace does", async () => {
  await mount(composer({}));
  expect(txt(document.querySelector(".room-line"))).toContain(
    "Clauseo works inside this matter's folder",
  );
});

test("questions are offered over an empty field only, and a press picks one", async () => {
  const onPick = vi.fn();
  const prompts = {
    label: "Questions to ask",
    items: [{ text: "When was the notice sent?", onPick }],
  };
  await mount(composer({ text: "", prompts }));
  const prompt = document.querySelector<HTMLButtonElement>(".room-composer .room-prompt");
  expect(txt(prompt)).toBe("When was the notice sent?");
  await act(async () => prompt?.click());
  expect(onPick).toHaveBeenCalledOnce();
  await act(async () => root?.unmount());
  host?.remove();
  await mount(composer({ text: "Something typed", prompts }));
  expect(document.querySelector(".room-composer .room-prompts")).toBeNull();
});

test("Clauseo stopped: the working line says so, and Restart asks for it again", async () => {
  const onRestart = vi.fn();
  await mount(<WorkingLine state={{ kind: "stopped", onRestart }} />);
  const line = document.querySelector(".working");
  expect(line?.getAttribute("data-kind")).toBe("stopped");
  expect(txt(line)).toContain("Clauseo stopped. Your question and the answer so far are kept.");
  const restart = [...document.querySelectorAll<HTMLButtonElement>("button")].find(
    (b) => txt(b) === "Restart",
  );
  await act(async () => restart?.click());
  expect(onRestart).toHaveBeenCalledOnce();
});

test("a series counts the judgments and names the one being checked, with its expected time", async () => {
  await mount(
    <WorkingLine
      state={{ kind: "series", total: 3, current: "Acme Ltd v. State", expected: "4:40 pm" }}
    />,
  );
  const line = document.querySelector(".working");
  expect(line?.getAttribute("data-kind")).toBe("series");
  expect(txt(line)).toContain(
    "Checking 3 judgments against later decisions. Now Acme Ltd v. State.",
  );
  expect(txt(line)).toContain("Expected by 4:40 pm");
});

test("the plan's limit says when it resets, and the question waits for then", async () => {
  await mount(<WorkingLine state={{ kind: "limit", resets: "6:10 pm" }} />);
  expect(txt(document.querySelector(".working"))).toContain(
    "Claude plan limit reached. It resets at 6:10 pm. Your question is saved.",
  );
  expect(txt(document.querySelector(".working"))).toContain("Asking at 6:10 pm");
});

test("a wait in the caller's words has no pill and no actions unless given", async () => {
  await mount(
    <WorkingLine
      state={{ kind: "wait", words: "Finish signing in in your browser.", pill: null }}
    />,
  );
  const line = document.querySelector(".working");
  expect(txt(line)).toBe("Finish signing in in your browser.");
  expect(line?.querySelector(".working-time")).toBeNull();
  expect(line?.querySelector(".working-acts")).toBeNull();
});

test("the thread names itself and holds the question for its pin, hidden at rest", async () => {
  await mount(
    <Thread question="What did the bank know?" who="You">
      <p className="test-answer">The bank had notice.</p>
    </Thread>,
  );
  const log = document.querySelector(".room-thread");
  expect(log?.getAttribute("role")).toBe("log");
  expect(log?.getAttribute("aria-label")).toBe("This matter's thread");
  expect(log?.querySelector(".test-answer")).not.toBeNull();
  const pin = document.querySelector<HTMLButtonElement>(".room-pin");
  expect(pin?.hidden).toBe(true);
  expect(txt(pin)).toContain("What did the bank know?");
  expect(txt(pin)).toContain("Scroll back to it");
});
