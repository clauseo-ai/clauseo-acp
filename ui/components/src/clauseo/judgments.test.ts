// The reader's pure helpers: how a judgment is cited, the words a paragraph marks, a draft's markup read into runs,
// its paragraphs as they stand once a whole one drops out, and which changes and citations it holds. Invented data.
import { expect, test } from "vitest";
import {
  changesIn,
  citedAs,
  citesIn,
  draftRuns,
  firstReport,
  fragmentRuns,
  markRuns,
  minutesOf,
  standingParas,
  wholeParagraph,
  type Change,
  type DraftCite,
  type Judgment,
  type Review,
} from "./judgments.ts";

const judgment = (reports: Judgment["reports"]): Judgment => ({
  id: "acme",
  short: "Acme Ltd",
  name: "Acme Ltd v. State",
  court: "Supreme Court",
  bench: 2,
  decided: "1 January 2001",
  stance: "for",
  reports,
  parties: [
    { name: "Acme Ltd", role: "Appellant" },
    { name: "State", role: "Respondent" },
  ],
  quoted: { para: 3, words: ["a bare demand"] },
  paras: [{ n: 3, text: "A bare demand is not a notice; a bare demand is a letter." }],
  citedBy: 10,
  since: [],
  more: { followed: 1, distinguished: 0 },
  read: null,
});

test("a judgment is cited SCC first, then AIR, then any other report, each at the quoted paragraph", () => {
  const both = judgment({ scc: "(2001) 1 ABC 1", sccAt: "5", air: "AIR 2001 XY 10", airAt: "12" });
  expect(citedAs(both)).toEqual([
    { report: "SCC", cite: "(2001) 1 ABC 1", at: "5", para: 3 },
    { report: "AIR", cite: "AIR 2001 XY 10", at: "12", para: 3 },
  ]);
  expect(firstReport(both)).toBe("(2001) 1 ABC 1");
  const other = judgment({
    scc: null,
    sccAt: null,
    air: null,
    airAt: null,
    other: "2001 Weekly (XY) 7",
  });
  expect(citedAs(other)).toEqual([
    { report: "Weekly", cite: "2001 Weekly (XY) 7", at: null, para: 3 },
  ]);
  expect(firstReport(other)).toBe("2001 Weekly (XY) 7");
  expect(citedAs(judgment({ scc: null, sccAt: null, air: null, airAt: null }))).toEqual([]);
});

test("markRuns marks every occurrence of the relied-on words, and nothing when they are absent", () => {
  expect(markRuns("one two one", ["one"])).toEqual([
    { text: "one", marked: true },
    { text: " two ", marked: false },
    { text: "one", marked: true },
  ]);
  expect(markRuns("nothing here", ["absent"])).toEqual([{ text: "nothing here", marked: false }]);
  expect(fragmentRuns("a **b** c")).toEqual([
    { text: "a ", marked: false },
    { text: "b", marked: true },
    { text: " c", marked: false },
  ]);
});

test("draftRuns reads insertions, deletions and citations; a malformed mark stays as words", () => {
  expect(draftRuns("Sent{+c1: today+} to {-c2:them-}[[acme]].")).toEqual([
    { kind: "same", text: "Sent" },
    { kind: "ins", change: "c1", runs: [{ kind: "text", text: " today" }] },
    { kind: "same", text: " to " },
    { kind: "del", change: "c2", runs: [{ kind: "text", text: "them" }] },
    { kind: "cite", id: "acme" },
    { kind: "same", text: "." },
  ]);
  expect(draftRuns("no {+c1 marks here")).toEqual([{ kind: "same", text: "no {+c1 marks here" }]);
  expect(wholeParagraph("{+y1:3. A new paragraph.+}")).toBe("y1");
  expect(wholeParagraph("3. Old{+y1: words+}.")).toBeNull();
});

test("a whole paragraph that drops out takes its number with it, and the rest close up", () => {
  const paras = ["1. One.", "{+y1:2. Two.+}", "3. Three{+y2: more+}.", "4. Four."];
  expect(standingParas(paras, (c) => c === "y1")).toEqual([
    "1. One.",
    null,
    "2. Three{+y2: more+}.",
    "3. Four.",
  ]);
  expect(standingParas(paras, () => false)).toEqual(paras);
});

test("the draft holds Clauseo's changes and citations, and the lawyer's once their edits come back", () => {
  const changes: Change[] = [
    { id: "c1", by: "clauseo", at: "4:31 pm" },
    { id: "y1", by: "you", at: "4:47 pm" },
  ];
  const cites: DraftCite[] = [
    { id: "acme", text: "Acme Ltd, (2001) 1 ABC 1", status: "verified", by: "clauseo" },
    { id: "beta", text: "Beta Traders, (2002) 2 ABC 2", status: "unchecked", by: "you" },
  ];
  const review: Review = { changes: {}, edits: false, focus: null, statuses: {} };
  expect(changesIn(review, changes).map((c) => c.id)).toEqual(["c1"]);
  expect(citesIn(review, cites).map((c) => c.id)).toEqual(["acme"]);
  const back = { ...review, edits: true };
  expect(changesIn(back, changes).map((c) => c.id)).toEqual(["c1", "y1"]);
  expect(citesIn(back, cites)).toHaveLength(2);
});

test("clock times order by the minute, afternoon after morning, and a time without digits counts as none", () => {
  expect(minutesOf("4:31 pm")).toBe(16 * 60 + 31);
  expect(minutesOf("12:05 pm")).toBe(12 * 60 + 5);
  expect(minutesOf("12:05 am")).toBe(5);
  expect(minutesOf("09:15")).toBe(9 * 60 + 15);
  expect(minutesOf("soon")).toBe(0);
});
