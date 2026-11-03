import { expect, test } from "vitest";
import { stepWords } from "./words-steps.ts";
import { stepLiveWords } from "./words-steps-live.ts";

const ALL = { ...stepWords, ...stepLiveWords };

test("no step string says I or a banned word, and each Hindi sentence ends in the danda", () => {
  for (const [en, hi] of Object.entries(ALL)) {
    expect(en).not.toMatch(
      /\bI\b|[\u2013\u2014]|\b(pencil|ink|step|git|token|session|terminal|command|safe)\b/i,
    );
    expect(hi).not.toMatch(/पेंसिल|स्याही|चरण|कदम|(^|\s)मैं(\s|$)/);
    if (en.endsWith(".")) expect(hi.endsWith("।"), en).toBe(true);
  }
});

test("each string takes the same slots in both languages", () => {
  for (const [en, hi] of Object.entries(ALL)) {
    const slots = (s: string) =>
      [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1] ?? "").sort((a, b) => a.localeCompare(b));
    expect(slots(hi), en).toEqual(slots(en));
  }
});
