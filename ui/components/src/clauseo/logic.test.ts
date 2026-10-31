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
  citedPaper,
  citedPapers,
  keepIds,
  pbWords,
  tileTitle,
  type Citation,
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
  test("the answer's citations, looked up against the matter's papers", () => {
    const papers: Paper[] = [
      { id: "notice", title: "Notice", pb: { from: 1, to: 4 }, kind: "correspondence" },
      { id: "plaint", title: "Plaint", pb: { from: 5, to: 9 }, kind: "pleading" },
    ];
    const quote = { pin: "Para 2, PB 1", excerpt: "told in writing", mark: "" };
    const citations: Citation[] = [
      { paperId: "notice", role: "supports", ...quote },
      { paperId: "gone", role: "against", ...quote },
      { paperId: "notice", role: "against", ...quote },
    ];
    expect(citedPaper(citations, papers, "notice")).toEqual({
      paper: papers[0],
      citation: citations[0],
    });
    // uncited, and cited but missing from the matter: neither lights nor opens a card
    expect(citedPaper(citations, papers, "plaint")).toBeUndefined();
    expect(citedPaper(citations, papers, "gone")).toBeUndefined();
    expect([...citedPapers(citations, papers)]).toEqual([["notice", "supports"]]);
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
