// @vitest-environment happy-dom
// A draft in review, rendered with invented data: the sheet with its tracked changes, the changes beside it with
// Accept and Reject, the citations with what the check found, and Word holding the file. The parts show what the
// review says and hand the lawyer's actions to the app.
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { TooltipProvider } from "../ui/tooltip.tsx";
import { DEFAULT_DISPLAY, showDisplay, type Lang } from "../lib/display.ts";
import { DraftPane } from "./draft-pane.tsx";
import { DraftSheet, WordLockDialog } from "./draft-review.tsx";
import type { ChangeState, Draft, Review } from "./judgments.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/* ---------- invented data ---------- */

const DRAFT: Draft = {
  file: "Notice to the bank.docx",
  title: "Reply to the bank",
  court: "Before the Example Tribunal",
  caseLine: "Application No. 1 of 2026",
  parties: [
    { name: "Acme Ltd", role: "Applicant" },
    { name: "Example Bank", role: "Respondent" },
  ],
  heading: "Reply",
  paras: [
    "1. The notice went to the bank on 2 March.",
    "2. The bank replied{+c1: within a week+} and {-c2:refused-}{+c2:declined, relying on [[acme]]+}.",
    "{+y1:3. The reply cites [[beta]], [[gamma]] and [[eps]].+}",
    "4. The applicant asks that the demand be withdrawn{+y2:, following [[delta]]+}.",
  ],
  cites: [
    { id: "acme", text: "Acme Ltd, (2001) 1 ABC 1", status: "verified", by: "clauseo" },
    {
      id: "beta",
      text: "Beta Traders, (2005) 3 ABC 30",
      status: "mismatched",
      by: "you",
      mismatch: { draft: "any letter will do", para: 3, source: "A bare demand is not enough." },
    },
    {
      id: "gamma",
      text: "Gamma Co, (2008) 4 ABC 44",
      status: "notfound",
      by: "you",
      notFound: { cited: "(2008) 4 ABC 44", name: "Gamma Co", right: "(2008) 4 ABC 40" },
    },
    { id: "eps", text: "Epsilon v. Board, order of 1 May 2024", status: "unverifiable", by: "you" },
    {
      id: "delta",
      text: "Delta Works, (2010) 5 ABC 50",
      status: "unchecked",
      by: "you",
      addedAt: "4:52 pm",
    },
  ],
  changes: [
    { id: "c1", by: "clauseo", at: "4:31 pm" },
    { id: "c2", by: "clauseo", at: "4:31 pm" },
    { id: "y1", by: "you", at: "4:47 pm" },
    { id: "y2", by: "you", at: "4:52 pm" },
  ],
  checkedAt: "4:49 pm",
  editsAt: "4:52 pm",
};

const pending: ChangeState = { state: "pending", at: null };
const review = (over: Partial<Review> = {}): Review => ({
  changes: { c1: pending, c2: pending, y1: pending, y2: pending },
  edits: false,
  focus: null,
  statuses: {},
  ...over,
});

/* ---------- rendering ---------- */

let host: HTMLDivElement | null = null;
let root: Root | null = null;

async function mount(node: ReactNode, lang: Lang = "en") {
  showDisplay({ ...DEFAULT_DISPLAY, lang, motion: "reduced" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<TooltipProvider>{node}</TooltipProvider>));
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  for (const n of document.body.querySelectorAll("[data-base-ui-portal], [role=dialog]"))
    n.remove();
  showDisplay(DEFAULT_DISPLAY);
  vi.useRealTimers();
});

const one = <T extends Element = HTMLElement>(selector: string) => {
  const n = document.querySelector<T>(selector);
  if (n === null) throw new Error(`no ${selector}`);
  return n;
};
const all = (selector: string) => [...document.querySelectorAll<HTMLElement>(selector)];
const text = (selector: string) => one(selector).textContent?.replace(/\u00A0/g, " ") ?? "";
const click = (el: Element | null | undefined) =>
  act(async () => (el as HTMLElement | null)?.click());
/* ---------- the draft in review ---------- */

const pane = (
  r: Review,
  tab: "changes" | "citations",
  on: { onAct?: () => void; onCheck?: (id: string) => void } = {},
) => (
  <>
    <DraftSheet draft={DRAFT} review={r} tab={tab} onFocus={() => {}} />
    <DraftPane
      draft={DRAFT}
      review={r}
      tab={tab}
      now="4:46 pm"
      onTab={() => {}}
      onAct={on.onAct ?? (() => {})}
      onCheck={on.onCheck ?? (() => {})}
      onFocus={() => {}}
    />
  </>
);

test("Word: each change with its author and time, and Accept and Reject handed to the app", async () => {
  const onAct = vi.fn();
  await mount(pane(review(), "changes", { onAct }));
  const cards = all(".rd-change");
  expect(cards).toHaveLength(2);
  expect(cards[0]?.textContent).toContain("Clauseo");
  expect(cards[0]?.textContent).toContain("Today, 4:31 pm");
  expect(cards[0]?.textContent).toContain("Not yet accepted");
  await click(cards[0]?.querySelector(".rd-accept"));
  expect(onAct).toHaveBeenLastCalledWith(["c1"], "accept");
  await click(one(".rd-accept-all"));
  expect(onAct).toHaveBeenLastCalledWith(["c1", "c2"], "accept");
  // the lawyer's own paragraph isn't on the sheet until their edits come back, and the next one closes up
  expect(text(".rd-draft")).not.toContain("The reply cites");
  expect(text(".rd-draft")).toContain("3. The applicant asks");
});

test("Word: accepted and rejected changes settle on the sheet, and with all accepted no marks are left", async () => {
  await mount(
    pane(
      review({
        changes: {
          c1: { state: "accepted", at: "4:46 pm" },
          c2: { state: "rejected", at: "4:46 pm" },
          y1: pending,
          y2: pending,
        },
      }),
      "changes",
    ),
  );
  expect(text('.rd-change[data-state="accepted"]')).toContain("Accepted 4:46 pm");
  expect(text('.rd-change[data-state="rejected"]')).toContain("Rejected 4:46 pm");
  expect(all(".rd-draft .rd-acc")).toHaveLength(1);
  expect(text(".rd-draft")).toContain("refused");
  expect(text(".rd-draft")).not.toContain("declined");
  await act(async () => root?.unmount());
  const done: ChangeState = { state: "accepted", at: "4:50 pm" };
  await mount(
    pane(review({ changes: { c1: done, c2: done, y1: done, y2: done }, edits: true }), "changes"),
  );
  expect(all('.rd-change[data-state="accepted"]')).toHaveLength(4);
  expect(all(".rd-draft ins, .rd-draft del")).toHaveLength(0);
});

test("Word: the lawyer's own edits come back at the lawyer's weight, and an insertion leads with the words before it", async () => {
  await mount(pane(review({ edits: true }), "changes"));
  expect(text(".rd-came-back")).toBe("Your edits in Word came back at 4:52 pm. 2 changes, by you.");
  expect(all('.rd-change[data-by="you"]')).toHaveLength(2);
  // newest first
  expect(all(".rd-change")[0]?.textContent).toContain("Today, 4:52 pm");
  expect(all('.rd-draft .rd-ins[data-by="you"]').length).toBeGreaterThanOrEqual(2);
  expect(text(".rd-draft")).toContain("4. The applicant asks");
  const runs = all(".rd-change-run[data-kind='ins']");
  for (const r of runs) {
    const words = r.querySelector("ins")?.textContent ?? "";
    if (/^[,;.]/.test(words))
      expect(r.querySelector(".rd-change-ctx")?.textContent ?? "").not.toBe("");
  }
  expect(all(".rd-change-ctx").length).toBeGreaterThan(0);
});

test("the citation checks: five statuses, a mismatch with its source passage, a report not found, and Check again", async () => {
  const onCheck = vi.fn();
  await mount(pane(review({ edits: true }), "citations", { onCheck }));
  expect(new Set(all(".rd-checkrow").map((r) => r.dataset["status"]))).toEqual(
    new Set(["verified", "mismatched", "notfound", "unverifiable", "unchecked"]),
  );
  expect(text('.rd-checkrow[data-status="mismatched"]')).toContain("Paragraph 3 says");
  expect(text('.rd-checkrow[data-status="notfound"]')).toContain(
    "Indian Kanoon has no judgment at (2008) 4 ABC 44. Gamma Co is reported at (2008) 4 ABC 40.",
  );
  expect(text('.rd-checkrow[data-status="unchecked"]')).toContain(
    "Added in Word at 4:52 pm, after the last check.",
  );
  expect(text(".rd-review-sub")).toContain("Checked 4 citations at 4:49 pm.");
  expect(text(".rd-review-sub")).toContain("None of these checks holds back saving or accepting.");
  expect(all(".rd-draft .rd-cite[data-status]")).toHaveLength(5);
  await click(one(".rd-check-again"));
  expect(onCheck).toHaveBeenCalledWith("delta");
  await act(async () => root?.unmount());
  await mount(pane(review({ edits: true, statuses: { delta: "checking" } }), "citations"));
  expect(document.querySelector('.rd-checkrow[data-status="checking"]')).not.toBeNull();
  expect(document.querySelector('.rd-checkrow[data-status="unchecked"]')).toBeNull();
});

test("before the lawyer's edits come back, only Clauseo's citations are checked", async () => {
  await mount(pane(review(), "citations"));
  expect(all(".rd-checkrow").map((r) => r.dataset["status"])).toEqual(["verified"]);
});

test("Word holds the file: close it to accept, and Try again goes to the app", async () => {
  const onTryAgain = vi.fn();
  await mount(
    <WordLockDialog
      file={DRAFT.file}
      open
      act="accept"
      onClose={() => {}}
      onTryAgain={onTryAgain}
    />,
  );
  const dialog = one('[role="dialog"]');
  expect(dialog.textContent).toContain(
    "Close 'Notice to the bank.docx' in Word to accept these changes.",
  );
  expect(dialog.textContent).toContain(
    "Word holds the file while it is open. Nothing has changed yet.",
  );
  await click(one(".rd-try"));
  expect(onTryAgain).toHaveBeenCalledOnce();
});

test.each(["en", "hi"] as const)("the draft's parts render in %s", async (lang: Lang) => {
  await mount(pane(review({ edits: true }), "citations"), lang);
  const words = document.body.textContent ?? "";
  if (lang === "hi") {
    expect(words).toMatch(/[\u0900-\u097F]/);
    expect(words).not.toMatch(/पेंसिल|स्याही|चरण|कदम|(^|\s)मैं(\s|$)/);
  } else {
    expect(words).not.toMatch(/[\u0900-\u097F]/);
  }
});
