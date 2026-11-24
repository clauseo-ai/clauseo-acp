import { expect, test } from "vitest";
import { stepWords } from "./words-steps.hi.ts";
import { stepLiveWords } from "./words-steps-live.hi.ts";

const ALL = { ...stepWords, ...stepLiveWords };

test("no step string says I or a banned word, and each Hindi sentence ends in the danda", () => {
  for (const [en, hi] of Object.entries(ALL)) {
    // "command" is allowed in a settled step since 24 Nov (issue 13): "Ran a command." over "Ran a task."
    expect(en).not.toMatch(
      /\bI\b|[\u2013\u2014]|\b(pencil|ink|step|git|token|session|terminal|safe)\b/i,
    );
    if (Object.hasOwn(stepLiveWords, en)) expect(en).not.toMatch(/\bcommand\b/i);
    // a string whose Hindi is still to come (hi: later) has only its English to screen
    if (hi === null) continue;
    expect(hi).not.toMatch(/पेंसिल|स्याही|चरण|कदम|(^|\s)मैं(\s|$)/);
    if (en.endsWith(".")) expect(hi.endsWith("।"), en).toBe(true);
  }
});

test("each string takes the same slots in both languages", () => {
  for (const [en, hi] of Object.entries(ALL)) {
    if (hi === null) continue;
    const slots = (s: string) =>
      [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1] ?? "").sort((a, b) => a.localeCompare(b));
    expect(slots(hi), en).toEqual(slots(en));
  }
});
