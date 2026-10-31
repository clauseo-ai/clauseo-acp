import { describe, expect, test } from "vitest";
import { t } from "../lib/words.ts";
import { rt, roomHindi } from "../lib/words-room.ts";
import {
  acceptNext,
  fitCitations,
  nextStatusFit,
  settleCurve,
  wholeRowsWithin,
  type AcceptState,
} from "./logic.ts";
import { runs } from "../lib/runs.ts";
import {
  benchOf,
  citationFor,
  citationsOf,
  citedPaper,
  citedPapers,
  keepIds,
  pbWords,
  tileTitle,
  type Citation,
  type MatterData,
  type Paper,
} from "./matter.ts";

describe("Citations show whole rows only", () => {
  // four rows of 56 px, 2 px apart, from 692 down: the room at 1440 by 900
  const rows = [0, 1, 2, 3].map((i) => ({ top: 692 + i * 58, bottom: 748 + i * 58 }));

  test("every row whole: all of them, no strip", () => {
    expect(fitCitations(rows, 600, 1000, 40)).toEqual({ shown: 4, more: 0 });
  });
  test("some cut by the foot: as many as stand whole above the strip", () => {
    // the pane's foot at 846: three rows would end at 864, so two stand whole above a 40 px strip
    expect(fitCitations(rows, 600, 846, 40)).toEqual({ shown: 2, more: 2 });
  });
  test("a row that only fits without the strip still gives way to it", () => {
    // the third row ends at 864; with a foot at 870 it fits alone but not above the strip
    expect(fitCitations(rows, 600, 870, 40)).toEqual({ shown: 2, more: 2 });
  });
  test("none whole: the strip stands under the heading", () => {
    expect(fitCitations(rows, 600, 720, 40)).toEqual({ shown: 0, more: 4 });
  });
  test("half a pixel of rounding counts as whole", () => {
    expect(wholeRowsWithin([{ top: 99.6, bottom: 200.4 }], 100, 200)).toBe(1);
    expect(wholeRowsWithin([{ top: 99, bottom: 200 }], 100, 200)).toBe(0);
  });
  test("a row above the top isn't whole, and nothing after it counts", () => {
    expect(wholeRowsWithin(rows, 700, 2000)).toBe(0);
  });
});

describe("Accept's states", () => {
  test("Accept takes a pending answer to accepted", () => {
    expect(acceptNext("pending", "accept")).toBe("accepted");
  });
  test("Undo takes it back, from the foot or the toolbar alike", () => {
    expect(acceptNext("accepted", "undo")).toBe("pending");
  });
  test("Undo with nothing accepted leaves it pending; Accept twice stays accepted", () => {
    expect(acceptNext("pending", "undo")).toBe("pending");
    expect(acceptNext("accepted", "accept")).toBe("accepted");
  });
  test("a run of presses ends where the last one says", () => {
    const end = (["accept", "undo", "accept"] as const).reduce<AcceptState>(acceptNext, "pending");
    expect(end).toBe("accepted");
  });
});

describe("the accept settle pairs each weight with a width", () => {
  // a measure that grows with weight and stretch, as Anek does
  const measure = (w: number, s: number) => 1000 * (s / 100) * (1 + (w - 400) / 1000);
  const c = settleCurve(104, measure);
  test("five steps, 400 keeps its own width", () => {
    expect(c.map((p) => p.w)).toEqual([400, 450, 500, 550, 600]);
    expect(c[0]?.s).toBe(104);
  });
  test("every heavier step sets no wider than the 400 measure, and narrows as it goes", () => {
    const goal = measure(400, 104) * 0.997;
    for (const p of c) if (p.w > 400) expect(measure(p.w, p.s)).toBeLessThanOrEqual(goal);
    for (let i = 1; i < c.length; i++) expect(c[i]?.s ?? 0).toBeLessThan(c[i - 1]?.s ?? 0);
  });
});

describe("the status line never truncates", () => {
  test("words go first where there's a key cap, then the line wraps", () => {
    expect(nextStatusFit("full", true)).toBe("tight");
    expect(nextStatusFit("tight", true)).toBe("wrapped");
  });
  test("with no key cap on a narrow window, it wraps at once", () => {
    expect(nextStatusFit("full", false)).toBe("wrapped");
  });
});

describe("word lookups in both languages", () => {
  test("English is the key itself, its slots filled", () => {
    expect(t("en", "{n} more", { n: 2 })).toBe("2 more");
    expect(t("en", "Accept into the matter")).toBe("Accept into the matter");
  });
  test("Hindi is the value beside the key, its slots filled", () => {
    expect(t("hi", "{n} more", { n: 2 })).toBe("2 और");
    expect(t("hi", "Accepted")).toBe("स्वीकृत");
    expect(t("hi", "Every change is kept.")).toBe("हर बदलाव सुरक्षित रहता है।");
  });
  test("the room's own strings, in both languages", () => {
    expect(rt("en", "Show fewer")).toBe("Show fewer");
    expect(rt("hi", "Show fewer")).toBe("कम दिखाएँ");
    expect(rt("hi", "Turn to it")).toBe(t("hi", "Turn to it"));
  });
  test("no banned word in the room's own strings", () => {
    const banned = /pencil|\bink\b|\bstep\b|पेंसिल|स्याही|चरण|कदम|मैं\b/i;
    for (const [en, hi] of Object.entries(roomHindi)) {
      expect(en).not.toMatch(banned);
      expect(hi).not.toMatch(banned);
    }
  });
});

describe("the matter's words", () => {
  test("paper book pages in words: a range, one page, or a word in the numeral's place", () => {
    expect(pbWords("en", { from: 33, to: 40 })).toBe("PB 33 to 40");
    expect(pbWords("hi", { from: 33, to: 40 })).toBe("PB 33 से 40");
    expect(pbWords("en", { from: 31, to: 31 })).toBe("PB 31");
    expect(pbWords("hi", { from: 31, to: 31 })).toBe("PB 31");
    expect(pbWords("en", { word: "Note" })).toBe("Note");
    expect(pbWords("hi", { word: "टिप्पणी" })).toBe("टिप्पणी");
  });
  test("the bench mark's fixed scale: a court by its judges, a tribunal whatever its size", () => {
    expect(benchOf("court", 5)).toBe("constitution");
    expect(benchOf("court", 3)).toBe("three");
    expect(benchOf("court", 2)).toBe("two");
    expect(benchOf("court", 1)).toBe("single");
    expect(benchOf("tribunal", 3)).toBe("tribunal");
    expect(benchOf("tribunal", 1)).toBe("tribunal");
  });
  test("a citation is found by its own id: two places quoted in one paper keep their own pins and excerpts", () => {
    const papers: Paper[] = [
      { id: "notice", title: "Notice", pb: { from: 1, to: 4 }, kind: "correspondence" },
      { id: "plaint", title: "Plaint", pb: { from: 5, to: 9 }, kind: "pleading" },
    ];
    const citations: Citation[] = [
      {
        id: "n2",
        paperId: "notice",
        role: "supports",
        pin: "Para 2, PB 1",
        excerpt: "told",
        mark: "",
      },
      {
        id: "n9",
        paperId: "notice",
        role: "supports",
        pin: "Para 9, PB 3",
        excerpt: "sent",
        mark: "",
      },
      { id: "g", paperId: "gone", role: "against", pin: "Para 1", excerpt: "lost", mark: "" },
    ];
    expect(citedPaper(citations, papers, "n2")).toEqual({
      paper: papers[0],
      citation: citations[0],
    });
    expect(citedPaper(citations, papers, "n9")).toEqual({
      paper: papers[0],
      citation: citations[1],
    });
    // an id the answer doesn't hold, and a citation of a paper the matter doesn't hold: nothing
    expect(citedPaper(citations, papers, "nope")).toBeUndefined();
    expect(citedPaper(citations, papers, "g")).toBeUndefined();
    // a paper's id is not a citation's
    expect(citedPaper(citations, papers, "notice")).toBeUndefined();
  });
  test("a reference looks only among its own answer's citations", () => {
    const papers: Paper[] = [
      { id: "notice", title: "Notice", pb: { from: 1, to: 4 }, kind: "correspondence" },
    ];
    const quote = { paperId: "notice", role: "supports", excerpt: "told", mark: "" } as const;
    const matter: MatterData = {
      papers,
      cites: {},
      citations: { a: [{ id: "n2", pin: "Para 2", ...quote }], b: [] },
      inView: "b",
    };
    expect(citedPaper(citationsOf(matter, "a"), papers, "n2")?.citation.pin).toBe("Para 2");
    expect(citedPaper(citationsOf(matter, "b"), papers, "n2")).toBeUndefined();
    expect(citationsOf(matter, "unknown")).toEqual([]);
    expect(citationsOf(matter, null)).toEqual([]);
    // the card from a reference in a: a's citation; from the tile, the answer in view (b) cites nothing
    expect(citationFor(matter, "notice", { answer: "a", id: "n2" })?.citation.pin).toBe("Para 2");
    expect(citationFor(matter, "notice", { answer: "b", id: "n2" })).toBeUndefined();
    expect(citationFor(matter, "notice", undefined)).toBeUndefined();
  });
  test("a paper cited twice in different roles lights in the stronger: against, then supports, then relied on", () => {
    const papers: Paper[] = [
      { id: "notice", title: "Notice", pb: { from: 1, to: 4 }, kind: "correspondence" },
      { id: "plaint", title: "Plaint", pb: { from: 5, to: 9 }, kind: "pleading" },
    ];
    const at = (id: string, paperId: string, role: Citation["role"]): Citation => ({
      id,
      paperId,
      role,
      pin: id,
      excerpt: "",
      mark: "",
    });
    const citations = [
      at("p1", "plaint", "relied"),
      at("n1", "notice", "supports"),
      at("n2", "notice", "against"),
      at("n3", "notice", "against"),
      at("p2", "plaint", "supports"),
      at("g", "gone", "against"),
    ];
    const lit = citedPapers(citations, papers);
    // in the order first cited; of a paper's citations, the first in its strongest role
    expect([...lit].map(([p, c]) => [p, c.citation.id])).toEqual([
      ["plaint", "p2"],
      ["notice", "n2"],
    ]);
    const matter: MatterData = { papers, cites: {}, citations: { a: citations }, inView: "a" };
    expect(citationFor(matter, "notice", undefined)?.citation.id).toBe("n2");
    // a named citation of another paper opens nothing on this one
    expect(citationFor(matter, "notice", { answer: "a", id: "p1" })).toBeUndefined();
  });
  test("identifiers hold together", () => {
    expect(keepIds("(2001) 1 SCC 1")).toBe("(2001)\u00A01\u00A0SCC\u00A01");
    expect(keepIds("Acme Ltd v. State, (2001) 1 SCC 1, para 3")).toBe(
      "Acme Ltd v. State, (2001)\u00A01\u00A0SCC\u00A01, para\u00A03",
    );
    // a report it doesn't know stays as given
    expect(keepIds("(2001) 1 ABC 1")).toBe("(2001) 1 ABC 1");
    expect(keepIds("Form DIR-12")).toBe("Form\u00A0DIR-\u206012");
    expect(tileTitle("Reply to the notice, 3 March 2001")).toBe("Reply");
  });
  test("English runs inside Hindi are marked; English alone is one run", () => {
    expect(runs("hi", "आपके Claude प्लान")).toEqual([
      { text: "आपके ", en: false },
      { text: "Claude", en: true },
      { text: " प्लान", en: false },
    ]);
    expect(runs("en", "Uses your Claude plan")).toEqual([
      { text: "Uses your Claude plan", en: false },
    ]);
  });
});
