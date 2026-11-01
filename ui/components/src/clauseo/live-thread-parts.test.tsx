// @vitest-environment happy-dom
// The parts the live thread uses, with the props it added: the composer's test ids, its disabled state and no room's
// line; the working line's error code; the change note's papers, its acts only when given, its refused undo and its
// dialog; Who without a time.
import { act, StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { TooltipProvider } from "../ui/tooltip.tsx";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { QuestionLine, Who } from "./answer.tsx";
import { ChangeNote } from "./change-note.tsx";
import { Composer, type ComposerProps } from "./composer.tsx";
import { RoomScopeProvider } from "./room-scope-provider.tsx";
import { WorkingLine } from "./working-line.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
if (!("fonts" in document)) {
  Object.defineProperty(document, "fonts", {
    value: { ready: Promise.resolve(), status: "loaded", load: async () => [] },
  });
}

let host: HTMLDivElement | null = null;
let root: ReturnType<typeof createRoot> | null = null;

async function mount(node: ReactNode, lang: "en" | "hi" = "en") {
  showDisplay({ ...DEFAULT_DISPLAY, lang, motion: "reduced" });
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
const byId = (id: string) => document.querySelector<HTMLElement>(`[data-testid="${id}"]`);
const composer = (p: Partial<ComposerProps>) => (
  <Composer took={null} plan="Uses your Claude plan" hint={null} {...p} />
);
const IDS = { field: "composer", ask: "send", stop: "cancel", working: "error" };
const button = (label: string) =>
  [...document.querySelectorAll<HTMLButtonElement>("button")].find((b) => txt(b) === label);

test("the composer's test ids land on the field, Ask and the working line's row", async () => {
  await mount(
    composer({
      text: "What did the bank know?",
      testIds: IDS,
      working: { kind: "wait", words: "Answering.", pill: null },
    }),
  );
  expect(byId("composer")?.tagName).toBe("TEXTAREA");
  expect(byId("send")?.getAttribute("type")).toBe("submit");
  expect(byId("error")?.classList.contains("working")).toBe(true);
  expect(byId("cancel")).toBeNull();
});

test("while busy the Stop test id lands on Stop", async () => {
  const onStop = vi.fn();
  await mount(composer({ busy: true, onStop, testIds: IDS }));
  expect(byId("cancel")?.getAttribute("aria-label")).toBe("Stop the answer");
  expect(byId("send")).toBeNull();
  await act(async () => byId("cancel")?.click());
  expect(onStop).toHaveBeenCalledOnce();
});

test("disabled, the field, Add papers and Ask take nothing, whatever the field holds", async () => {
  await mount(composer({ text: "What did the bank know?", disabled: true, testIds: IDS }));
  expect((byId("composer") as HTMLTextAreaElement | null)?.disabled).toBe(true);
  expect((byId("send") as HTMLButtonElement | null)?.disabled).toBe(true);
  expect(document.querySelector<HTMLButtonElement>(".room-field-add")?.disabled).toBe(true);
});

test("line={false} leaves no room's line at all", async () => {
  await mount(composer({ line: false }));
  expect(document.querySelector(".room-line")).toBeNull();
  expect(document.body.textContent).not.toContain("Safe workspace");
});

test("a wait with a code sets it as the row's data-code; without one there is none", async () => {
  await mount(
    <>
      <WorkingLine
        testId="error"
        state={{ kind: "wait", words: "The disk is full.", pill: null, code: "store.disk_full" }}
      />
      <WorkingLine testId="plain" state={{ kind: "wait", words: "Answering.", pill: null }} />
    </>,
  );
  expect(byId("error")?.getAttribute("data-code")).toBe("store.disk_full");
  expect(byId("plain")?.hasAttribute("data-code")).toBe(false);
});

const PAPERS = ["Notice to the bank.docx", "Reply, draft 2.docx"];
const NOTE_IDS = { paper: "file-changed", undo: "undo", error: "error" };

test("each paper stands in its own span, its name isolated, separated by a comma", async () => {
  await mount(<ChangeNote papers={PAPERS} testIds={NOTE_IDS} onUndo={() => {}} />);
  const spans = [...document.querySelectorAll('[data-testid="file-changed"]')];
  expect(spans.map((s) => txt(s))).toEqual(PAPERS);
  expect(spans.every((s) => s.querySelector("bdi") !== null)).toBe(true);
  expect(spans.every((s) => s.parentElement?.classList.contains("change-papers"))).toBe(true);
  expect(txt(document.querySelector(".change-papers"))).toBe(PAPERS.join(", "));
});

test("Compare and Undo show only when given", async () => {
  await mount(<ChangeNote papers={PAPERS} testIds={NOTE_IDS} />);
  expect(button("Compare")).toBeUndefined();
  expect(byId("undo")).toBeNull();
  await act(async () => root?.unmount());
  host?.remove();
  await mount(<ChangeNote papers={PAPERS} testIds={NOTE_IDS} onCompare={() => {}} />);
  expect(button("Compare")).toBeDefined();
  expect(byId("undo")).toBeNull();
});

test("Undo can be held off while Clauseo works", async () => {
  const onUndo = vi.fn();
  await mount(<ChangeNote papers={PAPERS} testIds={NOTE_IDS} onUndo={onUndo} undoDisabled />);
  expect((byId("undo") as HTMLButtonElement | null)?.disabled).toBe(true);
  await act(async () => byId("undo")?.click());
  expect(onUndo).not.toHaveBeenCalled();
});

test("given a state, Undo leaves it to the host to say when the changes are undone", async () => {
  const onUndo = vi.fn();
  await mount(<ChangeNote papers={PAPERS} state="changed" testIds={NOTE_IDS} onUndo={onUndo} />);
  await act(async () => byId("undo")?.click());
  expect(onUndo).toHaveBeenCalledOnce();
  expect(document.querySelector(".change-undone")).toBeNull();
  expect(byId("undo")).not.toBeNull();
});

test("a refused undo says why beside Undo, with its code", async () => {
  await mount(
    <ChangeNote
      papers={PAPERS}
      testIds={NOTE_IDS}
      onUndo={() => {}}
      error={{ words: "A paper is open in another program.", code: "matter.file_locked" }}
    />,
  );
  const line = byId("error");
  expect(line?.getAttribute("role")).toBe("alert");
  expect(line?.getAttribute("data-code")).toBe("matter.file_locked");
  expect(txt(line)).toBe("A paper is open in another program.");
  expect(line?.parentElement).toBe(byId("undo")?.parentElement);
});

test("one paper reads in the singular, changed and undone, in both languages", async () => {
  await mount(<ChangeNote papers={["Notice.docx"]} />);
  expect(txt(document.querySelector(".change-what"))).toBe("Clauseo changed 1 paper.");
  await act(async () => root?.unmount());
  host?.remove();
  await mount(<ChangeNote papers={["Notice.docx"]} state="undone" />, "hi");
  expect(txt(document.querySelector(".change-undone"))).toBe(
    "आपने 1 दस्तावेज़ में Clauseo के बदलाव पूर्ववत किए।",
  );
});

async function openDialog(onUndo: () => void) {
  await mount(
    <ChangeNote
      papers={PAPERS}
      editedSince="Notice to the bank.docx"
      state="changed"
      testIds={NOTE_IDS}
      onUndo={onUndo}
    />,
  );
  await act(async () => byId("undo")?.click());
  expect(onUndo).not.toHaveBeenCalled();
}

test("with a paper edited since, Undo asks first, and Undo anyway undoes", async () => {
  const onUndo = vi.fn();
  await openDialog(onUndo);
  await act(async () => button("Undo anyway")?.click());
  expect(onUndo).toHaveBeenCalledOnce();
});

test("with a paper edited since, Keep current sends nothing", async () => {
  const onUndo = vi.fn();
  await openDialog(onUndo);
  const keep = button("Keep current");
  expect(keep).toBeDefined();
  await act(async () => keep?.click());
  expect(onUndo).not.toHaveBeenCalled();
});

test("Who without a time says the name alone, no comma; with one, the name and the time", async () => {
  await mount(
    <>
      <QuestionLine who="You" text="What did the bank know?" />
      <Who name="Clauseo" time="4:31 pm" />
    </>,
  );
  const who = [...document.querySelectorAll(".who")].map((n) => txt(n));
  expect(who).toEqual(["You", "Clauseo, 4:31 pm"]);
});
