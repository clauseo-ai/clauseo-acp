// @vitest-environment happy-dom
// The reader's parts, rendered with invented data: the judgment reader and the pane beside it, Indian Kanoon's results,
// its guard and its absence, and the CCI and NCLAT index. Each renders in English and Hindi, says Indian Kanoon's line
// where its results or text show, and hands the lawyer's actions to the app. The draft in review has its own file.
import { act, createRef, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { TooltipProvider } from "../ui/tooltip.tsx";
import { DEFAULT_DISPLAY, showDisplay, type Lang } from "../lib/display.ts";
import { readerEnglish, readerHindi } from "../lib/words-reader.ts";
import { IkUnavailable, LookupGuard, ResearchResults } from "./ik.tsx";
import { JudgmentPane, IndexPane } from "./judgment-pane.tsx";
import { IndexReader, JudgmentReader } from "./judgment-reader.tsx";
import type { IndexEntry, Judgment, ReadAlready, Result } from "./judgments.ts";
import { MatterContext } from "./matter-context.ts";
import type { Cite } from "./matter.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/* ---------- invented data ---------- */

const ACME: Judgment = {
  id: "acme",
  short: "Acme Ltd",
  name: "Acme Ltd v. State",
  court: "Supreme Court",
  bench: 3,
  decided: "1 January 2001",
  stance: "for",
  reports: { scc: "(2001) 1 ABC 1", sccAt: "5", air: "AIR 2001 XY 10", airAt: "12" },
  parties: [
    { name: "Acme Ltd", role: "Appellant" },
    { name: "State and another", role: "Respondents" },
  ],
  quoted: { para: 3, words: ["the notice must name the debt", "a bare demand is not enough"] },
  paras: [
    { n: 1, text: "The appeal arises from a demand made by letter." },
    { n: 2, text: "The question is what a notice must say." },
    {
      n: 3,
      text: "We hold that the notice must name the debt, and that a bare demand is not enough.",
    },
    { n: 4, text: "The appeal is allowed." },
  ],
  citedBy: 1200,
  since: [
    {
      id: "beta",
      short: "Beta Traders",
      cite: "(2002) 3 ABC 30",
      decided: "2 February 2002",
      bench: 2,
      treatment: "followed",
      note: "Followed for the notice.",
    },
    {
      id: "gamma",
      short: "Gamma Co",
      cite: "(2008) 4 ABC 40",
      decided: "3 March 2008",
      bench: 2,
      treatment: "followed",
      note: "Followed.",
    },
    {
      id: "delta",
      short: "Delta Works",
      cite: "(2010) 5 ABC 50",
      decided: "4 April 2010",
      bench: 2,
      treatment: "distinguished",
      note: "Distinguished on its facts.",
    },
  ],
  more: { followed: 40, distinguished: 6 },
  read: "4:31 pm",
};

const RESULTS: Result[] = [
  {
    id: "beta",
    short: "Beta Traders",
    name: "Beta Traders v. Union",
    cite: "(2002) 3 ABC 30",
    decided: "2 February 2002",
    bench: 2,
    stance: "for",
    fragment: "the **notice** named no debt",
    read: "4:38 pm",
  },
  {
    id: "delta",
    short: "Delta Works",
    name: "Delta Works v. Board",
    cite: "(2010) 5 ABC 50",
    decided: "4 April 2010",
    bench: 2,
    stance: "against",
    fragment: "the letter was a **notice**",
    read: null,
  },
];

const READ: ReadAlready[] = [
  { id: "acme", short: "Acme Ltd", cite: "(2001) 1 ABC 1", read: "4:31 pm" },
  { id: "beta", short: "Beta Traders", cite: "(2002) 3 ABC 30", read: "4:38 pm" },
];

const ENTRY: IndexEntry = {
  title: "Example Forum v. Sample Suppliers",
  rows: [
    { label: "Case", value: "CCI Case No. 1 of 2020" },
    { label: "Outcome", value: "Closed." },
  ],
};

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
const reader = (onBack = () => {}) => (
  <>
    <JudgmentReader judgment={ACME} open from={null} backRef={createRef()} onBack={onBack} />
    <JudgmentPane judgment={ACME} offline={false} opens={(id) => id === "beta"} onOpen={() => {}} />
  </>
);

/* ---------- the judgment reader ---------- */

test("the reader: SCC first, then AIR, the quoted paragraph marked, who has followed it, and back to the answer", async () => {
  const onBack = vi.fn();
  await mount(reader(onBack));
  const cited = all(".rd-cited-row").map((r) => r.textContent?.replace(/\u00A0/g, " "));
  expect(cited).toEqual(["SCC(2001) 1 ABC 1, at 5, para 3", "AIRAIR 2001 XY 10, at 12, para 3"]);
  expect(text(".rd-para[data-quoted] .rd-num")).toBe("3");
  expect(all(".rd-quoted mark")).toHaveLength(2);
  expect(all('.rd-since-row[data-treatment="followed"]')).toHaveLength(2);
  expect(all('.rd-since-row[data-treatment="distinguished"]')).toHaveLength(1);
  expect(text(".rd-since-more")).toContain("No judgment has doubted it.");
  expect(text(".rd-checked")).toContain("1,200 judgments");
  // only a judgment the reader has opens from the list since
  expect(all(".rd-since-open").map((b) => b.textContent)).toEqual(["Beta Traders"]);
  const back = all(".rd-bar button").find((b) => b.textContent?.includes("Back to the answer"));
  await click(back);
  expect(onBack).toHaveBeenCalledOnce();
});

test("the reader closes once it is shut, and says powered by IKanoon at the pane's foot, not in its bar", async () => {
  vi.useFakeTimers();
  await mount(reader());
  expect(text(".rd-bar")).not.toContain("powered by");
  expect(text(".rd-pane-foot")).toContain("powered by IKanoon");
  expect(document.body.textContent).toMatch(/3 judgments since 2001/);
  await act(async () =>
    root?.render(
      <TooltipProvider>
        <JudgmentReader
          judgment={ACME}
          open={false}
          from={null}
          backRef={createRef()}
          onBack={() => {}}
        />
      </TooltipProvider>,
    ),
  );
  await act(async () => vi.runAllTimers());
  expect(document.querySelector(".rd-reader")).toBeNull();
});

test("the pane gives the matter's own check note when the matter has the judgment, and says offline where it came from", async () => {
  const cite: Cite = {
    short: "Acme Ltd",
    name: "Acme Ltd v. State",
    cite: "(2001) 1 ABC 1",
    decided: "1 January 2001",
    court: "Supreme Court",
    forum: "court",
    bench: 3,
    pin: "para 3",
    stance: "for",
    check: {
      waitingFor: "judgments since 2001 that cite it",
      result: "Good law",
      note: "Still followed on notices.",
      ms: 900,
    },
  };
  await mount(
    <MatterContext value={{ papers: [], cites: { acme: cite }, citations: [] }}>
      <JudgmentPane judgment={ACME} offline opens={() => false} onOpen={() => {}} />
    </MatterContext>,
  );
  expect(text(".rd-check")).toContain("Still followed on notices.");
  expect(text(".rd-read")).toBe(
    "Kept on this computer. It opens while Indian Kanoon is unavailable.",
  );
  expect(document.querySelector(".rd-since-open")).toBeNull();
});

test("each region of the reader has a name of its own", async () => {
  await mount(reader());
  const names = all("section[aria-label], [role='region'][aria-label]")
    .filter((n) => n.getAttribute("role") !== "group")
    .map((n) => n.getAttribute("aria-label"));
  expect(names.length).toBeGreaterThan(0);
  expect(new Set(names).size).toBe(names.length);
});

/* ---------- Indian Kanoon ---------- */

test("research results open in the reader; while Indian Kanoon is down, only what was read opens", async () => {
  const onOpen = vi.fn();
  await mount(<ResearchResults results={RESULTS} highCourts={7} offline onOpen={onOpen} />);
  expect(all(".ik-result")).toHaveLength(2);
  expect(text(".ik-results")).toContain("powered by IKanoon");
  expect(text(".ik-results-fold")).toContain("Show the 7 High Court judgments");
  const unread = all(".ik-result").find((r) => r.textContent?.includes("Delta Works"));
  const button = unread?.querySelector("button");
  expect(
    button?.hasAttribute("disabled") === true || button?.getAttribute("data-disabled") !== null,
  ).toBe(true);
  expect(unread?.textContent).toContain("Opens when Indian Kanoon answers");
  expect(all(".ik-result-fragment mark").map((m) => m.textContent)).toEqual(["notice", "notice"]);
  await click(all(".ik-result")[0]?.querySelector("button"));
  expect(onOpen).toHaveBeenCalledWith("beta", expect.anything());
});

test("the guard asks before going past 300 lookups, gives Continue's cost in time, and folds once answered", async () => {
  const onAnswer = vi.fn();
  const so = { searches: 120, read: 90, fit: 12 };
  await mount(
    <LookupGuard state="pending" at="4:39 pm" so={so} expected="4:44 pm" onAnswer={onAnswer} />,
  );
  expect(text(".ik-guard")).toContain(
    "This research has used 300 Indian Kanoon lookups. Continue?",
  );
  expect(text(".ik-guard")).toContain(
    "So far 120 searches and 90 judgments read. 12 judgments fit.",
  );
  expect(text(".ik-guard-when")).toBe("Continuing, expected by 4:44 pm");
  await click(one(".ik-guard-go"));
  expect(onAnswer).toHaveBeenCalledWith("continued");
  await act(async () =>
    root?.render(
      <TooltipProvider>
        <LookupGuard state="continued" at="4:39 pm" so={so} onAnswer={onAnswer} />
      </TooltipProvider>,
    ),
  );
  expect(document.querySelector(".ik-guard")).toBeNull();
  expect(text(".ik-guard-done")).toContain("Continued past 300 lookups at 4:39 pm.");
  expect(document.querySelector(".ik-guard-when")).toBeNull();
});

test("Indian Kanoon unavailable: when it tries again, and the judgments already read still open", async () => {
  const onOpen = vi.fn();
  await mount(<IkUnavailable retry="4:52 pm" read={READ} onOpen={onOpen} />);
  expect(text(".ik-down-title")).toBe("Indian Kanoon is unavailable");
  expect(text(".ik-down-body")).toContain("4:52 pm");
  expect(all(".ik-down-row")).toHaveLength(2);
  expect(document.body.textContent).toContain("powered by IKanoon");
  await click(one(".ik-down-row button"));
  expect(onOpen).toHaveBeenCalledWith("acme", expect.anything());
});

test("the CCI and NCLAT index: its snapshot's date, its size, and an index, not a record", async () => {
  await mount(
    <>
      <IndexReader
        entry={ENTRY}
        snapshot="1 June 2026"
        open
        from={null}
        backRef={createRef()}
        onBack={() => {}}
      />
      <IndexPane snapshot="1 June 2026" size={{ cases: 2900, appeals: 425 }} />
    </>,
  );
  expect(text(".rd-index")).toContain("Snapshot of 1 June 2026");
  expect(text(".rd-index-note")).toBe(
    "An index, not a record. Read the order itself before relying on it.",
  );
  expect(all(".rd-index-row dt").map((d) => d.textContent)).toEqual(["Case", "Outcome"]);
  expect(text(".rd-about")).toContain(
    "About 2,900 CCI cases and 425 NCLAT appeals, kept on this computer.",
  );
});

/* ---------- Hindi, and the words ---------- */

test.each(["en", "hi"] as const)("the reader's parts render in %s", async (lang) => {
  await mount(
    <>
      {reader()}
      <ResearchResults results={RESULTS} highCourts={7} offline={false} onOpen={() => {}} />
    </>,
    lang,
  );
  const words = document.body.textContent ?? "";
  if (lang === "hi") {
    expect(words).toMatch(/[\u0900-\u097F]/);
    expect(words).not.toMatch(/पेंसिल|स्याही|चरण|कदम|(^|\s)मैं(\s|$)/);
  } else {
    expect(words).not.toMatch(/[\u0900-\u097F]/);
  }
  expect(words).toContain("powered by IKanoon");
});

test("no reader string says I, names a model, or uses a dash or a banned word", () => {
  expect(readerEnglish.length).toBe(readerHindi.length);
  for (const s of readerEnglish) {
    expect(s, s).not.toMatch(
      /\bI\b|\bI'm\b|[\u2013\u2014]|\b(pencil|ink|step|git|token|session|terminal|CLI|adapter|thread|safe)\b|agent process|Opus|Sonnet|GPT-\d/i,
    );
  }
  for (const s of readerHindi) {
    expect(s, s).not.toMatch(/पेंसिल|स्याही|चरण|कदम|(^|\s)मैं(\s|$)|[\u2013\u2014]|[\u0966-\u096F]/);
    expect(s, s).not.toMatch(/[\u0900-\u097F]\.$/);
  }
});
