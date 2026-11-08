// R18's cards' words (words-denials.ts): every string has its Hindi, with the same slots, and no banned word in
// either language; a sentence ends as its English does, the danda for a full stop.
import { expect, test } from "vitest";
import "./words-hi.ts";
import { dn } from "./words-denials.ts";
import { denialWords } from "./words-denials.hi.ts";

const BANNED_EN = /\bI\b|[\u2013\u2014]|\b(agent|session|step|pencil|ink|git|token|terminal)s?\b/i;
const BANNED_HI = /पेंसिल|स्याही|चरण|कदम|सत्र|एजेंट|(^|\s)मैं(\s|$)/;
const slots = (s: string) =>
  [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1] ?? "").sort((a, b) => a.localeCompare(b));

test("every string has Hindi with its slots, ending as its English ends, with no banned word", () => {
  const keys = Object.keys(denialWords) as (keyof typeof denialWords)[];
  expect(keys.length).toBeGreaterThan(15);
  for (const en of keys) {
    const hi = denialWords[en];
    expect(hi, en).not.toBeNull();
    expect(slots(hi!), en).toEqual(slots(en));
    expect(en, en).not.toMatch(BANNED_EN);
    expect(hi!, en).not.toMatch(BANNED_HI);
    if (en.endsWith(".")) expect(hi!.endsWith("।"), en).toBe(true);
    if (en.endsWith("?")) expect(hi!.endsWith("?"), en).toBe(true);
    if (en.endsWith("…")) expect(hi!.endsWith("…"), en).toBe(true);
  }
});

test("a Hindi display reads the Hindi, its slots filled", () => {
  expect(dn("hi", "Not now")).toBe("अभी नहीं");
  expect(
    dn("hi", "Clauseo stopped after {n} blocked attempts outside this matter.", { n: 3 }),
  ).toBe("इस मामले के बाहर 3 कोशिशें रोकी गईं, इसलिए Clauseo रुक गया।");
});
