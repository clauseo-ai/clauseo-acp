// @vitest-environment happy-dom
// A Word file's tracked changes, rendered with invented data: each with its author as recorded, its time, its kind's
// label and its words, with Accept and Reject handed to the app by the change's ids, and Accept all and Reject all by
// every listed change's; the latest decision with Undo; the host's line, with Try again where it offers one; the empty
// file's line; and every press held while a decision is on its way.
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { DEFAULT_DISPLAY, showDisplay, type Lang } from "../lib/display.ts";
import { errorWords } from "../lib/words-errors.ts";
import { rdt } from "../lib/words-reader.ts";
import { WordChanges, type TrackedChange } from "./word-changes.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const CHANGES: TrackedChange[] = [
  {
    key: "1",
    ids: [1],
    kind: "insert",
    author: "A. Mehta",
    when: "Today, 9:11 am",
    text: "within fourteen days",
  },
  {
    key: "2,3",
    ids: [2, 3],
    kind: "delete",
    author: "R. Rao",
    when: "Monday 4:05 pm",
    text: "forthwith",
  },
  {
    key: "4",
    ids: [4],
    kind: "move",
    author: "A. Mehta",
    when: "",
    text: "The letter is annexed.",
  },
  { key: "5", ids: [5], kind: "format", author: "A. Mehta", when: "Today, 9:15 am", text: null },
  {
    key: "6",
    ids: [6],
    kind: "unknown",
    author: "A. Mehta",
    when: "Today, 9:16 am",
    text: "a new kind",
  },
];

let host: HTMLDivElement | null = null;
let root: Root | null = null;

type Props = Partial<Parameters<typeof WordChanges>[0]>;
const on = { onBack: vi.fn(), onDecide: vi.fn(), onUndo: vi.fn(), onRetry: vi.fn() };
const panel = (p: Props = {}): ReactNode => (
  <WordChanges
    file="Reply to the bank.docx"
    changes={CHANGES}
    sending={false}
    decided={null}
    line={null}
    {...on}
    {...p}
  />
);

async function mount(node: ReactNode, lang: Lang = "en") {
  showDisplay({ ...DEFAULT_DISPLAY, lang, motion: "reduced" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(node));
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  showDisplay(DEFAULT_DISPLAY);
  vi.clearAllMocks();
});

const all = (selector: string) => [...document.querySelectorAll<HTMLElement>(selector)];
const one = (selector: string) => document.querySelector<HTMLElement>(selector);
const click = (el: Element | null | undefined) =>
  act(async () => (el as HTMLElement | null)?.click());

test("each change with its author, its time, its kind's label and its words; no line of the lawyer's own edits", async () => {
  await mount(panel());
  expect(one("h2")?.textContent).toBe("Tracked changes");
  expect(one(".wc-file")?.textContent).toBe("Reply to the bank.docx");
  expect(one(".rd-review-sub")?.textContent).toBe(
    "5 changes, each with its author and time, as Word shows them.",
  );
  const cards = all(".rd-change");
  expect(cards.map((c) => c.querySelector(".rd-change-head b")?.textContent)).toEqual([
    "A. Mehta",
    "R. Rao",
    "A. Mehta",
    "A. Mehta",
    "A. Mehta",
  ]);
  expect(cards.map((c) => c.querySelector(".rd-change-when")?.textContent ?? null)).toEqual([
    "Today, 9:11 am",
    "Monday 4:05 pm",
    null,
    "Today, 9:15 am",
    "Today, 9:16 am",
  ]);
  expect(cards.map((c) => c.querySelector(".rd-change-kind")?.textContent ?? null)).toEqual([
    "Added",
    "Taken out",
    "Moved",
    "Formatting changed",
    // a kind this version doesn't know: its words, with no label
    null,
  ]);
  expect(cards[0]?.querySelector("ins")?.textContent).toBe("within fourteen days");
  expect(cards[1]?.querySelector("del")?.textContent).toBe("forthwith");
  expect(cards[4]?.querySelector(".rd-change-run")?.textContent).toBe("a new kind");
  for (const c of cards) expect(c.textContent).toContain("Not yet accepted");
  expect(document.body.textContent).not.toContain("by you");
  expect(document.body.textContent).not.toContain("You, in Word");
  expect(one(".rd-review-foot")?.textContent).toBe(
    "Accepting here accepts the change in the Word file too.",
  );
  expect(cards[1]?.querySelector(".rd-accept")?.getAttribute("aria-label")).toBe(
    "Accept. Change by R. Rao, Monday 4:05 pm",
  );
});

test("Accept and Reject hand the change's ids to the app, Accept all and Reject all every listed change's", async () => {
  await mount(panel());
  await click(all(".rd-change")[1]?.querySelector(".rd-accept"));
  expect(on.onDecide).toHaveBeenLastCalledWith([2, 3], "accept");
  await click(all(".rd-change")[0]?.querySelector(".rd-reject"));
  expect(on.onDecide).toHaveBeenLastCalledWith([1], "reject");
  await click(one(".rd-accept-all"));
  expect(on.onDecide).toHaveBeenLastCalledWith([1, 2, 3, 4, 5, 6], "accept");
  await click(one(".rd-reject-all"));
  expect(on.onDecide).toHaveBeenLastCalledWith([1, 2, 3, 4, 5, 6], "reject");
  await click(one('[data-testid="word-changes-back"]'));
  expect(on.onBack).toHaveBeenCalledTimes(1);
});

test("while a decision is on its way every press is held", async () => {
  await mount(panel({ sending: true, decided: { decision: "accept", time: "9:20 am" } }));
  for (const b of all(".rd-accept, .rd-reject, .rd-accept-all, .rd-reject-all, .rd-undo"))
    await click(b);
  expect(on.onDecide).not.toHaveBeenCalled();
  expect(on.onUndo).not.toHaveBeenCalled();
});

test("the latest decision says its time beside Undo, in either language", async () => {
  await mount(panel({ decided: { decision: "accept", time: "9:20 am" } }));
  expect(one('[data-testid="word-changes-decided"] [role="status"]')?.textContent).toBe(
    "Accepted 9:20 am",
  );
  await click(one('[data-testid="word-changes-undo"]'));
  expect(on.onUndo).toHaveBeenCalledTimes(1);
  await act(async () => root?.render(panel({ decided: { decision: "reject", time: "9:21 am" } })));
  expect(one('[data-testid="word-changes-decided"]')?.textContent).toContain("Rejected 9:21 am");
});

test("the host's line by its code, with Try again only where it offers one", async () => {
  await mount(panel({ changes: null, line: { code: "word.not_docx", retry: false } }));
  expect(one('[data-testid="word-changes-line"]')?.getAttribute("role")).toBe("alert");
  expect(one('[data-testid="word-changes-line"]')?.textContent).toBe(
    errorWords("en", "word.not_docx"),
  );
  expect(one('[data-testid="word-changes-retry"]')).toBeNull();
  expect(all(".rd-change")).toHaveLength(0);
  await act(async () => root?.render(panel({ line: { code: "word.edit_failed", retry: true } })));
  expect(one('[data-testid="word-changes-line"]')?.textContent).toContain(
    "Clauseo couldn't save your decision. The file is as it was.",
  );
  await click(one('[data-testid="word-changes-retry"]'));
  expect(on.onRetry).toHaveBeenCalledTimes(1);
});

test("a file with no tracked changes says so, with no Accept all and no foot", async () => {
  await mount(panel({ changes: [] }), "hi");
  expect(rdt("hi", "No tracked changes in this file.")).not.toBe(
    "No tracked changes in this file.",
  );
  expect(one('[data-testid="word-changes-empty"]')?.textContent).toBe(
    rdt("hi", "No tracked changes in this file."),
  );
  expect(one(".rd-accept-all")).toBeNull();
  expect(one(".rd-review-foot")).toBeNull();
});
